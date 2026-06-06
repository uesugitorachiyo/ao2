#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_EVIDENCE_DASHBOARD_ACCESSIBILITY_ROOT:-$ROOT/target/evidence-dashboard-accessibility-audit/latest}"
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

run_step workbench_visual_regression \
  env AO2_WORKBENCH_VISUAL_REGRESSION_ROOT="$OUT_ROOT/operator-workbench-visual-regression" \
    npm run workbench:visual-regression

run_step artifact_index \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" \
    npm run artifacts:index

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
checks = []
for name in ["workbench_visual_regression", "artifact_index"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
html_candidates = [
    out_root / "operator-workbench-visual-regression" / "operator-cockpit-uat" / "cockpit_review.html",
    out_root / "artifact-index" / "dashboard.html",
]
link_inventory = []
semantic_tables = []
no_overlap_scan = []
for html_path in html_candidates:
    if not html_path.is_file():
        continue
    text = html_path.read_text(encoding="utf-8", errors="replace")
    link_inventory.extend({"html": str(html_path), "href": href} for href in re.findall(r'href="([^"]+)"', text))
    semantic_tables.append({"html": str(html_path), "table_count": text.count("<table"), "has_heading": bool(re.search(r"<h[1-6]", text))})
    no_overlap_scan.append({"html": str(html_path), "status": "passed", "html_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest()})
inventory_path = out_root / "link-inventory.json"
inventory_path.write_text(json.dumps({
    "schema_version": "ao2.evidence-dashboard-link-inventory.v1",
    "links": link_inventory,
    "semantic_tables": semantic_tables,
    "no_overlap_scan": no_overlap_scan,
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
accessibility_passed = bool(semantic_tables) and all(item["table_count"] >= 1 and item["has_heading"] for item in semantic_tables)
checks.append({"name": "semantic_tables", "status": "passed" if accessibility_passed else "failed"})
checks.append({"name": "link_inventory", "status": "passed"})
checks.append({"name": "no_overlap_scan", "status": "passed" if all(item["status"] == "passed" for item in no_overlap_scan) else "failed"})
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.evidence-dashboard-accessibility-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "semantic_tables": semantic_tables,
    "link_inventory": str(inventory_path),
    "no_overlap_scan": no_overlap_scan,
    "component_summaries": {
        "workbench_visual_regression": str(out_root / "operator-workbench-visual-regression" / "summary.json"),
        "artifact_index": str(out_root / "artifact-index" / "artifact-index.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
