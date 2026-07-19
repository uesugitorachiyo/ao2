#!/usr/bin/env python3
"""Prepare and validate compact AO2 physical-Windows lifecycle evidence."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import re
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any


EVIDENCE_SCHEMA = "ao2.physical-windows-evidence.v1"
SUMMARY_SCHEMA = "ao2.physical-windows-qualification.v1"
PROBE_SCHEMA = "ao2.physical-windows-lifecycle-probe.v1"
FRESHNESS_WINDOW_SECONDS = 86400
MAX_ENCODED_PAYLOAD_CHARS = 60000
MAX_DECODED_PAYLOAD_BYTES = 45000
HOSTED_WINDOWS_EQUIVALENCE_EXCEPTIONS = [
    "portable test suites remain owned by hosted native Windows",
    "this probe covers only physical-Windows lifecycle evidence",
]


class ValidationError(ValueError):
    """Raised when physical-Windows evidence is malformed or insufficient."""


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("utf-8")


def _reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValidationError(f"duplicate key: {key}")
        result[key] = value
    return result


def _json_object(raw: str, label: str) -> dict[str, Any]:
    try:
        parsed = json.loads(raw, object_pairs_hook=_reject_duplicate_keys)
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        raise ValidationError(f"invalid {label} JSON") from exc
    if not isinstance(parsed, dict):
        raise ValidationError(f"{label} must be a JSON object")
    return parsed


def _require_mapping(value: object, label: str, keys: set[str]) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValidationError(f"{label} must be an object")
    actual_keys = set(value)
    if actual_keys != keys:
        raise ValidationError(f"{label} has unexpected keys: {sorted(actual_keys ^ keys)}")
    return value


def _require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValidationError(f"{label} must be a non-empty string")
    return value


def _require_bool(value: object, label: str) -> bool:
    if not isinstance(value, bool):
        raise ValidationError(f"{label} must be a boolean")
    return value


def _require_sha(value: object, label: str) -> str:
    text = _require_string(value, label)
    if not re.fullmatch(r"[0-9a-f]{40}", text):
        raise ValidationError(f"{label} must be a lowercase 40-character SHA")
    return text


def _require_positive_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise ValidationError(f"{label} must be a positive integer")
    return value


def _parse_completed_at(value: object) -> datetime:
    text = _require_string(value, "completed_at")
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValidationError("completed_at must be ISO-8601") from exc
    if parsed.tzinfo is None:
        raise ValidationError("completed_at must include a timezone")
    return parsed.astimezone(timezone.utc)


def _validate_probe(probe: dict[str, Any], expected_source_sha: str, expected_version: str) -> None:
    probe = _require_mapping(
        probe,
        "lifecycle probe",
        {
            "schema_version",
            "source_sha",
            "version",
            "request_id",
            "result_id",
            "completed_at",
            "scheduled_task",
            "persistent_outbound_worker",
            "installed_candidate_lifecycle",
            "safety_boundaries",
            "hosted_windows_equivalence_exceptions",
        },
    )
    if probe["schema_version"] != PROBE_SCHEMA:
        raise ValidationError("lifecycle probe schema_version is invalid")
    if _require_sha(probe["source_sha"], "lifecycle probe source_sha") != expected_source_sha:
        raise ValidationError("lifecycle probe source_sha does not match expected source_sha")
    if _require_string(probe["version"], "lifecycle probe version") != expected_version:
        raise ValidationError("lifecycle probe version does not match expected version")
    _require_string(probe["request_id"], "lifecycle probe request_id")
    _require_string(probe["result_id"], "lifecycle probe result_id")
    _parse_completed_at(probe["completed_at"])

    scheduled = _require_mapping(probe["scheduled_task"], "scheduled_task", {"task_name", "registered", "enabled", "state"})
    if scheduled["task_name"] != "AO2 Windows Outbound Worker" or scheduled["state"] != "Running":
        raise ValidationError("scheduled_task is not the running outbound worker task")
    if not _require_bool(scheduled["registered"], "scheduled_task.registered"):
        raise ValidationError("scheduled_task.registered must be true")
    if not _require_bool(scheduled["enabled"], "scheduled_task.enabled"):
        raise ValidationError("scheduled_task.enabled must be true")

    worker = _require_mapping(
        probe["persistent_outbound_worker"],
        "persistent_outbound_worker",
        {"process_id", "parent_process_id", "ancestry_verified", "outbound_only"},
    )
    process_id = _require_positive_int(worker["process_id"], "persistent_outbound_worker.process_id")
    parent_process_id = _require_positive_int(worker["parent_process_id"], "persistent_outbound_worker.parent_process_id")
    if process_id == parent_process_id:
        raise ValidationError("persistent_outbound_worker process ancestry is invalid")
    if not _require_bool(worker["ancestry_verified"], "persistent_outbound_worker.ancestry_verified"):
        raise ValidationError("persistent_outbound_worker ancestry is not verified")
    if not _require_bool(worker["outbound_only"], "persistent_outbound_worker.outbound_only"):
        raise ValidationError("persistent_outbound_worker must be outbound only")

    lifecycle = _require_mapping(
        probe["installed_candidate_lifecycle"],
        "installed_candidate_lifecycle",
        {"exact_head", "package_built", "install_completed", "use_completed", "rollback_completed", "uninstall_completed", "windows_safe"},
    )
    if _require_sha(lifecycle["exact_head"], "installed_candidate_lifecycle.exact_head") != expected_source_sha:
        raise ValidationError("installed_candidate_lifecycle exact_head does not match source_sha")
    for key in ("package_built", "install_completed", "use_completed", "rollback_completed", "uninstall_completed", "windows_safe"):
        if not _require_bool(lifecycle[key], f"installed_candidate_lifecycle.{key}"):
            raise ValidationError(f"installed_candidate_lifecycle.{key} must be true")

    boundaries = _require_mapping(
        probe["safety_boundaries"],
        "safety_boundaries",
        {"inbound_http", "arbitrary_remote_execution", "credential_changes", "release_mutation"},
    )
    for key, value in boundaries.items():
        if _require_bool(value, f"safety_boundaries.{key}"):
            raise ValidationError(f"safety_boundaries.{key} must be false")
    if probe["hosted_windows_equivalence_exceptions"] != HOSTED_WINDOWS_EQUIVALENCE_EXCEPTIONS:
        raise ValidationError("hosted_windows_equivalence_exceptions are invalid")


def _board_action_result(board: object, action: str) -> dict[str, Any]:
    if not isinstance(board, dict) or not isinstance(board.get("tasks"), list):
        raise ValidationError("task board must contain a tasks list")
    matches: list[dict[str, Any]] = []
    for task in board["tasks"]:
        if not isinstance(task, dict):
            continue
        cross_host = task.get("ao2_cross_host")
        if not isinstance(cross_host, dict) or cross_host.get("action") != action:
            continue
        result = cross_host.get("result")
        if isinstance(result, dict):
            matches.append(result)
    if len(matches) != 1:
        raise ValidationError(f"expected exactly one {action} result")
    return matches[0]


def prepare_evidence(
    status_board: dict[str, Any], qualification_board: dict[str, Any], source_sha: str, version: str
) -> tuple[dict[str, Any], dict[str, Any]]:
    source_sha = _require_sha(source_sha, "source_sha")
    version = _require_string(version, "version")
    status = _board_action_result(status_board, "status")
    if status.get("worker_source_commit") != source_sha:
        raise ValidationError("status worker_source_commit does not match source_sha")
    if status.get("arbitrary_command_execution") is not False:
        raise ValidationError("status must prove arbitrary_command_execution is false")

    qualification = _board_action_result(qualification_board, "windows_stack_qualification")
    if qualification.get("mode") != "physical_unique":
        raise ValidationError("qualification mode must be physical_unique")
    if qualification.get("worker_source_commit") != source_sha:
        raise ValidationError("qualification worker_source_commit does not match source_sha")
    rows = qualification.get("results")
    if not isinstance(rows, list):
        raise ValidationError("qualification results must be a list")
    failed_rows = [row for row in rows if not isinstance(row, dict) or row.get("status") != "accepted"]
    if failed_rows:
        raise ValidationError("qualification contains failed qualification rows")
    lifecycle_rows = [
        row
        for row in rows
        if isinstance(row, dict) and row.get("sanitized_command_name") == "physical-windows-lifecycle"
    ]
    if len(lifecycle_rows) != 1:
        raise ValidationError("expected exactly one physical-windows-lifecycle row")
    lifecycle_row = lifecycle_rows[0]
    if lifecycle_row.get("status") != "accepted" or lifecycle_row.get("output_truncated") is True:
        raise ValidationError("physical-windows-lifecycle row was not accepted with complete output")
    raw_probe = lifecycle_row.get("bounded_sanitized_output")
    if not isinstance(raw_probe, str):
        raise ValidationError("physical-windows-lifecycle row has no compact JSON output")
    probe = _json_object(raw_probe, "physical-windows-lifecycle output")
    _validate_probe(probe, source_sha, version)

    evidence = {
        "schema_version": EVIDENCE_SCHEMA,
        "source_sha": source_sha,
        "version": version,
        "worker_source_commit": source_sha,
        "request_id": probe["request_id"],
        "result_id": probe["result_id"],
        "completed_at": probe["completed_at"],
        "scheduled_task": probe["scheduled_task"],
        "persistent_outbound_worker": probe["persistent_outbound_worker"],
        "installed_candidate_lifecycle": probe["installed_candidate_lifecycle"],
        "safety_boundaries": probe["safety_boundaries"],
        "hosted_windows_equivalence_exceptions": probe["hosted_windows_equivalence_exceptions"],
    }
    return evidence, validate_evidence(evidence, source_sha, version, _parse_completed_at(probe["completed_at"]))


def validate_evidence(
    evidence: dict[str, Any], expected_source_sha: str, expected_version: str, now: datetime
) -> dict[str, Any]:
    expected_source_sha = _require_sha(expected_source_sha, "expected_source_sha")
    expected_version = _require_string(expected_version, "expected_version")
    if not isinstance(now, datetime):
        raise ValidationError("now must be a datetime")
    if now.tzinfo is None:
        raise ValidationError("now must include a timezone")
    now = now.astimezone(timezone.utc)
    evidence = _require_mapping(
        evidence,
        "evidence",
        {
            "schema_version",
            "source_sha",
            "version",
            "worker_source_commit",
            "request_id",
            "result_id",
            "completed_at",
            "scheduled_task",
            "persistent_outbound_worker",
            "installed_candidate_lifecycle",
            "safety_boundaries",
            "hosted_windows_equivalence_exceptions",
        },
    )
    if evidence["schema_version"] != EVIDENCE_SCHEMA:
        raise ValidationError("evidence schema_version is invalid")
    if _require_sha(evidence["source_sha"], "source_sha") != expected_source_sha:
        raise ValidationError("source_sha does not match expected_source_sha")
    if _require_sha(evidence["worker_source_commit"], "worker_source_commit") != expected_source_sha:
        raise ValidationError("worker_source_commit does not match source_sha")
    if _require_string(evidence["version"], "version") != expected_version:
        raise ValidationError("version does not match expected_version")
    completed_at = _parse_completed_at(evidence["completed_at"])
    if completed_at > now or now - completed_at > timedelta(seconds=FRESHNESS_WINDOW_SECONDS):
        raise ValidationError("evidence freshness exceeds the 24-hour window")
    _validate_probe({
        "schema_version": PROBE_SCHEMA,
        "source_sha": evidence["source_sha"],
        "version": evidence["version"],
        "request_id": evidence["request_id"],
        "result_id": evidence["result_id"],
        "completed_at": evidence["completed_at"],
        "scheduled_task": evidence["scheduled_task"],
        "persistent_outbound_worker": evidence["persistent_outbound_worker"],
        "installed_candidate_lifecycle": evidence["installed_candidate_lifecycle"],
        "safety_boundaries": evidence["safety_boundaries"],
        "hosted_windows_equivalence_exceptions": evidence["hosted_windows_equivalence_exceptions"],
    }, expected_source_sha, expected_version)
    return {
        "schema_version": SUMMARY_SCHEMA,
        "status": "passed",
        "source_sha": evidence["source_sha"],
        "worker_source_commit": evidence["worker_source_commit"],
        "version": evidence["version"],
        "evidence_sha256": hashlib.sha256(canonical_json(evidence)).hexdigest(),
        "request_id": evidence["request_id"],
        "result_id": evidence["result_id"],
        "completed_at": evidence["completed_at"],
        "freshness_window_seconds": FRESHNESS_WINDOW_SECONDS,
        "failed_row_count": 0,
        "portable_suite_owner": "hosted_windows",
        "passing": {
            "scheduled_task": True,
            "persistent_outbound_worker": True,
            "installed_candidate_lifecycle": True,
        },
        "safety_boundaries": evidence["safety_boundaries"],
        "hosted_windows_equivalence_exceptions": evidence["hosted_windows_equivalence_exceptions"],
    }


def decode_import_payload(encoded: str, expected_digest: str) -> dict[str, Any]:
    if not isinstance(encoded, str):
        raise ValidationError("encoded payload exceeds 60000 characters")
    if len(encoded) > MAX_ENCODED_PAYLOAD_CHARS:
        maximum_decoded_bytes = (len(encoded) // 4) * 3
        if maximum_decoded_bytes > MAX_DECODED_PAYLOAD_BYTES:
            raise ValidationError("decoded payload exceeds 45000 bytes")
        raise ValidationError("encoded payload exceeds 60000 characters")
    if not isinstance(expected_digest, str) or not re.fullmatch(r"[0-9a-f]{64}", expected_digest):
        raise ValidationError("expected digest must be a lowercase SHA-256")
    try:
        decoded = base64.b64decode(encoded.encode("ascii"), validate=True)
    except (UnicodeEncodeError, binascii.Error) as exc:
        raise ValidationError("encoded payload is not valid base64") from exc
    if len(decoded) > MAX_DECODED_PAYLOAD_BYTES:
        raise ValidationError("decoded payload exceeds 45000 bytes")
    actual_digest = hashlib.sha256(decoded).hexdigest()
    if actual_digest != expected_digest:
        raise ValidationError("payload digest does not match expected digest")
    try:
        raw = decoded.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ValidationError("decoded payload is not UTF-8") from exc
    evidence = _json_object(raw, "decoded payload")
    if canonical_json(evidence) != decoded:
        raise ValidationError("decoded payload is not canonical JSON")
    return evidence


def _read_json_file(path: str) -> dict[str, Any]:
    try:
        raw = Path(path).read_text(encoding="utf-8")
    except OSError as exc:
        raise ValidationError(f"could not read JSON file: {path}") from exc
    return _json_object(raw, "input")


def _print_json(value: object) -> None:
    sys.stdout.buffer.write(canonical_json(value))


def _utc_now() -> datetime:
    return datetime.now(timezone.utc)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    prepare = subparsers.add_parser("prepare")
    prepare.add_argument("--status-board", required=True)
    prepare.add_argument("--qualification-board", required=True)
    prepare.add_argument("--source-sha", required=True)
    prepare.add_argument("--version", required=True)
    validate = subparsers.add_parser("validate")
    validate.add_argument("--evidence", required=True)
    validate.add_argument("--source-sha", required=True)
    validate.add_argument("--version", required=True)
    imported = subparsers.add_parser("import")
    imported.add_argument("--encoded", required=True)
    imported.add_argument("--expected-digest", required=True)
    imported.add_argument("--source-sha", required=True)
    imported.add_argument("--version", required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        if args.command == "prepare":
            evidence, summary = prepare_evidence(
                _read_json_file(args.status_board),
                _read_json_file(args.qualification_board),
                args.source_sha,
                args.version,
            )
            _print_json({"evidence": evidence, "summary": summary})
        elif args.command == "validate":
            _print_json(validate_evidence(_read_json_file(args.evidence), args.source_sha, args.version, _utc_now()))
        else:
            evidence = decode_import_payload(args.encoded, args.expected_digest)
            _print_json(validate_evidence(evidence, args.source_sha, args.version, _utc_now()))
    except ValidationError as exc:
        _print_json({"error": str(exc), "status": "failed"})
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
