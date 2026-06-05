#!/bin/sh
set -eu

AO2_BIN="${AO2_BIN:-target/release/ao2}"
AO2_WORKBENCH_RELEASE_COMPARISON_ROOT="${AO2_WORKBENCH_RELEASE_COMPARISON_ROOT:-$PWD/target/workbench-release-comparison-export/$(date +%Y%m%d%H%M%S)}"
AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR="${AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR:-}"
AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON="${AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON:-$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/workbench-release-comparison-export.json}"
AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON="${AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON:-$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/workbench-support-preview.json}"
AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_EXPORT_JSON="${AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_EXPORT_JSON:-$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/workbench-support-export.json}"
AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON="${AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON:-$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/workbench-support-inspect.json}"
AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN="${AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN:-workbench-release-comparison-smoke}"
AO2_WORKBENCH_RELEASE_COMPARISON_SIGNER_ID="${AO2_WORKBENCH_RELEASE_COMPARISON_SIGNER_ID:-workbench-release-comparison-smoke}"

ao2_cmd() {
  if [ -x "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  else
    cargo run -p ao2-cli --quiet -- "$@"
  fi
}

mkdir -p "$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT"
AO2_WORKBENCH_RELEASE_COMPARISON_ROOT=$(CDPATH= cd -- "$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT" && pwd)

repo="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/repo"
signing_key="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/support-key.pem"
redaction_prompt="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/redaction-canary-prompt.sh"
redaction_start_json="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/redaction-canary-start.json"
redaction_queue_json="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/redaction-canary-queue.json"
serve_log="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/workbench-serve.log"
serve_err="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/workbench-serve.err"

rm -rf "$repo"
cp -R fixtures/discount-service "$repo"
ao2_cmd workbench support-keygen --out "$signing_key" --bits 2048 >/dev/null
cat > "$redaction_prompt" <<'PROMPT'
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: support redaction canary updated discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'OPENAI_API_KEY=sk-release-redaction-canary\n'
printf 'Authorization: Bearer bearer-release-redaction-canary\n'
printf 'callback=https://example.com/hook?token=url-release-redaction-canary&api_key=query-key-release-redaction-canary&signature=query-signature-release-redaction-canary&safe=ok\n'
PROMPT

if [ -z "$AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR" ]; then
  AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR=$(
    ls -td target/release-comparison-bundles/release-comparison-* 2>/dev/null | head -1 || true
  )
fi

if [ -z "$AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR" ]; then
  echo "release comparison bundle dir is required or target/release-comparison-bundles must contain a bundle" >&2
  exit 1
fi

ao2_cmd workbench serve \
  --target "$repo" \
  --port 0 \
  --api-token "$AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN" \
  --enable-execution \
  --support-signing-key "$signing_key" \
  --support-signer-id "$AO2_WORKBENCH_RELEASE_COMPARISON_SIGNER_ID" \
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

curl -fsS -X POST "http://127.0.0.1:$port/api/queue/start?token=$AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "template=bug-fix" \
  --data-urlencode "provider=scripted" \
  --data-urlencode "run_id=release-redaction-canary" \
  --data-urlencode "provider_prompt_file=$redaction_prompt" \
  --data-urlencode "max_repair_attempts=1" \
  -o "$redaction_start_json"

attempt=1
redaction_status=""
while [ "$attempt" -le 300 ]; do
  curl -fsS "http://127.0.0.1:$port/api/queue?token=$AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN" \
    -o "$redaction_queue_json"
  redaction_status=$(jq -r '.jobs[] | select(.run_id == "release-redaction-canary") | .status' "$redaction_queue_json" | head -1)
  if [ "$redaction_status" = "accepted" ]; then
    break
  fi
  if [ "$redaction_status" = "failed" ] || [ "$redaction_status" = "rejected" ] || [ "$redaction_status" = "cancelled" ]; then
    cat "$redaction_queue_json" >&2 || true
    exit 1
  fi
  sleep 0.1
  attempt=$((attempt + 1))
done

if [ "$redaction_status" != "accepted" ]; then
  cat "$redaction_queue_json" >&2 || true
  echo "release redaction canary did not reach accepted status" >&2
  exit 1
fi

redaction_stdout_log=$(jq -r '.jobs[] | select(.run_id == "release-redaction-canary") | .stdout_log' "$redaction_queue_json" | head -1)
if [ -z "$redaction_stdout_log" ] || [ ! -f "$redaction_stdout_log" ]; then
  cat "$redaction_queue_json" >&2 || true
  echo "release redaction canary stdout log missing" >&2
  exit 1
fi
{
  printf 'OPENAI_API_KEY=sk-release-redaction-canary\n'
  printf 'Authorization: Bearer bearer-release-redaction-canary\n'
  printf 'callback=https://example.com/hook?token=url-release-redaction-canary&api_key=query-key-release-redaction-canary&signature=query-signature-release-redaction-canary&safe=ok\n'
} >> "$redaction_stdout_log"

curl -fsS -X POST "http://127.0.0.1:$port/api/runs/evidence/export?token=$AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "kind=release-comparison-verification" \
  --data-urlencode "bundle_dir=$AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR" \
  -o "$AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON"

curl -fsS -X POST "http://127.0.0.1:$port/api/queue/export-preview?token=$AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN" \
  -H "Content-Length: 0" \
  -o "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON"

curl -fsS -X POST "http://127.0.0.1:$port/api/queue/export?token=$AO2_WORKBENCH_RELEASE_COMPARISON_TOKEN" \
  -H "Content-Length: 0" \
  -o "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_EXPORT_JSON"

support_bundle=$(awk -F '"' '/"bundle_path"/ { print $4; exit }' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_EXPORT_JSON")
if [ -z "$support_bundle" ]; then
  echo "support bundle path missing from $AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_EXPORT_JSON" >&2
  exit 1
fi

ao2_cmd workbench support-inspect \
  --bundle-dir "$(dirname "$support_bundle")" \
  --json > "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON"

grep -q '"export_kind": "release-comparison-verification"' "$AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON"
grep -q '"status": "verified"' "$AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON"
grep -q '"signature_verified": true' "$AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON"
grep -q '"schema_version": "ao2.workbench-support-bundle-preview.v1"' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON"
grep -q '"schema_version": "ao2.workbench-support-redaction-preview.v1"' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON"
grep -q '"would_write_bundle": false' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON"
jq -e '.redaction_audit.redaction_count > 0' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON" >/dev/null
jq -e '.redaction_audit.secret_classes.provider_api_key == 1' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON" >/dev/null
jq -e '.redaction_audit.secret_classes.bearer_authorization == 1' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON" >/dev/null
jq -e '.redaction_audit.secret_classes.query_token == 1' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON" >/dev/null
jq -e '.redaction_audit.secret_classes.query_api_key == 1' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON" >/dev/null
jq -e '.redaction_audit.secret_classes.query_signature == 1' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON" >/dev/null
grep -q '"evidence_export_count": 1' "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON"
grep -q '"kind": "release-comparison-verification"' "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON"
grep -q '"release_comparison_signature_verified": true' "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON"
jq -e '.redaction_audit.redaction_count > 0' "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON" >/dev/null
jq -e '.redaction_audit.secret_classes.provider_api_key == 1' "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON" >/dev/null
if grep -q 'sk-release-redaction-canary\|bearer-release-redaction-canary\|url-release-redaction-canary\|query-key-release-redaction-canary\|query-signature-release-redaction-canary' "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_EXPORT_JSON" "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON"; then
  echo "raw redaction canary secret leaked into support artifacts" >&2
  exit 1
fi

printf "workbench_release_comparison_root=%s\n" "$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT"
printf "workbench_release_comparison_bundle_dir=%s\n" "$AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR"
printf "workbench_release_comparison_export=%s\n" "$AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON"
printf "workbench_release_comparison_support_preview=%s\n" "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_PREVIEW_JSON"
printf "workbench_release_comparison_support_export=%s\n" "$AO2_WORKBENCH_RELEASE_COMPARISON_SUPPORT_EXPORT_JSON"
printf "workbench_release_comparison_support_inspect=%s\n" "$AO2_WORKBENCH_RELEASE_COMPARISON_INSPECT_JSON"
printf "workbench_release_comparison_export=passed\n"
