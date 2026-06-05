#!/bin/sh
set -eu

AO2_BIN="${AO2_BIN:-target/release/ao2}"
AO2_WORKBENCH_PROVIDER_PILOT_ROOT="${AO2_WORKBENCH_PROVIDER_PILOT_ROOT:-$PWD/target/workbench-provider-pilot-acceptance-export/$(date +%Y%m%d%H%M%S)}"
AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="${AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE:-}"
AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT="${AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT:-}"
AO2_WORKBENCH_PROVIDER_PILOT_DASHBOARD_HTML="${AO2_WORKBENCH_PROVIDER_PILOT_DASHBOARD_HTML:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-provider-pilot-dashboard.html}"
AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-provider-pilot-latest-acceptance.json}"
AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-provider-pilot-cost-ledger.json}"
AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-provider-pilot-cost-trend.json}"
AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_LATEST_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_LATEST_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-provider-pilot-export-latest.json}"
AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-provider-pilot-acceptance-export.json}"
AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_PREVIEW_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_PREVIEW_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-support-preview.json}"
AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_EXPORT_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_EXPORT_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-support-export.json}"
AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-support-inspect.json}"
AO2_WORKBENCH_PROVIDER_PILOT_TOKEN="${AO2_WORKBENCH_PROVIDER_PILOT_TOKEN:-workbench-provider-pilot-smoke}"
AO2_WORKBENCH_PROVIDER_PILOT_SIGNER_ID="${AO2_WORKBENCH_PROVIDER_PILOT_SIGNER_ID:-workbench-provider-pilot-smoke}"

ao2_cmd() {
  if [ -x "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  else
    cargo run -p ao2-cli --quiet -- "$@"
  fi
}

if [ -z "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" ]; then
  echo "AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE is required" >&2
  exit 1
fi

if [ ! -f "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" ]; then
  echo "provider pilot acceptance bundle not found: $AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" >&2
  exit 1
fi

if [ -z "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT" ]; then
  AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT=$(dirname "$(dirname "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE")")
fi

mkdir -p "$AO2_WORKBENCH_PROVIDER_PILOT_ROOT"
AO2_WORKBENCH_PROVIDER_PILOT_ROOT=$(CDPATH= cd -- "$AO2_WORKBENCH_PROVIDER_PILOT_ROOT" && pwd)

repo="$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/repo"
signing_key="$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/support-key.pem"
serve_log="$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-serve.log"
serve_err="$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-serve.err"

rm -rf "$repo"
cp -R fixtures/discount-service "$repo"
ao2_cmd workbench support-keygen --out "$signing_key" --bits 2048 >/dev/null

ao2_cmd workbench serve \
  --target "$repo" \
  --port 0 \
  --api-token "$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  --enable-execution \
  --support-signing-key "$signing_key" \
  --support-signer-id "$AO2_WORKBENCH_PROVIDER_PILOT_SIGNER_ID" \
  > "$serve_log" 2> "$serve_err" &
server_pid=$!

cleanup() {
  kill "$server_pid" >/dev/null 2>&1 || true
  wait "$server_pid" >/dev/null 2>&1 || true
  rm -f "$signing_key"
}
trap cleanup EXIT

port=""
attempt=1
while [ "$attempt" -le 100 ]; do
  if [ -s "$serve_log" ]; then
    port=$(sed -n 's#url=http://127.0.0.1:\([0-9][0-9]*\)/#\1#p' "$serve_log" | head -1)
    if [ -n "$port" ]; then
      break
    fi
  fi
  sleep 0.1
  attempt=$((attempt + 1))
done

if [ -z "$port" ]; then
  cat "$serve_log" >&2 || true
  cat "$serve_err" >&2 || true
  exit 1
fi

acceptance_provider=$(jq -r '.provider' "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE")
curl -fsS "http://127.0.0.1:$port/" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_DASHBOARD_HTML"

curl -fsS -G "http://127.0.0.1:$port/api/provider-pilot/acceptance/latest" \
  --data-urlencode "token=$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  --data-urlencode "provider=$acceptance_provider" \
  --data-urlencode "acceptance_root=$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT" \
  --data-urlencode "history_replay_status=accepted" \
  --data-urlencode "history_min_score=90" \
  --data-urlencode "history_sort=score_desc" \
  --data-urlencode "history_limit=10" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"

curl -fsS -G "http://127.0.0.1:$port/api/provider-pilot/cost-ledger" \
  --data-urlencode "token=$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  --data-urlencode "acceptance_root=$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON"

curl -fsS -G "http://127.0.0.1:$port/api/provider-pilot/cost-trend" \
  --data-urlencode "token=$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  --data-urlencode "acceptance_root=$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON"

curl -fsS -X POST "http://127.0.0.1:$port/api/runs/evidence/export?token=$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "kind=provider-pilot-acceptance" \
  --data-urlencode "acceptance_bundle=$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"

curl -fsS -X POST "http://127.0.0.1:$port/api/provider-pilot/acceptance/export-latest?token=$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "provider=$acceptance_provider" \
  --data-urlencode "acceptance_root=$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_ROOT" \
  --data-urlencode "history_replay_status=accepted" \
  --data-urlencode "history_min_score=90" \
  --data-urlencode "history_sort=score_desc" \
  --data-urlencode "history_limit=10" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_LATEST_JSON"

curl -fsS -X POST "http://127.0.0.1:$port/api/queue/export-preview?token=$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  -H "Content-Length: 0" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_PREVIEW_JSON"

curl -fsS -X POST "http://127.0.0.1:$port/api/queue/export?token=$AO2_WORKBENCH_PROVIDER_PILOT_TOKEN" \
  -H "Content-Length: 0" \
  -o "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_EXPORT_JSON"

support_bundle=$(awk -F '"' '/"bundle_path"/ { print $4; exit }' "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_EXPORT_JSON")
if [ -z "$support_bundle" ]; then
  echo "support bundle path missing from $AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_EXPORT_JSON" >&2
  exit 1
fi

ao2_cmd workbench support-inspect \
  --bundle-dir "$(dirname "$support_bundle")" \
  --json > "$AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON"

grep -q '"export_kind": "provider-pilot-acceptance"' "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"
grep -q '"schema_version": "ao2.workbench-latest-provider-pilot-acceptance.v1"' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"status": "passed"' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"replay_status": "accepted"' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"acceptance_filter"' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"schema_version": "ao2.workbench-provider-pilot-acceptance-trend.v1"' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"replay_status": "accepted"' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"min_score": 90' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"sort": "score_desc"' "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
grep -q '"schema_version": "ao2.provider-cost-ledger.v1"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON"
grep -q '"status": "ready"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON"
grep -q "\"provider\": \"$acceptance_provider\"" "$AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON"
grep -q '"max_budget_usd"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON"
grep -q '"total_tokens"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON"
grep -q '"schema_version": "ao2.provider-cost-trend.v1"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON"
grep -q '"status": "ready"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON"
grep -q '"latest_release_tag"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON"
grep -q '"delta"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON"
grep -q '"releases"' "$AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON"
grep -q 'provider-pilot-cost-trend-chart' "$AO2_WORKBENCH_PROVIDER_PILOT_DASHBOARD_HTML"
grep -q 'Provider pilot cost trend chart' "$AO2_WORKBENCH_PROVIDER_PILOT_DASHBOARD_HTML"
grep -q '"status": "passed"' "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"
grep -q '"replay"' "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"
grep -q '"evidence_export_count": 2' "$AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON"
grep -q '"kind": "provider-pilot-acceptance"' "$AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON"
grep -q '"provider_pilot_score": 100' "$AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON"
grep -q '"provider_pilot_replay_status": "accepted"' "$AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON"
grep -q '"provider_pilot_digest_failure_count": 0' "$AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON"
grep -q '"schema_version": "ao2.workbench-provider-pilot-acceptance-export-latest.v1"' "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_LATEST_JSON"
grep -q '"export_kind": "provider-pilot-acceptance"' "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_LATEST_JSON"
grep -q '"replay_status": "accepted"' "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_LATEST_JSON"
grep -q '"schema_version": "ao2.workbench-support-bundle-preview.v1"' "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_PREVIEW_JSON"
grep -q '"schema_version": "ao2.workbench-support-redaction-preview.v1"' "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_PREVIEW_JSON"
grep -q '"would_write_bundle": false' "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_PREVIEW_JSON"

printf "workbench_provider_pilot_acceptance_root=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_ROOT"
printf "workbench_provider_pilot_acceptance_bundle=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE"
printf "workbench_provider_pilot_dashboard=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_DASHBOARD_HTML"
printf "workbench_provider_pilot_latest_acceptance=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_LATEST_JSON"
printf "workbench_provider_pilot_cost_ledger=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_COST_LEDGER_JSON"
printf "workbench_provider_pilot_cost_trend=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_COST_TREND_JSON"
printf "workbench_provider_pilot_acceptance_export=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"
printf "workbench_provider_pilot_export_latest=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_LATEST_JSON"
printf "workbench_provider_pilot_acceptance_support_preview=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_PREVIEW_JSON"
printf "workbench_provider_pilot_acceptance_support_export=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_SUPPORT_EXPORT_JSON"
printf "workbench_provider_pilot_acceptance_support_inspect=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_INSPECT_JSON"
printf "workbench_provider_pilot_acceptance_export=passed\n"
