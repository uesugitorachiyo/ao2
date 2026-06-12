#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_RELEASE_SMOKE_ROOT:-$ROOT/target/dual-public-release-smoke}"
LATEST_ROOT="$OUT_ROOT/latest"
SMOKE_BIND="${AO2_PUBLIC_RELEASE_SMOKE_BIND:-127.0.0.1:19884}"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v0.4.80}"
AO2_CP_RELEASE_REPO="${AO2_CP_RELEASE_REPO:-uesugitorachiyo/ao2-control-plane}"
AO2_CP_RELEASE_TAG="${AO2_CP_RELEASE_TAG:-v0.1.13}"
TARGET_LABEL="${AO2_PUBLIC_RELEASE_SMOKE_TARGET:-}"

usage() {
  cat >&2 <<'USAGE'
usage: scripts/dual-public-release-smoke.sh [options]

Options:
  --out-root <path>      Evidence output root.
  --bind <host:port>     Temporary local server bind address.
  --target-label <label> Public archive target label. Defaults to host OS/arch.

This smoke downloads published AO2 and ao2-control-plane release archives,
verifies them against their published SHA256SUMS manifests, runs the installed
binaries from those archives, then performs an authenticated Authorization:
Bearer task-board ingest/readback. It stores no auth value and does not publish,
push, deploy, or mutate release metadata.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
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

if ! command -v gh >/dev/null 2>&1; then
  echo "missing gh CLI" >&2
  exit 1
fi

AO2_VERSION="${AO2_RELEASE_TAG#v}"
AO2_CP_VERSION="${AO2_CP_RELEASE_TAG#v}"
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
# Current default public smoke archive contract:
# ao2-0.4.80-linux-x86_64.tar.gz
# ao2-control-plane-0.1.13-linux-x86_64.tar.gz
AO2_ARCHIVE_NAME="ao2-$AO2_VERSION-$TARGET_LABEL.tar.gz"
AO2_CP_ARCHIVE_NAME="ao2-control-plane-$AO2_CP_VERSION-$TARGET_LABEL.tar.gz"

rm -rf "$LATEST_ROOT"
mkdir -p "$LATEST_ROOT"

AO2_DOWNLOAD="$LATEST_ROOT/public-downloads/ao2"
CP_DOWNLOAD="$LATEST_ROOT/public-downloads/control-plane"
SMOKE_ROOT="$LATEST_ROOT/smoke"
mkdir -p "$AO2_DOWNLOAD" "$CP_DOWNLOAD" "$SMOKE_ROOT"

gh release download "$AO2_RELEASE_TAG" \
  --repo "$AO2_RELEASE_REPO" \
  --pattern "$AO2_ARCHIVE_NAME" \
  --pattern SHA256SUMS \
  --dir "$AO2_DOWNLOAD" \
  --clobber

gh release download "$AO2_CP_RELEASE_TAG" \
  --repo "$AO2_CP_RELEASE_REPO" \
  --pattern "$AO2_CP_ARCHIVE_NAME" \
  --pattern SHA256SUMS \
  --pattern summary.json \
  --dir "$CP_DOWNLOAD" \
  --clobber

verify_checksum() {
  local dir="$1"
  local asset="$2"
  (cd "$dir" && grep "  $asset$" SHA256SUMS > SHA256SUMS.asset)
  if command -v shasum >/dev/null 2>&1; then
    (cd "$dir" && shasum -a 256 -c SHA256SUMS.asset)
  elif command -v sha256sum >/dev/null 2>&1; then
    (cd "$dir" && sha256sum -c SHA256SUMS.asset)
  else
    echo "missing checksum verifier: shasum or sha256sum required" >&2
    exit 1
  fi
}

verify_checksum "$AO2_DOWNLOAD" "$AO2_ARCHIVE_NAME"
verify_checksum "$CP_DOWNLOAD" "$AO2_CP_ARCHIVE_NAME"
verify_checksum "$CP_DOWNLOAD" "summary.json"

python3 - "$AO2_DOWNLOAD/$AO2_ARCHIVE_NAME" "$CP_DOWNLOAD/$AO2_CP_ARCHIVE_NAME" "$CP_DOWNLOAD/summary.json" "$SMOKE_ROOT" "$LATEST_ROOT/summary.json" "$SMOKE_BIND" "$TARGET_LABEL" "$AO2_RELEASE_REPO" "$AO2_RELEASE_TAG" "$AO2_CP_RELEASE_REPO" "$AO2_CP_RELEASE_TAG" <<'PY'
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
cp_release_summary = Path(sys.argv[3]).resolve()
smoke_root = Path(sys.argv[4]).resolve()
summary_path = Path(sys.argv[5]).resolve()
bind = sys.argv[6]
target_label = sys.argv[7]
ao2_repo = sys.argv[8]
ao2_tag = sys.argv[9]
cp_repo = sys.argv[10]
cp_tag = sys.argv[11]
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
    destination_root = destination.resolve()
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            if member.issym() or member.islnk():
                raise RuntimeError(f"refusing archive link entry: {member.name}")
            member_path = (destination / member.name).resolve()
            if member_path != destination_root and destination_root not in member_path.parents:
                raise RuntimeError(f"refusing archive path traversal entry: {member.name}")
        tar.extractall(destination)

def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))

def write_task_board(path: Path) -> None:
    board = {
        "schema_version": "ao2.ai-task-board.v1",
        "status": "ready",
        "release_objective": "Verify the published AO2 and control-plane release archives interoperate.",
        "source_recommendation": "Production readiness public dual-release smoke.",
        "release_train": {
            "version": "v0.4.80/v0.1.13",
            "theme": "Public release pair interoperability",
        },
        "tasks": [
            {
                "task_id": "ao2-public-dual-release-smoke",
                "title": "Public dual release smoke",
                "kind": "release-smoke",
                "status": "proposed",
                "objective": "Prove published AO2 can hand a task-board artifact to the published control plane.",
                "confidence": "high",
                "rationale": "Public release readiness needs evidence from downloaded archives, not only locally packaged builds.",
                "required_evidence": [
                    "ao2.dual-public-release-smoke.v1",
                    "ao2.ai-task-board.v1",
                    "ao2.cp-ai-task-board-readback.v1",
                ],
                "stop_conditions": [
                    "Stop if either published archive is missing or checksum-invalid.",
                    "Stop if control-plane readback requires credentials or release mutation authority.",
                ],
                "release_train": "v0.4.80/v0.1.13",
            }
        ],
        "control_plane_readback": {
            "role": "read_only_observer",
            "requires_credentials": False,
            "can_mutate_ao2_artifacts": False,
            "can_mutate_release_metadata": False,
        },
        "trust_boundary": {
            "local_only": True,
            "stores_credentials": False,
            "mutates_releases": False,
        },
    }
    path.write_text(json.dumps(board, indent=2, sort_keys=True) + "\n", encoding="utf-8")

def request_json(method, path, token, body=None):
    # Authorization: Bearer is required, but the bearer value is never stored.
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
    raise RuntimeError("published control-plane server did not become ready")

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
write_task_board(task_board)
token = secrets.token_urlsafe(32)
env = {
    "PATH": os.environ.get("PATH", ""),
    "AO2_CP_API_TOKEN": token,
    "AO2_CP_LOG_LEVEL": "warn",
    "AO2_CP_BIND": bind,
    "AO2_CP_DATA_DIR": str(data_dir),
}

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
if ao2_version_identity.get("target") != target_label:
    blockers.append("ao2_version_target")
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
    "schema_version": "ao2.dual-public-release-smoke.v1",
    "status": "passed" if not blockers else "failed",
    "generated_at": utc_now(),
    "target_label": target_label,
    "release_pair": {
        "ao2": {"repo": ao2_repo, "tag": ao2_tag},
        "ao2_control_plane": {"repo": cp_repo, "tag": cp_tag},
    },
    "archives": {
        "ao2": {
            "path": str(ao2_archive),
            "sha256": sha256(ao2_archive),
            "manifest_schema": ao2_manifest.get("schema_version"),
            "binary_path": ao2_manifest.get("binary_path"),
        },
        "ao2_control_plane": {
            "path": str(cp_archive),
            "sha256": sha256(cp_archive),
            "manifest_schema": cp_manifest.get("schema_version"),
            "binary_path": cp_manifest.get("binary_path"),
            "release_summary_path": str(cp_release_summary),
            "release_summary_sha256": sha256(cp_release_summary),
        },
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
        "downloads_public_release_archives": True,
        "stores_credentials": False,
        "auth_value_stored": False,
        "credential_material_included": False,
        "credential_material_in_urls": False,
        "mutates_releases": False,
        "mutates_github_releases": False,
        "control_plane_approves_release": False,
    },
    "blockers": blockers,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
if blockers:
    raise SystemExit("dual public release smoke failed: " + ",".join(blockers))
print(f"dual_public_release_smoke=passed")
print(f"summary={summary_path}")
PY
