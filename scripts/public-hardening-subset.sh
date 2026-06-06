#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_HARDENING_ROOT:-$ROOT/target/public-hardening-subset/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" public_stabilization_tests \
  env PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q
ao2_gate_run_step "$LOG_DIR" pulse_resume_dry_run \
  npm run pulse:resume -- --dry-run
ao2_gate_run_step "$LOG_DIR" pulse_lengthy_gate_contract \
  npm run pulse:lengthy-gate:contract
ao2_gate_run_step "$LOG_DIR" bash_syntax_check \
  bash -n scripts/lib/pulse-gate-lib.sh scripts/pulse-shared-gate-lib-audit.sh scripts/public-hardening-subset.sh scripts/operator-evidence-index.sh scripts/script-tracking-intent-audit.sh scripts/pulse-next-task-quality-filter.sh scripts/pulse-lengthy-gate-runner.sh
ao2_gate_forbidden_string_scan "$LOG_DIR" scripts/lib/pulse-gate-lib.sh scripts/pulse-shared-gate-lib-audit.sh scripts/public-hardening-subset.sh scripts/operator-evidence-index.sh scripts/script-tracking-intent-audit.sh scripts/pulse-next-task-quality-filter.sh scripts/pulse-lengthy-gate-runner.sh

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
steps = [
    ("public_stabilization_tests", "test_public_stabilization.py"),
    ("pulse_resume_dry_run", "pulse:resume -- --dry-run"),
    ("pulse_lengthy_gate_contract", "pulse:lengthy-gate:contract"),
    ("bash_syntax_check", "bash_syntax_check"),
    ("forbidden_string_scan", "forbidden_string_scan"),
]
checks = []
for name, label in steps:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "label": label, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.public-hardening-subset.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "stable_subset": ["test_public_stabilization.py", "pulse:resume -- --dry-run", "pulse:lengthy-gate:contract", "bash_syntax_check", "forbidden_string_scan"],
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
