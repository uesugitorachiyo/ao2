#!/usr/bin/env python3
"""Validate a fresh, exact-head, draft-only GitHub pull request readback."""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


MAX_INPUT_BYTES = 1024 * 1024
MAX_FUTURE_SKEW_SECONDS = 300
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
TOP_LEVEL_FIELDS = {
    "schema_version",
    "captured_at",
    "repository",
    "pull_request",
    "boundaries",
}
PULL_REQUEST_FIELDS = {
    "number",
    "state",
    "isDraft",
    "mergedAt",
    "headRefOid",
    "baseRefName",
    "statusCheckRollup",
}
CHECK_FIELDS = {"name", "status", "conclusion"}
BOUNDARY_FIELDS = {
    "issue_write_performed",
    "ready_for_review_performed",
    "review_approval_performed",
    "merge_performed",
}


class ValidationError(ValueError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--expected-head-sha", required=True)
    parser.add_argument("--max-age-seconds", type=int, default=3600)
    return parser.parse_args()


def require_exact_fields(value: dict[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    extra = sorted(actual - expected)
    missing = sorted(expected - actual)
    if extra:
        raise ValidationError(f"unexpected {label} fields: {', '.join(extra)}")
    if missing:
        raise ValidationError(f"missing {label} fields: {', '.join(missing)}")


def parse_timestamp(value: Any) -> datetime:
    if not isinstance(value, str) or not value.endswith("Z"):
        raise ValidationError("captured_at must be an RFC3339 UTC timestamp ending in Z")
    try:
        parsed = datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError as exc:
        raise ValidationError("captured_at must be a valid RFC3339 timestamp") from exc
    return parsed.astimezone(timezone.utc)


def read_payload(path: Path) -> dict[str, Any]:
    try:
        size = path.stat().st_size
    except OSError as exc:
        raise ValidationError(f"cannot stat input: {type(exc).__name__}") from exc
    if size > MAX_INPUT_BYTES:
        raise ValidationError(f"input exceeds {MAX_INPUT_BYTES} bytes")
    try:
        payload = json.loads(
            path.read_text(encoding="utf-8"),
            parse_constant=lambda value: (_ for _ in ()).throw(
                ValueError(f"non-standard JSON constant: {value}")
            ),
        )
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as exc:
        raise ValidationError(f"cannot parse input JSON: {type(exc).__name__}") from exc
    if not isinstance(payload, dict):
        raise ValidationError("input must be a JSON object")
    return payload


def validate(
    payload: dict[str, Any],
    *,
    expected_head_sha: str,
    max_age_seconds: int,
    now: datetime | None = None,
) -> dict[str, Any]:
    if not SHA_RE.fullmatch(expected_head_sha):
        raise ValidationError("expected head SHA must be 40 lowercase hexadecimal characters")
    if max_age_seconds < 1 or max_age_seconds > 86400:
        raise ValidationError("max age must be between 1 and 86400 seconds")

    require_exact_fields(payload, TOP_LEVEL_FIELDS, "top-level")
    if payload["schema_version"] != "ao2.github-draft-pr-readback.v1":
        raise ValidationError("schema_version is invalid")
    if not isinstance(payload["repository"], str) or not re.fullmatch(
        r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", payload["repository"]
    ):
        raise ValidationError("repository must be an owner/name identifier")

    current = now or datetime.now(timezone.utc)
    captured = parse_timestamp(payload["captured_at"])
    age_seconds = (current - captured).total_seconds()
    if age_seconds < -MAX_FUTURE_SKEW_SECONDS:
        raise ValidationError("readback timestamp is too far in the future")
    if age_seconds > max_age_seconds:
        raise ValidationError("readback is stale")

    pull = payload["pull_request"]
    if not isinstance(pull, dict):
        raise ValidationError("pull_request must be an object")
    require_exact_fields(pull, PULL_REQUEST_FIELDS, "pull_request")
    if (
        not isinstance(pull["number"], int)
        or isinstance(pull["number"], bool)
        or pull["number"] < 1
    ):
        raise ValidationError("pull request number must be a positive integer")
    if pull["state"] != "OPEN":
        raise ValidationError("pull request must remain open")
    if pull["isDraft"] is not True:
        raise ValidationError("pull request must remain draft")
    if pull["mergedAt"] is not None:
        raise ValidationError("pull request must not be merged")
    if pull["headRefOid"] != expected_head_sha:
        raise ValidationError("headRefOid does not match expected head SHA")
    if not isinstance(pull["baseRefName"], str) or not pull["baseRefName"]:
        raise ValidationError("baseRefName is required")

    checks = pull["statusCheckRollup"]
    if not isinstance(checks, list) or not checks:
        raise ValidationError("at least one status check is required")
    for index, check in enumerate(checks):
        if not isinstance(check, dict):
            raise ValidationError(f"status check {index} must be an object")
        require_exact_fields(check, CHECK_FIELDS, f"status check {index}")
        if not isinstance(check["name"], str) or not check["name"]:
            raise ValidationError(f"status check {index} name is required")
        if check["status"] != "COMPLETED" or check["conclusion"] != "SUCCESS":
            raise ValidationError("status checks must all be completed successfully")

    boundaries = payload["boundaries"]
    if not isinstance(boundaries, dict):
        raise ValidationError("boundaries must be an object")
    require_exact_fields(boundaries, BOUNDARY_FIELDS, "boundary")
    if any(value is not False for value in boundaries.values()):
        raise ValidationError("boundary flags must all be false")

    return {
        "schema_version": "ao2.github-draft-pr-readback-validation.v1",
        "status": "passed",
        "repository": payload["repository"],
        "pull_number": pull["number"],
        "head_sha": pull["headRefOid"],
        "captured_at": payload["captured_at"],
        "age_seconds": max(0, int(age_seconds)),
        "draft_only": True,
        "all_checks_successful": True,
        "promotion_eligible": False,
        "promotion_authority": "not_implemented",
        "boundaries": boundaries,
    }


def main() -> int:
    args = parse_args()
    try:
        payload = read_payload(args.input)
        summary = validate(
            payload,
            expected_head_sha=args.expected_head_sha,
            max_age_seconds=args.max_age_seconds,
        )
    except ValidationError as exc:
        print(f"validate_github_draft_pr_readback.py: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
