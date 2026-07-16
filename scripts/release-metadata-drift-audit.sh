#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_METADATA_DRIFT_AUDIT_ROOT:-$ROOT/target/release-metadata-drift-audit/latest}"
SUMMARY="$OUT_ROOT/summary.json"
eval "$("$ROOT/scripts/release-train-env.sh" "${AO2_RELEASE_TRAIN:-stable}")"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

# Reads public GitHub Release metadata with `gh release view` plus tracked
# public docs. This audit is intentionally read-only: it does not edit releases,
# push branches, or store credentials.
python3 - "$ROOT" "$SUMMARY" \
  "$AO2_RELEASE_TRAIN_MANIFEST" \
  "$AO2_RELEASE_TRAIN_NAME" \
  "$AO2_RELEASE_TRAIN_AO2_TAG" \
  "$AO2_RELEASE_TRAIN_CP_TAG" <<'PY'
import html
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
manifest_path = Path(sys.argv[3])
release_train_name = sys.argv[4]
ao2_tag = sys.argv[5]
cp_tag = sys.argv[6]

components = [
    {
        "component": "ao2",
        "repo": "uesugitorachiyo/ao2",
        "tag": ao2_tag,
        "expected_release_name": f"AO2 {ao2_tag}",
        "expected_channel": "stable",
        "doc_expectations": {
            "docs/release/PUBLIC-RELEASE-VERIFICATION.md": [
                f"AO2 stable release: `{ao2_tag}`",
            ],
            "docs/INSTALL.md": [
                f"The current stable public release line is `{ao2_tag}`.",
            ],
        },
        "forbidden_doc_fragments": [
            f"AO2 prerelease: `{ao2_tag}`",
            "AO2 public alpha",
        ],
    },
    {
        "component": "ao2-control-plane",
        "repo": "uesugitorachiyo/ao2-control-plane",
        "tag": cp_tag,
        "expected_release_name": f"ao2-control-plane {cp_tag}",
        "expected_channel": "stable",
        "doc_expectations": {
            "docs/release/PUBLIC-RELEASE-VERIFICATION.md": [
                f"AO2 control-plane stable release: `{cp_tag}`",
            ],
        },
        "forbidden_doc_fragments": [
            "AO2 control-plane prerelease",
            "ao2-control-plane public alpha",
        ],
    },
]


def run_json(args: list[str]) -> dict:
    return json.loads(subprocess.check_output(args, text=True))


def add_check(checks: list[dict], code: str, status: str, details: dict) -> None:
    checks.append({"code": code, "status": status, **details})


checks = []
components_out = []
for component in components:
    release = run_json(
        [
            "gh",
            "release",
            "view",
            component["tag"],
            "--repo",
            component["repo"],
            "--json",
            "tagName,name,isPrerelease,url,publishedAt",
        ]
    )
    actual_channel = "prerelease" if release.get("isPrerelease") else "stable"
    component_checks = []

    add_check(
        component_checks,
        "release_tag_drift",
        "passed" if release.get("tagName") == component["tag"] else "failed",
        {
            "expected": component["tag"],
            "observed": release.get("tagName"),
        },
    )
    add_check(
        component_checks,
        "release_name_drift",
        "passed"
        if release.get("name") == component["expected_release_name"]
        else "failed",
        {
            "expected": component["expected_release_name"],
            "observed": release.get("name"),
        },
    )
    add_check(
        component_checks,
        "release_channel_drift",
        "passed" if actual_channel == component["expected_channel"] else "failed",
        {
            "expected": component["expected_channel"],
            "observed": actual_channel,
        },
    )

    for doc_path, expected_fragments in component["doc_expectations"].items():
        text = (root / doc_path).read_text(encoding="utf-8")
        for fragment in expected_fragments:
            add_check(
                component_checks,
                "doc_channel_drift",
                "passed" if fragment in text else "failed",
                {
                    "doc": doc_path,
                    "expected_fragment": fragment,
                },
            )
        for forbidden in component["forbidden_doc_fragments"]:
            add_check(
                component_checks,
                "doc_channel_drift",
                "passed" if forbidden not in text else "failed",
                {
                    "doc": doc_path,
                    "forbidden_fragment": forbidden,
                },
            )

    status = "passed" if all(item["status"] == "passed" for item in component_checks) else "failed"
    checks.extend({"component": component["component"], **item} for item in component_checks)
    components_out.append(
        {
            "component": component["component"],
            "repo": component["repo"],
            "tag": component["tag"],
            "release_url": release.get("url"),
            "release_name": release.get("name"),
            "release_channel": actual_channel,
            "published_at": release.get("publishedAt"),
            "status": status,
            "checks": component_checks,
        }
    )

status = "passed" if all(item["status"] == "passed" for item in components_out) else "failed"
payload = {
    "schema_version": "ao2.release-metadata-drift-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(summary_path.parent),
    "release_train": {
        "name": release_train_name,
        "manifest": str(manifest_path),
        "ao2": {"tag": ao2_tag},
        "ao2_control_plane": {"tag": cp_tag},
    },
    "components": components_out,
    "checks": checks,
    "trust_boundary": {
        "queries_public_releases": True,
        "reads_tracked_docs": True,
        "mutates_releases": False,
        "stores_credentials": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

rows = []
for item in components_out:
    rows.append(
        "<tr>"
        f"<td>{html.escape(item['component'])}</td>"
        f"<td>{html.escape(item['tag'])}</td>"
        f"<td>{html.escape(str(item['release_name']))}</td>"
        f"<td>{html.escape(item['release_channel'])}</td>"
        f"<td>{html.escape(item['status'])}</td>"
        "</tr>"
    )
dashboard_path = summary_path.with_name("dashboard.html")
dashboard_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Release Metadata Drift Audit</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body><h1>Release Metadata Drift Audit</h1>"
    f"<p>Status: <code>{html.escape(status)}</code></p>"
    "<table><thead><tr><th>Component</th><th>Tag</th><th>Release Name</th><th>Channel</th><th>Status</th></tr></thead>"
    f"<tbody>{''.join(rows)}</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
