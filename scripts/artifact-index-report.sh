#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_DIR="${AO2_ARTIFACT_INDEX_ROOT:-$ROOT/target/artifact-index/latest}"
SUMMARY="$OUT_DIR/artifact-index.json"
REPORT="$OUT_DIR/report.md"

mkdir -p "$OUT_DIR"

python3 - "$ROOT" "$CP_ROOT" "$OUT_DIR" "$SUMMARY" "$REPORT" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
cp_root = Path(sys.argv[2]).resolve()
out_dir = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
report_path = Path(sys.argv[5]).resolve()

scan_specs = [
    ("ao2", root, ["target/ci-artifacts", "target/release-readiness-regression-gate", "target/release-readiness-ci", ".ao2-local/pulse/latest"]),
    ("ao2-control-plane", cp_root, ["target/ci-artifacts", "target/dr-restore-drill"]),
]

def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def schema_version(path: Path) -> str | None:
    if path.suffix != ".json":
        return None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    if isinstance(parsed, dict) and isinstance(parsed.get("schema_version"), str):
        return parsed["schema_version"]
    return None

repositories = []
total_files = 0
for repo_name, repo_root, rel_roots in scan_specs:
    bundles = []
    for rel_root in rel_roots:
        scan_root = repo_root / rel_root
        files = []
        if scan_root.exists():
            for path in sorted(p for p in scan_root.rglob("*") if p.is_file()):
                files.append({
                    "path": path.relative_to(repo_root).as_posix(),
                    "bytes": path.stat().st_size,
                    "sha256": sha256(path),
                    "schema_version": schema_version(path),
                })
        total_files += len(files)
        bundles.append({
            "root": rel_root,
            "exists": scan_root.exists(),
            "file_count": len(files),
            "files": files,
        })
    repositories.append({
        "name": repo_name,
        "root": str(repo_root),
        "bundles": bundles,
    })

payload = {
    "schema_version": "ao2.artifact-index-report.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "artifact_root": str(out_dir),
    "total_files": total_files,
    "repositories": repositories,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# AO2 Artifact Index",
    "",
    f"- Schema: `{payload['schema_version']}`",
    f"- Status: `{payload['status']}`",
    f"- Total files: `{total_files}`",
    "",
]
for repo in repositories:
    lines.append(f"## {repo['name']}")
    for bundle in repo["bundles"]:
        lines.append(f"- `{bundle['root']}`: {'present' if bundle['exists'] else 'missing'}, {bundle['file_count']} files")
    lines.append("")
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print("artifact_index=passed")
PY
