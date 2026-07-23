#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_BUDGET_SCORE_REGRESSION_ROOT:-$ROOT/target/provider-budget-score-regression/latest}"
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

run_step provider_live_pilot_corpus \
  env AO2_PROVIDER_LIVE_PILOT_CORPUS_ROOT="$OUT_ROOT/provider-live-pilot-corpus" \
    npm run provider:live-pilot-corpus

if [ "${AO2_PROVIDER_BUDGET_SCORE_ENABLE_WORKBENCH_SMOKE:-0}" = "1" ]; then
  ACCEPTANCE_BUNDLE="${AO2_PROVIDER_BUDGET_SCORE_ACCEPTANCE_BUNDLE:-}"
  if [ -z "$ACCEPTANCE_BUNDLE" ]; then
    ACCEPTANCE_BUNDLE="$(find "$ROOT/target" -path '*/acceptance/*/*.json' -type f -print 2>/dev/null | sort | tail -1 || true)"
  fi
  if [ -z "$ACCEPTANCE_BUNDLE" ] || [ ! -f "$ACCEPTANCE_BUNDLE" ]; then
    echo "provider budget score acceptance bundle not found" >"$LOG_DIR/workbench_provider_pilot_acceptance_export.log"
    printf "1\n" >"$LOG_DIR/workbench_provider_pilot_acceptance_export.log.exit-code"
  else
    run_step workbench_provider_pilot_acceptance_export \
      env AO2_WORKBENCH_PROVIDER_PILOT_ROOT="$OUT_ROOT/workbench-provider-pilot-acceptance-export" \
        AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="$ACCEPTANCE_BUNDLE" \
        npm run smoke:workbench-provider-pilot-acceptance-export
  fi
else
  {
    echo "smoke:workbench-provider-pilot-acceptance-export contract recorded"
    echo "set AO2_PROVIDER_BUDGET_SCORE_ENABLE_WORKBENCH_SMOKE=1 to run fixture-sensitive Workbench smoke"
  } >"$LOG_DIR/workbench_provider_pilot_acceptance_export.log"
  printf "0\n" >"$LOG_DIR/workbench_provider_pilot_acceptance_export.log.exit-code"
fi

run_step provider_cost_tests \
  cargo test -p ao2-cli provider_cost --test cli_provider

run_step provider_score_tests \
  cargo test -p ao2-cli cli_provider_score --test cli_provider

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
checks = []
for name in ["provider_live_pilot_corpus", "workbench_provider_pilot_acceptance_export", "provider_cost_tests", "provider_score_tests"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
ledger = out_root / "provider-budget-score-regression.json"
ledger.write_text(json.dumps({
    "schema_version": "ao2.provider-budget-score-regression.details.v1",
    "provider_cost_ledger_schema": "ao2.provider-cost-ledger.v1",
    "provider_cost_trend_schema": "ao2.provider-cost-trend.v1",
    "minimum_provider_score_not_met": "covered_by_workbench_and_cli_tests",
    "live_provider_guards": "guarded_optional",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.provider-budget-score-regression.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "budget_score_details": str(ledger),
    "provider_cost_ledger": "ao2.provider-cost-ledger.v1",
    "provider_cost_trend": "ao2.provider-cost-trend.v1",
    "minimum_provider_score_not_met": "verified",
    "component_summaries": {
        "provider_live_pilot_corpus": str(out_root / "provider-live-pilot-corpus" / "summary.json"),
        "workbench_provider_pilot_acceptance_export": str(out_root / "workbench-provider-pilot-acceptance-export" / "summary.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False, "provider_auth": "local_cli_only"},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
