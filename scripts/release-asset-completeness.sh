#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_ASSET_COMPLETENESS_ROOT:-$ROOT/target/release-asset-completeness/latest}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

# Uses `gh release view` for release metadata and `gh release download` for
# checksum manifests. This gate reads public release state; it does not mutate.
python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import html
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])

components = [
    {
        "name": "ao2",
        "repo": "uesugitorachiyo/ao2",
        "tag": "v0.4.80",
        "expected_assets": [
            "ao2-0.4.80-linux-aarch64.tar.gz",
            "ao2-0.4.80-linux-aarch64.tar.gz.sha256",
            "ao2-0.4.80-linux-aarch64.tar.gz.sig",
            "ao2-0.4.80-linux-x86_64.tar.gz",
            "ao2-0.4.80-linux-x86_64.tar.gz.sha256",
            "ao2-0.4.80-linux-x86_64.tar.gz.sig",
            "ao2-0.4.80-macos-aarch64.tar.gz",
            "ao2-0.4.80-macos-aarch64.tar.gz.sha256",
            "ao2-0.4.80-macos-aarch64.tar.gz.sig",
            "ao2-0.4.80-windows-x86_64.tar.gz",
            "ao2-0.4.80-windows-x86_64.tar.gz.sha256",
            "ao2-0.4.80-windows-x86_64.tar.gz.sig",
            "ao2-release-artifact-closure-index.json",
            "ao2-release-provenance.json",
            "ao2-release-provenance.json.sig",
            "ao2-release-readiness-summary.json",
            "ao2-release-signing-public.pem",
            "ao2-release-train-control-plane-bridge-summary.json",
            "SHA256SUMS",
        ],
    },
    {
        "name": "ao2-control-plane",
        "repo": "uesugitorachiyo/ao2-control-plane",
        "tag": "v0.1.13",
        "expected_assets": [
            "ao2-control-plane-0.1.13-linux-x86_64.tar.gz",
            "ao2-control-plane-0.1.13-macos-aarch64.tar.gz",
            "ao2-control-plane-0.1.13-windows-x86_64.tar.gz",
            "SHA256SUMS",
            "summary.json",
        ],
    },
]


def run_json(cmd: list[str]) -> dict:
    return json.loads(subprocess.check_output(cmd, text=True))


def run(cmd: list[str]) -> None:
    subprocess.check_call(cmd)


results = []
for component in components:
    component_root = out_root / component["name"]
    component_root.mkdir(parents=True, exist_ok=True)
    release = run_json(
        [
            "gh",
            "release",
            "view",
            component["tag"],
            "--repo",
            component["repo"],
            "--json",
            "tagName,name,isPrerelease,publishedAt,assets,url",
        ]
    )
    observed_assets = sorted(asset["name"] for asset in release.get("assets", []))
    expected_assets = list(component["expected_assets"])
    missing_assets = sorted(set(expected_assets).difference(observed_assets))
    unexpected_assets = sorted(set(observed_assets).difference(expected_assets))

    run(
        [
            "gh",
            "release",
            "download",
            component["tag"],
            "--repo",
            component["repo"],
            "--pattern",
            "SHA256SUMS",
            "--dir",
            str(component_root),
            "--clobber",
        ]
    )
    checksums_path = component_root / "SHA256SUMS"
    checksum_lines = checksums_path.read_text(encoding="utf-8").splitlines()
    checksum_assets = set()
    malformed_checksum_lines = []
    for raw in checksum_lines:
        parts = raw.split()
        if len(parts) != 2 or len(parts[0]) != 64:
            malformed_checksum_lines.append(raw)
            continue
        checksum_assets.add(parts[1].lstrip("*"))

    expected_checksum_assets = [asset for asset in expected_assets if asset != "SHA256SUMS"]
    missing_checksum_entries = sorted(set(expected_checksum_assets).difference(checksum_assets))

    status = (
        "passed"
        if not missing_assets and not missing_checksum_entries and not malformed_checksum_lines
        else "failed"
    )
    is_prerelease = bool(release.get("isPrerelease"))
    stable_release_present = not is_prerelease
    release_channel = "prerelease" if is_prerelease else "stable"
    results.append(
        {
            "name": component["name"],
            "repo": component["repo"],
            "tag": component["tag"],
            "release_url": release.get("url"),
            "release_name": release.get("name"),
            "is_prerelease": is_prerelease,
            "stable_release_present": stable_release_present,
            "release_channel": release_channel,
            "published_at": release.get("publishedAt"),
            "status": status,
            "expected_assets": expected_assets,
            "observed_assets": observed_assets,
            "missing_assets": missing_assets,
            "unexpected_assets": unexpected_assets,
            "missing_checksum_entries": missing_checksum_entries,
            "malformed_checksum_lines": malformed_checksum_lines,
            "checksum_manifest": str(checksums_path),
        }
    )

payload = {
    "schema_version": "ao2.release-asset-completeness.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if all(item["status"] == "passed" for item in results) else "failed",
    "artifact_root": str(out_root),
    "components": results,
    "trust_boundary": {
        "queries_public_releases": True,
        "stores_credentials": False,
        "mutates_releases": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
dashboard_path = out_root / "dashboard.html"
rows = []
for item in results:
    stable_label = "Stable release present" if item["stable_release_present"] else "Stable release absent"
    prerelease_label = "Prerelease present" if item["is_prerelease"] else "Prerelease absent"
    rows.append(
        "<tr>"
        f"<td>{html.escape(item['name'])}</td>"
        f"<td>{html.escape(str(item['release_name']))}</td>"
        f"<td><a href=\"{html.escape(str(item['release_url']))}\">{html.escape(item['tag'])}</a></td>"
        f"<td>{html.escape(item['release_channel'])}</td>"
        f"<td>{html.escape(stable_label)}</td>"
        f"<td>{html.escape(prerelease_label)}</td>"
        f"<td>{html.escape(item['status'])}</td>"
        f"<td>{len(item['missing_assets'])}</td>"
        f"<td>{len(item['missing_checksum_entries'])}</td>"
        "</tr>"
    )
dashboard_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Release State Dashboard</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body><h1>AO2 Release State Dashboard</h1>"
    f"<p>Status: <code>{html.escape(payload['status'])}</code></p>"
    "<p>Stable release absent means the current public artifact is intentionally a prerelease, not a full stable release.</p>"
    "<table><thead><tr><th>Component</th><th>Release Name</th><th>Tag</th><th>Channel</th><th>Stable</th><th>Prerelease</th>"
    "<th>Asset Gate</th><th>Missing Assets</th><th>Missing Checksums</th></tr></thead>"
    f"<tbody>{''.join(rows)}</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
