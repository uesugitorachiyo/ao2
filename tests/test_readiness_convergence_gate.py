import json
import os
import subprocess
from pathlib import Path
from typing import Optional


REPO_ROOT = Path(__file__).resolve().parents[1]


COMPONENT_ENV = {
    "AO2_READINESS_CONVERGENCE_RISKY_PR_PRODUCT_READINESS": (
        "risky_pr_product_readiness",
        "ao2.risky-pr-product-readiness-gate.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_RELEASE_EVIDENCE_CLOSURE": (
        "release_evidence_closure",
        "ao2.release-evidence-closure.v1",
        "accepted",
    ),
    "AO2_READINESS_CONVERGENCE_RELEASE_READINESS_STATIC": (
        "release_readiness_static",
        "ao2.release-readiness-local.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_RELEASE_READINESS_REGRESSION": (
        "release_readiness_regression",
        "ao2.release-readiness-regression-gate.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_RELEASE_ASSET_PUBLICATION_READINESS": (
        "release_asset_publication_readiness",
        "ao2.release-asset-publication-readiness.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_PUBLIC_SHIP_DRY_RUN": (
        "public_ship_dry_run",
        "ao2.public-ship-dry-run.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_RELEASE_CUTOVER_LOCK": (
        "release_cutover_readiness_lock",
        "ao2.release-cutover-readiness-lock.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_PULSE_TERMINAL_SCHEMA": (
        "pulse_terminal_eval_loop_schema_compatibility",
        "ao2.pulse-terminal-eval-loop-schema-compatibility.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_PULSE_AUTO_ADVANCE": (
        "pulse_auto_advance_integration_gate",
        "ao2.pulse-auto-advance-integration-gate.v1",
        "passed",
    ),
    "AO2_READINESS_CONVERGENCE_PULSE_RESUME": (
        "pulse_resume_dry_run",
        "ao2.pulse-resume.v1",
        "dry_run",
    ),
    "AO2_READINESS_CONVERGENCE_PULSE_DAEMON": (
        "pulse_daemon_status",
        "ao2.pulse-daemon.v1",
        "stopped",
    ),
}


def write_summary(path: Path, schema_version: str, status: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {
                "schema_version": schema_version,
                "status": status,
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "mutates_release": False,
                    "control_plane_approves_release": False,
                },
                "publish_guards": {
                    "tag_push_publish_deploy": "not executed",
                    "release_publish": "not executed",
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )


def run_gate(
    tmp_path: Path, failed_component: Optional[str] = None
) -> subprocess.CompletedProcess:
    evidence = tmp_path / "evidence"
    out = tmp_path / "out" / "latest"
    env = os.environ.copy()
    env["AO2_READINESS_CONVERGENCE_ROOT"] = str(out)

    for env_name, (component_id, schema_version, status) in COMPONENT_ENV.items():
        summary = evidence / component_id / "summary.json"
        component_status = "failed" if component_id == failed_component else status
        write_summary(summary, schema_version, component_status)
        env[env_name] = str(summary)

    return subprocess.run(
        ["npm", "run", "readiness:convergence"],
        cwd=REPO_ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )


def test_readiness_convergence_script_is_registered_and_operational():
    package = json.loads((REPO_ROOT / "package.json").read_text(encoding="utf-8"))
    assert (
        package["scripts"]["readiness:convergence"]
        == "node scripts/run-sh-script.js scripts/readiness-convergence-gate.sh"
    )

    script = (REPO_ROOT / "scripts/readiness-convergence-gate.sh").read_text(
        encoding="utf-8"
    )
    for needle in [
        "ao2.readiness-convergence-gate.v1",
        "operator_release_decision_required",
        "full_autonomous_self_mutating_rsi",
        "bounded_governed_rsi",
        "continue_pulse_loop",
    ]:
        assert needle in script


def test_readiness_convergence_gate_stops_loop_when_all_evidence_is_green(tmp_path):
    result = run_gate(tmp_path)
    assert result.returncode == 0, result.stderr + result.stdout

    summary_path = tmp_path / "out" / "latest" / "summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

    assert summary["schema_version"] == "ao2.readiness-convergence-gate.v1"
    assert summary["status"] == "passed"
    assert summary["readiness_converged"] is True
    assert summary["continue_pulse_loop"] is False
    assert summary["recommended_next_action"] == "operator_release_decision_required"
    assert summary["rsi_claim_boundary"]["bounded_governed_rsi"] == "supported"
    assert summary["rsi_claim_boundary"]["full_autonomous_self_mutating_rsi"] == "denied"
    assert summary["rsi_claim_boundary"]["claim_publish_authority"] is False
    assert len(summary["components"]) == len(COMPONENT_ENV)
    assert {component["status"] for component in summary["components"]} == {"passed"}

    report = (tmp_path / "out" / "latest" / "report.md").read_text(encoding="utf-8")
    assert "operator_release_decision_required" in report
    assert "full_autonomous_self_mutating_rsi: denied" in report


def test_readiness_convergence_gate_keeps_loop_in_repair_mode_on_failed_evidence(tmp_path):
    result = run_gate(tmp_path, failed_component="release_readiness_regression")
    assert result.returncode != 0

    summary = json.loads(
        (tmp_path / "out" / "latest" / "summary.json").read_text(encoding="utf-8")
    )
    assert summary["status"] == "failed"
    assert summary["readiness_converged"] is False
    assert summary["continue_pulse_loop"] is True
    assert summary["recommended_next_action"] == "repair_readiness_evidence"
    assert summary["blocking_next_actions"]
    assert any(
        blocker["component_id"] == "release_readiness_regression"
        for blocker in summary["blocking_next_actions"]
    )
