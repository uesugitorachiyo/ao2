#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${AO2_RELEASE_TRAIN_MANIFEST:-$ROOT/docs/release/release-train.json}"
TRAIN="${1:-${AO2_RELEASE_TRAIN:-stable}}"

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
else
  echo "missing Python interpreter: python3 or python required" >&2
  exit 1
fi

"$python_bin" - "$MANIFEST" "$TRAIN" <<'PY'
import json
import shlex
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1]).resolve()
train_name = sys.argv[2]
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

if manifest.get("schema_version") != "ao2.release-train-manifest.v1":
    raise SystemExit(f"unexpected release train manifest schema: {manifest.get('schema_version')}")
if train_name not in manifest or not isinstance(manifest[train_name], dict):
    raise SystemExit(f"unknown release train: {train_name}")

train = manifest[train_name]
values = {
    "AO2_RELEASE_TRAIN_MANIFEST": str(manifest_path),
    "AO2_RELEASE_TRAIN_MANIFEST_SCHEMA": manifest["schema_version"],
    "AO2_RELEASE_TRAIN_NAME": train_name,
    "AO2_RELEASE_TRAIN_AO2_TAG": train["ao2"]["tag"],
    "AO2_RELEASE_TRAIN_AO2_VERSION": train["ao2"]["version"],
    "AO2_RELEASE_TRAIN_CP_TAG": train["ao2_control_plane"]["tag"],
    "AO2_RELEASE_TRAIN_CP_VERSION": train["ao2_control_plane"]["version"],
    "AO2_RELEASE_TRAIN_PROMOTION_CONFIRM": train["promotion_confirm"],
    "AO2_RELEASE_TRAIN_PUBLIC_OPERATOR_CONFIRM": train["public_operator_confirm"],
}
for key, value in values.items():
    print(f"{key}={shlex.quote(str(value))}")
PY
