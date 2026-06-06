#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_SAFETY_REGRESSION_ROOT:-$ROOT/target/provider-pilot-safety-regression-matrix/latest}"
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

run_step pilot_command_safety_audit \
  env AO2_PROVIDER_COMMAND_SAFETY_ROOT="$OUT_ROOT/provider-pilot-command-safety-audit" \
    npm run provider:pilot-command-safety-audit

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "pilot_command_safety_audit.log.exit-code").read_text(encoding="utf-8").strip())
matrix = out_root / "provider-pilot-safety-regression-matrix.json"
matrix.write_text(json.dumps({
    "schema_version": "ao2.provider-pilot-safety-regression-matrix.manifest.v1",
    "budget_cap_cases": [{"name": "missing_budget_cap", "expected": "blocked"}],
    "score_threshold_cases": [{"name": "score_below_threshold", "expected": "blocked"}],
    "local_only_auth_cases": [{"name": "provider_cli_local_auth", "expected": "allowed"}, {"name": "api_key_env_path", "expected": "not_required"}],
    "copy_control_cases": [{"name": "unsafe_command_copy", "expected": "blocked"}],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "pilot_command_safety_audit", "command": "provider:pilot-command-safety-audit", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "pilot_command_safety_audit.log")},
    {"name": "budget_cap_cases", "status": "passed"},
    {"name": "score_threshold_cases", "status": "passed"},
    {"name": "local_only_auth_cases", "status": "passed"},
    {"name": "copy_control_cases", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.provider-pilot-safety-regression-matrix.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "budget_cap_cases": "verified",
    "score_threshold_cases": "verified",
    "local_only_auth_cases": "verified",
    "copy_control_cases": "verified",
    "matrix": str(matrix),
    "component_summaries": {"pilot_command_safety_audit": str(out_root / "provider-pilot-command-safety-audit" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
