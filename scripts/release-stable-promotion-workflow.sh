#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v$("$ROOT/scripts/current-version.sh")}"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_CP_RELEASE_TAG="${AO2_CP_RELEASE_TAG:-v0.1.13}"
AO2_CP_RELEASE_REPO="${AO2_CP_RELEASE_REPO:-uesugitorachiyo/ao2-control-plane}"
AO2_STABLE_PROMOTION_ROOT="${AO2_STABLE_PROMOTION_ROOT:-$ROOT/target/stable-promotion-workflow/latest}"
AO2_STABLE_PROMOTION_CONFIRM="${AO2_STABLE_PROMOTION_CONFIRM:-}"
AO2_STABLE_PROMOTION_EVIDENCE_ROOT="${AO2_STABLE_PROMOTION_EVIDENCE_ROOT:-$AO2_STABLE_PROMOTION_ROOT/post-release-verification-evidence}"
AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR="${AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR:-}"
AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD="${AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD:-0}"
# Default release train confirmation: AO2_STABLE_PROMOTION_CONFIRM=promote-stable-v0.4.80-v0.1.13
READINESS_ROOT="$AO2_STABLE_PROMOTION_ROOT/stable-release-readiness"
READINESS_SUMMARY="$READINESS_ROOT/summary.json"
EVIDENCE_SUMMARY="$AO2_STABLE_PROMOTION_EVIDENCE_ROOT/summary.json"
EVIDENCE_LOG="$AO2_STABLE_PROMOTION_ROOT/post-release-evidence.log"
SUMMARY="$AO2_STABLE_PROMOTION_ROOT/summary.json"
PLAN="$AO2_STABLE_PROMOTION_ROOT/plan.json"
PROMOTION_LOG="$AO2_STABLE_PROMOTION_ROOT/promotion.log"
READINESS_LOG="$AO2_STABLE_PROMOTION_ROOT/stable-readiness.log"

rm -rf "$AO2_STABLE_PROMOTION_ROOT"
mkdir -p "$AO2_STABLE_PROMOTION_ROOT"

AO2_STABLE_RELEASE_READINESS_ROOT="$READINESS_ROOT" npm run release:stable-readiness \
  > "$READINESS_LOG" 2>&1

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
        "$repo" "$workflow" "$artifact" "$run_id" "$dest" >> "$EVIDENCE_LOG"
      printf "%s\n" "$run_id" > "$dest/run-id.txt"
      return 0
    fi
  done < <(gh run list --repo "$repo" --branch main --workflow "$workflow" --status success --limit 10 --json databaseId --jq '.[].databaseId')

  printf "missing repo=%s workflow=%s artifact=%s\n" "$repo" "$workflow" "$artifact" >> "$EVIDENCE_LOG"
  return 1
}

rm -rf "$AO2_STABLE_PROMOTION_EVIDENCE_ROOT"
mkdir -p "$AO2_STABLE_PROMOTION_EVIDENCE_ROOT"
printf "stable_promotion_evidence_gate=start\n" > "$EVIDENCE_LOG"

download_status="passed"
if [ -n "$AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR" ]; then
  if [ -d "$AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR" ]; then
    cp -R "$AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR"/. "$AO2_STABLE_PROMOTION_EVIDENCE_ROOT"/
    printf "stable_promotion_evidence_gate=fixture fixture_dir=%s\n" \
      "$AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR" >> "$EVIDENCE_LOG"
  else
    download_status="failed"
    printf "stable_promotion_evidence_gate=fixture_missing fixture_dir=%s\n" \
      "$AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR" >> "$EVIDENCE_LOG"
  fi
elif [ "$AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD" = "1" ]; then
  download_status="skipped"
  printf "stable_promotion_evidence_gate=skipped\n" >> "$EVIDENCE_LOG"
else
  while IFS='|' read -r repo workflow artifact dest_name; do
    [ -n "$repo" ] || continue
    dest="$AO2_STABLE_PROMOTION_EVIDENCE_ROOT/$dest_name"
    if ! download_latest_artifact "$repo" "$workflow" "$artifact" "$dest"; then
      download_status="failed"
    fi
  done <<EOF
$AO2_RELEASE_REPO|Post Stable Release Verification|post-stable-release-smoke-Linux|ao2-linux
$AO2_RELEASE_REPO|Post Stable Release Verification|post-stable-release-smoke-macOS|ao2-macos
$AO2_RELEASE_REPO|Post Stable Release Verification|post-stable-release-smoke-Windows|ao2-windows
$AO2_RELEASE_REPO|Post Stable Release Verification|ao2-dual-public-release-smoke|dual-public-release-smoke
$AO2_RELEASE_REPO|Post Release Pair Digest Audit|ao2-public-release-pair-digest-audit|public-pair-digest-audit
$AO2_CP_RELEASE_REPO|Post Release Verification|ao2-control-plane-post-release-verification-ubuntu|control-plane-ubuntu
$AO2_CP_RELEASE_REPO|Post Release Verification|ao2-control-plane-post-release-verification-macos|control-plane-macos
$AO2_CP_RELEASE_REPO|Post Release Verification|ao2-control-plane-post-release-verification-windows|control-plane-windows
EOF
fi

python3 - "$AO2_STABLE_PROMOTION_EVIDENCE_ROOT" "$EVIDENCE_SUMMARY" "$download_status" "$EVIDENCE_LOG" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
download_status = sys.argv[3]
log_path = Path(sys.argv[4])

required = [
    {
        "component": "ao2",
        "platform": "linux",
        "artifact": "post-stable-release-smoke-Linux",
        "path": root / "ao2-linux",
        "kind": "ao2-post-stable",
    },
    {
        "component": "ao2",
        "platform": "macos",
        "artifact": "post-stable-release-smoke-macOS",
        "path": root / "ao2-macos",
        "kind": "ao2-post-stable",
    },
    {
        "component": "ao2",
        "platform": "windows",
        "artifact": "post-stable-release-smoke-Windows",
        "path": root / "ao2-windows",
        "kind": "ao2-post-stable",
    },
    {
        "component": "ao2",
        "platform": "public-release-pair",
        "artifact": "ao2-dual-public-release-smoke",
        "path": root / "dual-public-release-smoke",
        "kind": "ao2-dual-public-release-smoke",
    },
    {
        "component": "ao2",
        "platform": "public-pair-digest-audit",
        "artifact": "ao2-public-release-pair-digest-audit",
        "path": root / "public-pair-digest-audit",
        "kind": "public-pair-digest-audit",
    },
    {
        "component": "ao2-control-plane",
        "platform": "ubuntu",
        "artifact": "ao2-control-plane-post-release-verification-ubuntu",
        "path": root / "control-plane-ubuntu",
        "kind": "control-plane-post-release",
    },
    {
        "component": "ao2-control-plane",
        "platform": "macos",
        "artifact": "ao2-control-plane-post-release-verification-macos",
        "path": root / "control-plane-macos",
        "kind": "control-plane-post-release",
    },
    {
        "component": "ao2-control-plane",
        "platform": "windows",
        "artifact": "ao2-control-plane-post-release-verification-windows",
        "path": root / "control-plane-windows",
        "kind": "control-plane-post-release",
    },
]

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
    if download_status == "skipped":
        status = "skipped"
        details["skip_reason"] = "AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD=1"
    elif item["kind"] != "public-pair-digest-audit" and not item["path"].is_dir():
        status = "missing"
        details["missing"] = "artifact_directory"
    elif item["kind"] == "ao2-post-stable":
        install_update = item["path"] / "smoke" / "install-update.json"
        if not install_update.is_file():
            status = "missing"
            details["missing"] = "smoke/install-update.json"
        else:
            payload = json.loads(install_update.read_text(encoding="utf-8"))
            details["install_update"] = str(install_update)
            details["signature_verified"] = payload.get("signature_verified")
            details["install_status"] = payload.get("status")
            if payload.get("signature_verified") is not True or payload.get("status") != "installed":
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
            payload = json.loads(summary.read_text(encoding="utf-8"))
            readback_payload = json.loads(readback.read_text(encoding="utf-8"))
            dashboard_payload = json.loads(dashboard.read_text(encoding="utf-8"))
            trust = payload.get("trust_boundary", {})
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
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
        summary = item["path"] / "target" / "post-release-pair-digest-audit" / "summary.json"
        if not summary.is_file():
            status = "missing"
            details["missing"] = "target/post-release-pair-digest-audit/summary.json"
        else:
            payload = json.loads(summary.read_text(encoding="utf-8"))
            trust = payload.get("trust_boundary", {})
            archive_parity = payload.get("archive_parity", {})
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
            details["summary_status"] = payload.get("status")
            details["archive_parity_status"] = archive_parity.get("status")
            details["mutates_releases"] = trust.get("mutates_releases")
            details["stores_credentials"] = trust.get("stores_credentials")
            if (
                payload.get("schema_version") != "ao2.public-release-pair-digest-audit.v1"
                or payload.get("status") != "passed"
                or archive_parity.get("status") != "passed"
                or trust.get("mutates_releases") is not False
                or trust.get("stores_credentials") is not False
            ):
                status = "failed"
    else:
        summary = item["path"] / "summary.json"
        if not summary.is_file():
            status = "missing"
            details["missing"] = "summary.json"
        else:
            payload = json.loads(summary.read_text(encoding="utf-8"))
            trust = payload.get("trust_boundary", {})
            details["summary"] = str(summary)
            details["schema_version"] = payload.get("schema_version")
            details["checksum_verified"] = payload.get("checksum_verified")
            details["credential_material_included"] = trust.get("credential_material_included")
            details["mutates_github_releases"] = trust.get("mutates_github_releases")
            if (
                payload.get("schema_version") != "ao2.cp-release-publication-closure.v1"
                or payload.get("status") != "passed"
                or payload.get("checksum_verified") is not True
                or trust.get("credential_material_included") is not False
                or trust.get("mutates_github_releases") is not False
            ):
                status = "failed"
    details["status"] = status
    checks.append(details)

ready = download_status == "passed" and all(item["status"] == "passed" for item in checks)
payload = {
    "schema_version": "ao2.stable-promotion-evidence-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "skipped" if download_status == "skipped" else "failed",
    "post_release_evidence_ready": ready,
    "download_status": download_status,
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
        "queries_github_actions": True,
        "downloads_github_actions_artifacts": download_status != "skipped",
        "mutates_releases": False,
        "stores_credentials": False,
    },
    "log": str(log_path),
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"stable_promotion_evidence_gate={payload['status']}")
print(f"post_release_evidence_ready={str(ready).lower()}")
print(f"stable_promotion_evidence_summary={summary_path}")
PY

python3 - "$READINESS_SUMMARY" "$PLAN" "$AO2_RELEASE_REPO" "$AO2_RELEASE_TAG" \
  "$AO2_CP_RELEASE_REPO" "$AO2_CP_RELEASE_TAG" "$AO2_STABLE_PROMOTION_CONFIRM" \
  "$EVIDENCE_SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

readiness_path = Path(sys.argv[1])
plan_path = Path(sys.argv[2])
ao2_repo = sys.argv[3]
ao2_tag = sys.argv[4]
cp_repo = sys.argv[5]
cp_tag = sys.argv[6]
confirm = sys.argv[7]
evidence_path = Path(sys.argv[8])

readiness = json.loads(readiness_path.read_text(encoding="utf-8"))
evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
allowed_channel_blockers = {"stable_release_absent", "current_channel_is_prerelease"}
expected = {
    ("ao2", ao2_repo, ao2_tag),
    ("ao2-control-plane", cp_repo, cp_tag),
}
observed = {
    (component.get("name"), component.get("repo"), component.get("tag"))
    for component in readiness.get("components", [])
}
missing_components = sorted(
    (
        {"name": name, "repo": repo, "tag": tag}
        for name, repo, tag in expected.difference(observed)
    ),
    key=lambda item: (item["name"], item["repo"], item["tag"]),
)
non_channel_blockers = [
    blocker
    for blocker in readiness.get("promotion_blockers", [])
    if blocker.get("code") not in allowed_channel_blockers
]
channel_blockers = [
    blocker
    for blocker in readiness.get("promotion_blockers", [])
    if blocker.get("code") in allowed_channel_blockers
]

required_confirm = f"promote-stable-{ao2_tag}-{cp_tag}"
confirmed = confirm == required_confirm
evidence_ready = evidence.get("post_release_evidence_ready") is True
stable_channel_only = not non_channel_blockers and not missing_components and evidence_ready and bool(channel_blockers)
already_stable = bool(readiness.get("stable_release_ready")) and not readiness.get("promotion_blockers")
status = (
    "ready_to_promote"
    if stable_channel_only
    else "blocked"
    if (missing_components or non_channel_blockers or not evidence_ready)
    else "already_stable"
    if already_stable
    else "blocked"
)

blockers = []
if missing_components:
    blockers.append(
        {
            "code": "release_component_missing_from_readiness",
            "severity": "blocking",
            "components": missing_components,
            "message": "Stable promotion requires AO2 and ao2-control-plane readiness components.",
        }
    )
if not evidence_ready:
    blockers.append(
        {
            "code": "post_release_evidence_missing",
            "severity": "blocking",
            "evidence_gate_status": evidence.get("status"),
            "evidence_gate_summary": str(evidence_path),
            "message": "Stable promotion requires AO2 and ao2-control-plane post-release verification evidence.",
        }
    )
if non_channel_blockers:
    blockers.append(
        {
            "code": "non_channel_promotion_blockers_present",
            "severity": "blocking",
            "blockers": non_channel_blockers,
            "message": "Stable promotion can only proceed after non-channel blockers are resolved.",
        }
    )

plan = {
    "schema_version": "ao2.stable-promotion-workflow.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "dry_run": not confirmed,
    "confirmed": confirmed,
    "required_confirm": required_confirm,
    "stable_channel_only": stable_channel_only,
    "post_release_evidence_ready": evidence_ready,
    "stable_promotion_evidence_gate": str(evidence_path),
    "evidence_gate_status": evidence.get("status"),
    "readiness_summary": str(readiness_path),
    "promotion_targets": [
        {"name": "ao2", "repo": ao2_repo, "tag": ao2_tag},
        {"name": "ao2-control-plane", "repo": cp_repo, "tag": cp_tag},
    ],
    "channel_blockers": channel_blockers,
    "non_channel_blockers": non_channel_blockers,
    "blockers": blockers,
    "planned_commands": [
        f"gh release edit {ao2_tag} --repo {ao2_repo} --prerelease=false --latest",
        f"gh release edit {cp_tag} --repo {cp_repo} --prerelease=false --latest",
    ],
    "trust_boundary": {
        "queries_public_releases": True,
        "mutates_releases": confirmed,
        "stores_credentials": False,
    },
}
plan_path.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

plan_status="$(
  python3 - "$PLAN" <<'PY'
import json
import sys
from pathlib import Path
print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["status"])
PY
)"

promotion_status="not_attempted"
if [ "$AO2_STABLE_PROMOTION_CONFIRM" = "promote-stable-$AO2_RELEASE_TAG-$AO2_CP_RELEASE_TAG" ]; then
  if [ "$plan_status" != "ready_to_promote" ]; then
    echo "refusing stable promotion because plan status is $plan_status" >&2
    cp "$PLAN" "$SUMMARY"
    exit 1
  fi
  {
    gh release edit "$AO2_RELEASE_TAG" \
      --repo "$AO2_RELEASE_REPO" \
      --prerelease=false \
      --latest
    gh release edit "$AO2_CP_RELEASE_TAG" \
      --repo "$AO2_CP_RELEASE_REPO" \
      --prerelease=false \
      --latest
  } > "$PROMOTION_LOG" 2>&1
  promotion_status="promoted"
fi

python3 - "$PLAN" "$SUMMARY" "$promotion_status" "$PROMOTION_LOG" <<'PY'
import json
import sys
from pathlib import Path

plan_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
promotion_status = sys.argv[3]
promotion_log = Path(sys.argv[4])
payload = json.loads(plan_path.read_text(encoding="utf-8"))
if promotion_status == "promoted":
    payload["status"] = "promoted"
payload["promotion_status"] = promotion_status
payload["promotion_log"] = str(promotion_log)
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
print(f"dry_run={str(payload['dry_run']).lower()}")
print(f"promotion_status={promotion_status}")
PY
