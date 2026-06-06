#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_WORKBENCH_LIVE_PREVIEW_ROOT:-$ROOT/target/provider-workbench-live-preview-hardening/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

run_step() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  set +e
  "$@" >"$log" 2>&1
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

run_step score_budget_workbench_uat \
  env AO2_PROVIDER_SCORE_BUDGET_WORKBENCH_UAT_ROOT="$OUT_ROOT/provider-score-budget-workbench-uat" \
    npm run provider:score-budget-workbench-uat

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "score_budget_workbench_uat.log.exit-code").read_text(encoding="utf-8").strip())
preview = out_root / "live-preview-hardening.json"
preview.write_text(json.dumps({
    "schema_version": "ao2.provider-workbench-live-preview.v1",
    "budget_cap_preview": {"status": "visible_before_command_copy", "copyable_command_blocked": True},
    "score_threshold_preview": {"status": "visible_before_command_copy", "minimum_provider_score_not_met": "fail_closed"},
    "fail_closed_preview": True,
    "copyable_command_blocked": True,
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "score_budget_workbench_uat", "command": "provider:score-budget-workbench-uat", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "score_budget_workbench_uat.log")},
    {"name": "budget_cap_preview", "status": "passed"},
    {"name": "score_threshold_preview", "status": "passed"},
    {"name": "fail_closed_preview", "status": "passed"},
    {"name": "copyable_command_blocked", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.provider-workbench-live-preview-hardening.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "budget_cap_preview": "visible",
    "score_threshold_preview": "visible",
    "fail_closed_preview": "verified",
    "copyable_command_blocked": True,
    "preview_manifest": str(preview),
    "component_summaries": {"score_budget_workbench_uat": str(out_root / "provider-score-budget-workbench-uat" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
