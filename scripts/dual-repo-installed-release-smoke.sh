#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_DUAL_REPO_INSTALLED_SMOKE_ROOT:-$ROOT/target/dual-repo-installed-release-smoke}"
LATEST_ROOT="$OUT_ROOT/latest"
CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
SMOKE_BIND="${AO2_DUAL_REPO_INSTALLED_SMOKE_BIND:-127.0.0.1:19883}"
TARGET_LABEL="${AO2_DUAL_REPO_INSTALLED_SMOKE_TARGET:-}"

usage() {
  cat >&2 <<'USAGE'
usage: scripts/dual-repo-installed-release-smoke.sh [options]

Options:
  --control-plane-root <path>  ao2-control-plane checkout root.
  --out-root <path>            Evidence output root.
  --bind <host:port>           Temporary local server bind address.
  --target-label <label>       Archive target label for both packages.

This smoke packages AO2 and ao2-control-plane into local release archives, uses
the installed binaries from those archives, then performs an authenticated
Authorization: Bearer task-board ingest/readback. It stores no auth value and
does not publish, push, deploy, or mutate release metadata.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --control-plane-root)
      CONTROL_PLANE_ROOT="${2:?missing value for --control-plane-root}"
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
    --target-label)
      TARGET_LABEL="${2:?missing value for --target-label}"
      shift 2
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

if [[ -z "$TARGET_LABEL" ]]; then
  os_name="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch_name="$(uname -m)"
  case "$os_name" in
    darwin) os_label="macos" ;;
    linux) os_label="linux" ;;
    msys*|mingw*|cygwin*) os_label="windows" ;;
    *) os_label="$os_name" ;;
  esac
  case "$arch_name" in
    arm64|aarch64) arch_label="aarch64" ;;
    x86_64|amd64) arch_label="x86_64" ;;
    *) arch_label="$arch_name" ;;
  esac
  TARGET_LABEL="$os_label-$arch_label"
fi

rm -rf "$LATEST_ROOT"
mkdir -p "$LATEST_ROOT"

AO2_VERSION="$("$ROOT/scripts/current-version.sh")"
CP_VERSION="$(awk -F '"' '/^VERSION=/ { print $2; exit }' "$CONTROL_PLANE_ROOT/scripts/package-local.sh")"
if [[ -z "$CP_VERSION" ]]; then
  echo "unable to derive ao2-control-plane version from scripts/package-local.sh" >&2
  exit 1
fi

AO2_DIST="$LATEST_ROOT/ao2-dist"
CP_DIST="$LATEST_ROOT/control-plane-dist"
TASK_BOARD_ROOT="$LATEST_ROOT/generated-task-board"
SMOKE_ROOT="$LATEST_ROOT/smoke"
mkdir -p "$AO2_DIST" "$CP_DIST" "$TASK_BOARD_ROOT" "$SMOKE_ROOT"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY cargo build --release -p ao2-cli
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo run -p ao2-cli -- release package \
    --out-dir "$AO2_DIST" \
    --version "$AO2_VERSION" \
    --binary "$ROOT/target/release/ao2" \
    --target-label "$TARGET_LABEL" >/dev/null

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo build --release -p ao2-cp-server --bins --manifest-path "$CONTROL_PLANE_ROOT/Cargo.toml"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$CONTROL_PLANE_ROOT/scripts/package-local.sh" \
    --out-dir "$CP_DIST" \
    --version "$CP_VERSION" \
    --binary "$CONTROL_PLANE_ROOT/target/release/ao2-cp-server" \
    --target-label "$TARGET_LABEL" >/dev/null

AO2_ARCHIVE="$AO2_DIST/ao2-$AO2_VERSION-$TARGET_LABEL.tar.gz"
CP_ARCHIVE="$CP_DIST/ao2-control-plane-$CP_VERSION-$TARGET_LABEL.tar.gz"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  AO2_PULSE_GENERATE_NEXT_REGISTER=0 \
  AO2_PULSE_TASK_BOARD_ROOT="$TASK_BOARD_ROOT" \
  "$ROOT/scripts/pulse-generate-next.sh" >/dev/null

python3 - "$AO2_ARCHIVE" "$CP_ARCHIVE" "$TASK_BOARD_ROOT/summary.json" "$SMOKE_ROOT" "$LATEST_ROOT/summary.json" "$SMOKE_BIND" "$TARGET_LABEL" "$AO2_VERSION" "$CP_VERSION" <<'PY'
import hashlib
import json
import os
import secrets
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ao2_archive = Path(sys.argv[1]).resolve()
cp_archive = Path(sys.argv[2]).resolve()
task_board_source = Path(sys.argv[3]).resolve()
smoke_root = Path(sys.argv[4]).resolve()
summary_path = Path(sys.argv[5]).resolve()
bind = sys.argv[6]
target_label = sys.argv[7]
ao2_version = sys.argv[8]
cp_version = sys.argv[9]
base_url = f"http://{bind}"

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def extract(archive: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:gz") as tar:
        tar.extractall(destination)

def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))

def normalize_task_board(source: Path, destination: Path) -> None:
    board = load_json(source)
    trust_boundary = board.setdefault("trust_boundary", {})
    trust_boundary.setdefault("local_only", True)
    trust_boundary.setdefault("stores_credentials", False)
    trust_boundary.setdefault("mutates_releases", False)
    trust_boundary.setdefault("control_plane_approves_release", False)
    destination.write_text(json.dumps(board, indent=2, sort_keys=True) + "\n", encoding="utf-8")

def request_json(method, path, token, body=None):
    headers = {"Authorization": f"Bearer {token}"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(base_url + path, data=body, method=method, headers=headers)
    with urllib.request.urlopen(request, timeout=5) as response:
        return json.loads(response.read().decode("utf-8"))

def wait_for_server(token):
    for _ in range(80):
        try:
            request_json("GET", "/api/v1/status", token)
            return
        except Exception:
            time.sleep(0.25)
    raise RuntimeError("installed control-plane server did not become ready")

smoke_root.mkdir(parents=True, exist_ok=True)
ao2_extract = smoke_root / "ao2-extract"
cp_extract = smoke_root / "control-plane-extract"
data_dir = smoke_root / "control-plane-data"
for path in [ao2_extract, cp_extract, data_dir]:
    if path.exists():
        shutil.rmtree(path)

extract(ao2_archive, ao2_extract)
extract(cp_archive, cp_extract)

ao2_manifest = load_json(ao2_extract / "RELEASE-MANIFEST.json")
cp_manifest = load_json(cp_extract / "RELEASE-MANIFEST.json")
ao2_binary = ao2_extract / ao2_manifest["binary_path"]
cp_binary = cp_extract / cp_manifest["binary_path"]
ao2_version_json = smoke_root / "ao2-version.json"
ao2_version_result = subprocess.run(
    [str(ao2_binary), "version", "--json"],
    text=True,
    capture_output=True,
    check=True,
)
ao2_version_json.write_text(ao2_version_result.stdout, encoding="utf-8")

task_board = smoke_root / "task-board.json"
normalize_task_board(task_board_source, task_board)
if load_json(task_board).get("schema_version") != "ao2.ai-task-board.v1":
    raise SystemExit("generated task board is not ao2.ai-task-board.v1")
token = secrets.token_urlsafe(32)
env = os.environ.copy()
env.pop("OPENAI_API_KEY", None)
env.pop("ANTHROPIC_API_KEY", None)
env["AO2_CP_API_TOKEN"] = token
env["AO2_CP_LOG_LEVEL"] = "warn"
env["AO2_CP_BIND"] = bind
env["AO2_CP_DATA_DIR"] = str(data_dir)

process = subprocess.Popen(
    [str(cp_binary)],
    cwd=str(cp_binary.parent),
    env=env,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)
try:
    wait_for_server(token)
    receipt = request_json("POST", "/api/v1/ai/task-board", token, task_board.read_bytes())
    latest = request_json("GET", "/api/v1/ai/task-board/latest", token)
    dashboard = request_json("GET", "/api/v1/ai/task-board/dashboard.json", token)
finally:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)

(smoke_root / "ingest-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(smoke_root / "task-board-readback.json").write_text(json.dumps(latest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(smoke_root / "task-board-dashboard.json").write_text(json.dumps(dashboard, indent=2, sort_keys=True) + "\n", encoding="utf-8")

blockers = []
if ao2_manifest.get("schema_version") != "ao2.release-manifest.v1":
    blockers.append("ao2_release_manifest_schema")
if cp_manifest.get("schema_version") != "ao2-control-plane.release-manifest.v1":
    blockers.append("control_plane_release_manifest_schema")
ao2_version_identity = load_json(ao2_version_json)
if ao2_version_identity.get("package") != "ao2":
    blockers.append("ao2_version_package")
if ao2_version_identity.get("version") != ao2_version:
    blockers.append("ao2_version_value")
if ao2_version_identity.get("target") != target_label:
    blockers.append("ao2_version_target")
if ao2_version_identity.get("release_manifest_schema") != "ao2.release-manifest.v1":
    blockers.append("ao2_version_release_manifest_schema")
if receipt.get("schema_version") != "ao2.cp-ingest-receipt.v1":
    blockers.append("receipt_schema")
if latest.get("schema_version") != "ao2.cp-ai-task-board-readback.v1":
    blockers.append("task_board_readback_schema")
if dashboard.get("schema_version") != "ao2.cp-ai-task-board-dashboard.v1":
    blockers.append("task_board_dashboard_schema")
dashboard_summary = dashboard.get("summary") or {}
if dashboard_summary.get("stores_credentials") is not False:
    blockers.append("dashboard_stores_credentials")
if dashboard_summary.get("mutates_releases") is not False:
    blockers.append("dashboard_mutates_releases")
if dashboard_summary.get("control_plane_approves_release") is not False:
    blockers.append("dashboard_control_plane_approves_release")

summary = {
    "schema_version": "ao2.dual-repo-installed-release-smoke.v1",
    "status": "passed" if not blockers else "failed",
    "generated_at": utc_now(),
    "target_label": target_label,
    "archives": {
        "ao2": {
            "version": ao2_version,
            "path": str(ao2_archive),
            "sha256": sha256(ao2_archive),
            "manifest_schema": ao2_manifest.get("schema_version"),
            "binary_path": ao2_manifest.get("binary_path"),
        },
        "ao2_control_plane": {
            "version": cp_version,
            "path": str(cp_archive),
            "sha256": sha256(cp_archive),
            "manifest_schema": cp_manifest.get("schema_version"),
            "binary_path": cp_manifest.get("binary_path"),
        },
    },
    "installed_binaries": {
        "ao2": str(ao2_binary),
        "ao2_control_plane": str(cp_binary),
    },
    "evidence": {
        "ao2_version": str(ao2_version_json),
        "task_board": str(task_board),
        "ingest_receipt": str(smoke_root / "ingest-receipt.json"),
        "task_board_readback": str(smoke_root / "task-board-readback.json"),
        "task_board_dashboard": str(smoke_root / "task-board-dashboard.json"),
    },
    "control_plane": {
        "base_url": base_url,
        "endpoints": {
            "ingest": "/api/v1/ai/task-board",
            "latest": "/api/v1/ai/task-board/latest",
            "dashboard": "/api/v1/ai/task-board/dashboard.json",
        },
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
    raise SystemExit("dual-repo installed release smoke failed: " + ",".join(blockers))
print(f"summary={summary_path}")
PY
