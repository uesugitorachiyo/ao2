#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RESUME_JSON="${AO2_PULSE_RESUME_JSON:-$ROOT/.ao2-local/pulse/latest/resume.json}"
OUT_ROOT="${AO2_PULSE_RESUME_ROOT:-$ROOT/target/pulse-resume/latest}"
SUMMARY="$OUT_ROOT/summary.json"
EXECUTE=0
DRY_RUN=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --execute)
      EXECUTE=1
      shift
      ;;
    *)
      echo "usage: $0 (--dry-run | --execute)" >&2
      exit 2
      ;;
  esac
done

if [ "$DRY_RUN" = "1" ] && [ "$EXECUTE" = "1" ]; then
  echo "--dry-run and --execute are mutually exclusive" >&2
  exit 2
fi

mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$RESUME_JSON" "$SUMMARY" "$DRY_RUN" "$EXECUTE" <<'PY'
import json
import shlex
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
resume_json = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
dry_run = sys.argv[4] == "1"
execute = sys.argv[5] == "1"

def shasum(path: Path) -> str:
    return subprocess.check_output(["shasum", "-a", "256", str(path)], text=True).split()[0]

if not resume_json.is_file():
    raise SystemExit(f"resume.json not found: {resume_json}")

resume = json.loads(resume_json.read_text(encoding="utf-8"))
eval_loop_path = resume_json.parent / str(resume["pulse_eval_loop_path"])
observed_sha = shasum(eval_loop_path)
expected_sha = str(resume["pulse_eval_loop_sha256"])
sha_matches = observed_sha == expected_sha
resume_command = str(resume["resume_command"])
exit_code = 0
reason = None

if not dry_run and not execute:
    reason = "refusing to execute without --execute"
    exit_code = 2
elif not sha_matches:
    reason = "hash_mismatch"
    exit_code = 1
elif execute:
    result = subprocess.run(shlex.split(resume_command), cwd=root, check=False)
    exit_code = int(result.returncode)

if dry_run and sha_matches:
    status = "dry_run"
elif execute and sha_matches and exit_code == 0:
    status = "passed"
else:
    status = "failed"
payload = {
    "schema_version": "ao2.pulse-resume.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "dry_run": dry_run,
    "execute": execute,
    "execution_mode": "execute" if execute else ("dry_run" if dry_run else "refused"),
    "reason": reason,
    "resume_json": str(resume_json),
    "pulse_eval_loop_path": str(eval_loop_path),
    "pulse_eval_loop_sha256": expected_sha,
    "observed_sha256": observed_sha,
    "sha256_matches": sha_matches,
    "resume_command": resume_command,
    "exit_code": exit_code,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status == "failed":
    raise SystemExit(1)
PY
