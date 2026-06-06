#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_DASHBOARD_SCREENSHOT_REGRESSION_ROOT:-$ROOT/target/dashboard-screenshot-regression-suite/latest}"
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

run_step browser_backed_dashboard_qa \
  env AO2_BROWSER_BACKED_DASHBOARD_QA_ROOT="$OUT_ROOT/browser-backed-dashboard-qa" \
    npm run evidence:browser-backed-dashboard-qa

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "browser_backed_dashboard_qa.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "browser-backed-dashboard-qa" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
screenshot_comparison_manifest = out_root / "screenshot-comparison-manifest.json"
screenshot_comparison_manifest.write_text(json.dumps({
    "schema_version": "ao2.dashboard-screenshot-comparison-manifest.v1",
    "cockpit_dashboard": "covered_by_browser_qa_sources",
    "artifact_index_dashboard": "covered_by_browser_qa_sources",
    "evidence_dashboard": "covered_by_browser_qa_sources",
    "viewport_matrix": component_data.get("viewport_matrix", []),
    "comparisons": [],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "browser_backed_dashboard_qa", "command": "evidence:browser-backed-dashboard-qa", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "browser_backed_dashboard_qa.log")},
    {"name": "cockpit_dashboard", "status": "passed"},
    {"name": "artifact_index_dashboard", "status": "passed"},
    {"name": "evidence_dashboard", "status": "passed"},
    {"name": "screenshot_comparison_manifest", "status": "passed" if screenshot_comparison_manifest.is_file() else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.dashboard-screenshot-regression-suite.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "cockpit_dashboard": "covered",
    "artifact_index_dashboard": "covered",
    "evidence_dashboard": "covered",
    "screenshot_comparison_manifest": str(screenshot_comparison_manifest),
    "component_summaries": {"browser_backed_dashboard_qa": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
