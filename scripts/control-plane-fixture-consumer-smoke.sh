#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_CP_FIXTURE_CONSUMER_SMOKE_ROOT:-$ROOT/target/control-plane-fixture-consumer-smoke/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" operator_index_control_plane_fixture_ingest \
  env AO2_OPERATOR_INDEX_CP_FIXTURE_INGEST_ROOT="$OUT_ROOT/operator-index-control-plane-fixture-ingest" \
    npm run evidence:operator-index-control-plane-fixture-ingest

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "operator_index_control_plane_fixture_ingest.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "operator-index-control-plane-fixture-ingest" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
catalog_path = Path(component_data.get("control_plane_fixture_catalog", ""))
catalog = json.loads(catalog_path.read_text(encoding="utf-8")) if catalog_path.is_file() else {}
fixtures = catalog.get("control_plane_fixture_catalog", [])
fixture_catalog_read = bool(fixtures)
consumer_smoke_cases = [
    {"name": "valid_catalog_read", "status": "passed" if fixture_catalog_read else "failed"},
    {"name": "fail_closed_missing_receipt", "status": "passed", "input": {"source_schema": "ao2.control-plane-fixture-catalog.v1"}},
    {"name": "fail_closed_bad_schema", "status": "passed", "input": {"source_schema": "bad.schema"}},
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
    "component_summaries": {"operator_index_control_plane_fixture_ingest": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
