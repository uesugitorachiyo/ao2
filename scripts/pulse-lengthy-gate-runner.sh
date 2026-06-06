#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_LENGTHY_GATE_ROOT:-$ROOT/target/pulse-lengthy-gate/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
MANIFEST="${AO2_PULSE_LENGTHY_GATE_MANIFEST:-$ROOT/scripts/pulse-lengthy-gates-manifest.json}"
MODE="run"
GATE_ID=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --contract)
      MODE="contract"
      shift
      ;;
    --list)
      MODE="list"
      shift
      ;;
    --gate)
      GATE_ID="${2:-}"
      if [ -z "$GATE_ID" ]; then
        echo "--gate requires an id" >&2
        exit 2
      fi
      shift 2
      ;;
    --manifest)
      MANIFEST="${2:-}"
      if [ -z "$MANIFEST" ]; then
        echo "--manifest requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--contract | --list | --gate <id>] [--manifest <path>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$MANIFEST" "$MODE" "$GATE_ID" <<'PY'
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
log_dir = Path(sys.argv[4]).resolve()
manifest_path = Path(sys.argv[5]).resolve()
mode = sys.argv[6]
gate_id = sys.argv[7]

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def load_package_scripts() -> dict:
    return json.loads((root / "package.json").read_text(encoding="utf-8")).get("scripts", {})

manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
scripts = load_package_scripts()
gates = manifest.get("gates", [])
gate_by_id = {gate.get("id"): gate for gate in gates}
missing_commands = []
for gate in gates:
    for command in gate.get("commands", []):
        if command not in scripts:
            missing_commands.append({"gate": gate.get("id"), "command": command})

checks = [
    {
        "name": "manifest_schema",
        "status": "passed" if manifest.get("schema_version") == "ao2.pulse-lengthy-gates-manifest.v1" else "failed",
    },
    {"name": "gate_count", "status": "passed" if len(gates) >= 10 else "failed", "count": len(gates)},
    {
        "name": "wrapper_replacement_manifest",
        "status": "passed" if all(gate.get("replaces", "").endswith(".sh") for gate in gates) else "failed",
    },
]
results = []
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
reason = "contract_checked"

if mode == "list":
    reason = "listed"
elif mode == "contract":
    reason = "contract_checked"
elif mode == "run":
    if not gate_id:
        status = "blocked"
        reason = "missing_gate_id"
    elif gate_id not in gate_by_id:
        status = "blocked"
        reason = "unknown_gate_id"
    else:
        gate = gate_by_id[gate_id]
        gate_missing = [item for item in missing_commands if item["gate"] == gate_id]
        if gate_missing:
            status = "blocked"
            reason = "missing_package_commands"
        else:
            reason = "gate_executed"
            for index, command_name in enumerate(gate.get("commands", []), start=1):
                log = log_dir / f"{index:02d}-{command_name.replace(':', '_')}.log"
                completed = subprocess.run(
                    ["npm", "run", command_name],
                    cwd=root,
                    text=True,
                    stdout=log.open("w", encoding="utf-8"),
                    stderr=subprocess.STDOUT,
                    check=False,
                )
                item = {
                    "name": command_name,
                    "status": "passed" if completed.returncode == 0 else "failed",
                    "exit_code": completed.returncode,
                    "log": str(log),
                }
                results.append(item)
                if completed.returncode != 0:
                    status = "failed"
                    break
else:
    status = "failed"
    reason = "unknown_mode"

payload = {
    "schema_version": "ao2.pulse-lengthy-gate-runner.v1",
    "generated_at_utc": utc_now(),
    "status": status,
    "reason": reason,
    "mode": mode,
    "gate_id": gate_id,
    "artifact_root": str(out_root),
    "manifest": str(manifest_path),
    "consolidated_gate_count": len(gates),
    "missing_package_commands": missing_commands,
    "checks": checks,
    "results": results,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "deletes_files": False,
        "pushes": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
print(f"reason={reason}")
if status in {"blocked", "failed"}:
    raise SystemExit(1)
PY
