#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_REPO:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_CP_LOCAL_BOOTSTRAP_ROOT:-$ROOT/target/control-plane-local-bootstrap/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
DATA_ROOT="$OUT_ROOT/long-lived-control-plane"
BIND="${AO2_CP_LOCAL_BOOTSTRAP_BIND:-127.0.0.1:18745}"
TOKEN_SOURCE="${AO2_CONTROL_PLANE_TOKEN_SOURCE:-}"

# Default bootstrap helper: ../ao2-control-plane/scripts/start-long-lived-dev.sh

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

start_args=(--once-check --no-build --data-dir "$DATA_ROOT" --bind "$BIND")
if [ -n "$TOKEN_SOURCE" ]; then
  start_args+=(--token-source "$TOKEN_SOURCE")
fi

set +e
"$CP_ROOT/scripts/start-long-lived-dev.sh" "${start_args[@]}" >"$LOG_DIR/start-long-lived-dev.log" 2>"$LOG_DIR/start-long-lived-dev.err"
start_code=$?
set -e
printf "%s\n" "$start_code" >"$LOG_DIR/start-long-lived-dev.log.exit-code"

python3 - "$OUT_ROOT" "$SUMMARY" "$DATA_ROOT" "$BIND" "$LOG_DIR/start-long-lived-dev.log" "$LOG_DIR/start-long-lived-dev.err" "$start_code" "$TOKEN_SOURCE" <<'PY'
import json
import os
import re
import stat
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
data_root = Path(sys.argv[3]).resolve()
bind = sys.argv[4]
stdout_log = Path(sys.argv[5]).resolve()
stderr_log = Path(sys.argv[6]).resolve()
start_code = int(sys.argv[7])
token_source = sys.argv[8]
token_file = data_root / "api-token"
combined = stdout_log.read_text(encoding="utf-8", errors="replace") + stderr_log.read_text(encoding="utf-8", errors="replace")

token = token_file.read_text(encoding="utf-8").strip() if token_file.is_file() else ""
mode = stat.S_IMODE(token_file.stat().st_mode) if token_file.is_file() else None
checks = [
    {"name": "start_long_lived_once_check", "status": "passed" if start_code == 0 and "once_check=passed" in combined else "failed"},
    {"name": "token_file_exists", "status": "passed" if token_file.is_file() else "failed"},
    {"name": "token_file_mode", "status": "passed" if mode == 0o600 else "failed", "observed": oct(mode) if mode is not None else None},
    {"name": "token_shape", "status": "passed" if re.fullmatch(r"[0-9A-Za-z._~+/=-]{16,}", token) else "failed"},
    {"name": "token_leak_scan", "status": "passed" if token and token not in combined else "failed"},
    {"name": "provider_key_path_absent", "status": "passed" if "API_KEY" not in combined else "failed"},
    {"name": "bind_reported", "status": "passed" if f"bind={bind}" in combined else "failed"},
]
status = "passed" if all(check["status"] == "passed" for check in checks) else "failed"
payload = {
    "schema_version": "ao2.control-plane-local-bootstrap.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "control_plane_repo": str(Path(os.path.relpath(Path(sys.argv[1]).resolve().parents[2] / "../ao2-control-plane", Path.cwd())).as_posix()),
    "data_root": str(data_root),
    "bind": bind,
    "token_file": str(token_file),
    "token_file_mode": oct(mode) if mode is not None else None,
    "token_source_configured": bool(token_source),
    "token_source_stored": False,
    "logs": {"stdout": str(stdout_log), "stderr": str(stderr_log)},
    "checks": checks,
    "trust_boundary": {"local_only": True, "stores_credentials": False, "token_value_recorded": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
