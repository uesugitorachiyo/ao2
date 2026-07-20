#!/usr/bin/env python3
"""Validate the fixed producer run and artifact metadata before download."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path
from typing import Mapping


EXPECTED_WORKFLOW_PATH = ".github/workflows/import-physical-windows-qualification.yml"
EXPECTED_WORKFLOW_NAME = "Import Physical Windows Qualification"
EXPECTED_ARTIFACT_NAME = "ao2-physical-windows-qualification"
MAX_METADATA_BYTES = 1_000_000
RUN_ID_PATTERN = re.compile(r"[1-9][0-9]{0,19}")
REPOSITORY_PATTERN = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
SHA_PATTERN = re.compile(r"[0-9a-f]{40}")


class MetadataValidationError(ValueError):
    """Raised when producer workflow metadata does not match the fixed contract."""


def validate_run_id(value: str) -> int:
    if not isinstance(value, str) or RUN_ID_PATTERN.fullmatch(value) is None:
        raise MetadataValidationError("run id must be 1 to 20 decimal digits without leading zeroes")
    return int(value)


def _mapping(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise MetadataValidationError(f"{label} must be a JSON object")
    return value


def _positive_integer(value: object, label: str) -> int:
    if type(value) is not int or value <= 0:
        raise MetadataValidationError(f"{label} must be a positive integer")
    return value


def _reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise MetadataValidationError(f"metadata contains duplicate key: {key}")
        result[key] = value
    return result


def _read_metadata(path: str, label: str) -> dict[str, object]:
    metadata_path = Path(path)
    try:
        payload = metadata_path.read_bytes()
    except OSError as exc:
        raise MetadataValidationError(f"could not read {label}") from exc
    if not payload or len(payload) > MAX_METADATA_BYTES:
        raise MetadataValidationError(f"{label} size is invalid")
    try:
        value = json.loads(
            payload.decode("utf-8"),
            object_pairs_hook=_reject_duplicate_keys,
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise MetadataValidationError(f"{label} is not valid UTF-8 JSON") from exc
    return _mapping(value, label)


def validate_metadata(
    run: dict[str, object],
    artifact_listing: dict[str, object],
    run_id_text: str,
    expected_repository: str,
    expected_source_sha: str,
) -> int:
    run_id = validate_run_id(run_id_text)
    if (
        not isinstance(expected_repository, str)
        or REPOSITORY_PATTERN.fullmatch(expected_repository) is None
    ):
        raise MetadataValidationError("expected repository is invalid")
    if (
        not isinstance(expected_source_sha, str)
        or SHA_PATTERN.fullmatch(expected_source_sha) is None
    ):
        raise MetadataValidationError("expected source SHA is invalid")

    run = _mapping(run, "workflow run metadata")
    if _positive_integer(run.get("id"), "workflow run id") != run_id:
        raise MetadataValidationError("workflow run id does not match requested run id")
    repository = _mapping(run.get("repository"), "workflow run repository")
    repository_id = _positive_integer(repository.get("id"), "workflow run repository id")
    if repository.get("full_name") != expected_repository:
        raise MetadataValidationError("workflow run repository does not match current repository")
    if run.get("path") != EXPECTED_WORKFLOW_PATH:
        raise MetadataValidationError("workflow path does not identify the fixed import workflow")
    if run.get("name") != EXPECTED_WORKFLOW_NAME:
        raise MetadataValidationError("workflow name does not identify the fixed import workflow")
    if run.get("event") != "workflow_dispatch":
        raise MetadataValidationError("workflow run event must be workflow_dispatch")
    if run.get("status") != "completed":
        raise MetadataValidationError("workflow run status must be completed")
    if run.get("conclusion") != "success":
        raise MetadataValidationError("workflow run conclusion must be success")
    if run.get("head_sha") != expected_source_sha:
        raise MetadataValidationError("workflow run head_sha does not match immutable source SHA")

    artifact_listing = _mapping(artifact_listing, "artifact listing metadata")
    total_count = artifact_listing.get("total_count")
    if total_count != 1 or type(total_count) is not int:
        raise MetadataValidationError("artifact total_count must be exactly 1")
    artifacts = artifact_listing.get("artifacts")
    if not isinstance(artifacts, list) or len(artifacts) != 1:
        raise MetadataValidationError("artifact listing must contain exactly one entry")
    artifact = _mapping(artifacts[0], "artifact metadata")
    artifact_id = _positive_integer(artifact.get("id"), "artifact id")
    if artifact.get("name") != EXPECTED_ARTIFACT_NAME:
        raise MetadataValidationError("artifact name does not match the fixed qualification artifact")
    if artifact.get("expired") is not False:
        raise MetadataValidationError("artifact expired must be false")

    artifact_run = artifact.get("workflow_run")
    if artifact_run is not None:
        artifact_run = _mapping(artifact_run, "artifact workflow run metadata")
        if "id" in artifact_run and artifact_run["id"] != run_id:
            raise MetadataValidationError("artifact workflow run id does not match requested run id")
        if (
            "head_sha" in artifact_run
            and artifact_run["head_sha"] != expected_source_sha
        ):
            raise MetadataValidationError(
                "artifact workflow run head_sha does not match immutable source SHA"
            )
        if "repository_id" in artifact_run and artifact_run["repository_id"] != repository_id:
            raise MetadataValidationError(
                "artifact workflow run repository_id does not match run repository"
            )
        if (
            "head_repository_id" in artifact_run
            and artifact_run["head_repository_id"] != repository_id
        ):
            raise MetadataValidationError(
                "artifact workflow run head_repository_id does not match run repository"
            )
    return artifact_id


def _required_environment(environ: Mapping[str, str], name: str) -> str:
    value = environ.get(name)
    if not isinstance(value, str) or not value:
        raise MetadataValidationError(f"{name} must be a non-empty environment value")
    return value


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command",
        choices=("validate-run-id", "validate-metadata"),
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        run_id_text = _required_environment(os.environ, "RUN_ID")
        validate_run_id(run_id_text)
        if args.command == "validate-metadata":
            validate_metadata(
                _read_metadata(
                    _required_environment(os.environ, "RUN_METADATA_PATH"),
                    "workflow run metadata",
                ),
                _read_metadata(
                    _required_environment(os.environ, "ARTIFACTS_METADATA_PATH"),
                    "artifact listing metadata",
                ),
                run_id_text,
                _required_environment(os.environ, "EXPECTED_REPOSITORY"),
                _required_environment(os.environ, "EXPECTED_SOURCE_SHA"),
            )
    except MetadataValidationError as exc:
        print(f"physical Windows producer metadata validation failed: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
