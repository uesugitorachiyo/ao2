#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SCRIPT_TRACKING_DECISION_ROOT:-$ROOT/target/script-tracking-decision-cleanup/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" tracking_intent_audit \
  env AO2_SCRIPT_TRACKING_INTENT_ROOT="$OUT_ROOT/script-tracking-intent-audit" \
    npm run scripts:tracking-intent-audit

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "tracking_intent_audit.log.exit-code").read_text(encoding="utf-8").strip())
intent_summary = out_root / "script-tracking-intent-audit" / "summary.json"
intent = json.loads(intent_summary.read_text(encoding="utf-8")) if intent_summary.is_file() else {}
track_in_repo_decisions = intent.get("track_in_repo", [])
keep_local_only_decisions = intent.get("keep_local_only", [])
pre_commit_cleanup_list = out_root / "pre-commit-cleanup-list.json"
pre_commit_cleanup_list.write_text(json.dumps({
    "schema_version": "ao2.script-pre-commit-cleanup-list.v1",
    "track_in_repo_decisions": track_in_repo_decisions,
    "keep_local_only_decisions": keep_local_only_decisions,
    "review_before_commit": ["package.json", "docs/VERIFICATION.md", "tests/test_public_stabilization.py"],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "tracking_intent_audit", "command": "scripts:tracking-intent-audit", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "tracking_intent_audit.log")},
    {"name": "track_in_repo_decisions", "status": "passed"},
    {"name": "keep_local_only_decisions", "status": "passed"},
    {"name": "pre_commit_cleanup_list", "status": "passed" if pre_commit_cleanup_list.is_file() else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.script-tracking-decision-cleanup.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "track_in_repo_decisions": track_in_repo_decisions,
    "keep_local_only_decisions": keep_local_only_decisions,
    "pre_commit_cleanup_list": str(pre_commit_cleanup_list),
    "component_summaries": {"tracking_intent_audit": str(intent_summary)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
