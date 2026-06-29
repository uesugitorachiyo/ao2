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


def test_live_mutation_dry_run_packet_emits_non_mutating_execution_plan(tmp_path):
    package = read_json("package.json")
    assert package["scripts"]["live-mutation:dry-run-packet"] == (
        "node scripts/run-sh-script.js scripts/live-mutation-dry-run-packet.sh"
    )

    readme = read("README.md")
    assert "npm run live-mutation:dry-run-packet" in readme
    assert "ao2.live-mutation-dry-run-packet.v1" in readme
    assert "does not apply the patch" in readme
    assert "does not call providers" in readme

    verification_doc = REPO / "docs/VERIFICATION.md"
    before_sha = sha256(verification_doc)
    out_root = tmp_path / "live-mutation-dry-run-packet"
    result = subprocess.run(
        ["npm", "run", "live-mutation:dry-run-packet"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_LIVE_MUTATION_DRY_RUN_PACKET_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert "live_mutation_dry_run_packet=passed" in result.stdout
    assert sha256(verification_doc) == before_sha

    summary_path = out_root / "summary.json"
    proposed_patch_path = out_root / "proposed-live-mutation.patch"
    rollback_patch_path = out_root / "rollback-live-mutation.patch"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

    assert proposed_patch_path.read_text(encoding="utf-8").startswith("--- ")
    assert rollback_patch_path.read_text(encoding="utf-8").startswith("--- ")
    assert summary["schema_version"] == "ao2.live-mutation-dry-run-packet.v1"
    assert summary["status"] == "dry_run_packet_ready"
    assert summary["target"] == {
        "repo": "ao2",
        "mutation_class": "tiny_documentation_change",
        "allowed_path_class": "docs_only",
        "target_files": ["docs/VERIFICATION.md"],
    }
    assert summary["changed_file_plan"] == [
        {
            "path": "docs/VERIFICATION.md",
            "action": "modify",
            "before_sha256": before_sha,
            "proposed_patch": {
                "path": "proposed-live-mutation.patch",
                "sha256": summary["changed_file_plan"][0]["proposed_patch"]["sha256"],
            },
        }
    ]
    assert len(summary["changed_file_plan"][0]["proposed_patch"]["sha256"]) == 64
    assert summary["verification_plan"] == {
        "required": True,
        "commands": [
            "git diff --check",
            "npm run public:hardening",
            "npm run rsi:claim-readiness",
        ],
        "evidence_paths": [
            "target/live-mutation-dry-run-packet/latest/verification-plan.json"
        ],
    }
    assert summary["rollback_artifact"] == {
        "required": True,
        "path": "rollback-live-mutation.patch",
        "sha256": summary["rollback_artifact"]["sha256"],
        "same_change_class": True,
        "rehearsal_status": "not_executed_dry_run_packet",
    }
    assert len(summary["rollback_artifact"]["sha256"]) == 64
    assert summary["authority_boundary"] == {
        "requires_covenant_authority": True,
        "requires_forge_plan": True,
        "requires_foundry_gate": True,
        "requires_operator_kill_switch": True,
        "authority_status": "not_granted_in_ao2_packet",
    }
    assert summary["provider_boundary"] == {
        "provider_calls_allowed": False,
        "requires_provider_api_key": False,
        "uses_openai_api_key": False,
        "uses_anthropic_api_key": False,
        "exact_digest_approval_required_for_provider_patch": True,
    }
    assert summary["session_boundary"] == {
        "local_only": True,
        "network_required": False,
        "mutates_repositories": False,
        "applies_patch": False,
        "creates_branch": False,
        "pushes_commits": False,
        "uploads_artifacts": False,
        "publishes_releases": False,
    }
    assert summary["rollback_plan"] == {
        "restore_strategy": "apply rollback-live-mutation.patch in the isolated worktree before any PR is opened",
        "quarantine_on_failure": True,
        "requires_clean_worktree_before_start": True,
    }
    assert summary["next_actions"] == [
        "bind this packet to Covenant authority, Forge dry-run plan, Foundry gate, Sentinel verdict, rollback rehearsal, and Command readback before any live mutation class is requested"
    ]

    serialized = json.dumps(summary, sort_keys=True)
    assert str(REPO) not in serialized
    assert str(Path.home()) not in serialized
