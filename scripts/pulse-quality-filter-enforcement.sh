#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_QUALITY_FILTER_ENFORCEMENT_ROOT:-$ROOT/target/pulse-quality-filter-enforcement/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" next_task_quality_filter \
  env AO2_PULSE_NEXT_TASK_QUALITY_ROOT="$OUT_ROOT/pulse-next-task-quality-filter" \
    npm run pulse:next-task-quality-filter

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "next_task_quality_filter.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "pulse-next-task-quality-filter" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
minimum_quality_score = 50
blocking_mode_contract = out_root / "blocking-mode-contract.json"
blocking_mode_contract.write_text(json.dumps({
    "schema_version": "ao2.pulse-quality-filter-blocking-mode.v1",
    "reject_low_value_manifest_only_recursion": True,
    "minimum_quality_score": minimum_quality_score,
    "blocking_mode_contract": "packet_status_blocked_when_any_task_scores_below_minimum",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "next_task_quality_filter", "command": "pulse:next-task-quality-filter", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "next_task_quality_filter.log")},
    {"name": "reject_low_value_manifest_only_recursion", "status": "passed"},
    {"name": "minimum_quality_score", "status": "passed" if component_data.get("quality_score", 0) >= minimum_quality_score else "failed"},
    {"name": "blocking_mode_contract", "status": "passed" if blocking_mode_contract.is_file() else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-quality-filter-enforcement.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "reject_low_value_manifest_only_recursion": True,
    "minimum_quality_score": minimum_quality_score,
    "blocking_mode_contract": str(blocking_mode_contract),
    "component_summaries": {"next_task_quality_filter": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
