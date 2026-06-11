#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_AI_TASK_BOARD_CP_BRIDGE_ROOT:-$ROOT/target/ai-task-board-control-plane-bridge}"
LATEST_ROOT="$OUT_ROOT/latest"
TASK_BOARD_SOURCE="${AO2_AI_TASK_BOARD_CP_BRIDGE_BOARD:-}"
CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
SERVER_BIN="${AO2_CP_SERVER_BIN:-}"
SMOKE_BIND="${AO2_AI_TASK_BOARD_CP_BRIDGE_BIND:-127.0.0.1:19881}"
SKIP_SMOKE=0

usage() {
  cat >&2 <<'USAGE'
usage: scripts/ai-task-board-control-plane-bridge.sh [options]

Options:
  --task-board <path>          Existing ao2.ai-task-board.v1 JSON to bridge.
  --control-plane-root <path>  ao2-control-plane checkout root.
  --server-bin <path>          Prebuilt ao2-cp-server binary.
  --out-root <path>            Evidence output root.
  --bind <host:port>           Temporary local server bind address.
  --skip-smoke                 Validate and summarize only; do not launch server.

This script is local-first evidence plumbing. It uses Authorization: Bearer for
the temporary local smoke only, stores no auth value, and must not publish,
push, deploy, or mutate release metadata.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --task-board)
      TASK_BOARD_SOURCE="${2:?missing value for --task-board}"
      shift 2
      ;;
    --control-plane-root)
      CONTROL_PLANE_ROOT="${2:?missing value for --control-plane-root}"
      shift 2
      ;;
    --server-bin)
      SERVER_BIN="${2:?missing value for --server-bin}"
      shift 2
      ;;
    --out-root)
      OUT_ROOT="${2:?missing value for --out-root}"
      LATEST_ROOT="$OUT_ROOT/latest"
      shift 2
      ;;
    --bind)
      SMOKE_BIND="${2:?missing value for --bind}"
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
      echo "unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

rm -rf "$LATEST_ROOT"
mkdir -p "$LATEST_ROOT"

if [[ -z "$TASK_BOARD_SOURCE" ]]; then
  GENERATED_ROOT="$LATEST_ROOT/generated-task-board"
  env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
    AO2_PULSE_GENERATE_NEXT_REGISTER=0 \
    AO2_PULSE_TASK_BOARD_ROOT="$GENERATED_ROOT" \
    "$ROOT/scripts/pulse-generate-next.sh" >/dev/null
  TASK_BOARD_SOURCE="$GENERATED_ROOT/summary.json"
fi

TASK_BOARD_STABLE="$LATEST_ROOT/task-board.json"
BRIDGE_SUMMARY="$LATEST_ROOT/summary.json"
SMOKE_ROOT="$LATEST_ROOT/control-plane-smoke"

python3 - "$TASK_BOARD_SOURCE" "$TASK_BOARD_STABLE" "$BRIDGE_SUMMARY" "$SKIP_SMOKE" "$SMOKE_BIND" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

source = Path(sys.argv[1]).resolve()
stable = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
skip_smoke = sys.argv[4] == "1"
bind = sys.argv[5]

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def require_false(container, field, blockers):
    if not isinstance(container, dict) or container.get(field) is not False:
        blockers.append(field)

board = json.loads(source.read_text(encoding="utf-8"))
if isinstance(board, dict):
    trust_boundary = board.setdefault("trust_boundary", {})
    if isinstance(trust_boundary, dict):
        trust_boundary.setdefault("local_only", True)
        trust_boundary.setdefault("stores_credentials", False)
        trust_boundary.setdefault("mutates_releases", False)
        trust_boundary.setdefault("control_plane_approves_release", False)
blockers = []
if board.get("schema_version") != "ao2.ai-task-board.v1":
    blockers.append("schema_version")
if not str(board.get("release_objective") or "").strip():
    blockers.append("missing_release_objective")
tasks = board.get("tasks")
if not isinstance(tasks, list) or not tasks:
    blockers.append("missing_tasks")
else:
    for index, task in enumerate(tasks):
        task_id = task.get("task_id") or f"task-{index}"
        if not isinstance(task.get("required_evidence"), list) or not any(str(item).strip() for item in task.get("required_evidence", [])):
            blockers.append(f"task_missing_required_evidence:{task_id}")
        if not isinstance(task.get("stop_conditions"), list) or not any(str(item).strip() for item in task.get("stop_conditions", [])):
            blockers.append(f"task_missing_stop_conditions:{task_id}")

readback = board.get("control_plane_readback") or {}
require_false(readback, "requires_credentials", blockers)
require_false(readback, "can_mutate_ao2_artifacts", blockers)
require_false(readback, "can_mutate_release_metadata", blockers)
trust = board.get("trust_boundary") or {}
require_false(trust, "stores_credentials", blockers)
require_false(trust, "mutates_releases", blockers)

stable.parent.mkdir(parents=True, exist_ok=True)
stable.write_text(json.dumps(board, indent=2, sort_keys=True) + "\n", encoding="utf-8")
sha256 = hashlib.sha256(stable.read_bytes()).hexdigest()

summary = {
    "schema_version": "ao2.ai-task-board-control-plane-bridge.v1",
    "status": "failed" if blockers else ("passed" if skip_smoke else "pending_smoke"),
    "generated_at": utc_now(),
    "task_board": {
        "source": str(source),
        "path": str(stable),
        "schema_version": board.get("schema_version"),
        "sha256": sha256,
        "task_count": len(tasks) if isinstance(tasks, list) else 0,
    },
    "control_plane": {
        "bind": bind,
        "smoke": "skipped" if skip_smoke else "pending",
        "endpoints": {
            "ingest": "/api/v1/ai/task-board",
            "latest": "/api/v1/ai/task-board/latest",
            "dashboard": "/api/v1/ai/task-board/dashboard.json",
        },
        "expected_schemas": [
            "ao2.cp-ingest-receipt.v1",
            "ao2.cp-ai-task-board-readback.v1",
            "ao2.cp-ai-task-board-dashboard.v1",
        ],
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "auth_value_stored": False,
        "credential_material_included": False,
        "credential_material_in_urls": False,
        "mutates_releases": False,
        "control_plane_approves_release": False,
    },
    "blockers": blockers,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
if blockers:
    raise SystemExit("invalid task board for control-plane bridge: " + ",".join(blockers))
print(f"bridge_summary={summary_path}")
PY

if [[ "$SKIP_SMOKE" == "1" ]]; then
  exit 0
fi

if [[ -z "$SERVER_BIN" ]]; then
  env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
    cargo build --release -p ao2-cp-server --manifest-path "$CONTROL_PLANE_ROOT/Cargo.toml"
  if [[ -x "$CONTROL_PLANE_ROOT/target/release/ao2-cp-server" ]]; then
    SERVER_BIN="$CONTROL_PLANE_ROOT/target/release/ao2-cp-server"
  else
    SERVER_BIN="$CONTROL_PLANE_ROOT/target/release/ao2-cp-server.exe"
  fi
fi

python3 - "$SERVER_BIN" "$SMOKE_BIND" "$TASK_BOARD_STABLE" "$SMOKE_ROOT" "$BRIDGE_SUMMARY" <<'PY'
import json
import os
import secrets
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

server_bin = Path(sys.argv[1]).resolve()
bind = sys.argv[2]
task_board = Path(sys.argv[3]).resolve()
smoke_root = Path(sys.argv[4]).resolve()
bridge_summary_path = Path(sys.argv[5]).resolve()
base_url = f"http://{bind}"
data_dir = smoke_root / "data"
token = secrets.token_urlsafe(32)

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def request_json(method, path, body=None):
    headers = {"Authorization": f"Bearer {token}"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(
        base_url + path,
        data=body,
        method=method,
        headers=headers,
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        return json.loads(response.read().decode("utf-8"))

def wait_for_server():
    for _ in range(80):
        try:
            request_json("GET", "/api/v1/status")
            return
        except Exception:
            time.sleep(0.25)
    raise RuntimeError("control-plane server did not become ready")

smoke_root.mkdir(parents=True, exist_ok=True)
data_dir.mkdir(parents=True, exist_ok=True)
env = os.environ.copy()
env.pop("OPENAI_API_KEY", None)
env.pop("ANTHROPIC_API_KEY", None)
env["AO2_CP_API_TOKEN"] = token
env["AO2_CP_LOG_LEVEL"] = "warn"
env["AO2_CP_BIND"] = bind
env["AO2_CP_DATA_DIR"] = str(data_dir)

process = subprocess.Popen(
    [str(server_bin)],
    cwd=str(server_bin.parent),
    env=env,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)
try:
    wait_for_server()
    board_bytes = task_board.read_bytes()
    receipt = request_json("POST", "/api/v1/ai/task-board", board_bytes)
    latest = request_json("GET", "/api/v1/ai/task-board/latest")
    dashboard = request_json("GET", "/api/v1/ai/task-board/dashboard.json")
finally:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)

blockers = []
if receipt.get("schema_version") != "ao2.cp-ingest-receipt.v1":
    blockers.append("receipt_schema")
if latest.get("schema_version") != "ao2.cp-ai-task-board-readback.v1":
    blockers.append("latest_schema")
if dashboard.get("schema_version") != "ao2.cp-ai-task-board-dashboard.v1":
    blockers.append("dashboard_schema")
if latest.get("task_count", 0) <= 0:
    blockers.append("latest_missing_tasks")
summary = dashboard.get("summary") or {}
if summary.get("stores_credentials") is not False:
    blockers.append("dashboard_stores_credentials")
if summary.get("mutates_releases") is not False:
    blockers.append("dashboard_mutates_releases")
if summary.get("control_plane_approves_release") is not False:
    blockers.append("dashboard_control_plane_approves_release")

(smoke_root / "ingest-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(smoke_root / "task-board-readback.json").write_text(json.dumps(latest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(smoke_root / "task-board-dashboard.json").write_text(json.dumps(dashboard, indent=2, sort_keys=True) + "\n", encoding="utf-8")
smoke_summary = {
    "schema_version": "ao2.ai-task-board-control-plane-bridge-smoke.v1",
    "status": "passed" if not blockers else "failed",
    "generated_at": utc_now(),
    "base_url": base_url,
    "auth": {
        "transport": "Authorization: Bearer",
        "auth_value_stored": False,
        "credential_material_included": False,
    },
    "receipt": receipt,
    "latest": latest,
    "dashboard": dashboard,
    "blockers": blockers,
}
(smoke_root / "summary.json").write_text(json.dumps(smoke_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

bridge_summary = json.loads(bridge_summary_path.read_text(encoding="utf-8"))
bridge_summary["status"] = "passed" if not blockers else "failed"
bridge_summary["control_plane"]["smoke"] = "passed" if not blockers else "failed"
bridge_summary["control_plane"]["smoke_summary"] = str(smoke_root / "summary.json")
bridge_summary["blockers"] = blockers
bridge_summary_path.write_text(json.dumps(bridge_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
if blockers:
    raise SystemExit("control-plane bridge smoke failed: " + ",".join(blockers))
print(f"smoke_summary={smoke_root / 'summary.json'}")
PY
