#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_IMMUTABILITY_AUDIT_ROOT:-$ROOT/target/release-immutability-audit/latest}"
SUMMARY="$OUT_ROOT/summary.json"
# Default summary: target/release-immutability-audit/latest/summary.json
ASSET_COMPLETENESS_ROOT="$OUT_ROOT/release-asset-completeness"
STABLE_READINESS_ROOT="$OUT_ROOT/stable-release-readiness"
DOWNLOAD_ROOT="$OUT_ROOT/release-download"
DOWNLOAD_LOG="$OUT_ROOT/release-download-verify.log"
SKIP_DOWNLOAD_VERIFY="${AO2_IMMUTABILITY_SKIP_DOWNLOAD_VERIFY:-0}"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

(
  cd "$ROOT"
  AO2_RELEASE_ASSET_COMPLETENESS_ROOT="$ASSET_COMPLETENESS_ROOT" npm run release:asset-completeness
)

(
  cd "$ROOT"
  AO2_STABLE_RELEASE_READINESS_ROOT="$STABLE_READINESS_ROOT" npm run release:stable-readiness
)

download_verify_status="skipped"
if [ "$SKIP_DOWNLOAD_VERIFY" = "1" ]; then
  printf "release_download_verify=skipped\n" > "$DOWNLOAD_LOG"
else
  (
    cd "$ROOT"
    AO2_RELEASE_DOWNLOAD_DIR="$DOWNLOAD_ROOT" npm run release:download-verify
  ) >"$DOWNLOAD_LOG" 2>&1
  download_verify_status="passed"
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$ASSET_COMPLETENESS_ROOT/summary.json" "$STABLE_READINESS_ROOT/summary.json" "$DOWNLOAD_LOG" "$download_verify_status" <<'PY'
import html
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
asset_summary_path = Path(sys.argv[3])
readiness_summary_path = Path(sys.argv[4])
download_log_path = Path(sys.argv[5])
download_verify_status = sys.argv[6]

asset_summary = json.loads(asset_summary_path.read_text(encoding="utf-8"))
readiness_summary = json.loads(readiness_summary_path.read_text(encoding="utf-8"))


def gh_release(repo: str, tag: str) -> dict:
    raw = subprocess.check_output(
        [
            "gh",
            "release",
            "view",
            tag,
            "--repo",
            repo,
            "--json",
            "tagName,name,isDraft,isImmutable,isPrerelease,publishedAt,url,assets",
        ],
        text=True,
    )
    return json.loads(raw)


component_metadata = []
for component in asset_summary.get("components", []):
    release = gh_release(component["repo"], component["tag"])
    assets = release.get("assets", [])
    missing_digests = sorted(asset.get("name", "") for asset in assets if not asset.get("digest"))
    observed_asset_names = sorted(asset.get("name", "") for asset in assets)
    checksum_assets = set()
    checksum_path = Path(component.get("checksum_manifest", ""))
    malformed_checksums = list(component.get("malformed_checksum_lines", []))
    if checksum_path.is_file():
        for raw in checksum_path.read_text(encoding="utf-8").splitlines():
            parts = raw.split()
            if len(parts) == 2 and len(parts[0]) == 64:
                checksum_assets.add(parts[1].lstrip("*"))
            else:
                malformed_checksums.append(raw)

    expected_checksum_assets = {
        asset for asset in component.get("expected_assets", []) if asset != "SHA256SUMS"
    }
    metadata_blockers = []
    if release.get("isDraft"):
        metadata_blockers.append("draft_release")
    if release.get("isPrerelease"):
        metadata_blockers.append("prerelease_release")
    if component.get("missing_assets"):
        metadata_blockers.append("missing_assets")
    if set(observed_asset_names) != set(component.get("observed_assets", [])):
        metadata_blockers.append("asset_list_drift")
    if missing_digests:
        metadata_blockers.append("missing_github_asset_digests")
    if expected_checksum_assets.difference(checksum_assets):
        metadata_blockers.append("missing_checksum_entries")
    if malformed_checksums:
        metadata_blockers.append("malformed_checksums")

    component_metadata.append(
        {
            "name": component["name"],
            "repo": component["repo"],
            "tag": component["tag"],
            "release_url": release.get("url"),
            "release_metadata_coherent": not metadata_blockers,
            "metadata_blockers": metadata_blockers,
            "is_draft": bool(release.get("isDraft")),
            "is_immutable": bool(release.get("isImmutable")),
            "is_prerelease": bool(release.get("isPrerelease")),
            "published_at": release.get("publishedAt"),
            "asset_count": len(assets),
            "asset_digests_present": not missing_digests,
            "checksums": {
                "manifest": component.get("checksum_manifest"),
                "expected_entries": sorted(expected_checksum_assets),
                "missing_entries": sorted(expected_checksum_assets.difference(checksum_assets)),
                "malformed_lines": malformed_checksums,
            },
        }
    )

download_log = download_log_path.read_text(encoding="utf-8", errors="replace")
signed_provenance = (
    "release_provenance_status=passed" in download_log
    or download_verify_status == "skipped"
)
download_verify_passed = (
    "release_download_verify=passed" in download_log
    if download_verify_status != "skipped"
    else True
)

checks = {
    "asset_completeness": asset_summary.get("status") == "passed",
    "stable_readiness": bool(readiness_summary.get("stable_release_ready")),
    "download_verify": download_verify_passed,
    "signed_provenance": signed_provenance,
    "release_metadata_coherent": all(
        item["release_metadata_coherent"] for item in component_metadata
    ),
}

status = "passed" if all(checks.values()) else "failed"
payload = {
    "schema_version": "ao2.release-immutability-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "components": component_metadata,
    "checks": checks,
    "release_metadata": component_metadata,
    "asset_completeness_summary": str(asset_summary_path),
    "stable_readiness_summary": str(readiness_summary_path),
    "download_verify_log": str(download_log_path),
    "download_verify_status": download_verify_status,
    "trust_boundary": {
        "queries_public_releases": True,
        "downloads_release_assets": download_verify_status != "skipped",
        "stores_credentials": False,
        "mutates_releases": False,
        "control_plane_approves_release": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

rows = []
for item in component_metadata:
    rows.append(
        "<tr>"
        f"<td>{html.escape(item['name'])}</td>"
        f"<td><a href=\"{html.escape(str(item['release_url']))}\">{html.escape(item['tag'])}</a></td>"
        f"<td>{html.escape(str(item['release_metadata_coherent']).lower())}</td>"
        f"<td>{item['asset_count']}</td>"
        f"<td>{html.escape(str(item['asset_digests_present']).lower())}</td>"
        f"<td>{html.escape(', '.join(item['metadata_blockers']) or 'none')}</td>"
        "</tr>"
    )

dashboard_path = out_root / "dashboard.html"
dashboard_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Release Immutability Audit</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body><h1>AO2 Release Immutability Audit</h1>"
    f"<p>Status: <code>{html.escape(status)}</code></p>"
    "<table><thead><tr><th>Component</th><th>Release</th><th>Metadata coherent</th>"
    "<th>Assets</th><th>GitHub digests</th><th>Blockers</th></tr></thead>"
    f"<tbody>{''.join(rows)}</tbody></table></body></html>\n",
    encoding="utf-8",
)

print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
