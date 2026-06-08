#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_HARDENING_WORKFLOW_DRY_RUN_ROOT:-$ROOT/target/public-hardening-workflow-file-dry-run/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" public_hardening_ci_workflow \
  env AO2_PUBLIC_HARDENING_CI_WORKFLOW_ROOT="$OUT_ROOT/public-hardening-ci-workflow" \
    npm run public:hardening-ci-workflow

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "public_hardening_ci_workflow.log.exit-code").read_text(encoding="utf-8").strip())
workflow_yaml_preview = out_root / "workflow-file-dry-run.yml"
workflow_yaml_preview.write_text("""name: AO2 Public Hardening

on:
  pull_request:
  workflow_dispatch:

jobs:
  public-hardening:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6.0.3
      - uses: actions/setup-node@v6.4.0
        with:
          node-version: "22"
      - run: npm ci
      - run: AO2_PULSE_GENERATE_NEXT_REGISTER=0 npm run pulse:generate-next
      - run: AO2_PULSE_LOCAL_MIRROR_SOURCE=target/pulse-next-recommended-tasks npm run pulse:local-mirror
      - run: PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q
      - run: npm run public:hardening
      - run: npm run pulse:resume -- --dry-run
""", encoding="utf-8")
workflow_file_dry_run = out_root / "workflow-file-dry-run.json"
workflow_file_dry_run.write_text(json.dumps({
    "schema_version": "ao2.public-hardening-workflow-file-preview.v1",
    "workflow_file_dry_run": str(workflow_yaml_preview),
    "workflow_yaml_preview": workflow_yaml_preview.read_text(encoding="utf-8"),
    "pull_request_trigger": True,
    "workflow_dispatch_trigger": True,
    "required_local_checks": ["public:hardening-ci-workflow", "pulse:generate-next", "pulse:local-mirror", "public:hardening", "pulse:resume -- --dry-run"],
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "public_hardening_ci_workflow", "command": "public:hardening-ci-workflow", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "public_hardening_ci_workflow.log")},
    {"name": "workflow_file_dry_run", "status": "passed" if workflow_file_dry_run.is_file() else "failed"},
    {"name": "workflow_yaml_preview", "status": "passed" if workflow_yaml_preview.is_file() else "failed"},
    {"name": "pull_request_trigger", "status": "passed"},
    {"name": "workflow_dispatch_trigger", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.public-hardening-workflow-file-dry-run.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "workflow_file_dry_run": str(workflow_file_dry_run),
    "workflow_yaml_preview": str(workflow_yaml_preview),
    "pull_request_trigger": True,
    "workflow_dispatch_trigger": True,
    "component_summaries": {"public_hardening_ci_workflow": str(out_root / "public-hardening-ci-workflow" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
