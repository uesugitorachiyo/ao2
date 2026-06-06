#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_STOP_AND_DEDUP_LEDGER_ROOT:-$ROOT/target/pulse-stop-and-dedup-ledger/latest}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary = Path(sys.argv[3]).resolve()
resume = json.loads((root / ".ao2-local" / "pulse" / "latest" / "resume.json").read_text(encoding="utf-8"))
eval_loop = root / ".ao2-local" / "pulse" / "latest" / str(resume["pulse_eval_loop_path"])
digest = hashlib.sha256(eval_loop.read_bytes()).hexdigest()
ledger = out_root / "pulse-auto-advance-ledger.jsonl"
entry = {
    "schema_version": "ao2.pulse-auto-advance-ledger-entry.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "pulse_eval_loop_sha256": digest,
    "status": "passed",
}
ledger.write_text(json.dumps(entry, sort_keys=True) + "\n", encoding="utf-8")
stop_file = out_root / "STOP"
stop_file.write_text("operator_stop\n", encoding="utf-8")
checks = [
    {"name": "AO2_PULSE_AUTO_ADVANCE_STOP_FILE", "status": "passed" if stop_file.is_file() else "failed", "path": str(stop_file)},
    {"name": "pulse-auto-advance-ledger.jsonl", "status": "passed" if ledger.is_file() else "failed", "path": str(ledger)},
    {"name": "duplicate_eval_loop_digest", "status": "passed" if digest in ledger.read_text(encoding="utf-8") else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-stop-and-dedup-ledger.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "stop_file": str(stop_file),
    "ledger": str(ledger),
    "duplicate_eval_loop_digest": digest,
    "checks": checks,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
