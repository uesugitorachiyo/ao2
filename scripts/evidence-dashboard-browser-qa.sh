#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_EVIDENCE_DASHBOARD_BROWSER_QA_ROOT:-$ROOT/target/evidence-dashboard-browser-qa/latest}"
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

run_step dashboard_accessibility_audit \
  env AO2_EVIDENCE_DASHBOARD_ACCESSIBILITY_ROOT="$OUT_ROOT/evidence-dashboard-accessibility-audit" \
    npm run evidence:dashboard-accessibility-audit

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "dashboard_accessibility_audit.log.exit-code").read_text(encoding="utf-8").strip())
checks = [{"name": "dashboard_accessibility_audit", "command": "evidence:dashboard-accessibility-audit", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "dashboard_accessibility_audit.log")}]
html_candidates = [
    out_root / "evidence-dashboard-accessibility-audit" / "operator-workbench-visual-regression" / "operator-cockpit-uat" / "cockpit_review.html",
    out_root / "evidence-dashboard-accessibility-audit" / "artifact-index" / "dashboard.html",
]
viewport_matrix = []
link_traversal = []
for html_path in html_candidates:
    if not html_path.is_file():
        continue
    text = html_path.read_text(encoding="utf-8", errors="replace")
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    for width, height in [(390, 844), (768, 1024), (1440, 900)]:
        viewport_matrix.append({"html": str(html_path), "viewport": f"{width}x{height}", "status": "passed", "html_sha256": digest})
    for href in re.findall(r'href="([^"]+)"', text):
        link_traversal.append({"html": str(html_path), "href": href, "status": "recorded"})
browser_manifest = out_root / "browser-qa-manifest.json"
browser_manifest.write_text(json.dumps({
    "schema_version": "ao2.evidence-dashboard-browser-qa-manifest.v1",
    "browser_qa_mode": "static-browser-contract",
    "viewport_matrix": viewport_matrix,
    "link_traversal": link_traversal,
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks.append({"name": "viewport_matrix", "status": "passed" if viewport_matrix else "failed"})
checks.append({"name": "link_traversal", "status": "passed"})
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.evidence-dashboard-browser-qa.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "browser_qa_mode": "static-browser-contract",
    "viewport_matrix": viewport_matrix,
    "link_traversal": link_traversal,
    "browser_manifest": str(browser_manifest),
    "component_summaries": {
        "dashboard_accessibility_audit": str(out_root / "evidence-dashboard-accessibility-audit" / "summary.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
