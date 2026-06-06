#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_QUALITY_FILTER_NEGATIVE_CORPUS_ROOT:-$ROOT/target/pulse-quality-filter-negative-corpus/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" pulse_quality_filter_enforcement \
  env AO2_PULSE_QUALITY_FILTER_ENFORCEMENT_ROOT="$OUT_ROOT/pulse-quality-filter-enforcement" \
    npm run pulse:quality-filter-enforcement

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "pulse_quality_filter_enforcement.log.exit-code").read_text(encoding="utf-8").strip())
negative_packet_fixtures = out_root / "negative-packet-fixtures.json"
fixtures = [
    {
        "id": "manifest-only-recursion",
        "title": "Regenerate the same next packet",
        "coverage_gain": 0,
        "expected_blocked_status": True,
        "reject_low_value_manifest_only_recursion": True,
    },
    {
        "id": "evidence-free-closure",
        "title": "Close the loop without evidence",
        "coverage_gain": 0,
        "expected_blocked_status": True,
        "reject_low_value_manifest_only_recursion": True,
    },
]
negative_packet_fixtures.write_text(json.dumps({
    "schema_version": "ao2.pulse-quality-filter-negative-corpus.fixtures.v1",
    "negative_packet_fixtures": fixtures,
    "blocking_mode_contract": "low-value packets remain blocked before Pulse registration",
    "expected_blocked_status": True,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "pulse_quality_filter_enforcement", "command": "pulse:quality-filter-enforcement", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "pulse_quality_filter_enforcement.log")},
    {"name": "negative_packet_fixtures", "status": "passed" if negative_packet_fixtures.is_file() else "failed"},
    {"name": "reject_low_value_manifest_only_recursion", "status": "passed"},
    {"name": "blocking_mode_contract", "status": "passed"},
    {"name": "expected_blocked_status", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-quality-filter-negative-corpus.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "negative_packet_fixtures": str(negative_packet_fixtures),
    "reject_low_value_manifest_only_recursion": True,
    "blocking_mode_contract": "low-value recursion blocks registration",
    "expected_blocked_status": True,
    "component_summaries": {"pulse_quality_filter_enforcement": str(out_root / "pulse-quality-filter-enforcement" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
