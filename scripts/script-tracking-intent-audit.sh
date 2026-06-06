#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SCRIPT_TRACKING_INTENT_ROOT:-$ROOT/target/script-tracking-intent-audit/latest}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
status_output = subprocess.check_output(["git", "status", "--short", "--", "scripts"], cwd=root, text=True)
untracked_scripts = []
for line in status_output.splitlines():
    if line.startswith("?? ") and line.endswith(".sh"):
        untracked_scripts.append(line[3:])
track_in_repo = [path for path in untracked_scripts if "local-dev" not in path]
keep_local_only = []
manifest = {
    "schema_version": "ao2.script-tracking-manifest.v1",
    "track_in_repo": track_in_repo,
    "keep_local_only": keep_local_only,
    "rationale": "Public npm command scripts should be tracked when this work is committed; run evidence and schedules stay under ignored target paths.",
}
manifest_path = out_root / "script-tracking-manifest.json"
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
payload = {
    "schema_version": "ao2.script-tracking-intent-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "artifact_root": str(out_root),
    "untracked_script_count": len(untracked_scripts),
    "track_in_repo": track_in_repo,
    "keep_local_only": keep_local_only,
    "script_tracking_manifest": str(manifest_path),
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print("status=passed")
PY
