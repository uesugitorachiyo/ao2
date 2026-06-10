#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_VERSION="${AO2_VERSION:-$("$ROOT/scripts/current-version.sh")}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v$AO2_VERSION}"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_RELEASE_PROVENANCE_DIR="${AO2_RELEASE_PROVENANCE_DIR:-$ROOT/dist-provenance}"
AO2_RELEASE_SYNC_ROOT="${AO2_RELEASE_SYNC_ROOT:-$ROOT/target/release-sync-provenance-assets/latest}"
AO2_RELEASE_SYNC_CONFIRM="${AO2_RELEASE_SYNC_CONFIRM:-}"
SUMMARY="$AO2_RELEASE_SYNC_ROOT/summary.json"
RELEASE_JSON="$AO2_RELEASE_SYNC_ROOT/release.json"
PLAN_JSON="$AO2_RELEASE_SYNC_ROOT/plan.json"
UPLOAD_LIST="$AO2_RELEASE_SYNC_ROOT/upload-assets.txt"
UPLOAD_LOG="$AO2_RELEASE_SYNC_ROOT/upload.log"

rm -rf "$AO2_RELEASE_SYNC_ROOT"
mkdir -p "$AO2_RELEASE_SYNC_ROOT"

gh release view "$AO2_RELEASE_TAG" \
  --repo "$AO2_RELEASE_REPO" \
  --json tagName,name,isPrerelease,publishedAt,assets,url \
  > "$RELEASE_JSON"

python3 - "$ROOT" "$AO2_RELEASE_SYNC_ROOT" "$AO2_RELEASE_TAG" "$AO2_RELEASE_REPO" \
  "$AO2_RELEASE_PROVENANCE_DIR" "$RELEASE_JSON" "$PLAN_JSON" "$UPLOAD_LIST" \
  "$AO2_RELEASE_SYNC_CONFIRM" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
out_root = Path(sys.argv[2])
release_tag = sys.argv[3]
release_repo = sys.argv[4]
provenance_dir = Path(sys.argv[5])
release_json_path = Path(sys.argv[6])
plan_path = Path(sys.argv[7])
upload_list_path = Path(sys.argv[8])
confirm = sys.argv[9]

release = json.loads(release_json_path.read_text(encoding="utf-8"))
observed_assets = sorted(asset["name"] for asset in release.get("assets", []))
observed_set = set(observed_assets)
required_assets = [
    "ao2-release-provenance.json",
    "ao2-release-provenance.json.sig",
    "ao2-release-signing-public.pem",
]
optional_assets = sorted(
    path.name
    for path in provenance_dir.glob("ao2-*.tar.gz.*")
    if path.suffix in {".sha256", ".sig"}
)
candidate_assets = required_assets + optional_assets
missing_local_assets = [
    name for name in candidate_assets if not (provenance_dir / name).is_file()
]
already_published_assets = [
    name for name in candidate_assets if name in observed_set
]
missing_remote_assets = [
    name for name in candidate_assets if name not in observed_set
]
upload_assets = [
    str((provenance_dir / name).resolve())
    for name in missing_remote_assets
    if (provenance_dir / name).is_file()
]
upload_list_path.write_text("\n".join(upload_assets) + ("\n" if upload_assets else ""), encoding="utf-8")

confirm_token = f"sync-{release_tag}"
confirmed = confirm == confirm_token
blockers = []
if missing_local_assets:
    blockers.append(
        {
            "code": "local_provenance_assets_missing",
            "severity": "blocking",
            "missing_assets": missing_local_assets,
            "message": "Build and sign release provenance before syncing GitHub Release sidecars.",
        }
    )
if not upload_assets and missing_remote_assets:
    blockers.append(
        {
            "code": "no_uploadable_provenance_assets",
            "severity": "blocking",
            "message": "Remote assets are missing, but no local files are available to upload.",
        }
    )

status = "blocked" if blockers else ("ready_to_upload" if upload_assets else "already_synced")
plan = {
    "schema_version": "ao2.release-sync-provenance-assets.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "dry_run": not confirmed,
    "confirmed": confirmed,
    "required_confirm": confirm_token,
    "release_tag": release_tag,
    "release_repo": release_repo,
    "release_url": release.get("url"),
    "is_prerelease": bool(release.get("isPrerelease")),
    "provenance_dir": str(provenance_dir),
    "required_assets": required_assets,
    "optional_assets": optional_assets,
    "observed_assets": observed_assets,
    "already_published_assets": already_published_assets,
    "missing_remote_assets": missing_remote_assets,
    "missing_local_assets": missing_local_assets,
    "upload_assets": upload_assets,
    "upload_list": str(upload_list_path),
    "blockers": blockers,
    "trust_boundary": {
        "queries_public_releases": True,
        "mutates_releases": confirmed,
        "stores_credentials": False,
    },
}
plan_path.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

plan_status="$(python3 - "$PLAN_JSON" <<'PY'
import json
import sys
from pathlib import Path
print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["status"])
PY
)"

upload_status="not_attempted"
if [ "$AO2_RELEASE_SYNC_CONFIRM" = "sync-$AO2_RELEASE_TAG" ]; then
  if [ "$plan_status" != "ready_to_upload" ]; then
    echo "refusing provenance sync because plan status is $plan_status" >&2
    cp "$PLAN_JSON" "$SUMMARY"
    exit 1
  fi
  # shellcheck disable=SC2046
  gh release upload "$AO2_RELEASE_TAG" $(cat "$UPLOAD_LIST") \
    --repo "$AO2_RELEASE_REPO" \
    --clobber \
    > "$UPLOAD_LOG" 2>&1
  upload_status="uploaded"
fi

python3 - "$PLAN_JSON" "$SUMMARY" "$upload_status" "$UPLOAD_LOG" <<'PY'
import json
import sys
from pathlib import Path

plan_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
upload_status = sys.argv[3]
upload_log = Path(sys.argv[4])
payload = json.loads(plan_path.read_text(encoding="utf-8"))
if upload_status == "uploaded":
    payload["status"] = "uploaded"
payload["upload_status"] = upload_status
payload["upload_log"] = str(upload_log)
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
print(f"dry_run={str(payload['dry_run']).lower()}")
print(f"upload_status={upload_status}")
PY
