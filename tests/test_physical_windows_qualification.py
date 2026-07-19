from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "physical_windows_qualification.py"
SOURCE_SHA = "a" * 40
VERSION = "1.2.3"
NOW = datetime(2026, 7, 19, 20, 34, 30, tzinfo=timezone.utc)


def load_qualification_module():
    spec = importlib.util.spec_from_file_location("physical_windows_qualification", MODULE_PATH)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def lifecycle_probe_output(*, completed_at: str | None = None) -> dict[str, object]:
    return {
        "schema_version": "ao2.physical-windows-lifecycle-probe.v1",
        "source_sha": SOURCE_SHA,
        "version": VERSION,
        "request_id": "physical-windows-request-001",
        "result_id": "physical-windows-result-001",
        "completed_at": completed_at or NOW.isoformat().replace("+00:00", "Z"),
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
            "package_built": True,
            "install_completed": True,
            "use_completed": True,
            "rollback_completed": True,
            "uninstall_completed": True,
            "windows_safe": True,
        },
        "safety_boundaries": {
            "inbound_http": False,
            "arbitrary_remote_execution": False,
            "credential_changes": False,
            "release_mutation": False,
        },
        "hosted_windows_equivalence_exceptions": [
            "portable test suites remain owned by hosted native Windows",
            "this probe covers only physical-Windows lifecycle evidence",
        ],
    }


def worker_status_board() -> dict[str, object]:
    return {
        "tasks": [
            {
                "ao2_cross_host": {
                    "action": "status",
                    "result": {
                        "worker_source_commit": SOURCE_SHA,
                        "arbitrary_command_execution": False,
                    },
                }
            }
        ]
    }


def qualification_board(probe: dict[str, object]) -> dict[str, object]:
    return {
        "tasks": [
            {
                "ao2_cross_host": {
                    "action": "windows_stack_qualification",
                    "result": {
                        "mode": "physical_unique",
                        "worker_source_commit": SOURCE_SHA,
                        "results": [
                            {
                                "sanitized_command_name": "physical-windows-lifecycle",
                                "status": "accepted",
                                "bounded_sanitized_output": json.dumps(probe, separators=(",", ":")),
                            }
                        ],
                    },
                }
            }
        ]
    }


def prepared_evidence():
    qualification = load_qualification_module()
    evidence, summary = qualification.prepare_evidence(
        worker_status_board(), qualification_board(lifecycle_probe_output()), SOURCE_SHA, VERSION
    )
    return qualification, evidence, summary


def test_prepare_emits_compact_canonical_evidence_and_strict_summary() -> None:
    qualification, evidence, summary = prepared_evidence()

    assert "bounded_sanitized_output" not in json.dumps(evidence)
    assert evidence["source_sha"] == SOURCE_SHA
    assert summary == qualification.validate_evidence(evidence, SOURCE_SHA, VERSION, NOW)
    assert summary["schema_version"] == "ao2.physical-windows-qualification.v1"
    assert summary["evidence_sha256"] == hashlib.sha256(qualification.canonical_json(evidence)).hexdigest()
    assert summary["freshness_window_seconds"] == 86400
    assert summary["failed_row_count"] == 0
    assert summary["portable_suite_owner"] == "hosted_windows"
    assert summary["passing"] == {
        "scheduled_task": True,
        "persistent_outbound_worker": True,
        "installed_candidate_lifecycle": True,
    }


@pytest.mark.parametrize(
    ("mutation", "error_fragment"),
    [
        (lambda evidence: evidence.__setitem__("source_sha", "b" * 40), "source_sha"),
        (lambda evidence: evidence["scheduled_task"].__setitem__("state", "Ready"), "scheduled_task"),
        (lambda evidence: evidence["persistent_outbound_worker"].__setitem__("outbound_only", False), "persistent_outbound_worker"),
        (lambda evidence: evidence["installed_candidate_lifecycle"].__setitem__("uninstall_completed", False), "installed_candidate_lifecycle"),
        (lambda evidence: evidence["safety_boundaries"].__setitem__("credential_changes", True), "credential_changes"),
        (lambda evidence: evidence.__setitem__("bounded_sanitized_output", "not compact"), "unexpected keys"),
    ],
)
def test_validate_rejects_nonqualifying_or_noncompact_evidence(mutation, error_fragment: str) -> None:
    qualification, evidence, _ = prepared_evidence()
    mutation(evidence)

    with pytest.raises(qualification.ValidationError, match=error_fragment):
        qualification.validate_evidence(evidence, SOURCE_SHA, VERSION, NOW)


def test_validate_rejects_evidence_older_than_24_hours() -> None:
    qualification, evidence, _ = prepared_evidence()
    evidence["completed_at"] = (NOW - timedelta(seconds=86401)).isoformat().replace("+00:00", "Z")

    with pytest.raises(qualification.ValidationError, match="freshness"):
        qualification.validate_evidence(evidence, SOURCE_SHA, VERSION, NOW)


def test_prepare_rejects_failed_qualification_rows() -> None:
    qualification = load_qualification_module()
    board = qualification_board(lifecycle_probe_output())
    board["tasks"][0]["ao2_cross_host"]["result"]["results"].append({
        "sanitized_command_name": "windows-worker-pytest",
        "status": "failed",
    })

    with pytest.raises(qualification.ValidationError, match="failed qualification rows"):
        qualification.prepare_evidence(worker_status_board(), board, SOURCE_SHA, VERSION)


def test_decode_import_payload_enforces_digest_duplicate_keys_and_size_limits() -> None:
    qualification, evidence, _ = prepared_evidence()
    payload = qualification.canonical_json(evidence)
    encoded = base64.b64encode(payload).decode("ascii")
    digest = hashlib.sha256(payload).hexdigest()

    assert qualification.decode_import_payload(encoded, digest) == evidence
    with pytest.raises(qualification.ValidationError, match="digest"):
        qualification.decode_import_payload(encoded, "0" * 64)
    duplicate = base64.b64encode(b'{"source_sha":"one","source_sha":"two"}').decode("ascii")
    with pytest.raises(qualification.ValidationError, match="duplicate key"):
        qualification.decode_import_payload(duplicate, hashlib.sha256(base64.b64decode(duplicate)).hexdigest())
    with pytest.raises(qualification.ValidationError, match="encoded payload"):
        qualification.decode_import_payload("a" * 60001, digest)
    too_large = base64.b64encode(b"x" * 45001).decode("ascii")
    with pytest.raises(qualification.ValidationError, match="decoded payload"):
        qualification.decode_import_payload(too_large, hashlib.sha256(b"x" * 45001).hexdigest())


def test_cli_writes_sorted_json_with_trailing_newline(tmp_path: Path, capsys) -> None:
    qualification = load_qualification_module()
    status_path = tmp_path / "status.json"
    qualification_path = tmp_path / "qualification.json"
    status_path.write_text(json.dumps(worker_status_board()), encoding="utf-8")
    qualification_path.write_text(json.dumps(qualification_board(lifecycle_probe_output())), encoding="utf-8")

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
