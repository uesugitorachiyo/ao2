#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RISKY_PR_PRODUCT_READINESS_ROOT:-$ROOT/target/risky-pr-product-readiness/latest}"
SUMMARY="$OUT_ROOT/summary.json"
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

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$RISKY_ROOT" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
risky_root = Path(sys.argv[4]).resolve()
risky_summary_path = risky_root / "summary.json"
risky_exit = int((log_dir / "risky_pr_golden.log.exit-code").read_text(encoding="utf-8").strip())
risky_summary = json.loads(risky_summary_path.read_text(encoding="utf-8")) if risky_summary_path.is_file() else {}
evidence_pack = Path(str(risky_summary.get("evidence_pack", "")))
report = Path(str(risky_summary.get("report", "")))
cockpit_index = Path(str(risky_summary.get("cockpit_index", "")))

evidence = {}
if evidence_pack.is_file():
    evidence = json.loads(evidence_pack.read_text(encoding="utf-8"))

local_run_record = (
    risky_exit == 0
    and risky_summary.get("schema_version") == "ao2.risky-pr-golden-path.v1"
    and evidence.get("schema_version") == "ao2.evidence-pack.v1"
    and evidence.get("run_id") == risky_summary.get("run_id")
    and evidence.get("verdict") == "accepted"
)
static_report_export = (
    report.is_file()
    and cockpit_index.is_file()
    and str(risky_summary.get("replay_status")) == "accepted"
    and int(risky_summary.get("digest_failure_count") or 0) == 0
)
evaluator_closure_evidence = all(bool(risky_summary.get(name)) for name in [
    "policy_denial_observed",
    "exact_approval_observed",
    "evaluator_rejection_observed",
    "evaluator_acceptance_observed",
    "acceptance_evidence_observed",
])

checks = [
    {
        "name": "risky_pr_golden",
        "command": "risky-pr:golden",
        "status": "passed" if risky_exit == 0 else "failed",
        "exit_code": risky_exit,
        "log": str(log_dir / "risky_pr_golden.log"),
        "evidence": str(risky_summary_path),
    },
    {
        "name": "local_run_record",
        "status": "passed" if local_run_record else "failed",
        "evidence": str(evidence_pack),
        "schema": evidence.get("schema_version"),
    },
    {
        "name": "static_report_export",
        "status": "passed" if static_report_export else "failed",
        "report": str(report),
        "cockpit_index": str(cockpit_index),
    },
    {
        "name": "evaluator_closure_evidence",
        "status": "passed" if evaluator_closure_evidence else "failed",
        "evidence": str(risky_summary_path),
    },
    {
        "name": "manual_filesystem_archaeology_required",
        "status": "passed",
        "value": False,
    },
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.risky-pr-product-readiness-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "risky_pr_golden_summary": str(risky_summary_path),
    "local_run_record": local_run_record,
    "static_report_export": static_report_export,
    "evaluator_closure_evidence": evaluator_closure_evidence,
    "manual_filesystem_archaeology_required": False,
    "checks": checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
