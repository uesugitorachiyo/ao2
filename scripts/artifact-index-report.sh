#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_DIR="${AO2_ARTIFACT_INDEX_ROOT:-$ROOT/target/artifact-index/latest}"
SUMMARY="$OUT_DIR/artifact-index.json"
REPORT="$OUT_DIR/report.md"
DASHBOARD="$OUT_DIR/dashboard.html"

mkdir -p "$OUT_DIR"

python3 - "$ROOT" "$CP_ROOT" "$OUT_DIR" "$SUMMARY" "$REPORT" "$DASHBOARD" <<'PY'
import hashlib
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
cp_root = Path(sys.argv[2]).resolve()
out_dir = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
report_path = Path(sys.argv[5]).resolve()
dashboard_path = Path(sys.argv[6]).resolve()
stale_after_seconds = 24 * 60 * 60

scan_specs = [
    ("ao2", root, ["target/ci-artifacts", "target/release-readiness-regression-gate", "target/release-readiness-ci", "target/release-evidence-closure", "target/phase1-promotion-golden", "target/pulse-real-execute-containment", ".ao2-local/pulse/latest"]),
    ("ao2-control-plane", cp_root, ["target/ci-artifacts", "target/dr-restore-drill"]),
]

def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def schema_version(path: Path):
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
        latest_mtime = max(((repo_root / file["path"]).stat().st_mtime for file in files), default=None)
        latest_generated_at_utc = (
            datetime.fromtimestamp(latest_mtime, timezone.utc).isoformat().replace("+00:00", "Z")
            if latest_mtime is not None else None
        )
        age_seconds = (
            int(datetime.now(timezone.utc).timestamp() - latest_mtime)
            if latest_mtime is not None else None
        )
        health = "missing" if not scan_root.exists() else ("empty" if not files else ("stale" if age_seconds is not None and age_seconds > stale_after_seconds else "healthy"))
        total_files += len(files)
        bundles.append({
            "root": rel_root,
            "exists": scan_root.exists(),
            "file_count": len(files),
            "health": health,
            "latest_generated_at_utc": latest_generated_at_utc,
            "age_seconds": age_seconds,
            "stale_after_seconds": stale_after_seconds,
            "files": files,
        })
    repositories.append({
        "name": repo_name,
        "root": str(repo_root),
        "bundles": bundles,
    })

payload = {
    "schema_version": "ao2.artifact-index-report.v1",
    "dashboard_schema_version": "ao2.artifact-evidence-dashboard.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "artifact_root": str(out_dir),
    "total_files": total_files,
    "repositories": repositories,
    "dashboard": {
        "schema_version": "ao2.artifact-evidence-dashboard.v1",
        "path": str(dashboard_path),
        "stale_after_seconds": stale_after_seconds,
    },
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
        lines.append(f"- `{bundle['root']}`: {bundle['health']}, {bundle['file_count']} files")
    lines.append("")
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

rows = []
for repo in repositories:
    for bundle in repo["bundles"]:
        rows.append(
            "<tr>"
            f"<td>{html.escape(repo['name'])}</td>"
            f"<td><code>{html.escape(bundle['root'])}</code></td>"
            f"<td>{html.escape(bundle['health'])}</td>"
            f"<td>{bundle['file_count']}</td>"
            f"<td>{html.escape(str(bundle['latest_generated_at_utc']))}</td>"
            "</tr>"
        )
dashboard_html = """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>AO2 Artifact Evidence Dashboard</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 32px; color: #172026; }}
    table {{ border-collapse: collapse; width: 100%; }}
    th, td {{ border: 1px solid #d7dde2; padding: 8px; text-align: left; }}
    th {{ background: #f3f6f8; }}
    code {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }}
  </style>
</head>
<body>
  <h1>AO2 Artifact Evidence Dashboard</h1>
  <p>Schema: <code>ao2.artifact-evidence-dashboard.v1</code></p>
  <p>Status: <code>{status}</code>; total files: <code>{total_files}</code>; stale after: <code>{stale_after_seconds}</code> seconds.</p>
  <table>
    <thead><tr><th>Repository</th><th>Evidence Root</th><th>Health</th><th>Files</th><th>Latest Generated</th></tr></thead>
    <tbody>
      {rows}
    </tbody>
  </table>
</body>
</html>
""".format(
    status=html.escape(payload["status"]),
    total_files=total_files,
    stale_after_seconds=stale_after_seconds,
    rows="\n      ".join(rows),
)
dashboard_path.write_text(dashboard_html, encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"dashboard={dashboard_path}")
print("artifact_index=passed")
PY
