#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${AO2_PULSE_LOCAL_MIRROR_SOURCE:-$ROOT/target/pulse-next-recommended-tasks}"
DEST="${AO2_PULSE_LOCAL_MIRROR_DEST:-$ROOT/.ao2-local/pulse/latest}"
SUMMARY="$DEST/pulse-local-mirror-summary.json"

if [ ! -d "$SOURCE" ]; then
  echo "pulse mirror source not found: $SOURCE" >&2
  exit 1
fi

rm -rf "$DEST"
mkdir -p "$DEST"

(
  cd "$SOURCE"
  find . -type f -print | while IFS= read -r file; do
    mkdir -p "$DEST/$(dirname "$file")"
    cp "$file" "$DEST/$file"
  done
)

python3 - "$SOURCE" "$DEST" "$SUMMARY" <<'PY'
import json
import subprocess
import sys
from pathlib import Path

source = Path(sys.argv[1])
dest = Path(sys.argv[2])
summary = Path(sys.argv[3])

files = []
for path in sorted(p for p in dest.rglob("*") if p.is_file() and p.name != summary.name):
    rel = path.relative_to(dest).as_posix()
    # Keep this equivalent to `shasum -a 256 <file>` for macOS/Linux parity.
    digest = subprocess.check_output(["shasum", "-a", "256", str(path)], text=True).split()[0]
    files.append({"path": rel, "sha256": digest})

required = {
    "packet.md",
    "board.md",
    "executor-evidence.json",
}
present = {item["path"] for item in files}
eval_loop_files = sorted(path for path in present if path.endswith("pulse-eval-loop.json"))
missing = sorted(required.difference(present))
if not eval_loop_files:
    missing.append("*/pulse-eval-loop.json")
payload = {
    "schema_version": "ao2.pulse-local-mirror.v1",
    "status": "passed" if not missing else "incomplete",
    "source": str(source),
    "destination": str(dest),
    "file_count": len(files),
    "required_files_present": sorted(required.intersection(present)),
    "pulse_eval_loop_files": eval_loop_files,
    "missing_required_files": missing,
    "files": files,
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY

printf "pulse_local_mirror_source=%s\n" "$SOURCE"
printf "pulse_local_mirror_dest=%s\n" "$DEST"
printf "pulse_local_mirror=passed\n"
