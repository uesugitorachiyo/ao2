#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_BROWSER_BACKED_DASHBOARD_QA_ROOT:-$ROOT/target/browser-backed-dashboard-qa/latest}"
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

run_step dashboard_browser_qa \
  env AO2_EVIDENCE_DASHBOARD_BROWSER_QA_ROOT="$OUT_ROOT/evidence-dashboard-browser-qa" \
    npm run evidence:dashboard-browser-qa

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "dashboard_browser_qa.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "evidence-dashboard-browser-qa" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
screenshot_manifest = out_root / "screenshot-manifest.json"
viewport_matrix = component_data.get("viewport_matrix", [])
screenshot_manifest.write_text(json.dumps({
    "schema_version": "ao2.browser-backed-dashboard-screenshot-manifest.v1",
    "browser_qa_mode": component_data.get("browser_qa_mode", "static-browser-contract"),
    "viewport_matrix": viewport_matrix,
    "screenshots": [
        {"viewport": item.get("viewport"), "status": item.get("status"), "source_html": item.get("html")}
        for item in viewport_matrix
    ],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "dashboard_browser_qa", "command": "evidence:dashboard-browser-qa", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "dashboard_browser_qa.log")},
    {"name": "viewport_matrix", "status": "passed" if viewport_matrix else "failed"},
    {"name": "screenshot_manifest", "status": "passed" if screenshot_manifest.is_file() else "failed"},
    {"name": "link_traversal", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.browser-backed-dashboard-qa.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "browser_qa_mode": component_data.get("browser_qa_mode", "static-browser-contract"),
    "viewport_matrix": viewport_matrix,
    "screenshot_manifest": str(screenshot_manifest),
    "link_traversal": component_data.get("link_traversal", []),
    "component_summaries": {"dashboard_browser_qa": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
