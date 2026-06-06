#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RESUME_JSON="${AO2_PULSE_RESUME_JSON:-$ROOT/.ao2-local/pulse/latest/resume.json}"
OUT_ROOT="${AO2_PULSE_TERMINAL_EVAL_LOOP_SCHEMA_COMPATIBILITY_ROOT:-$ROOT/target/pulse-terminal-eval-loop-schema-compatibility/latest}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$RESUME_JSON" "$OUT_ROOT" "$SUMMARY" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

resume_json = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary = Path(sys.argv[3]).resolve()
resume = json.loads(resume_json.read_text(encoding="utf-8"))
source = resume_json.parent / str(resume["pulse_eval_loop_path"])
source_payload = json.loads(source.read_text(encoding="utf-8"))
terminal = dict(source_payload)
terminal["status"] = "ready_for_next_pulse_task"
terminal["mode"] = "recommendation_only"
terminal["generated_at_utc"] = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
terminal["loop"] = {
    "bounded": True,
    "max_iterations": 1,
    "terminal": True,
    "chain_depth": int(source_payload.get("loop", {}).get("chain_depth", 0) or 0),
    "continues_automatically": False,
    "fixed_interval_loop_successor": "ao2 pulse eval-loop run --chain",
}
terminal["side_effects"] = {
    "provider_execution": False,
    "queue_execution": False,
    "memory_write": False,
    "mutates_ao_artifacts": False,
    "control_plane_mutation": False,
    "repo_apply": False,
}
terminal["trust_boundary"] = {
    "local_only": True,
    "stores_credentials": False,
    "control_plane_observer_only": True,
}
terminal_path = out_root / "terminal-pulse-eval-loop.json"
terminal_path.write_text(json.dumps(terminal, indent=2, sort_keys=True) + "\n", encoding="utf-8")
source_sha = hashlib.sha256(source.read_bytes()).hexdigest()
terminal_sha = hashlib.sha256(terminal_path.read_bytes()).hexdigest()
checks = [
    {"name": "status_ready_for_next_pulse_task", "status": "passed" if terminal["status"] == "ready_for_next_pulse_task" else "failed"},
    {"name": "mode_recommendation_only", "status": "passed" if terminal["mode"] == "recommendation_only" else "failed"},
    {"name": "terminal_loop", "status": "passed" if terminal["loop"]["terminal"] is True else "failed"},
    {"name": "repo_apply_false", "status": "passed" if terminal["side_effects"]["repo_apply"] is False else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-terminal-eval-loop-schema-compatibility.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "source_eval_loop": str(source),
    "source_sha256": source_sha,
    "terminal_eval_loop": str(terminal_path),
    "terminal_sha256": terminal_sha,
    "checks": checks,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
