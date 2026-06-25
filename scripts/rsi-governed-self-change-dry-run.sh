#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RSI_SELF_CHANGE_DRY_RUN_ROOT:-$ROOT/target/rsi-self-change-dry-run/latest}"
SUMMARY="$OUT_ROOT/summary.json"
PROPOSED_PATCH="$OUT_ROOT/proposed-self-change.patch"
ROLLBACK_PATCH="$OUT_ROOT/rollback-self-change.patch"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$SUMMARY" "$PROPOSED_PATCH" "$ROLLBACK_PATCH" <<'PY'
import hashlib
import json
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
proposed_patch_path = Path(sys.argv[3])
rollback_patch_path = Path(sys.argv[4])

target_file = Path("scripts/rsi-claim-readiness-audit.sh")
target_path = root / target_file
target_text = target_path.read_text(encoding="utf-8")
target_lines = target_text.splitlines()
target_sha = hashlib.sha256(target_path.read_bytes()).hexdigest()

insert_after = '    "docs/VERIFICATION.md",'
new_line = '    "scripts/rsi-governed-self-change-dry-run.sh",'
try:
    insert_index = target_lines.index(insert_after)
except ValueError:
    raise SystemExit("unable to locate RSI claim-readiness evidence list insertion point")

if new_line in target_lines:
    raise SystemExit("self-change dry-run evidence is already part of claim-readiness audit")

context_before = target_lines[max(0, insert_index - 2):insert_index + 1]
context_after = target_lines[insert_index + 1:insert_index + 4]
old_hunk = context_before + context_after
new_hunk = context_before + [new_line] + context_after
start_line = max(1, insert_index - 1)

def unified_patch(remove_lines, add_lines):
    body = [
        f"diff --git a/{target_file} b/{target_file}",
        f"--- a/{target_file}",
        f"+++ b/{target_file}",
        f"@@ -{start_line},{len(remove_lines)} +{start_line},{len(add_lines)} @@",
    ]
    shared_prefix = []
    for before, after in zip(remove_lines, add_lines):
        if before == after:
            shared_prefix.append(before)
        else:
            break
    prefix_len = len(shared_prefix)
    suffix_len = 0
    while (
        suffix_len < len(remove_lines) - prefix_len
        and suffix_len < len(add_lines) - prefix_len
        and remove_lines[len(remove_lines) - 1 - suffix_len] == add_lines[len(add_lines) - 1 - suffix_len]
    ):
        suffix_len += 1

    for line in remove_lines[:prefix_len]:
        body.append(f" {line}")
    for line in remove_lines[prefix_len:len(remove_lines) - suffix_len if suffix_len else len(remove_lines)]:
        body.append(f"-{line}")
    for line in add_lines[prefix_len:len(add_lines) - suffix_len if suffix_len else len(add_lines)]:
        body.append(f"+{line}")
    if suffix_len:
        for line in remove_lines[len(remove_lines) - suffix_len:]:
            body.append(f" {line}")
    return "\n".join(body) + "\n"

proposed_patch = unified_patch(old_hunk, new_hunk)
rollback_patch = unified_patch(new_hunk, old_hunk)
proposed_patch_path.write_text(proposed_patch, encoding="utf-8")
rollback_patch_path.write_text(rollback_patch, encoding="utf-8")

proposed_sha = hashlib.sha256(proposed_patch.encode("utf-8")).hexdigest()
rollback_sha = hashlib.sha256(rollback_patch.encode("utf-8")).hexdigest()

rehearsal_rel = Path("rollback-rehearsal/worktree")
rehearsal_root = summary_path.parent / rehearsal_rel
rehearsal_target_path = rehearsal_root / target_file
rehearsal_target_path.parent.mkdir(parents=True, exist_ok=True)
shutil.copy2(target_path, rehearsal_target_path)

def file_sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def run_checked(command):
    result = subprocess.run(
        command,
        cwd=rehearsal_root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(
            f"rollback rehearsal command failed: {' '.join(command)}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return result

run_checked(["patch", "-p1", "-i", str(proposed_patch_path)])
target_after_proposed_sha = file_sha(rehearsal_target_path)
run_checked(["bash", "-n", str(target_file)])
run_checked(["patch", "-p1", "-i", str(rollback_patch_path)])
target_after_rollback_sha = file_sha(rehearsal_target_path)

if target_after_proposed_sha == target_sha:
    raise SystemExit("rollback rehearsal did not change the temporary target")
if target_after_rollback_sha != target_sha:
    raise SystemExit("rollback rehearsal did not restore the temporary target")

payload = {
    "schema_version": "ao2.rsi-governed-self-change-dry-run.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "dry_run_evidence_ready",
    "claim_boundary": {
        "bounded_governed_rsi": "allowed",
        "full_autonomous_self_mutating_rsi": "denied",
    },
    "self_change": {
        "mode": "dry_run",
        "repository": "ao2",
        "change_class": "verification_path_hardening",
        "target_files": [str(target_file)],
        "target_before_sha256": {str(target_file): target_sha},
        "applies_patch": False,
        "proposed_patch": {
            "path": proposed_patch_path.name,
            "sha256": proposed_sha,
        },
        "intent": "Require the AO2 RSI claim-readiness audit to account for governed self-change dry-run evidence before any stronger RSI wording can advance.",
    },
    "rollback": {
        "mode": "dry_run",
        "rehearsal_status": "planned_not_executed",
        "rollback_patch": {
            "path": rollback_patch_path.name,
            "sha256": rollback_sha,
        },
        "same_change_class": True,
    },
    "rollback_rehearsal": {
        "mode": "executed_in_temporary_workspace",
        "status": "passed",
        "workspace": str(rehearsal_rel),
        "target_file": str(target_file),
        "target_before_sha256": target_sha,
        "target_after_proposed_sha256": target_after_proposed_sha,
        "target_after_rollback_sha256": target_after_rollback_sha,
        "proposed_patch_applied": True,
        "rollback_patch_applied": True,
        "same_change_class": True,
        "verification": [
            f"bash -n {target_file}",
        ],
    },
    "full_claim_blockers": [
        "mutation_authority",
        "live_self_change_evidence",
        "executed_rollback_evidence",
        "observer_readback",
        "covenant_claim_publish_approval",
    ],
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "requires_provider_api_key": False,
        "stores_credentials": False,
        "mutates_repositories": False,
        "applies_patch": False,
        "publishes_claims": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print("self_change_dry_run=passed")
print("rollback_rehearsal=passed")
PY
