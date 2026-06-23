#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_AO2_EVENT_LOOP_SMOKE_ROOT:-$ROOT/target/pulse-ao2-event-loop-smoke}"
LATEST_ROOT="$OUT_ROOT/latest"

usage() {
  cat >&2 <<'USAGE'
usage: scripts/pulse-ao2-event-loop-smoke.sh [options]

Options:
  --out-root <path>  Evidence output root.

This smoke runs AO2 Pulse through AO2's native bounded event-loop runtime,
consuming ao2-event-loop-decision.json via --decision-file. It stores no
credentials and performs no provider execution, release publication, or git push.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-root)
      OUT_ROOT="${2:?missing value for --out-root}"
      LATEST_ROOT="$OUT_ROOT/latest"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

rm -rf "$LATEST_ROOT"
mkdir -p "$LATEST_ROOT"

OUT_ROOT_ABS="$(cd "$(dirname "$OUT_ROOT")" && pwd)/$(basename "$OUT_ROOT")"
case "$OUT_ROOT_ABS" in
  "$ROOT"/*) OUT_ROOT_FOR_WORKDIR="${OUT_ROOT_ABS#$ROOT/}" ;;
  *) OUT_ROOT_FOR_WORKDIR="$OUT_ROOT_ABS" ;;
esac

PULSE_GENERATE_ROOT_REL="$OUT_ROOT_FOR_WORKDIR/latest/pulse-generate-next"
PULSE_PACKET_ROOT_REL="$OUT_ROOT_FOR_WORKDIR/latest/pulse-next-recommended-tasks"
PULSE_TASK_BOARD_ROOT_REL="$OUT_ROOT_FOR_WORKDIR/latest/pulse-task-board"
AO2_RUN_LOOP_ROOT_REL="$OUT_ROOT_FOR_WORKDIR/latest/ao2-run-loop"
DECISION_FILE_REL="$PULSE_PACKET_ROOT_REL/ao2-event-loop-decision.json"
RUNNER="$LATEST_ROOT/run-pulse-generate-next.sh"
AO2_RUN_LOOP_STDOUT="$LATEST_ROOT/ao2-run-loop.stdout"
AO2_RUN_LOOP_STDERR="$LATEST_ROOT/ao2-run-loop.stderr"
SUMMARY="$LATEST_ROOT/summary.json"

cat >"$RUNNER" <<EOF
#!/usr/bin/env bash
set -euo pipefail
cd "$ROOT"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \\
  AO2_PULSE_GENERATE_NEXT_REGISTER=0 \\
  AO2_PULSE_GENERATE_NEXT_ROOT="$PULSE_GENERATE_ROOT_REL" \\
  AO2_PULSE_GENERATE_NEXT_PACKET_ROOT="$PULSE_PACKET_ROOT_REL" \\
  AO2_PULSE_TASK_BOARD_ROOT="$PULSE_TASK_BOARD_ROOT_REL" \\
  npm run pulse:generate-next
EOF
chmod +x "$RUNNER"

set +e
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo run -p ao2-cli -- pulse run-loop \
    --command "$RUNNER" \
    --decision-file "$DECISION_FILE_REL" \
    --max-chain-runs 1 \
    --max-runtime-seconds 120 \
    --out-dir "$AO2_RUN_LOOP_ROOT_REL" \
    --apply-root "$ROOT" \
    --json >"$AO2_RUN_LOOP_STDOUT" 2>"$AO2_RUN_LOOP_STDERR"
RUN_LOOP_EXIT=$?
set -e

python3 - "$ROOT" "$LATEST_ROOT" "$SUMMARY" "$DECISION_FILE_REL" "$PULSE_GENERATE_ROOT_REL" "$AO2_RUN_LOOP_ROOT_REL" "$RUN_LOOP_EXIT" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
latest_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
decision_file_rel = sys.argv[4]
pulse_generate_root_rel = sys.argv[5]
run_loop_root_rel = sys.argv[6]
run_loop_exit = int(sys.argv[7])

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))

def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

decision_path = root / decision_file_rel
generator_summary_path = root / pulse_generate_root_rel / "summary.json"
run_loop_summary_path = root / run_loop_root_rel / "summary.json"

blockers = []
if run_loop_exit != 0:
    blockers.append("ao2_run_loop_nonzero")
if not run_loop_summary_path.is_file():
    blockers.append("missing_ao2_event_loop_run_summary")
if not decision_path.is_file():
    blockers.append("missing_ao2_event_loop_decision")
if not generator_summary_path.is_file():
    blockers.append("missing_ao2_pulse_generate_next_summary")

run_loop_summary = load_json(run_loop_summary_path) if run_loop_summary_path.is_file() else {}
decision = load_json(decision_path) if decision_path.is_file() else {}
generator_summary = load_json(generator_summary_path) if generator_summary_path.is_file() else {}
first_decision = (run_loop_summary.get("decisions") or [{}])[0]

if run_loop_summary.get("schema_version") != "ao2.pulse-event-loop-run.v1":
    blockers.append("unexpected_ao2_event_loop_run_schema")
if run_loop_summary.get("decision_source") != "file":
    blockers.append("ao2_decision_source_not_file")
if first_decision.get("decision_file") != str(decision_path):
    blockers.append("ao2_decision_file_mismatch")
if decision.get("schema_version") != "ao2.pulse-event-loop-decision.v1":
    blockers.append("unexpected_ao2_decision_schema")
if decision.get("ao2", {}).get("schema_version") != "ao2.pulse-event-loop-decision-metadata.v1":
    blockers.append("unexpected_ao2_decision_metadata_schema")
if generator_summary.get("schema_version") != "ao2.pulse-generate-next.v1":
    blockers.append("unexpected_pulse_generate_next_schema")

payload = {
    "schema_version": "ao2.pulse-event-loop-smoke.v1",
    "generated_at_utc": utc_now(),
    "status": "passed" if not blockers else "failed",
    "artifact_root": str(latest_root),
    "ao2": {
        "run_loop_exit": run_loop_exit,
        "run_loop_summary": str(run_loop_summary_path),
        "run_loop_schema": run_loop_summary.get("schema_version"),
        "iterations": run_loop_summary.get("iterations"),
        "status": run_loop_summary.get("status"),
        "decision_source": run_loop_summary.get("decision_source"),
        "pulse_generate_next_summary": str(generator_summary_path),
        "event_loop_decision": str(decision_path),
        "decision_schema": decision.get("schema_version"),
        "decision_metadata_schema": decision.get("ao2", {}).get("schema_version"),
        "decision_sha256": sha256(decision_path) if decision_path.is_file() else None,
    },
    "logs": {
        "stdout": str(latest_root / "ao2-run-loop.stdout"),
        "stderr": str(latest_root / "ao2-run-loop.stderr"),
    },
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "provider_execution": False,
        "publishes_release": False,
        "pushes_git": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if blockers:
    raise SystemExit("pulse AO2 event-loop smoke failed: " + ",".join(blockers))
PY
