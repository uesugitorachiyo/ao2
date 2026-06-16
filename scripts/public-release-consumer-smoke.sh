#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_CONSUMER_SMOKE_ROOT:-$ROOT/target/public-release-consumer-smoke}"
LATEST_ROOT="$OUT_ROOT/latest"
eval "$("$ROOT/scripts/release-train-env.sh" "${AO2_RELEASE_TRAIN:-stable}")"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-$AO2_RELEASE_TRAIN_AO2_TAG}"
AO2_CP_RELEASE_REPO="${AO2_CP_RELEASE_REPO:-uesugitorachiyo/ao2-control-plane}"
AO2_CP_RELEASE_TAG="${AO2_CP_RELEASE_TAG:-$AO2_RELEASE_TRAIN_CP_TAG}"
TARGET_LABEL="${AO2_PUBLIC_CONSUMER_SMOKE_TARGET:-}"
FIXTURE_DIR=""

usage() {
  cat >&2 <<'USAGE'
usage: scripts/public-release-consumer-smoke.sh [options]

Options:
  --out-root <path>      Evidence output root.
  --target-label <label> Public archive target label. Defaults to host OS/arch.
  --fixture-dir <path>   Offline fixture with ao2/ and control-plane/ downloads.

This read-only smoke downloads AO2 and ao2-control-plane public GitHub Release
archives for one target label, verifies SHA256SUMS entries, extracts release
manifests safely, runs AO2 version/help and control-plane help, and writes
ao2.public-release-consumer-smoke.v1 evidence.

Required command probes include AO2 `version --json`, AO2 `--help`, and
ao2-control-plane `--help`.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-root)
      OUT_ROOT="${2:?missing value for --out-root}"
      LATEST_ROOT="$OUT_ROOT/latest"
      shift 2
      ;;
    --target-label)
      TARGET_LABEL="${2:?missing value for --target-label}"
      shift 2
      ;;
    --fixture-dir)
      FIXTURE_DIR="${2:?missing value for --fixture-dir}"
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

case "$TARGET_LABEL" in
  linux-x86_64|macos-aarch64|windows-x86_64) ;;
  *)
    echo "unsupported public consumer target label: $TARGET_LABEL" >&2
    exit 2
    ;;
esac

AO2_VERSION="${AO2_RELEASE_TAG#v}"
AO2_CP_VERSION="${AO2_CP_RELEASE_TAG#v}"
AO2_ARCHIVE_NAME="ao2-$AO2_VERSION-$TARGET_LABEL.tar.gz"
AO2_CP_ARCHIVE_NAME="ao2-control-plane-$AO2_CP_VERSION-$TARGET_LABEL.tar.gz"

rm -rf "$LATEST_ROOT"
mkdir -p "$LATEST_ROOT"

AO2_DOWNLOAD="$LATEST_ROOT/public-downloads/ao2"
CP_DOWNLOAD="$LATEST_ROOT/public-downloads/control-plane"
SMOKE_ROOT="$LATEST_ROOT/smoke"
mkdir -p "$AO2_DOWNLOAD" "$CP_DOWNLOAD" "$SMOKE_ROOT"

if [[ -n "$FIXTURE_DIR" ]]; then
  if [[ ! -d "$FIXTURE_DIR/ao2" || ! -d "$FIXTURE_DIR/control-plane" ]]; then
    echo "fixture dir must contain ao2/ and control-plane/: $FIXTURE_DIR" >&2
    exit 1
  fi
  cp -R "$FIXTURE_DIR/ao2/." "$AO2_DOWNLOAD/"
  cp -R "$FIXTURE_DIR/control-plane/." "$CP_DOWNLOAD/"
else
  if ! command -v gh >/dev/null 2>&1; then
    echo "missing gh CLI" >&2
    exit 1
  fi

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
fi

verify_checksum() {
  local dir="$1"
  local asset="$2"
  (cd "$dir" && grep "  $asset$" SHA256SUMS > "SHA256SUMS.$asset")
  if command -v shasum >/dev/null 2>&1; then
    (cd "$dir" && shasum -a 256 -c "SHA256SUMS.$asset")
  elif command -v sha256sum >/dev/null 2>&1; then
    (cd "$dir" && sha256sum -c "SHA256SUMS.$asset")
  else
    echo "missing checksum verifier: shasum or sha256sum required" >&2
    exit 1
  fi
}

verify_checksum "$AO2_DOWNLOAD" "$AO2_ARCHIVE_NAME"
verify_checksum "$CP_DOWNLOAD" "$AO2_CP_ARCHIVE_NAME"
if [[ -f "$CP_DOWNLOAD/summary.json" ]]; then
  verify_checksum "$CP_DOWNLOAD" "summary.json"
fi

python3 - \
  "$AO2_DOWNLOAD/$AO2_ARCHIVE_NAME" \
  "$CP_DOWNLOAD/$AO2_CP_ARCHIVE_NAME" \
  "${CP_DOWNLOAD}/summary.json" \
  "$SMOKE_ROOT" \
  "$LATEST_ROOT/summary.json" \
  "$TARGET_LABEL" \
  "$AO2_RELEASE_REPO" \
  "$AO2_RELEASE_TAG" \
  "$AO2_CP_RELEASE_REPO" \
  "$AO2_CP_RELEASE_TAG" \
  "$FIXTURE_DIR" <<'PY'
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
from datetime import datetime, timezone
from pathlib import Path

ao2_archive = Path(sys.argv[1]).resolve()
cp_archive = Path(sys.argv[2]).resolve()
cp_release_summary = Path(sys.argv[3]).resolve()
smoke_root = Path(sys.argv[4]).resolve()
summary_path = Path(sys.argv[5]).resolve()
target_label = sys.argv[6]
ao2_repo = sys.argv[7]
ao2_tag = sys.argv[8]
cp_repo = sys.argv[9]
cp_tag = sys.argv[10]
fixture_dir = sys.argv[11] or None


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


def run_command(name: str, command: list[str], output_path: Path) -> dict:
    env = {
        "PATH": os.environ.get("PATH", ""),
    }
    result = subprocess.run(
        command,
        text=True,
        capture_output=True,
        env=env,
        check=False,
        timeout=30,
    )
    output_path.write_text(result.stdout, encoding="utf-8")
    stderr_path = output_path.with_suffix(output_path.suffix + ".stderr")
    stderr_path.write_text(result.stderr, encoding="utf-8")
    return {
        "name": name,
        "command": [Path(command[0]).name, *command[1:]],
        "status": "passed" if result.returncode == 0 else "failed",
        "exit_code": result.returncode,
        "stdout_path": str(output_path),
        "stderr_path": str(stderr_path),
    }


smoke_root.mkdir(parents=True, exist_ok=True)
ao2_extract = smoke_root / "ao2-extract"
cp_extract = smoke_root / "control-plane-extract"
for path in [ao2_extract, cp_extract]:
    if path.exists():
        shutil.rmtree(path)

extract(ao2_archive, ao2_extract)
extract(cp_archive, cp_extract)

ao2_manifest = load_json(ao2_extract / "RELEASE-MANIFEST.json")
cp_manifest = load_json(cp_extract / "RELEASE-MANIFEST.json")
ao2_binary = ao2_extract / ao2_manifest["binary_path"]
cp_binary = cp_extract / cp_manifest["binary_path"]
if not ao2_binary.name.endswith(".exe"):
    ao2_binary.chmod(ao2_binary.stat().st_mode | 0o755)
if not cp_binary.name.endswith(".exe"):
    cp_binary.chmod(cp_binary.stat().st_mode | 0o755)

commands = {
    "ao2_version": run_command(
        "ao2_version",
        [str(ao2_binary), "version", "--json"],
        smoke_root / "ao2-version.json",
    ),
    "ao2_help": run_command(
        "ao2_help",
        [str(ao2_binary), "--help"],
        smoke_root / "ao2-help.txt",
    ),
    "control_plane_help": run_command(
        "control_plane_help",
        [str(cp_binary), "--help"],
        smoke_root / "control-plane-help.txt",
    ),
}

blockers = []
if ao2_manifest.get("schema_version") != "ao2.release-manifest.v1":
    blockers.append("ao2_release_manifest_schema")
if cp_manifest.get("schema_version") != "ao2-control-plane.release-manifest.v1":
    blockers.append("control_plane_release_manifest_schema")
if ao2_manifest.get("target") not in (None, target_label):
    blockers.append("ao2_release_manifest_target")
if cp_manifest.get("target") not in (None, target_label):
    blockers.append("control_plane_release_manifest_target")
for command_name, command_summary in commands.items():
    if command_summary["status"] != "passed":
        blockers.append(f"{command_name}_command")

ao2_version_identity = None
if commands["ao2_version"]["status"] == "passed":
    try:
        ao2_version_identity = load_json(smoke_root / "ao2-version.json")
    except Exception:
        blockers.append("ao2_version_json")
if isinstance(ao2_version_identity, dict):
    if ao2_version_identity.get("package") != "ao2":
        blockers.append("ao2_version_package")
    if ao2_version_identity.get("target") != target_label:
        blockers.append("ao2_version_target")
    if ao2_version_identity.get("release_manifest_schema") != "ao2.release-manifest.v1":
        blockers.append("ao2_version_release_manifest_schema")

cp_summary_schema = None
if cp_release_summary.is_file():
    try:
        cp_summary_schema = load_json(cp_release_summary).get("schema_version")
    except Exception:
        blockers.append("control_plane_release_summary_json")

summary = {
    "schema_version": "ao2.public-release-consumer-smoke.v1",
    "status": "passed" if not blockers else "failed",
    "generated_at": utc_now(),
    "target_label": target_label,
    "fixture_dir": fixture_dir,
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
            "release_summary_path": str(cp_release_summary) if cp_release_summary.is_file() else None,
            "release_summary_schema": cp_summary_schema,
        },
    },
    "commands": commands,
    "evidence": {
        "ao2_version": str(smoke_root / "ao2-version.json"),
        "ao2_help": str(smoke_root / "ao2-help.txt"),
        "control_plane_help": str(smoke_root / "control-plane-help.txt"),
    },
    "trust_boundary": {
        "local_only": True,
        "downloads_public_release_archives": True,
        "stores_credentials": False,
        "auth_value_stored": False,
        "credential_material_included": False,
        "credential_material_in_urls": False,
        "mutates_github_releases": False,
        "control_plane_approves_release": False,
    },
    "blockers": blockers,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={summary['status']}")
if blockers:
    raise SystemExit("public release consumer smoke failed: " + ",".join(blockers))
PY
