#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_GENERATE_NEXT_CONTRACT_ROOT:-$ROOT/target/pulse-generate-next-contract/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

bash -n "$ROOT/scripts/pulse-generate-next.sh" >"$LOG_DIR/bash-n.log" 2>&1

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary = Path(sys.argv[3]).resolve()
log_dir = Path(sys.argv[4]).resolve()
text = (root / "scripts" / "pulse-generate-next.sh").read_text(encoding="utf-8")
needles = [
    "ao2.pulse-generate-next.v1",
    "ao2.pulse-next-lengthy-tasks.v1",
    "pulse-eval-loop.json",
    "packet.md",
    "board.md",
    "executor-evidence.json",
    "AO2_PULSE_GENERATE_NEXT_REGISTER",
    "pulse:register-auto-advance",
    "cursor",
    "recommended_tasks",
    "stores_credentials",
]
checks = [{"name": needle, "status": "passed" if needle in text else "failed"} for needle in needles]
checks.append({"name": "bash_syntax", "status": "passed", "log": str(log_dir / "bash-n.log")})
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-generate-next-contract.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
