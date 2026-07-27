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
$AO2_RELEASE_REPO|Public Release Consumer Smoke|public-release-consumer-smoke-linux|public-consumer-linux
$AO2_RELEASE_REPO|Public Release Consumer Smoke|public-release-consumer-smoke-macos|public-consumer-macos
$AO2_RELEASE_REPO|Public Release Consumer Smoke|public-release-consumer-smoke-windows|public-consumer-windows
$AO2_RELEASE_REPO|Post Stable Release Verification|ao2-dual-public-release-smoke|dual-public-release-smoke
$AO2_RELEASE_REPO|Post Release Pair Digest Audit|ao2-public-release-pair-digest-audit|public-pair-digest-audit
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
        "component": "ao2+ao2-control-plane",
        "platform": "linux",
        "artifact": "public-release-consumer-smoke-linux",
        "path": download_root / "public-consumer-linux",
        "kind": "public-release-consumer-smoke",
        "target_label": "linux-x86_64",
    },
    {
        "component": "ao2+ao2-control-plane",
        "platform": "macos",
        "artifact": "public-release-consumer-smoke-macos",
        "path": download_root / "public-consumer-macos",
        "kind": "public-release-consumer-smoke",
        "target_label": "macos-aarch64",
    },
    {
        "component": "ao2+ao2-control-plane",
        "platform": "windows",
        "artifact": "public-release-consumer-smoke-windows",
        "path": download_root / "public-consumer-windows",
        "kind": "public-release-consumer-smoke",
        "target_label": "windows-x86_64",
    },
    {
        "component": "ao2",
        "platform": "public-release-pair",
        "artifact": "ao2-dual-public-release-smoke",
        "path": download_root / "dual-public-release-smoke",
        "kind": "ao2-dual-public-release-smoke",
    },
    {
        "component": "ao2",
        "platform": "public-release-pair",
        "artifact": "ao2-public-release-pair-digest-audit",
        "path": download_root / "public-pair-digest-audit",
        "kind": "public-pair-digest-audit",
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
        install_evidence_paths = sorted(
            (item["path"] / "smoke").rglob("ao2*.install-verification.json")
        )
        doctor_path = item["path"] / "smoke" / "doctor.json"
        if len(install_evidence_paths) != 1:
            status = "missing"
            details["missing"] = "smoke/home/**/ao2*.install-verification.json"
            details["install_evidence_count"] = len(install_evidence_paths)
        elif not doctor_path.is_file():
            status = "missing"
            details["missing"] = "smoke/doctor.json"
        else:
            install_evidence = load_json(install_evidence_paths[0])
            doctor = load_json(doctor_path)
            offline = install_evidence.get("offline_verification", {})
            doctor_install = doctor.get("install", {})
            doctor_evidence = doctor_install.get("verification_evidence", {})
            expected_target = {
                "linux": "linux-x86_64",
                "macos": "macos-aarch64",
                "windows": "windows-x86_64",
            }[item["platform"]]
            details["install_evidence"] = str(install_evidence_paths[0])
            details["install_schema_version"] = install_evidence.get("schema_version")
            details["install_status"] = install_evidence.get("install_status")
            details["offline_verification_status"] = offline.get("status")
            details["doctor"] = str(doctor_path)
            details["doctor_schema_version"] = doctor.get("schema_version")
            details["doctor_status"] = doctor.get("status")
            details["target"] = doctor.get("target")
            if not (
                install_evidence.get("schema_version")
                == "ao2.install-verification-evidence.v1"
                and install_evidence.get("status") == "verified"
                and install_evidence.get("install_status") == "installed"
                and install_evidence.get("target") == expected_target
                and offline.get("schema_version")
                == "ao2.release-archive-offline-verification.v1"
                and offline.get("status") == "verified"
                and offline.get("checksum_coverage_verified") is True
                and doctor.get("schema_version") == "ao2.doctor.v1"
                and doctor.get("status") == "ok"
                and doctor.get("target") == expected_target
                and doctor.get("version") == install_evidence.get("version")
                and doctor_install.get("installed") is True
                and doctor_install.get("on_path") is True
                and doctor_evidence.get("present") is True
                and doctor_evidence.get("status") == "verified"
            ):
                status = "failed"
    elif item["kind"] == "public-release-consumer-smoke":
        summary = item["path"] / "latest" / "summary.json"
        if not summary.is_file():
            status = "missing"
            details["missing"] = "latest/summary.json"
        else:
            payload = load_json(summary)
            archives = payload.get("archives", {})
            commands = payload.get("commands", {})
            trust = payload.get("trust_boundary", {})
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
            details["summary_status"] = payload.get("status")
            details["target_label"] = payload.get("target_label")
            details["ao2_manifest_schema"] = archives.get("ao2", {}).get("manifest_schema")
            details["control_plane_manifest_schema"] = archives.get(
                "ao2_control_plane", {}
            ).get("manifest_schema")
            details["ao2_version_status"] = commands.get("ao2_version", {}).get("status")
            details["ao2_help_status"] = commands.get("ao2_help", {}).get("status")
            details["control_plane_help_status"] = commands.get(
                "control_plane_help", {}
            ).get("status")
            details["downloads_public_release_archives"] = trust.get(
                "downloads_public_release_archives"
            )
            details["auth_value_stored"] = trust.get("auth_value_stored")
            details["credential_material_in_urls"] = trust.get(
                "credential_material_in_urls"
            )
            details["credential_material_included"] = trust.get(
                "credential_material_included"
            )
            details["mutates_github_releases"] = trust.get("mutates_github_releases")
            details["control_plane_approves_release"] = trust.get(
                "control_plane_approves_release"
            )
            if (
                payload.get("schema_version") != "ao2.public-release-consumer-smoke.v1"
                or payload.get("status") != "passed"
                or payload.get("target_label") != item["target_label"]
                or archives.get("ao2", {}).get("manifest_schema") != "ao2.release-manifest.v1"
                or archives.get("ao2_control_plane", {}).get("manifest_schema")
                != "ao2-control-plane.release-manifest.v1"
                or commands.get("ao2_version", {}).get("status") != "passed"
                or commands.get("ao2_help", {}).get("status") != "passed"
                or commands.get("control_plane_help", {}).get("status") != "passed"
                or trust.get("downloads_public_release_archives") is not True
                or trust.get("auth_value_stored") is not False
                or trust.get("credential_material_in_urls") is not False
                or trust.get("credential_material_included") is not False
                or trust.get("mutates_github_releases") is not False
                or trust.get("control_plane_approves_release") is not False
            ):
                status = "failed"
    elif item["kind"] == "ao2-dual-public-release-smoke":
        summary = item["path"] / "latest" / "summary.json"
        readback = item["path"] / "latest" / "smoke" / "task-board-readback.json"
        dashboard = item["path"] / "latest" / "smoke" / "task-board-dashboard.json"
        if not summary.is_file():
            status = "missing"
            details["missing"] = "latest/summary.json"
        elif not readback.is_file():
            status = "missing"
            details["missing"] = "latest/smoke/task-board-readback.json"
        elif not dashboard.is_file():
            status = "missing"
            details["missing"] = "latest/smoke/task-board-dashboard.json"
        else:
            payload = load_json(summary)
            readback_payload = load_json(readback)
            dashboard_payload = load_json(dashboard)
            trust = payload.get("trust_boundary", {})
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
            details["summary_status"] = payload.get("status")
            details["readback"] = str(readback)
            details["task_board_readback_schema"] = readback_payload.get("schema_version")
            details["dashboard"] = str(dashboard)
            details["task_board_dashboard_schema"] = dashboard_payload.get("schema_version")
            details["auth_value_stored"] = trust.get("auth_value_stored")
            details["credential_material_in_urls"] = trust.get("credential_material_in_urls")
            details["credential_material_included"] = trust.get("credential_material_included")
            details["mutates_github_releases"] = trust.get("mutates_github_releases")
            details["control_plane_approves_release"] = trust.get("control_plane_approves_release")
            if (
                payload.get("schema_version") != "ao2.dual-public-release-smoke.v1"
                or payload.get("status") != "passed"
                or readback_payload.get("schema_version") != "ao2.cp-ai-task-board-readback.v1"
                or dashboard_payload.get("schema_version") != "ao2.cp-ai-task-board-dashboard.v1"
                or trust.get("auth_value_stored") is not False
                or trust.get("credential_material_in_urls") is not False
                or trust.get("credential_material_included") is not False
                or trust.get("mutates_github_releases") is not False
                or trust.get("control_plane_approves_release") is not False
            ):
                status = "failed"
    elif item["kind"] == "public-pair-digest-audit":
        summary = find_json_with_schema(item["path"], "ao2.public-release-pair-digest-audit.v1")
        if summary is None:
            status = "missing"
            details["missing"] = "ao2.public-release-pair-digest-audit.v1"
        else:
            payload = load_json(summary)
            trust = payload.get("trust_boundary", {})
            archive_parity = payload.get("archive_parity", {})
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
            details["summary_status"] = payload.get("status")
            details["archive_parity_status"] = archive_parity.get("status")
            details["mutates_releases"] = trust.get("mutates_releases")
            details["stores_credentials"] = trust.get("stores_credentials")
            if (
                payload.get("status") != "passed"
                or archive_parity.get("status") != "passed"
                or trust.get("mutates_releases") is not False
                or trust.get("stores_credentials") is not False
            ):
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
