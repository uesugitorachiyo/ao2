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
            "ao2-0.4.80-linux-x86_64.tar.gz",
            "ao2-0.4.80-macos-aarch64.tar.gz",
            "ao2-0.4.80-windows-x86_64.tar.gz",
            "ao2-release-artifact-closure-index.json",
            "ao2-release-readiness-summary.json",
            "ao2-release-train-control-plane-bridge-summary.json",
            "SHA256SUMS",
        ],
    },
    {
        "name": "ao2-control-plane",
        "repo": "uesugitorachiyo/ao2-control-plane",
        "tag": "v0.1.12",
        "expected_assets": [
            "ao2-control-plane-0.1.12-macos-aarch64.tar.gz",
            "ao2-control-plane-release-support-fixture-parity-summary.json",
            "ao2-control-plane-release-train-bridge-macos-summary.json",
            "ao2-control-plane-release-train-bridge-ubuntu-summary.json",
            "ao2-control-plane-release-train-bridge-windows-summary.json",
            "SHA256SUMS",
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
    results.append(
        {
            "name": component["name"],
            "repo": component["repo"],
            "tag": component["tag"],
            "release_url": release.get("url"),
            "is_prerelease": release.get("isPrerelease"),
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
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
