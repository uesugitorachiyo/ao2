#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_ARTIFACT_CONSUMER_ROOT:-$ROOT/target/release-artifact-consumer-smoke/$(date -u +%Y%m%dT%H%M%SZ)}"
SUMMARY="$OUT_ROOT/summary.json"
DRY_RUN=0
REPOS="${AO2_RELEASE_ARTIFACT_CONSUMER_REPOS:-uesugitorachiyo/ao2 uesugitorachiyo/ao2-control-plane}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    *)
      echo "usage: $0 [--dry-run]" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$OUT_ROOT/clean-workspace"

if [ "$DRY_RUN" = "0" ]; then
  for repo in $REPOS; do
    repo_dir="$OUT_ROOT/clean-workspace/$(printf "%s" "$repo" | tr '/' '-')"
    mkdir -p "$repo_dir"
    run_id="$(gh run list --repo "$repo" --workflow CI --branch main --status success --limit 1 --json databaseId --jq '.[0].databaseId')"
    if [ -z "$run_id" ] || [ "$run_id" = "null" ]; then
      echo "no successful CI run found for $repo" >&2
      exit 1
    fi
    gh run download "$run_id" --repo "$repo" --dir "$repo_dir"
  done
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$DRY_RUN" "$REPOS" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
dry_run = sys.argv[3] == "1"
repos = sys.argv[4].split()
clean_workspace = out_root / "clean-workspace"

def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

files = []
schema_versions = []
for path in sorted(p for p in clean_workspace.rglob("*") if p.is_file()):
    item = {
        "path": path.relative_to(out_root).as_posix(),
        "bytes": path.stat().st_size,
        "sha256": sha256(path),
        "schema_version": None,
    }
    if path.suffix == ".json":
        try:
            parsed = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            parsed = None
        if isinstance(parsed, dict) and isinstance(parsed.get("schema_version"), str):
            item["schema_version"] = parsed["schema_version"]
            schema_versions.append(parsed["schema_version"])
    files.append(item)

payload = {
    "schema_version": "ao2.release-artifact-consumer-smoke.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "dry_run" if dry_run else ("passed" if files else "failed"),
    "dry_run": dry_run,
    "artifact_root": str(out_root),
    "clean_workspace": str(clean_workspace),
    "repos": repos,
    "download_command": "gh run download",
    "schema_versions": sorted(set(schema_versions)),
    "files": files,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "source": "github_actions_artifacts",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] == "failed":
    raise SystemExit(1)
PY
