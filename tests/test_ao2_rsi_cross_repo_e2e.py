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


def valid_blueprint_authorization() -> dict:
    return {
        "schema": "ao.blueprint.build-authorization.v0.1",
        "project_id": "ao2-rsi-tiered-gate",
        "status": "ready",
        "score": 100,
        "approved_by_user": True,
        "blocking_assumptions": [],
        "next_allowed_action": "ao-foundry",
        "authorization_scope": {
            "domain": "rsi",
            "gate_model": "tiered",
            "candidate_id": "ao2-rsi-evidence-hardening",
            "requires_new_blueprint_for": [
                "new_product_direction",
                "new_architecture",
                "new_repo_or_component",
                "new_public_claim",
                "new_policy_or_authority_surface",
                "ao_blueprint_self_change",
                "production_readiness_definition_change",
                "safety_privacy_secrets_release_or_promotion_change",
            ],
        },
        "authority_boundary": {
            "source": "ao-blueprint",
            "downstream_of_operator_intent": True,
            "self_authorized_by_rsi": False,
            "authorizes_implementation": True,
            "authorizes_claim_publication": False,
            "authorizes_ao_blueprint_self_change": False,
        },
    }


def test_rsi_blueprint_authorization_gate_requires_tiered_gate_authorization(tmp_path):
    package = json.loads(read("package.json"))
    assert package["scripts"]["rsi:blueprint-authorization-gate"] == (
        "node scripts/run-sh-script.js scripts/rsi-blueprint-authorization-gate.sh"
    )

    script = REPO / "scripts" / "rsi-blueprint-authorization-gate.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.rsi-blueprint-authorization-gate.v1",
        "ao.blueprint.build-authorization.v0.1",
        "score",
        "approved_by_user",
        "authorization_scope",
        "gate_model",
        "tiered",
        "self_authorized_by_rsi",
        "authorizes_ao_blueprint_self_change",
        "authorizes_claim_publication",
    ]:
        assert needle in text

    authorization = tmp_path / "blueprint" / "build-authorization.json"
    write_json(authorization, valid_blueprint_authorization())

    out_root = tmp_path / "gate"
    result = subprocess.run(
        ["npm", "run", "rsi:blueprint-authorization-gate"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT": str(out_root),
            "AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY": str(authorization),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.rsi-blueprint-authorization-gate.v1"
    assert summary["status"] == "passed"
    assert summary["blueprint_authorization_ready"] is True
    assert summary["source_authorization"]["schema"] == "ao.blueprint.build-authorization.v0.1"
    assert summary["source_authorization"]["score"] == 100
    assert summary["authorization_scope"]["gate_model"] == "tiered"
    assert summary["authority_boundary"]["source"] == "ao-blueprint"
    assert summary["authority_boundary"]["self_authorized_by_rsi"] is False
    assert summary["authority_boundary"]["authorizes_claim_publication"] is False
    assert summary["authority_boundary"]["authorizes_ao_blueprint_self_change"] is False

    blocked_payload = valid_blueprint_authorization()
    blocked_payload["authority_boundary"]["self_authorized_by_rsi"] = True
    blocked_authorization = tmp_path / "blocked" / "build-authorization.json"
    write_json(blocked_authorization, blocked_payload)
    blocked = subprocess.run(
        ["npm", "run", "rsi:blueprint-authorization-gate"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT": str(tmp_path / "blocked-gate"),
            "AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY": str(blocked_authorization),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert blocked.returncode != 0
    assert "blocker=blueprint_self_authorized_by_rsi" in blocked.stderr


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
        "AO2_RSI_IMPROVEMENT_BLUEPRINT_AUTHORIZATION_SUMMARY",
        "blueprint_authorization",
        "ao2.rsi-blueprint-authorization-gate.v1",
        "AO2_RSI_IMPROVEMENT_RELEASE_READINESS_DASHBOARD_READBACK_SUMMARY",
        "release_readiness_dashboard_readback",
        "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
        "dashboard_link_ready",
        "control_surface_readback",
        "bounded_governed_rsi",
        "target_exceeded",
        "workflow_hardening_coverage_not_publication_authority",
    ]:
        assert needle in text

    evidence = tmp_path / "evidence"
    write_json(
        evidence / "blueprint-authorization" / "summary.json",
        {
            "schema_version": "ao2.rsi-blueprint-authorization-gate.v1",
            "status": "passed",
            "blueprint_authorization_ready": True,
            "authorization_scope": {
                "domain": "rsi",
                "gate_model": "tiered",
                "candidate_id": "ao2-rsi-evidence-hardening",
            },
            "authority_boundary": {
                "source": "ao-blueprint",
                "downstream_of_operator_intent": True,
                "self_authorized_by_rsi": False,
                "authorizes_implementation": True,
                "authorizes_claim_publication": False,
                "authorizes_ao_blueprint_self_change": False,
            },
        },
    )
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
    write_json(
        evidence / "release-readiness-dashboard-readback" / "summary.json",
        {
            "schema_version": "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
            "status": "passed",
            "dashboard_link_ready": True,
            "dashboard_artifact": "ao2-release-readiness-consumer/dashboard.html",
            "dashboard_schema_version": "ao2.release-readiness-artifact-consumer.v1",
            "claim_publish_decision": "deny",
            "claim_publish_authority": False,
            "control_plane_approves_release": False,
            "mutates_ao_artifacts": False,
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
            "AO2_RSI_IMPROVEMENT_BLUEPRINT_AUTHORIZATION_SUMMARY": str(
                evidence / "blueprint-authorization" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY": str(
                evidence / "covenant-gate" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_RELEASE_READINESS_DASHBOARD_READBACK_SUMMARY": str(
                evidence / "release-readiness-dashboard-readback" / "summary.json"
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
    assert summary["metric"]["observed_check_count"] == 9
    assert summary["metric"]["target_percent"] == 5.0
    assert summary["metric"]["measured_improvement_percent"] >= 5.0
    assert summary["release_readiness_dashboard_readback"] == {
        "schema_version": "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
        "status": "passed",
        "dashboard_link_ready": True,
        "dashboard_artifact": "ao2-release-readiness-consumer/dashboard.html",
        "dashboard_schema_version": "ao2.release-readiness-artifact-consumer.v1",
        "claim_publish_decision": "deny",
        "claim_publish_authority": False,
        "control_plane_approves_release": False,
        "mutates_ao_artifacts": False,
    }
    assert summary["blueprint_authorization"]["schema_version"] == (
        "ao2.rsi-blueprint-authorization-gate.v1"
    )
    assert summary["blueprint_authorization"]["gate_model"] == "tiered"
    assert summary["blueprint_authorization"]["self_authorized_by_rsi"] is False
    assert summary["claim_publish_decision"] == "deny"
    assert summary["claim_publish_authority"] is False
    assert summary["control_surface_readback"] == {
        "loop_goal": "bounded_governed_rsi_control_surface_readback",
        "bounded_governed_rsi": {
            "status": "supported",
            "evidence_state": "passing",
            "improvement_state": "target_exceeded",
        },
        "full_autonomous_self_mutating_rsi": {
            "status": "denied",
            "decision": "deny",
            "publish_authority": False,
            "boundary_state": "enforced_by_design",
        },
        "improvement_score": {
            "target_exceeded": True,
            "interpretation": "workflow_hardening_coverage_not_publication_authority",
        },
    }
    assert summary["trust_boundary"]["publishes_claims"] is False
    assert summary["trust_boundary"]["approves_rsi_claims"] is False
    assert "bounded_governed_rsi=supported evidence_state=passing" in result.stdout
    assert (
        "improvement_score=target_exceeded interpretation="
        "workflow_hardening_coverage_not_publication_authority"
    ) in result.stdout

    blocked = subprocess.run(
        ["npm", "run", "rsi:improvement-evidence-gate"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT": str(tmp_path / "blocked"),
            "AO2_RSI_IMPROVEMENT_BASELINE_CHECK_COUNT": "9",
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
            "AO2_RSI_IMPROVEMENT_BLUEPRINT_AUTHORIZATION_SUMMARY": str(
                evidence / "blueprint-authorization" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY": str(
                evidence / "covenant-gate" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_RELEASE_READINESS_DASHBOARD_READBACK_SUMMARY": str(
                evidence / "release-readiness-dashboard-readback" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE": str(schema_exit),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert blocked.returncode != 0
    assert "rsi_improvement_evidence_gate=failed" in blocked.stdout

    missing_blueprint = subprocess.run(
        ["npm", "run", "rsi:improvement-evidence-gate"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT": str(
                tmp_path / "missing-blueprint"
            ),
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
            "AO2_RSI_IMPROVEMENT_BLUEPRINT_AUTHORIZATION_SUMMARY": str(
                evidence / "missing-blueprint" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY": str(
                evidence / "covenant-gate" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_RELEASE_READINESS_DASHBOARD_READBACK_SUMMARY": str(
                evidence / "release-readiness-dashboard-readback" / "summary.json"
            ),
            "AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE": str(schema_exit),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert missing_blueprint.returncode != 0
    assert "blocker=evidence_check_failed" in missing_blueprint.stderr


def test_rsi_improvement_trend_persists_history_across_runs(tmp_path):
    package = json.loads(read("package.json"))
    assert package["scripts"]["rsi:improvement-trend"] == (
        "node scripts/run-sh-script.js scripts/rsi-improvement-trend.sh"
    )

    script = REPO / "scripts" / "rsi-improvement-trend.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.rsi-improvement-trend.v1",
        "AO2_RSI_IMPROVEMENT_TREND_HISTORY",
        "AO2_RSI_IMPROVEMENT_TREND_CURRENT_SUMMARY",
        "delta_from_previous_percent",
        "claim_publish_decision",
        "claim_publish_authority",
        "publishes_claims",
        "approves_rsi_claims",
        "control_surface_readback",
        "bounded_governed_rsi",
        "target_exceeded",
        "workflow_hardening_coverage_not_publication_authority",
    ]:
        assert needle in text

    current = tmp_path / "current" / "summary.json"
    history = tmp_path / "history" / "trend.jsonl"

    write_json(
        current,
        {
            "schema_version": "ao2.rsi-improvement-evidence-gate.v1",
            "status": "passed",
            "improvement_ready": True,
            "claim_level": "full_autonomous_self_mutating_rsi",
            "claim_publish_decision": "deny",
            "claim_publish_authority": False,
            "control_surface_readback": {
                "loop_goal": "bounded_governed_rsi_control_surface_readback",
                "bounded_governed_rsi": {
                    "status": "supported",
                    "evidence_state": "passing",
                    "improvement_state": "target_exceeded",
                },
                "full_autonomous_self_mutating_rsi": {
                    "status": "denied",
                    "decision": "deny",
                    "publish_authority": False,
                    "boundary_state": "enforced_by_design",
                },
                "improvement_score": {
                    "target_exceeded": True,
                    "interpretation": (
                        "workflow_hardening_coverage_not_publication_authority"
                    ),
                },
            },
            "metric": {
                "unit": "enforced_rsi_evidence_checks",
                "baseline_check_count": 6,
                "observed_check_count": 8,
                "target_percent": 5.0,
                "measured_improvement_percent": 33.3333,
            },
            "trust_boundary": {
                "publishes_claims": False,
                "approves_rsi_claims": False,
            },
        },
    )

    first_out = tmp_path / "first"
    first = subprocess.run(
        ["npm", "run", "rsi:improvement-trend"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_IMPROVEMENT_TREND_ROOT": str(first_out),
            "AO2_RSI_IMPROVEMENT_TREND_CURRENT_SUMMARY": str(current),
            "AO2_RSI_IMPROVEMENT_TREND_HISTORY": str(history),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert first.returncode == 0, first.stdout + first.stderr
    first_summary = json.loads((first_out / "summary.json").read_text(encoding="utf-8"))
    assert first_summary["schema_version"] == "ao2.rsi-improvement-trend.v1"
    assert first_summary["status"] == "passed"
    assert first_summary["trend_ready"] is True
    assert first_summary["run_count"] == 1
    assert first_summary["previous_measured_improvement_percent"] is None
    assert first_summary["current_measured_improvement_percent"] == 33.3333
    assert first_summary["delta_from_previous_percent"] is None
    assert first_summary["claim_publish_decision"] == "deny"
    assert first_summary["claim_publish_authority"] is False
    assert first_summary["control_surface_readback"] == {
        "loop_goal": "bounded_governed_rsi_control_surface_readback",
        "bounded_governed_rsi": {
            "status": "supported",
            "evidence_state": "passing",
            "improvement_state": "target_exceeded",
        },
        "full_autonomous_self_mutating_rsi": {
            "status": "denied",
            "decision": "deny",
            "publish_authority": False,
            "boundary_state": "enforced_by_design",
        },
        "improvement_score": {
            "target_exceeded": True,
            "interpretation": "workflow_hardening_coverage_not_publication_authority",
        },
    }
    assert first_summary["trust_boundary"]["publishes_claims"] is False
    assert first_summary["trust_boundary"]["approves_rsi_claims"] is False
    assert "bounded_governed_rsi=supported evidence_state=passing" in first.stdout
    assert (
        "full_autonomous_self_mutating_rsi=denied boundary_state=enforced_by_design"
        in first.stdout
    )
    assert history.read_text(encoding="utf-8").count("\n") == 1

    current_payload = json.loads(current.read_text(encoding="utf-8"))
    current_payload["metric"]["observed_check_count"] = 9
    current_payload["metric"]["measured_improvement_percent"] = 50.0
    write_json(current, current_payload)

    second_out = tmp_path / "second"
    second = subprocess.run(
        ["npm", "run", "rsi:improvement-trend"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_IMPROVEMENT_TREND_ROOT": str(second_out),
            "AO2_RSI_IMPROVEMENT_TREND_CURRENT_SUMMARY": str(current),
            "AO2_RSI_IMPROVEMENT_TREND_HISTORY": str(history),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert second.returncode == 0, second.stdout + second.stderr
    second_summary = json.loads((second_out / "summary.json").read_text(encoding="utf-8"))
    assert second_summary["run_count"] == 2
    assert second_summary["previous_measured_improvement_percent"] == 33.3333
    assert second_summary["current_measured_improvement_percent"] == 50.0
    assert second_summary["delta_from_previous_percent"] == 16.6667
    assert history.read_text(encoding="utf-8").count("\n") == 2


def test_rsi_cross_repo_e2e_contract():
    package = json.loads(read("package.json"))
    assert package["scripts"]["rsi:cross-repo-e2e"] == (
        "node scripts/run-sh-script.js scripts/rsi-cross-repo-e2e.sh"
    )
    assert package["scripts"]["rsi:control-plane-release-readiness-dashboard-smoke"] == (
        "node scripts/run-sh-script.js "
        "scripts/rsi-control-plane-release-readiness-dashboard-smoke.sh"
    )

    readme = read("README.md")
    for needle in [
        "npm run rsi:cross-repo-e2e",
        "ao2.rsi-cross-repo-e2e.v1",
        "target/rsi-cross-repo-e2e/latest/summary.json",
        "ao2.rsi-improvement-evidence-gate.v1",
        "ao2.rsi-improvement-trend.v1",
        "ao2.rsi-blueprint-authorization-gate.v1",
        "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
        "release_readiness_dashboard_readback",
        "dashboard_artifact",
        "AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY",
        "measured_improvement_percent",
        "control_surface_readback",
        "bounded_governed_rsi",
        "target_exceeded",
        "workflow-hardening coverage",
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
        "rsi:control-plane-release-readiness-dashboard-smoke",
        "rsi:claim-readiness",
        "rsi:blueprint-authorization-gate",
        "rsi:improvement-evidence-gate",
        "rsi:improvement-trend",
        "release_readiness_dashboard_readback",
        "improvement_evidence_gate",
        "improvement_trend",
        "ao2.rsi-improvement-evidence-gate.v1",
        "ao2.rsi-improvement-trend.v1",
        "ao2.rsi-blueprint-authorization-gate.v1",
        "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
        "dashboard_link_ready",
        "dashboard_artifact",
        "blueprint_authorization",
        "self_authorized_by_rsi",
        "measured_improvement_percent",
        "delta_from_previous_percent",
        "policy claim-publish-gate",
        "covenant.rsi-claim-publish-gate.v1",
        "publishes_claims",
        "approves_rsi_claims",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text


def test_rsi_control_plane_release_readiness_dashboard_smoke_contract():
    script = REPO / "scripts" / "rsi-control-plane-release-readiness-dashboard-smoke.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
        "release:train-control-plane-bridge",
        "ao2.release-readiness-artifact-consumer.v1",
        "ao2-release-readiness-consumer/dashboard.html",
        "AO2 Release Train Readback",
        "AO2 release-readiness consumer dashboard",
        "dashboard_link_ready",
        "claim_publish_decision",
        "claim_publish_authority",
        "control_plane_approves_release",
        "mutates_ao_artifacts",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
    ]:
        assert needle in text


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
        "ao2.rsi-improvement-trend.v1",
        "ao2.rsi-blueprint-authorization-gate.v1",
        "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
        "release-readiness-dashboard-readback/summary.json",
        '"release_readiness_dashboard_readback"]["dashboard_link_ready"] is True',
        '"measured_improvement_percent"] >= 5.0',
        '"trend_ready"] is True',
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
        "ao2.rsi-improvement-trend.v1",
        "ao2.rsi-blueprint-authorization-gate.v1",
        "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
        "release-readiness dashboard readback",
        "measured_improvement_percent",
        "claim_publish_decision=deny",
        "publish_authority=false",
    ]:
        assert needle in verification
