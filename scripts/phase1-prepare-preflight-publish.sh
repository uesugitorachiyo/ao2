#!/bin/sh
set -eu

# One reproducible Phase 1 promotion path:
#   1. materialize local AO2 prerequisite evidence
#   2. source the generated env file internally
#   3. run the promotion preflight
#   4. publish and read back via ao2-control-plane
#
# Operators must provide the token through AO2_PHASE1_API_TOKEN_ENV. This script
# never reads token files, prints token values, or places bearer tokens in URLs.

timestamp=$(date -u +%Y%m%dT%H%M%SZ)
AO2_PHASE1_ONE_COMMAND_ROOT="${AO2_PHASE1_ONE_COMMAND_ROOT:-target/phase1-prepare-preflight-publish/$timestamp}"
AO2_PHASE1_PREPARE_ROOT="${AO2_PHASE1_PREPARE_ROOT:-$AO2_PHASE1_ONE_COMMAND_ROOT/prerequisites}"
AO2_PHASE1_PREPARE_JSON="${AO2_PHASE1_PREPARE_JSON:-$AO2_PHASE1_ONE_COMMAND_ROOT/prepare-prerequisites.json}"
AO2_PHASE1_PROMOTION_ROOT="${AO2_PHASE1_PROMOTION_ROOT:-$AO2_PHASE1_ONE_COMMAND_ROOT/promotion}"
AO2_PHASE1_CONTROL_PLANE_URL="${AO2_PHASE1_CONTROL_PLANE_URL:-http://127.0.0.1:18745}"
AO2_PHASE1_API_TOKEN_ENV="${AO2_PHASE1_API_TOKEN_ENV:-AO2_CP_API_TOKEN}"
AO2_PHASE1_SIGNING_KEY="${AO2_PHASE1_SIGNING_KEY:-.release-signing/ao2-release-signing-key.pem}"
AO2_PHASE1_DASHBOARD_SNAPSHOT="${AO2_PHASE1_DASHBOARD_SNAPSHOT:-0}"
AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT="${AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT:-$AO2_PHASE1_ONE_COMMAND_ROOT/control-plane-dashboard-snapshot}"

mkdir -p "$AO2_PHASE1_ONE_COMMAND_ROOT"

if [ -z "$AO2_PHASE1_API_TOKEN_ENV" ]; then
  echo "missing AO2_PHASE1_API_TOKEN_ENV; publish uses env-token auth to avoid leaking bearer tokens" >&2
  exit 1
fi

python3 - "$AO2_PHASE1_API_TOKEN_ENV" <<'PY'
import os
import sys

name = sys.argv[1]
if not os.environ.get(name):
    print(f"missing {name}; export it before publishing so the token stays out of command args and URLs", file=sys.stderr)
    raise SystemExit(1)
PY

python3 scripts/prepare_phase1_promotion_prerequisites.py \
  --out-root "$AO2_PHASE1_PREPARE_ROOT" \
  --json "$@" > "$AO2_PHASE1_PREPARE_JSON"

env_file=$(
  python3 - "$AO2_PHASE1_PREPARE_JSON" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    report = json.load(f)
if report.get("status") != "passed":
    print("Phase 1 prerequisite preparation failed", file=sys.stderr)
    raise SystemExit(1)
print(report["env_file"])
PY
)

. "$env_file"

export AO2_PHASE1_PROMOTION_ROOT
export AO2_PHASE1_CONTROL_PLANE_URL
export AO2_PHASE1_API_TOKEN_ENV
export AO2_PHASE1_SIGNING_KEY

AO2_PHASE1_PROMOTION_PREFLIGHT=1 \
AO2_PHASE1_PROMOTION_PUBLISH=1 \
  scripts/phase1-replacement-promotion.sh > "$AO2_PHASE1_ONE_COMMAND_ROOT/preflight.stdout"

AO2_PHASE1_PROMOTION_PUBLISH=1 \
  scripts/phase1-replacement-promotion.sh > "$AO2_PHASE1_ONE_COMMAND_ROOT/publish.stdout"

if [ "$AO2_PHASE1_DASHBOARD_SNAPSHOT" = "1" ]; then
  export AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT
  scripts/phase1-control-plane-dashboard-snapshot.sh > "$AO2_PHASE1_ONE_COMMAND_ROOT/dashboard-snapshot.stdout"
fi

printf "phase1_prepare_preflight_publish_root=%s\n" "$AO2_PHASE1_ONE_COMMAND_ROOT"
printf "phase1_prerequisites_manifest=%s\n" "$AO2_PHASE1_PREPARE_ROOT/phase1-promotion-prerequisites.json"
printf "phase1_prerequisites_env=%s\n" "$env_file"
printf "phase1_preflight_stdout=%s\n" "$AO2_PHASE1_ONE_COMMAND_ROOT/preflight.stdout"
printf "phase1_publish_stdout=%s\n" "$AO2_PHASE1_ONE_COMMAND_ROOT/publish.stdout"
if [ "$AO2_PHASE1_DASHBOARD_SNAPSHOT" = "1" ]; then
  printf "phase1_dashboard_snapshot_stdout=%s\n" "$AO2_PHASE1_ONE_COMMAND_ROOT/dashboard-snapshot.stdout"
  printf "phase1_dashboard_snapshot_index=%s\n" "$AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT/index.html"
  printf "phase1_dashboard_snapshot_manifest=%s\n" "$AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT/manifest.json"
fi
cat "$AO2_PHASE1_ONE_COMMAND_ROOT/publish.stdout"
