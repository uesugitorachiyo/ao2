#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_WORKBENCH_BROWSER_QA_ROOT:-$ROOT/target/workbench-browser-qa/latest}"
SUMMARY="$OUT_ROOT/summary.json"
AUDIT_ROOT="$OUT_ROOT/no-archaeology-workbench"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

env AO2_NO_ARCHAEOLOGY_WORKBENCH_ROOT="$AUDIT_ROOT" npm run workbench:no-archaeology-audit >"$OUT_ROOT/no-archaeology.log" 2>&1

python3 - "$OUT_ROOT" "$SUMMARY" "$AUDIT_ROOT/summary.json" <<'PY'
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
audit_summary_path = Path(sys.argv[3]).resolve()
audit = json.loads(audit_summary_path.read_text(encoding="utf-8"))
workbench = Path(audit["workbench_export"])
html_text = workbench.read_text(encoding="utf-8", errors="replace")
questions = [
    "objective",
    "denied_action",
    "approved_digest",
    "changed_files",
    "test_evidence",
    "rejection_reason",
    "correction",
    "closure_verdict",
    "export_path",
    "replay_status",
]
question_status = {item["question"]: item["status"] for item in audit.get("answers", [])}
html_inspection = {
    "workbench_export": str(workbench),
    "contains_doctype_or_html": bool(re.search(r"<!doctype html|<html", html_text, re.I)),
    "contains_all_reviewer_questions": all(q in html_text or q in question_status for q in questions),
    "manual_filesystem_archaeology_required": audit.get("manual_filesystem_archaeology_required"),
}
screenshot_manifest = out_root / "screenshot-manifest.json"
screenshot_manifest.write_text(json.dumps({
    "schema_version": "ao2.workbench-browser-qa.screenshot-manifest.v1",
    "browser_review": "static-html-inspection",
    "captures": [
        {"label": "workbench", "path": str(workbench), "status": "html_inspected"}
    ],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
status = "passed" if audit.get("status") == "passed" and html_inspection["contains_doctype_or_html"] and html_inspection["contains_all_reviewer_questions"] and audit.get("manual_filesystem_archaeology_required") is False else "failed"
payload = {
    "schema_version": "ao2.workbench-browser-qa.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "browser_review": "static-html-inspection",
    "html_inspection": html_inspection,
    "screenshot_manifest": str(screenshot_manifest),
    "required_questions": questions,
    "question_status": question_status,
    "component_summaries": {"workbench:no-archaeology-audit": str(audit_summary_path)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
