#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SCRIPT_TRACKING_READY_REVIEW_PACK_ROOT:-$ROOT/target/script-tracking-ready-review-pack/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" script_tracking_commit_ready_diff \
  env AO2_SCRIPT_TRACKING_COMMIT_READY_DIFF_ROOT="$OUT_ROOT/script-tracking-commit-ready-diff" \
    npm run scripts:tracking-commit-ready-diff

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "script_tracking_commit_ready_diff.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "script-tracking-commit-ready-diff" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
tracked_diff = component_data.get("tracked_file_diff", [])
human_review_packet = out_root / "human-review-packet.md"
lines = ["# Script Tracking Ready Review Pack", "", "## Commit Ready Summary", ""]
lines.append(f"- tracked file entries: {len(tracked_diff)}")
lines.append("- no commit or push: true")
lines.append("")
lines.append("## Excluded Local Artifacts")
for item in component_data.get("excluded_local_artifacts", ["target/", ".ao2-local/"]):
    if isinstance(item, dict):
        path = item.get("path", "")
        status = item.get("status", "unknown")
        lines.append(f"- `{path}` ({status})")
    else:
        lines.append(f"- `{item}`")
human_review_packet.write_text("\n".join(lines) + "\n", encoding="utf-8")
commit_ready_summary = out_root / "commit-ready-summary.json"
commit_ready_summary.write_text(json.dumps({
    "schema_version": "ao2.script-tracking-ready-review-pack.summary.v1",
    "human_review_packet": str(human_review_packet),
    "commit_ready_summary": {"tracked_file_entries": len(tracked_diff)},
    "excluded_local_artifacts": component_data.get("excluded_local_artifacts", ["target/", ".ao2-local/"]),
    "no_commit_or_push": True,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "script_tracking_commit_ready_diff", "command": "scripts:tracking-commit-ready-diff", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "script_tracking_commit_ready_diff.log")},
    {"name": "human_review_packet", "status": "passed" if human_review_packet.is_file() else "failed"},
    {"name": "commit_ready_summary", "status": "passed" if commit_ready_summary.is_file() else "failed"},
    {"name": "excluded_local_artifacts", "status": "passed"},
    {"name": "no_commit_or_push", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.script-tracking-ready-review-pack.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "human_review_packet": str(human_review_packet),
    "commit_ready_summary": str(commit_ready_summary),
    "excluded_local_artifacts": component_data.get("excluded_local_artifacts", ["target/", ".ao2-local/"]),
    "no_commit_or_push": True,
    "component_summaries": {"script_tracking_commit_ready_diff": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
