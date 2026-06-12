#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_STABLE_RELEASE_READINESS_ROOT:-$ROOT/target/stable-release-readiness/latest}"
SUMMARY="$OUT_ROOT/summary.json"
ASSET_ROOT="$OUT_ROOT/release-asset-completeness"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

AO2_RELEASE_ASSET_COMPLETENESS_ROOT="$ASSET_ROOT" npm run release:asset-completeness

python3 - "$OUT_ROOT" "$SUMMARY" "$ASSET_ROOT/summary.json" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
asset_summary_path = Path(sys.argv[3])
asset_summary = json.loads(asset_summary_path.read_text(encoding="utf-8"))

component_results = []
for component in asset_summary["components"]:
    blockers = []
    if not component.get("stable_release_present", False):
        blockers.append(
            {
                "code": "stable_release_absent",
                "severity": "blocking",
                "message": "No stable GitHub Release is published for this component.",
            }
        )
    if component.get("release_channel") == "prerelease":
        blockers.append(
            {
                "code": "current_channel_is_prerelease",
                "severity": "blocking",
                "message": "The current public artifact is explicitly marked as a prerelease.",
            }
        )
    release_name = str(component.get("release_name") or "")
    if component.get("stable_release_present", False) and any(
        marker in release_name.lower()
        for marker in ("alpha", "pre-release", "prerelease", "preview")
    ):
        blockers.append(
            {
                "code": "stable_release_label_mentions_alpha",
                "severity": "blocking",
                "message": "Stable release metadata must not describe the release as alpha, prerelease, or preview.",
                "release_name": release_name,
            }
        )
    if component["name"] == "ao2":
        observed = set(component.get("observed_assets", []))
        required_signed_provenance = {
            "ao2-release-signing-public.pem",
            "ao2-release-provenance.json",
            "ao2-release-provenance.json.sig",
        }
        missing_signed_provenance = sorted(required_signed_provenance.difference(observed))
        if missing_signed_provenance:
            blockers.append(
                {
                    "code": "signed_provenance_public_key_missing",
                    "severity": "blocking",
                    "message": "Stable release promotion requires signed provenance/public-key sidecars.",
                    "missing_assets": missing_signed_provenance,
                }
            )
    if component.get("status") != "passed":
        blockers.append(
            {
                "code": "release_asset_completeness_failed",
                "severity": "blocking",
                "message": "Release asset completeness must pass before stable promotion.",
            }
        )

    component_results.append(
        {
            "name": component["name"],
            "repo": component["repo"],
            "tag": component["tag"],
            "release_name": component.get("release_name"),
            "release_url": component["release_url"],
            "release_channel": component["release_channel"],
            "stable_release_present": component["stable_release_present"],
            "asset_completeness_status": component["status"],
            "stable_release_ready": not blockers,
            "promotion_blockers": blockers,
        }
    )

payload = {
    "schema_version": "ao2.stable-release-readiness.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "ready" if all(item["stable_release_ready"] for item in component_results) else "blocked",
    "stable_release_ready": all(item["stable_release_ready"] for item in component_results),
    "artifact_root": str(out_root),
    "release_asset_completeness": str(asset_summary_path),
    "components": component_results,
    "promotion_blockers": [
        {
            "component": item["name"],
            **blocker,
        }
        for item in component_results
        for blocker in item["promotion_blockers"]
    ],
    "trust_boundary": {
        "queries_public_releases": True,
        "mutates_releases": False,
        "stores_credentials": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

dashboard_path = out_root / "dashboard.html"
rows = []
for item in component_results:
    blocker_codes = ", ".join(blocker["code"] for blocker in item["promotion_blockers"]) or "none"
    rows.append(
        "<tr>"
        f"<td>{html.escape(item['name'])}</td>"
        f"<td>{html.escape(str(item['release_name']))}</td>"
        f"<td><a href=\"{html.escape(item['release_url'])}\">{html.escape(item['tag'])}</a></td>"
        f"<td>{html.escape(item['release_channel'])}</td>"
        f"<td>{html.escape(str(item['stable_release_present']).lower())}</td>"
        f"<td>{html.escape(item['asset_completeness_status'])}</td>"
        f"<td>{html.escape(str(item['stable_release_ready']).lower())}</td>"
        f"<td>{html.escape(blocker_codes)}</td>"
        "</tr>"
    )
headline = "Ready for stable release" if payload["stable_release_ready"] else "Not ready for stable release"
dashboard_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Stable Release Readiness</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body><h1>Stable Release Readiness</h1>"
    f"<p>{html.escape(headline)}</p>"
    f"<p>Status: <code>{html.escape(payload['status'])}</code></p>"
    "<table><thead><tr><th>Component</th><th>Release Name</th><th>Tag</th><th>Channel</th><th>Stable Present</th>"
    "<th>Asset Gate</th><th>Stable Ready</th><th>Promotion Blockers</th></tr></thead>"
    f"<tbody>{''.join(rows)}</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
print(f"status={payload['status']}")
print(f"stable_release_ready={str(payload['stable_release_ready']).lower()}")
PY
