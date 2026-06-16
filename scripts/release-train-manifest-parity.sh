#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_RELEASE_TRAIN_MANIFEST_PARITY_ROOT:-$ROOT/target/release-train-manifest-parity/latest}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --control-plane-root)
      CONTROL_PLANE_ROOT="$2"
      shift 2
      ;;
    --out-root)
      OUT_ROOT="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
else
  echo "missing Python interpreter: python3 or python required" >&2
  exit 1
fi

AO2_MANIFEST="$ROOT/docs/release/release-train.json"
CP_MANIFEST="$CONTROL_PLANE_ROOT/docs/release/release-train.json"

"$python_bin" - "$AO2_MANIFEST" "$CP_MANIFEST" "$OUT_ROOT" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

ao2_manifest_path = Path(sys.argv[1]).resolve()
cp_manifest_path = Path(sys.argv[2]).resolve()
out_root = Path(sys.argv[3]).resolve()
summary_path = out_root / "summary.json"

failures = []
for path in (ao2_manifest_path, cp_manifest_path):
    if not path.is_file():
        failures.append(f"missing release train manifest: {path}")

ao2_manifest = {}
cp_manifest = {}
if ao2_manifest_path.is_file():
    ao2_manifest = json.loads(ao2_manifest_path.read_text(encoding="utf-8"))
if cp_manifest_path.is_file():
    cp_manifest = json.loads(cp_manifest_path.read_text(encoding="utf-8"))

expected_schema = "ao2.release-train-manifest.v1"
schema_aligned = (
    ao2_manifest.get("schema_version") == expected_schema
    and cp_manifest.get("schema_version") == expected_schema
)
if not schema_aligned:
    failures.append("manifest schema did not align to ao2.release-train-manifest.v1")

target_keys = ("stable", "next_patch")
target_aligned = all(ao2_manifest.get(key) == cp_manifest.get(key) for key in target_keys)
if not target_aligned:
    failures.append("stable or next_patch release train targets diverged")

def canonical_digest(payload: dict) -> str:
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(canonical).hexdigest()

ao2_raw = ao2_manifest_path.read_bytes() if ao2_manifest_path.is_file() else b""
cp_raw = cp_manifest_path.read_bytes() if cp_manifest_path.is_file() else b""
byte_identical = ao2_raw == cp_raw
if not byte_identical:
    failures.append("release train manifest files are not byte-identical")

raw_sha256 = {
    "ao2": hashlib.sha256(ao2_raw).hexdigest(),
    "ao2_control_plane": hashlib.sha256(cp_raw).hexdigest(),
}
canonical_sha256 = {
    "ao2": canonical_digest(ao2_manifest) if ao2_manifest else "",
    "ao2_control_plane": canonical_digest(cp_manifest) if cp_manifest else "",
}

status = "passed" if not failures else "failed"
summary = {
    "schema_version": "ao2.release-train-manifest-parity.v1",
    "status": status,
    "ao2_manifest": str(ao2_manifest_path),
    "ao2_control_plane_manifest": str(cp_manifest_path),
    "byte_identical": byte_identical,
    "schema_aligned": schema_aligned,
    "target_aligned": target_aligned,
    "raw_sha256": raw_sha256,
    "canonical_sha256": canonical_sha256,
    "stable": ao2_manifest.get("stable", {}),
    "next_patch": ao2_manifest.get("next_patch", {}),
    "failures": failures,
    "trust_boundary": {
        "local_only": True,
        "mutates_ao_artifacts": False,
        "mutates_control_plane_artifacts": False,
        "mutates_github_releases": False,
        "stores_credentials": False,
    },
}
out_root.mkdir(parents=True, exist_ok=True)
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
if status == "passed":
    print("release_train_manifest_parity=passed")
else:
    print("release_train_manifest_parity=failed")
print(f"release_train_manifest_parity_summary={summary_path}")
if status != "passed":
    raise SystemExit(1)
PY
