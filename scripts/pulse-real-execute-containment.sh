#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_REAL_EXECUTE_ROOT:-$ROOT/target/pulse-real-execute-containment/latest}"
SUMMARY="$OUT_ROOT/summary.json"
ALLOWED_OUTPUT="$OUT_ROOT/allowed-output"
RESUME_FIXTURE="$OUT_ROOT/resume-fixture"
RESUME_JSON="$RESUME_FIXTURE/resume.json"
EVAL_LOOP="$RESUME_FIXTURE/pulse-eval-loop.json"
WRITE_SCRIPT="$RESUME_FIXTURE/write-contained-output.sh"
PULSE_RESUME_ROOT="$OUT_ROOT/pulse-resume"

rm -rf "$OUT_ROOT"
mkdir -p "$ALLOWED_OUTPUT" "$RESUME_FIXTURE" "$PULSE_RESUME_ROOT"

cat >"$EVAL_LOOP" <<'JSON'
{"schema_version":"ao2.pulse-eval-loop.v1","status":"passed","mode":"real_execute_containment"}
JSON

cat >"$WRITE_SCRIPT" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
out_dir="$1"
case "$out_dir" in
  */target/pulse-real-execute-containment/latest/allowed-output) ;;
  */pulse-real-execute-containment/allowed-output) ;;
  *)
    echo "refusing output outside allowed-output: $out_dir" >&2
    exit 1
    ;;
esac
mkdir -p "$out_dir"
printf '{"schema_version":"ao2.pulse-contained-real-output.v1","status":"passed"}\n' > "$out_dir/contained-output.json"
SH
chmod +x "$WRITE_SCRIPT"

eval_sha="$(shasum -a 256 "$EVAL_LOOP" | awk '{print $1}')"
resume_command="bash $WRITE_SCRIPT $ALLOWED_OUTPUT"
resume_command_digest="$(printf "%s" "$resume_command" | shasum -a 256 | awk '{print $1}')"

python3 - "$RESUME_JSON" "$eval_sha" "$resume_command" "$resume_command_digest" <<'PY'
import json
import sys
from pathlib import Path

resume_json = Path(sys.argv[1])
payload = {
    "schema_version": "ao2.pulse-resume-packet.v1",
    "pulse_eval_loop_path": "pulse-eval-loop.json",
    "pulse_eval_loop_sha256": sys.argv[2],
    "resume_command": sys.argv[3],
    "resume_command_digest": sys.argv[4],
    "simulation": False,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "allowed-output-only",
    },
}
resume_json.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

AO2_PULSE_RESUME_ROOT="$PULSE_RESUME_ROOT" \
  npm run pulse:resume -- --resume-json "$RESUME_JSON" --execute

python3 - "$OUT_ROOT" "$SUMMARY" "$RESUME_JSON" "$PULSE_RESUME_ROOT/summary.json" "$ALLOWED_OUTPUT/contained-output.json" "$resume_command_digest" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
resume_json = Path(sys.argv[3]).resolve()
pulse_resume_summary_path = Path(sys.argv[4]).resolve()
contained_output_path = Path(sys.argv[5]).resolve()
resume_command_digest = sys.argv[6]

pulse_resume = json.loads(pulse_resume_summary_path.read_text(encoding="utf-8"))
contained_output = json.loads(contained_output_path.read_text(encoding="utf-8")) if contained_output_path.exists() else {}
passed = (
    pulse_resume.get("status") == "passed"
    and pulse_resume.get("execute") is True
    and pulse_resume.get("sha256_matches") is True
    and contained_output.get("schema_version") == "ao2.pulse-contained-real-output.v1"
    and contained_output.get("status") == "passed"
)
payload = {
    "schema_version": "ao2.pulse-real-execute-containment.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if passed else "failed",
    "artifact_root": str(out_root),
    "resume_json": str(resume_json),
    "pulse_resume_summary": str(pulse_resume_summary_path),
    "contained_output": str(contained_output_path),
    "allowed_output_root": str(contained_output_path.parent),
    "sha256_matches": pulse_resume.get("sha256_matches"),
    "resume_command_digest": resume_command_digest,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "allowed-output-only",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
