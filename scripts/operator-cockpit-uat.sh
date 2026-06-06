#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_OPERATOR_COCKPIT_UAT_ROOT:-$ROOT/target/operator-cockpit-uat/latest}"
SUMMARY="$OUT_ROOT/summary.json"
HTML="$OUT_ROOT/cockpit_review.html"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

set +e
env AO2_WORKBENCH_BROWSER_QA_ROOT="$OUT_ROOT/workbench-browser-qa" npm run workbench:browser-qa >"$LOG_DIR/workbench-browser-qa.log" 2>&1
qa_code=$?
set -e
printf "%s\n" "$qa_code" >"$LOG_DIR/workbench-browser-qa.log.exit-code"

python3 - "$OUT_ROOT" "$SUMMARY" "$HTML" "$qa_code" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
html_path = Path(sys.argv[3]).resolve()
qa_code = int(sys.argv[4])
qa_summary_path = out_root / "workbench-browser-qa" / "summary.json"
qa_summary = json.loads(qa_summary_path.read_text(encoding="utf-8")) if qa_summary_path.is_file() else {}
required_operator_decisions = [
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
question_status = qa_summary.get("question_status", {})
answers_present = all(question_status.get(item) in {"answered", "passed"} for item in required_operator_decisions)
manual_fs = qa_summary.get("html_inspection", {}).get("manual_filesystem_archaeology_required")
checks = [
    {"name": "workbench_browser_qa", "status": "passed" if qa_code == 0 else "failed"},
    {"name": "required_operator_decisions", "status": "passed" if answers_present else "failed"},
    {"name": "manual_filesystem_archaeology_required", "status": "passed" if manual_fs is False else "failed", "observed": manual_fs},
]
status = "passed" if all(check["status"] == "passed" for check in checks) else "failed"
rows = "\n".join(
    f"<tr><td>{html.escape(name)}</td><td>{html.escape(str(question_status.get(name)))}</td></tr>"
    for name in required_operator_decisions
)
html_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Operator Cockpit UAT</title></head><body>"
    "<h1>AO2 Operator Cockpit UAT</h1>"
    f"<p>Status: <code>{html.escape(status)}</code></p>"
    "<table><thead><tr><th>Decision</th><th>Evidence</th></tr></thead>"
    f"<tbody>{rows}</tbody></table></body></html>\n",
    encoding="utf-8",
)
payload = {
    "schema_version": "ao2.operator-cockpit-uat.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "cockpit_review.html": str(html_path),
    "required_operator_decisions": required_operator_decisions,
    "question_status": question_status,
    "checks": checks,
    "component_summaries": {"workbench:browser-qa": str(qa_summary_path)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"cockpit_review={html_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
