#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_CI_ARTIFACT_DOWNLOAD_ROOT:-$ROOT/target/ci-artifacts/latest}"
SUMMARY="$OUT_ROOT/summary.json"
CONSUMER_ROOT="$OUT_ROOT/release-artifact-consumer-smoke"
FIXTURE_DIR="${AO2_CI_ARTIFACT_DOWNLOAD_FIXTURE_DIR:-}"
REQUIRED_ARTIFACTS="${AO2_CI_ARTIFACT_REQUIRED_ARTIFACTS:-ao2-python-guard}"
REQUIRED_SCHEMAS="${AO2_CI_ARTIFACT_REQUIRED_SCHEMAS:-ao2.python-guard-ci-artifacts.v1}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --fixture-dir)
      FIXTURE_DIR="${2:-}"
      if [ -z "$FIXTURE_DIR" ]; then
        echo "--fixture-dir requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --require-artifact)
      REQUIRED_ARTIFACTS="${REQUIRED_ARTIFACTS}${REQUIRED_ARTIFACTS:+ }${2:-}"
      if [ -z "${2:-}" ]; then
        echo "--require-artifact requires a name fragment" >&2
        exit 2
      fi
      shift 2
      ;;
    --require-schema)
      REQUIRED_SCHEMAS="${REQUIRED_SCHEMAS}${REQUIRED_SCHEMAS:+ }${2:-}"
      if [ -z "${2:-}" ]; then
        echo "--require-schema requires a schema_version" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--fixture-dir <path>] [--require-artifact <name-fragment>] [--require-schema <schema_version>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

consumer_args=()
if [ -n "$FIXTURE_DIR" ]; then
  consumer_args+=(--fixture-dir "$FIXTURE_DIR")
fi
for artifact in $REQUIRED_ARTIFACTS; do
  consumer_args+=(--require-artifact "$artifact")
done
for schema in $REQUIRED_SCHEMAS; do
  consumer_args+=(--require-schema "$schema")
done

AO2_RELEASE_ARTIFACT_CONSUMER_ROOT="$CONSUMER_ROOT" \
  npm run release:artifact-consumer-smoke -- "${consumer_args[@]}"

mkdir -p "$CP_ROOT/target/ci-artifacts/latest"

python3 - "$ROOT" "$CP_ROOT" "$OUT_ROOT" "$CONSUMER_ROOT/summary.json" "$SUMMARY" "$FIXTURE_DIR" <<'PY'
import hashlib
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
cp_root = Path(sys.argv[2]).resolve()
out_root = Path(sys.argv[3]).resolve()
consumer_summary_path = Path(sys.argv[4]).resolve()
summary_path = Path(sys.argv[5]).resolve()
fixture_dir = sys.argv[6] or None

consumer = json.loads(consumer_summary_path.read_text(encoding="utf-8"))
clean_workspace = Path(consumer["clean_workspace"]).resolve()

def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()

def copy_repo_artifacts(repo: str, destination: Path) -> list[str]:
    destination.mkdir(parents=True, exist_ok=True)
    copied = []
    candidates = [
        clean_workspace / repo,
        clean_workspace / repo.replace("/", "-"),
        clean_workspace / f"uesugitorachiyo-{repo.split('/')[-1]}",
    ]
    for candidate in candidates:
        if candidate.exists():
            shutil.copytree(candidate, destination / "downloaded-artifacts", dirs_exist_ok=True)
            copied.append(str(candidate))
            break
    return copied

ao2_sources = copy_repo_artifacts("uesugitorachiyo/ao2", out_root)
cp_sources = copy_repo_artifacts("uesugitorachiyo/ao2-control-plane", cp_root / "target/ci-artifacts/latest")

cp_marker = cp_root / "target/ci-artifacts/latest/summary.json"
cp_payload = {
    "schema_version": "ao2.cp-ci-artifact-download-contract.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if consumer.get("status") == "passed" else "failed",
    "source_consumer_summary": str(consumer_summary_path),
    "copied_sources": cp_sources,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "source": "gh run download",
    },
}
cp_marker.write_text(json.dumps(cp_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

payload = {
    "schema_version": "ao2.ci-artifact-download-contract.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if consumer.get("status") == "passed" else "failed",
    "artifact_root": str(out_root),
    "fixture_dir": fixture_dir,
    "consumer_summary": str(consumer_summary_path),
    "download_command": "gh run download",
    "required_artifacts": consumer.get("required_artifacts", []),
    "missing_required_artifacts": consumer.get("missing_required_artifacts", []),
    "required_schemas": consumer.get("required_schemas", []),
    "missing_required_schemas": consumer.get("missing_required_schemas", []),
    "schema_versions": consumer.get("schema_versions", []),
    "files": consumer.get("files", []),
    "mirrors": {
        "ao2": {
            "root": str(out_root),
            "copied_sources": ao2_sources,
        },
        "ao2-control-plane": {
            "root": str(cp_root / "target/ci-artifacts/latest"),
            "summary": str(cp_marker),
            "copied_sources": cp_sources,
        },
    },
    "resume_command_digest": sha256_text("npm run artifacts:ci-download-contract"),
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "source": "github_actions_artifacts",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
