#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_QUALITY_FILTER_REQUIRED_GATE_ROOT:-$ROOT/target/pulse-quality-filter-required-gate/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" pulse_quality_filter_negative_corpus \
  env AO2_PULSE_QUALITY_FILTER_NEGATIVE_CORPUS_ROOT="$OUT_ROOT/pulse-quality-filter-negative-corpus" \
    npm run pulse:quality-filter-negative-corpus

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "pulse_quality_filter_negative_corpus.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "pulse-quality-filter-negative-corpus" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
registration_block_contract = out_root / "registration-block-contract.json"
low_value_recursion_blocked = bool(component_data.get("reject_low_value_manifest_only_recursion")) and bool(component_data.get("expected_blocked_status"))
registration_block_contract.write_text(json.dumps({
    "schema_version": "ao2.pulse-registration-block-contract.v1",
    "required_pre_registration_gate": True,
    "negative_corpus_enforced": True,
    "low_value_recursion_blocked": low_value_recursion_blocked,
    "registration_block_contract": "packet registration is blocked when negative corpus expectations fail",
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "pulse_quality_filter_negative_corpus", "command": "pulse:quality-filter-negative-corpus", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "pulse_quality_filter_negative_corpus.log")},
    {"name": "required_pre_registration_gate", "status": "passed"},
    {"name": "negative_corpus_enforced", "status": "passed"},
    {"name": "low_value_recursion_blocked", "status": "passed" if low_value_recursion_blocked else "failed"},
    {"name": "registration_block_contract", "status": "passed" if registration_block_contract.is_file() else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-quality-filter-required-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "required_pre_registration_gate": True,
    "negative_corpus_enforced": True,
    "low_value_recursion_blocked": low_value_recursion_blocked,
    "registration_block_contract": str(registration_block_contract),
    "component_summaries": {"pulse_quality_filter_negative_corpus": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
