#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INDEX_JSON="${AO2_ARTIFACT_HEALTH_INDEX:-$ROOT/target/artifact-index/latest/artifact-index.json}"
OUT_DIR="${AO2_ARTIFACT_HEALTH_ROOT:-$ROOT/target/artifact-health/latest}"
SUMMARY="$OUT_DIR/summary.json"
REPORT="$OUT_DIR/report.md"

mkdir -p "$OUT_DIR"

python3 - "$ROOT" "$INDEX_JSON" "$SUMMARY" "$REPORT" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
index_path = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
report_path = Path(sys.argv[4]).resolve()

generated_at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
payload = {
    "schema_version": "ao2.artifact-evidence-health.v1",
    "generated_at_utc": generated_at,
    "status": "passed",
    "artifact_index": str(index_path),
    "artifact_health_root": str(summary_path.parent),
    "failing_bundles": [],
    "missing_bundles": [],
    "stale_bundles": [],
    "empty_bundles": [],
    "healthy_bundles": [],
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
    },
}

if not index_path.is_file():
    payload["status"] = "failed"
    payload["reason"] = "artifact-index.json not found"
else:
    index = json.loads(index_path.read_text(encoding="utf-8"))
    if index.get("status") != "passed":
        payload["status"] = "failed"
        payload["reason"] = f"artifact index status was {index.get('status')!r}"
    for repo in index.get("repositories", []):
        repo_name = str(repo.get("name", "unknown"))
        for bundle in repo.get("bundles", []):
            item = {
                "repository": repo_name,
                "root": str(bundle.get("root", "")),
                "health": str(bundle.get("health", "unknown")),
                "file_count": int(bundle.get("file_count", 0) or 0),
                "latest_generated_at_utc": bundle.get("latest_generated_at_utc"),
                "age_seconds": bundle.get("age_seconds"),
                "stale_after_seconds": bundle.get("stale_after_seconds"),
            }
            health = item["health"]
            if health == "healthy":
                payload["healthy_bundles"].append(item)
            elif health == "missing":
                payload["missing_bundles"].append(item)
            elif health == "empty":
                payload["empty_bundles"].append(item)
            elif health == "stale":
                payload["stale_bundles"].append(item)
            else:
                payload["failing_bundles"].append(item)

payload["attention_required_count"] = (
    len(payload["failing_bundles"])
    + len(payload["missing_bundles"])
    + len(payload["stale_bundles"])
    + len(payload["empty_bundles"])
)
if payload["status"] == "passed" and payload["attention_required_count"]:
    payload["status"] = "attention_required"

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# AO2 Artifact Evidence Health",
    "",
    f"- Schema: `{payload['schema_version']}`",
    f"- Status: `{payload['status']}`",
    f"- Artifact index: `{index_path.relative_to(root) if index_path.is_relative_to(root) else index_path}`",
    f"- Attention required: `{payload['attention_required_count']}`",
    "",
]
for key in ["failing_bundles", "missing_bundles", "stale_bundles", "empty_bundles", "healthy_bundles"]:
    lines.append(f"## {key}")
    items = payload[key]
    if not items:
        lines.append("- none")
    for item in items:
        lines.append(f"- `{item['repository']}/{item['root']}`: {item['health']}, {item['file_count']} files")
    lines.append("")
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"status={payload['status']}")
if payload["status"] == "failed":
    raise SystemExit(1)
PY
