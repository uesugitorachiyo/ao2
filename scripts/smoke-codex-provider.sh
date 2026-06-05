#!/bin/sh
set -eu

AO2_LIVE_CODEX_SMOKE="${AO2_LIVE_CODEX_SMOKE:-0}"
AO2_LIVE_CODEX_REQUIRED="${AO2_LIVE_CODEX_REQUIRED:-0}"
AO2_BIN="${AO2_BIN:-target/debug/ao2}"
AO2_CODEX_SMOKE_ROOT="${AO2_CODEX_SMOKE_ROOT:-$PWD/target/codex-provider-smoke/$(date +%Y%m%d%H%M%S)}"

if [ "$AO2_LIVE_CODEX_SMOKE" != "1" ]; then
  printf "codex_provider_smoke=skipped reason=explicit_flag_required\n"
  printf "hint=run AO2_LIVE_CODEX_SMOKE=1 npm run smoke:provider:codex after Codex CLI OAuth login\n"
  exit 0
fi

if [ ! -x "$AO2_BIN" ]; then
  cargo build -p ao2-cli >/dev/null
fi

mkdir -p "$AO2_CODEX_SMOKE_ROOT"
AO2_CODEX_SMOKE_ROOT=$(CDPATH= cd -- "$AO2_CODEX_SMOKE_ROOT" && pwd)
doctor_out="$AO2_CODEX_SMOKE_ROOT/codex-doctor.json"

if ! "$AO2_BIN" adapter doctor --provider codex >"$doctor_out" 2>&1; then
  cat "$doctor_out" >&2
  if [ "$AO2_LIVE_CODEX_REQUIRED" = "1" ]; then
    exit 1
  fi
  printf "codex_provider_smoke=skipped reason=doctor_failed root=%s\n" "$AO2_CODEX_SMOKE_ROOT"
  exit 0
fi

if ! grep -q '"available": true' "$doctor_out"; then
  if [ "$AO2_LIVE_CODEX_REQUIRED" = "1" ]; then
    cat "$doctor_out" >&2
    exit 1
  fi
  printf "codex_provider_smoke=skipped reason=codex_cli_unavailable root=%s\n" "$AO2_CODEX_SMOKE_ROOT"
  exit 0
fi

smoke_out="$AO2_CODEX_SMOKE_ROOT/provider-smoke-all.json"
smoke_err="$AO2_CODEX_SMOKE_ROOT/provider-smoke-all.err"

if ! /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY AO2_LIVE_CODEX_SMOKE=1 \
  "$AO2_BIN" provider smoke-all \
    --target "$AO2_CODEX_SMOKE_ROOT" \
    --live-provider codex \
    --json >"$smoke_out" 2>"$smoke_err"; then
  if grep -Eiq "auth|oauth|login|log in|not logged|unauthorized|permission" "$smoke_out" "$smoke_err"; then
    if [ "$AO2_LIVE_CODEX_REQUIRED" = "1" ]; then
      cat "$smoke_out"
      cat "$smoke_err" >&2
      exit 1
    fi
    printf "codex_provider_smoke=skipped reason=codex_oauth_unavailable root=%s\n" "$AO2_CODEX_SMOKE_ROOT"
    exit 0
  fi
  cat "$smoke_out"
  cat "$smoke_err" >&2
  exit 1
fi

grep -q '"schema": "ao2.provider-smoke-all.v1"' "$smoke_out"
grep -q '"provider": "codex"' "$smoke_out"
grep -q '"verdict": "ready"' "$smoke_out"
test -f "$AO2_CODEX_SMOKE_ROOT/.ao2/provider-smoke/history.json"

printf "codex_provider_smoke_root=%s\n" "$AO2_CODEX_SMOKE_ROOT"
printf "codex_provider_smoke_report=%s\n" "$smoke_out"
printf "codex_provider_smoke_history=%s\n" "$AO2_CODEX_SMOKE_ROOT/.ao2/provider-smoke/history.json"
printf "codex_provider_smoke=passed\n"
