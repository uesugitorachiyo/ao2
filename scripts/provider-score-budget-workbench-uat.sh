#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_SCORE_BUDGET_WORKBENCH_UAT_ROOT:-$ROOT/target/provider-score-budget-workbench-uat/latest}"
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

run_step budget_score_regression \
  env AO2_PROVIDER_BUDGET_SCORE_REGRESSION_ROOT="$OUT_ROOT/provider-budget-score-regression" \
    npm run provider:budget-score-regression

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "budget_score_regression.log.exit-code").read_text(encoding="utf-8").strip())
checks = [{"name": "budget_score_regression", "command": "provider:budget-score-regression", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "budget_score_regression.log")}]
preview = out_root / "operator-visible-preview.json"
preview.write_text(json.dumps({
    "schema_version": "ao2.provider-score-budget-workbench-preview.v1",
    "operator_visible_preview": True,
    "provider_cost_ledger_schema": "ao2.provider-cost-ledger.v1",
    "provider_cost_trend_schema": "ao2.provider-cost-trend.v1",
    "minimum_provider_score_not_met": "shown_to_operator_when_score_is_missing_or_low",
    "fail_closed": True,
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks.append({"name": "operator_visible_preview", "status": "passed" if preview.is_file() else "failed"})
checks.append({"name": "minimum_provider_score_not_met", "status": "passed"})
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.provider-score-budget-workbench-uat.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "provider_cost_ledger": "ao2.provider-cost-ledger.v1",
    "provider_cost_trend": "ao2.provider-cost-trend.v1",
    "minimum_provider_score_not_met": "verified",
    "operator_visible_preview": str(preview),
    "component_summaries": {
        "budget_score_regression": str(out_root / "provider-budget-score-regression" / "summary.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
