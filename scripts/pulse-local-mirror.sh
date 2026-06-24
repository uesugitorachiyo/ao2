#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${AO2_PULSE_LOCAL_MIRROR_SOURCE:-$ROOT/target/pulse-next-recommended-tasks}"
DEST="${AO2_PULSE_LOCAL_MIRROR_DEST:-$ROOT/.ao2-local/pulse/latest}"
SUMMARY="$DEST/pulse-local-mirror-summary.json"
DEFAULT_AUTO_ADVANCE_PROMPT="After each task batch, re-evaluate AO2 and ao2-control-plane at project level. Choose next tasks by highest long-term value, not similarity to last tasks. Prefer the Risky PR Run MVP product loop, local run record, static report/export, evaluator closure evidence, public reliability, Ubuntu/macOS/Windows correctness, CI confidence, evidence quality, security/safety boundaries, control-plane integration, release readiness, and developer/operator usability. Do not create new shell wrappers unless they directly unlock a product-slice or release-readiness bottleneck. Avoid narrow recursion or low-value daemon work unless it is the bottleneck. Generate next lengthy tasks with rationale, required evidence, and stop conditions only when the readiness exit gate is not satisfied; emit stop when AO2 and ao2-control-plane readiness gates are green."
AUTO_ADVANCE_PROMPT="${AO2_PULSE_AUTO_ADVANCE_PROMPT:-$DEFAULT_AUTO_ADVANCE_PROMPT}"

if [ ! -d "$SOURCE" ]; then
  echo "pulse mirror source not found: $SOURCE" >&2
  exit 1
fi
SOURCE="$(cd "$SOURCE" && pwd -P)"

rm -rf "$DEST"
mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd -P)"

(
  cd "$SOURCE"
  find . -type f -print | while IFS= read -r file; do
    mkdir -p "$DEST/$(dirname "$file")"
    cp "$file" "$DEST/$file"
  done
)

python3 - "$SOURCE" "$DEST" "$SUMMARY" "$AUTO_ADVANCE_PROMPT" <<'PY'
import json
import subprocess
import sys
from pathlib import Path

source = Path(sys.argv[1])
dest = Path(sys.argv[2])
summary = Path(sys.argv[3])
operator_prompt = sys.argv[4]

def shasum(path: Path) -> str:
    return subprocess.check_output(["shasum", "-a", "256", str(path)], text=True).split()[0]

operator_prompt_path = dest / "operator-prompt.txt"
operator_prompt_path.write_text(operator_prompt + "\n", encoding="utf-8")
operator_prompt_sha256 = shasum(operator_prompt_path)
auto_advance = {
    "enabled": True,
    "event_driven": True,
    "registered_once": True,
    "continue_until_exit_gate": True,
    "stop_signal": "operator_stop",
    "prompt_source": "AO2_PULSE_AUTO_ADVANCE_PROMPT",
    "stores_credentials": False,
}

files = []
for path in sorted(p for p in dest.rglob("*") if p.is_file() and p.name != summary.name):
    rel = path.relative_to(dest).as_posix()
    # Keep this equivalent to `shasum -a 256 <file>` for macOS/Linux parity.
    digest = shasum(path)
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

resume = None
if eval_loop_files:
    pulse_eval_loop_path = eval_loop_files[-1]
    eval_loop_abs = dest / pulse_eval_loop_path
    pulse_eval_loop_sha256 = shasum(eval_loop_abs)
    resume_command = (
        "ao2 pulse eval-loop run --chain "
        f"--eval-loop-evidence .ao2-local/pulse/latest/{pulse_eval_loop_path} "
        f"--eval-loop-sha256 {pulse_eval_loop_sha256} "
        "--verification-status passed "
        "--packet .ao2-local/pulse/latest/packet.md "
        "--board .ao2-local/pulse/latest/board.md "
        "--out-dir target/pulse-next-recommended-tasks/loop-next "
        "--json"
    )
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "status": "ready",
        "pulse_eval_loop_path": pulse_eval_loop_path,
        "pulse_eval_loop_sha256": pulse_eval_loop_sha256,
        "operator_prompt": operator_prompt,
        "operator_prompt_path": "operator-prompt.txt",
        "operator_prompt_sha256": operator_prompt_sha256,
        "auto_advance": auto_advance,
        "resume_command": resume_command,
        "trust_boundary": {
            "local_only": True,
            "stores_credentials": False,
        },
    }
    resume_json = dest / "resume.json"
    resume_script = dest / "resume-command.sh"
    resume_json.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    resume_script.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        f"# operator_prompt_sha256={operator_prompt_sha256}\n"
        f"# operator_prompt_path=.ao2-local/pulse/latest/operator-prompt.txt\n"
        + resume_command
        + "\n",
        encoding="utf-8",
    )
    resume_script.chmod(0o755)
    for path in [resume_json, resume_script]:
        rel = path.relative_to(dest).as_posix()
        digest = shasum(path)
        files.append({"path": rel, "sha256": digest})

payload = {
    "schema_version": "ao2.pulse-local-mirror.v1",
    "status": "passed" if not missing else "incomplete",
    "source": str(source),
    "destination": str(dest),
    "file_count": len(files),
    "required_files_present": sorted(required.intersection(present)),
    "pulse_eval_loop_files": eval_loop_files,
    "missing_required_files": missing,
    "operator_prompt": operator_prompt,
    "operator_prompt_path": "operator-prompt.txt",
    "operator_prompt_sha256": operator_prompt_sha256,
    "auto_advance": auto_advance,
    "resume": resume,
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
