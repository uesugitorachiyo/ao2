#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INDEX_JSON="${AO2_ARTIFACT_HEALTH_INDEX:-$ROOT/target/artifact-index/latest/artifact-index.json}"
OUT_DIR="${AO2_ARTIFACT_HEALTH_ROOT:-$ROOT/target/artifact-health/latest}"
SUMMARY="$OUT_DIR/summary.json"
REPORT="$OUT_DIR/report.md"
REQUIRED_ROOTS="${AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS:-}"
ALLOWED_MISSING_ROOTS="${AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS:-}"
FAIL_ON_ATTENTION="${AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION:-0}"
STALE_AFTER_SECONDS="${AO2_ARTIFACT_HEALTH_STALE_AFTER_SECONDS:-}"

mkdir -p "$OUT_DIR"

python3 - "$ROOT" "$INDEX_JSON" "$SUMMARY" "$REPORT" "$REQUIRED_ROOTS" "$ALLOWED_MISSING_ROOTS" "$FAIL_ON_ATTENTION" "$STALE_AFTER_SECONDS" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
index_path = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
report_path = Path(sys.argv[4]).resolve()
required_roots = [item for item in sys.argv[5].split() if item]
allowed_missing_roots = set(item for item in sys.argv[6].split() if item)
fail_on_attention = sys.argv[7].lower() in {"1", "true", "yes"}
stale_threshold_override_seconds = int(sys.argv[8]) if sys.argv[8] else None

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
    "allowed_missing_bundles": [],
    "policy": {
        "required_roots": required_roots,
        "allowed_missing_roots": sorted(allowed_missing_roots),
        "fail_on_attention": fail_on_attention,
        "stale_threshold_override_seconds": stale_threshold_override_seconds,
    },
    "policy_violations": [],
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
    observed_roots = set()
    for repo in index.get("repositories", []):
        repo_name = str(repo.get("name", "unknown"))
        for bundle in repo.get("bundles", []):
            bundle_root = str(bundle.get("root", ""))
            root_key = f"{repo_name}/{bundle_root}"
            observed_roots.add(bundle_root)
            observed_roots.add(root_key)
            age_seconds = bundle.get("age_seconds")
            stale_after_seconds = (
                stale_threshold_override_seconds
                if stale_threshold_override_seconds is not None
                else bundle.get("stale_after_seconds")
            )
            health = str(bundle.get("health", "unknown"))
            if (
                health == "healthy"
                and age_seconds is not None
                and stale_after_seconds is not None
                and int(age_seconds) > int(stale_after_seconds)
            ):
                health = "stale"
            item = {
                "repository": repo_name,
                "root": bundle_root,
                "root_key": root_key,
                "health": health,
                "file_count": int(bundle.get("file_count", 0) or 0),
                "latest_generated_at_utc": bundle.get("latest_generated_at_utc"),
                "age_seconds": age_seconds,
                "stale_after_seconds": stale_after_seconds,
            }
            if health == "healthy":
                payload["healthy_bundles"].append(item)
            elif health == "missing":
                if bundle_root in allowed_missing_roots or root_key in allowed_missing_roots:
                    payload["allowed_missing_bundles"].append(item)
                else:
                    payload["missing_bundles"].append(item)
            elif health == "empty":
                payload["empty_bundles"].append(item)
            elif health == "stale":
                payload["stale_bundles"].append(item)
            else:
                payload["failing_bundles"].append(item)
    for required in required_roots:
        if required not in observed_roots:
            payload["policy_violations"].append(
                {
                    "type": "required_root_missing",
                    "root": required,
                }
            )

payload["attention_required_count"] = (
    len(payload["failing_bundles"])
    + len(payload["missing_bundles"])
    + len(payload["stale_bundles"])
    + len(payload["empty_bundles"])
    + len(payload["policy_violations"])
)
if payload["status"] == "passed" and payload["attention_required_count"]:
    payload["status"] = "attention_required"
if payload["status"] == "attention_required" and fail_on_attention:
    payload["status"] = "failed"
    payload["reason"] = "attention_required_policy_failed"

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# AO2 Artifact Evidence Health",
    "",
    f"- Schema: `{payload['schema_version']}`",
    f"- Status: `{payload['status']}`",
    f"- Artifact index: `{index_path.relative_to(root) if index_path.is_relative_to(root) else index_path}`",
    f"- Attention required: `{payload['attention_required_count']}`",
    f"- Fail on attention: `{fail_on_attention}`",
    "",
]
for key in ["policy_violations", "failing_bundles", "missing_bundles", "stale_bundles", "empty_bundles", "allowed_missing_bundles", "healthy_bundles"]:
    lines.append(f"## {key}")
    items = payload[key]
    if not items:
        lines.append("- none")
    for item in items:
        if key == "policy_violations":
            lines.append(f"- `{item['type']}`: `{item['root']}`")
        else:
            lines.append(f"- `{item['repository']}/{item['root']}`: {item['health']}, {item['file_count']} files")
    lines.append("")
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"status={payload['status']}")
if payload["status"] == "failed":
    raise SystemExit(1)
PY
