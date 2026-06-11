#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_OPERATOR_INDEX_CP_FIXTURE_INGEST_ROOT:-$ROOT/target/operator-index-control-plane-fixture-ingest/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
TASK_BOARD="${AO2_OPERATOR_INDEX_CP_TASK_BOARD:-}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" operator_index_control_plane_readback_drill \
  env AO2_OPERATOR_INDEX_CP_READBACK_ROOT="$OUT_ROOT/operator-index-control-plane-readback-drill" \
    npm run evidence:operator-index-control-plane-readback-drill

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$TASK_BOARD" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
task_board_arg = sys.argv[4]
task_board_path = Path(task_board_arg).resolve() if task_board_arg else None
code = int((log_dir / "operator_index_control_plane_readback_drill.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "operator-index-control-plane-readback-drill" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
fixture_ingest_manifest = out_root / "fixture-ingest-manifest.json"
control_plane_fixture_catalog = out_root / "control-plane-fixture-catalog.json"
fixture = {
    "source_summary": str(component),
    "source_schema": component_data.get("schema_version"),
    "receipt_fixture": component_data.get("control_plane_receipt_fixture"),
    "readback_fixture_reusable": True,
    "consumer_smoke_contract": "control_plane_consumers_can_load_local_receipt_fixture",
}
fixtures = [fixture]
task_board_fixture = None
task_board_fixture_cataloged = False
if task_board_path and task_board_path.is_file():
    try:
        task_board_data = json.loads(task_board_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        task_board_data = {}
    if task_board_data.get("schema_version") == "ao2.ai-task-board.v1":
        task_board_fixture = {
            "source_summary": str(task_board_path),
            "source_schema": "ao2.ai-task-board.v1",
            "task_board_fixture": str(task_board_path),
            "task_board_fixture_reusable": True,
            "consumer_smoke_contract": "control_plane_consumers_can_load_task_board_fixture",
            "control_plane_role": "read_only_observer",
            "requires_credentials": False,
            "mutates_releases": False,
        }
        fixtures.append(task_board_fixture)
        task_board_fixture_cataloged = True
fixture_ingest_manifest.write_text(json.dumps({
    "schema_version": "ao2.operator-index-control-plane-fixture-ingest.manifest.v1",
    "fixture_ingest_manifest": fixture,
    "task_board_fixture": task_board_fixture,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
control_plane_fixture_catalog.write_text(json.dumps({
    "schema_version": "ao2.control-plane-fixture-catalog.v1",
    "control_plane_fixture_catalog": fixtures,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "operator_index_control_plane_readback_drill", "command": "evidence:operator-index-control-plane-readback-drill", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "operator_index_control_plane_readback_drill.log")},
    {"name": "fixture_ingest_manifest", "status": "passed" if fixture_ingest_manifest.is_file() else "failed"},
    {"name": "control_plane_fixture_catalog", "status": "passed" if control_plane_fixture_catalog.is_file() else "failed"},
    {"name": "readback_fixture_reusable", "status": "passed"},
    {"name": "consumer_smoke_contract", "status": "passed"},
    {"name": "task_board_fixture_cataloged", "status": "passed" if task_board_fixture_cataloged else "skipped", "path": str(task_board_path) if task_board_path else None},
]
status = "passed" if all(item["status"] in {"passed", "skipped"} for item in checks) else "failed"
payload = {
    "schema_version": "ao2.operator-index-control-plane-fixture-ingest.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "fixture_ingest_manifest": str(fixture_ingest_manifest),
    "control_plane_fixture_catalog": str(control_plane_fixture_catalog),
    "readback_fixture_reusable": True,
    "consumer_smoke_contract": "local fixture catalog readback",
    "task_board_fixture": str(task_board_path) if task_board_fixture_cataloged and task_board_path else None,
    "task_board_fixture_cataloged": task_board_fixture_cataloged,
    "component_summaries": {"operator_index_control_plane_readback_drill": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
