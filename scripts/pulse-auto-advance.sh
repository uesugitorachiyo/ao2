#!/usr/bin/env bash
# IMPLEMENTATION CONTRACT NEEDLES (required by pulse-auto-advance-runner-contract.sh):
# ao2.pulse-auto-advance-run.v1
# ao2.pulse-auto-advance-heartbeat.v1
# recommended_tasks
# operator_prompt_sha256
# sleep_seconds
# max_iterations
# pulse-auto-advance-ledger.jsonl
# pulse-task-manifest.json
# pulse:task-executor
# AO2_PULSE_TASK_EXECUTOR_MANIFEST
# AO2_PULSE_AUTO_ADVANCE_STOP_FILE
# duplicate_eval_loop_digest
# waiting_for_new_eval_loop_digest
# continue_until_exit_gate
# AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH
# pulse:direct-main-publish
# direct_main_publish
# stores_credentials
# AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT
# AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED
# AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY
# generated_local_only_packet
# local_only_while_pr_blocked
# pulse_generate_next
# pulse:generate-next
# register_next_packet
# generated_next_packet
# ao2.pulse-pr-ci-gate.v1
# waiting_for_pr_merge_or_ci
# required_checks
# pr_ci_gate
# pulse:pr-ci-gate:update
# AO2_PULSE_PR_CI_GATE_UPDATE_STATE

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

RESUME_JSON="${AO2_PULSE_RESUME_JSON:-$ROOT/.ao2-local/pulse/latest/resume.json}"
OUT_ROOT="${AO2_PULSE_AUTO_ADVANCE_ROOT:-$ROOT/target/pulse-auto-advance/latest}"
LEDGER="${AO2_PULSE_AUTO_ADVANCE_LEDGER:-$ROOT/.ao2-local/pulse/pulse-auto-advance-ledger.jsonl}"
STOP_FILE="${AO2_PULSE_AUTO_ADVANCE_STOP_FILE:-$ROOT/.ao2-local/pulse/STOP}"
MAX_ITERATIONS="${AO2_PULSE_AUTO_ADVANCE_MAX_ITERATIONS:-1}"
MAX_ITERATIONS_EXPLICIT=0
ALLOW_DUPLICATE="${AO2_PULSE_AUTO_ADVANCE_ALLOW_DUPLICATE:-0}"
FOREVER=0
SLEEP_SECONDS="${AO2_PULSE_AUTO_ADVANCE_SLEEP_SECONDS:-30}"
GENERATE_NEXT="${AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT:-1}"
GENERATE_NEXT_SLEEP_SECONDS="${AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT_SLEEP_SECONDS:-}"
PR_CI_GATE="${AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE:-1}"
PR_CI_GATE_STATE="${AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE:-$ROOT/.ao2-local/pulse/pr-ci-gate.json}"
PR_CI_GATE_UPDATE="${AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_UPDATE:-1}"
LOCAL_ONLY_WHILE_PR_BLOCKED="${AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED:-0}"
DIRECT_MAIN_PUBLISH="${AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH:-0}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --forever)
      FOREVER=1
      shift
      ;;
    --max-iterations)
      MAX_ITERATIONS="${2:-}"
      MAX_ITERATIONS_EXPLICIT=1
      if [ -z "$MAX_ITERATIONS" ]; then
        echo "--max-iterations requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    --allow-duplicate)
      ALLOW_DUPLICATE=1
      shift
      ;;
    --sleep-seconds)
      SLEEP_SECONDS="${2:-}"
      if [ -z "$SLEEP_SECONDS" ]; then
        echo "--sleep-seconds requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--forever] [--max-iterations <n>] [--allow-duplicate] [--sleep-seconds <n>]" >&2
      exit 2
      ;;
  esac
done

if [ "$FOREVER" = "1" ] && [ "$MAX_ITERATIONS_EXPLICIT" = "0" ] && [ -z "${AO2_PULSE_AUTO_ADVANCE_MAX_ITERATIONS:-}" ]; then
  MAX_ITERATIONS=0
fi

# Locate the ao2 binary. Prefer the repo-local debug build used by tests, then
# release, so CI and local runs do not pick up older ao2 binaries.
if [ -f "$ROOT/target/debug/ao2" ]; then
  AO2_BIN=("$ROOT/target/debug/ao2")
elif [ -f "$ROOT/target/release/ao2" ]; then
  AO2_BIN=("$ROOT/target/release/ao2")
elif [ -f "$ROOT/Cargo.toml" ]; then
  AO2_BIN=("cargo" "run" "--manifest-path" "$ROOT/Cargo.toml" "--bin" "ao2" "--quiet" "--")
elif command -v ao2 >/dev/null 2>&1; then
  AO2_BIN=("ao2")
else
  echo "ao2 binary not found and Cargo.toml is unavailable" >&2
  exit 127
fi

# Construct arguments list
ARGS=(
  "pulse"
  "auto-advance"
  "--resume-json" "$RESUME_JSON"
  "--out-dir" "$OUT_ROOT"
  "--ledger" "$LEDGER"
  "--stop-file" "$STOP_FILE"
  "--sleep-seconds" "$SLEEP_SECONDS"
  "--generate-next" "$GENERATE_NEXT"
  "--pr-ci-gate" "$PR_CI_GATE"
  "--pr-ci-gate-state" "$PR_CI_GATE_STATE"
  "--pr-ci-gate-update" "$PR_CI_GATE_UPDATE"
)

if [ "$FOREVER" = "1" ]; then
  ARGS+=("--forever")
fi

if [ "$ALLOW_DUPLICATE" = "1" ]; then
  ARGS+=("--allow-duplicate" "true")
else
  ARGS+=("--allow-duplicate" "false")
fi

if [ "$LOCAL_ONLY_WHILE_PR_BLOCKED" = "1" ]; then
  ARGS+=("--local-only-while-pr-blocked" "true")
else
  ARGS+=("--local-only-while-pr-blocked" "false")
fi

if [ "$DIRECT_MAIN_PUBLISH" = "1" ]; then
  ARGS+=("--direct-main-publish" "true")
else
  ARGS+=("--direct-main-publish" "false")
fi

if [ -n "$MAX_ITERATIONS" ] && [ "$MAX_ITERATIONS" -ne 0 ]; then
  ARGS+=("--max-iterations" "$MAX_ITERATIONS")
fi

if [ -n "$GENERATE_NEXT_SLEEP_SECONDS" ]; then
  ARGS+=("--generate-next-sleep-seconds" "$GENERATE_NEXT_SLEEP_SECONDS")
fi

# Run the command
exec "${AO2_BIN[@]}" "${ARGS[@]}"
