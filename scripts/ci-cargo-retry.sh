#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PHASE="${1:-unknown}"
OUT_ROOT="${AO2_CI_CARGO_RETRY_ROOT:-$ROOT/target/ci-cargo-retry/$PHASE}"
MAX_ATTEMPTS="${AO2_CI_CARGO_RETRY_MAX_ATTEMPTS:-3}"
SLEEP_SECONDS="${AO2_CI_CARGO_RETRY_SLEEP_SECONDS:-5}"
SUMMARY="$OUT_ROOT/summary.json"
COMBINED_LOG="$OUT_ROOT/output.log"
COMMAND_FILE="$OUT_ROOT/command.sh"

mkdir -p "$OUT_ROOT"

{
  printf '%s\n' 'set -euo pipefail'
  cat
} > "$COMMAND_FILE"
chmod +x "$COMMAND_FILE"
: > "$COMBINED_LOG"

json_summary() {
  local status="$1"
  local attempts="$2"
  local retried="$3"
  local transient="$4"
  local exit_code="$5"
  cat > "$SUMMARY" <<JSON
{
  "schema_version": "ao2.ci-cargo-retry.v1",
  "status": "$status",
  "phase": "$PHASE",
  "attempts": $attempts,
  "max_attempts": $MAX_ATTEMPTS,
  "retried": $retried,
  "transient_failure_detected": $transient,
  "exit_code": $exit_code,
  "log": "$COMBINED_LOG"
}
JSON
}

is_transient_cargo_network_failure() {
  local log_file="$1"
  grep -Eiq \
    'failed to get|download of|Connection reset by peer|Broken pipe|Failure when receiving data|Failed sending data|unable to update registry|crates-io|curl failed' \
    "$log_file"
}

attempt=1
retried=false
transient_failure_detected=false
last_exit_code=1

while [ "$attempt" -le "$MAX_ATTEMPTS" ]; do
  attempt_log="$OUT_ROOT/attempt-$attempt.log"
  {
    printf 'ao2-ci-cargo-retry phase=%s attempt=%s/%s\n' "$PHASE" "$attempt" "$MAX_ATTEMPTS"
  } | tee -a "$COMBINED_LOG"

  set +e
  bash "$COMMAND_FILE" 2>&1 | tee "$attempt_log"
  last_exit_code="${PIPESTATUS[0]}"
  set -e
  cat "$attempt_log" >> "$COMBINED_LOG"

  if [ "$last_exit_code" -eq 0 ]; then
    json_summary "passed" "$attempt" "$retried" "$transient_failure_detected" 0
    exit 0
  fi

  if ! is_transient_cargo_network_failure "$attempt_log"; then
    json_summary "failed" "$attempt" "$retried" false "$last_exit_code"
    exit "$last_exit_code"
  fi

  transient_failure_detected=true
  if [ "$attempt" -ge "$MAX_ATTEMPTS" ]; then
    json_summary "failed" "$attempt" "$retried" true "$last_exit_code"
    exit "$last_exit_code"
  fi

  retried=true
  printf 'transient Cargo network failure detected; retrying in %s seconds\n' "$SLEEP_SECONDS" | tee -a "$COMBINED_LOG"
  sleep "$SLEEP_SECONDS"
  attempt=$((attempt + 1))
done

json_summary "failed" "$attempt" "$retried" "$transient_failure_detected" "$last_exit_code"
exit "$last_exit_code"
