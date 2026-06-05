#!/bin/sh
set -eu

# Fetch local token-safe ao2-control-plane dashboard snapshots after Phase 1
# promotion. The bearer value stays in the environment variable named by
# AO2_PHASE1_API_TOKEN_ENV and is passed to the control-plane helper only by
# env-var name, never by value.

timestamp=$(date -u +%Y%m%dT%H%M%SZ)
AO2_CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-../ao2-control-plane}"
AO2_CP_DASHBOARD_SNAPSHOT_HELPER="${AO2_CP_DASHBOARD_SNAPSHOT_HELPER:-../ao2-control-plane/scripts/cp_dashboard_snapshot.py}"
AO2_PHASE1_CONTROL_PLANE_URL="${AO2_PHASE1_CONTROL_PLANE_URL:-http://127.0.0.1:18745}"
AO2_PHASE1_API_TOKEN_ENV="${AO2_PHASE1_API_TOKEN_ENV:-AO2_CP_API_TOKEN}"
AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT="${AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT:-target/phase1-control-plane-dashboard-snapshot/$timestamp}"
AO2_PHASE1_DASHBOARD_SNAPSHOT_OPEN="${AO2_PHASE1_DASHBOARD_SNAPSHOT_OPEN:-0}"

helper="$AO2_CP_DASHBOARD_SNAPSHOT_HELPER"
if [ ! -f "$helper" ]; then
  helper="$AO2_CONTROL_PLANE_ROOT/scripts/cp_dashboard_snapshot.py"
fi
if [ ! -f "$helper" ]; then
  echo "missing ao2-control-plane dashboard snapshot helper: $helper" >&2
  exit 1
fi

if [ -z "$AO2_PHASE1_API_TOKEN_ENV" ]; then
  echo "missing AO2_PHASE1_API_TOKEN_ENV; dashboard snapshots use env-token auth" >&2
  exit 1
fi

python3 - "$AO2_PHASE1_API_TOKEN_ENV" <<'PY'
import os
import sys

name = sys.argv[1]
if not os.environ.get(name):
    print(f"missing {name}; export it before fetching dashboard snapshots", file=sys.stderr)
    raise SystemExit(1)
PY

set -- \
  "$helper" \
  --base-url "$AO2_PHASE1_CONTROL_PLANE_URL" \
  --api-token-env "$AO2_PHASE1_API_TOKEN_ENV" \
  --out-dir "$AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT"

if [ "$AO2_PHASE1_DASHBOARD_SNAPSHOT_OPEN" = "1" ]; then
  set -- "$@" --open
fi

python3 "$@"
