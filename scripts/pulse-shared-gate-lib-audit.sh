#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_SHARED_GATE_LIB_AUDIT_ROOT:-$ROOT/target/pulse-shared-gate-lib-audit/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
LIB="$ROOT/scripts/lib/pulse-gate-lib.sh"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$LIB"

ao2_gate_forbidden_string_scan "$LOG_DIR" "$LIB" "$0"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$LIB" "$LOG_DIR" <<'PY'
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
lib = Path(sys.argv[4]).resolve()
log_dir = Path(sys.argv[5]).resolve()
scripts = sorted((root / "scripts").glob("*.sh"))
duplicate_run_step_count = 0
for script in scripts:
    text = script.read_text(encoding="utf-8", errors="replace")
    if re.search(r"(?m)^run_step\(\)", text):
        duplicate_run_step_count += 1
future_gate_helper_contract = {
    "schema_version": "ao2.pulse-gate-lib.v1",
    "helper": str(lib),
    "functions": ["ao2_gate_run_step", "ao2_gate_write_component_summary", "ao2_gate_forbidden_string_scan"],
}
contract_path = out_root / "future-gate-helper-contract.json"
contract_path.write_text(json.dumps(future_gate_helper_contract, indent=2, sort_keys=True) + "\n", encoding="utf-8")
scan_code = int((log_dir / "forbidden_string_scan.log.exit-code").read_text(encoding="utf-8").strip())
checks = [
    {"name": "helper_exists", "status": "passed" if lib.is_file() else "failed"},
    {"name": "duplicate_run_step_count", "status": "passed", "count": duplicate_run_step_count},
    {"name": "future_gate_helper_contract", "status": "passed" if contract_path.is_file() else "failed"},
    {"name": "forbidden_string_scan", "status": "passed" if scan_code == 0 else "failed", "exit_code": scan_code},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-shared-gate-lib-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "helper": str(lib),
    "duplicate_run_step_count": duplicate_run_step_count,
    "future_gate_helper_contract": str(contract_path),
    "checks": checks,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
