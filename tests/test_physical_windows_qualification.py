from __future__ import annotations

import base64
import copy
import hashlib
import importlib.util
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "physical_windows_qualification.py"
WORKER_PATH = ROOT / "scripts" / "ao2_windows_outbound_worker.py"
SOURCE_SHA = "a" * 40
VERSION = "0.5.2"
NODE_ID = "windows-hp255_g10"
STATUS_REQUEST_ID = "physical-windows-status-001"
QUALIFICATION_REQUEST_ID = "physical-windows-qualification-001"
NOW = datetime(2026, 7, 19, 20, 34, 30, tzinfo=timezone.utc)
STATUS_COMPLETED_AT = "2026-07-19T20:30:00Z"
QUALIFICATION_COMPLETED_AT = "2026-07-19T20:33:50Z"
WRAPPER_COMPLETED_AT = "2026-07-19T20:34:00Z"
EXPECTED_ROWS = (
    "windows-worker-pytest",
    "ao2-doctor",
    "windows-file-locking-rollback",
    "physical-windows-lifecycle",
)
EQUIVALENCE_EXCEPTIONS = [
    "portable test suites remain owned by hosted native Windows",
    "this probe covers only physical-Windows lifecycle evidence",
]


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def load_qualification_module():
    module = load_module("physical_windows_qualification", MODULE_PATH)
    module._utc_now = lambda: NOW
    return module


def lifecycle_probe_output() -> dict[str, object]:
    candidate = "1" * 64
    prior = "2" * 64
    return {
        "schema_version": "ao2.physical-windows-lifecycle-probe.v1",
        "source_sha": SOURCE_SHA,
        "version": VERSION,
        "scheduled_task": {
            "task_name": "AO2 Windows Outbound Worker",
            "registered": True,
            "enabled": True,
            "state": "Running",
        },
        "persistent_outbound_worker": {
            "process_id": 101,
            "parent_process_id": 100,
            "ancestry_verified": True,
            "outbound_only": True,
        },
        "installed_candidate_lifecycle": {
            "exact_head": SOURCE_SHA,
            "source_version_verified": True,
            "debug_prior_built": True,
            "release_candidate_built": True,
            "candidate_package_created": True,
            "package_manifest_verified": True,
            "package_provenance_verified": True,
            "install_completed": True,
            "install_verification_verified": True,
            "candidate_use_verified": True,
            "candidate_digest": candidate,
            "prior_digest": prior,
            "installed_candidate_digest": candidate,
            "rollback_runner_separate": True,
            "rollback_status": "rolled_back",
            "rollback_completed": True,
            "installed_rollback_digest": prior,
            "rollback_use_verified": True,
            "uninstall_completed": True,
            "temp_cleanup_completed": True,
            "windows_safe": True,
        },
        "safety_boundaries": {
            "inbound_http": False,
            "arbitrary_remote_execution": False,
            "credential_changes": False,
            "release_mutation": False,
            "self_hosted_public_repository_runner": False,
        },
        "hosted_windows_equivalence_exceptions": EQUIVALENCE_EXCEPTIONS,
    }


def stack_row(name: str, probe: dict[str, object]) -> dict[str, object]:
    output = json.dumps(probe, separators=(",", ":")) if name == "physical-windows-lifecycle" else "ok"
    return {
        "node_id": NODE_ID,
        "worker_source_commit": SOURCE_SHA,
        "request_id": QUALIFICATION_REQUEST_ID,
        "canonical_repository": "ao2",
        "repository_head": SOURCE_SHA,
        "verification_profile": "physical_unique",
        "sanitized_command_name": name,
        "status": "accepted",
        "exit_code": 0,
        "timeout_state": "completed",
        "timed_out": False,
        "duration_seconds": 0.01,
        "error_category": "none",
        "bounded_sanitized_output": output,
        "output_truncated": False,
        "completed_timestamp": QUALIFICATION_COMPLETED_AT,
    }


def result_board(action: str, request_id: str, result: dict[str, object], completed_at: str) -> dict[str, object]:
    worker = load_module(f"ao2_windows_outbound_worker_{action}", WORKER_PATH)
    worker.utc_now = lambda: completed_at
    runtime = worker.WindowsOutboundWorker(
        node_id=NODE_ID,
        factory_root=ROOT,
        state=worker.WorkerState(ROOT / "target" / "physical-windows-fixture-state"),
        transport=worker.MemoryTransport(),
    )
    return runtime.result_board(request_id, action, result)


def worker_status_board() -> dict[str, object]:
    result = {
        "node_id": NODE_ID,
        "hostname": "ao2-physical-windows",
        "os_caption": "Microsoft Windows",
        "os_version": "Microsoft Windows 11 Pro",
        "factory_root": r"C:\ao\factory",
        "state_root": r"C:\ao\state",
        "allowed_actions": [
            "status",
            "publish_capability",
            "sync_ao_stack",
            "ao2_doctor",
            "timeout_fixture",
            "windows_stack_qualification",
        ],
        "worker_source_commit": SOURCE_SHA,
        "stack_qualification_profile_version": "ao2.windows-stack-qualification.profiles.v1",
        "mac_should_probe_windows": False,
        "windows_http_endpoint": None,
        "windows_inbound_ports_opened": False,
        "running_actions": 0,
    }
    return result_board("status", STATUS_REQUEST_ID, result, STATUS_COMPLETED_AT)


def qualification_board(probe: dict[str, object] | None = None) -> dict[str, object]:
    result = {
        "schema_version": "ao2.windows-stack-qualification-result.v1",
        "status": "accepted",
        "mode": "physical_unique",
        "profile_version": "ao2.windows-stack-qualification.profiles.v1",
        "repositories": ["ao2"],
        "results": [stack_row(name, probe or lifecycle_probe_output()) for name in EXPECTED_ROWS],
        "completed_at": QUALIFICATION_COMPLETED_AT,
    }
    return result_board(
        "windows_stack_qualification",
        QUALIFICATION_REQUEST_ID,
        result,
        WRAPPER_COMPLETED_AT,
    )


def task(board: dict[str, object]) -> dict[str, object]:
    return board["tasks"][0]


def wrapper(board: dict[str, object]) -> dict[str, object]:
    return task(board)["ao2_cross_host"]


def qualification_result(board: dict[str, object]) -> dict[str, object]:
    return wrapper(board)["result"]


def rows(board: dict[str, object]) -> list[dict[str, object]]:
    return qualification_result(board)["results"]


def prepared_evidence():
    qualification = load_qualification_module()
    evidence, summary = qualification.prepare_evidence(
        worker_status_board(), qualification_board(), SOURCE_SHA, VERSION
    )
    return qualification, evidence, summary


def test_production_fixture_matches_windows_outbound_worker_result_board_shape() -> None:
    status = worker_status_board()
    qualification = qualification_board()

    assert status["schema_version"] == "ao2.ai-task-board.v1"
    assert task(status)["task_id"] == f"windows-worker-result-status-{STATUS_REQUEST_ID}"
    assert wrapper(status)["schema_version"] == "ao2.cross-host.windows-worker-result.v1"
    assert "arbitrary_command_execution" not in wrapper(status)["result"]
    assert task(qualification)["task_id"] == (
        f"windows-worker-result-windows-stack-qualification-{QUALIFICATION_REQUEST_ID}"
    )
    assert "worker_source_commit" not in qualification_result(qualification)
    assert set(row["sanitized_command_name"] for row in rows(qualification)) == set(EXPECTED_ROWS)


def test_prepare_binds_real_wrapper_task_and_row_provenance() -> None:
    qualification, evidence, summary = prepared_evidence()

    assert "bounded_sanitized_output" not in json.dumps(evidence)
    assert evidence["status_request_id"] == STATUS_REQUEST_ID
    assert evidence["status_result_id"] == f"windows-worker-result-status-{STATUS_REQUEST_ID}"
    assert evidence["request_id"] == QUALIFICATION_REQUEST_ID
    assert evidence["result_id"] == (
        f"windows-worker-result-windows-stack-qualification-{QUALIFICATION_REQUEST_ID}"
    )
    assert evidence["completed_at"] == WRAPPER_COMPLETED_AT
    assert evidence["qualification_completed_at"] == QUALIFICATION_COMPLETED_AT
    assert evidence["repository_head"] == SOURCE_SHA
    assert set(evidence["row_provenance"]) == set(EXPECTED_ROWS)
    assert all(item["request_id"] == QUALIFICATION_REQUEST_ID for item in evidence["row_provenance"].values())
    assert all(item["status"] == "accepted" for item in evidence["row_provenance"].values())
    assert all(item["timed_out"] is False for item in evidence["row_provenance"].values())
    assert evidence["observed_worker_boundaries"] == {
        "status_arbitrary_command_execution": False,
        "qualification_arbitrary_command_execution": False,
        "windows_inbound_ports_opened": False,
        "windows_http_endpoint": None,
        "requires_credentials": False,
        "can_mutate_ao2_artifacts": False,
        "can_mutate_release_metadata": False,
        "stores_credentials": False,
        "mutates_releases": False,
    }
    assert summary == qualification.validate_evidence(evidence, SOURCE_SHA, VERSION, NOW)


def test_summary_matches_strict_hosted_consumer_contract() -> None:
    qualification, evidence, summary = prepared_evidence()

    assert set(summary) == {
        "schema_version",
        "status",
        "mode",
        "source_sha",
        "worker_source_commit",
        "version",
        "evidence_sha256",
        "request_id",
        "result_id",
        "completed_at",
        "expires_at",
        "freshness_window_seconds",
        "failed_row_count",
        "portable_suite_owner",
        "checks",
        "safety_boundaries",
        "hosted_windows_equivalence_exceptions",
    }
    assert summary["schema_version"] == "ao2.physical-windows-qualification-summary.v1"
    assert summary["status"] == "passed"
    assert summary["mode"] == "physical_unique"
    assert summary["expires_at"] == "2026-07-20T20:34:00Z"
    assert summary["checks"] == {
        "scheduled_task": "passed",
        "persistent_outbound_worker": "passed",
        "installed_candidate_lifecycle": "passed",
    }
    assert summary["safety_boundaries"]["self_hosted_public_repository_runner"] is False
    assert summary["evidence_sha256"] == hashlib.sha256(qualification.canonical_json(evidence)).hexdigest()


@pytest.mark.parametrize(
    ("mutate", "error"),
    [
        (
            lambda status, board: qualification_result(board).__setitem__("mode", "full"),
            "mode",
        ),
        (
            lambda status, board: qualification_result(board).__setitem__("schema_version", "wrong"),
            "schema_version",
        ),
        (
            lambda status, board: qualification_result(board).__setitem__("status", "failed"),
            "qualification status",
        ),
        (
            lambda status, board: qualification_result(board).__setitem__("profile_version", "wrong"),
            "profile_version",
        ),
        (
            lambda status, board: qualification_result(board).__setitem__("repositories", ["ao2", "ao-command"]),
            "repositories",
        ),
    ],
)
def test_prepare_rejects_wrong_top_level_qualification_contract(mutate, error: str) -> None:
    qualification = load_qualification_module()
    status = worker_status_board()
    board = qualification_board()
    mutate(status, board)

    with pytest.raises(qualification.ValidationError, match=error):
        qualification.prepare_evidence(status, board, SOURCE_SHA, VERSION)


def test_prepare_rejects_wrong_probe_version() -> None:
    qualification = load_qualification_module()
    probe = lifecycle_probe_output()
    probe["version"] = "9.9.9"

    with pytest.raises(qualification.ValidationError, match="version"):
        qualification.prepare_evidence(worker_status_board(), qualification_board(probe), SOURCE_SHA, VERSION)


def test_prepare_rejects_missing_row_worker_provenance() -> None:
    qualification = load_qualification_module()
    board = qualification_board()
    rows(board)[0].pop("worker_source_commit")

    with pytest.raises(qualification.ValidationError, match="worker_source_commit"):
        qualification.prepare_evidence(worker_status_board(), board, SOURCE_SHA, VERSION)


def test_prepare_rejects_required_row_omission() -> None:
    qualification = load_qualification_module()
    board = qualification_board()
    qualification_result(board)["results"] = rows(board)[1:]

    with pytest.raises(qualification.ValidationError, match="row inventory"):
        qualification.prepare_evidence(worker_status_board(), board, SOURCE_SHA, VERSION)


@pytest.mark.parametrize(
    ("field", "value", "error"),
    [
        ("repository_head", "b" * 40, "repository_head"),
        ("request_id", "wrong-request", "request_id"),
        ("worker_source_commit", "b" * 40, "worker_source_commit"),
    ],
)
def test_prepare_rejects_row_provenance_mismatch(field: str, value: object, error: str) -> None:
    qualification = load_qualification_module()
    board = qualification_board()
    rows(board)[1][field] = value

    with pytest.raises(qualification.ValidationError, match=error):
        qualification.prepare_evidence(worker_status_board(), board, SOURCE_SHA, VERSION)


@pytest.mark.parametrize(
    ("updates", "error"),
    [
        ({"timed_out": True, "timeout_state": "timed_out"}, "timed_out"),
        ({"output_truncated": True}, "output_truncated"),
        ({"status": "failed", "exit_code": 1}, "status"),
    ],
)
def test_prepare_rejects_failed_timeout_or_truncated_sibling_rows(updates: dict[str, object], error: str) -> None:
    qualification = load_qualification_module()
    board = qualification_board()
    rows(board)[0].update(updates)

    with pytest.raises(qualification.ValidationError, match=error):
        qualification.prepare_evidence(worker_status_board(), board, SOURCE_SHA, VERSION)


@pytest.mark.parametrize(
    ("mutate", "error"),
    [
        (
            lambda status, board: wrapper(board).__setitem__("arbitrary_command_execution", True),
            "arbitrary_command_execution",
        ),
        (
            lambda status, board: wrapper(status)["result"].__setitem__("windows_inbound_ports_opened", True),
            "windows_inbound_ports_opened",
        ),
        (
            lambda status, board: wrapper(status)["result"].__setitem__(
                "windows_http_endpoint", "http://127.0.0.1:8080"
            ),
            "windows_http_endpoint",
        ),
        (
            lambda status, board: status["control_plane_readback"].__setitem__("requires_credentials", True),
            "requires_credentials",
        ),
        (
            lambda status, board: board["trust_boundary"].__setitem__("mutates_releases", True),
            "mutates_releases",
        ),
    ],
)
def test_prepare_rejects_unsafe_observed_board_boundaries(mutate, error: str) -> None:
    qualification = load_qualification_module()
    status = worker_status_board()
    board = qualification_board()
    mutate(status, board)

    with pytest.raises(qualification.ValidationError, match=error):
        qualification.prepare_evidence(status, board, SOURCE_SHA, VERSION)


@pytest.mark.parametrize("boundary", [
    "inbound_http",
    "arbitrary_remote_execution",
    "credential_changes",
    "release_mutation",
    "self_hosted_public_repository_runner",
])
def test_prepare_rejects_unsafe_probe_boundaries(boundary: str) -> None:
    qualification = load_qualification_module()
    probe = lifecycle_probe_output()
    probe["safety_boundaries"][boundary] = True

    with pytest.raises(qualification.ValidationError, match=boundary):
        qualification.prepare_evidence(worker_status_board(), qualification_board(probe), SOURCE_SHA, VERSION)


def test_validate_rejects_future_and_stale_completion_times() -> None:
    qualification, evidence, _ = prepared_evidence()

    future = copy.deepcopy(evidence)
    future["completed_at"] = (NOW + timedelta(seconds=1)).isoformat().replace("+00:00", "Z")
    with pytest.raises(qualification.ValidationError, match="future"):
        qualification.validate_evidence(future, SOURCE_SHA, VERSION, NOW)

    stale = copy.deepcopy(evidence)
    stale["completed_at"] = (NOW - timedelta(seconds=86401)).isoformat().replace("+00:00", "Z")
    with pytest.raises(qualification.ValidationError, match="freshness"):
        qualification.validate_evidence(stale, SOURCE_SHA, VERSION, NOW)

    stale_status = copy.deepcopy(evidence)
    stale_status["status_completed_at"] = (NOW - timedelta(seconds=86401)).isoformat().replace("+00:00", "Z")
    with pytest.raises(qualification.ValidationError, match="status freshness"):
        qualification.validate_evidence(stale_status, SOURCE_SHA, VERSION, NOW)


def test_validate_rejects_mutated_observed_worker_boundaries() -> None:
    qualification, evidence, _ = prepared_evidence()
    evidence["observed_worker_boundaries"]["windows_inbound_ports_opened"] = True

    with pytest.raises(qualification.ValidationError, match="windows_inbound_ports_opened"):
        qualification.validate_evidence(evidence, SOURCE_SHA, VERSION, NOW)


def test_validate_rejects_wrong_expected_version_and_noncompact_evidence() -> None:
    qualification, evidence, _ = prepared_evidence()

    with pytest.raises(qualification.ValidationError, match="version"):
        qualification.validate_evidence(evidence, SOURCE_SHA, "9.9.9", NOW)
    evidence["bounded_sanitized_output"] = "not compact"
    with pytest.raises(qualification.ValidationError, match="unexpected keys"):
        qualification.validate_evidence(evidence, SOURCE_SHA, VERSION, NOW)


def test_decode_import_payload_rejects_digest_duplicates_malformed_and_non_json() -> None:
    qualification, evidence, _ = prepared_evidence()
    payload = qualification.canonical_json(evidence)
    encoded = base64.b64encode(payload).decode("ascii")
    digest = hashlib.sha256(payload).hexdigest()

    assert qualification.decode_import_payload(encoded, digest) == evidence
    with pytest.raises(qualification.ValidationError, match="digest"):
        qualification.decode_import_payload(encoded, "0" * 64)
    duplicate_bytes = b'{"source_sha":"one","source_sha":"two"}'
    duplicate = base64.b64encode(duplicate_bytes).decode("ascii")
    with pytest.raises(qualification.ValidationError, match="duplicate key"):
        qualification.decode_import_payload(duplicate, hashlib.sha256(duplicate_bytes).hexdigest())
    with pytest.raises(qualification.ValidationError, match="base64"):
        qualification.decode_import_payload("not-base64!", hashlib.sha256(b"").hexdigest())
    non_json_bytes = b"not JSON\n"
    with pytest.raises(qualification.ValidationError, match="JSON"):
        qualification.decode_import_payload(
            base64.b64encode(non_json_bytes).decode("ascii"),
            hashlib.sha256(non_json_bytes).hexdigest(),
        )
    with pytest.raises(qualification.ValidationError, match="string"):
        qualification.decode_import_payload(123, digest)


def test_decode_import_payload_enforces_encoded_and_decoded_size_limits() -> None:
    qualification = load_qualification_module()

    with pytest.raises(qualification.ValidationError, match="encoded payload"):
        qualification.decode_import_payload("a" * 60001, "0" * 64)
    decoded = b"x" * 45001
    with pytest.raises(qualification.ValidationError, match="decoded payload"):
        qualification.decode_import_payload(
            base64.b64encode(decoded).decode("ascii"),
            hashlib.sha256(decoded).hexdigest(),
        )


def test_cli_writes_canonical_json_with_trailing_newline(tmp_path: Path, capsys) -> None:
    qualification = load_qualification_module()
    status_path = tmp_path / "status.json"
    qualification_path = tmp_path / "qualification.json"
    status_path.write_text(json.dumps(worker_status_board()), encoding="utf-8")
    qualification_path.write_text(json.dumps(qualification_board()), encoding="utf-8")

    assert qualification.main([
        "prepare",
        "--status-board",
        str(status_path),
        "--qualification-board",
        str(qualification_path),
        "--source-sha",
        SOURCE_SHA,
        "--version",
        VERSION,
    ]) == 0
    output = capsys.readouterr().out

    assert output.endswith("\n")
    assert output == json.dumps(json.loads(output), sort_keys=True, separators=(",", ":")) + "\n"
