#!/bin/sh
# publish-provider-readiness.sh
#
# Publishes an AO2-produced provider-readiness artifact to a control-plane
# instance. Reads the long-lived control-plane bind + token from the standard
# location (target/long-lived-control-plane/api-token + server.pid). Falls
# back to environment overrides AO2_PHASE1_CP_BASE_URL / AO2_CP_API_TOKEN.
#
# Companion to scripts/build-provider-readiness.sh — but accepts an artifact
# path so it can be reused with any locally-produced readiness JSON.
#
# Usage:
#   bash scripts/publish-provider-readiness.sh <artifact_path>
#
# Output: publish-receipt.json next to the artifact + dashboard.json snapshot.
set -eu

if [ $# -lt 1 ]; then
  # If no arg, find the latest provider-readiness build under target/.
  latest=$(ls -td target/provider-readiness/* 2>/dev/null | head -1 || true)
  if [ -z "$latest" ] || [ ! -f "$latest/provider-readiness.json" ]; then
    echo "usage: $0 <artifact_path>" >&2
    echo "  no target/provider-readiness/<ts>/provider-readiness.json found either" >&2
    exit 2
  fi
  ARTIFACT="$latest/provider-readiness.json"
else
  ARTIFACT="$1"
fi

if [ ! -f "$ARTIFACT" ]; then
  echo "artifact not found: $ARTIFACT" >&2
  exit 3
fi

ART_DIR=$(dirname "$ARTIFACT")
LL_DIR="${AO2_LONG_LIVED_CP_DIR:-target/long-lived-control-plane}"

if [ -z "${AO2_PHASE1_CP_BASE_URL:-}" ]; then
  if [ -f "$LL_DIR/bind" ]; then
    BIND=$(cat "$LL_DIR/bind")
    AO2_PHASE1_CP_BASE_URL="http://$BIND"
  else
    AO2_PHASE1_CP_BASE_URL="http://127.0.0.1:18745"
  fi
fi

if [ -z "${AO2_CP_API_TOKEN:-}" ]; then
  if [ -f "$LL_DIR/api-token" ]; then
    AO2_CP_API_TOKEN=$(cat "$LL_DIR/api-token")
  else
    echo "AO2_CP_API_TOKEN not set and $LL_DIR/api-token missing" >&2
    exit 4
  fi
fi

export AO2_PHASE1_CP_BASE_URL AO2_CP_API_TOKEN ARTIFACT ART_DIR

python3 - <<'PY'
import json
import os
import hashlib
import urllib.request

base = os.environ["AO2_PHASE1_CP_BASE_URL"].rstrip("/")
token = os.environ["AO2_CP_API_TOKEN"]
artifact_path = os.environ["ARTIFACT"]
out_dir = os.environ["ART_DIR"]

with open(artifact_path, "rb") as fh:
    body = fh.read()

# POST to provider-readiness ingest endpoint.
req = urllib.request.Request(
    f"{base}/api/v1/provider/readiness",
    data=body,
    headers={
        "authorization": f"Bearer {token}",
        "content-type": "application/json",
    },
    method="POST",
)
try:
    resp = urllib.request.urlopen(req, timeout=30)
    code = resp.status
    payload = resp.read().decode()
except urllib.error.HTTPError as exc:
    code = exc.code
    payload = exc.read().decode()

receipt = {
    "publish_status_code": code,
    "publish_response": json.loads(payload) if payload.strip().startswith(("{", "[")) else payload,
    "artifact_sha256": hashlib.sha256(body).hexdigest(),
    "artifact_path": artifact_path,
    "cp_base_url": base,
}

with open(os.path.join(out_dir, "publish-receipt.json"), "w", encoding="utf-8") as fh:
    json.dump(receipt, fh, indent=2, sort_keys=True)

# Snapshot dashboard after publish.
dash_req = urllib.request.Request(
    f"{base}/api/v1/phase1/promotion/dashboard.json",
    headers={"authorization": f"Bearer {token}"},
)
try:
    dash_resp = urllib.request.urlopen(dash_req, timeout=30).read().decode()
    dashboard = json.loads(dash_resp)
except Exception as e:
    dashboard = {"error": str(e)}

with open(os.path.join(out_dir, "dashboard-after-publish.json"), "w", encoding="utf-8") as fh:
    json.dump(dashboard, fh, indent=2, sort_keys=True)

print(f"publish_status_code={code}")
if isinstance(receipt["publish_response"], dict):
    print(f"publish_sha256={receipt['publish_response'].get('sha256', 'n/a')}")
    print(f"publish_schema_version={receipt['publish_response'].get('schema_version', 'n/a')}")
checklist = (dashboard or {}).get("checklist", {})
pr = checklist.get("provider_readiness") or {}
print(f"dashboard_provider_readiness_status={pr.get('status')}")
print(f"dashboard_provider_readiness_phase1_state={pr.get('phase1_state')}")
print(f"dashboard_state={dashboard.get('state')}")
PY
