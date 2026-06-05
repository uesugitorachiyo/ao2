import json
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "prepare_phase1_promotion_prerequisites.py"


def write_json(path, payload):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def governed_run_payload(os_label):
    return {
        "schema_version": "ao2.factory-v3-compat-governed-run.v1",
        "status": "accepted",
        "host_os": os_label,
        "plan": {"ao2_native_plan": {}},
        "governed_run_checklist": {},
        "ao2_decision_owner": "ao2-native-governed-run",
    }


def provider_acceptance_payload(provider):
    return {
        "schema_version": f"ao2.{provider}-provider-pilot-acceptance.v1",
        "provider": provider,
        "status": "passed",
        "source_class": "live",
        "run_id": f"live-{provider}",
        "smoke": {"score": 100, "minimum_score": 90},
        "replay": {"status": "accepted", "digest_failures": []},
    }


def seed_complete_prerequisites(tmp_path):
    governed_root = tmp_path / "three-os-real-runspec-evidence"
    for os_label in ("macos", "ubuntu", "windows"):
        write_json(
            governed_root / "20260530T010101Z" / os_label / "governed-run.json",
            governed_run_payload(os_label),
        )

    project_root = tmp_path / "project-runs"
    for os_label in ("macos", "ubuntu", "windows"):
        write_json(
            project_root
            / f"{os_label}-latest"
            / "factory-project-run-summary.json",
            {
                "schema_version": "ao2.factory-project-run-summary.v1",
                "status": "accepted",
                "host_os": os_label,
            },
        )

    provider_root = tmp_path / "provider-pilot-acceptance"
    for provider in ("codex", "claude", "antigravity"):
        write_json(
            provider_root / "v0.4.80" / provider / "provider-pilot-acceptance.json",
            provider_acceptance_payload(provider),
        )

    return governed_root, project_root, provider_root


def run_prepare(tmp_path, *extra_args):
    governed_root, project_root, provider_root = seed_complete_prerequisites(tmp_path)
    out_root = tmp_path / "prepared"
    cmd = [
        sys.executable,
        str(SCRIPT),
        "--governed-run-root",
        str(governed_root),
        "--project-run-root",
        str(project_root),
        "--provider-acceptance-root",
        str(provider_root),
        "--provider-acceptance-tag",
        "v0.4.80",
        "--out-root",
        str(out_root),
        "--json",
        *extra_args,
    ]
    return subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def test_prepare_phase1_prerequisites_materializes_manifest_env_and_normalized_inputs(tmp_path):
    result = run_prepare(tmp_path)

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["schema_version"] == "ao2.phase1-promotion-prerequisites.v1"
    assert report["status"] == "passed"
    assert report["trust_boundary"]["control_plane_role"] == "read_only_observer"
    assert report["trust_boundary"]["mutates_ao_artifacts"] is False

    manifest = Path(report["manifest"])
    env_file = Path(report["env_file"])
    provider_summary = Path(report["provider_acceptance_preservation"])
    assert manifest.exists()
    assert env_file.exists()
    assert provider_summary.exists()

    env_text = env_file.read_text(encoding="utf-8")
    assert "AO2_MACOS_GOVERNED_RUN_EVIDENCE=" in env_text
    assert "AO2_UBUNTU_GOVERNED_RUN_EVIDENCE=" in env_text
    assert "AO2_WINDOWS_GOVERNED_RUN_EVIDENCE=" in env_text
    assert "AO2_PROVIDER_ACCEPTANCE_PRESERVATION=" in env_text

    materialized = json.loads(manifest.read_text(encoding="utf-8"))
    macos_governed = Path(materialized["governed_run_evidence"]["macos"])
    governed_payload = json.loads(macos_governed.read_text(encoding="utf-8"))
    assert (
        governed_payload["plan"]["ao2_native_plan"]["role_contract_discovery"]["mode"]
        == "auto_discovered_from_ao_runspec_layout"
    )
    assert (
        governed_payload["plan"]["ao2_native_plan"]["role_contract_discovery"]["loaded_count"]
        >= 1
    )
    assert governed_payload["governed_run_checklist"]["ao2_auto_loaded_role_contracts"] is True
    assert Path(materialized["factory_project_run_summary"]["windows"]).exists()


def test_prepare_phase1_prerequisites_fails_closed_when_platform_evidence_is_missing(tmp_path):
    governed_root, project_root, provider_root = seed_complete_prerequisites(tmp_path)
    (governed_root / "20260530T010101Z" / "windows" / "governed-run.json").unlink()

    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--governed-run-root",
            str(governed_root),
            "--project-run-root",
            str(project_root),
            "--provider-acceptance-root",
            str(provider_root),
            "--provider-acceptance-tag",
            "v0.4.80",
            "--out-root",
            str(tmp_path / "prepared"),
            "--json",
        ],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    assert result.returncode == 1
    report = json.loads(result.stdout)
    assert report["status"] == "failed"
    assert "windows_governed_run_evidence_missing" in report["failures"]


def test_package_json_exposes_phase1_prerequisite_preparation_command():
    package_json = json.loads((REPO_ROOT / "package.json").read_text(encoding="utf-8"))

    assert (
        package_json["scripts"]["phase1:prepare-prerequisites"]
        == "python3 scripts/prepare_phase1_promotion_prerequisites.py"
    )


def test_prepare_phase1_prerequisites_accepts_os_labeled_restored_governed_run_names(tmp_path):
    governed_root, project_root, provider_root = seed_complete_prerequisites(tmp_path)
    for os_label in ("macos", "ubuntu", "windows"):
        source = governed_root / "20260530T010101Z" / os_label / "governed-run.json"
        destination = source.with_name(f"{os_label}-governed-run.json")
        source.rename(destination)

    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--governed-run-root",
            str(governed_root),
            "--project-run-root",
            str(project_root),
            "--provider-acceptance-root",
            str(provider_root),
            "--provider-acceptance-tag",
            "v0.4.80",
            "--out-root",
            str(tmp_path / "prepared"),
            "--json",
        ],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["status"] == "passed"


def test_prepare_phase1_prerequisites_accepts_os_labeled_project_summary_names(tmp_path):
    governed_root, project_root, provider_root = seed_complete_prerequisites(tmp_path)
    for os_label in ("macos", "ubuntu", "windows"):
        source = project_root / f"{os_label}-latest" / "factory-project-run-summary.json"
        destination = source.with_name(f"{os_label}-factory-project-run-summary.json")
        source.rename(destination)

    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--governed-run-root",
            str(governed_root),
            "--project-run-root",
            str(project_root),
            "--provider-acceptance-root",
            str(provider_root),
            "--provider-acceptance-tag",
            "v0.4.80",
            "--out-root",
            str(tmp_path / "prepared"),
            "--json",
        ],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["status"] == "passed"
