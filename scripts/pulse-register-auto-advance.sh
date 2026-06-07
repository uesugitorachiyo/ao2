#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_AUTO_ADVANCE_REGISTRATION_ROOT:-$ROOT/target/pulse-auto-advance-registration/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
DEFAULT_AUTO_ADVANCE_PROMPT="After each task batch, re-evaluate AO2 and ao2-control-plane at project level. Choose next tasks by highest long-term value, not similarity to last tasks. Prefer the Risky PR Run MVP product loop, local run record, static report/export, evaluator closure evidence, public reliability, Ubuntu/macOS/Windows correctness, CI confidence, evidence quality, security/safety boundaries, control-plane integration, release readiness, and developer/operator usability. Do not create new shell wrappers unless they directly unlock a product-slice or release-readiness bottleneck. Avoid narrow recursion or low-value daemon work unless it is the bottleneck. Generate next lengthy tasks with rationale, required evidence, and stop conditions, then register and continue through the AO2 event loop."
AUTO_ADVANCE_PROMPT="${AO2_PULSE_AUTO_ADVANCE_PROMPT:-$DEFAULT_AUTO_ADVANCE_PROMPT}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" pulse_local_mirror \
  env AO2_PULSE_AUTO_ADVANCE_PROMPT="$AUTO_ADVANCE_PROMPT" npm run pulse:local-mirror

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$AUTO_ADVANCE_PROMPT" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
log_dir = Path(sys.argv[4]).resolve()
operator_prompt = sys.argv[5]
resume_json = root / ".ao2-local" / "pulse" / "latest" / "resume.json"
operator_prompt_path = root / ".ao2-local" / "pulse" / "latest" / "operator-prompt.txt"
mirror_summary = root / ".ao2-local" / "pulse" / "latest" / "pulse-local-mirror-summary.json"
mirror_code = int((log_dir / "pulse_local_mirror.log.exit-code").read_text(encoding="utf-8").strip())

resume = {}
operator_prompt_sha256 = None
operator_prompt_sha256_matches = False
resume_ready = False
auto_advance = {}
resume_command = None
if resume_json.is_file():
    resume = json.loads(resume_json.read_text(encoding="utf-8"))
    auto_advance = resume.get("auto_advance") if isinstance(resume.get("auto_advance"), dict) else {}
    resume_command = resume.get("resume_command")
    resume_ready = resume.get("status") == "ready"
if operator_prompt_path.is_file():
    operator_prompt_sha256 = hashlib.sha256(operator_prompt_path.read_bytes()).hexdigest()
    operator_prompt_sha256_matches = operator_prompt_sha256 == resume.get("operator_prompt_sha256")

checks = [
    {
        "name": "pulse_local_mirror",
        "command": "pulse:local-mirror",
        "status": "passed" if mirror_code == 0 else "failed",
        "exit_code": mirror_code,
        "log": str(log_dir / "pulse_local_mirror.log"),
    },
    {"name": "resume_json", "status": "passed" if resume_json.is_file() else "failed", "path": str(resume_json)},
    {"name": "resume_ready", "status": "passed" if resume_ready else "failed"},
    {
        "name": "operator_prompt_sha256",
        "status": "passed" if operator_prompt_sha256_matches else "failed",
        "sha256": operator_prompt_sha256,
    },
    {"name": "auto_advance_registered_once", "status": "passed" if auto_advance.get("registered_once") is True else "failed"},
    {
        "name": "auto_advance_continue_until_stopped",
        "status": "passed" if auto_advance.get("continue_until_stopped") is True else "failed",
    },
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.pulse-auto-advance-registration.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "operator_prompt": operator_prompt,
    "operator_prompt_path": str(operator_prompt_path),
    "operator_prompt_sha256": operator_prompt_sha256,
    "operator_prompt_sha256_matches": operator_prompt_sha256_matches,
    "auto_advance": auto_advance,
    "resume_json": str(resume_json),
    "resume_command": resume_command,
    "mirror_summary": str(mirror_summary),
    "checks": checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_registration_only",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
