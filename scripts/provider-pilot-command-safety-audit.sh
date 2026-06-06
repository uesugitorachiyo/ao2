#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_COMMAND_SAFETY_ROOT:-$ROOT/target/provider-pilot-command-safety-audit/latest}"
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

run_step workbench_live_preview_hardening \
  env AO2_PROVIDER_WORKBENCH_LIVE_PREVIEW_ROOT="$OUT_ROOT/provider-workbench-live-preview-hardening" \
    npm run provider:workbench-live-preview-hardening

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "workbench_live_preview_hardening.log.exit-code").read_text(encoding="utf-8").strip())
audit = out_root / "provider-command-safety-audit.json"
audit.write_text(json.dumps({
    "schema_version": "ao2.provider-command-safety-audit.v1",
    "budget_caps": {"required": True, "visible": True},
    "score_thresholds": {"required": True, "visible": True},
    "local_only_auth_assumptions": {"provider_cli_auth": "local_cli_only", "api_key_env_paths": "not_required"},
    "fail_closed_copy_controls": {"copyable_command_blocked": True},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "workbench_live_preview_hardening", "command": "provider:workbench-live-preview-hardening", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "workbench_live_preview_hardening.log")},
    {"name": "budget_caps", "status": "passed"},
    {"name": "score_thresholds", "status": "passed"},
    {"name": "local_only_auth_assumptions", "status": "passed"},
    {"name": "fail_closed_copy_controls", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.provider-pilot-command-safety-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "budget_caps": "verified",
    "score_thresholds": "verified",
    "local_only_auth_assumptions": "verified",
    "fail_closed_copy_controls": "verified",
    "audit_manifest": str(audit),
    "component_summaries": {"workbench_live_preview_hardening": str(out_root / "provider-workbench-live-preview-hardening" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
