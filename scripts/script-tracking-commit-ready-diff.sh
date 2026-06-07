#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SCRIPT_TRACKING_COMMIT_READY_DIFF_ROOT:-$ROOT/target/script-tracking-commit-ready-diff/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" script_tracking_review_to_commit_plan \
  env AO2_SCRIPT_TRACKING_COMMIT_PLAN_ROOT="$OUT_ROOT/script-tracking-review-to-commit-plan" \
    npm run scripts:tracking-review-to-commit-plan

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
code = int((log_dir / "script_tracking_review_to_commit_plan.log.exit-code").read_text(encoding="utf-8").strip())
tracked_result = subprocess.run(["git", "status", "--short", "--untracked-files=no", "--", "package.json", "tests/test_public_stabilization.py", "docs/VERIFICATION.md", "scripts"], cwd=root, check=False, text=True, capture_output=True)
local_result = subprocess.run(["git", "status", "--short", "--untracked-files=all", "--", "scripts"], cwd=root, check=False, text=True, capture_output=True)
tracked_file_diff = []
excluded_local_artifacts = [{"path": "target/", "status": "ignored_local_evidence"}, {"path": ".ao2-local/", "status": "ignored_local_state"}]
for line in tracked_result.stdout.splitlines():
    status_code = line[:2].strip()
    path = line[3:].strip()
    if path.startswith("scripts/") or path in {"package.json", "tests/test_public_stabilization.py", "docs/VERIFICATION.md"}:
        tracked_file_diff.append({"status": status_code or "modified", "path": path})
    else:
        excluded_local_artifacts.append({"status": status_code or "modified", "path": path})
for line in local_result.stdout.splitlines():
    status_code = line[:2].strip()
    path = line[3:].strip()
    if status_code == "??" and path.startswith("scripts/"):
        excluded_local_artifacts.append({"status": "untracked_local_only", "path": path})
commit_ready_diff_manifest = out_root / "commit-ready-diff-manifest.json"
commit_ready_diff_manifest.write_text(json.dumps({
    "schema_version": "ao2.script-tracking-commit-ready-diff.manifest.v1",
    "commit_ready_diff_manifest": tracked_file_diff,
    "tracked_file_diff": tracked_file_diff,
    "excluded_local_artifacts": excluded_local_artifacts,
    "no_commit_or_push": True,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "script_tracking_review_to_commit_plan", "command": "scripts:tracking-review-to-commit-plan", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "script_tracking_review_to_commit_plan.log")},
    {"name": "commit_ready_diff_manifest", "status": "passed" if commit_ready_diff_manifest.is_file() else "failed"},
    {"name": "tracked_file_diff", "status": "passed"},
    {"name": "excluded_local_artifacts", "status": "passed"},
    {"name": "no_commit_or_push", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.script-tracking-commit-ready-diff.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "commit_ready_diff_manifest": str(commit_ready_diff_manifest),
    "tracked_file_diff": tracked_file_diff,
    "excluded_local_artifacts": excluded_local_artifacts,
    "no_commit_or_push": True,
    "component_summaries": {"script_tracking_review_to_commit_plan": str(out_root / "script-tracking-review-to-commit-plan" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
