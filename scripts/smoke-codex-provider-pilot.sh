#!/bin/sh
set -eu

AO2_LIVE_CODEX_PILOT="${AO2_LIVE_CODEX_PILOT:-0}"
AO2_LIVE_CODEX_PILOT_REQUIRED="${AO2_LIVE_CODEX_PILOT_REQUIRED:-0}"
AO2_BIN="${AO2_BIN:-target/debug/ao2}"
AO2_CODEX_PROVIDER_PILOT_ROOT="${AO2_CODEX_PROVIDER_PILOT_ROOT:-$PWD/target/codex-provider-pilot/$(date +%Y%m%d%H%M%S)}"
AO2_CODEX_PROVIDER_PILOT_RUN_ID="${AO2_CODEX_PROVIDER_PILOT_RUN_ID:-live-codex-provider-pilot}"
AO2_PROVIDER_PILOT_MAX_BUDGET_USD="${AO2_PROVIDER_PILOT_MAX_BUDGET_USD:-1.00}"
AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION="${AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION:-}"
# Default acceptance path: run examples/risky-pr-run/risky-pr.yaml with Codex.
AO2_CODEX_PROVIDER_PILOT_WORKFLOW="${AO2_CODEX_PROVIDER_PILOT_WORKFLOW:-examples/risky-pr-run/risky-pr.yaml}"

if [ "$AO2_LIVE_CODEX_PILOT" != "1" ]; then
  printf "codex_provider_pilot_acceptance=skipped reason=explicit_flag_required\n"
  printf "hint=run AO2_LIVE_CODEX_PILOT=1 npm run smoke:provider:codex-pilot after Codex CLI OAuth login\n"
  exit 0
fi

if [ ! -x "$AO2_BIN" ]; then
  cargo build -p ao2-cli >/dev/null
fi

mkdir -p "$AO2_CODEX_PROVIDER_PILOT_ROOT"
AO2_CODEX_PROVIDER_PILOT_ROOT=$(CDPATH= cd -- "$AO2_CODEX_PROVIDER_PILOT_ROOT" && pwd)
repo="$AO2_CODEX_PROVIDER_PILOT_ROOT/discount-service"
prompt="$AO2_CODEX_PROVIDER_PILOT_ROOT/codex-pilot-prompt.txt"
doctor_out="$AO2_CODEX_PROVIDER_PILOT_ROOT/codex-doctor.json"
smoke_out="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-smoke-all.json"
smoke_err="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-smoke-all.err"
pilot_plan="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-pilot-plan.json"
run_out="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-pilot-run.out"
run_err="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-pilot-run.err"
replay_out="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-pilot-replay.json"
score_out="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-pilot-score.json"
pytest_log="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-pilot-pytest.log"
acceptance_bundle="$AO2_CODEX_PROVIDER_PILOT_ROOT/provider-pilot-acceptance.json"

if ! "$AO2_BIN" adapter doctor --provider codex >"$doctor_out" 2>&1; then
  cat "$doctor_out" >&2
  if [ "$AO2_LIVE_CODEX_PILOT_REQUIRED" = "1" ]; then
    exit 1
  fi
  printf "codex_provider_pilot_acceptance=skipped reason=doctor_failed root=%s\n" "$AO2_CODEX_PROVIDER_PILOT_ROOT"
  exit 0
fi

if ! grep -q '"available": true' "$doctor_out"; then
  if [ "$AO2_LIVE_CODEX_PILOT_REQUIRED" = "1" ]; then
    cat "$doctor_out" >&2
    exit 1
  fi
  printf "codex_provider_pilot_acceptance=skipped reason=codex_cli_unavailable root=%s\n" "$AO2_CODEX_PROVIDER_PILOT_ROOT"
  exit 0
fi

rm -rf -- "$repo"
cp -R fixtures/discount-service "$repo"
cat > "$prompt" <<'PROMPT'
Fix the discount validation bug in this Python project. Add input validation so
calculate_discount rejects negative prices and discount rates outside 0..1 with
ValueError. Add or update focused pytest regression coverage. Keep the change
minimal. At the end, print a short Summary line and a Changed files line.
PROMPT

if ! /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY AO2_LIVE_CODEX_SMOKE=1 \
  "$AO2_BIN" provider smoke-all \
    --target "$repo" \
    --live-provider codex \
    --json >"$smoke_out" 2>"$smoke_err"; then
  if grep -Eiq "auth|oauth|login|log in|not logged|unauthorized|permission" "$smoke_out" "$smoke_err"; then
    if [ "$AO2_LIVE_CODEX_PILOT_REQUIRED" = "1" ]; then
      cat "$smoke_out"
      cat "$smoke_err" >&2
      exit 1
    fi
    printf "codex_provider_pilot_acceptance=skipped reason=codex_oauth_unavailable root=%s\n" "$AO2_CODEX_PROVIDER_PILOT_ROOT"
    exit 0
  fi
  cat "$smoke_out"
  cat "$smoke_err" >&2
  exit 1
fi

grep -q '"schema": "ao2.provider-smoke-all.v1"' "$smoke_out"
grep -q '"provider": "codex"' "$smoke_out"
grep -q '"verdict": "ready"' "$smoke_out"

/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY AO2_LIVE_CODEX_SMOKE=1 \
  "$AO2_BIN" provider pilot \
    --target "$repo" \
    --provider codex \
    --provider-prompt-file "$prompt" \
    --run-id "$AO2_CODEX_PROVIDER_PILOT_RUN_ID" \
    --provider-max-budget-usd "$AO2_PROVIDER_PILOT_MAX_BUDGET_USD" \
    --json >"$pilot_plan"

grep -q '"schema": "ao2.provider-pilot-plan.v1"' "$pilot_plan"
grep -q '"status": "ready"' "$pilot_plan"

if ! /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY AO2_LIVE_CODEX_SMOKE=1 \
  "$AO2_BIN" run "$AO2_CODEX_PROVIDER_PILOT_WORKFLOW" \
    --target "$repo" \
    --run-id "$AO2_CODEX_PROVIDER_PILOT_RUN_ID" \
    --provider codex \
    --provider-prompt-file "$prompt" \
    --provider-max-budget-usd "$AO2_PROVIDER_PILOT_MAX_BUDGET_USD" \
    --max-repair-attempts 1 >"$run_out" 2>"$run_err"; then
  cat "$run_out"
  cat "$run_err" >&2
  exit 1
fi

"$AO2_BIN" replay "$AO2_CODEX_PROVIDER_PILOT_RUN_ID" --target "$repo" > "$replay_out"
"$AO2_BIN" report "$AO2_CODEX_PROVIDER_PILOT_RUN_ID" --target "$repo" >> "$run_out"
"$AO2_BIN" provider score --target "$repo" --run-id "$AO2_CODEX_PROVIDER_PILOT_RUN_ID" --json > "$score_out"

(cd "$repo" && python3 -m pytest) > "$pytest_log" 2>&1

grep -q '"status": "accepted"' "$replay_out"
jq -e '.digest_failures == []' "$replay_out" >/dev/null
grep -q '"schema": "ao2.provider-evidence-scorecard.v1"' "$score_out"
grep -q '"verdict": "ready"' "$score_out"
grep -q '"score": 100' "$score_out"
grep -q "all tests passed" "$pytest_log"

jq -n \
  --arg schema "ao2.codex-provider-pilot-acceptance.v1" \
  --arg provider "codex" \
  --arg release_candidate_version "$AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION" \
  --arg run_id "$AO2_CODEX_PROVIDER_PILOT_RUN_ID" \
  --arg root "$AO2_CODEX_PROVIDER_PILOT_ROOT" \
  --arg repo "$repo" \
  --arg prompt "$prompt" \
  --arg doctor_path "$doctor_out" \
  --arg smoke_path "$smoke_out" \
  --arg pilot_plan_path "$pilot_plan" \
  --arg run_stdout_path "$run_out" \
  --arg run_stderr_path "$run_err" \
  --arg replay_path "$replay_out" \
  --arg score_path "$score_out" \
  --arg pytest_path "$pytest_log" \
  --argjson max_budget_usd "$AO2_PROVIDER_PILOT_MAX_BUDGET_USD" \
  --arg evidence_pack "$repo/.ao2/runs/$AO2_CODEX_PROVIDER_PILOT_RUN_ID/evidence-pack/evidence-pack.json" \
  --arg cockpit "$repo/.ao2/runs/$AO2_CODEX_PROVIDER_PILOT_RUN_ID/cockpit/index.html" \
  --slurpfile smoke "$smoke_out" \
  --slurpfile pilot "$pilot_plan" \
  --slurpfile replay "$replay_out" \
  --slurpfile score "$score_out" \
  '{
    schema_version: $schema,
    status: "passed",
    source_class: "live",
    release_candidate_version: $release_candidate_version,
    provider: $provider,
    run_id: $run_id,
    root: $root,
    target: $repo,
    provider_prompt_file: $prompt,
    evidence_pack: $evidence_pack,
    cockpit: $cockpit,
    artifacts: {
      doctor: $doctor_path,
      smoke: $smoke_path,
      pilot_plan: $pilot_plan_path,
      run_stdout: $run_stdout_path,
      run_stderr: $run_stderr_path,
      replay: $replay_path,
      score: $score_path,
      pytest: $pytest_path
    },
    budget: {
      max_budget_usd: $max_budget_usd,
      provider_enforced: false,
      provider_enforcement_note: "Codex CLI does not expose a direct max-budget flag in the current exec interface; AO2 records the cap and bounds execution with timeout and repair budget.",
      timeout_seconds: 900,
      max_repair_attempts: 1
    },
    smoke: $smoke[0],
    pilot: $pilot[0],
    replay: $replay[0],
    score: $score[0]
  }' > "$acceptance_bundle"

printf "codex_provider_pilot_acceptance_root=%s\n" "$AO2_CODEX_PROVIDER_PILOT_ROOT"
printf "codex_provider_pilot_acceptance_bundle=%s\n" "$acceptance_bundle"
printf "codex_provider_pilot_evidence_pack=%s\n" "$repo/.ao2/runs/$AO2_CODEX_PROVIDER_PILOT_RUN_ID/evidence-pack/evidence-pack.json"
printf "codex_provider_pilot_cockpit=%s\n" "$repo/.ao2/runs/$AO2_CODEX_PROVIDER_PILOT_RUN_ID/cockpit/index.html"
printf "codex_provider_pilot_acceptance=passed\n"
