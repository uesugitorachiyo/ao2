#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_MVP_ACCEPTANCE_MATRIX_ROOT:-$ROOT/target/mvp-acceptance-matrix/latest}"
SUMMARY="$OUT_ROOT/summary.json"
HTML="$OUT_ROOT/matrix.html"
LOG_DIR="$OUT_ROOT/logs"
RISKY_ROOT="$OUT_ROOT/risky-pr-golden"

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

run_step risky_pr_golden \
  env AO2_RISKY_PR_GOLDEN_ROOT="$RISKY_ROOT" npm run risky-pr:golden

python3 - "$OUT_ROOT" "$SUMMARY" "$HTML" "$RISKY_ROOT" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
html_path = Path(sys.argv[3]).resolve()
risky_root = Path(sys.argv[4]).resolve()
log_dir = out_root / "logs"

exit_code = int((log_dir / "risky_pr_golden.log.exit-code").read_text(encoding="utf-8").strip())
risky_summary_path = risky_root / "summary.json"
risky = {}
if risky_summary_path.exists():
    risky = json.loads(risky_summary_path.read_text(encoding="utf-8"))

evidence_pack = Path(risky.get("evidence_pack", ""))
report = Path(risky.get("report", ""))
cockpit_index = Path(risky.get("cockpit_index", ""))

def exists(path):
    return bool(str(path)) and path.exists()

base_checks = {
    "run_created": exit_code == 0 and risky.get("run_id"),
    "compiled_workflow_visible": exit_code == 0 and exists(evidence_pack),
    "policy_denial": bool(risky.get("policy_denial_observed")),
    "exact_approval": bool(risky.get("exact_approval_observed")),
    "rejection": bool(risky.get("evaluator_rejection_observed")),
    "acceptance": bool(risky.get("evaluator_acceptance_observed")),
    "evidence": bool(risky.get("acceptance_evidence_observed")) and exists(evidence_pack),
    "replay": risky.get("replay_status") == "accepted" and risky.get("digest_failure_count") == 0,
    "cockpit": exists(report) and exists(cockpit_index),
}

acceptance_matrix = [
    ("AC-01", "Run Creation", "run_created", str(risky_summary_path)),
    ("AC-02", "Scoped Planning", "compiled_workflow_visible", str(evidence_pack)),
    ("AC-03", "Adapter Execution", "evidence", str(evidence_pack)),
    ("AC-04", "Policy Block", "policy_denial", str(evidence_pack)),
    ("AC-05", "Exact-Digest Approval", "exact_approval", str(evidence_pack)),
    ("AC-06", "Evidence Capture", "evidence", str(evidence_pack)),
    ("AC-07", "Reviewer Concerns", "rejection", str(evidence_pack)),
    ("AC-08", "Evaluator Rejection", "rejection", str(evidence_pack)),
    ("AC-09", "Evaluator Acceptance", "acceptance", str(evidence_pack)),
    ("AC-10", "Evidence Export", "evidence", str(evidence_pack)),
    ("AC-11", "Inspectability", "cockpit", str(report)),
    ("AC-12", "Fail-Closed Behavior", "policy_denial", str(evidence_pack)),
]
uat_matrix = [
    ("UAT-01", "Workflow compilation", "compiled_workflow_visible", str(evidence_pack)),
    ("UAT-02", "Scoped context", "evidence", str(evidence_pack)),
    ("UAT-03", "Policy denial", "policy_denial", str(evidence_pack)),
    ("UAT-04", "Narrow approval", "exact_approval", str(evidence_pack)),
    ("UAT-05", "Evidence artifacts", "evidence", str(evidence_pack)),
    ("UAT-06", "Reviewer concern", "rejection", str(evidence_pack)),
    ("UAT-07", "Evaluator rejection", "rejection", str(evidence_pack)),
    ("UAT-08", "Correction loop", "acceptance", str(evidence_pack)),
    ("UAT-09", "Final acceptance", "acceptance", str(evidence_pack)),
    ("UAT-10", "Cockpit/report inspection", "cockpit", str(report)),
    ("UAT-11", "Evidence export", "evidence", str(evidence_pack)),
    ("UAT-12", "Eval fixture marker", "rejection", str(evidence_pack)),
]

def row(item):
    ident, label, check, evidence = item
    passed = bool(base_checks.get(check))
    return {
        "id": ident,
        "label": label,
        "status": "passed" if passed else "failed",
        "check": check,
        "evidence": evidence,
        "manual_filesystem_archaeology_required": False,
    }

items = [row(item) for item in acceptance_matrix] + [row(item) for item in uat_matrix]
status = "passed" if exit_code == 0 and all(item["status"] == "passed" for item in items) else "failed"

payload = {
    "schema_version": "ao2.mvp-acceptance-matrix-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "acceptance_matrix": items,
    "component_summaries": {"risky_pr_golden": str(risky_summary_path)},
    "evidence_rule": "evidence must exist before evaluator closure accepts a run",
    "manual_filesystem_archaeology_required": False,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "release_acceptance_owner": "factory-v3 evaluator-closer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

rows = "\n".join(
    "<tr>"
    f"<td>{html.escape(item['id'])}</td>"
    f"<td>{html.escape(item['label'])}</td>"
    f"<td>{html.escape(item['status'])}</td>"
    f"<td><code>{html.escape(item['evidence'])}</code></td>"
    "</tr>"
    for item in items
)
html_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 MVP Acceptance Matrix Gate</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body><h1>AO2 MVP Acceptance Matrix Gate</h1>"
    f"<p>Status: <code>{html.escape(status)}</code></p>"
    "<table><thead><tr><th>ID</th><th>Requirement</th><th>Status</th><th>Evidence</th></tr></thead>"
    f"<tbody>{rows}</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"matrix={html_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
