#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_ARTIFACT_CONSUMER_ROOT:-$ROOT/target/release-artifact-consumer-smoke/$(date -u +%Y%m%dT%H%M%SZ)}"
SUMMARY="$OUT_ROOT/summary.json"
DOWNLOAD_TIMEOUT_SECONDS="${AO2_RELEASE_ARTIFACT_DOWNLOAD_TIMEOUT_SECONDS:-300}"
DOWNLOAD_FAILURES="$OUT_ROOT/download-failures.jsonl"
DOWNLOAD_LOG_DIR="$OUT_ROOT/download-logs"
DRY_RUN=0
FIXTURE_DIR=""
REPOS="${AO2_RELEASE_ARTIFACT_CONSUMER_REPOS:-uesugitorachiyo/ao2 uesugitorachiyo/ao2-control-plane}"
REQUIRED_ARTIFACTS=""
REQUIRED_SCHEMAS=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --fixture-dir)
      FIXTURE_DIR="${2:-}"
      if [ -z "$FIXTURE_DIR" ]; then
        echo "--fixture-dir requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --require-artifact)
      REQUIRED_ARTIFACTS="${REQUIRED_ARTIFACTS}${REQUIRED_ARTIFACTS:+ }${2:-}"
      if [ -z "${2:-}" ]; then
        echo "--require-artifact requires a name fragment" >&2
        exit 2
      fi
      shift 2
      ;;
    --require-schema)
      REQUIRED_SCHEMAS="${REQUIRED_SCHEMAS}${REQUIRED_SCHEMAS:+ }${2:-}"
      if [ -z "${2:-}" ]; then
        echo "--require-schema requires a schema_version" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--dry-run] [--fixture-dir <path>] [--require-artifact <name-fragment>] [--require-schema <schema_version>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$OUT_ROOT/clean-workspace"
mkdir -p "$OUT_ROOT/clean-workspace" "$DOWNLOAD_LOG_DIR"
: >"$DOWNLOAD_FAILURES"

run_with_timeout() {
  local timeout_seconds="$1"
  shift
  python3 - "$timeout_seconds" "$@" <<'PY'
import subprocess
import sys

timeout_seconds = float(sys.argv[1])
cmd = sys.argv[2:]
try:
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=timeout_seconds,
        check=False,
    )
except subprocess.TimeoutExpired as exc:
    if exc.stdout:
        sys.stdout.write(exc.stdout if isinstance(exc.stdout, str) else exc.stdout.decode(errors="replace"))
    if exc.stderr:
        sys.stderr.write(exc.stderr if isinstance(exc.stderr, str) else exc.stderr.decode(errors="replace"))
    sys.stderr.write(f"command timed out after {int(timeout_seconds)}s: {' '.join(cmd)}\n")
    raise SystemExit(124)

sys.stdout.write(result.stdout)
sys.stderr.write(result.stderr)
raise SystemExit(result.returncode)
PY
}

record_download_failure() {
  local repo="$1"
  local phase="$2"
  local exit_code="$3"
  local log="$4"
  python3 - "$DOWNLOAD_FAILURES" "$repo" "$phase" "$exit_code" "$log" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
exit_code = int(sys.argv[4])
item = {
    "repo": sys.argv[2],
    "phase": sys.argv[3],
    "exit_code": exit_code,
    "timed_out": exit_code == 124,
    "log": sys.argv[5],
}
with path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(item, sort_keys=True) + "\n")
PY
}

download_required_artifacts() {
  local run_id="$1"
  local repo="$2"
  local repo_dir="$3"
  local download_log="$4"
  local downloaded=0
  : >"$download_log"
  if [ -z "$REQUIRED_ARTIFACTS" ]; then
    run_with_timeout "$DOWNLOAD_TIMEOUT_SECONDS" \
      gh run download "$run_id" --repo "$repo" --dir "$repo_dir" \
      >"$download_log" 2>&1
    return "$?"
  fi
  for artifact in $REQUIRED_ARTIFACTS; do
    set +e
    run_with_timeout "$DOWNLOAD_TIMEOUT_SECONDS" \
      gh run download "$run_id" --repo "$repo" --pattern "$artifact*" --dir "$repo_dir" \
      >>"$download_log" 2>&1
    local artifact_code=$?
    set -e
    if [ "$artifact_code" -eq 0 ]; then
      downloaded=1
    else
      printf "focused artifact pattern not present in %s: %s* exit_code=%s\n" \
        "$repo" "$artifact" "$artifact_code" >>"$download_log"
    fi
  done
  if [ "$downloaded" = "1" ]; then
    return 0
  fi
  return 0
}

if [ -n "$FIXTURE_DIR" ]; then
  if [ ! -d "$FIXTURE_DIR" ]; then
    echo "fixture dir not found: $FIXTURE_DIR" >&2
    exit 1
  fi
  cp -R "$FIXTURE_DIR"/. "$OUT_ROOT/clean-workspace/"
elif [ "$DRY_RUN" = "0" ]; then
  for repo in $REPOS; do
    repo_dir="$OUT_ROOT/clean-workspace/$(printf "%s" "$repo" | tr '/' '-')"
    mkdir -p "$repo_dir"
    list_log="$DOWNLOAD_LOG_DIR/$(printf "%s" "$repo" | tr '/' '-')-run-list.log"
    set +e
    run_id="$(run_with_timeout "$DOWNLOAD_TIMEOUT_SECONDS" gh run list --repo "$repo" --workflow CI --branch main --status success --limit 1 --json databaseId --jq '.[0].databaseId' 2>"$list_log")"
    list_code=$?
    set -e
    if [ "$list_code" -ne 0 ]; then
      record_download_failure "$repo" "gh_run_list" "$list_code" "$list_log"
      break
    fi
    if [ -z "$run_id" ] || [ "$run_id" = "null" ]; then
      echo "no successful CI run found for $repo" >&2
      record_download_failure "$repo" "gh_run_list" 1 "$list_log"
      break
    fi
    download_log="$DOWNLOAD_LOG_DIR/$(printf "%s" "$repo" | tr '/' '-')-run-download.log"
    set +e
    download_required_artifacts "$run_id" "$repo" "$repo_dir" "$download_log"
    download_code=$?
    set -e
    if [ "$download_code" -ne 0 ]; then
      record_download_failure "$repo" "gh_run_download" "$download_code" "$download_log"
      break
    fi
  done
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$DRY_RUN" "$REPOS" "$FIXTURE_DIR" "$REQUIRED_ARTIFACTS" "$REQUIRED_SCHEMAS" "$DOWNLOAD_TIMEOUT_SECONDS" "$DOWNLOAD_FAILURES" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
dry_run = sys.argv[3] == "1"
repos = sys.argv[4].split()
fixture_dir = sys.argv[5]
required_artifacts = [item for item in sys.argv[6].split() if item]
required_schemas = [item for item in sys.argv[7].split() if item]
download_timeout_seconds = int(sys.argv[8])
download_failures_path = Path(sys.argv[9])
download_failures = [
    json.loads(line)
    for line in download_failures_path.read_text(encoding="utf-8").splitlines()
    if line.strip()
] if download_failures_path.is_file() else []
clean_workspace = out_root / "clean-workspace"

def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

files = []
schema_versions = []
artifact_names = set()
for path in sorted(p for p in clean_workspace.rglob("*") if p.is_file()):
    rel = path.relative_to(clean_workspace)
    for part in rel.parts[:-1]:
        artifact_names.add(part)
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

schema_set = set(schema_versions)
path_text = "\n".join(item["path"] for item in files)
missing_required_artifacts = [
    artifact
    for artifact in required_artifacts
    if artifact not in artifact_names and artifact not in path_text
]
missing_required_schemas = [
    schema
    for schema in required_schemas
    if schema not in schema_set
]
status = "dry_run" if dry_run and not fixture_dir else "passed"
if (not dry_run or fixture_dir) and not files:
    status = "failed"
if missing_required_artifacts or missing_required_schemas:
    status = "failed"
if download_failures:
    status = "failed"

payload = {
    "schema_version": "ao2.release-artifact-consumer-smoke.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "dry_run": dry_run,
    "fixture_dir": fixture_dir or None,
    "artifact_root": str(out_root),
    "clean_workspace": str(clean_workspace),
    "repos": repos,
    "download_command": "gh run download",
    "download_timeout_seconds": download_timeout_seconds,
    "download_failures": download_failures,
    "artifact_names": sorted(artifact_names),
    "required_artifacts": required_artifacts,
    "missing_required_artifacts": missing_required_artifacts,
    "required_schemas": required_schemas,
    "missing_required_schemas": missing_required_schemas,
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
