#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_CP_RELEASE_REPO="${AO2_CP_RELEASE_REPO:-uesugitorachiyo/ao2-control-plane}"
AO2_OPERATOR_RELEASE_EVIDENCE_ROOT="${AO2_OPERATOR_RELEASE_EVIDENCE_ROOT:-$ROOT/target/operator-release-evidence-bundle/latest}"
AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR="${AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR:-}"
DOWNLOAD_ROOT="$AO2_OPERATOR_RELEASE_EVIDENCE_ROOT/downloaded-artifacts"
SUMMARY="$AO2_OPERATOR_RELEASE_EVIDENCE_ROOT/summary.json"
DOWNLOAD_LOG="$AO2_OPERATOR_RELEASE_EVIDENCE_ROOT/download.log"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --fixture-dir)
      AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR="${2:-}"
      if [ -z "$AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR" ]; then
        echo "--fixture-dir requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--fixture-dir <path>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$AO2_OPERATOR_RELEASE_EVIDENCE_ROOT"
mkdir -p "$DOWNLOAD_ROOT"
: >"$DOWNLOAD_LOG"

download_latest_artifact() {
  local repo="$1"
  local workflow="$2"
  local artifact="$3"
  local dest="$4"
  local run_id=""
  rm -rf "$dest"
  mkdir -p "$dest"

  while IFS= read -r candidate_run_id; do
    [ -n "$candidate_run_id" ] || continue
    rm -rf "$dest"
    mkdir -p "$dest"
    run_id="$candidate_run_id"
    if gh run download "$run_id" --repo "$repo" --name "$artifact" --dir "$dest"; then
      printf "downloaded repo=%s workflow=%s artifact=%s run_id=%s dest=%s\n" \
        "$repo" "$workflow" "$artifact" "$run_id" "$dest" >>"$DOWNLOAD_LOG"
      printf "%s\n" "$run_id" >"$dest/run-id.txt"
      return 0
    fi
  done < <(gh run list --repo "$repo" --branch main --workflow "$workflow" --status success --limit 10 --json databaseId --jq '.[].databaseId')

  printf "missing repo=%s workflow=%s artifact=%s\n" "$repo" "$workflow" "$artifact" >>"$DOWNLOAD_LOG"
  return 1
}

download_status="passed"
if [ -n "$AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR" ]; then
  if [ ! -d "$AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR" ]; then
    echo "fixture dir not found: $AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR" >&2
    exit 1
  fi
  cp -R "$AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR"/. "$DOWNLOAD_ROOT"/
  download_status="fixture"
  printf "operator_release_evidence_bundle=fixture source=%s\n" \
    "$AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR" >>"$DOWNLOAD_LOG"
else
  while IFS='|' read -r repo workflow artifact dest_name; do
    [ -n "$repo" ] || continue
    dest="$DOWNLOAD_ROOT/$dest_name"
    if ! download_latest_artifact "$repo" "$workflow" "$artifact" "$dest"; then
      download_status="failed"
    fi
  done <<EOF
$AO2_RELEASE_REPO|CI|ao2-dual-repo-release-publication-closure-index|ao2-dual-repo-release-publication-closure-index
$AO2_RELEASE_REPO|Post Stable Release Verification|post-stable-release-smoke-Linux|ao2-linux
$AO2_RELEASE_REPO|Post Stable Release Verification|post-stable-release-smoke-macOS|ao2-macos
$AO2_RELEASE_REPO|Post Stable Release Verification|post-stable-release-smoke-Windows|ao2-windows
$AO2_CP_RELEASE_REPO|Post Release Verification|ao2-control-plane-post-release-verification-ubuntu|control-plane-ubuntu
$AO2_CP_RELEASE_REPO|Post Release Verification|ao2-control-plane-post-release-verification-macos|control-plane-macos
$AO2_CP_RELEASE_REPO|Post Release Verification|ao2-control-plane-post-release-verification-windows|control-plane-windows
EOF
fi

python3 - "$DOWNLOAD_ROOT" "$SUMMARY" "$download_status" "$AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR" "$DOWNLOAD_LOG" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

download_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
download_status = sys.argv[3]
fixture_dir = sys.argv[4] or None
download_log = Path(sys.argv[5]).resolve()

required = [
    {
        "component": "ao2",
        "platform": "ci",
        "artifact": "ao2-dual-repo-release-publication-closure-index",
        "path": download_root / "ao2-dual-repo-release-publication-closure-index",
        "kind": "dual-repo-index",
    },
    {
        "component": "ao2",
        "platform": "linux",
        "artifact": "post-stable-release-smoke-Linux",
        "path": download_root / "ao2-linux",
        "kind": "ao2-post-stable",
    },
    {
        "component": "ao2",
        "platform": "macos",
        "artifact": "post-stable-release-smoke-macOS",
        "path": download_root / "ao2-macos",
        "kind": "ao2-post-stable",
    },
    {
        "component": "ao2",
        "platform": "windows",
        "artifact": "post-stable-release-smoke-Windows",
        "path": download_root / "ao2-windows",
        "kind": "ao2-post-stable",
    },
    {
        "component": "ao2-control-plane",
        "platform": "ubuntu",
        "artifact": "ao2-control-plane-post-release-verification-ubuntu",
        "path": download_root / "control-plane-ubuntu",
        "kind": "control-plane-post-release",
    },
    {
        "component": "ao2-control-plane",
        "platform": "macos",
        "artifact": "ao2-control-plane-post-release-verification-macos",
        "path": download_root / "control-plane-macos",
        "kind": "control-plane-post-release",
    },
    {
        "component": "ao2-control-plane",
        "platform": "windows",
        "artifact": "ao2-control-plane-post-release-verification-windows",
        "path": download_root / "control-plane-windows",
        "kind": "control-plane-post-release",
    },
]


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def find_json_with_schema(root: Path, schema_version: str) -> Optional[Path]:
    for path in sorted(root.rglob("*.json")):
        try:
            payload = load_json(path)
        except Exception:
            continue
        if payload.get("schema_version") == schema_version:
            return path
    return None


checks = []
for item in required:
    status = "passed"
    details = {
        "component": item["component"],
        "platform": item["platform"],
        "artifact": item["artifact"],
        "path": str(item["path"]),
        "kind": item["kind"],
    }
    if not item["path"].is_dir():
        status = "missing"
        details["missing"] = "artifact_directory"
    elif item["kind"] == "dual-repo-index":
        summary = find_json_with_schema(
            item["path"], "ao2.dual-repo-release-publication-closure-index.v1"
        )
        if summary is None:
            status = "missing"
            details["missing"] = "ao2.dual-repo-release-publication-closure-index.v1"
        else:
            payload = load_json(summary)
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
            details["summary_status"] = payload.get("status")
            if payload.get("status") not in {"passed", "ready", "indexed"}:
                status = "failed"
    elif item["kind"] == "ao2-post-stable":
        install_update = item["path"] / "smoke" / "install-update.json"
        if not install_update.is_file():
            status = "missing"
            details["missing"] = "smoke/install-update.json"
        else:
            payload = load_json(install_update)
            details["install_update"] = str(install_update)
            details["signature_verified"] = payload.get("signature_verified")
            details["install_status"] = payload.get("status")
            if payload.get("signature_verified") is not True or payload.get("status") != "installed":
                status = "failed"
    else:
        summary = find_json_with_schema(item["path"], "ao2.cp-release-publication-closure.v1")
        if summary is None:
            status = "missing"
            details["missing"] = "ao2.cp-release-publication-closure.v1"
        else:
            payload = load_json(summary)
            trust = payload.get("trust_boundary", {})
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
            details["summary_status"] = payload.get("status")
            details["checksum_verified"] = payload.get("checksum_verified")
            details["credential_material_included"] = trust.get("credential_material_included")
            details["mutates_github_releases"] = trust.get("mutates_github_releases")
            if (
                payload.get("status") != "passed"
                or payload.get("checksum_verified") is not True
                or trust.get("credential_material_included") is not False
                or trust.get("mutates_github_releases") is not False
            ):
                status = "failed"
    details["status"] = status
    checks.append(details)

ready = download_status != "failed" and all(check["status"] == "passed" for check in checks)
payload = {
    "schema_version": "ao2.operator-release-evidence-bundle.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "operator_release_evidence_ready": ready,
    "artifact_root": str(download_root),
    "download_status": download_status,
    "fixture_dir": fixture_dir,
    "checks": checks,
    "required_artifacts": [
        {
            "component": item["component"],
            "platform": item["platform"],
            "artifact": item["artifact"],
            "kind": item["kind"],
        }
        for item in required
    ],
    "trust_boundary": {
        "queries_github_actions": fixture_dir is None,
        "downloads_github_actions_artifacts": fixture_dir is None,
        "mutates_releases": False,
        "stores_credentials": False,
    },
    "download_log": str(download_log),
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
print(f"operator_release_evidence_ready={str(ready).lower()}")
if not ready:
    raise SystemExit(1)
PY
