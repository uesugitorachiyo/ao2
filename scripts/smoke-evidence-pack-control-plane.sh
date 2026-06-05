#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_EVIDENCE_CP_SMOKE_ROOT:-$ROOT/target/evidence-pack-control-plane-smoke/$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_ID="${AO2_EVIDENCE_CP_RUN_ID:-evidence-cp-smoke}"
PORT="${AO2_EVIDENCE_CP_PORT:-}"

choose_port() {
  python3 - <<'PY'
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

if [ -z "$PORT" ]; then
  PORT="$(choose_port)"
fi

mkdir -p "$OUT_ROOT"
TOKEN_FILE="$OUT_ROOT/api-token"
CP_DATA_DIR="$OUT_ROOT/control-plane-data"
SIGNING_KEY="$OUT_ROOT/evidence-signing-key.pem"
PUBLIC_KEY="$OUT_ROOT/evidence-signing-public.pem"
FIXTURE_ROOT="$OUT_ROOT/fixture"
PUBLISH_JSON="$OUT_ROOT/publish.json"
DASHBOARD_JSON="$OUT_ROOT/dashboard.json"
DETAIL_JSON="$OUT_ROOT/detail.json"
LATEST_JSON="$OUT_ROOT/latest.json"
SUMMARY_JSON="$OUT_ROOT/summary.json"

python3 - "$TOKEN_FILE" <<'PY'
import secrets
import stat
import sys
from pathlib import Path

path = Path(sys.argv[1])
path.write_text(secrets.token_hex(32) + "\n", encoding="utf-8")
path.chmod(stat.S_IRUSR | stat.S_IWUSR)
PY

export AO2_CP_API_TOKEN
cp_token="$(cat "$TOKEN_FILE")"
AO2_CP_API_TOKEN="$cp_token"
BASE_URL="http://127.0.0.1:$PORT"

echo "smoke_root=$OUT_ROOT"
echo "control_plane_url=$BASE_URL"

echo "=== build ao2 ==="
cargo build --release -p ao2-cli

echo "=== build ao2-control-plane ==="
(cd "$CP_ROOT" && cargo build --release -p ao2-cp-server)

echo "=== start ephemeral control plane ==="
mkdir -p "$CP_DATA_DIR"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  AO2_CP_API_TOKEN="$cp_token" \
  AO2_CP_BIND="127.0.0.1:$PORT" \
  AO2_CP_DATA_DIR="$CP_DATA_DIR" \
  "$CP_ROOT/target/release/ao2-cp-server" \
  > "$OUT_ROOT/ao2-cp-server.log" \
  2> "$OUT_ROOT/ao2-cp-server.err" &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true' EXIT

for _ in $(seq 1 50); do
  if curl -fsS "$BASE_URL/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done
curl -fsS "$BASE_URL/healthz" > "$OUT_ROOT/healthz.json"

echo "=== ao2 run governed fixture ==="
mkdir -p "$FIXTURE_ROOT"
cp -R "$ROOT/fixtures/discount-service" "$FIXTURE_ROOT/discount-service"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$ROOT/target/release/ao2" \
  run "$ROOT/examples/risky-pr-run/risky-pr.yaml" \
  --target "$FIXTURE_ROOT/discount-service" \
  --run-id "$RUN_ID" \
  > "$OUT_ROOT/ao2-run.txt"

EVIDENCE_PACK="$FIXTURE_ROOT/discount-service/.ao2/runs/$RUN_ID/evidence-pack/evidence-pack.json"
require_file "$EVIDENCE_PACK"

echo "=== ao2 evidence publish ==="
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$SIGNING_KEY" >/dev/null 2>"$OUT_ROOT/openssl-genpkey.err"
openssl rsa -in "$SIGNING_KEY" -pubout -out "$PUBLIC_KEY" >/dev/null 2>"$OUT_ROOT/openssl-pubout.err"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  AO2_CP_API_TOKEN="$cp_token" \
  "$ROOT/target/release/ao2" \
  evidence publish \
  --evidence-pack "$EVIDENCE_PACK" \
  --signing-key "$SIGNING_KEY" \
  --signer-id "ao2-public-ci-smoke" \
  --control-plane-url "$BASE_URL" \
  --api-token-env AO2_CP_API_TOKEN \
  --json > "$PUBLISH_JSON"

SHA="$(node -e 'const fs=require("fs"); const j=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); process.stdout.write(j.receipt.sha256)' "$PUBLISH_JSON")"

echo "=== read back observer endpoints ==="
auth_header="Authorization: Bearer ${cp_token}"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/evidence-pack/dashboard.json" > "$DASHBOARD_JSON"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/evidence-pack/$SHA/detail.json" > "$DETAIL_JSON"
curl -fsS -H "$auth_header" "$BASE_URL/api/v1/evidence-pack/run/$RUN_ID/latest" > "$LATEST_JSON"

python3 - "$SUMMARY_JSON" "$PUBLISH_JSON" "$DASHBOARD_JSON" "$DETAIL_JSON" "$LATEST_JSON" "$RUN_ID" "$BASE_URL" "$SHA" <<'PY'
import json
import sys
from pathlib import Path

summary, publish_path, dashboard_path, detail_path, latest_path, run_id, base_url, sha = sys.argv[1:]
publish = json.loads(Path(publish_path).read_text(encoding="utf-8"))
dashboard = json.loads(Path(dashboard_path).read_text(encoding="utf-8"))
detail = json.loads(Path(detail_path).read_text(encoding="utf-8"))
latest = json.loads(Path(latest_path).read_text(encoding="utf-8"))

detail_pack = detail.get("evidence_pack", detail)
latest_pack = latest.get("evidence_pack", latest)
if publish["receipt"]["sha256"] != sha:
    raise SystemExit("publish sha mismatch")
if detail_pack.get("run_id") != run_id:
    raise SystemExit("detail run_id mismatch")
if latest_pack.get("run_id") != run_id:
    raise SystemExit("latest run_id mismatch")
if not dashboard.get("summary", {}).get("read_only_observer", False):
    raise SystemExit("dashboard did not declare read-only observer")

payload = {
    "schema_version": "ao2.evidence-pack-control-plane-smoke.v1",
    "status": "passed",
    "run_id": run_id,
    "control_plane_url": base_url,
    "published_sha256": sha,
    "dashboard_schema_version": dashboard.get("schema_version"),
    "detail_schema_version": detail.get("schema_version"),
    "latest_schema_version": latest.get("schema_version"),
    "verdict": detail_pack.get("verdict"),
    "read_only_observer": True,
    "token_leak_detected": False,
}
Path(summary).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

python3 - "$cp_token" "$SUMMARY_JSON" "$PUBLISH_JSON" "$DASHBOARD_JSON" "$DETAIL_JSON" "$LATEST_JSON" <<'PY'
import sys
from pathlib import Path

token = sys.argv[1]
for raw in sys.argv[2:]:
    text = Path(raw).read_text(encoding="utf-8", errors="replace")
    if token and token in text:
        print(f"token_leak_detected={raw}", file=sys.stderr)
        raise SystemExit(1)
PY

echo "summary=$SUMMARY_JSON"
echo "status=passed"
