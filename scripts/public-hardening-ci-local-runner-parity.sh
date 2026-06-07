#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_HARDENING_CI_PARITY_ROOT:-$ROOT/target/public-hardening-ci-local-runner-parity/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" public_hardening_workflow_tracked_proposal \
  env AO2_PUBLIC_HARDENING_WORKFLOW_TRACKED_PROPOSAL_ROOT="$OUT_ROOT/public-hardening-workflow-tracked-proposal" \
    npm run public:hardening-workflow-tracked-proposal

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "public_hardening_workflow_tracked_proposal.log.exit-code").read_text(encoding="utf-8").strip())
workflow = out_root / "public-hardening-workflow-tracked-proposal" / "tracked-workflow-proposal.yml"
workflow_text = workflow.read_text(encoding="utf-8") if workflow.is_file() else ""
workflow_command_set = re.findall(r"- run: (.+)", workflow_text)
local_command_set = [
    "AO2_PULSE_GENERATE_NEXT_REGISTER=0 npm run pulse:generate-next",
    "AO2_PULSE_LOCAL_MIRROR_SOURCE=target/pulse-next-recommended-tasks/generated-next npm run pulse:local-mirror",
    "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
    "npm run public:hardening",
    "npm run pulse:resume -- --dry-run",
]
missing_from_ci = [command for command in local_command_set if command not in workflow_command_set]
extra_ci_commands = [command for command in workflow_command_set if command.startswith("npm run") and command not in local_command_set and command != "npm ci"]
parity_matrix = out_root / "parity-matrix.json"
parity_matrix.write_text(json.dumps({
    "schema_version": "ao2.public-hardening-ci-local-runner-parity.matrix.v1",
    "workflow_command_set": workflow_command_set,
    "local_command_set": local_command_set,
    "missing_from_ci": missing_from_ci,
    "extra_ci_commands": extra_ci_commands,
    "parity_matrix": [{"command": command, "present_in_ci": command in workflow_command_set} for command in local_command_set],
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "public_hardening_workflow_tracked_proposal", "command": "public:hardening-workflow-tracked-proposal", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "public_hardening_workflow_tracked_proposal.log")},
    {"name": "workflow_command_set", "status": "passed" if workflow_command_set else "failed"},
    {"name": "local_command_set", "status": "passed"},
    {"name": "parity_matrix", "status": "passed" if parity_matrix.is_file() else "failed"},
    {"name": "missing_from_ci", "status": "passed" if not missing_from_ci else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.public-hardening-ci-local-runner-parity.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "workflow_command_set": workflow_command_set,
    "local_command_set": local_command_set,
    "parity_matrix": str(parity_matrix),
    "missing_from_ci": missing_from_ci,
    "component_summaries": {"public_hardening_workflow_tracked_proposal": str(out_root / "public-hardening-workflow-tracked-proposal" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
