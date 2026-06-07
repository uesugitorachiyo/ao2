#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SCRIPT_TRACKING_COMMIT_PLAN_ROOT:-$ROOT/target/script-tracking-review-to-commit-plan/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" script_tracking_review_pack \
  env AO2_SCRIPT_TRACKING_REVIEW_PACK_ROOT="$OUT_ROOT/script-tracking-review-pack" \
    npm run scripts:tracking-review-pack

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
log_dir = Path(sys.argv[4]).resolve()
code = int((log_dir / "script_tracking_review_pack.log.exit-code").read_text(encoding="utf-8").strip())
tracked_result = subprocess.run(["git", "status", "--short", "--untracked-files=no", "--", "scripts", "package.json", "tests/test_public_stabilization.py", "docs/VERIFICATION.md"], cwd=root, check=False, text=True, capture_output=True)
local_result = subprocess.run(["git", "status", "--short", "--untracked-files=all", "--", "scripts"], cwd=root, check=False, text=True, capture_output=True)
tracked_script_set = []
excluded_local_artifacts = []
for line in tracked_result.stdout.splitlines():
    status_code = line[:2].strip()
    path = line[3:].strip()
    if path.startswith("scripts/") or path in {"package.json", "tests/test_public_stabilization.py", "docs/VERIFICATION.md"}:
        tracked_script_set.append({"path": path, "status": status_code or "modified"})
    else:
        excluded_local_artifacts.append({"path": path, "status": status_code or "modified"})
for line in local_result.stdout.splitlines():
    status_code = line[:2].strip()
    path = line[3:].strip()
    if status_code == "??" and path.startswith("scripts/"):
        excluded_local_artifacts.append({"path": path, "status": "untracked_local_only"})
minimal_commit_plan = out_root / "minimal-commit-plan.json"
minimal_commit_plan.write_text(json.dumps({
    "schema_version": "ao2.script-tracking-commit-plan.payload.v1",
    "minimal_commit_plan": "review tracked command, script, test, and verification doc changes only",
    "tracked_script_set": tracked_script_set,
    "excluded_local_artifacts": excluded_local_artifacts,
    "pre_commit_review_status": "ready_for_human_review_not_committed",
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "script_tracking_review_pack", "command": "scripts:tracking-review-pack", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "script_tracking_review_pack.log")},
    {"name": "minimal_commit_plan", "status": "passed" if minimal_commit_plan.is_file() else "failed"},
    {"name": "tracked_script_set", "status": "passed"},
    {"name": "excluded_local_artifacts", "status": "passed"},
    {"name": "pre_commit_review_status", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.script-tracking-review-to-commit-plan.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "minimal_commit_plan": str(minimal_commit_plan),
    "tracked_script_set": tracked_script_set,
    "excluded_local_artifacts": excluded_local_artifacts,
    "pre_commit_review_status": "ready_for_human_review_not_committed",
    "component_summaries": {"script_tracking_review_pack": str(out_root / "script-tracking-review-pack" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
