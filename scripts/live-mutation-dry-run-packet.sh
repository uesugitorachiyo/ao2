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
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
proposed_patch_path = Path(sys.argv[3])
rollback_patch_path = Path(sys.argv[4])
verification_plan_path = Path(sys.argv[5])

target_rel = "docs/VERIFICATION.md"
target_path = root / target_rel
original = target_path.read_text(encoding="utf-8")
before_sha = hashlib.sha256(target_path.read_bytes()).hexdigest()
marker = "npm run live-mutation:dry-run-packet # dry-run AO2 mutation execution packet"

if marker in original:
    raise SystemExit("live-mutation dry-run packet marker already exists in target")

needle = "npm run verify\n"
if needle not in original:
    raise SystemExit("unable to locate verification ledger insertion point")

proposed = original.replace(needle, needle + marker + "\n", 1)


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


proposed_patch = unified_diff(original, proposed)
rollback_patch = unified_diff(proposed, original)
proposed_patch_path.write_text(proposed_patch, encoding="utf-8")
rollback_patch_path.write_text(rollback_patch, encoding="utf-8")

forbidden_path_patterns = [
    ".github/",
    "crates/",
    "scripts/",
    "schemas/",
    "package.json",
    "package-lock.json",
    "Cargo.toml",
    "Cargo.lock",
]
forbidden_path_violations = [
    target_rel
    for forbidden in forbidden_path_patterns
    if target_rel == forbidden.rstrip("/") or target_rel.startswith(forbidden)
]
if forbidden_path_violations:
    raise SystemExit(f"target path is outside docs-only allowlist: {target_rel}")

with tempfile.TemporaryDirectory(prefix="ao2-docs-only-patch-") as temp_dir:
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
    "commands": [
        "git diff --check",
        "npm run public:hardening",
        "npm run rsi:claim-readiness",
    ],
}
verification_plan_path.write_text(
    json.dumps(verification_plan, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)

source_digest = hashlib.sha256(
    "\n".join([
        before_sha,
        sha256_text(proposed_patch),
        sha256_text(rollback_patch),
        sha256_file(verification_plan_path),
    ]).encode("utf-8")
).hexdigest()

payload = {
    "schema_version": "ao2.live-mutation-dry-run-packet.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "dry_run_packet_ready",
    "target": {
        "repo": "ao2",
        "mutation_class": "tiny_documentation_change",
        "allowed_path_class": "docs_only",
        "target_files": [target_rel],
    },
    "changed_file_plan": [
        {
            "path": target_rel,
            "action": "modify",
            "before_sha256": before_sha,
            "allowed_path_class": "docs_only",
            "forbidden_path_check": "passed",
            "proposed_patch": {
                "path": proposed_patch_path.name,
                "sha256": sha256_text(proposed_patch),
            },
        }
    ],
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
        "sha256": sha256_text(rollback_patch),
        "same_change_class": True,
        "rehearsal_status": "passed_in_isolated_workspace",
    },
    "exact_docs_only_patch": {
        "required": True,
        "status": "dry_run_apply_passed",
        "isolated_workspace": True,
        "isolated_workspace_retained": False,
        "target_after_apply_sha256": isolated_after_sha,
        "target_after_rollback_sha256": isolated_rollback_sha,
        "proposed_patch_sha256": sha256_text(proposed_patch),
        "rollback_patch_sha256": sha256_text(rollback_patch),
        "applies_to_live_repo": False,
    },
    "forbidden_path_checks": {
        "status": "passed",
        "allowed_path_class": "docs_only",
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
        ],
    },
    "next_actions": [
        "bind this packet to Covenant authority, Forge dry-run plan, Foundry gate, Sentinel verdict, rollback rehearsal, and Command readback before any live mutation class is requested",
    ],
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print("live_mutation_dry_run_packet=passed")
PY
