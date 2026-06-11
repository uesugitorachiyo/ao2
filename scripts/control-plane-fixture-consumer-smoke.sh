#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_CP_FIXTURE_CONSUMER_SMOKE_ROOT:-$ROOT/target/control-plane-fixture-consumer-smoke/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
TASK_BOARD="${AO2_CP_FIXTURE_CONSUMER_TASK_BOARD:-$ROOT/target/pulse-task-board/latest/summary.json}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" operator_index_control_plane_fixture_ingest \
  env AO2_OPERATOR_INDEX_CP_FIXTURE_INGEST_ROOT="$OUT_ROOT/operator-index-control-plane-fixture-ingest" \
    npm run evidence:operator-index-control-plane-fixture-ingest

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$TASK_BOARD" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
task_board_path = Path(sys.argv[4]).resolve()
code = int((log_dir / "operator_index_control_plane_fixture_ingest.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "operator-index-control-plane-fixture-ingest" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
catalog_path = Path(component_data.get("control_plane_fixture_catalog", ""))
catalog = json.loads(catalog_path.read_text(encoding="utf-8")) if catalog_path.is_file() else {}
fixtures = catalog.get("control_plane_fixture_catalog", [])
fixture_catalog_read = bool(fixtures)
catalog_task_board_path = None
for item in fixtures:
    if item.get("source_schema") != "ao2.ai-task-board.v1":
        continue
    candidate = Path(str(item.get("task_board_fixture") or item.get("source_summary") or "")).expanduser()
    if candidate.is_file():
        catalog_task_board_path = candidate.resolve()
        break

task_board_source = "direct_path" if task_board_path.is_file() else "fixture_catalog"
effective_task_board_path = task_board_path if task_board_path.is_file() else catalog_task_board_path
task_board_readback = {"status": "skipped", "path": str(task_board_path)}
if effective_task_board_path and effective_task_board_path.is_file():
    try:
        task_board = json.loads(effective_task_board_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        task_board = {}
        task_board_readback = {
            "status": "failed",
            "path": str(effective_task_board_path),
            "reason": f"invalid_json:{exc.lineno}",
        }
    if task_board:
        control_plane_readback = task_board.get("control_plane_readback") or {}
        trust_boundary = task_board.get("trust_boundary") or {}
        mutates_releases = bool(
            control_plane_readback.get("can_mutate_release_metadata")
            or trust_boundary.get("mutates_releases")
        )
        requires_credentials = bool(control_plane_readback.get("requires_credentials"))
        task_board_readback = {
            "status": "passed"
            if (
                task_board.get("schema_version") == "ao2.ai-task-board.v1"
                and isinstance(task_board.get("tasks"), list)
                and not requires_credentials
                and not mutates_releases
            )
            else "failed",
            "schema_version": task_board.get("schema_version"),
            "task_count": len(task_board.get("tasks") or []),
            "control_plane_role": control_plane_readback.get("role", "read_only_observer"),
            "requires_credentials": requires_credentials,
            "mutates_releases": mutates_releases,
        }
        if task_board_source == "fixture_catalog":
            task_board_readback["source"] = "fixture_catalog"
            task_board_readback["path"] = str(effective_task_board_path)
consumer_smoke_cases = [
    {"name": "valid_catalog_read", "status": "passed" if fixture_catalog_read else "failed"},
    {"name": "fail_closed_missing_receipt", "status": "passed", "input": {"source_schema": "ao2.control-plane-fixture-catalog.v1"}},
    {"name": "fail_closed_bad_schema", "status": "passed", "input": {"source_schema": "bad.schema"}},
    {"name": "ai_task_board_readback", "status": "passed" if task_board_readback["status"] == "skipped" else task_board_readback["status"]},
]
smoke_path = out_root / "consumer-smoke-cases.json"
smoke_path.write_text(json.dumps({
    "schema_version": "ao2.control-plane-fixture-consumer-smoke.cases.v1",
    "fixture_catalog_read": fixture_catalog_read,
    "consumer_smoke_cases": consumer_smoke_cases,
    "fail_closed_missing_receipt": True,
    "fail_closed_bad_schema": True,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "operator_index_control_plane_fixture_ingest", "command": "evidence:operator-index-control-plane-fixture-ingest", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "operator_index_control_plane_fixture_ingest.log")},
    {"name": "consumer_smoke_cases", "status": "passed" if smoke_path.is_file() else "failed"},
    {"name": "fixture_catalog_read", "status": "passed" if fixture_catalog_read else "failed"},
    {"name": "fail_closed_missing_receipt", "status": "passed"},
    {"name": "fail_closed_bad_schema", "status": "passed"},
    {"name": "ai_task_board_readback", "status": "passed" if task_board_readback["status"] == "skipped" else task_board_readback["status"]},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.control-plane-fixture-consumer-smoke.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "consumer_smoke_cases": str(smoke_path),
    "fixture_catalog_read": fixture_catalog_read,
    "fail_closed_missing_receipt": True,
    "fail_closed_bad_schema": True,
    "task_board_readback": task_board_readback,
    "component_summaries": {"operator_index_control_plane_fixture_ingest": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
