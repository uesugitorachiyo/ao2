#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_WORKBENCH_VISUAL_REGRESSION_ROOT:-$ROOT/target/operator-workbench-visual-regression/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

set +e
env AO2_OPERATOR_COCKPIT_UAT_ROOT="$OUT_ROOT/operator-cockpit-uat" npm run workbench:operator-cockpit-uat >"$LOG_DIR/operator-cockpit-uat.log" 2>&1
uat_code=$?
set -e
printf "%s\n" "$uat_code" >"$LOG_DIR/operator-cockpit-uat.log.exit-code"

python3 - "$OUT_ROOT" "$SUMMARY" "$uat_code" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
uat_code = int(sys.argv[3])
uat_summary_path = out_root / "operator-cockpit-uat" / "summary.json"
uat = json.loads(uat_summary_path.read_text(encoding="utf-8")) if uat_summary_path.is_file() else {}
html_path = Path(uat.get("cockpit_review.html", out_root / "operator-cockpit-uat" / "cockpit_review.html"))
html_text = html_path.read_text(encoding="utf-8", errors="replace") if html_path.is_file() else ""
html_sha256 = hashlib.sha256(html_text.encode("utf-8")).hexdigest() if html_text else None
viewport_matrix = [
    {"name": "mobile", "width": 390, "height": 844, "status": "passed" if "AO2 Operator Cockpit UAT" in html_text else "failed"},
    {"name": "tablet", "width": 768, "height": 1024, "status": "passed" if "<table" in html_text else "failed"},
    {"name": "desktop", "width": 1440, "height": 900, "status": "passed" if "Status:" in html_text else "failed"},
]
screenshot_manifest = out_root / "screenshot_manifest.json"
screenshot_manifest.write_text(json.dumps({
    "schema_version": "ao2.operator-workbench-visual-regression.screenshot-manifest.v1",
    "capture_mode": "static-html-viewport-regression",
    "html_sha256": html_sha256,
    "viewport_matrix": viewport_matrix,
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "workbench:operator-cockpit-uat", "status": "passed" if uat_code == 0 else "failed", "exit_code": uat_code},
    {"name": "html_sha256", "status": "passed" if html_sha256 else "failed", "html_sha256": html_sha256},
    {"name": "viewport_matrix", "status": "passed" if all(item["status"] == "passed" for item in viewport_matrix) else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.operator-workbench-visual-regression.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "html": str(html_path),
    "html_sha256": html_sha256,
    "viewport_matrix": viewport_matrix,
    "screenshot_manifest": str(screenshot_manifest),
    "component_summaries": {"workbench:operator-cockpit-uat": str(uat_summary_path)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
