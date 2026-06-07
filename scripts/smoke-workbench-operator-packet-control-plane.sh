#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_WORKBENCH_OPERATOR_PACKET_CP_SMOKE_ROOT:-$ROOT/target/workbench-operator-packet-control-plane-smoke/$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_ID="${AO2_WORKBENCH_OPERATOR_PACKET_CP_RUN_ID:-workbench-operator-packet-cp-smoke}"
CP_PORT="${AO2_WORKBENCH_OPERATOR_PACKET_CP_PORT:-}"
PROFILE="${AO2_WORKBENCH_OPERATOR_PACKET_CP_PROFILE:-${AO2_OPERATOR_PACKET_CP_PROFILE:-release}}"
TOKEN="${AO2_WORKBENCH_OPERATOR_PACKET_CP_WORKBENCH_TOKEN:-workbench-operator-packet-smoke-token}"
SIGNER_ID="${AO2_WORKBENCH_OPERATOR_PACKET_CP_SIGNER_ID:-ao2-workbench-operator-smoke}"

case "$PROFILE" in
  release)
    TARGET_SUBDIR="release"
    ;;
  debug)
    TARGET_SUBDIR="debug"
    ;;
  *)
    echo "unsupported AO2_WORKBENCH_OPERATOR_PACKET_CP_PROFILE=$PROFILE; expected release or debug" >&2
    exit 1
    ;;
esac

cargo_build_profile() {
  if [ "$PROFILE" = "release" ]; then
    cargo build --release "$@"
  else
    cargo build "$@"
  fi
}

python_command() {
  if [ -n "${PYTHON:-}" ]; then
    printf "%s\n" "$PYTHON"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    command -v python3
    return
  fi
  if command -v python >/dev/null 2>&1; then
    command -v python
    return
  fi
  echo "missing python interpreter; set PYTHON=/path/to/python" >&2
  return 1
}

exe_suffix() {
  case "$(uname -s 2>/dev/null || true)" in
    MINGW* | MSYS* | CYGWIN*)
      printf ".exe"
      ;;
    *)
      printf ""
      ;;
  esac
}

binary_path() {
  local base="$1"
  if [ -f "$base" ]; then
    printf "%s\n" "$base"
    return
  fi
  if [ -n "$EXE_SUFFIX" ] && [ -f "$base$EXE_SUFFIX" ]; then
    printf "%s\n" "$base$EXE_SUFFIX"
    return
  fi
  if [ -n "$EXE_SUFFIX" ]; then
    printf "%s\n" "$base$EXE_SUFFIX"
  else
    printf "%s\n" "$base"
  fi
}

choose_port() {
  "$PYTHON_BIN" - <<'PY'
import socket
with socket.socket() as s:
    s.bind(("127.0.0.1", 0))
    print(s.getsockname()[1])
PY
}

require_file() {
  if [ ! -f "$1" ]; then
    echo "missing required file: $1" >&2
    exit 1
  fi
}

PYTHON_BIN="$(python_command)"
EXE_SUFFIX="$(exe_suffix)"

if [ -z "$CP_PORT" ]; then
  CP_PORT="$(choose_port)"
fi

mkdir -p "$OUT_ROOT"
TOKEN_FILE="$OUT_ROOT/api-token"
CP_DATA_DIR="$OUT_ROOT/control-plane-data"
FIXTURE_ROOT="$OUT_ROOT/fixture"
REPO="$FIXTURE_ROOT/discount-service"
SIGNING_KEY="$OUT_ROOT/workbench-operator-packet-support-key.pem"
SERVE_LOG="$OUT_ROOT/workbench-serve.log"
SERVE_ERR="$OUT_ROOT/workbench-serve.err"
PUBLISH_JSON="$OUT_ROOT/publish.json"
DASHBOARD_JSON="$OUT_ROOT/dashboard.json"
DETAIL_JSON="$OUT_ROOT/detail.json"
LATEST_JSON="$OUT_ROOT/latest.json"
RAW_JSON="$OUT_ROOT/raw.json"
SIGNATURE_JSON="$OUT_ROOT/signature.json"
SUMMARY_JSON="$OUT_ROOT/summary.json"

"$PYTHON_BIN" - "$TOKEN_FILE" <<'PY'
import secrets
import stat
import sys
from pathlib import Path

path = Path(sys.argv[1])
path.write_text(secrets.token_hex(32) + "\n", encoding="utf-8")
path.chmod(stat.S_IRUSR | stat.S_IWUSR)
PY

cp_token="$(cat "$TOKEN_FILE")"
BASE_URL="http://127.0.0.1:$CP_PORT"

echo "smoke_root=$OUT_ROOT"
echo "control_plane_url=$BASE_URL"
echo "profile=$PROFILE"

echo "=== build ao2 ==="
cargo_build_profile -p ao2-cli
AO2_BIN="$(binary_path "$ROOT/target/$TARGET_SUBDIR/ao2")"

echo "=== build ao2-control-plane ==="
(cd "$CP_ROOT" && cargo_build_profile -p ao2-cp-server)
CP_SERVER_BIN="$(binary_path "$CP_ROOT/target/$TARGET_SUBDIR/ao2-cp-server")"

echo "=== start ephemeral control plane ==="
mkdir -p "$CP_DATA_DIR"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  AO2_CP_API_TOKEN="$cp_token" \
  AO2_CP_BIND="127.0.0.1:$CP_PORT" \
  AO2_CP_DATA_DIR="$CP_DATA_DIR" \
  "$CP_SERVER_BIN" \
  > "$OUT_ROOT/ao2-cp-server.log" \
  2> "$OUT_ROOT/ao2-cp-server.err" &
CP_PID=$!
WB_PID=""
cleanup() {
  if [ -n "$WB_PID" ]; then
    kill "$WB_PID" >/dev/null 2>&1 || true
    wait "$WB_PID" >/dev/null 2>&1 || true
  fi
  kill "$CP_PID" >/dev/null 2>&1 || true
  wait "$CP_PID" >/dev/null 2>&1 || true
  rm -f "$SIGNING_KEY"
}
trap cleanup EXIT

for _ in $(seq 1 50); do
  if curl -fsS "$BASE_URL/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done
curl -fsS "$BASE_URL/healthz" > "$OUT_ROOT/healthz.json"

echo "=== ao2 run governed fixture ==="
rm -rf "$FIXTURE_ROOT"
mkdir -p "$FIXTURE_ROOT"
cp -R "$ROOT/fixtures/discount-service" "$REPO"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" \
  run "$ROOT/examples/risky-pr-run/risky-pr.yaml" \
  --target "$REPO" \
  --run-id "$RUN_ID" \
  > "$OUT_ROOT/ao2-run.txt"

require_file "$REPO/.ao2/runs/$RUN_ID/run-record.json"
require_file "$REPO/.ao2/runs/$RUN_ID/evidence-pack/evidence-pack.json"

echo "=== start workbench ==="
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" \
  workbench support-keygen \
  --out "$SIGNING_KEY" \
  --bits 2048 \
  > "$OUT_ROOT/support-keygen.txt"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" \
  workbench serve \
  --target "$REPO" \
  --port 0 \
  --once \
  --api-token "$TOKEN" \
  --support-signing-key "$SIGNING_KEY" \
  --support-signer-id "$SIGNER_ID" \
  > "$SERVE_LOG" 2> "$SERVE_ERR" &
WB_PID=$!

WORKBENCH_PORT=""
attempt=1
while [ "$attempt" -le 100 ]; do
  if [ -s "$SERVE_LOG" ]; then
    WORKBENCH_PORT="$(sed -n 's#url=http://127.0.0.1:\([0-9][0-9]*\)/#\1#p' "$SERVE_LOG" | head -1)"
    if [ -n "$WORKBENCH_PORT" ]; then
      break
    fi
  fi
  sleep 0.1
  attempt=$((attempt + 1))
done

if [ -z "$WORKBENCH_PORT" ]; then
  cat "$SERVE_LOG" >&2 || true
  cat "$SERVE_ERR" >&2 || true
  exit 1
fi

echo "=== publish real Workbench operator packet ==="
curl -fsS -X POST "http://127.0.0.1:$WORKBENCH_PORT/api/runs/evidence/publish?token=$TOKEN" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "kind=operator-packet" \
  --data-urlencode "run_id=$RUN_ID" \
  --data-urlencode "control_plane_url=$BASE_URL" \
  --data-urlencode "api_token=$cp_token" \
  -o "$PUBLISH_JSON"

wait "$WB_PID"
WB_PID=""

SHA="$("$PYTHON_BIN" - "$PUBLISH_JSON" <<'PY'
import json
import sys
from pathlib import Path
payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload["receipt"]["sha256"], end="")
PY
)"

echo "=== read back observer endpoints ==="
auth_header="Authorization: Bearer ${cp_token}"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/operator-packet/dashboard.json" > "$DASHBOARD_JSON"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/operator-packet/$SHA/detail.json" > "$DETAIL_JSON"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/operator-packet/run/$RUN_ID/latest" > "$LATEST_JSON"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/operator-packet/$SHA" > "$RAW_JSON"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/operator-packet/$SHA/signature" > "$SIGNATURE_JSON"

"$PYTHON_BIN" - "$SUMMARY_JSON" "$PUBLISH_JSON" "$DASHBOARD_JSON" "$DETAIL_JSON" "$LATEST_JSON" "$RAW_JSON" "$SIGNATURE_JSON" "$RUN_ID" "$BASE_URL" "$SHA" "$SIGNER_ID" <<'PY'
import json
import sys
from pathlib import Path

summary, publish_path, dashboard_path, detail_path, latest_path, raw_path, signature_path, run_id, base_url, sha, signer_id = sys.argv[1:]
publish = json.loads(Path(publish_path).read_text(encoding="utf-8"))
dashboard = json.loads(Path(dashboard_path).read_text(encoding="utf-8"))
detail = json.loads(Path(detail_path).read_text(encoding="utf-8"))
latest = json.loads(Path(latest_path).read_text(encoding="utf-8"))
raw = json.loads(Path(raw_path).read_text(encoding="utf-8"))
signature = json.loads(Path(signature_path).read_text(encoding="utf-8"))

expected = {
    "publish": "ao2.operator-packet-control-plane-publish.v1",
    "receipt": "ao2.cp-ingest-receipt.v1",
    "dashboard": "ao2.cp-operator-packet-dashboard.v1",
    "detail": "ao2.cp-operator-packet-detail.v1",
    "latest": "ao2.cp-operator-packet-detail.v1",
    "raw": "ao2.operator-evidence-packet.v1",
    "signature": "ao2.cp-operator-packet-signature.v1",
}
observed = {
    "publish": publish.get("schema_version"),
    "receipt": publish.get("receipt", {}).get("schema_version"),
    "dashboard": dashboard.get("schema_version"),
    "detail": detail.get("schema_version"),
    "latest": latest.get("schema_version"),
    "raw": raw.get("schema_version"),
    "signature": signature.get("schema_version"),
}
for key, schema in expected.items():
    if observed.get(key) != schema:
        raise SystemExit(f"{key} schema mismatch: {observed.get(key)} != {schema}")
if publish["receipt"]["sha256"] != sha:
    raise SystemExit("publish sha mismatch")
if publish["receipt"].get("ingested_schema_version") != "ao2.operator-evidence-packet.v1":
    raise SystemExit("receipt ingested schema mismatch")
if publish.get("endpoint") != f"{base_url}/api/v1/operator-packet/signed":
    raise SystemExit("publish endpoint mismatch")
if publish.get("publish_kind") != "operator-packet":
    raise SystemExit("publish kind mismatch")
if detail.get("run_id") != run_id or latest.get("run_id") != run_id or raw.get("run_id") != run_id:
    raise SystemExit("run_id readback mismatch")
if signature.get("operator_packet_sha256") != sha:
    raise SystemExit("signature sidecar sha mismatch")
signature_block = signature.get("signature", {})
if signature_block.get("signer_id") != signer_id:
    raise SystemExit("signature signer mismatch")
if signature_block.get("signature_verified") is not True:
    raise SystemExit("signature verification mismatch")
if not dashboard.get("summary", {}).get("read_only_observer", False):
    raise SystemExit("dashboard did not declare read-only observer")
for label, payload in [("detail", detail), ("latest", latest)]:
    trust = payload.get("trust_boundary", {})
    if trust.get("role") != "read_only_observer_for_signed_operator_packets":
        raise SystemExit(f"{label} trust boundary role mismatch")
    if trust.get("can_approve_runs") is not False:
        raise SystemExit(f"{label} trust boundary must not approve runs")
    if trust.get("can_mutate_ao2_evidence") is not False:
        raise SystemExit(f"{label} trust boundary must not mutate AO2 evidence")

if raw.get("run_record", {}).get("run_id") != run_id:
    raise SystemExit("raw operator packet run_record mismatch")
if raw.get("evidence_pack", {}).get("schema_version") != "ao2.evidence-pack.v1":
    raise SystemExit("raw operator packet evidence_pack schema mismatch")
if raw.get("evaluator_closure", {}).get("verdict") != "accepted":
    raise SystemExit("raw operator packet evaluator closure mismatch")
if raw.get("replay", {}).get("status") != "accepted":
    raise SystemExit("raw operator packet replay status mismatch")
if raw.get("provider_scorecard", {}).get("present") is not True:
    raise SystemExit("raw operator packet provider scorecard missing")
if len(str(raw.get("artifacts", {}).get("run_record", {}).get("sha256", ""))) != 64:
    raise SystemExit("raw operator packet run_record sha missing")
if len(str(raw.get("artifacts", {}).get("evidence_pack", {}).get("sha256", ""))) != 64:
    raise SystemExit("raw operator packet evidence_pack sha missing")

payload = {
    "schema_version": "ao2.workbench-operator-packet-control-plane-smoke.v1",
    "status": "passed",
    "run_id": run_id,
    "control_plane_url": base_url,
    "published_sha256": sha,
    "contract_schemas": observed,
    "operator_packet": {
        "schema_version": raw.get("schema_version"),
        "run_record_run_id": raw.get("run_record", {}).get("run_id"),
        "evidence_pack_schema_version": raw.get("evidence_pack", {}).get("schema_version"),
        "evaluator_closure_verdict": raw.get("evaluator_closure", {}).get("verdict"),
        "replay_status": raw.get("replay", {}).get("status"),
        "provider_score_present": raw.get("provider_scorecard", {}).get("present"),
    },
    "read_only_observer": True,
    "can_approve_runs": False,
    "can_mutate_ao2_evidence": False,
    "token_leak_detected": False,
}
Path(summary).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

"$PYTHON_BIN" - "$cp_token" "$TOKEN" "$SUMMARY_JSON" "$PUBLISH_JSON" "$DASHBOARD_JSON" "$DETAIL_JSON" "$LATEST_JSON" "$RAW_JSON" "$SIGNATURE_JSON" <<'PY'
import sys
from pathlib import Path

tokens = [value for value in sys.argv[1:3] if value]
for raw in sys.argv[3:]:
    text = Path(raw).read_text(encoding="utf-8", errors="replace")
    for token in tokens:
        if token in text:
            print(f"token_leak_detected={raw}", file=sys.stderr)
            raise SystemExit(1)
PY

echo "summary=$SUMMARY_JSON"
echo "status=passed"
