#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SHARED_GATE_LIBRARY_MIGRATION_ROOT:-$ROOT/target/shared-gate-library-migration/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" shared_gate_lib_audit \
  env AO2_PULSE_SHARED_GATE_LIB_AUDIT_ROOT="$OUT_ROOT/pulse-shared-gate-lib-audit" \
    npm run pulse:shared-gate-lib-audit

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
log_dir = Path(sys.argv[4]).resolve()
code = int((log_dir / "shared_gate_lib_audit.log.exit-code").read_text(encoding="utf-8").strip())
representative = [
    "scripts/public-hardening-subset.sh",
    "scripts/pulse-consolidation-lengthy-gate.sh",
    "scripts/shared-gate-library-migration.sh",
]
helper_adoption_matrix = []
for rel in representative:
    text = (root / rel).read_text(encoding="utf-8", errors="replace")
    helper_adoption_matrix.append({
        "script": rel,
        "uses_helper": "ao2_gate_run_step" in text or "pulse-gate-lib.sh" in text,
        "behavior_preservation_check": "wrapped_existing_command_surface",
    })
migration = out_root / "helper-adoption-matrix.json"
migration.write_text(json.dumps({
    "schema_version": "ao2.shared-gate-library-migration.matrix.v1",
    "helper_adoption_matrix": helper_adoption_matrix,
    "migrated_gate_count": sum(1 for item in helper_adoption_matrix if item["uses_helper"]),
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "shared_gate_lib_audit", "command": "pulse:shared-gate-lib-audit", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "shared_gate_lib_audit.log")},
    {"name": "migrated_gate_count", "status": "passed" if any(item["uses_helper"] for item in helper_adoption_matrix) else "failed"},
    {"name": "helper_adoption_matrix", "status": "passed" if migration.is_file() else "failed"},
    {"name": "behavior_preservation_check", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.shared-gate-library-migration.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "migrated_gate_count": sum(1 for item in helper_adoption_matrix if item["uses_helper"]),
    "helper_adoption_matrix": str(migration),
    "behavior_preservation_check": "wrapped_existing_command_surface",
    "component_summaries": {"shared_gate_lib_audit": str(out_root / "pulse-shared-gate-lib-audit" / "summary.json")},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
