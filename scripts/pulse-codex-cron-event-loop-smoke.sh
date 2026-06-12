#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_CODEX_CRON_SMOKE_ROOT:-$ROOT/target/pulse-codex-cron-event-loop-smoke}"
LATEST_ROOT="$OUT_ROOT/latest"
CODEX_CRON_BIN="${AO2_CODEX_CRON_BIN:-}"
CODEX_CRON_ROOT="${AO2_CODEX_CRON_ROOT:-$ROOT/../codex-cron}"

usage() {
  cat >&2 <<'USAGE'
usage: scripts/pulse-codex-cron-event-loop-smoke.sh [options]

Options:
  --codex-cron-bin <path>   Installed codex-cron binary to exercise.
  --codex-cron-root <path>  codex-cron checkout root; builds release binary if needed.
  --out-root <path>         Evidence output root.

This smoke registers AO2 Pulse as a bounded codex-cron event-loop job, runs
`npm run pulse:generate-next`, and verifies codex-cron consumed AO2's generated
codex-cron-event-loop-decision.json via --event-loop-decision-file. It stores no
credentials and performs no provider execution, release publication, or git push.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --codex-cron-bin)
      CODEX_CRON_BIN="${2:?missing value for --codex-cron-bin}"
      shift 2
      ;;
    --codex-cron-root)
      CODEX_CRON_ROOT="${2:?missing value for --codex-cron-root}"
      shift 2
      ;;
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

if [[ -z "$CODEX_CRON_BIN" ]]; then
  if [[ -x "$CODEX_CRON_ROOT/target/release/codex-cron" ]]; then
    CODEX_CRON_BIN="$CODEX_CRON_ROOT/target/release/codex-cron"
  elif [[ -f "$CODEX_CRON_ROOT/Cargo.toml" ]]; then
    env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
      cargo build --release --manifest-path "$CODEX_CRON_ROOT/Cargo.toml" -p codex-cron-cli --bin codex-cron
    CODEX_CRON_BIN="$CODEX_CRON_ROOT/target/release/codex-cron"
  elif command -v codex-cron >/dev/null 2>&1; then
    CODEX_CRON_BIN="$(command -v codex-cron)"
  else
    echo "codex-cron binary unavailable; pass --codex-cron-bin or --codex-cron-root" >&2
    exit 1
  fi
fi

if [[ ! -x "$CODEX_CRON_BIN" ]]; then
  echo "codex-cron binary is not executable: $CODEX_CRON_BIN" >&2
  exit 1
fi

CODEX_CRON_HOME="$LATEST_ROOT/codex-cron-home"
PULSE_GENERATE_ROOT_REL="$OUT_ROOT_FOR_WORKDIR/latest/pulse-generate-next"
PULSE_PACKET_ROOT_REL="$OUT_ROOT_FOR_WORKDIR/latest/pulse-next-recommended-tasks"
PULSE_TASK_BOARD_ROOT_REL="$OUT_ROOT_FOR_WORKDIR/latest/pulse-task-board"
DECISION_FILE_REL="$PULSE_PACKET_ROOT_REL/codex-cron-event-loop-decision.json"
RUNNER="$LATEST_ROOT/run-pulse-generate-next.sh"
CODEX_CRON_STDOUT="$LATEST_ROOT/codex-cron-run-loop.stdout"
CODEX_CRON_STDERR="$LATEST_ROOT/codex-cron-run-loop.stderr"
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

mkdir -p "$CODEX_CRON_HOME"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  CODEX_CRON_HOME="$CODEX_CRON_HOME" \
  "$CODEX_CRON_BIN" add "every 5m" "AO2 Pulse codex-cron event-loop smoke" \
    --executor shell \
    --workdir "$ROOT" \
    --script "$RUNNER" \
    --event-loop \
    --event-loop-decision-file "$DECISION_FILE_REL" \
    --max-chain-runs 1 \
    --max-runtime-seconds 120 >/dev/null

JOB_ID="$(
  CODEX_CRON_HOME="$CODEX_CRON_HOME" "$CODEX_CRON_BIN" list --json |
    python3 -c 'import json,sys; jobs=json.load(sys.stdin); print(jobs[0]["id"])'
)"

set +e
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  CODEX_CRON_HOME="$CODEX_CRON_HOME" \
  "$CODEX_CRON_BIN" run-loop "$JOB_ID" --max-chain-runs 1 >"$CODEX_CRON_STDOUT" 2>"$CODEX_CRON_STDERR"
RUN_LOOP_EXIT=$?
set -e

python3 - "$ROOT" "$LATEST_ROOT" "$SUMMARY" "$CODEX_CRON_BIN" "$CODEX_CRON_HOME" "$JOB_ID" "$DECISION_FILE_REL" "$PULSE_GENERATE_ROOT_REL" "$RUN_LOOP_EXIT" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
latest_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
codex_cron_bin = Path(sys.argv[4]).resolve()
codex_cron_home = Path(sys.argv[5]).resolve()
job_id = sys.argv[6]
decision_file_rel = sys.argv[7]
pulse_generate_root_rel = sys.argv[8]
run_loop_exit = int(sys.argv[9])

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))

def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

event_loop_summary_path = codex_cron_home / "event-loop" / job_id / "latest.json"
decision_path = root / decision_file_rel
generator_summary_path = root / pulse_generate_root_rel / "summary.json"

blockers = []
if run_loop_exit != 0:
    blockers.append("codex_cron_run_loop_nonzero")
if not event_loop_summary_path.is_file():
    blockers.append("missing_codex_cron_event_loop_summary")
if not decision_path.is_file():
    blockers.append("missing_ao2_codex_cron_event_loop_decision")
if not generator_summary_path.is_file():
    blockers.append("missing_ao2_pulse_generate_next_summary")

event_loop_summary = load_json(event_loop_summary_path) if event_loop_summary_path.is_file() else {}
decision = load_json(decision_path) if decision_path.is_file() else {}
generator_summary = load_json(generator_summary_path) if generator_summary_path.is_file() else {}
first_decision = (event_loop_summary.get("decisions") or [{}])[0]

if event_loop_summary.get("schema_version") != "codex-cron.event-loop-run.v1":
    blockers.append("unexpected_codex_cron_event_loop_schema")
if first_decision.get("decision_source") != "file":
    blockers.append("codex_cron_decision_source_not_file")
if first_decision.get("decision_file") != str(decision_path):
    blockers.append("codex_cron_decision_file_mismatch")
if decision.get("schema_version") != "codex-cron.event-loop-decision.v1":
    blockers.append("unexpected_decision_schema")
if decision.get("ao2", {}).get("schema_version") != "ao2.pulse-codex-cron-event-loop-decision.v1":
    blockers.append("unexpected_ao2_decision_schema")
if generator_summary.get("schema_version") != "ao2.pulse-generate-next.v1":
    blockers.append("unexpected_pulse_generate_next_schema")

payload = {
    "schema_version": "ao2.pulse-codex-cron-event-loop-smoke.v1",
    "generated_at_utc": utc_now(),
    "status": "passed" if not blockers else "failed",
    "artifact_root": str(latest_root),
    "codex_cron": {
        "binary": str(codex_cron_bin),
        "home": str(codex_cron_home),
        "job_id": job_id,
        "run_loop_exit": run_loop_exit,
        "event_loop_summary": str(event_loop_summary_path),
        "event_loop_schema": event_loop_summary.get("schema_version"),
        "iterations": event_loop_summary.get("iterations"),
        "status": event_loop_summary.get("status"),
        "decision_source": first_decision.get("decision_source"),
        "decision_file": first_decision.get("decision_file"),
    },
    "ao2": {
        "pulse_generate_next_summary": str(generator_summary_path),
        "codex_cron_event_loop_decision": str(decision_path),
        "decision_schema": decision.get("schema_version"),
        "ao2_decision_schema": decision.get("ao2", {}).get("schema_version"),
        "decision_sha256": sha256(decision_path) if decision_path.is_file() else None,
    },
    "logs": {
        "stdout": str(latest_root / "codex-cron-run-loop.stdout"),
        "stderr": str(latest_root / "codex-cron-run-loop.stderr"),
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
    raise SystemExit("pulse codex-cron event-loop smoke failed: " + ",".join(blockers))
PY
