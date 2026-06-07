#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SCRIPT_TRACKING_REVIEW_PACK_ROOT:-$ROOT/target/script-tracking-review-pack/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" script_tracking_decision_cleanup \
  env AO2_SCRIPT_TRACKING_DECISION_ROOT="$OUT_ROOT/script-tracking-decision-cleanup" \
    npm run scripts:tracking-decision-cleanup

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
code = int((log_dir / "script_tracking_decision_cleanup.log.exit-code").read_text(encoding="utf-8").strip())
result = subprocess.run(["git", "status", "--short", "--", "scripts"], cwd=root, check=False, text=True, capture_output=True)
tracked_script_candidates = []
local_only_artifacts = []
for line in result.stdout.splitlines():
    status_code = line[:2].strip()
    path = line[3:].strip()
    if path.startswith("scripts/") and path.endswith(".sh"):
        tracked_script_candidates.append({"path": path, "status": status_code or "modified", "pre_commit_review": True})
    else:
        local_only_artifacts.append({"path": path, "status": status_code or "modified"})
tracking_review_pack = out_root / "tracking-review-pack.json"
tracking_review_pack.write_text(json.dumps({
    "schema_version": "ao2.script-tracking-review-pack.payload.v1",
    "tracked_script_candidates": tracked_script_candidates,
    "local_only_artifacts": local_only_artifacts,
    "pre_commit_review": "manual review required before any commit",
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
review_md = out_root / "tracking-review-pack.md"
lines = ["# Script Tracking Review Pack", "", "## Tracked Script Candidates", ""]
for item in tracked_script_candidates:
    lines.append(f"- `{item['path']}` ({item['status']})")
lines.extend(["", "## Local Only Artifacts", ""])
for item in local_only_artifacts:
    lines.append(f"- `{item['path']}` ({item['status']})")
review_md.write_text("\n".join(lines) + "\n", encoding="utf-8")
checks = [
    {"name": "script_tracking_decision_cleanup", "command": "scripts:tracking-decision-cleanup", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "script_tracking_decision_cleanup.log")},
    {"name": "tracking_review_pack", "status": "passed" if tracking_review_pack.is_file() else "failed"},
    {"name": "tracked_script_candidates", "status": "passed"},
    {"name": "local_only_artifacts", "status": "passed"},
    {"name": "pre_commit_review", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.script-tracking-review-pack.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "tracking_review_pack": str(tracking_review_pack),
    "tracked_script_candidates": tracked_script_candidates,
    "local_only_artifacts": local_only_artifacts,
    "pre_commit_review": str(review_md),
    "component_summaries": {"script_tracking_decision_cleanup": str(out_root / "script-tracking-decision-cleanup" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
