from __future__ import annotations

import base64
import copy
import hashlib
import importlib.util
import io
import json
import re
import subprocess
import sys
import tarfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest
import yaml


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "physical_windows_qualification.py"
IMPORT_SCRIPT_PATH = ROOT / "scripts" / "import_physical_windows_qualification.py"
RUN_METADATA_SCRIPT_PATH = ROOT / "scripts" / "validate_physical_windows_workflow_run.py"
HOSTED_CANDIDATE_SCRIPT_PATH = ROOT / "scripts" / "validate_hosted_release_candidates.py"
HOSTED_PROMOTION_SCRIPT_PATH = ROOT / "scripts" / "hosted_release_promotion.py"
WORKER_PATH = ROOT / "scripts" / "ao2_windows_outbound_worker.py"
IMPORT_WORKFLOW_PATH = ROOT / ".github" / "workflows" / "import-physical-windows-qualification.yml"
PUBLIC_RELEASE_WORKFLOW_PATH = ROOT / ".github" / "workflows" / "public-release-build.yml"
SOURCE_SHA = "a" * 40
VERSION = "0.5.3"
NODE_ID = "windows-hp255_g10"
STATUS_REQUEST_ID = "physical-windows-status-001"
QUALIFICATION_REQUEST_ID = "physical-windows-qualification-001"
NOW = datetime(2026, 7, 19, 20, 34, 30, tzinfo=timezone.utc)
STATUS_COMPLETED_AT = "2026-07-19T20:30:00Z"
QUALIFICATION_COMPLETED_AT = "2026-07-19T20:33:50Z"
WRAPPER_COMPLETED_AT = "2026-07-19T20:34:00Z"
CHECKPOINT_ID = "physical-windows-checkpoint"
PROFILE_DIGEST = "sha256:physical-windows-profile"
SHARD_ID = "physical-windows-shard"
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


def load_import_workflow() -> dict[str, object]:
    return yaml.load(IMPORT_WORKFLOW_PATH.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def load_public_release_workflow() -> dict[str, object]:
    return yaml.load(
        PUBLIC_RELEASE_WORKFLOW_PATH.read_text(encoding="utf-8"),
        Loader=yaml.BaseLoader,
    )


def load_import_script():
    load_qualification_module()
    return load_module("import_physical_windows_qualification", IMPORT_SCRIPT_PATH)


def load_run_metadata_script():
    return load_module(
        "validate_physical_windows_workflow_run",
        RUN_METADATA_SCRIPT_PATH,
    )


def load_hosted_candidate_script():
    return load_module(
        "validate_hosted_release_candidates",
        HOSTED_CANDIDATE_SCRIPT_PATH,
    )


def load_hosted_promotion_script():
    return load_module(
        "hosted_release_promotion",
        HOSTED_PROMOTION_SCRIPT_PATH,
    )


def create_hosted_candidate_fixture(
    root: Path,
    *,
    source_sha: str = SOURCE_SHA,
    version: str = VERSION,
) -> Path:
    binary_names = {
        "linux-x86_64": "bin/ao2",
        "macos-aarch64": "bin/ao2",
        "windows-x86_64": "bin/ao2.exe",
    }
    common_files = {
        "LICENSE": b"license\n",
        "NOTICE": b"notice\n",
        "README.txt": b"readme\n",
        "SBOM.cdx.json": b'{"bomFormat":"CycloneDX"}\n',
        "UNINSTALL.txt": b"uninstall\n",
        "VERSION": f"{version}\n".encode(),
        "Verify-Release.ps1": b"Write-Output verified\n",
        "install.ps1": b"Write-Output installed\n",
        "install.sh": b"#!/bin/sh\n",
        "verify-release.sh": b"#!/bin/sh\n",
    }
    for target, binary_name in binary_names.items():
        artifact = root / f"ao2-hosted-native-candidate-{target}-{source_sha}"
        dist = artifact / "dist"
        dist.mkdir(parents=True)
        archive_name = f"ao2-{version}-{target}.tar.gz"
        archive = dist / archive_name
        files = dict(common_files)
        files[binary_name] = f"binary:{target}\n".encode()
        files["BUILD-PROVENANCE.json"] = (
            json.dumps(
                {
                    "build_profile": "release",
                    "git_commit": source_sha,
                    "package": "ao2",
                    "schema_version": "ao2.build-provenance.v1",
                    "target": target,
                    "version": version,
                },
                sort_keys=True,
            )
            + "\n"
        ).encode()
        manifest_files = sorted([*files, "RELEASE-MANIFEST.json", "RELEASE-VERIFICATION.json", "SHA256SUMS"])
        files["RELEASE-MANIFEST.json"] = (
            json.dumps(
                {
                    "binary": "ao2",
                    "binary_path": binary_name,
                    "binary_sha256": hashlib.sha256(files[binary_name]).hexdigest(),
                    "build_provenance": "BUILD-PROVENANCE.json",
                    "checksum_file": "SHA256SUMS",
                    "files": manifest_files,
                    "package": f"ao2-{version}-{target}",
                    "schema_version": "ao2.release-manifest.v1",
                    "target": target,
                    "version": version,
                },
                sort_keys=True,
            )
            + "\n"
        ).encode()
        files["RELEASE-VERIFICATION.json"] = (
            json.dumps(
                {
                    "binary_path": binary_name,
                    "control_plane_approves_release": False,
                    "mutates_ao_artifacts": False,
                    "provider_api_keys_required": False,
                    "schema_version": "ao2.release-archive-offline-verification.v1",
                    "status": "packaged",
                    "target": target,
                    "version": version,
                },
                sort_keys=True,
            )
            + "\n"
        ).encode()
        files["SHA256SUMS"] = "".join(
            f"{hashlib.sha256(body).hexdigest()}  {name}\n"
            for name, body in sorted(files.items())
        ).encode()
        with tarfile.open(archive, "w:gz") as bundle:
            for name, body in sorted(files.items()):
                info = tarfile.TarInfo(name)
                info.mode = 0o755 if name == binary_name or name.endswith(".sh") else 0o644
                info.size = len(body)
                bundle.addfile(info, io.BytesIO(body))
        summary = {
            "archive": (
                f"target\\release-archive-hosted-smoke\\{target}\\dist\\{archive_name}"
                if target == "windows-x86_64"
                else f"target/release-archive-hosted-smoke/{target}/dist/{archive_name}"
            ),
            "control_plane_approves_release": False,
            "install_verification_evidence": "target/install-verification.json",
            "install_verification_schema": "ao2.install-verification-evidence.v1",
            "installed_binary": binary_name,
            "mutates_ao_artifacts": False,
            "provider_api_keys_required": False,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "schema_version": "ao2.release-archive-hosted-smoke.v1",
            "status": "passed",
            "target": target,
            "version": version,
        }
        (artifact / "summary.json").write_text(
            json.dumps(summary, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        if target == "windows-x86_64":
            (artifact / "windows-coverage-ownership.json").write_text(
                json.dumps(
                    {
                        "hosted_windows_portable_suite_owner": True,
                        "linux_mingw_x86_64_pc_windows_gnu": "non_authoritative",
                        "physical_windows_mode": "physical_unique",
                        "schema_version": "ao2.windows-coverage-ownership.v1",
                        "status": "passed",
                        "target_triple": "x86_64-pc-windows-msvc",
                    },
                    sort_keys=True,
                )
                + "\n",
                encoding="utf-8",
            )
    return root


def producer_run_metadata(
    *,
    run_id: int = 123456789,
    repository: str = "uesugitorachiyo/ao2",
    source_sha: str = SOURCE_SHA,
) -> dict[str, object]:
    return {
        "id": run_id,
        "name": "Import Physical Windows Qualification",
        "path": ".github/workflows/import-physical-windows-qualification.yml",
        "event": "workflow_dispatch",
        "status": "completed",
        "conclusion": "success",
        "head_sha": source_sha,
        "repository": {
            "id": 987654321,
            "full_name": repository,
        },
    }


def producer_artifact_metadata(
    *,
    run_id: int = 123456789,
    source_sha: str = SOURCE_SHA,
) -> dict[str, object]:
    return {
        "total_count": 1,
        "artifacts": [
            {
                "id": 24681012,
                "name": "ao2-physical-windows-qualification",
                "expired": False,
                "workflow_run": {
                    "id": run_id,
                    "repository_id": 987654321,
                    "head_repository_id": 987654321,
                    "head_sha": source_sha,
                },
            }
        ],
    }


def replace_source_sha(value: object, source_sha: str) -> object:
    if isinstance(value, dict):
        return {key: replace_source_sha(item, source_sha) for key, item in value.items()}
    if isinstance(value, list):
        return [replace_source_sha(item, source_sha) for item in value]
    return source_sha if value == SOURCE_SHA else value


def create_import_fixture(tmp_path: Path) -> tuple[Path, dict[str, str], bytes]:
    repository = tmp_path / "repo"
    scripts = repository / "scripts"
    scripts.mkdir(parents=True)
    version_script = scripts / "current-version.sh"
    version_script.write_text(
        "#!/bin/sh\nset -eu\nawk '$1 == \"version\" && $2 == \"=\" {gsub(/\\\"/, \"\", $3); print $3; exit}' Cargo.toml\n",
        encoding="utf-8",
    )
    version_script.chmod(0o755)
    (repository / "Cargo.toml").write_text(
        f'[workspace.package]\nversion = "{VERSION}"\n',
        encoding="utf-8",
    )
    subprocess.run(["git", "init", "-q"], cwd=repository, check=True)
    subprocess.run(["git", "add", "Cargo.toml", "scripts/current-version.sh"], cwd=repository, check=True)
    subprocess.run(
        [
            "git",
            "-c",
            "user.name=AO2 Test",
            "-c",
            "user.email=ao2-test@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
        cwd=repository,
        check=True,
    )
    source_sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
    qualification, evidence, _ = prepared_evidence()
    evidence = replace_source_sha(evidence, source_sha)
    payload = qualification.canonical_json(evidence)
    environment = {
        "EVIDENCE_BASE64": base64.b64encode(payload).decode("ascii"),
        "EVIDENCE_SHA256": hashlib.sha256(payload).hexdigest(),
        "SOURCE_SHA": source_sha,
        "VERSION": VERSION,
        "GITHUB_SHA": source_sha,
    }
    return repository, environment, payload


def install_import_environment(monkeypatch, environment: dict[str, str]) -> None:
    for name, value in environment.items():
        monkeypatch.setenv(name, value)


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
            "last_task_result": 267009,
            "result_acceptable": True,
            "action_matches_worker": True,
        },
        "persistent_outbound_worker": {
            "probe_process_id": 102,
            "process_id": 101,
            "parent_process_id": 100,
            "probe_parent_is_worker": True,
            "worker_executable_is_python": True,
            "worker_script_matches": True,
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
        "checkpoint_id": CHECKPOINT_ID,
        "profile_digest": PROFILE_DIGEST,
        "shard_id": SHARD_ID,
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


def qualification_board(
    probe: dict[str, object] | None = None,
    *,
    wrapper_completed_at: str = WRAPPER_COMPLETED_AT,
) -> dict[str, object]:
    result = {
        "schema_version": "ao2.windows-stack-qualification-result.v1",
        "status": "accepted",
        "mode": "physical_unique",
        "profile_version": "ao2.windows-stack-qualification.profiles.v1",
        "checkpoint_id": CHECKPOINT_ID,
        "profile_digest": PROFILE_DIGEST,
        "shard_id": SHARD_ID,
        "repositories": ["ao2"],
        "results": [stack_row(name, probe or lifecycle_probe_output()) for name in EXPECTED_ROWS],
        "completed_at": QUALIFICATION_COMPLETED_AT,
    }
    return result_board(
        "windows_stack_qualification",
        QUALIFICATION_REQUEST_ID,
        result,
        wrapper_completed_at,
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
    assert all(row["checkpoint_id"] == CHECKPOINT_ID for row in rows(qualification))
    assert all(row["profile_digest"] == PROFILE_DIGEST for row in rows(qualification))
    assert all(row["shard_id"] == SHARD_ID for row in rows(qualification))


@pytest.mark.parametrize(
    ("field", "invalid"),
    [
        ("checkpoint_id", "wrong-checkpoint"),
        ("profile_digest", "sha256:wrong-profile"),
        ("shard_id", "wrong-shard"),
    ],
)
def test_prepare_rejects_row_qualification_provenance_mismatch(field: str, invalid: str) -> None:
    qualification = load_qualification_module()
    board = qualification_board()
    rows(board)[0][field] = invalid

    with pytest.raises(qualification.ValidationError, match=field):
        qualification.prepare_evidence(worker_status_board(), board, SOURCE_SHA, VERSION)


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
        "physical_evidence_sha256",
        "status_request_id",
        "status_result_id",
        "qualification_request_id",
        "qualification_result_id",
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
    assert summary["status_request_id"] == STATUS_REQUEST_ID
    assert summary["status_result_id"] == f"windows-worker-result-status-{STATUS_REQUEST_ID}"
    assert summary["qualification_request_id"] == QUALIFICATION_REQUEST_ID
    assert summary["qualification_result_id"] == (
        f"windows-worker-result-windows-stack-qualification-{QUALIFICATION_REQUEST_ID}"
    )
    assert summary["physical_evidence_sha256"] == hashlib.sha256(
        qualification.canonical_json(evidence)
    ).hexdigest()


def test_summary_preserves_production_fractional_seconds_in_expiry() -> None:
    qualification = load_qualification_module()
    fractional_completed_at = "2026-07-19T20:34:00.123456Z"

    _, summary = qualification.prepare_evidence(
        worker_status_board(),
        qualification_board(wrapper_completed_at=fractional_completed_at),
        SOURCE_SHA,
        VERSION,
    )

    assert summary["completed_at"] == fractional_completed_at
    assert summary["expires_at"] == "2026-07-20T20:34:00.123456Z"


def test_prepare_accepts_ready_task_with_successful_last_result() -> None:
    qualification = load_qualification_module()
    probe = lifecycle_probe_output()
    probe["scheduled_task"]["state"] = "Ready"
    probe["scheduled_task"]["last_task_result"] = 0

    _, summary = qualification.prepare_evidence(
        worker_status_board(),
        qualification_board(probe),
        SOURCE_SHA,
        VERSION,
    )

    assert summary["checks"]["scheduled_task"] == "passed"


@pytest.mark.parametrize(
    ("state", "last_task_result", "result_acceptable"),
    [
        ("Running", 1, False),
        ("Ready", 267009, False),
        ("Disabled", 0, True),
    ],
)
def test_prepare_rejects_unacceptable_scheduled_task_result(
    state: str,
    last_task_result: int,
    result_acceptable: bool,
) -> None:
    qualification = load_qualification_module()
    probe = lifecycle_probe_output()
    probe["scheduled_task"].update(
        {
            "state": state,
            "last_task_result": last_task_result,
            "result_acceptable": result_acceptable,
        }
    )

    with pytest.raises(qualification.ValidationError, match="scheduled_task"):
        qualification.prepare_evidence(
            worker_status_board(),
            qualification_board(probe),
            SOURCE_SHA,
            VERSION,
        )


@pytest.mark.parametrize(
    ("section", "field"),
    [
        ("scheduled_task", "action_matches_worker"),
        ("persistent_outbound_worker", "probe_parent_is_worker"),
        ("persistent_outbound_worker", "worker_executable_is_python"),
        ("persistent_outbound_worker", "worker_script_matches"),
        ("persistent_outbound_worker", "ancestry_verified"),
    ],
)
def test_prepare_rejects_unverified_worker_task_correlation(
    section: str,
    field: str,
) -> None:
    qualification = load_qualification_module()
    probe = lifecycle_probe_output()
    probe[section][field] = False

    with pytest.raises(qualification.ValidationError, match=field):
        qualification.prepare_evidence(
            worker_status_board(),
            qualification_board(probe),
            SOURCE_SHA,
            VERSION,
        )


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


def test_prepare_rejects_status_result_node_mismatch() -> None:
    qualification = load_qualification_module()
    status = worker_status_board()
    wrapper(status)["result"]["node_id"] = "different-node"

    with pytest.raises(qualification.ValidationError, match="status result node_id"):
        qualification.prepare_evidence(status, qualification_board(), SOURCE_SHA, VERSION)


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


def test_validate_rejects_evidence_at_exact_expiry_boundary() -> None:
    qualification, evidence, _ = prepared_evidence()

    expired = copy.deepcopy(evidence)
    exact_expiry_source = (NOW - timedelta(seconds=86400)).isoformat().replace("+00:00", "Z")
    expired["status_completed_at"] = exact_expiry_source
    expired["qualification_completed_at"] = exact_expiry_source
    expired["completed_at"] = exact_expiry_source
    with pytest.raises(qualification.ValidationError, match="freshness"):
        qualification.validate_evidence(expired, SOURCE_SHA, VERSION, NOW)

    expired_status = copy.deepcopy(evidence)
    expired_status["status_completed_at"] = (
        NOW - timedelta(seconds=86400)
    ).isoformat().replace("+00:00", "Z")
    with pytest.raises(qualification.ValidationError, match="status freshness"):
        qualification.validate_evidence(expired_status, SOURCE_SHA, VERSION, NOW)


@pytest.mark.parametrize(
    "timestamp",
    [
        "2026-07-19X20:34:30Z",
        "2026-07-19T20:34Z",
        "2026-07-19T20:34:30+00:00:30",
    ],
)
def test_validate_rejects_non_rfc3339_timestamp_spellings(timestamp: str) -> None:
    qualification, evidence, _ = prepared_evidence()
    evidence["completed_at"] = timestamp

    with pytest.raises(qualification.ValidationError, match="RFC 3339"):
        qualification.validate_evidence(evidence, SOURCE_SHA, VERSION, NOW)


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


def test_import_workflow_is_manual_read_only_and_binds_exact_source() -> None:
    workflow = load_import_workflow()

    assert set(workflow) == {"name", "on", "permissions", "jobs"}
    assert set(workflow["on"]) == {"workflow_dispatch"}
    inputs = workflow["on"]["workflow_dispatch"]["inputs"]
    assert set(inputs) == {"evidence_base64", "evidence_sha256", "source_sha", "version"}
    for input_spec in inputs.values():
        assert input_spec["required"] == "true"
        assert input_spec["type"] == "string"
    assert workflow["permissions"] == {"contents": "read"}

    jobs = workflow["jobs"]
    assert set(jobs) == {"import-physical-windows-qualification"}
    job = jobs["import-physical-windows-qualification"]
    assert job["runs-on"] == "ubuntu-latest"
    assert job["permissions"] == {"contents": "read"}
    checkout = job["steps"][0]
    assert checkout["uses"] == "actions/checkout@v6.0.3"
    assert checkout["with"] == {
        "ref": "${{ inputs.source_sha }}",
        "fetch-depth": "1",
        "persist-credentials": "false",
    }


def test_import_workflow_uses_fixed_env_only_script_and_exact_artifact_files() -> None:
    workflow = load_import_workflow()
    job = workflow["jobs"]["import-physical-windows-qualification"]
    import_step = next(step for step in job["steps"] if step["name"] == "Validate and materialize qualification")
    assert import_step["env"] == {
        "EVIDENCE_BASE64": "${{ inputs.evidence_base64 }}",
        "EVIDENCE_SHA256": "${{ inputs.evidence_sha256 }}",
        "SOURCE_SHA": "${{ inputs.source_sha }}",
        "VERSION": "${{ inputs.version }}",
        "GITHUB_SHA": "${{ github.sha }}",
    }
    assert import_step["run"].strip() == "python3 scripts/import_physical_windows_qualification.py"

    upload = job["steps"][-1]
    assert upload["uses"] == "actions/upload-artifact@v7.0.1"
    assert upload["with"]["name"] == "ao2-physical-windows-qualification"
    assert set(upload["with"]["path"].splitlines()) == {
        "target/physical-windows-qualification/evidence.json",
        "target/physical-windows-qualification/summary.json",
    }
    assert upload["with"]["if-no-files-found"] == "error"
    assert upload["with"]["retention-days"] == "7"


def test_public_release_consumes_only_the_canonical_physical_qualification_bundle() -> None:
    workflow = load_public_release_workflow()
    jobs = workflow["jobs"]
    verification = jobs["verify-physical-windows-qualification"]

    assert verification["outputs"] == {
        "physical_evidence_sha256": "${{ steps.verify.outputs.physical_evidence_sha256 }}",
    }
    download = next(
        step
        for step in verification["steps"]
        if step["name"] == "Download physical Windows qualification result"
    )
    assert download["env"] == {
        "GH_TOKEN": "${{ github.token }}",
        "PHYSICAL_WINDOWS_QUALIFICATION_RUN_ID": "${{ inputs.physical_windows_qualification_run_id }}",
    }
    assert "--name ao2-physical-windows-qualification" in download["run"]
    verify = next(
        step
        for step in verification["steps"]
        if step["name"] == "Validate canonical physical Windows qualification bundle"
    )
    assert verify["id"] == "verify"
    assert verify["env"] == {
        "SOURCE_SHA": "${{ needs.bind-release-plan.outputs.source_sha }}",
        "RELEASE_VERSION": "${{ needs.bind-release-plan.outputs.version }}",
    }
    run = verify["run"]
    assert "python3 scripts/physical_windows_qualification.py validate" in run
    assert "--evidence \"$root/evidence.json\"" in run
    assert "--source-sha \"$SOURCE_SHA\"" in run
    assert "--version \"$RELEASE_VERSION\"" in run
    assert "rglob" not in run
    assert "assert " not in run
    assert 'expected_inventory=$\'evidence.json\\nsummary.json\'' in run
    assert "physical_evidence_sha256" in run
    assert "GITHUB_OUTPUT" in run


def test_public_release_authenticates_producer_run_and_exact_artifact_before_download() -> None:
    workflow = load_public_release_workflow()
    verification = workflow["jobs"]["verify-physical-windows-qualification"]
    metadata = next(
        step
        for step in verification["steps"]
        if step["name"] == "Authenticate producer workflow run and artifact"
    )
    download_index = next(
        index
        for index, step in enumerate(verification["steps"])
        if step["name"] == "Download physical Windows qualification result"
    )
    metadata_index = verification["steps"].index(metadata)

    assert metadata_index < download_index
    assert metadata["env"] == {
        "GH_TOKEN": "${{ github.token }}",
        "RUN_ID": "${{ inputs.physical_windows_qualification_run_id }}",
        "EXPECTED_REPOSITORY": "${{ github.repository }}",
        "EXPECTED_SOURCE_SHA": "${{ needs.bind-release-plan.outputs.source_sha }}",
        "RUN_METADATA_PATH": "target/hosted-release/physical-windows-metadata/run.json",
        "ARTIFACTS_METADATA_PATH": "target/hosted-release/physical-windows-metadata/artifacts.json",
    }
    run = metadata["run"]
    assert "python3 scripts/validate_physical_windows_workflow_run.py validate-run-id" in run
    assert "python3 scripts/validate_physical_windows_workflow_run.py validate-metadata" in run
    assert 'gh api --method GET "repos/$EXPECTED_REPOSITORY/actions/runs/$RUN_ID"' in run
    assert run.count("gh api --method GET") == 2
    assert '"repos/$EXPECTED_REPOSITORY/actions/runs/$RUN_ID/artifacts?per_page=100"' in run
    assert '>"$RUN_METADATA_PATH"' in run
    assert '>"$ARTIFACTS_METADATA_PATH"' in run
    assert "assert " not in run


def test_workflow_run_metadata_validator_accepts_exact_successful_producer() -> None:
    validator = load_run_metadata_script()

    artifact_id = validator.validate_metadata(
        producer_run_metadata(),
        producer_artifact_metadata(),
        "123456789",
        "uesugitorachiyo/ao2",
        SOURCE_SHA,
    )

    assert artifact_id == 24681012


@pytest.mark.parametrize(
    "run_id",
    [
        "",
        "0",
        "0123456789",
        "123abc",
        "-123",
        "1" * 21,
    ],
)
def test_workflow_run_metadata_validator_rejects_unbounded_run_id(run_id: str) -> None:
    validator = load_run_metadata_script()

    with pytest.raises(validator.MetadataValidationError, match="run id"):
        validator.validate_run_id(run_id)


@pytest.mark.parametrize(
    ("mutation", "error"),
    [
        (("id", 123456788), "run id"),
        (("repository.full_name", "other/ao2"), "repository"),
        (("path", ".github/workflows/ci.yml"), "workflow path"),
        (("name", "Other Workflow"), "workflow name"),
        (("event", "push"), "workflow_dispatch"),
        (("status", "in_progress"), "completed"),
        (("conclusion", "failure"), "success"),
        (("head_sha", "b" * 40), "head_sha"),
    ],
)
def test_workflow_run_metadata_validator_rejects_wrong_producer_run(
    mutation: tuple[str, object],
    error: str,
) -> None:
    validator = load_run_metadata_script()
    run = producer_run_metadata()
    field, value = mutation
    if field == "repository.full_name":
        run["repository"]["full_name"] = value
    else:
        run[field] = value

    with pytest.raises(validator.MetadataValidationError, match=error):
        validator.validate_metadata(
            run,
            producer_artifact_metadata(),
            "123456789",
            "uesugitorachiyo/ao2",
            SOURCE_SHA,
        )


@pytest.mark.parametrize(
    ("artifacts", "error"),
    [
        ({"total_count": 0, "artifacts": []}, "total_count"),
        (
            {
                "total_count": 2,
                "artifacts": [
                    producer_artifact_metadata()["artifacts"][0],
                    {
                        **producer_artifact_metadata()["artifacts"][0],
                        "id": 24681013,
                    },
                ],
            },
            "total_count",
        ),
        (
            {
                "total_count": 1,
                "artifacts": [],
            },
            "exactly one",
        ),
        (
            {
                "total_count": 1,
                "artifacts": [
                    {
                        **producer_artifact_metadata()["artifacts"][0],
                        "name": "other-artifact",
                    }
                ],
            },
            "artifact name",
        ),
        (
            {
                "total_count": 1,
                "artifacts": [
                    {
                        **producer_artifact_metadata()["artifacts"][0],
                        "expired": True,
                    }
                ],
            },
            "expired",
        ),
    ],
)
def test_workflow_run_metadata_validator_rejects_wrong_artifact_cardinality_or_state(
    artifacts: dict[str, object],
    error: str,
) -> None:
    validator = load_run_metadata_script()

    with pytest.raises(validator.MetadataValidationError, match=error):
        validator.validate_metadata(
            producer_run_metadata(),
            artifacts,
            "123456789",
            "uesugitorachiyo/ao2",
            SOURCE_SHA,
        )


@pytest.mark.parametrize(
    ("field", "value", "error"),
    [
        ("id", 123456788, "workflow run id"),
        ("head_sha", "b" * 40, "workflow run head_sha"),
        ("repository_id", 1, "repository_id"),
        ("head_repository_id", 1, "head_repository_id"),
    ],
)
def test_workflow_run_metadata_validator_rejects_artifact_run_binding_mismatch(
    field: str,
    value: object,
    error: str,
) -> None:
    validator = load_run_metadata_script()
    artifacts = producer_artifact_metadata()
    artifacts["artifacts"][0]["workflow_run"][field] = value

    with pytest.raises(validator.MetadataValidationError, match=error):
        validator.validate_metadata(
            producer_run_metadata(),
            artifacts,
            "123456789",
            "uesugitorachiyo/ao2",
            SOURCE_SHA,
        )


def test_public_release_promotion_plan_binds_verified_physical_evidence_digest() -> None:
    workflow = load_public_release_workflow()
    plan = workflow["jobs"]["assemble-promotion-plan"]

    assert "verify-physical-windows-qualification" in plan["needs"]
    assemble = next(
        step
        for step in plan["steps"]
        if step["name"] == "Assemble promotion plan and dry-run boundary"
    )
    assert assemble["env"]["PHYSICAL_WINDOWS_EVIDENCE_SHA256"] == (
        "${{ needs.verify-physical-windows-qualification.outputs.physical_evidence_sha256 }}"
    )
    assert "re.fullmatch(r\"[0-9a-f]{64}\", physical_evidence_sha256)" in assemble["run"]
    assert '"physical_windows_evidence_sha256": physical_evidence_sha256' in assemble["run"]
    assert '"physical_windows_evidence_mismatch"' in assemble["run"]
    assert 're.split(r"[\\\\/]", str(summary.get("archive", "")))[-1]' in assemble["run"]


def test_public_release_hosted_guard_builds_exact_source_before_replacement_gate() -> None:
    workflow = load_public_release_workflow()
    guard = workflow["jobs"]["hosted-release-guard"]
    steps = guard["steps"]
    download = next(step for step in steps if step["name"] == "Download native gate candidates")
    validation = next(
        step for step in steps if step["name"] == "Validate exact hosted native candidates"
    )
    build = next(step for step in steps if step["name"] == "Build hosted release gate binary")
    replacement = next(step for step in steps if step["name"] == "Run replacement parity guard")

    assert guard["needs"] == ["bind-release-plan", "native-build"]
    assert guard["permissions"] == {"actions": "read", "contents": "read"}
    assert download["uses"] == "actions/download-artifact@v8.0.1"
    assert download["with"] == {
        "pattern": "ao2-hosted-native-candidate-*-${{ github.sha }}",
        "path": "target/hosted-release/gate-candidates",
        "merge-multiple": "false",
    }
    assert steps.index(download) < steps.index(validation) < steps.index(build) < steps.index(replacement)
    assert build["env"] == {
        "AO2_BUILD_GIT_COMMIT": "${{ needs.bind-release-plan.outputs.source_sha }}",
    }
    assert build["run"] == "cargo build --locked --release -p ao2-cli --bin ao2"


def test_hosted_candidate_validator_accepts_exact_three_platform_contract(tmp_path: Path) -> None:
    validator = load_hosted_candidate_script()
    root = create_hosted_candidate_fixture(tmp_path / "candidates")

    report = validator.validate_candidates(root, SOURCE_SHA, VERSION)

    assert report["schema_version"] == "ao2.hosted-native-candidate-gate.v1"
    assert report["status"] == "passed"
    assert report["source_sha"] == SOURCE_SHA
    assert report["version"] == VERSION
    assert [item["target"] for item in report["artifacts"]] == [
        "linux-x86_64",
        "macos-aarch64",
        "windows-x86_64",
    ]
    assert all(len(item["archive_sha256"]) == 64 for item in report["artifacts"])
    assert report["trust_boundary"] == {
        "mutates_ao_artifacts": False,
        "mutates_releases": False,
        "requires_signing_credentials": False,
        "signed_four_archive_release_gate": "separate_canonical_gate",
    }


@pytest.mark.parametrize(
    ("mutation", "error"),
    [
        ("wrong_source", "git_commit"),
        ("unsafe_summary", "mutates_ao_artifacts"),
        ("missing_target", "target mismatch"),
        ("unexpected_root_file", "candidate root inventory"),
        ("unsafe_tar_member", "unsafe archive member"),
        ("altered_checksum", "checksum mismatch"),
    ],
)
def test_hosted_candidate_validator_rejects_invalid_candidate_contract(
    tmp_path: Path,
    mutation: str,
    error: str,
) -> None:
    validator = load_hosted_candidate_script()
    root = create_hosted_candidate_fixture(tmp_path / "candidates")
    linux = next(root.glob("*linux-x86_64*"))
    if mutation == "unsafe_summary":
        summary_path = linux / "summary.json"
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
        summary["mutates_ao_artifacts"] = True
        summary_path.write_text(json.dumps(summary) + "\n", encoding="utf-8")
    elif mutation == "missing_target":
        macos = next(item for item in root.iterdir() if "macos-aarch64" in item.name)
        for path in sorted(macos.rglob("*"), reverse=True):
            if path.is_file():
                path.unlink()
            else:
                path.rmdir()
        macos.rmdir()
    elif mutation == "unexpected_root_file":
        (root / "unexpected.txt").write_text("unexpected\n", encoding="utf-8")
    else:
        archive = next((linux / "dist").glob("*.tar.gz"))
        with tarfile.open(archive, "r:gz") as source:
            members = [(member, source.extractfile(member).read()) for member in source.getmembers()]
        if mutation == "wrong_source":
            members = [
                (
                    member,
                    (
                        body.replace(SOURCE_SHA.encode(), ("b" * 40).encode())
                        if member.name == "BUILD-PROVENANCE.json"
                        else body
                    ),
                )
                for member, body in members
            ]
        elif mutation == "unsafe_tar_member":
            info = tarfile.TarInfo("../escape")
            info.size = 1
            members.append((info, b"x"))
        elif mutation == "altered_checksum":
            members = [
                (
                    member,
                    re.sub(rb"^[0-9a-f]{64}", b"0" * 64, body, count=1)
                    if member.name == "SHA256SUMS"
                    else body,
                )
                for member, body in members
            ]
        with tarfile.open(archive, "w:gz") as destination:
            for member, body in members:
                member.size = len(body)
                destination.addfile(member, io.BytesIO(body))

    with pytest.raises(validator.CandidateValidationError, match=error):
        validator.validate_candidates(root, SOURCE_SHA, VERSION)


def test_public_release_hosted_guard_uses_three_candidate_gate_not_signed_four_archive_gate() -> None:
    workflow = load_public_release_workflow()
    guard = workflow["jobs"]["hosted-release-guard"]
    steps = guard["steps"]
    validation = next(
        step for step in steps if step["name"] == "Validate exact hosted native candidates"
    )
    replacement = next(step for step in steps if step["name"] == "Run replacement parity guard")

    assert validation["env"] == {
        "RELEASE_VERSION": "${{ needs.bind-release-plan.outputs.version }}",
        "SOURCE_SHA": "${{ needs.bind-release-plan.outputs.source_sha }}",
    }
    assert validation["run"] == (
        "python3 scripts/validate_hosted_release_candidates.py "
        "--root target/hosted-release/gate-candidates "
        "--source-sha \"$SOURCE_SHA\" --version \"$RELEASE_VERSION\" "
        "--out target/hosted-release/native-gate/summary.json"
    )
    assert replacement["run"] == "npm run verify:replacement"
    assert all(step.get("run") != "npm run gate:full" for step in steps)
    assert any(
        step["name"] == "Upload hosted native candidate gate"
        and step["with"]["path"] == "target/hosted-release/native-gate/summary.json"
        for step in steps
    )


def create_hosted_promotion_fixture(tmp_path: Path) -> tuple[Path, Path, str]:
    candidates = create_hosted_candidate_fixture(tmp_path / "candidates")
    validator = load_hosted_candidate_script()
    validated = validator.validate_candidates(candidates, SOURCE_SHA, VERSION)
    plan = {
        "schema_version": "ao2.hosted-release-promotion-plan.v1",
        "status": "passed",
        "version": VERSION,
        "tag": f"v{VERSION}",
        "source_sha": SOURCE_SHA,
        "approved_asset_manifest_sha256": "b" * 64,
        "physical_windows_evidence_sha256": "c" * 64,
        "artifacts": [
            {
                "target": item["target"],
                "runner": item["runner"],
                "target_triple": item["target_triple"],
                "archive": f"target/hosted-release/{item['archive']}",
                "sha256": item["archive_sha256"],
                "canonical_public_archive": True,
            }
            for item in validated["artifacts"]
        ],
        "windows": {
            "canonical_target_triple": "x86_64-pc-windows-msvc",
            "canonical_runner": "windows-latest",
            "linux_mingw_cross_build": {
                "target_triple": "x86_64-pc-windows-gnu",
                "classification": "non_authoritative",
                "canonical_public_windows_archive": False,
            },
        },
        "rejection_policy": [
            "missing_artifact",
            "duplicate_artifact",
            "stale_source_sha",
            "substituted_archive",
            "unexpected_artifact",
            "version_tag_mismatch",
            "approved_manifest_mismatch",
            "physical_windows_evidence_mismatch",
            "incorrect_live_confirmation",
        ],
        "trust_boundary": {
            "build_jobs_mutate_releases": False,
            "plan_job_mutates_releases": False,
            "stores_credentials": False,
            "uses_workflow_scoped_github_token": True,
        },
    }
    plan_root = tmp_path / "plan"
    plan_root.mkdir()
    plan_bytes = json.dumps(plan, indent=2, sort_keys=True).encode()
    (plan_root / "promotion-plan.json").write_bytes(plan_bytes)
    digest = hashlib.sha256(plan_bytes).hexdigest()
    (plan_root / "promotion-plan.sha256").write_text(digest + "\n", encoding="utf-8")
    (plan_root / "dry-run-boundary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.hosted-release-dry-run-boundary.v1",
                "status": "passed",
                "dry_run": True,
                "publication_status": "not_attempted",
                "publication_status: not_attempted": True,
                "tag_creation_attempted": False,
                "tag_creation_attempted: false": True,
                "release_creation_attempted": False,
                "release_creation_attempted: false": True,
                "public_upload_attempted": False,
                "public_upload_attempted: false": True,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return candidates, plan_root, digest


def test_hosted_promotion_stages_exact_frozen_publication(tmp_path: Path) -> None:
    promotion = load_hosted_promotion_script()
    candidates, plan_root, digest = create_hosted_promotion_fixture(tmp_path)

    report = promotion.stage_publication(
        candidates,
        plan_root,
        tmp_path / "publication",
        SOURCE_SHA,
        VERSION,
        f"v{VERSION}",
        "b" * 64,
        digest,
        "c" * 64,
    )

    expected = {
        f"ao2-{VERSION}-linux-x86_64.tar.gz",
        f"ao2-{VERSION}-macos-aarch64.tar.gz",
        f"ao2-{VERSION}-windows-x86_64.tar.gz",
        "promotion-plan.json",
        "SHA256SUMS",
    }
    assert set(report["assets"]) == expected
    assert report["promotion_plan_sha256"] == digest
    assert report["status"] == "passed"
    assert set(path.name for path in (tmp_path / "publication").iterdir()) == expected


@pytest.mark.parametrize(
    ("mutation", "error"),
    [
        ("digest", "promotion plan digest"),
        ("source", "source_sha"),
        ("version", "version"),
        ("archive", "archive digest"),
        ("boundary", "dry-run boundary"),
        ("unexpected_plan_file", "promotion plan inventory"),
    ],
)
def test_hosted_promotion_rejects_substitution_and_unsafe_inputs(
    tmp_path: Path,
    mutation: str,
    error: str,
) -> None:
    promotion = load_hosted_promotion_script()
    candidates, plan_root, digest = create_hosted_promotion_fixture(tmp_path)
    source_sha = SOURCE_SHA
    version = VERSION
    if mutation == "digest":
        digest = "d" * 64
    elif mutation == "source":
        source_sha = "d" * 40
    elif mutation == "version":
        version = "0.5.4"
    elif mutation == "archive":
        archive = next(candidates.rglob("*.tar.gz"))
        archive.write_bytes(archive.read_bytes() + b"altered")
    elif mutation == "boundary":
        boundary_path = plan_root / "dry-run-boundary.json"
        boundary = json.loads(boundary_path.read_text(encoding="utf-8"))
        boundary["dry_run"] = False
        boundary_path.write_text(json.dumps(boundary) + "\n", encoding="utf-8")
    else:
        (plan_root / "unexpected").write_text("unsafe\n", encoding="utf-8")

    with pytest.raises(promotion.PromotionValidationError, match=error):
        promotion.stage_publication(
            candidates,
            plan_root,
            tmp_path / "publication",
            source_sha,
            version,
            f"v{version}",
            "b" * 64,
            digest,
            "c" * 64,
        )


def test_hosted_promotion_authenticates_exact_dry_run_metadata() -> None:
    promotion = load_hosted_promotion_script()
    run = {
        "id": 123456789,
        "event": "workflow_dispatch",
        "status": "completed",
        "conclusion": "success",
        "head_sha": SOURCE_SHA,
        "path": ".github/workflows/public-release-build.yml",
        "repository": {"full_name": "uesugitorachiyo/ao2", "id": 7},
        "head_repository": {"full_name": "uesugitorachiyo/ao2", "id": 7},
    }
    artifact_names = {
        f"ao2-hosted-native-candidate-{target}-{SOURCE_SHA}"
        for target in ("linux-x86_64", "macos-aarch64", "windows-x86_64")
    }
    artifact_names.add(f"ao2-hosted-release-promotion-plan-{SOURCE_SHA}")
    artifacts = {
        "artifacts": [
            {
                "name": name,
                "expired": False,
                "workflow_run": {
                    "id": 123456789,
                    "head_sha": SOURCE_SHA,
                    "repository_id": 7,
                    "head_repository_id": 7,
                },
            }
            for name in sorted(artifact_names)
        ]
    }

    report = promotion.validate_frozen_run(
        run,
        artifacts,
        "123456789",
        "987654321",
        "uesugitorachiyo/ao2",
        SOURCE_SHA,
    )

    assert report["status"] == "passed"
    assert report["artifact_names"] == sorted(artifact_names)


def test_hosted_promotion_rejects_wrong_physical_evidence_binding(tmp_path: Path) -> None:
    promotion = load_hosted_promotion_script()
    candidates, plan_root, digest = create_hosted_promotion_fixture(tmp_path)

    with pytest.raises(
        promotion.PromotionValidationError,
        match="physical Windows evidence digest mismatch",
    ):
        promotion.stage_publication(
            candidates,
            plan_root,
            tmp_path / "publication",
            SOURCE_SHA,
            VERSION,
            f"v{VERSION}",
            "b" * 64,
            digest,
            "d" * 64,
        )


@pytest.mark.parametrize(
    ("field", "value", "error"),
    [
        ("event", "push", "workflow_dispatch"),
        ("conclusion", "failure", "successful"),
        ("head_sha", "d" * 40, "source SHA"),
        ("path", ".github/workflows/other.yml", "workflow path"),
    ],
)
def test_hosted_promotion_rejects_wrong_frozen_run(
    field: str,
    value: object,
    error: str,
) -> None:
    promotion = load_hosted_promotion_script()
    run = {
        "id": 123456789,
        "event": "workflow_dispatch",
        "status": "completed",
        "conclusion": "success",
        "head_sha": SOURCE_SHA,
        "path": ".github/workflows/public-release-build.yml",
        "repository": {"full_name": "uesugitorachiyo/ao2", "id": 7},
        "head_repository": {"full_name": "uesugitorachiyo/ao2", "id": 7},
    }
    run[field] = value
    artifacts = {"artifacts": []}

    with pytest.raises(promotion.PromotionValidationError, match=error):
        promotion.validate_frozen_run(
            run,
            artifacts,
            "123456789",
            "987654321",
            "uesugitorachiyo/ao2",
            SOURCE_SHA,
        )


def test_hosted_public_verification_rebinds_archives_to_plan(tmp_path: Path) -> None:
    promotion = load_hosted_promotion_script()
    candidates, plan_root, digest = create_hosted_promotion_fixture(tmp_path)
    publication = tmp_path / "publication"
    promotion.stage_publication(
        candidates,
        plan_root,
        publication,
        SOURCE_SHA,
        VERSION,
        f"v{VERSION}",
        "b" * 64,
        digest,
        "c" * 64,
    )
    plan_path = publication / "promotion-plan.json"
    plan = json.loads(plan_path.read_text(encoding="utf-8"))
    plan["artifacts"][0]["sha256"] = "d" * 64
    plan_bytes = json.dumps(plan, indent=2, sort_keys=True).encode()
    plan_path.write_bytes(plan_bytes)
    altered_plan_digest = hashlib.sha256(plan_bytes).hexdigest()
    checksum_paths = sorted(
        path for path in publication.iterdir() if path.name != "SHA256SUMS"
    )
    (publication / "SHA256SUMS").write_text(
        "\n".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}"
            for path in checksum_paths
        )
        + "\n",
        encoding="ascii",
    )

    with pytest.raises(promotion.PromotionValidationError, match="archive digest"):
        promotion.verify_publication(
            publication,
            SOURCE_SHA,
            VERSION,
            f"v{VERSION}",
            "b" * 64,
            altered_plan_digest,
            "c" * 64,
        )


def test_hosted_public_verification_accepts_exact_assets(tmp_path: Path) -> None:
    promotion = load_hosted_promotion_script()
    candidates, plan_root, digest = create_hosted_promotion_fixture(tmp_path)
    publication = tmp_path / "publication"
    promotion.stage_publication(
        candidates,
        plan_root,
        publication,
        SOURCE_SHA,
        VERSION,
        f"v{VERSION}",
        "b" * 64,
        digest,
        "c" * 64,
    )

    report = promotion.verify_publication(
        publication,
        SOURCE_SHA,
        VERSION,
        f"v{VERSION}",
        "b" * 64,
        digest,
        "c" * 64,
    )

    assert report["status"] == "passed"
    assert report["promotion_plan_sha256"] == digest


def test_import_script_main_materializes_exact_canonical_artifact(
    tmp_path: Path,
    monkeypatch,
) -> None:
    importer = load_import_script()
    qualification = load_qualification_module()
    repository, environment, payload = create_import_fixture(tmp_path)
    install_import_environment(monkeypatch, environment)
    monkeypatch.chdir(repository)
    monkeypatch.setattr(importer, "_utc_now", lambda: NOW)

    assert importer.main() == 0

    destination = repository / "target" / "physical-windows-qualification"
    assert importer.relative_file_inventory(destination) == ["evidence.json", "summary.json"]
    assert (destination / "evidence.json").read_bytes() == payload
    summary_bytes = (destination / "summary.json").read_bytes()
    summary = json.loads(summary_bytes)
    assert summary_bytes == qualification.canonical_json(summary)
    assert summary["physical_evidence_sha256"] == environment["EVIDENCE_SHA256"]
    assert summary["source_sha"] == environment["SOURCE_SHA"]
    assert summary["version"] == environment["VERSION"]


@pytest.mark.parametrize(
    ("field", "value", "error"),
    [
        ("EVIDENCE_SHA256", "0" * 64, "digest"),
        ("SOURCE_SHA", "not-a-sha", "lowercase 40-character SHA"),
        ("SOURCE_SHA", "b" * 40, "HEAD"),
        ("VERSION", "9.9.9", "version"),
    ],
)
def test_import_script_rejects_bad_bindings_without_partial_artifact(
    tmp_path: Path,
    monkeypatch,
    capsys,
    field: str,
    value: str,
    error: str,
) -> None:
    importer = load_import_script()
    repository, environment, _ = create_import_fixture(tmp_path)
    environment[field] = value
    if field == "SOURCE_SHA":
        environment["GITHUB_SHA"] = value
    install_import_environment(monkeypatch, environment)
    monkeypatch.chdir(repository)
    monkeypatch.setattr(importer, "_utc_now", lambda: NOW)

    assert importer.main() == 2
    assert error in capsys.readouterr().err
    assert not (repository / "target" / "physical-windows-qualification").exists()


def test_import_script_rejects_strict_validation_failure_without_partial_artifact(
    tmp_path: Path,
    monkeypatch,
    capsys,
) -> None:
    importer = load_import_script()
    qualification = load_qualification_module()
    repository, environment, payload = create_import_fixture(tmp_path)
    evidence = json.loads(payload)
    evidence["mode"] = "hosted"
    invalid_payload = qualification.canonical_json(evidence)
    environment["EVIDENCE_BASE64"] = base64.b64encode(invalid_payload).decode("ascii")
    environment["EVIDENCE_SHA256"] = hashlib.sha256(invalid_payload).hexdigest()
    install_import_environment(monkeypatch, environment)
    monkeypatch.chdir(repository)
    monkeypatch.setattr(importer, "_utc_now", lambda: NOW)

    assert importer.main() == 2
    assert "physical_unique" in capsys.readouterr().err
    assert not (repository / "target" / "physical-windows-qualification").exists()


def test_import_script_rejects_preexisting_or_extra_file_inventory(
    tmp_path: Path,
    monkeypatch,
    capsys,
) -> None:
    importer = load_import_script()
    repository, environment, _ = create_import_fixture(tmp_path)
    destination = repository / "target" / "physical-windows-qualification"
    destination.mkdir(parents=True)
    (destination / "unexpected.txt").write_text("preserve\n", encoding="utf-8")
    install_import_environment(monkeypatch, environment)
    monkeypatch.chdir(repository)
    monkeypatch.setattr(importer, "_utc_now", lambda: NOW)

    assert importer.main() == 2
    assert "already exists" in capsys.readouterr().err
    assert (destination / "unexpected.txt").read_text(encoding="utf-8") == "preserve\n"

    inventory_root = tmp_path / "inventory"
    inventory_root.mkdir()
    for name in ("evidence.json", "summary.json", "extra.json"):
        (inventory_root / name).write_text("{}\n", encoding="utf-8")
    with pytest.raises(importer.ArtifactImportError, match="inventory"):
        importer.verify_exact_inventory(inventory_root)


def test_import_script_cleans_staging_after_atomic_write_failure(
    tmp_path: Path,
    monkeypatch,
) -> None:
    importer = load_import_script()
    repository, environment, _ = create_import_fixture(tmp_path)
    writes = 0
    real_atomic_write = importer.atomic_write

    def fail_second_write(path: Path, payload: bytes) -> None:
        nonlocal writes
        writes += 1
        if writes == 2:
            raise OSError("simulated write failure")
        real_atomic_write(path, payload)

    monkeypatch.setattr(importer, "atomic_write", fail_second_write)
    monkeypatch.setattr(importer, "_utc_now", lambda: NOW)

    with pytest.raises(OSError, match="simulated write failure"):
        importer.import_qualification(repository, environment)

    target = repository / "target"
    assert not (target / "physical-windows-qualification").exists()
    assert not list(target.glob(".physical-windows-qualification-*"))


def test_import_script_never_places_payload_in_child_argv(
    tmp_path: Path,
    monkeypatch,
) -> None:
    importer = load_import_script()
    repository, environment, _ = create_import_fixture(tmp_path)
    child_argv: list[list[str]] = []
    real_check_output = importer.subprocess.check_output

    def capture_check_output(argv, **kwargs):
        child_argv.append([str(item) for item in argv])
        return real_check_output(argv, **kwargs)

    monkeypatch.setattr(importer.subprocess, "check_output", capture_check_output)
    monkeypatch.setattr(importer, "_utc_now", lambda: NOW)

    importer.import_qualification(repository, environment)

    flattened = "\n".join(item for argv in child_argv for item in argv)
    assert environment["EVIDENCE_BASE64"] not in flattened
    assert child_argv == [
        ["git", "rev-parse", "HEAD"],
        [str(repository / "scripts" / "current-version.sh")],
    ]


def test_import_workflow_rejects_mutation_and_arbitrary_execution_primitives() -> None:
    text = IMPORT_WORKFLOW_PATH.read_text(encoding="utf-8").lower()

    for forbidden in [
        "self-hosted",
        "contents: write",
        "actions: write",
        "pull-requests: write",
        "id-token: write",
        "gh_token",
        "secrets.",
        "actions/create-release",
        "softprops/action-gh-release",
        "gh release",
        "git tag",
        "environment:",
        "deployment",
        "publication",
        "workflow_call",
        "pull_request:",
        "push:",
        "schedule:",
    ]:
        assert forbidden not in text
