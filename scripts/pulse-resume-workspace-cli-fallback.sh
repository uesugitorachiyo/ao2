#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_RESUME_WORKSPACE_CLI_FALLBACK_ROOT:-$ROOT/target/pulse-resume-workspace-cli-fallback/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

set +e
ao2 --help >"$LOG_DIR/global-ao2-help.log" 2>&1
GLOBAL_HELP_CODE=$?
ao2 pulse eval-loop run --help >"$LOG_DIR/global-ao2-pulse-help.log" 2>&1
GLOBAL_PULSE_CODE=$?
cargo run -q -p ao2-cli -- pulse eval-loop run --help >"$LOG_DIR/workspace-ao2-pulse-help.log" 2>&1
WORKSPACE_PULSE_CODE=$?
set -e

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$GLOBAL_HELP_CODE" "$GLOBAL_PULSE_CODE" "$WORKSPACE_PULSE_CODE" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
global_help_code = int(sys.argv[4])
global_pulse_code = int(sys.argv[5])
workspace_pulse_code = int(sys.argv[6])
global_ao2_supports_pulse = global_pulse_code == 0
workspace_cli_supports_pulse = workspace_pulse_code == 0
fallback_required = not global_ao2_supports_pulse and workspace_cli_supports_pulse
status = "passed" if workspace_cli_supports_pulse and global_help_code == 0 else "failed"
payload = {
    "schema_version": "ao2.pulse-resume-workspace-cli-fallback.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "global_ao2_supports_pulse": global_ao2_supports_pulse,
    "workspace_cli_supports_pulse": workspace_cli_supports_pulse,
    "fallback_required": fallback_required,
    "recommended_fallback": "cargo run -q -p ao2-cli -- pulse eval-loop run --help",
    "logs": {
        "global_ao2_help": str(log_dir / "global-ao2-help.log"),
        "global_ao2_pulse_help": str(log_dir / "global-ao2-pulse-help.log"),
        "workspace_ao2_pulse_help": str(log_dir / "workspace-ao2-pulse-help.log"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
