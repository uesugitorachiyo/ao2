#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_HARDENING_CI_WORKFLOW_ROOT:-$ROOT/target/public-hardening-ci-workflow/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" pulse_generate_next_seed \
  env AO2_PULSE_GENERATE_NEXT_ROOT="$OUT_ROOT/pulse-generate-next" \
    AO2_PULSE_GENERATE_NEXT_PACKET_ROOT="$OUT_ROOT/pulse-next-recommended-tasks" \
    AO2_PULSE_GENERATE_NEXT_CURSOR="$OUT_ROOT/pulse-generate-next-cursor.json" \
    AO2_PULSE_GENERATE_NEXT_REGISTER=0 \
    npm run pulse:generate-next
ao2_gate_run_step "$LOG_DIR" pulse_local_mirror_seed \
  env AO2_PULSE_LOCAL_MIRROR_SOURCE="$OUT_ROOT/pulse-next-recommended-tasks" \
    AO2_PULSE_LOCAL_MIRROR_DEST="$OUT_ROOT/pulse-local-mirror" \
    npm run pulse:local-mirror
ao2_gate_run_step "$LOG_DIR" public_hardening \
  env AO2_PUBLIC_HARDENING_ROOT="$OUT_ROOT/public-hardening-subset" \
    npm run public:hardening

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
seed_code = int((log_dir / "pulse_generate_next_seed.log.exit-code").read_text(encoding="utf-8").strip())
mirror_code = int((log_dir / "pulse_local_mirror_seed.log.exit-code").read_text(encoding="utf-8").strip())
code = int((log_dir / "public_hardening.log.exit-code").read_text(encoding="utf-8").strip())
ci_workflow_contract = out_root / "ci-workflow-contract.json"
required_checks = ["pulse:generate-next", "pulse:local-mirror", "test_public_stabilization.py", "pulse:resume -- --dry-run", "bash_syntax_check", "forbidden_string_scan"]
ci_workflow_contract.write_text(json.dumps({
    "schema_version": "ao2.public-hardening-ci-workflow-contract.v1",
    "required_checks": required_checks,
    "predictable_runtime_budget": "under_5_minutes_local_expected",
    "workflow_trigger": "pull_request_or_local_manual",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "pulse_generate_next_seed", "command": "pulse:generate-next", "status": "passed" if seed_code == 0 else "failed", "exit_code": seed_code, "log": str(log_dir / "pulse_generate_next_seed.log")},
    {"name": "pulse_local_mirror_seed", "command": "pulse:local-mirror", "status": "passed" if mirror_code == 0 else "failed", "exit_code": mirror_code, "log": str(log_dir / "pulse_local_mirror_seed.log")},
    {"name": "public_hardening", "command": "public:hardening", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "public_hardening.log")},
    {"name": "ci_workflow_contract", "status": "passed" if ci_workflow_contract.is_file() else "failed"},
    {"name": "predictable_runtime_budget", "status": "passed"},
    {"name": "required_checks", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.public-hardening-ci-workflow.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "ci_workflow_contract": str(ci_workflow_contract),
    "predictable_runtime_budget": "under_5_minutes_local_expected",
    "required_checks": required_checks,
    "component_summaries": {
        "pulse_generate_next_seed": str(out_root / "pulse-generate-next" / "summary.json"),
        "pulse_local_mirror_seed": str(out_root / "pulse-local-mirror" / "pulse-local-mirror-summary.json"),
        "public_hardening": str(out_root / "public-hardening-subset" / "summary.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
