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
    --resume-json)
      RESUME_JSON="${2:-}"
      if [ -z "$RESUME_JSON" ]; then
        echo "--resume-json requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --execute)
      EXECUTE=1
      shift
      ;;
    *)
      echo "usage: $0 [--resume-json <path>] (--dry-run | --execute)" >&2
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

DEFAULT_OPERATOR_PROMPT = "After each task batch, re-evaluate AO2 and ao2-control-plane at project level. Choose next tasks by highest long-term value, not similarity to last tasks. Prefer the Risky PR Run MVP product loop, local run record, static report/export, evaluator closure evidence, public reliability, Ubuntu/macOS/Windows correctness, CI confidence, evidence quality, security/safety boundaries, control-plane integration, release readiness, and developer/operator usability. Do not create new shell wrappers unless they directly unlock a product-slice or release-readiness bottleneck. Avoid narrow recursion or low-value daemon work unless it is the bottleneck. Generate next lengthy tasks with rationale, required evidence, and stop conditions only when the readiness exit gate is not satisfied; emit stop when AO2 and ao2-control-plane readiness gates are green."

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
operator_prompt = str(resume.get("operator_prompt", DEFAULT_OPERATOR_PROMPT))
operator_prompt_path_value = resume.get("operator_prompt_path")
operator_prompt_sha256 = str(resume.get("operator_prompt_sha256", ""))
auto_advance = resume.get("auto_advance") if isinstance(resume.get("auto_advance"), dict) else {}
simulation = bool(resume.get("simulation", False))
simulation_output_path = resume.get("simulation_output_path")
simulated_exit_code = int(resume.get("simulated_exit_code", 0) or 0)
exit_code = 0
reason = None
simulation_executed = False
resolved_simulation_output_path = None
operator_prompt_observed_sha256 = None
operator_prompt_sha256_matches = None
resolved_operator_prompt_path = None

def safe_relative_path(value: object) -> Path:
    rel = Path(str(value))
    if rel.is_absolute() or ".." in rel.parts:
        raise ValueError(f"unsafe simulation_output_path: {value!r}")
    return root / rel

def safe_resume_relative_path(value: object, label: str) -> Path:
    rel = Path(str(value))
    if rel.is_absolute() or ".." in rel.parts:
        raise ValueError(f"unsafe {label}: {value!r}")
    return resume_json.parent / rel

if operator_prompt_path_value:
    try:
        operator_prompt_path = safe_resume_relative_path(operator_prompt_path_value, "operator_prompt_path")
    except ValueError as exc:
        reason = str(exc)
        exit_code = 1
    else:
        resolved_operator_prompt_path = str(operator_prompt_path)
        if not operator_prompt_path.is_file():
            reason = f"operator_prompt_path not found: {operator_prompt_path}"
            exit_code = 1
        else:
            operator_prompt_observed_sha256 = shasum(operator_prompt_path)
            operator_prompt_sha256_matches = operator_prompt_observed_sha256 == operator_prompt_sha256
            if not operator_prompt_sha256_matches:
                reason = "operator_prompt_hash_mismatch"
                exit_code = 1

if exit_code != 0:
    pass
elif not dry_run and not execute:
    reason = "refusing to execute without --execute"
    exit_code = 2
elif not sha_matches:
    reason = "hash_mismatch"
    exit_code = 1
elif execute and simulation:
    if simulated_exit_code != 0:
        reason = str(resume.get("simulation_reason", "simulated failure"))
        exit_code = simulated_exit_code
    elif simulation_output_path is None:
        reason = "simulation_output_path missing"
        exit_code = 1
    else:
        try:
            output_path = safe_relative_path(simulation_output_path)
        except ValueError as exc:
            reason = str(exc)
            exit_code = 1
        else:
            resolved_simulation_output_path = str(output_path)
            output_path.parent.mkdir(parents=True, exist_ok=True)
            output_payload = {
                "schema_version": "ao2.pulse-execute-simulation.v1",
                "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
                "status": "passed",
                "resume_json": str(resume_json),
                "resume_command": resume_command,
                "pulse_eval_loop_sha256": expected_sha,
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "simulation_evidence_only",
                },
            }
            output_path.write_text(json.dumps(output_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            simulation_executed = True
elif execute:
    result = subprocess.run(shlex.split(resume_command), cwd=root, check=False)
    exit_code = int(result.returncode)

if dry_run and sha_matches and exit_code == 0:
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
    "simulation": simulation,
    "simulation_executed": simulation_executed,
    "simulated_exit_code": simulated_exit_code,
    "simulation_output_path": resolved_simulation_output_path or (str(simulation_output_path) if simulation_output_path else None),
    "resume_json": str(resume_json),
    "pulse_eval_loop_path": str(eval_loop_path),
    "pulse_eval_loop_sha256": expected_sha,
    "observed_sha256": observed_sha,
    "sha256_matches": sha_matches,
    "operator_prompt": operator_prompt,
    "operator_prompt_path": resolved_operator_prompt_path or operator_prompt_path_value,
    "operator_prompt_sha256": operator_prompt_sha256 or None,
    "operator_prompt_observed_sha256": operator_prompt_observed_sha256,
    "operator_prompt_sha256_matches": operator_prompt_sha256_matches,
    "auto_advance": auto_advance,
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
