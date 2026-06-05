#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PHASE1_PROMOTION_GOLDEN_ROOT:-$ROOT/target/phase1-promotion-golden/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG="$OUT_ROOT/phase1-operator-golden.log"
OPERATOR_ROOT="$OUT_ROOT/phase1-operator-golden"
TOKEN_ENV_NAME="${AO2_PHASE1_API_TOKEN_ENV:-AO2_PHASE1_CP_TOKEN}"
TOKEN_VALUE="${!TOKEN_ENV_NAME:-}"
SMOKE_TOKEN="${AO2_PHASE1_CP_TOKEN:-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef}"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

AO2_PHASE1_OPERATOR_SMOKE_ROOT="$OPERATOR_ROOT" \
AO2_PHASE1_CP_TOKEN="$SMOKE_TOKEN" \
  npm run smoke:phase1-operator-golden >"$LOG" 2>&1

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG" "$OPERATOR_ROOT/summary.json" "$TOKEN_ENV_NAME" "$TOKEN_VALUE" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_path = Path(sys.argv[3]).resolve()
operator_summary_path = Path(sys.argv[4]).resolve()
token_env_name = sys.argv[5]
token_value = sys.argv[6]

operator = json.loads(operator_summary_path.read_text(encoding="utf-8"))
scan_paths = [log_path, operator_summary_path]
for value in operator.get("artifacts", {}).values():
    path = Path(value)
    if path.exists() and path.is_file():
        scan_paths.append(path)
if operator.get("readback_summary"):
    path = Path(operator["readback_summary"])
    if path.exists():
        scan_paths.append(path)

leaks = []
for path in scan_paths:
    text = path.read_text(encoding="utf-8", errors="replace")
    if "Authorization: Bearer" in text:
        leaks.append({"path": str(path), "pattern": "Authorization: Bearer"})
    if token_value and token_value in text:
        leaks.append({"path": str(path), "pattern": token_env_name})

payload = {
    "schema_version": "ao2.phase1-promotion-golden-path.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if operator.get("status") == "passed" and not leaks else "failed",
    "artifact_root": str(out_root),
    "operator_summary": str(operator_summary_path),
    "readback_summary": operator.get("readback_summary"),
    "dashboard_snapshot": operator.get("artifacts", {}).get("dashboard_html") or operator.get("artifacts", {}).get("dashboard_json"),
    "token_boundary": {
        "api_token_env_name": token_env_name,
        "uses_env_token": True,
        "stores_credentials": False,
        "forbidden_literal": "Authorization: Bearer",
    },
    "token_leak_scan": {
        "status": "passed" if not leaks else "failed",
        "scanned_files": [str(path) for path in scan_paths],
        "leaks": leaks,
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
