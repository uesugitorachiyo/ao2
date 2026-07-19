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


TASK_BOARD_SCHEMA = "ao2.ai-task-board.v1"
WORKER_RESULT_SCHEMA = "ao2.cross-host.windows-worker-result.v1"
QUALIFICATION_RESULT_SCHEMA = "ao2.windows-stack-qualification-result.v1"
PROFILE_VERSION = "ao2.windows-stack-qualification.profiles.v1"
PROBE_SCHEMA = "ao2.physical-windows-lifecycle-probe.v1"
EVIDENCE_SCHEMA = "ao2.physical-windows-qualification-evidence.v1"
SUMMARY_SCHEMA = "ao2.physical-windows-qualification-summary.v1"
FRESHNESS_WINDOW_SECONDS = 86400
MAX_ENCODED_PAYLOAD_CHARS = 60000
MAX_DECODED_PAYLOAD_BYTES = 45000
EXPECTED_ROWS = (
    "windows-worker-pytest",
    "ao2-doctor",
    "windows-file-locking-rollback",
    "physical-windows-lifecycle",
)
HOSTED_WINDOWS_EQUIVALENCE_EXCEPTIONS = [
    "portable test suites remain owned by hosted native Windows",
    "this probe covers only physical-Windows lifecycle evidence",
]
SAFETY_BOUNDARY_KEYS = {
    "inbound_http",
    "arbitrary_remote_execution",
    "credential_changes",
    "release_mutation",
    "self_hosted_public_repository_runner",
}
ROW_KEYS = {
    "node_id",
    "worker_source_commit",
    "request_id",
    "canonical_repository",
    "repository_head",
    "verification_profile",
    "sanitized_command_name",
    "status",
    "exit_code",
    "timeout_state",
    "timed_out",
    "duration_seconds",
    "error_category",
    "bounded_sanitized_output",
    "output_truncated",
    "completed_timestamp",
}
LIFECYCLE_KEYS = {
    "exact_head",
    "source_version_verified",
    "debug_prior_built",
    "release_candidate_built",
    "candidate_package_created",
    "package_manifest_verified",
    "package_provenance_verified",
    "install_completed",
    "install_verification_verified",
    "candidate_use_verified",
    "candidate_digest",
    "prior_digest",
    "installed_candidate_digest",
    "rollback_runner_separate",
    "rollback_status",
    "rollback_completed",
    "installed_rollback_digest",
    "rollback_use_verified",
    "uninstall_completed",
    "temp_cleanup_completed",
    "windows_safe",
}
EVIDENCE_KEYS = {
    "schema_version",
    "mode",
    "source_sha",
    "worker_source_commit",
    "version",
    "node_id",
    "status_request_id",
    "status_result_id",
    "status_completed_at",
    "request_id",
    "result_id",
    "completed_at",
    "qualification_completed_at",
    "profile_version",
    "repositories",
    "repository_head",
    "row_provenance",
    "scheduled_task",
    "persistent_outbound_worker",
    "installed_candidate_lifecycle",
    "observed_worker_boundaries",
    "safety_boundaries",
    "hosted_windows_equivalence_exceptions",
}


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
    except json.JSONDecodeError as exc:
        raise ValidationError(f"invalid {label} JSON") from exc
    if not isinstance(parsed, dict):
        raise ValidationError(f"{label} must be a JSON object")
    return parsed


def _require_mapping(value: object, label: str, keys: set[str] | None = None) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValidationError(f"{label} must be an object")
    if keys is not None and set(value) != keys:
        raise ValidationError(f"{label} has unexpected keys: {sorted(set(value) ^ keys)}")
    return value


def _require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValidationError(f"{label} must be a non-empty string")
    return value


def _require_bool(value: object, label: str) -> bool:
    if not isinstance(value, bool):
        raise ValidationError(f"{label} must be a boolean")
    return value


def _require_false(value: object, label: str) -> None:
    if _require_bool(value, label):
        raise ValidationError(f"{label} must be false")


def _require_true(value: object, label: str) -> None:
    if not _require_bool(value, label):
        raise ValidationError(f"{label} must be true")


def _require_sha(value: object, label: str, length: int = 40) -> str:
    text = _require_string(value, label)
    if not re.fullmatch(rf"[0-9a-f]{{{length}}}", text):
        raise ValidationError(f"{label} must be a lowercase {length}-character SHA")
    return text


def _require_positive_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise ValidationError(f"{label} must be a positive integer")
    return value


def _require_zero(value: object, label: str) -> None:
    if isinstance(value, bool) or not isinstance(value, int) or value != 0:
        raise ValidationError(f"{label} must be zero")


def _parse_time(value: object, label: str) -> datetime:
    text = _require_string(value, label)
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValidationError(f"{label} must be ISO-8601") from exc
    if parsed.tzinfo is None:
        raise ValidationError(f"{label} must include a timezone")
    return parsed.astimezone(timezone.utc)


def _format_time(value: datetime) -> str:
    value = value.astimezone(timezone.utc)
    return value.isoformat(timespec="seconds").replace("+00:00", "Z")


def _result_board(board: object, action: str) -> dict[str, Any]:
    board = _require_mapping(board, f"{action} board")
    if board.get("schema_version") != TASK_BOARD_SCHEMA:
        raise ValidationError(f"{action} board schema_version is invalid")
    if board.get("status") != "accepted":
        raise ValidationError(f"{action} board status must be accepted")
    tasks = board.get("tasks")
    if not isinstance(tasks, list) or len(tasks) != 1:
        raise ValidationError(f"{action} board must contain exactly one result task")
    task = _require_mapping(tasks[0], f"{action} result task")
    if task.get("kind") != "cross-host-worker-result" or task.get("status") != "accepted":
        raise ValidationError(f"{action} result task is not accepted")
    wrapper = _require_mapping(
        task.get("ao2_cross_host"),
        f"{action} ao2_cross_host",
        {
            "schema_version",
            "status",
            "node_id",
            "request_id",
            "action",
            "arbitrary_command_execution",
            "result",
            "completed_at",
        },
    )
    if wrapper["schema_version"] != WORKER_RESULT_SCHEMA:
        raise ValidationError(f"{action} wrapper schema_version is invalid")
    if wrapper["status"] != "accepted" or wrapper["action"] != action:
        raise ValidationError(f"{action} wrapper status/action is invalid")
    _require_false(wrapper["arbitrary_command_execution"], f"{action} arbitrary_command_execution")
    request_id = _require_string(wrapper["request_id"], f"{action} request_id")
    result_id = _require_string(task.get("task_id"), f"{action} task_id")
    expected_result_id = f"windows-worker-result-{action.replace('_', '-')}-{request_id}"
    if result_id != expected_result_id:
        raise ValidationError(f"{action} task_id does not bind request_id")
    _require_string(wrapper["node_id"], f"{action} node_id")
    _parse_time(wrapper["completed_at"], f"{action} wrapper completed_at")
    result = _require_mapping(wrapper["result"], f"{action} result")
    return {
        "board": board,
        "task": task,
        "wrapper": wrapper,
        "result": result,
        "result_id": result_id,
    }


def _validate_board_boundaries(parsed: dict[str, Any], label: str) -> None:
    board = parsed["board"]
    readback = _require_mapping(
        board.get("control_plane_readback"),
        f"{label} control_plane_readback",
        {"role", "requires_credentials", "can_mutate_ao2_artifacts", "can_mutate_release_metadata"},
    )
    if readback["role"] != "read_only_observer":
        raise ValidationError(f"{label} control_plane_readback role is invalid")
    _require_false(readback["requires_credentials"], f"{label} requires_credentials")
    _require_false(readback["can_mutate_ao2_artifacts"], f"{label} can_mutate_ao2_artifacts")
    _require_false(readback["can_mutate_release_metadata"], f"{label} can_mutate_release_metadata")
    trust = _require_mapping(
        board.get("trust_boundary"),
        f"{label} trust_boundary",
        {"local_only", "stores_credentials", "mutates_releases"},
    )
    _require_false(trust["stores_credentials"], f"{label} stores_credentials")
    _require_false(trust["mutates_releases"], f"{label} mutates_releases")


def _validate_probe(probe: dict[str, Any], expected_source_sha: str, expected_version: str) -> None:
    probe = _require_mapping(
        probe,
        "lifecycle probe",
        {
            "schema_version",
            "source_sha",
            "version",
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

    scheduled = _require_mapping(
        probe["scheduled_task"],
        "scheduled_task",
        {"task_name", "registered", "enabled", "state"},
    )
    if scheduled["task_name"] != "AO2 Windows Outbound Worker" or scheduled["state"] != "Running":
        raise ValidationError("scheduled_task is not the running outbound worker task")
    _require_true(scheduled["registered"], "scheduled_task.registered")
    _require_true(scheduled["enabled"], "scheduled_task.enabled")

    worker = _require_mapping(
        probe["persistent_outbound_worker"],
        "persistent_outbound_worker",
        {"process_id", "parent_process_id", "ancestry_verified", "outbound_only"},
    )
    process_id = _require_positive_int(worker["process_id"], "persistent_outbound_worker.process_id")
    parent_id = _require_positive_int(worker["parent_process_id"], "persistent_outbound_worker.parent_process_id")
    if process_id == parent_id:
        raise ValidationError("persistent_outbound_worker ancestry is invalid")
    _require_true(worker["ancestry_verified"], "persistent_outbound_worker.ancestry_verified")
    _require_true(worker["outbound_only"], "persistent_outbound_worker.outbound_only")

    lifecycle = _require_mapping(
        probe["installed_candidate_lifecycle"],
        "installed_candidate_lifecycle",
        LIFECYCLE_KEYS,
    )
    if _require_sha(lifecycle["exact_head"], "installed_candidate_lifecycle.exact_head") != expected_source_sha:
        raise ValidationError("installed_candidate_lifecycle exact_head does not match source_sha")
    for key in sorted(LIFECYCLE_KEYS - {
        "exact_head",
        "candidate_digest",
        "prior_digest",
        "installed_candidate_digest",
        "rollback_status",
        "installed_rollback_digest",
    }):
        _require_true(lifecycle[key], f"installed_candidate_lifecycle.{key}")
    candidate = _require_sha(lifecycle["candidate_digest"], "installed_candidate_lifecycle.candidate_digest", 64)
    prior = _require_sha(lifecycle["prior_digest"], "installed_candidate_lifecycle.prior_digest", 64)
    installed_candidate = _require_sha(
        lifecycle["installed_candidate_digest"],
        "installed_candidate_lifecycle.installed_candidate_digest",
        64,
    )
    installed_rollback = _require_sha(
        lifecycle["installed_rollback_digest"],
        "installed_candidate_lifecycle.installed_rollback_digest",
        64,
    )
    if candidate == prior:
        raise ValidationError("installed_candidate_lifecycle candidate and prior digests must differ")
    if installed_candidate != candidate:
        raise ValidationError("installed_candidate_lifecycle installed candidate digest mismatch")
    if installed_rollback != prior:
        raise ValidationError("installed_candidate_lifecycle installed rollback digest mismatch")
    if lifecycle["rollback_status"] != "rolled_back":
        raise ValidationError("installed_candidate_lifecycle rollback_status must be rolled_back")

    boundaries = _require_mapping(probe["safety_boundaries"], "safety_boundaries", SAFETY_BOUNDARY_KEYS)
    for key in sorted(SAFETY_BOUNDARY_KEYS):
        _require_false(boundaries[key], f"safety_boundaries.{key}")
    if probe["hosted_windows_equivalence_exceptions"] != HOSTED_WINDOWS_EQUIVALENCE_EXCEPTIONS:
        raise ValidationError("hosted_windows_equivalence_exceptions are invalid")


def _validate_row(
    row: object,
    *,
    expected_name: str,
    expected_node_id: str,
    expected_request_id: str,
    expected_source_sha: str,
    qualification_completed_at: datetime,
) -> dict[str, str]:
    row = _require_mapping(row, f"{expected_name} row", ROW_KEYS)
    if row["sanitized_command_name"] != expected_name:
        raise ValidationError(f"{expected_name} sanitized_command_name is invalid")
    if row["node_id"] != expected_node_id:
        raise ValidationError(f"{expected_name} node_id mismatch")
    if row["request_id"] != expected_request_id:
        raise ValidationError(f"{expected_name} request_id mismatch")
    if row["worker_source_commit"] != expected_source_sha:
        raise ValidationError(f"{expected_name} worker_source_commit mismatch")
    if row["canonical_repository"] != "ao2":
        raise ValidationError(f"{expected_name} canonical_repository must be ao2")
    if row["repository_head"] != expected_source_sha:
        raise ValidationError(f"{expected_name} repository_head mismatch")
    if row["verification_profile"] != "physical_unique":
        raise ValidationError(f"{expected_name} verification_profile must be physical_unique")
    if row["status"] != "accepted":
        raise ValidationError(f"{expected_name} status must be accepted")
    _require_zero(row["exit_code"], f"{expected_name} exit_code")
    if _require_bool(row["timed_out"], f"{expected_name} timed_out"):
        raise ValidationError(f"{expected_name} timed_out must be false")
    if row["timeout_state"] != "completed":
        raise ValidationError(f"{expected_name} timeout_state must be completed")
    if _require_bool(row["output_truncated"], f"{expected_name} output_truncated"):
        raise ValidationError(f"{expected_name} output_truncated must be false")
    if row["error_category"] != "none":
        raise ValidationError(f"{expected_name} error_category must be none")
    duration = row["duration_seconds"]
    if isinstance(duration, bool) or not isinstance(duration, (int, float)) or duration < 0:
        raise ValidationError(f"{expected_name} duration_seconds must be non-negative")
    _require_string(row["bounded_sanitized_output"], f"{expected_name} bounded_sanitized_output")
    completed = _parse_time(row["completed_timestamp"], f"{expected_name} completed_timestamp")
    if completed > qualification_completed_at:
        raise ValidationError(f"{expected_name} completed_timestamp exceeds qualification completed_at")
    return {
        "node_id": row["node_id"],
        "request_id": row["request_id"],
        "worker_source_commit": row["worker_source_commit"],
        "repository_head": row["repository_head"],
        "status": row["status"],
        "exit_code": row["exit_code"],
        "timeout_state": row["timeout_state"],
        "timed_out": row["timed_out"],
        "output_truncated": row["output_truncated"],
        "error_category": row["error_category"],
        "completed_at": row["completed_timestamp"],
    }


def prepare_evidence(
    status_board: dict[str, Any],
    qualification_board: dict[str, Any],
    source_sha: str,
    version: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    source_sha = _require_sha(source_sha, "source_sha")
    version = _require_string(version, "version")
    status = _result_board(status_board, "status")
    qualification = _result_board(qualification_board, "windows_stack_qualification")
    _validate_board_boundaries(status, "status")
    _validate_board_boundaries(qualification, "qualification")

    status_wrapper = status["wrapper"]
    status_result = status["result"]
    qualification_wrapper = qualification["wrapper"]
    qualification_result = qualification["result"]
    if status_wrapper["node_id"] != qualification_wrapper["node_id"]:
        raise ValidationError("status and qualification node_id mismatch")
    if status_result.get("worker_source_commit") != source_sha:
        raise ValidationError("status worker_source_commit does not match source_sha")
    _require_false(status_result.get("windows_inbound_ports_opened"), "windows_inbound_ports_opened")
    if "windows_http_endpoint" not in status_result or status_result["windows_http_endpoint"] is not None:
        raise ValidationError("windows_http_endpoint must be null")
    if status_result.get("stack_qualification_profile_version") != PROFILE_VERSION:
        raise ValidationError("status stack_qualification_profile_version is invalid")

    if qualification_result.get("schema_version") != QUALIFICATION_RESULT_SCHEMA:
        raise ValidationError("qualification schema_version is invalid")
    if qualification_result.get("status") != "accepted":
        raise ValidationError("qualification status must be accepted")
    if qualification_result.get("mode") != "physical_unique":
        raise ValidationError("qualification mode must be physical_unique")
    if qualification_result.get("profile_version") != PROFILE_VERSION:
        raise ValidationError("qualification profile_version is invalid")
    if qualification_result.get("repositories") != ["ao2"]:
        raise ValidationError("qualification repositories must be exactly ['ao2']")

    wrapper_completed = _parse_time(qualification_wrapper["completed_at"], "qualification wrapper completed_at")
    result_completed = _parse_time(qualification_result.get("completed_at"), "qualification completed_at")
    if result_completed > wrapper_completed:
        raise ValidationError("qualification completed_at exceeds wrapper completed_at")
    raw_rows = qualification_result.get("results")
    if not isinstance(raw_rows, list):
        raise ValidationError("qualification results must be a list")
    names = [row.get("sanitized_command_name") if isinstance(row, dict) else None for row in raw_rows]
    if len(names) != len(EXPECTED_ROWS) or set(names) != set(EXPECTED_ROWS):
        raise ValidationError("qualification row inventory is invalid")
    rows_by_name = {row["sanitized_command_name"]: row for row in raw_rows}
    row_provenance: dict[str, dict[str, str]] = {}
    for name in EXPECTED_ROWS:
        row_provenance[name] = _validate_row(
            rows_by_name[name],
            expected_name=name,
            expected_node_id=qualification_wrapper["node_id"],
            expected_request_id=qualification_wrapper["request_id"],
            expected_source_sha=source_sha,
            qualification_completed_at=result_completed,
        )

    lifecycle_output = rows_by_name["physical-windows-lifecycle"]["bounded_sanitized_output"]
    probe = _json_object(lifecycle_output, "physical-windows-lifecycle output")
    _validate_probe(probe, source_sha, version)
    boundaries = dict(probe["safety_boundaries"])
    _require_false(status_wrapper["arbitrary_command_execution"], "status arbitrary_command_execution")
    _require_false(
        qualification_wrapper["arbitrary_command_execution"],
        "qualification arbitrary_command_execution",
    )
    status_readback = status["board"]["control_plane_readback"]
    status_trust = status["board"]["trust_boundary"]
    observed_boundaries = {
        "status_arbitrary_command_execution": status_wrapper["arbitrary_command_execution"],
        "qualification_arbitrary_command_execution": qualification_wrapper["arbitrary_command_execution"],
        "windows_inbound_ports_opened": status_result["windows_inbound_ports_opened"],
        "windows_http_endpoint": status_result["windows_http_endpoint"],
        "requires_credentials": status_readback["requires_credentials"],
        "can_mutate_ao2_artifacts": status_readback["can_mutate_ao2_artifacts"],
        "can_mutate_release_metadata": status_readback["can_mutate_release_metadata"],
        "stores_credentials": status_trust["stores_credentials"],
        "mutates_releases": status_trust["mutates_releases"],
    }

    evidence = {
        "schema_version": EVIDENCE_SCHEMA,
        "mode": "physical_unique",
        "source_sha": source_sha,
        "worker_source_commit": status_result["worker_source_commit"],
        "version": version,
        "node_id": qualification_wrapper["node_id"],
        "status_request_id": status_wrapper["request_id"],
        "status_result_id": status["result_id"],
        "status_completed_at": status_wrapper["completed_at"],
        "request_id": qualification_wrapper["request_id"],
        "result_id": qualification["result_id"],
        "completed_at": qualification_wrapper["completed_at"],
        "qualification_completed_at": qualification_result["completed_at"],
        "profile_version": qualification_result["profile_version"],
        "repositories": ["ao2"],
        "repository_head": source_sha,
        "row_provenance": row_provenance,
        "scheduled_task": probe["scheduled_task"],
        "persistent_outbound_worker": probe["persistent_outbound_worker"],
        "installed_candidate_lifecycle": probe["installed_candidate_lifecycle"],
        "observed_worker_boundaries": observed_boundaries,
        "safety_boundaries": boundaries,
        "hosted_windows_equivalence_exceptions": probe["hosted_windows_equivalence_exceptions"],
    }
    return evidence, validate_evidence(evidence, source_sha, version, _utc_now())


def validate_evidence(
    evidence: dict[str, Any],
    expected_source_sha: str,
    expected_version: str,
    now: datetime,
) -> dict[str, Any]:
    expected_source_sha = _require_sha(expected_source_sha, "expected_source_sha")
    expected_version = _require_string(expected_version, "expected_version")
    if not isinstance(now, datetime) or now.tzinfo is None:
        raise ValidationError("now must be a timezone-aware datetime")
    now = now.astimezone(timezone.utc)
    evidence = _require_mapping(evidence, "evidence", EVIDENCE_KEYS)
    if evidence["schema_version"] != EVIDENCE_SCHEMA:
        raise ValidationError("evidence schema_version is invalid")
    if evidence["mode"] != "physical_unique":
        raise ValidationError("evidence mode must be physical_unique")
    if _require_sha(evidence["source_sha"], "source_sha") != expected_source_sha:
        raise ValidationError("source_sha does not match expected_source_sha")
    if _require_sha(evidence["worker_source_commit"], "worker_source_commit") != expected_source_sha:
        raise ValidationError("worker_source_commit does not match source_sha")
    if _require_string(evidence["version"], "version") != expected_version:
        raise ValidationError("version does not match expected_version")
    _require_string(evidence["node_id"], "node_id")
    _require_string(evidence["status_request_id"], "status_request_id")
    _require_string(evidence["status_result_id"], "status_result_id")
    _require_string(evidence["request_id"], "request_id")
    _require_string(evidence["result_id"], "result_id")
    if evidence["profile_version"] != PROFILE_VERSION:
        raise ValidationError("profile_version is invalid")
    if evidence["repositories"] != ["ao2"]:
        raise ValidationError("repositories must be exactly ['ao2']")
    if _require_sha(evidence["repository_head"], "repository_head") != expected_source_sha:
        raise ValidationError("repository_head does not match source_sha")

    status_completed = _parse_time(evidence["status_completed_at"], "status_completed_at")
    completed = _parse_time(evidence["completed_at"], "completed_at")
    qualification_completed = _parse_time(
        evidence["qualification_completed_at"],
        "qualification_completed_at",
    )
    if completed > now:
        raise ValidationError("completed_at is in the future")
    if now - completed > timedelta(seconds=FRESHNESS_WINDOW_SECONDS):
        raise ValidationError("evidence freshness exceeds the 24-hour window")
    if status_completed > completed:
        raise ValidationError("status_completed_at exceeds completed_at")
    if now - status_completed > timedelta(seconds=FRESHNESS_WINDOW_SECONDS):
        raise ValidationError("status freshness exceeds the 24-hour window")
    if qualification_completed > completed:
        raise ValidationError("qualification_completed_at exceeds completed_at")

    provenance = _require_mapping(evidence["row_provenance"], "row_provenance", set(EXPECTED_ROWS))
    for name in EXPECTED_ROWS:
        row = _require_mapping(
            provenance[name],
            f"row_provenance.{name}",
            {
                "node_id",
                "request_id",
                "worker_source_commit",
                "repository_head",
                "status",
                "exit_code",
                "timeout_state",
                "timed_out",
                "output_truncated",
                "error_category",
                "completed_at",
            },
        )
        if row["node_id"] != evidence["node_id"]:
            raise ValidationError(f"row_provenance.{name}.node_id mismatch")
        if row["request_id"] != evidence["request_id"]:
            raise ValidationError(f"row_provenance.{name}.request_id mismatch")
        if row["worker_source_commit"] != expected_source_sha:
            raise ValidationError(f"row_provenance.{name}.worker_source_commit mismatch")
        if row["repository_head"] != expected_source_sha:
            raise ValidationError(f"row_provenance.{name}.repository_head mismatch")
        if row["status"] != "accepted":
            raise ValidationError(f"row_provenance.{name}.status must be accepted")
        _require_zero(row["exit_code"], f"row_provenance.{name}.exit_code")
        if row["timeout_state"] != "completed":
            raise ValidationError(f"row_provenance.{name}.timeout_state must be completed")
        _require_false(row["timed_out"], f"row_provenance.{name}.timed_out")
        _require_false(row["output_truncated"], f"row_provenance.{name}.output_truncated")
        if row["error_category"] != "none":
            raise ValidationError(f"row_provenance.{name}.error_category must be none")
        if _parse_time(row["completed_at"], f"row_provenance.{name}.completed_at") > qualification_completed:
            raise ValidationError(f"row_provenance.{name}.completed_at exceeds qualification")

    observed = _require_mapping(
        evidence["observed_worker_boundaries"],
        "observed_worker_boundaries",
        {
            "status_arbitrary_command_execution",
            "qualification_arbitrary_command_execution",
            "windows_inbound_ports_opened",
            "windows_http_endpoint",
            "requires_credentials",
            "can_mutate_ao2_artifacts",
            "can_mutate_release_metadata",
            "stores_credentials",
            "mutates_releases",
        },
    )
    for key in (
        "status_arbitrary_command_execution",
        "qualification_arbitrary_command_execution",
        "windows_inbound_ports_opened",
        "requires_credentials",
        "can_mutate_ao2_artifacts",
        "can_mutate_release_metadata",
        "stores_credentials",
        "mutates_releases",
    ):
        _require_false(observed[key], f"observed_worker_boundaries.{key}")
    if observed["windows_http_endpoint"] is not None:
        raise ValidationError("observed_worker_boundaries.windows_http_endpoint must be null")

    _validate_probe(
        {
            "schema_version": PROBE_SCHEMA,
            "source_sha": evidence["source_sha"],
            "version": evidence["version"],
            "scheduled_task": evidence["scheduled_task"],
            "persistent_outbound_worker": evidence["persistent_outbound_worker"],
            "installed_candidate_lifecycle": evidence["installed_candidate_lifecycle"],
            "safety_boundaries": evidence["safety_boundaries"],
            "hosted_windows_equivalence_exceptions": evidence["hosted_windows_equivalence_exceptions"],
        },
        expected_source_sha,
        expected_version,
    )
    return {
        "schema_version": SUMMARY_SCHEMA,
        "status": "passed",
        "mode": "physical_unique",
        "source_sha": evidence["source_sha"],
        "worker_source_commit": evidence["worker_source_commit"],
        "version": evidence["version"],
        "evidence_sha256": hashlib.sha256(canonical_json(evidence)).hexdigest(),
        "request_id": evidence["request_id"],
        "result_id": evidence["result_id"],
        "completed_at": evidence["completed_at"],
        "expires_at": _format_time(completed + timedelta(seconds=FRESHNESS_WINDOW_SECONDS)),
        "freshness_window_seconds": FRESHNESS_WINDOW_SECONDS,
        "failed_row_count": 0,
        "portable_suite_owner": "hosted_windows",
        "checks": {
            "scheduled_task": "passed",
            "persistent_outbound_worker": "passed",
            "installed_candidate_lifecycle": "passed",
        },
        "safety_boundaries": evidence["safety_boundaries"],
        "hosted_windows_equivalence_exceptions": evidence["hosted_windows_equivalence_exceptions"],
    }


def decode_import_payload(encoded: str, expected_digest: str) -> dict[str, Any]:
    if not isinstance(encoded, str):
        raise ValidationError("encoded payload must be a string")
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
    if hashlib.sha256(decoded).hexdigest() != expected_digest:
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
