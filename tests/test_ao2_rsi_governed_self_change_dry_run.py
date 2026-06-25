import json
import hashlib
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


def test_rsi_governed_self_change_dry_run_emits_replayable_evidence(tmp_path):
    package = read_json("package.json")
    assert package["scripts"]["rsi:self-change-dry-run"] == (
        "node scripts/run-sh-script.js scripts/rsi-governed-self-change-dry-run.sh"
    )

    readme = read("README.md")
    assert "npm run rsi:self-change-dry-run" in readme
    assert "ao2.rsi-governed-self-change-dry-run.v1" in readme
    assert "does not apply the patch" in readme
    assert "temporary workspace" in readme

    out_root = tmp_path / "rsi-self-change-dry-run"
    claim_readiness_script = REPO / "scripts/rsi-claim-readiness-audit.sh"
    repo_target_before = sha256(claim_readiness_script)
    result = subprocess.run(
        ["npm", "run", "rsi:self-change-dry-run"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_SELF_CHANGE_DRY_RUN_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert "self_change_dry_run=passed" in result.stdout
    assert sha256(claim_readiness_script) == repo_target_before

    summary_path = out_root / "summary.json"
    proposed_patch_path = out_root / "proposed-self-change.patch"
    rollback_patch_path = out_root / "rollback-self-change.patch"
    authority_packet_path = out_root / "live-self-change-authority.packet.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    authority_packet = json.loads(authority_packet_path.read_text(encoding="utf-8"))

    assert proposed_patch_path.read_text(encoding="utf-8").startswith("diff --git")
    assert rollback_patch_path.read_text(encoding="utf-8").startswith("diff --git")

    assert summary["schema_version"] == "ao2.rsi-governed-self-change-dry-run.v1"
    assert summary["status"] == "dry_run_evidence_ready"
    assert summary["claim_boundary"] == {
        "bounded_governed_rsi": "allowed",
        "full_autonomous_self_mutating_rsi": "denied",
    }
    assert summary["self_change"]["mode"] == "dry_run"
    assert summary["self_change"]["repository"] == "ao2"
    assert summary["self_change"]["change_class"] == "verification_path_hardening"
    assert summary["self_change"]["target_files"] == ["scripts/rsi-claim-readiness-audit.sh"]
    assert summary["self_change"]["applies_patch"] is False
    assert summary["self_change"]["proposed_patch"] == {
        "path": "proposed-self-change.patch",
        "sha256": summary["self_change"]["proposed_patch"]["sha256"],
    }
    assert len(summary["self_change"]["proposed_patch"]["sha256"]) == 64
    assert summary["rollback"]["mode"] == "dry_run"
    assert summary["rollback"]["rehearsal_status"] == "planned_not_executed"
    assert summary["rollback"]["rollback_patch"] == {
        "path": "rollback-self-change.patch",
        "sha256": summary["rollback"]["rollback_patch"]["sha256"],
    }
    assert len(summary["rollback"]["rollback_patch"]["sha256"]) == 64
    assert summary["mutation_authority_packet"] == {
        "mode": "dry_run_candidate",
        "schema_version": "covenant.live-self-change-authority.v1",
        "path": "live-self-change-authority.packet.json",
        "sha256": summary["mutation_authority_packet"]["sha256"],
        "schema_valid_for_claim_publish": False,
        "reason": "live self-change execution and observer readback are not present in dry-run evidence",
    }
    assert len(summary["mutation_authority_packet"]["sha256"]) == 64
    assert authority_packet["schema_version"] == "covenant.live-self-change-authority.v1"
    assert authority_packet["authority_id"] == "ao2-rsi-self-change-dry-run-authority"
    assert authority_packet["claim_level"] == "full_autonomous_self_mutating_rsi"
    assert authority_packet["repository"] == "ao2"
    assert authority_packet["branch"] == "codex/live-self-change-rehearsal"
    assert authority_packet["allowed_write_surface"] == ["scripts/rsi-claim-readiness-audit.sh"]
    assert authority_packet["change_class"] == "verification_path"
    assert authority_packet["approval_identity"] == "ao-operator"
    assert authority_packet["approval_ticket_id"] == "ticket-ao2-rsi-dry-run-authority"
    assert authority_packet["exact_digest"]["algorithm"] == "sha256"
    assert authority_packet["exact_digest"]["covers"] == [
        "proposed-self-change.patch",
        "rollback-self-change.patch",
        "summary.json",
    ]
    assert len(authority_packet["exact_digest"]["value"]) == 64
    assert authority_packet["rollback_evidence"] == {
        "status": "passed",
        "evidence_paths": ["summary.json"],
    }
    assert authority_packet["live_self_change_evidence"] == {
        "status": "dry_run_not_live",
        "evidence_paths": [],
    }
    assert authority_packet["observer_readback"] == {
        "status": "missing",
        "observer": "ao2-control-plane",
        "evidence_paths": [],
    }
    assert authority_packet["claim_publish_resource"] == "full-autonomous-self-mutating-rsi"
    assert summary["rollback_rehearsal"]["mode"] == "executed_in_temporary_workspace"
    assert summary["rollback_rehearsal"]["status"] == "passed"
    assert summary["rollback_rehearsal"]["workspace"] == "rollback-rehearsal/worktree"
    assert summary["rollback_rehearsal"]["target_file"] == "scripts/rsi-claim-readiness-audit.sh"
    assert summary["rollback_rehearsal"]["proposed_patch_applied"] is True
    assert summary["rollback_rehearsal"]["rollback_patch_applied"] is True
    assert summary["rollback_rehearsal"]["same_change_class"] is True
    assert summary["rollback_rehearsal"]["verification"] == [
        "bash -n scripts/rsi-claim-readiness-audit.sh"
    ]
    assert (
        summary["rollback_rehearsal"]["target_before_sha256"]
        == summary["self_change"]["target_before_sha256"]["scripts/rsi-claim-readiness-audit.sh"]
    )
    assert (
        summary["rollback_rehearsal"]["target_after_proposed_sha256"]
        != summary["rollback_rehearsal"]["target_before_sha256"]
    )
    assert (
        summary["rollback_rehearsal"]["target_after_rollback_sha256"]
        == summary["rollback_rehearsal"]["target_before_sha256"]
    )
    assert summary["full_claim_blockers"] == [
        "mutation_authority",
        "live_self_change_evidence",
        "executed_rollback_evidence",
        "observer_readback",
        "covenant_claim_publish_approval",
    ]
    assert summary["trust_boundary"] == {
        "local_only": True,
        "uses_network": False,
        "requires_provider_api_key": False,
        "stores_credentials": False,
        "mutates_repositories": False,
        "applies_patch": False,
        "emits_authority_packet_candidate": True,
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
            "AO2_RSI_SELF_CHANGE_DRY_RUN_SUMMARY": str(summary_path),
        },
        capture_output=True,
        text=True,
    )

    assert claim_result.returncode == 0, claim_result.stderr + claim_result.stdout
    claim_summary = json.loads((claim_out_root / "summary.json").read_text(encoding="utf-8"))
    full_claim = claim_summary["claims"]["full_autonomous_self_mutating_rsi"]
    assert full_claim["decision"] == "denied"
    assert full_claim["partial_evidence"]["governed_self_change_dry_run"] == {
        "evidence_state": "present",
        "schema_version": "ao2.rsi-governed-self-change-dry-run.v1",
        "mutation_authority_packet": "dry_run_candidate",
        "rollback_rehearsal_status": "passed",
        "status": "dry_run_evidence_ready",
    }
