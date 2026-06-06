#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_DASHBOARD_VISUAL_BASELINE_ROOT:-$ROOT/target/dashboard-visual-baseline-lock/latest}"
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

run_step dashboard_screenshot_regression_suite \
  env AO2_DASHBOARD_SCREENSHOT_REGRESSION_ROOT="$OUT_ROOT/dashboard-screenshot-regression-suite" \
    npm run evidence:dashboard-screenshot-regression-suite

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "dashboard_screenshot_regression_suite.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "dashboard-screenshot-regression-suite" / "summary.json"
baseline_digest = hashlib.sha256(component.read_bytes()).hexdigest() if component.is_file() else None
visual_baseline_manifest = out_root / "visual-baseline-manifest.json"
visual_baseline_manifest.write_text(json.dumps({
    "schema_version": "ao2.dashboard-visual-baseline-manifest.v1",
    "visual_baseline_manifest": "locked",
    "baseline_digest": baseline_digest,
    "future_comparison_contract": "compare_future_browser_backed_dashboard_qa_against_this_digest",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "dashboard_screenshot_regression_suite", "command": "evidence:dashboard-screenshot-regression-suite", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "dashboard_screenshot_regression_suite.log")},
    {"name": "visual_baseline_manifest", "status": "passed" if visual_baseline_manifest.is_file() else "failed"},
    {"name": "baseline_digest", "status": "passed" if baseline_digest else "failed"},
    {"name": "future_comparison_contract", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.dashboard-visual-baseline-lock.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "visual_baseline_manifest": str(visual_baseline_manifest),
    "baseline_digest": baseline_digest,
    "future_comparison_contract": "compare_future_browser_backed_dashboard_qa_against_this_digest",
    "component_summaries": {"dashboard_screenshot_regression_suite": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
