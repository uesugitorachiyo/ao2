#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_LIVE_MUTATION_DRY_RUN_PACKET_ROOT:-$ROOT/target/live-mutation-dry-run-packet/latest}"
SUMMARY="$OUT_ROOT/summary.json"
PROPOSED_PATCH="$OUT_ROOT/proposed-live-mutation.patch"
ROLLBACK_PATCH="$OUT_ROOT/rollback-live-mutation.patch"
VERIFICATION_PLAN="$OUT_ROOT/verification-plan.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$SUMMARY" "$PROPOSED_PATCH" "$ROLLBACK_PATCH" "$VERIFICATION_PLAN" <<'PY'
import difflib
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
summary_path = Path(sys.argv[2]).resolve()
proposed_patch_path = Path(sys.argv[3]).resolve()
rollback_patch_path = Path(sys.argv[4]).resolve()
verification_plan_path = Path(sys.argv[5]).resolve()

mutation_class = os.environ.get("AO2_LIVE_MUTATION_CLASS", "docs_only_single_file")
class_profiles = {
    "docs_only_single_file": {
        "target_rel": "docs/VERIFICATION.md",
        "allowed_path_class": "docs_only",
        "marker": "npm run live-mutation:dry-run-packet # dry-run AO2 mutation execution packet",
        "needle": "npm run verify\n",
        "insert": "after",
        "forbidden_path_patterns": [
            ".github/",
            "crates/",
            "scripts/",
            "schemas/",
            "package.json",
            "package-lock.json",
            "Cargo.toml",
            "Cargo.lock",
        ],
        "verification_commands": [
            "git diff --check",
            "npm run public:hardening",
            "npm run rsi:claim-readiness",
        ],
        "exact_patch_key": "exact_docs_only_patch",
        "temp_prefix": "ao2-docs-only-patch-",
        "max_changed_files": 1,
        "max_added_lines": 1,
        "max_deleted_lines": 0,
    },
    "docs_only_multi_file": {
        "target_rel": "docs/VERIFICATION.md",
        "allowed_path_class": "docs_only",
        "marker": "npm run live-mutation:dry-run-packet # dry-run AO2 mutation execution packet",
        "needle": "npm run verify\n",
        "insert": "after",
        "forbidden_path_patterns": [
            ".github/",
            "crates/",
            "scripts/",
            "schemas/",
            "package.json",
            "package-lock.json",
            "Cargo.toml",
            "Cargo.lock",
        ],
        "verification_commands": [
            "git diff --check",
            "npm run public:hardening",
            "npm run rsi:claim-readiness",
        ],
        "exact_patch_key": "exact_docs_only_patch",
        "temp_prefix": "ao2-docs-only-patch-",
        "max_changed_files": 2,
        "max_added_lines": 8,
        "max_deleted_lines": 4,
    },
    "test_only": {
        "target_rel": "tests/test_readiness_convergence_gate.py",
        "allowed_path_class": "test_only",
        "marker": "# AO2 test_only dry-run mutation packet marker",
        "needle": "def test_readiness_convergence_script_is_registered_and_operational():\n",
        "insert": "before",
        "forbidden_path_patterns": [
            ".github/",
            "crates/",
            "docs/",
            "scripts/",
            "schemas/",
            "package.json",
            "package-lock.json",
            "Cargo.toml",
            "Cargo.lock",
        ],
        "verification_commands": [
            "git diff --check",
            "python3 -m pytest tests/test_readiness_convergence_gate.py",
        ],
        "exact_patch_key": "exact_test_only_patch",
        "temp_prefix": "ao2-test-only-patch-",
        "max_changed_files": 1,
        "max_added_lines": 1,
        "max_deleted_lines": 0,
    },
    "low_risk_code": {
        "target_rel": "crates/ao2-core/src/lib.rs",
        "allowed_path_class": "low_risk_code",
        "marker": "// AO2 low_risk_code dry-run mutation packet marker",
        "needle": "mod obligations;\n",
        "insert": "before",
        "forbidden_path_patterns": [
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
        "verification_commands": [
            "git diff --check",
            "cargo test -p ao2-core",
        ],
        "exact_patch_key": "exact_low_risk_code_patch",
        "temp_prefix": "ao2-low-risk-code-patch-",
        "max_changed_files": 2,
        "max_source_files": 1,
        "max_test_files": 1,
        "max_added_lines": 1,
        "max_deleted_lines": 0,
        "denied_path_classes": [
            "scripts",
            "ci_workflows",
            "release",
            "secrets",
            "config_expansion",
            "provider_paths",
            "broad_refactors",
        ],
    },
    "non_ao_repo_diff_approval_packet": {
        "target_rel": "fixtures/discount-service/README.md",
        "display_target_rel": "README.md",
        "target_repo": "non-ao-fixture/discount-service",
        "sample_repo_path": "fixtures/discount-service",
        "allowed_path_class": "non_ao_fixture_docs_only",
        "marker": "AO2 non-AO diff approval packet rehearsal marker",
        "needle": "Verifier:\n",
        "insert": "before",
        "forbidden_path_patterns": [
            ".github/",
            "crates/",
            "docs/",
            "examples/",
            "scripts/",
            "schemas/",
            "package.json",
            "package-lock.json",
            "Cargo.toml",
            "Cargo.lock",
        ],
        "verification_commands": [
            "git diff --check",
            "python3 -m pytest fixtures/discount-service/tests",
        ],
        "exact_patch_key": "exact_non_ao_fixture_docs_patch",
        "temp_prefix": "ao2-non-ao-approval-packet-",
        "max_changed_files": 1,
        "max_added_lines": 1,
        "max_deleted_lines": 0,
        "include_non_ao_approval_packet": True,
    },
}
denied_classes = {
    "docs_config_only",
    "multi_repo_low_risk",
    "complex_repo_mutation",
}
if mutation_class in denied_classes:
    raise SystemExit(
        f"mutation class {mutation_class} is denied by AO2 bounded patch packet policy"
    )
if mutation_class not in class_profiles:
    raise SystemExit(
        f"mutation class {mutation_class} is not supported by AO2 bounded patch packet policy"
    )

profile = class_profiles[mutation_class]
target_rel = profile["target_rel"]
if mutation_class == "low_risk_code":
    target_rel = os.environ.get("AO2_LIVE_MUTATION_TARGET", target_rel)


def low_risk_path_kind(path: str) -> str:
    if path.startswith("crates/ao2-core/src/") and path.endswith(".rs"):
        return "source"
    if path.startswith("crates/ao2-core/tests/") and path.endswith(".rs"):
        return "test"
    return "forbidden"


def changed_path_counts(paths: list[str]) -> dict:
    counts = {"source": 0, "test": 0}
    for path in paths:
        if mutation_class != "low_risk_code":
            continue
        kind = low_risk_path_kind(path)
        if kind == "forbidden":
            raise SystemExit(
                f"target path is outside low_risk_code allowlist: {path}"
            )
        counts[kind] += 1
    return counts


if mutation_class == "low_risk_code":
    changed_path_counts([target_rel])

target_path = root / target_rel
original = target_path.read_text(encoding="utf-8")
before_sha = hashlib.sha256(target_path.read_bytes()).hexdigest()
marker = profile["marker"]
if marker in original:
    raise SystemExit("live-mutation dry-run packet marker already exists in target")

needle = profile["needle"]
if needle not in original:
    raise SystemExit("unable to locate verification ledger insertion point")

if profile["insert"] == "after":
    proposed = original.replace(needle, needle + marker + "\n", 1)
else:
    proposed = original.replace(needle, marker + "\n" + needle, 1)


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def unified_diff(before: str, after: str) -> str:
    diff = difflib.unified_diff(
        before.splitlines(keepends=True),
        after.splitlines(keepends=True),
        fromfile=f"a/{target_rel}",
        tofile=f"b/{target_rel}",
    )
    text = "".join(diff)
    if not text.endswith("\n"):
        text += "\n"
    return text


def diff_counts(diff_text: str) -> dict:
    added = 0
    deleted = 0
    for line in diff_text.splitlines():
        if line.startswith("+++") or line.startswith("---"):
            continue
        if line.startswith("+"):
            added += 1
        elif line.startswith("-"):
            deleted += 1
    return {"added": added, "deleted": deleted}


proposed_patch = unified_diff(original, proposed)
rollback_patch = unified_diff(proposed, original)
proposed_patch_sha = sha256_text(proposed_patch)
rollback_patch_sha = sha256_text(rollback_patch)
patch_counts = diff_counts(proposed_patch)
class_limits = profile
if patch_counts["added"] > class_limits["max_added_lines"]:
    raise SystemExit(
        f"proposed patch additions exceed {mutation_class} limit: {patch_counts['added']}"
    )
if patch_counts["deleted"] > class_limits["max_deleted_lines"]:
    raise SystemExit(
        f"proposed patch deletions exceed {mutation_class} limit: {patch_counts['deleted']}"
    )
if class_limits["max_changed_files"] < 1:
    raise SystemExit("bounded patch packet class limit must allow at least one file")

forbidden_path_patterns = profile["forbidden_path_patterns"]
forbidden_path_violations = [
    target_rel
    for forbidden in forbidden_path_patterns
    if target_rel == forbidden.rstrip("/") or target_rel.startswith(forbidden)
]
if forbidden_path_violations:
    raise SystemExit(
        f"target path is outside {profile['allowed_path_class']} allowlist: {target_rel}"
    )
allowed_paths = [target_rel]
display_target_rel = profile.get("display_target_rel", target_rel)
if target_rel not in allowed_paths:
    raise SystemExit("target path is not in bounded patch packet allowed_paths")
if len(allowed_paths) > class_limits["max_changed_files"]:
    raise SystemExit(
        f"bounded patch packet changed-file count exceeds {mutation_class} limit"
    )
path_counts = changed_path_counts(allowed_paths)
if mutation_class == "low_risk_code":
    if path_counts["source"] > class_limits["max_source_files"]:
        raise SystemExit("low_risk_code packet exceeds one source file")
    if path_counts["test"] > class_limits["max_test_files"]:
        raise SystemExit("low_risk_code packet exceeds one test file")

proposed_patch_path.write_text(proposed_patch, encoding="utf-8")
rollback_patch_path.write_text(rollback_patch, encoding="utf-8")

with tempfile.TemporaryDirectory(prefix=profile["temp_prefix"]) as temp_dir:
    isolated_root = Path(temp_dir)
    isolated_target = isolated_root / target_rel
    isolated_target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(target_path, isolated_target)
    subprocess.run(
        ["git", "apply", "--check", str(proposed_patch_path)],
        cwd=isolated_root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    subprocess.run(
        ["git", "apply", str(proposed_patch_path)],
        cwd=isolated_root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    isolated_after_sha = sha256_file(isolated_target)
    if isolated_after_sha != sha256_text(proposed):
        raise SystemExit("isolated dry-run apply produced unexpected target digest")
    subprocess.run(
        ["git", "apply", str(rollback_patch_path)],
        cwd=isolated_root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    isolated_rollback_sha = sha256_file(isolated_target)
    if isolated_rollback_sha != before_sha:
        raise SystemExit("isolated rollback did not restore target digest")

verification_plan = {
    "schema_version": "ao2.live-mutation-dry-run-verification-plan.v1",
    "required": True,
    "commands": profile["verification_commands"],
}
verification_plan_path.write_text(
    json.dumps(verification_plan, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
verification_plan_sha = sha256_file(verification_plan_path)

source_digest = hashlib.sha256(
    "\n".join([
        before_sha,
        proposed_patch_sha,
        rollback_patch_sha,
        verification_plan_sha,
        mutation_class,
        "ao2.bounded-patch-packet.v1",
    ]).encode("utf-8")
).hexdigest()
rollback_receipt_digest = hashlib.sha256(
    "\n".join([
        "ao2.rollback-receipt-replay.v1",
        mutation_class,
        target_rel,
        before_sha,
        isolated_after_sha,
        isolated_rollback_sha,
        proposed_patch_sha,
        rollback_patch_sha,
    ]).encode("utf-8")
).hexdigest()
non_ao_approval_packet = None
if profile.get("include_non_ao_approval_packet"):
    approval_packet_sha = hashlib.sha256(
        "\n".join([
            "ao2.non-ao-diff-approval-packet-rehearsal.v1",
            profile["target_repo"],
            profile["sample_repo_path"],
            display_target_rel,
            before_sha,
            proposed_patch_sha,
            rollback_patch_sha,
            source_digest,
            "approval_granted=false",
            "calls_providers=false",
            "mutates_live_repo=false",
        ]).encode("utf-8")
    ).hexdigest()
    non_ao_approval_packet = {
        "schema_version": "ao2.non-ao-diff-approval-packet-rehearsal.v1",
        "status": "approval_packet_ready_but_not_granted",
        "sample_repo": profile["target_repo"],
        "sample_repo_path": profile["sample_repo_path"],
        "target_file": display_target_rel,
        "base_commit": "fixture-only-no-git-commit",
        "base_tree_sha256": before_sha,
        "proposed_patch_sha256": proposed_patch_sha,
        "rollback_patch_sha256": rollback_patch_sha,
        "approval_packet_sha256": approval_packet_sha,
        "approval_granted": False,
        "operator_review_required": True,
        "mutates_sample_repo": False,
        "mutates_live_repo": False,
        "calls_providers": False,
        "creates_branch": False,
        "pushes_commits": False,
        "rsi_status": "denied",
    }
bounded_patch_packet = {
    "schema_version": "ao2.bounded-patch-packet.v1",
    "status": "class_validated_dry_run_only",
    "mutation_class": mutation_class,
    "allowed_paths": allowed_paths,
    "forbidden_paths": forbidden_path_patterns,
    "proposed_patch": {
        "path": proposed_patch_path.name,
        "sha256": proposed_patch_sha,
    },
    "rollback_patch": {
        "path": rollback_patch_path.name,
        "sha256": rollback_patch_sha,
    },
    "verification_commands": verification_plan["commands"],
    "expected_diff_limits": {
        "max_changed_files": class_limits["max_changed_files"],
        "max_added_lines": class_limits["max_added_lines"],
        "max_deleted_lines": class_limits["max_deleted_lines"],
        "max_patch_bytes": len(proposed_patch.encode("utf-8")),
    },
    "evidence_digests": {
        "target_before_sha256": before_sha,
        "proposed_patch_sha256": proposed_patch_sha,
        "rollback_patch_sha256": rollback_patch_sha,
        "verification_plan_sha256": verification_plan_sha,
        "source_digest_sha256": source_digest,
    },
    "execution_boundary": {
        "applies_to_live_repo": False,
        "execute_outside_class": False,
        "class_enforced_before_apply": True,
    },
}
if mutation_class == "low_risk_code":
    bounded_patch_packet["path_limits"] = {
        "mutation_class": mutation_class,
        "max_source_files": class_limits["max_source_files"],
        "max_test_files": class_limits["max_test_files"],
        "max_changed_files": class_limits["max_changed_files"],
        "requires_rollback_patch": True,
        "requires_verification_commands": True,
        "denied_path_classes": class_limits["denied_path_classes"],
    }

payload = {
    "schema_version": "ao2.live-mutation-dry-run-packet.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "dry_run_packet_ready",
    "target": {
        "repo": profile.get("target_repo", "ao2"),
        "mutation_class": mutation_class,
        "allowed_path_class": profile["allowed_path_class"],
        "target_files": [display_target_rel],
    },
    "bounded_patch_packet": bounded_patch_packet,
    "changed_file_plan": [],
    "verification_plan": {
        "required": True,
        "commands": verification_plan["commands"],
        "evidence_paths": [
            "target/live-mutation-dry-run-packet/latest/verification-plan.json",
        ],
    },
    "rollback_artifact": {
        "required": True,
        "path": rollback_patch_path.name,
        "sha256": rollback_patch_sha,
        "same_change_class": True,
        "rehearsal_status": "passed_in_isolated_workspace",
    },
    profile["exact_patch_key"]: {
        "required": True,
        "status": "dry_run_apply_passed",
        "isolated_workspace": True,
        "isolated_workspace_retained": False,
        "target_after_apply_sha256": isolated_after_sha,
        "target_after_rollback_sha256": isolated_rollback_sha,
        "proposed_patch_sha256": proposed_patch_sha,
        "rollback_patch_sha256": rollback_patch_sha,
        "applies_to_live_repo": False,
    },
    "rollback_receipt_replay": {
        "schema_version": "ao2.rollback-receipt-replay.v1",
        "status": "passed",
        "mode": "dry_run_only",
        "sample_repo": "isolated_temp_workspace",
        "target_file": target_rel,
        "target_before_sha256": before_sha,
        "target_after_apply_sha256": isolated_after_sha,
        "target_after_rollback_sha256": isolated_rollback_sha,
        "rollback_patch": {
            "path": rollback_patch_path.name,
            "sha256": rollback_patch_sha,
        },
        "replay_steps": [
            "copy target into isolated workspace",
            "apply proposed patch",
            "apply rollback patch",
            "verify target digest restored",
        ],
        "receipt_digest": rollback_receipt_digest,
        "mutates_live_repo": False,
        "calls_providers": False,
        "approval_granted": False,
    },
    "forbidden_path_checks": {
        "status": "passed",
        "allowed_path_class": profile["allowed_path_class"],
        "forbidden_patterns": forbidden_path_patterns,
        "violations": [],
    },
    "authority_boundary": {
        "requires_covenant_authority": True,
        "requires_forge_plan": True,
        "requires_foundry_gate": True,
        "requires_operator_kill_switch": True,
        "authority_status": "not_granted_in_ao2_packet",
    },
    "provider_boundary": {
        "provider_calls_allowed": False,
        "requires_provider_api_key": False,
        "uses_openai_api_key": False,
        "uses_anthropic_api_key": False,
        "exact_digest_approval_required_for_provider_patch": True,
    },
    "session_boundary": {
        "local_only": True,
        "network_required": False,
        "mutates_repositories": False,
        "applies_patch": False,
        "creates_branch": False,
        "pushes_commits": False,
        "uploads_artifacts": False,
        "publishes_releases": False,
    },
    "rollback_plan": {
        "restore_strategy": "apply rollback-live-mutation.patch in the isolated worktree before any PR is opened",
        "quarantine_on_failure": True,
        "requires_clean_worktree_before_start": True,
    },
    "source_digest": {
        "algorithm": "sha256",
        "value": source_digest,
        "covers": [
            target_rel,
            proposed_patch_path.name,
            rollback_patch_path.name,
            verification_plan_path.name,
            "bounded_patch_packet",
        ],
    },
    "next_actions": [
        "bind this packet to Covenant authority, Forge dry-run plan, Foundry gate, Sentinel verdict, rollback rehearsal, and Command readback before any live mutation class is requested",
    ],
}
changed_file_record = {
    "path": target_rel,
    "action": "modify",
    "before_sha256": before_sha,
    "allowed_path_class": profile["allowed_path_class"],
    "forbidden_path_check": "passed",
    "proposed_patch": {
        "path": proposed_patch_path.name,
        "sha256": proposed_patch_sha,
    },
}
if display_target_rel != target_rel:
    changed_file_record["display_path"] = display_target_rel
payload["changed_file_plan"] = [changed_file_record]
if non_ao_approval_packet is not None:
    payload["non_ao_repo_diff_approval_packet"] = non_ao_approval_packet

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print("live_mutation_dry_run_packet=passed")
PY
