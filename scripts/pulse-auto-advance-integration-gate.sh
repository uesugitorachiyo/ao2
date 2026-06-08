#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_AUTO_ADVANCE_INTEGRATION_GATE_ROOT:-$ROOT/target/pulse-auto-advance-integration-gate/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" pulse_pr_ci_gate_update npm run pulse:pr-ci-gate:update
ao2_gate_run_step "$LOG_DIR" pulse_resume_workspace_cli_fallback npm run pulse:resume-workspace-cli-fallback
ao2_gate_run_step "$LOG_DIR" pulse_terminal_eval_loop_schema_compatibility npm run pulse:terminal-eval-loop-schema-compatibility
ao2_gate_run_step "$LOG_DIR" pulse_auto_advance_runner_contract npm run pulse:auto-advance-runner-contract
ao2_gate_run_step "$LOG_DIR" pulse_stop_and_dedup_ledger npm run pulse:stop-and-dedup-ledger

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
components = [
    ("pulse_pr_ci_gate_update", "pulse:pr-ci-gate:update", "ao2.pulse-pr-ci-gate-update.v1"),
    ("pulse_resume_workspace_cli_fallback", "pulse:resume-workspace-cli-fallback", "ao2.pulse-resume-workspace-cli-fallback.v1"),
    ("pulse_terminal_eval_loop_schema_compatibility", "pulse:terminal-eval-loop-schema-compatibility", "ao2.pulse-terminal-eval-loop-schema-compatibility.v1"),
    ("pulse_auto_advance_runner_contract", "pulse:auto-advance-runner-contract", "ao2.pulse-auto-advance-runner-contract.v1"),
    ("pulse_stop_and_dedup_ledger", "pulse:stop-and-dedup-ledger", "ao2.pulse-stop-and-dedup-ledger.v1"),
]
checks = []
for name, command, schema in components:
    code_path = log_dir / f"{name}.log.exit-code"
    code = int(code_path.read_text(encoding="utf-8").strip())
    checks.append({
        "name": name,
        "command": command,
        "expected_schema": schema,
        "status": "passed" if code == 0 else "failed",
        "exit_code": code,
        "log": str(log_dir / f"{name}.log"),
    })
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-auto-advance-integration-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
