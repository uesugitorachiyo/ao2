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
        "mutation_class": "docs_only_single_file",
        "allowed_path_class": "docs_only",
        "target_files": ["docs/VERIFICATION.md"],
    }
    assert summary["bounded_patch_packet"] == {
        "schema_version": "ao2.bounded-patch-packet.v1",
        "status": "class_validated_dry_run_only",
        "mutation_class": "docs_only_single_file",
        "allowed_paths": ["docs/VERIFICATION.md"],
        "forbidden_paths": [
            ".github/",
            "crates/",
            "scripts/",
            "schemas/",
            "package.json",
            "package-lock.json",
            "Cargo.toml",
            "Cargo.lock",
        ],
        "proposed_patch": {
            "path": "proposed-live-mutation.patch",
            "sha256": summary["bounded_patch_packet"]["proposed_patch"]["sha256"],
        },
        "rollback_patch": {
            "path": "rollback-live-mutation.patch",
            "sha256": summary["bounded_patch_packet"]["rollback_patch"]["sha256"],
        },
        "verification_commands": [
            "git diff --check",
            "npm run public:hardening",
            "npm run rsi:claim-readiness",
        ],
        "expected_diff_limits": {
            "max_changed_files": 1,
            "max_added_lines": 1,
            "max_deleted_lines": 0,
            "max_patch_bytes": summary["bounded_patch_packet"]["expected_diff_limits"][
                "max_patch_bytes"
            ],
        },
        "evidence_digests": {
            "target_before_sha256": before_sha,
            "proposed_patch_sha256": summary["changed_file_plan"][0]["proposed_patch"][
                "sha256"
            ],
            "rollback_patch_sha256": summary["rollback_artifact"]["sha256"],
            "verification_plan_sha256": summary["bounded_patch_packet"][
                "evidence_digests"
            ]["verification_plan_sha256"],
            "source_digest_sha256": summary["source_digest"]["value"],
        },
        "execution_boundary": {
            "applies_to_live_repo": False,
            "execute_outside_class": False,
            "class_enforced_before_apply": True,
        },
    }
    assert len(summary["bounded_patch_packet"]["proposed_patch"]["sha256"]) == 64
    assert len(summary["bounded_patch_packet"]["rollback_patch"]["sha256"]) == 64
    assert len(
        summary["bounded_patch_packet"]["evidence_digests"][
            "verification_plan_sha256"
        ]
    ) == 64
    assert summary["bounded_patch_packet"]["expected_diff_limits"]["max_patch_bytes"] > 0
    assert summary["changed_file_plan"] == [
        {
            "path": "docs/VERIFICATION.md",
            "action": "modify",
            "before_sha256": before_sha,
            "allowed_path_class": "docs_only",
            "forbidden_path_check": "passed",
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
        "rehearsal_status": "passed_in_isolated_workspace",
    }
    assert len(summary["rollback_artifact"]["sha256"]) == 64
    assert summary["exact_docs_only_patch"] == {
        "required": True,
        "status": "dry_run_apply_passed",
        "isolated_workspace": True,
        "isolated_workspace_retained": False,
        "target_after_apply_sha256": summary["exact_docs_only_patch"][
            "target_after_apply_sha256"
        ],
        "target_after_rollback_sha256": before_sha,
        "proposed_patch_sha256": summary["changed_file_plan"][0]["proposed_patch"][
            "sha256"
        ],
        "rollback_patch_sha256": summary["rollback_artifact"]["sha256"],
        "applies_to_live_repo": False,
    }
    assert len(summary["exact_docs_only_patch"]["target_after_apply_sha256"]) == 64
    assert summary["rollback_receipt_replay"] == {
        "schema_version": "ao2.rollback-receipt-replay.v1",
        "status": "passed",
        "mode": "dry_run_only",
        "sample_repo": "isolated_temp_workspace",
        "target_file": "docs/VERIFICATION.md",
        "target_before_sha256": before_sha,
        "target_after_apply_sha256": summary["exact_docs_only_patch"][
            "target_after_apply_sha256"
        ],
        "target_after_rollback_sha256": before_sha,
        "rollback_patch": {
            "path": "rollback-live-mutation.patch",
            "sha256": summary["rollback_artifact"]["sha256"],
        },
        "replay_steps": [
            "copy target into isolated workspace",
            "apply proposed patch",
            "apply rollback patch",
            "verify target digest restored",
        ],
        "receipt_digest": summary["rollback_receipt_replay"]["receipt_digest"],
        "mutates_live_repo": False,
        "calls_providers": False,
        "approval_granted": False,
    }
    assert len(summary["rollback_receipt_replay"]["receipt_digest"]) == 64
    assert summary["forbidden_path_checks"] == {
        "status": "passed",
        "allowed_path_class": "docs_only",
        "forbidden_patterns": [
            ".github/",
            "crates/",
            "scripts/",
            "schemas/",
            "package.json",
            "package-lock.json",
            "Cargo.toml",
            "Cargo.lock",
        ],
        "violations": [],
    }
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


def test_live_mutation_dry_run_packet_emits_test_only_packet(tmp_path):
    test_target = REPO / "tests/test_readiness_convergence_gate.py"
    before_sha = sha256(test_target)
    out_root = tmp_path / "live-mutation-test-only-packet"
    result = subprocess.run(
        ["npm", "run", "live-mutation:dry-run-packet"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_LIVE_MUTATION_DRY_RUN_PACKET_ROOT": str(out_root),
            "AO2_LIVE_MUTATION_CLASS": "test_only",
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert "live_mutation_dry_run_packet=passed" in result.stdout
    assert sha256(test_target) == before_sha

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["target"] == {
        "repo": "ao2",
        "mutation_class": "test_only",
        "allowed_path_class": "test_only",
        "target_files": ["tests/test_readiness_convergence_gate.py"],
    }
    assert summary["bounded_patch_packet"]["mutation_class"] == "test_only"
    assert summary["bounded_patch_packet"]["allowed_paths"] == [
        "tests/test_readiness_convergence_gate.py"
    ]
    assert summary["bounded_patch_packet"]["expected_diff_limits"][
        "max_changed_files"
    ] == 1
    assert summary["bounded_patch_packet"]["expected_diff_limits"][
        "max_added_lines"
    ] == 1
    assert summary["bounded_patch_packet"]["verification_commands"] == [
        "git diff --check",
        "python3 -m pytest tests/test_readiness_convergence_gate.py",
    ]
    assert summary["changed_file_plan"] == [
        {
            "path": "tests/test_readiness_convergence_gate.py",
            "action": "modify",
            "before_sha256": before_sha,
            "allowed_path_class": "test_only",
            "forbidden_path_check": "passed",
            "proposed_patch": {
                "path": "proposed-live-mutation.patch",
                "sha256": summary["changed_file_plan"][0]["proposed_patch"][
                    "sha256"
                ],
            },
        }
    ]
    assert summary["exact_test_only_patch"] == {
        "required": True,
        "status": "dry_run_apply_passed",
        "isolated_workspace": True,
        "isolated_workspace_retained": False,
        "target_after_apply_sha256": summary["exact_test_only_patch"][
            "target_after_apply_sha256"
        ],
        "target_after_rollback_sha256": before_sha,
        "proposed_patch_sha256": summary["changed_file_plan"][0]["proposed_patch"][
            "sha256"
        ],
        "rollback_patch_sha256": summary["rollback_artifact"]["sha256"],
        "applies_to_live_repo": False,
    }
    assert summary["forbidden_path_checks"]["allowed_path_class"] == "test_only"
    assert summary["session_boundary"]["mutates_repositories"] is False
    assert summary["session_boundary"]["applies_patch"] is False


def test_live_mutation_dry_run_packet_emits_low_risk_code_dry_run_packet(tmp_path):
    code_target = REPO / "crates/ao2-core/src/lib.rs"
    before_sha = sha256(code_target)
    out_root = tmp_path / "live-mutation-low-risk-code-packet"
    result = subprocess.run(
        ["npm", "run", "live-mutation:dry-run-packet"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_LIVE_MUTATION_DRY_RUN_PACKET_ROOT": str(out_root),
            "AO2_LIVE_MUTATION_CLASS": "low_risk_code",
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert "live_mutation_dry_run_packet=passed" in result.stdout
    assert sha256(code_target) == before_sha

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["target"] == {
        "repo": "ao2",
        "mutation_class": "low_risk_code",
        "allowed_path_class": "low_risk_code",
        "target_files": ["crates/ao2-core/src/lib.rs"],
    }
    assert summary["bounded_patch_packet"]["mutation_class"] == "low_risk_code"
    assert summary["bounded_patch_packet"]["allowed_paths"] == [
        "crates/ao2-core/src/lib.rs"
    ]
    assert summary["bounded_patch_packet"]["forbidden_paths"] == [
        ".github/",
        "docs/",
        "examples/",
        "fixtures/",
        "schemas/",
        "scripts/",
        "skills/",
        "package.json",
        "package-lock.json",
        "pnpm-workspace.yaml",
        "Cargo.toml",
        "Cargo.lock",
        "deny.toml",
        "rust-toolchain.toml",
        "crates/ao2-adapters/",
        "crates/ao2-adapter-codex/",
        "crates/ao2-adapter-claude/",
        "crates/sdd-planner/src/provider/",
    ]
    assert summary["bounded_patch_packet"]["expected_diff_limits"][
        "max_changed_files"
    ] == 2
    assert summary["bounded_patch_packet"]["expected_diff_limits"][
        "max_added_lines"
    ] == 1
    assert summary["bounded_patch_packet"]["expected_diff_limits"][
        "max_deleted_lines"
    ] == 0
    assert summary["bounded_patch_packet"]["verification_commands"] == [
        "git diff --check",
        "cargo test -p ao2-core",
    ]
    assert summary["bounded_patch_packet"]["path_limits"] == {
        "mutation_class": "low_risk_code",
        "max_source_files": 1,
        "max_test_files": 1,
        "max_changed_files": 2,
        "requires_rollback_patch": True,
        "requires_verification_commands": True,
        "denied_path_classes": [
            "scripts",
            "ci_workflows",
            "release",
            "secrets",
            "config_expansion",
            "provider_paths",
            "broad_refactors",
        ],
    }
    assert summary["bounded_patch_packet"]["execution_boundary"] == {
        "applies_to_live_repo": False,
        "execute_outside_class": False,
        "class_enforced_before_apply": True,
    }
    assert summary["changed_file_plan"] == [
        {
            "path": "crates/ao2-core/src/lib.rs",
            "action": "modify",
            "before_sha256": before_sha,
            "allowed_path_class": "low_risk_code",
            "forbidden_path_check": "passed",
            "proposed_patch": {
                "path": "proposed-live-mutation.patch",
                "sha256": summary["changed_file_plan"][0]["proposed_patch"][
                    "sha256"
                ],
            },
        }
    ]
    assert summary["exact_low_risk_code_patch"] == {
        "required": True,
        "status": "dry_run_apply_passed",
        "isolated_workspace": True,
        "isolated_workspace_retained": False,
        "target_after_apply_sha256": summary["exact_low_risk_code_patch"][
            "target_after_apply_sha256"
        ],
        "target_after_rollback_sha256": before_sha,
        "proposed_patch_sha256": summary["changed_file_plan"][0]["proposed_patch"][
            "sha256"
        ],
        "rollback_patch_sha256": summary["rollback_artifact"]["sha256"],
        "applies_to_live_repo": False,
    }
    assert summary["forbidden_path_checks"] == {
        "status": "passed",
        "allowed_path_class": "low_risk_code",
        "forbidden_patterns": [
            ".github/",
            "docs/",
            "examples/",
            "fixtures/",
            "schemas/",
            "scripts/",
            "skills/",
            "package.json",
            "package-lock.json",
            "pnpm-workspace.yaml",
            "Cargo.toml",
            "Cargo.lock",
            "deny.toml",
            "rust-toolchain.toml",
            "crates/ao2-adapters/",
            "crates/ao2-adapter-codex/",
            "crates/ao2-adapter-claude/",
            "crates/sdd-planner/src/provider/",
        ],
        "violations": [],
    }
    assert summary["session_boundary"]["mutates_repositories"] is False
    assert summary["session_boundary"]["applies_patch"] is False
    assert summary["authority_boundary"]["authority_status"] == (
        "not_granted_in_ao2_packet"
    )


def test_live_mutation_dry_run_packet_emits_non_ao_repo_diff_approval_rehearsal(tmp_path):
    sample_target = REPO / "fixtures/discount-service/README.md"
    before_sha = sha256(sample_target)
    out_root = tmp_path / "non-ao-diff-approval-packet"
    result = subprocess.run(
        ["npm", "run", "live-mutation:dry-run-packet"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_LIVE_MUTATION_DRY_RUN_PACKET_ROOT": str(out_root),
            "AO2_LIVE_MUTATION_CLASS": "non_ao_repo_diff_approval_packet",
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert "live_mutation_dry_run_packet=passed" in result.stdout
    assert sha256(sample_target) == before_sha

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    approval_packet = summary["non_ao_repo_diff_approval_packet"]
    opt_in_packet = summary["explicit_opt_in_approval_rehearsal_packet"]
    assert summary["target"] == {
        "repo": "non-ao-fixture/discount-service",
        "mutation_class": "non_ao_repo_diff_approval_packet",
        "allowed_path_class": "non_ao_fixture_docs_only",
        "target_files": ["README.md"],
    }
    assert summary["bounded_patch_packet"]["mutation_class"] == (
        "non_ao_repo_diff_approval_packet"
    )
    assert summary["bounded_patch_packet"]["allowed_paths"] == [
        "fixtures/discount-service/README.md"
    ]
    assert approval_packet == {
        "schema_version": "ao2.non-ao-diff-approval-packet-rehearsal.v1",
        "status": "approval_packet_ready_but_not_granted",
        "sample_repo": "non-ao-fixture/discount-service",
        "sample_repo_path": "fixtures/discount-service",
        "target_file": "README.md",
        "base_commit": "fixture-only-no-git-commit",
        "base_tree_sha256": before_sha,
        "proposed_patch_sha256": summary["changed_file_plan"][0]["proposed_patch"][
            "sha256"
        ],
        "rollback_patch_sha256": summary["rollback_artifact"]["sha256"],
        "approval_packet_sha256": approval_packet["approval_packet_sha256"],
        "approval_granted": False,
        "operator_review_required": True,
        "mutates_sample_repo": False,
        "mutates_live_repo": False,
        "calls_providers": False,
        "creates_branch": False,
        "pushes_commits": False,
        "rsi_status": "denied",
    }
    assert len(approval_packet["approval_packet_sha256"]) == 64
    assert opt_in_packet == {
        "schema_version": "ao2.explicit-opt-in-approval-rehearsal.v1",
        "status": "operator_opt_in_required_not_granted",
        "source_approval_packet_schema": "ao2.non-ao-diff-approval-packet-rehearsal.v1",
        "approval_packet_sha256": approval_packet["approval_packet_sha256"],
        "explicit_opt_in_required": True,
        "explicit_opt_in_granted": False,
        "operator_identity_bound": False,
        "base_tree_sha256": before_sha,
        "proposed_patch_sha256": summary["changed_file_plan"][0]["proposed_patch"][
            "sha256"
        ],
        "rollback_patch_sha256": summary["rollback_artifact"]["sha256"],
        "provider_execution_started": False,
        "mutates_live_repo": False,
        "creates_branch": False,
        "pushes_commits": False,
        "publishes_releases": False,
        "no_promotion_requested": True,
        "rsi_status": "denied",
    }
    assert summary["changed_file_plan"] == [
        {
            "path": "fixtures/discount-service/README.md",
            "display_path": "README.md",
            "action": "modify",
            "before_sha256": before_sha,
            "allowed_path_class": "non_ao_fixture_docs_only",
            "forbidden_path_check": "passed",
            "proposed_patch": {
                "path": "proposed-live-mutation.patch",
                "sha256": summary["changed_file_plan"][0]["proposed_patch"][
                    "sha256"
                ],
            },
        }
    ]
    assert summary["session_boundary"]["mutates_repositories"] is False
    assert summary["session_boundary"]["applies_patch"] is False
    assert summary["provider_boundary"]["provider_calls_allowed"] is False
    assert summary["authority_boundary"]["authority_status"] == (
        "not_granted_in_ao2_packet"
    )


def test_live_mutation_dry_run_packet_denies_low_risk_script_target(tmp_path):
    code_target = REPO / "scripts/run-sh-script.js"
    before_sha = sha256(code_target)
    out_root = tmp_path / "live-mutation-low-risk-script-denied"
    result = subprocess.run(
        ["npm", "run", "live-mutation:dry-run-packet"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_LIVE_MUTATION_DRY_RUN_PACKET_ROOT": str(out_root),
            "AO2_LIVE_MUTATION_CLASS": "low_risk_code",
            "AO2_LIVE_MUTATION_TARGET": "scripts/run-sh-script.js",
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "target path is outside low_risk_code allowlist" in (
        result.stderr + result.stdout
    )
    assert not (out_root / "summary.json").exists()
    assert sha256(code_target) == before_sha


def test_live_mutation_dry_run_packet_denies_higher_code_class(tmp_path):
    code_target = REPO / "scripts/run-sh-script.js"
    before_sha = sha256(code_target)
    out_root = tmp_path / "live-mutation-denied-packet"
    result = subprocess.run(
        ["npm", "run", "live-mutation:dry-run-packet"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_LIVE_MUTATION_DRY_RUN_PACKET_ROOT": str(out_root),
            "AO2_LIVE_MUTATION_CLASS": "multi_repo_low_risk",
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert (
        "mutation class multi_repo_low_risk is denied by AO2 bounded patch packet policy"
        in (result.stderr + result.stdout)
    )
    assert not (out_root / "summary.json").exists()
    assert sha256(code_target) == before_sha
