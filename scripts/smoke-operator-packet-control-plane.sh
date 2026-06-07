#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_OPERATOR_PACKET_CP_SMOKE_ROOT:-$ROOT/target/operator-packet-control-plane-smoke/$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_ID="${AO2_OPERATOR_PACKET_CP_RUN_ID:-operator-packet-cp-smoke}"
PORT="${AO2_OPERATOR_PACKET_CP_PORT:-}"
PROFILE="${AO2_OPERATOR_PACKET_CP_PROFILE:-release}"

case "$PROFILE" in
  release)
    TARGET_SUBDIR="release"
    ;;
  debug)
    TARGET_SUBDIR="debug"
    ;;
  *)
    echo "unsupported AO2_OPERATOR_PACKET_CP_PROFILE=$PROFILE; expected release or debug" >&2
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
SIGNING_KEY="$OUT_ROOT/operator-packet-signing-key.pem"
PUBLIC_KEY="$OUT_ROOT/operator-packet-signing-public.pem"
OPERATOR_PACKET="$OUT_ROOT/operator-packet.json"
PUBLISH_JSON="$OUT_ROOT/publish.json"
DASHBOARD_JSON="$OUT_ROOT/dashboard.json"
DETAIL_JSON="$OUT_ROOT/detail.json"
LATEST_JSON="$OUT_ROOT/latest.json"
RAW_JSON="$OUT_ROOT/raw.json"
SIGNATURE_JSON="$OUT_ROOT/signature.json"
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

cp_token="$(cat "$TOKEN_FILE")"
BASE_URL="http://127.0.0.1:$PORT"

echo "smoke_root=$OUT_ROOT"
echo "control_plane_url=$BASE_URL"
echo "profile=$PROFILE"

echo "=== build ao2 ==="
cargo_build_profile -p ao2-cli

echo "=== build ao2-control-plane ==="
(cd "$CP_ROOT" && cargo_build_profile -p ao2-cp-server)

echo "=== start ephemeral control plane ==="
mkdir -p "$CP_DATA_DIR"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  AO2_CP_API_TOKEN="$cp_token" \
  AO2_CP_BIND="127.0.0.1:$PORT" \
  AO2_CP_DATA_DIR="$CP_DATA_DIR" \
  "$CP_ROOT/target/$TARGET_SUBDIR/ao2-cp-server" \
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

echo "=== create signed operator packet fixture ==="
python3 - "$OPERATOR_PACKET" "$RUN_ID" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

path = Path(sys.argv[1])
run_id = sys.argv[2]
payload = {
    "schema_version": "ao2.operator-evidence-packet.v1",
    "run_id": run_id,
    "status": "passed",
    "operator_id": "ao2-public-ci-smoke",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "summary": {
        "recommended_task": "verify signed operator packet control-plane readback",
        "evidence_count": 2,
    },
    "evidence": [
        {
            "kind": "local_fixture",
            "path": str(path),
            "schema_version": "ao2.operator-evidence-packet.v1",
        },
        {
            "kind": "control_plane_readback",
            "dashboard": "/api/v1/operator-packet/dashboard.json",
            "latest": f"/api/v1/operator-packet/run/{run_id}/latest",
        },
    ],
    "trust_boundary": {
        "control_plane_role": "read_only_observer_after_signed_operator_packet",
        "mutates_ao2": False,
        "provider_api_key_auth": False,
    },
}
path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
require_file "$OPERATOR_PACKET"

echo "=== ao2 operator packet publish ==="
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$SIGNING_KEY" >/dev/null 2>"$OUT_ROOT/openssl-genpkey.err"
openssl rsa -in "$SIGNING_KEY" -pubout -out "$PUBLIC_KEY" >/dev/null 2>"$OUT_ROOT/openssl-pubout.err"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  AO2_CP_API_TOKEN="$cp_token" \
  "$ROOT/target/$TARGET_SUBDIR/ao2" \
  evidence publish-operator-packet \
  --operator-packet "$OPERATOR_PACKET" \
  --signing-key "$SIGNING_KEY" \
  --signer-id "ao2-public-ci-smoke" \
  --control-plane-url "$BASE_URL" \
  --api-token-env AO2_CP_API_TOKEN \
  --json > "$PUBLISH_JSON"

SHA="$(python3 - "$PUBLISH_JSON" <<'PY'
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

python3 - "$SUMMARY_JSON" "$PUBLISH_JSON" "$DASHBOARD_JSON" "$DETAIL_JSON" "$LATEST_JSON" "$RAW_JSON" "$SIGNATURE_JSON" "$RUN_ID" "$BASE_URL" "$SHA" <<'PY'
import json
import sys
from pathlib import Path

summary, publish_path, dashboard_path, detail_path, latest_path, raw_path, signature_path, run_id, base_url, sha = sys.argv[1:]
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
if detail.get("run_id") != run_id or latest.get("run_id") != run_id or raw.get("run_id") != run_id:
    raise SystemExit("run_id readback mismatch")
if signature.get("operator_packet_sha256") != sha:
    raise SystemExit("signature sidecar sha mismatch")
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

payload = {
    "schema_version": "ao2.operator-packet-control-plane-smoke.v1",
    "status": "passed",
    "run_id": run_id,
    "control_plane_url": base_url,
    "published_sha256": sha,
    "contract_schemas": observed,
    "read_only_observer": True,
    "can_approve_runs": False,
    "can_mutate_ao2_evidence": False,
    "token_leak_detected": False,
}
Path(summary).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

python3 - "$cp_token" "$SUMMARY_JSON" "$PUBLISH_JSON" "$DASHBOARD_JSON" "$DETAIL_JSON" "$LATEST_JSON" "$RAW_JSON" "$SIGNATURE_JSON" <<'PY'
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
