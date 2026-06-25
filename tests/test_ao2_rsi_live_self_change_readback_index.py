import hashlib
import json
import os
import stat
import subprocess
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]
SCRIPT = REPO / "scripts/rsi-live-self-change-readback-index.sh"


def read_json(path: str) -> dict:
    return json.loads((REPO / path).read_text(encoding="utf-8"))


def read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def live_rehearsal_summary(**overrides):
    payload = {
        "schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
        "status": "live_rehearsal_passed",
        "claim_boundary": {
            "bounded_governed_rsi": "allowed",
            "full_autonomous_self_mutating_rsi": "denied",
        },
        "self_change": {
            "mode": "live_rehearsal",
            "repository": "ao2",
            "change_class": "verification_path_hardening",
            "target_files": ["scripts/rsi-claim-readiness-audit.sh"],
            "target_before_sha256": {"scripts/rsi-claim-readiness-audit.sh": "a" * 64},
            "target_after_mutation_sha256": "b" * 64,
            "target_after_rollback_sha256": "a" * 64,
            "applies_patch": True,
            "repository_restored": True,
            "proposed_patch": {"path": "proposed-live-self-change.patch", "sha256": "c" * 64},
        },
        "rollback": {
            "mode": "live_rehearsal",
            "status": "passed",
            "same_change_class": True,
            "rollback_patch": {"path": "rollback-live-self-change.patch", "sha256": "d" * 64},
        },
        "live_self_change_evidence": {
            "status": "passed",
            "evidence_paths": [
                "summary.json",
                "proposed-live-self-change.patch",
                "rollback-live-self-change.patch",
            ],
        },
        "observer_readback": {
            "status": "missing",
            "observer": "ao2-control-plane",
            "evidence_paths": [],
        },
        "full_claim_blockers": [
            "observer_readback",
            "covenant_claim_publish_approval",
            "retained_claim_level_evidence",
        ],
        "trust_boundary": {
            "local_only": True,
            "uses_network": False,
            "requires_provider_api_key": False,
            "stores_credentials": False,
            "mutates_repositories": True,
            "applies_patch": True,
            "rollback_applied": True,
            "publishes_claims": False,
        },
    }
    payload.update(overrides)
    return payload


def control_plane_readback_summary(**overrides):
    payload = {
        "schema_version": "ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1",
        "status": "passed",
        "producer_schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
        "producer_status": "live_rehearsal_passed",
        "claim_boundary": {
            "bounded_governed_rsi": "allowed",
            "full_autonomous_self_mutating_rsi": "denied",
        },
        "self_change": {
            "mode": "live_rehearsal",
            "repository": "ao2",
            "change_class": "verification_path_hardening",
            "target_files": ["scripts/rsi-claim-readiness-audit.sh"],
            "target_before_sha256": {"scripts/rsi-claim-readiness-audit.sh": "a" * 64},
            "target_after_mutation_sha256": "b" * 64,
            "target_after_rollback_sha256": "a" * 64,
            "applies_patch": True,
            "repository_restored": True,
            "proposed_patch": {"path": "proposed-live-self-change.patch", "sha256": "c" * 64},
        },
        "rollback": {
            "mode": "live_rehearsal",
            "status": "passed",
            "same_change_class": True,
            "rollback_patch": {"path": "rollback-live-self-change.patch", "sha256": "d" * 64},
        },
        "live_self_change_evidence": {
            "status": "passed",
            "evidence_paths": [
                "summary.json",
                "proposed-live-self-change.patch",
                "rollback-live-self-change.patch",
            ],
        },
        "observer_readback": {
            "status": "missing",
            "observer": "ao2-control-plane",
            "evidence_paths": [],
        },
        "observed_full_claim_blockers": [
            "observer_readback",
            "covenant_claim_publish_approval",
            "retained_claim_level_evidence",
        ],
        "gaps": [],
        "trust_boundary": {
            "downloads_github_actions_artifacts": False,
            "control_plane_approves_rsi_claims": False,
            "mutates_ao_artifacts": False,
            "applies_ao_patches": False,
            "mutates_github_repositories": False,
            "mutates_observer_storage": False,
            "publishes_claims": False,
            "credential_material_included": False,
            "provider_api_keys_allowed": False,
        },
    }
    payload.update(overrides)
    return payload


def run_index(tmp_path: Path, live_summary: dict, readback_summary: dict):
    live_path = tmp_path / "live" / "summary.json"
    readback_path = tmp_path / "readback" / "summary.json"
    out_root = tmp_path / "index"
    write_json(live_path, live_summary)
    write_json(readback_path, readback_summary)

    result = subprocess.run(
        ["npm", "run", "rsi:live-self-change-readback-index"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_SUMMARY": str(live_path),
            "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_READBACK_SUMMARY": str(readback_path),
            "AO2_RSI_LIVE_SELF_CHANGE_READBACK_INDEX_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
    )
    summary_path = out_root / "summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8")) if summary_path.exists() else {}
    return result, summary, out_root, live_path, readback_path


def test_rsi_live_self_change_readback_index_retains_control_plane_evidence(tmp_path):
    package = read_json("package.json")
    assert package["scripts"]["rsi:live-self-change-readback-index"] == (
        "node scripts/run-sh-script.js scripts/rsi-live-self-change-readback-index.sh"
    )

    readme = read("README.md")
    assert "npm run rsi:live-self-change-readback-index" in readme
    assert "ao2.rsi-live-self-change-readback-evidence-index.v1" in readme
    assert "ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1" in readme
    assert "does not approve the full RSI claim" in readme

    assert SCRIPT.is_file()
    assert SCRIPT.stat().st_mode & stat.S_IXUSR

    result, summary, out_root, live_path, readback_path = run_index(
        tmp_path,
        live_rehearsal_summary(),
        control_plane_readback_summary(),
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert "rsi_live_self_change_readback_index=passed" in result.stdout
    assert "claim_level=full_autonomous_self_mutating_rsi decision=denied" in result.stdout
    assert (out_root / "index.md").is_file()

    assert summary["schema_version"] == "ao2.rsi-live-self-change-readback-evidence-index.v1"
    assert summary["status"] == "passed"
    assert summary["retained_claim_level_evidence"]["status"] == "present"
    assert summary["retained_claim_level_evidence"]["artifact"] == (
        "ao2-control-plane-ao2-rsi-live-self-change-rehearsal-readback"
    )
    assert summary["retained_claim_level_evidence"]["summary_sha256"] == sha256(readback_path)
    assert summary["sources"]["live_rehearsal"] == {
        "schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
        "status": "live_rehearsal_passed",
        "summary_sha256": sha256(live_path),
        "evidence_paths": [
            "summary.json",
            "proposed-live-self-change.patch",
            "rollback-live-self-change.patch",
        ],
    }
    assert summary["sources"]["control_plane_readback"] == {
        "schema_version": "ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1",
        "status": "passed",
        "producer_schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
        "producer_status": "live_rehearsal_passed",
        "summary_sha256": sha256(readback_path),
    }
    assert summary["claim_boundary"] == {
        "bounded_governed_rsi": "allowed",
        "full_autonomous_self_mutating_rsi": "denied",
    }
    assert summary["full_claim_boundary"] == {
        "decision": "denied",
        "remaining_blockers": [
            "covenant_claim_publish_approval",
            "rehearsal_not_claim_publish_evidence",
        ],
    }
    assert summary["trust_boundary"] == {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "mutates_control_plane_artifacts": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
    }

    claim_out_root = tmp_path / "claim-readiness"
    claim_result = subprocess.run(
        ["npm", "run", "rsi:claim-readiness"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_CLAIM_READINESS_ROOT": str(claim_out_root),
            "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_SUMMARY": str(live_path),
            "AO2_RSI_LIVE_SELF_CHANGE_READBACK_INDEX_SUMMARY": str(out_root / "summary.json"),
        },
        capture_output=True,
        text=True,
    )

    assert claim_result.returncode == 0, claim_result.stderr + claim_result.stdout
    claim_summary = json.loads((claim_out_root / "summary.json").read_text(encoding="utf-8"))
    full_claim = claim_summary["claims"]["full_autonomous_self_mutating_rsi"]
    assert full_claim["decision"] == "denied"
    assert full_claim["partial_evidence"]["live_self_change_readback_index"] == {
        "evidence_state": "present",
        "schema_version": "ao2.rsi-live-self-change-readback-evidence-index.v1",
        "status": "passed",
        "control_plane_readback_status": "passed",
        "retained_claim_level_evidence_status": "present",
        "claim_publish_approved": False,
    }


def test_rsi_live_self_change_readback_index_blocks_mismatched_readback(tmp_path):
    result, summary, _out_root, _live_path, _readback_path = run_index(
        tmp_path,
        live_rehearsal_summary(),
        control_plane_readback_summary(
            status="blocked",
            producer_schema_version="ao2.unexpected.v1",
            gaps=[{"gap_kind": "producer_schema_mismatch"}],
        ),
    )

    assert result.returncode != 0
    assert "rsi_live_self_change_readback_index=failed" in result.stdout
    assert summary["status"] == "failed"
    assert summary["blockers"] == [
        {
            "code": "control_plane_readback_not_passed",
            "severity": "blocking",
            "status": "blocked",
        },
        {
            "code": "control_plane_readback_producer_mismatch",
            "severity": "blocking",
            "producer_schema_version": "ao2.unexpected.v1",
            "producer_status": "live_rehearsal_passed",
        },
        {
            "code": "control_plane_readback_reported_gaps",
            "severity": "blocking",
            "gap_count": 1,
        },
    ]
