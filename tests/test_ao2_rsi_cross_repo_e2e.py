import json
import os
import stat
import subprocess
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")


def test_rsi_improvement_evidence_gate_measures_five_percent_hardening(tmp_path):
    package = json.loads(read("package.json"))
    assert package["scripts"]["rsi:improvement-evidence-gate"] == (
        "node scripts/run-sh-script.js scripts/rsi-improvement-evidence-gate.sh"
    )

    script = REPO / "scripts" / "rsi-improvement-evidence-gate.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.rsi-improvement-evidence-gate.v1",
        "AO2_RSI_IMPROVEMENT_TARGET_PERCENT",
        "AO2_RSI_IMPROVEMENT_BASELINE_CHECK_COUNT",
        "measured_improvement_percent",
        "claim_publish_decision",
        "claim_publish_authority",
        "publishes_claims",
        "approves_rsi_claims",
    ]:
        assert needle in text

    evidence = tmp_path / "evidence"
    write_json(
        evidence / "live-self-change-rehearsal" / "summary.json",
        {
            "schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
            "status": "live_rehearsal_passed",
            "self_change": {"repository_restored": True},
        },
    )
    write_json(
        evidence / "control-plane-readback" / "summary.json",
        {
            "schema_version": "ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1",
            "status": "passed",
        },
    )
    write_json(
        evidence / "readback-index" / "summary.json",
        {
            "schema_version": "ao2.rsi-live-self-change-readback-evidence-index.v1",
            "status": "passed",
        },
    )
    write_json(
        evidence / "claim-readiness" / "summary.json",
        {
            "schema_version": "ao2.rsi-claim-readiness-audit.v1",
            "status": "claim_boundary_enforced",
        },
    )
    write_json(
        evidence / "covenant-gate" / "summary.json",
        {
            "schema_version": "covenant.rsi-claim-publish-gate.v1",
            "status": "denied",
            "decision": "deny",
            "publish_authority": False,
        },
    )
    schema_exit = evidence / "logs" / "covenant_gate_schema_validate.log.exit-code"
    schema_exit.parent.mkdir(parents=True)
    schema_exit.write_text("0\n", encoding="utf-8")

    out_root = tmp_path / "out"
    result = subprocess.run(
        ["npm", "run", "rsi:improvement-evidence-gate"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT": str(out_root),
            "AO2_RSI_IMPROVEMENT_LIVE_SUMMARY": str(
                evidence / "live-self-change-rehearsal" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_READBACK_SUMMARY": str(
                evidence / "control-plane-readback" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_READBACK_INDEX_SUMMARY": str(
                evidence / "readback-index" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_CLAIM_READINESS_SUMMARY": str(
                evidence / "claim-readiness" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY": str(
                evidence / "covenant-gate" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE": str(schema_exit),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.rsi-improvement-evidence-gate.v1"
    assert summary["status"] == "passed"
    assert summary["improvement_ready"] is True
    assert summary["metric"]["unit"] == "enforced_rsi_evidence_checks"
    assert summary["metric"]["baseline_check_count"] == 6
    assert summary["metric"]["observed_check_count"] == 7
    assert summary["metric"]["target_percent"] == 5.0
    assert summary["metric"]["measured_improvement_percent"] >= 5.0
    assert summary["claim_publish_decision"] == "deny"
    assert summary["claim_publish_authority"] is False
    assert summary["trust_boundary"]["publishes_claims"] is False
    assert summary["trust_boundary"]["approves_rsi_claims"] is False

    blocked = subprocess.run(
        ["npm", "run", "rsi:improvement-evidence-gate"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT": str(tmp_path / "blocked"),
            "AO2_RSI_IMPROVEMENT_BASELINE_CHECK_COUNT": "7",
            "AO2_RSI_IMPROVEMENT_LIVE_SUMMARY": str(
                evidence / "live-self-change-rehearsal" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_READBACK_SUMMARY": str(
                evidence / "control-plane-readback" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_READBACK_INDEX_SUMMARY": str(
                evidence / "readback-index" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_CLAIM_READINESS_SUMMARY": str(
                evidence / "claim-readiness" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY": str(
                evidence / "covenant-gate" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE": str(schema_exit),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert blocked.returncode != 0
    assert "rsi_improvement_evidence_gate=failed" in blocked.stdout


def test_rsi_cross_repo_e2e_contract():
    package = json.loads(read("package.json"))
    assert package["scripts"]["rsi:cross-repo-e2e"] == (
        "node scripts/run-sh-script.js scripts/rsi-cross-repo-e2e.sh"
    )

    readme = read("README.md")
    for needle in [
        "npm run rsi:cross-repo-e2e",
        "ao2.rsi-cross-repo-e2e.v1",
        "target/rsi-cross-repo-e2e/latest/summary.json",
        "ao2.rsi-improvement-evidence-gate.v1",
        "measured_improvement_percent",
        "covenant.rsi-claim-publish-gate.v1",
        "publish_authority=false",
    ]:
        assert needle in readme

    script = REPO / "scripts" / "rsi-cross-repo-e2e.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.rsi-cross-repo-e2e.v1",
        "AO2_CONTROL_PLANE_REPO",
        "AO_COVENANT_REPO",
        'CP_ROOT="$(cd "$CP_ROOT" && pwd)"',
        'COVENANT_ROOT="$(cd "$COVENANT_ROOT" && pwd)"',
        'OUT_PARENT="$(cd "$OUT_PARENT" && pwd)"',
        "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1",
        "rsi:live-self-change-rehearsal",
        "verify_ao2_rsi_live_self_change_rehearsal.py",
        "rsi:live-self-change-readback-index",
        "rsi:claim-readiness",
        "rsi:improvement-evidence-gate",
        "improvement_evidence_gate",
        "ao2.rsi-improvement-evidence-gate.v1",
        "measured_improvement_percent",
        "policy claim-publish-gate",
        "covenant.rsi-claim-publish-gate.v1",
        "publishes_claims",
        "approves_rsi_claims",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text


def test_rsi_cross_repo_e2e_ci_artifact_job_contract():
    ci = read(".github/workflows/ci.yml")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "rsi-cross-repo-e2e-artifacts:",
        "name: RSI cross-repo E2E artifacts",
        "repository: uesugitorachiyo/ao2-control-plane",
        "repository: uesugitorachiyo/ao-covenant",
        "go-version: '1.26.x'",
        "cache-dependency-path: ao-covenant/go.sum",
        "AO2_CONTROL_PLANE_REPO=ao2-control-plane",
        "AO_COVENANT_REPO=ao-covenant",
        "AO2_RSI_CROSS_REPO_E2E_ROOT=target/rsi-cross-repo-e2e-ci/latest",
        "npm run rsi:cross-repo-e2e",
        "ao2.rsi-cross-repo-e2e.v1",
        "ao2.rsi-improvement-evidence-gate.v1",
        '"measured_improvement_percent"] >= 5.0',
        "covenant.rsi-claim-publish-gate.v1",
        '"claim_publish_decision"] == "deny"',
        '"claim_publish_authority"] is False',
        "name: ao2-rsi-cross-repo-e2e",
        "target/rsi-cross-repo-e2e-ci",
        "uses: actions/upload-artifact@v7.0.1",
    ]:
        assert needle in ci

    for needle in [
        "npm run rsi:cross-repo-e2e",
        "ao2.rsi-cross-repo-e2e.v1",
        "target/rsi-cross-repo-e2e/latest/summary.json",
        "ao2-rsi-cross-repo-e2e",
        "ao2.rsi-improvement-evidence-gate.v1",
        "measured_improvement_percent",
        "claim_publish_decision=deny",
        "publish_authority=false",
    ]:
        assert needle in verification
