#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_OPERATOR_INDEX_CP_READBACK_ROOT:-$ROOT/target/operator-index-control-plane-readback-drill/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" operator_index_control_plane_smoke \
  env AO2_OPERATOR_INDEX_CP_SMOKE_ROOT="$OUT_ROOT/operator-evidence-index-control-plane-smoke" \
    npm run evidence:operator-index-control-plane-smoke

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "operator_index_control_plane_smoke.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "operator-evidence-index-control-plane-smoke" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
control_plane_receipt_fixture = out_root / "control-plane-receipt-fixture.json"
control_plane_receipt_fixture.write_text(json.dumps({
    "schema_version": "ao2.control-plane-receipt-fixture.v1",
    "source_summary": str(component),
    "source_schema": component_data.get("schema_version"),
    "operator_index_readback": component_data.get("operator_index_dashboard"),
    "dashboard_link_check": "local_index_html_exists",
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
operator_index_readback = component_data.get("operator_index_dashboard") or ""
dashboard_link_check = bool(operator_index_readback)
checks = [
    {"name": "operator_index_control_plane_smoke", "command": "evidence:operator-index-control-plane-smoke", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "operator_index_control_plane_smoke.log")},
    {"name": "readback_drill", "status": "passed" if component.is_file() else "failed"},
    {"name": "operator_index_readback", "status": "passed" if operator_index_readback else "failed"},
    {"name": "control_plane_receipt_fixture", "status": "passed" if control_plane_receipt_fixture.is_file() else "failed"},
    {"name": "dashboard_link_check", "status": "passed" if dashboard_link_check else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.operator-index-control-plane-readback-drill.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "readback_drill": "operator index summary copied into control-plane receipt fixture",
    "operator_index_readback": operator_index_readback,
    "control_plane_receipt_fixture": str(control_plane_receipt_fixture),
    "dashboard_link_check": dashboard_link_check,
    "component_summaries": {"operator_index_control_plane_smoke": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
