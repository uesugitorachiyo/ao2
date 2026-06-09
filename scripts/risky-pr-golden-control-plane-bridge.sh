#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RISKY_PR_CP_BRIDGE_ROOT:-$ROOT/target/risky-pr-golden-control-plane-bridge}"
LATEST_ROOT="$OUT_ROOT/latest"
ARTIFACT_ROOT="${AO2_RISKY_PR_GOLDEN_ARTIFACT_ROOT:-$ROOT/target/risky-pr-golden-ci}"
MANIFEST_PATH="${AO2_RISKY_PR_GOLDEN_ARTIFACT_MANIFEST:-}"
CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
CP_BASE_URL="${AO2_CP_BASE_URL:-}"
API_TOKEN_ENV="${AO2_CP_API_TOKEN_ENV:-AO2_CP_API_TOKEN}"

usage() {
  cat >&2 <<'EOF'
usage: risky-pr-golden-control-plane-bridge.sh [options]

Options:
  --artifact-root <path>       Root containing artifact-manifest.json.
  --manifest <path>            Explicit AO2 risky PR golden artifact manifest.
  --control-plane-root <path>  ao2-control-plane checkout or staging root.
  --cp-base-url <url>          Optional running control-plane base URL to smoke.
  --api-token-env <name>       Token env var name for optional smoke.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --artifact-root)
      ARTIFACT_ROOT="${2:-}"
      if [ -z "$ARTIFACT_ROOT" ]; then
        echo "--artifact-root requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --manifest)
      MANIFEST_PATH="${2:-}"
      if [ -z "$MANIFEST_PATH" ]; then
        echo "--manifest requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --control-plane-root)
      CONTROL_PLANE_ROOT="${2:-}"
      if [ -z "$CONTROL_PLANE_ROOT" ]; then
        echo "--control-plane-root requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --cp-base-url)
      CP_BASE_URL="${2:-}"
      if [ -z "$CP_BASE_URL" ]; then
        echo "--cp-base-url requires a URL" >&2
        exit 2
      fi
      shift 2
      ;;
    --api-token-env)
      API_TOKEN_ENV="${2:-}"
      if [ -z "$API_TOKEN_ENV" ]; then
        echo "--api-token-env requires an environment variable name" >&2
        exit 2
      fi
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [ -z "$MANIFEST_PATH" ]; then
  MANIFEST_PATH="$ARTIFACT_ROOT/artifact-manifest.json"
fi

STABLE_MANIFEST="$LATEST_ROOT/artifact-manifest.json"
SUMMARY="$LATEST_ROOT/summary.json"
ENV_FILE="$LATEST_ROOT/control-plane.env"
SMOKE_JSON="$LATEST_ROOT/control-plane-observer.json"
SMOKE_HTML="$LATEST_ROOT/control-plane-observer.html"
CP_MIRROR_ROOT="$CONTROL_PLANE_ROOT/target/risky-pr-golden-control-plane-bridge"
CP_MIRROR_MANIFEST="$CP_MIRROR_ROOT/artifact-manifest.json"

rm -rf "$LATEST_ROOT"
mkdir -p "$LATEST_ROOT" "$CP_MIRROR_ROOT"

python3 - "$MANIFEST_PATH" "$ARTIFACT_ROOT" "$STABLE_MANIFEST" "$CP_MIRROR_MANIFEST" "$ENV_FILE" "$SUMMARY" "$CP_BASE_URL" "$API_TOKEN_ENV" <<'PY'
import hashlib
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

manifest_path = Path(sys.argv[1]).resolve()
artifact_root_arg = Path(sys.argv[2]).resolve()
stable_manifest = Path(sys.argv[3]).resolve()
mirror_manifest = Path(sys.argv[4]).resolve()
env_file = Path(sys.argv[5]).resolve()
summary_path = Path(sys.argv[6]).resolve()
cp_base_url = sys.argv[7].rstrip("/")
api_token_env = sys.argv[8]

MANIFEST_SCHEMA = "ao2.risky-pr-golden-artifact-manifest.v1"
BRIDGE_SCHEMA = "ao2.risky-pr-golden-control-plane-bridge.v1"
OBSERVER_SCHEMA = "ao2.cp-risky-pr-golden-artifact-manifest-observer.v1"
MANIFEST_ENV = "AO2_CP_RISKY_PR_GOLDEN_ARTIFACT_MANIFEST"

def fail(message: str) -> None:
    raise SystemExit(message)

def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

if not manifest_path.is_file():
    fail(f"missing risky PR golden artifact manifest: {manifest_path}")

manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("schema_version") != MANIFEST_SCHEMA:
    fail(f"unexpected artifact manifest schema: {manifest.get('schema_version')}")
if manifest.get("status") != "indexed":
    fail(f"artifact manifest status is not indexed: {manifest.get('status')}")

declared_artifacts = manifest.get("artifacts")
if not isinstance(declared_artifacts, list):
    fail("artifact manifest artifacts must be a list")
if manifest.get("artifact_count") != len(declared_artifacts):
    fail("artifact manifest artifact_count does not match artifacts length")

manifest_artifact_root = manifest.get("artifact_root") or "."
if manifest_artifact_root == ".":
    artifact_root = manifest_path.parent
else:
    artifact_root = (manifest_path.parent / manifest_artifact_root).resolve()
if not artifact_root.exists():
    artifact_root = artifact_root_arg

validated_artifacts = []
for artifact in declared_artifacts:
    relative_path = artifact.get("relative_path") or artifact.get("path")
    if not relative_path or Path(relative_path).is_absolute() or ".." in Path(relative_path).parts:
        fail(f"unsafe artifact relative path: {relative_path}")
    path = artifact_root / relative_path
    if not path.is_file():
        fail(f"artifact manifest references missing file: {relative_path}")
    observed_sha = sha256_file(path)
    if artifact.get("sha256") != observed_sha:
        fail(f"artifact sha256 mismatch: {relative_path}")
    if artifact.get("size_bytes") != path.stat().st_size:
        fail(f"artifact size mismatch: {relative_path}")
    validated_artifacts.append(
        {
            "relative_path": relative_path,
            "sha256": observed_sha,
            "size_bytes": path.stat().st_size,
            "schema_version": artifact.get("schema_version"),
        }
    )

stable_manifest.parent.mkdir(parents=True, exist_ok=True)
mirror_manifest.parent.mkdir(parents=True, exist_ok=True)
shutil.copyfile(manifest_path, stable_manifest)
shutil.copyfile(manifest_path, mirror_manifest)
env_file.write_text(f"{MANIFEST_ENV}={stable_manifest}\n", encoding="utf-8")

summary = {
    "schema_version": BRIDGE_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "source_manifest": str(manifest_path),
    "artifact_root": str(artifact_root),
    "manifest": {
        "schema_version": MANIFEST_SCHEMA,
        "run_id": manifest.get("run_id"),
        "artifact_count": len(validated_artifacts),
        "artifacts": validated_artifacts,
    },
    "control_plane": {
        "observer_schema": OBSERVER_SCHEMA,
        "configured_env": MANIFEST_ENV,
        "stable_manifest": str(stable_manifest),
        "mirror_manifest": str(mirror_manifest),
        "env_file": str(env_file),
        "role": "read-only-observer",
        "json_endpoint": "/api/v1/risky-pr/golden/artifact-manifest.json",
        "html_endpoint": "/api/v1/risky-pr/golden/artifact-manifest",
        "base_url": cp_base_url or None,
        "api_token_env": api_token_env,
        "credential_material_included": False,
        "credential_material_in_urls": False,
        "smoke": "pending" if cp_base_url else "not_run",
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_approves_release": False,
        "mutates_ao2_artifacts": False,
        "mutates_observer_storage": False,
    },
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"stable_manifest={stable_manifest}")
print(f"control_plane_env_file={env_file}")
print("status=passed")
PY

if [ -n "$CP_BASE_URL" ]; then
  TOKEN_VALUE="${!API_TOKEN_ENV:-}"
  if [ -z "$TOKEN_VALUE" ]; then
    echo "--cp-base-url requires token material in env var $API_TOKEN_ENV" >&2
    exit 2
  fi

  env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
    curl -fsS \
      -H "Authorization: Bearer $TOKEN_VALUE" \
      "$CP_BASE_URL/api/v1/risky-pr/golden/artifact-manifest.json" \
      -o "$SMOKE_JSON"
  env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
    curl -fsS \
      -H "Authorization: Bearer $TOKEN_VALUE" \
      "$CP_BASE_URL/api/v1/risky-pr/golden/artifact-manifest" \
      -o "$SMOKE_HTML"

  python3 - "$SUMMARY" "$SMOKE_JSON" "$SMOKE_HTML" "$TOKEN_VALUE" <<'PY'
import json
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
json_path = Path(sys.argv[2])
html_path = Path(sys.argv[3])
token_value = sys.argv[4]

summary = json.loads(summary_path.read_text(encoding="utf-8"))
observer = json.loads(json_path.read_text(encoding="utf-8"))
html = html_path.read_text(encoding="utf-8", errors="replace")

if observer.get("schema_version") != "ao2.cp-risky-pr-golden-artifact-manifest-observer.v1":
    raise SystemExit("control-plane observer schema mismatch")
if observer.get("control_plane_role") != "read-only-observer":
    raise SystemExit("control-plane observer role mismatch")
if observer.get("control_plane_approves_release") is not False:
    raise SystemExit("control-plane observer approval boundary mismatch")
if observer.get("mutates_ao_artifacts") is not False:
    raise SystemExit("control-plane observer artifact mutation boundary mismatch")
auth = observer.get("auth") or {}
if auth.get("credential_material_included") is not False:
    raise SystemExit("control-plane observer includes credential material")
if auth.get("credential_material_in_urls") is not False:
    raise SystemExit("control-plane observer includes credential-bearing URLs")
if token_value and token_value in json_path.read_text(encoding="utf-8"):
    raise SystemExit("token leaked into JSON observer response")
if token_value and token_value in html:
    raise SystemExit("token leaked into HTML observer response")
if "Risky PR Golden Artifact Manifest" not in html:
    raise SystemExit("HTML observer did not render risky PR manifest view")

summary["control_plane"]["smoke"] = "passed"
summary["control_plane"]["json_smoke_response"] = str(json_path)
summary["control_plane"]["html_smoke_response"] = str(html_path)
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print("control_plane_smoke=passed")
PY
fi
