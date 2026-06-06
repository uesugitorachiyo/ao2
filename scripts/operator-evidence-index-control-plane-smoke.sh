#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_OPERATOR_INDEX_CP_SMOKE_ROOT:-$ROOT/target/operator-evidence-index-control-plane-smoke/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" operator_index \
  env AO2_OPERATOR_EVIDENCE_INDEX_ROOT="$OUT_ROOT/operator-evidence-index" \
    npm run evidence:operator-index

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
code = int((log_dir / "operator_index.log.exit-code").read_text(encoding="utf-8").strip())
index_json = out_root / "operator-evidence-index" / "index.json"
index = json.loads(index_json.read_text(encoding="utf-8")) if index_json.is_file() else {}
control_plane_publish_smoke = out_root / "control-plane-publish-smoke.json"
control_plane_publish_smoke.write_text(json.dumps({
    "schema_version": "ao2.operator-index-control-plane-smoke-readback.v1",
    "readback_source": str(index_json),
    "control_plane_publish_smoke": "local_readback_only",
    "operator_index_dashboard": str(out_root / "operator-evidence-index" / "index.html"),
    "summary_count": index.get("summary_count", 0),
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "operator_index", "command": "evidence:operator-index", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "operator_index.log")},
    {"name": "readback_source", "status": "passed" if index_json.is_file() else "failed"},
    {"name": "control_plane_publish_smoke", "status": "passed" if control_plane_publish_smoke.is_file() else "failed"},
    {"name": "operator_index_dashboard", "status": "passed" if (out_root / "operator-evidence-index" / "index.html").is_file() else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.operator-evidence-index-control-plane-smoke.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "readback_source": str(index_json),
    "control_plane_publish_smoke": str(control_plane_publish_smoke),
    "operator_index_dashboard": str(out_root / "operator-evidence-index" / "index.html"),
    "component_summaries": {"operator_index": str(out_root / "operator-evidence-index" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
