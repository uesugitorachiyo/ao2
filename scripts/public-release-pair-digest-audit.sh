#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT:-$ROOT/target/public-release-pair-digest-audit/latest}"
SUMMARY="$OUT_ROOT/summary.json"
eval "$("$ROOT/scripts/release-train-env.sh" "${AO2_RELEASE_TRAIN:-stable}")"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

# Read-only post-release audit. It compares public GitHub Release asset digest
# metadata from `gh release view` against the dual-repo publication closure
# index produced by CI.
python3 - "$ROOT" "$SUMMARY" \
  "$AO2_RELEASE_TRAIN_MANIFEST" \
  "$AO2_RELEASE_TRAIN_NAME" \
  "$AO2_RELEASE_TRAIN_AO2_TAG" \
  "$AO2_RELEASE_TRAIN_AO2_VERSION" \
  "$AO2_RELEASE_TRAIN_CP_TAG" \
  "$AO2_RELEASE_TRAIN_CP_VERSION" <<'PY'
import html
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
manifest_path = Path(sys.argv[3])
release_train_name = sys.argv[4]
ao2_tag = sys.argv[5]
ao2_version = sys.argv[6]
cp_tag = sys.argv[7]
cp_version = sys.argv[8]

components = [
    {
        "component": "ao2",
        "repo": "uesugitorachiyo/ao2",
        "tag": ao2_tag,
        "expected_release_name": f"AO2 {ao2_tag} stable",
        "release_view_env": "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON",
        "archive_prefix": "ao2-",
        "required_archive_names": [
            f"ao2-{ao2_version}-linux-aarch64.tar.gz",
            f"ao2-{ao2_version}-linux-x86_64.tar.gz",
            f"ao2-{ao2_version}-macos-aarch64.tar.gz",
            f"ao2-{ao2_version}-windows-x86_64.tar.gz",
        ],
    },
    {
        "component": "ao2-control-plane",
        "repo": "uesugitorachiyo/ao2-control-plane",
        "tag": cp_tag,
        "expected_release_name": f"ao2-control-plane {cp_tag}",
        "release_view_env": "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON",
        "archive_prefix": "ao2-control-plane-",
        "required_archive_names": [
            f"ao2-control-plane-{cp_version}-linux-x86_64.tar.gz",
            f"ao2-control-plane-{cp_version}-macos-aarch64.tar.gz",
            f"ao2-control-plane-{cp_version}-windows-x86_64.tar.gz",
        ],
    },
]


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def release_view(component: dict) -> dict:
    fixture = os.environ.get(component["release_view_env"], "")
    if fixture:
        return load_json(Path(fixture))
    return json.loads(
        subprocess.check_output(
            [
                "gh",
                "release",
                "view",
                component["tag"],
                "--repo",
                component["repo"],
                "--json",
                "tagName,name,isPrerelease,url,publishedAt,assets",
            ],
            text=True,
        )
    )


def normalize_digest(value) -> str:
    digest = str(value or "")
    if digest.startswith("sha256:"):
        digest = digest.removeprefix("sha256:")
    return digest.lower()


def valid_sha256(value) -> bool:
    digest = normalize_digest(value)
    return len(digest) == 64 and all(char in "0123456789abcdef" for char in digest)


def archive_assets(release: dict, prefix: str) -> list[dict]:
    return [
        asset
        for asset in release.get("assets", [])
        if isinstance(asset, dict)
        and str(asset.get("name", "")).startswith(prefix)
        and str(asset.get("name", "")).endswith(".tar.gz")
    ]


def closure_archive_assets(closure_component: dict, prefix: str) -> list[dict]:
    candidates = closure_component.get("archive_assets")
    if not isinstance(candidates, list):
        candidates = closure_component.get("assets", [])
    return [
        asset
        for asset in candidates
        if isinstance(asset, dict)
        and str(asset.get("name", "")).startswith(prefix)
        and str(asset.get("name", "")).endswith(".tar.gz")
    ]


def add_check(checks: list[dict], component: str, code: str, status: str, **details) -> None:
    checks.append({"component": component, "code": code, "status": status, **details})


checks = []
components_out = []
release_views = {}
for component in components:
    release = release_view(component)
    release_views[component["component"]] = release
    component_checks = []
    add_check(
        component_checks,
        component["component"],
        "release_tag_drift",
        "passed" if release.get("tagName") == component["tag"] else "failed",
        expected=component["tag"],
        observed=release.get("tagName"),
    )
    add_check(
        component_checks,
        component["component"],
        "release_name_drift",
        "passed"
        if release.get("name") == component["expected_release_name"]
        else "failed",
        expected=component["expected_release_name"],
        observed=release.get("name"),
    )
    add_check(
        component_checks,
        component["component"],
        "release_channel_drift",
        "passed" if release.get("isPrerelease") is False else "failed",
        expected="stable",
        observed="prerelease" if release.get("isPrerelease") else "stable",
    )
    archives = archive_assets(release, component["archive_prefix"])
    published_archive_names = {asset.get("name") for asset in archives}
    missing_published_required = [
        name
        for name in component["required_archive_names"]
        if name not in published_archive_names
    ]
    add_check(
        component_checks,
        component["component"],
        "published_archive_assets_present",
        "passed" if archives else "failed",
        archive_count=len(archives),
    )
    add_check(
        component_checks,
        component["component"],
        "published_required_archive_presence",
        "passed" if not missing_published_required else "failed",
        required_archive_names=component["required_archive_names"],
        missing_assets=missing_published_required,
    )
    for asset in archives:
        add_check(
            component_checks,
            component["component"],
            "published_asset_digest_present",
            "passed" if valid_sha256(asset.get("digest")) else "failed",
            asset=asset.get("name"),
            digest=asset.get("digest"),
        )
        add_check(
            component_checks,
            component["component"],
            "published_asset_size_match",
            "passed"
            if isinstance(asset.get("size"), int) and asset.get("size") > 0
            else "failed",
            asset=asset.get("name"),
            size=asset.get("size"),
        )
    checks.extend(component_checks)
    components_out.append(
        {
            "component": component["component"],
            "repo": component["repo"],
            "tag": component["tag"],
            "release_url": release.get("url"),
            "archive_count": len(archives),
            "status": "passed"
            if all(item["status"] == "passed" for item in component_checks)
            else "failed",
        }
    )

closure_path = os.environ.get(
    "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON", ""
)
closure_checks = []
archive_parity_components = {}
closure = None
if closure_path:
    closure = load_json(Path(closure_path))
    add_check(
        closure_checks,
        "dual-repo",
        "dual_repo_closure_schema",
        "passed"
        if closure.get("schema_version")
        == "ao2.dual-repo-release-publication-closure-index.v1"
        else "failed",
        observed=closure.get("schema_version"),
    )
    for component in components:
        component_name = component["component"]
        closure_key = "control_plane" if component_name == "ao2-control-plane" else "ao2"
        closure_component = closure.get(closure_key, {})
        published_assets = {
            asset.get("name"): asset
            for asset in release_views[component_name].get("assets", [])
            if isinstance(asset, dict) and asset.get("name")
        }
        published_archives_by_name = {
            name: asset
            for name, asset in published_assets.items()
            if str(name).startswith(component["archive_prefix"])
            and str(name).endswith(".tar.gz")
        }
        closure_archives = closure_archive_assets(
            closure_component,
            component["archive_prefix"],
        )
        closure_archives_by_name = {
            asset.get("name"): asset for asset in closure_archives if asset.get("name")
        }
        missing_closure_required = [
            name
            for name in component["required_archive_names"]
            if name not in closure_archives_by_name
        ]
        missing_published_required = [
            name
            for name in component["required_archive_names"]
            if name not in published_archives_by_name
        ]
        missing_required = sorted(
            set(missing_closure_required + missing_published_required)
        )
        closure_without_published_assets = sorted(
            set(closure_archives_by_name) - set(published_archives_by_name)
        )
        published_without_closure_assets = sorted(
            set(published_archives_by_name) - set(closure_archives_by_name)
        )
        add_check(
            closure_checks,
            component_name,
            "required_archive_presence",
            "passed" if not missing_required else "failed",
            required_archive_names=component["required_archive_names"],
            missing_assets=missing_required,
            closure_missing_assets=missing_closure_required,
            published_missing_assets=missing_published_required,
        )
        add_check(
            closure_checks,
            component_name,
            "public_archive_closure_parity",
            "passed"
            if not closure_without_published_assets
            and not published_without_closure_assets
            else "failed",
            closure_without_published_assets=closure_without_published_assets,
            published_without_closure_assets=published_without_closure_assets,
        )
        mismatched_assets = []
        for asset in closure_archives:
            asset_name = asset["name"]
            published = published_archives_by_name.get(asset_name)
            expected_digest = normalize_digest(asset.get("sha256"))
            observed_digest = normalize_digest((published or {}).get("digest"))
            digest_matches = (
                published is not None
                and valid_sha256(asset.get("sha256"))
                and expected_digest == observed_digest
            )
            size_matches = (
                published is not None
                and isinstance(asset.get("size_bytes"), int)
                and asset.get("size_bytes") == published.get("size")
            )
            if not digest_matches or not size_matches:
                mismatched_assets.append(asset_name)
            add_check(
                closure_checks,
                component_name,
                "dual_repo_closure_digest_match",
                "passed" if digest_matches else "failed",
                asset=asset_name,
                closure_sha256=asset.get("sha256"),
                published_digest=(published or {}).get("digest"),
            )
            add_check(
                closure_checks,
                component_name,
                "dual_repo_closure_size_match",
                "passed" if size_matches else "failed",
                asset=asset_name,
                closure_size_bytes=asset.get("size_bytes"),
                published_size=(published or {}).get("size"),
            )
        archive_parity_components[component_name] = {
            "status": "passed"
            if not missing_required
            and not closure_without_published_assets
            and not published_without_closure_assets
            and not mismatched_assets
            else "failed",
            "required_archive_names": component["required_archive_names"],
            "required_archive_count": len(component["required_archive_names"]),
            "closure_archive_assets": sorted(closure_archives_by_name),
            "published_archive_assets": sorted(published_archives_by_name),
            "missing_assets": missing_required,
            "closure_without_published_assets": closure_without_published_assets,
            "published_without_closure_assets": published_without_closure_assets,
            "mismatched_assets": sorted(mismatched_assets),
        }
else:
    add_check(
        closure_checks,
        "dual-repo",
        "dual_repo_closure_index_supplied",
        "failed",
        expected_env="AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON",
    )

checks.extend(closure_checks)
full_archive_parity = (
    "passed"
    if archive_parity_components
    and all(item["status"] == "passed" for item in archive_parity_components.values())
    else "failed"
)
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.public-release-pair-digest-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(summary_path.parent),
    "release_train": {
        "name": release_train_name,
        "manifest": str(manifest_path),
        "ao2": {"tag": ao2_tag, "version": ao2_version},
        "ao2_control_plane": {"tag": cp_tag, "version": cp_version},
    },
    "components": components_out,
    "closure_index": str(Path(closure_path)) if closure_path else None,
    "archive_parity": {
        "status": full_archive_parity,
        "components": archive_parity_components,
    },
    "checks": checks,
    "trust_boundary": {
        "queries_public_releases": True,
        "reads_dual_repo_closure_index": True,
        "mutates_releases": False,
        "stores_credentials": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

rows = [
    "<tr>"
    f"<td>{html.escape(item['component'])}</td>"
    f"<td>{html.escape(item['code'])}</td>"
    f"<td>{html.escape(item['status'])}</td>"
    f"<td>{html.escape(str(item.get('asset', '')))}</td>"
    "</tr>"
    for item in checks
]
dashboard_path = summary_path.with_name("dashboard.html")
dashboard_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Public Release Pair Digest Audit</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body><h1>Public Release Pair Digest Audit</h1>"
    f"<p>Status: <code>{html.escape(status)}</code></p>"
    "<table><thead><tr><th>Component</th><th>Check</th><th>Status</th><th>Asset</th></tr></thead>"
    f"<tbody>{''.join(rows)}</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
