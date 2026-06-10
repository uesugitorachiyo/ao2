#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_TRAIN_CP_BRIDGE_ROOT:-$ROOT/target/release-train-control-plane-bridge}"
LATEST_ROOT="$OUT_ROOT/latest"
SUMMARY_SOURCE="${AO2_RELEASE_TRAIN_SUMMARY:-}"
CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
SERVER_BIN="${AO2_CP_SERVER_BIN:-}"
SMOKE_BIND="${AO2_CP_RELEASE_TRAIN_BRIDGE_SMOKE_BIND:-127.0.0.1:19880}"
SKIP_SMOKE=0

usage() {
  cat >&2 <<'EOF'
usage: release-train-control-plane-bridge.sh [options]

Options:
  --summary <path>             Existing AO2 public release-train summary.
  --control-plane-root <path>  ao2-control-plane checkout or staging root.
  --server-bin <path>          Optional ao2-cp-server binary path.
  --out-root <path>            Evidence output root.
  --bind <host:port>           Control-plane smoke bind address.
  --skip-smoke                 Materialize bridge evidence without starting ao2-cp-server.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --summary)
      SUMMARY_SOURCE="${2:-}"
      if [ -z "$SUMMARY_SOURCE" ]; then
        echo "--summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --control-plane-root)
      CONTROL_PLANE_ROOT="${2:-}"
      if [ -z "$CONTROL_PLANE_ROOT" ]; then
        echo "--control-plane-root requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --server-bin)
      SERVER_BIN="${2:-}"
      if [ -z "$SERVER_BIN" ]; then
        echo "--server-bin requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --out-root)
      OUT_ROOT="${2:-}"
      if [ -z "$OUT_ROOT" ]; then
        echo "--out-root requires a path" >&2
        exit 2
      fi
      LATEST_ROOT="$OUT_ROOT/latest"
      shift 2
      ;;
    --bind)
      SMOKE_BIND="${2:-}"
      if [ -z "$SMOKE_BIND" ]; then
        echo "--bind requires host:port" >&2
        exit 2
      fi
      shift 2
      ;;
    --skip-smoke)
      SKIP_SMOKE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

DRILL_ROOT="$LATEST_ROOT/public-release-train-drill"
STABLE_SUMMARY="$LATEST_ROOT/release-train-summary.json"
BRIDGE_SUMMARY="$LATEST_ROOT/summary.json"
ENV_FILE="$LATEST_ROOT/control-plane.env"
SMOKE_ROOT="$LATEST_ROOT/control-plane-smoke"
CP_MIRROR_ROOT="$CONTROL_PLANE_ROOT/target/release-train-control-plane-bridge"
CP_MIRROR_SUMMARY="$CP_MIRROR_ROOT/release-train-summary.json"
CP_SMOKE_SCRIPT="$CONTROL_PLANE_ROOT/scripts/smoke-release-train-bridge.py"

rm -rf "$LATEST_ROOT"
mkdir -p "$LATEST_ROOT" "$CP_MIRROR_ROOT"

if [ -z "$SUMMARY_SOURCE" ]; then
  env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
    AO2_PUBLIC_RELEASE_TRAIN_DRILL_ROOT="$DRILL_ROOT" \
    npm run release:train-drill
  SUMMARY_SOURCE="$DRILL_ROOT/summary.json"
fi

python3 - "$SUMMARY_SOURCE" "$STABLE_SUMMARY" "$CP_MIRROR_SUMMARY" "$ENV_FILE" "$BRIDGE_SUMMARY" "$SKIP_SMOKE" <<'PY'
import hashlib
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_source = Path(sys.argv[1]).resolve()
stable_summary = Path(sys.argv[2]).resolve()
mirror_summary = Path(sys.argv[3]).resolve()
env_file = Path(sys.argv[4]).resolve()
bridge_summary = Path(sys.argv[5]).resolve()
skip_smoke = sys.argv[6] == "1"

BRIDGE_SCHEMA = "ao2.release-train-control-plane-bridge.v1"
RELEASE_TRAIN_SCHEMA = "ao2.public-release-train-drill.v1"
SMOKE_SCHEMA = "ao2.cp-release-train-bridge-smoke.v1"
SUMMARY_ENV = "AO2_CP_RELEASE_TRAIN_SUMMARY"

def fail(message: str) -> None:
    raise SystemExit(message)

def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

if not summary_source.is_file():
    fail(f"missing AO2 release-train summary: {summary_source}")

release_train = json.loads(summary_source.read_text(encoding="utf-8"))
if release_train.get("schema_version") != RELEASE_TRAIN_SCHEMA:
    fail(f"unexpected release-train schema: {release_train.get('schema_version')}")
if release_train.get("status") != "passed":
    fail(f"release-train status is not passed: {release_train.get('status')}")
checks = release_train.get("checks")
if not isinstance(checks, list) or not checks:
    fail("release-train checks must be a non-empty list")
consumer = release_train.get("release_readiness_artifact_consumer_contract")
if not isinstance(consumer, dict) or consumer.get("status") != "passed":
    fail("release-train consumer contract is not passed")
guards = release_train.get("publish_guards")
if not isinstance(guards, dict) or guards.get("refuses_publish_side_effects_by_default") is not True:
    fail("release-train publish guard must refuse side effects by default")

stable_summary.parent.mkdir(parents=True, exist_ok=True)
mirror_summary.parent.mkdir(parents=True, exist_ok=True)
shutil.copyfile(summary_source, stable_summary)
shutil.copyfile(summary_source, mirror_summary)
env_file.write_text(f"{SUMMARY_ENV}={stable_summary}\n", encoding="utf-8")

summary = {
    "schema_version": BRIDGE_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "source_summary": str(summary_source),
    "release_train": {
        "schema_version": release_train.get("schema_version"),
        "status": release_train.get("status"),
        "check_count": len(checks),
        "sha256": sha256_file(stable_summary),
    },
    "control_plane": {
        "observer_schema": SMOKE_SCHEMA,
        "configured_env": SUMMARY_ENV,
        "stable_summary": str(stable_summary),
        "mirror_summary": str(mirror_summary),
        "env_file": str(env_file),
        "role": "read-only-observer",
        "json_endpoint": "/api/v1/release/train.json",
        "html_endpoint": "/api/v1/release/train",
        "credential_material_included": False,
        "credential_material_in_urls": False,
        "smoke": "not_run" if skip_smoke else "pending",
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_approves_release": False,
        "mutates_ao2_artifacts": False,
        "mutates_observer_storage": False,
    },
}
bridge_summary.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={bridge_summary}")
print(f"stable_summary={stable_summary}")
print(f"control_plane_env_file={env_file}")
PY

if [ "$SKIP_SMOKE" -eq 0 ]; then
  if [ ! -f "$CP_SMOKE_SCRIPT" ]; then
    echo "missing control-plane release train smoke script: $CP_SMOKE_SCRIPT" >&2
    exit 2
  fi
  if [ -z "$SERVER_BIN" ]; then
    suffix=""
    if [ "${OS:-}" = "Windows_NT" ]; then
      suffix=".exe"
    fi
    SERVER_BIN="$CONTROL_PLANE_ROOT/target/release/ao2-cp-server$suffix"
  fi
  if [ ! -x "$SERVER_BIN" ]; then
    env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
      cargo build --release -p ao2-cp-server --manifest-path "$CONTROL_PLANE_ROOT/Cargo.toml"
  fi
  if [ ! -x "$SERVER_BIN" ]; then
    echo "missing executable ao2-cp-server binary: $SERVER_BIN" >&2
    exit 2
  fi

  env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
    python3 "$CP_SMOKE_SCRIPT" \
      --summary "$STABLE_SUMMARY" \
      --server-bin "$SERVER_BIN" \
      --out-root "$SMOKE_ROOT" \
      --bind "$SMOKE_BIND"

  python3 - "$BRIDGE_SUMMARY" "$SMOKE_ROOT/summary.json" <<'PY'
import json
import sys
from pathlib import Path

bridge_summary_path = Path(sys.argv[1])
smoke_summary_path = Path(sys.argv[2])
bridge = json.loads(bridge_summary_path.read_text(encoding="utf-8"))
smoke = json.loads(smoke_summary_path.read_text(encoding="utf-8"))

if smoke.get("schema_version") != "ao2.cp-release-train-bridge-smoke.v1":
    raise SystemExit("control-plane smoke schema mismatch")
if smoke.get("status") != "passed":
    raise SystemExit("control-plane smoke did not pass")
bridge["control_plane"]["smoke"] = "passed"
bridge["control_plane"]["smoke_summary"] = str(smoke_summary_path)
bridge_summary_path.write_text(json.dumps(bridge, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print("control_plane_smoke=passed")
PY
fi
