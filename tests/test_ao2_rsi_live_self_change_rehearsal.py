import hashlib
import json
import os
import subprocess
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


def read_json(path: str) -> dict:
    return json.loads((REPO / path).read_text(encoding="utf-8"))


def read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def test_rsi_live_self_change_rehearsal_is_explicitly_gated_and_rolls_back(tmp_path):
    package = read_json("package.json")
    assert package["scripts"]["rsi:live-self-change-rehearsal"] == (
        "node scripts/run-sh-script.js scripts/rsi-live-self-change-rehearsal.sh"
    )

    readme = read("README.md")
    assert "npm run rsi:live-self-change-rehearsal" in readme
    assert "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1" in readme
    assert "ao2.rsi-live-self-change-rehearsal.v1" in readme
    assert "rolls the file back" in readme
    assert "does not publish the full RSI claim" in readme

    target = REPO / "scripts/rsi-claim-readiness-audit.sh"
    before_sha = sha256(target)
    refused_root = tmp_path / "refused-live-self-change"
    refused = subprocess.run(
        ["npm", "run", "rsi:live-self-change-rehearsal"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_ROOT": str(refused_root),
        },
        capture_output=True,
        text=True,
    )

    assert refused.returncode != 0
    assert "live_self_change_rehearsal=refused" in refused.stdout
    assert sha256(target) == before_sha
    refused_summary = json.loads((refused_root / "summary.json").read_text(encoding="utf-8"))
    assert refused_summary["schema_version"] == "ao2.rsi-live-self-change-rehearsal.v1"
    assert refused_summary["status"] == "refused_missing_operator_flag"
    assert refused_summary["trust_boundary"]["mutates_repositories"] is False
    assert refused_summary["trust_boundary"]["publishes_claims"] is False

    out_root = tmp_path / "live-self-change"
    result = subprocess.run(
        ["npm", "run", "rsi:live-self-change-rehearsal"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL": "1",
            "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert "live_self_change_rehearsal=passed" in result.stdout
    assert "rollback=passed" in result.stdout
    assert sha256(target) == before_sha

    summary_path = out_root / "summary.json"
    proposed_patch_path = out_root / "proposed-live-self-change.patch"
    rollback_patch_path = out_root / "rollback-live-self-change.patch"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

    assert proposed_patch_path.read_text(encoding="utf-8").startswith("--- ")
    assert rollback_patch_path.read_text(encoding="utf-8").startswith("--- ")
    assert summary["schema_version"] == "ao2.rsi-live-self-change-rehearsal.v1"
    assert summary["status"] == "live_rehearsal_passed"
    assert summary["claim_boundary"] == {
        "bounded_governed_rsi": "allowed",
        "full_autonomous_self_mutating_rsi": "denied",
    }
    assert summary["self_change"] == {
        "mode": "live_rehearsal",
        "repository": "ao2",
        "change_class": "verification_path_hardening",
        "target_files": ["scripts/rsi-claim-readiness-audit.sh"],
        "target_before_sha256": {
            "scripts/rsi-claim-readiness-audit.sh": before_sha,
        },
        "target_after_mutation_sha256": summary["self_change"]["target_after_mutation_sha256"],
        "target_after_rollback_sha256": before_sha,
        "applies_patch": True,
        "repository_restored": True,
        "proposed_patch": {
            "path": "proposed-live-self-change.patch",
            "sha256": summary["self_change"]["proposed_patch"]["sha256"],
        },
    }
    assert len(summary["self_change"]["target_after_mutation_sha256"]) == 64
    assert summary["self_change"]["target_after_mutation_sha256"] != before_sha
    assert len(summary["self_change"]["proposed_patch"]["sha256"]) == 64
    assert summary["rollback"] == {
        "mode": "live_rehearsal",
        "status": "passed",
        "same_change_class": True,
        "rollback_patch": {
            "path": "rollback-live-self-change.patch",
            "sha256": summary["rollback"]["rollback_patch"]["sha256"],
        },
    }
    assert len(summary["rollback"]["rollback_patch"]["sha256"]) == 64
    assert summary["live_self_change_evidence"] == {
        "status": "passed",
        "evidence_paths": [
            "summary.json",
            "proposed-live-self-change.patch",
            "rollback-live-self-change.patch",
        ],
    }
    assert summary["observer_readback"] == {
        "status": "missing",
        "observer": "ao2-control-plane",
        "evidence_paths": [],
    }
    assert summary["full_claim_blockers"] == [
        "observer_readback",
        "covenant_claim_publish_approval",
        "retained_claim_level_evidence",
    ]
    assert summary["trust_boundary"] == {
        "local_only": True,
        "uses_network": False,
        "requires_provider_api_key": False,
        "stores_credentials": False,
        "mutates_repositories": True,
        "applies_patch": True,
        "rollback_applied": True,
        "publishes_claims": False,
    }

    serialized = json.dumps(summary, sort_keys=True)
    assert str(REPO) not in serialized
    assert str(Path.home()) not in serialized

    claim_out_root = tmp_path / "rsi-claim-readiness"
    claim_result = subprocess.run(
        ["npm", "run", "rsi:claim-readiness"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_CLAIM_READINESS_ROOT": str(claim_out_root),
            "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_SUMMARY": str(summary_path),
        },
        capture_output=True,
        text=True,
    )

    assert claim_result.returncode == 0, claim_result.stderr + claim_result.stdout
    claim_summary = json.loads((claim_out_root / "summary.json").read_text(encoding="utf-8"))
    full_claim = claim_summary["claims"]["full_autonomous_self_mutating_rsi"]
    assert full_claim["decision"] == "denied"
    assert full_claim["partial_evidence"]["live_self_change_rehearsal"] == {
        "evidence_state": "present",
        "schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
        "status": "live_rehearsal_passed",
        "repository_restored": True,
        "observer_readback_status": "missing",
    }
