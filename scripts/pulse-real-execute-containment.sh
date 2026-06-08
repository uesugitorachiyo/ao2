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
PULSE_GENERATE_NEXT_ROOT="$OUT_ROOT/pulse-generate-next"
PULSE_GENERATE_NEXT_PACKET_ROOT="$OUT_ROOT/generated-next-packet"
PULSE_GENERATE_NEXT_CURSOR="$OUT_ROOT/pulse-generate-next-cursor.json"
PRODUCT_CODE_FIXTURE_ROOT="$OUT_ROOT/product-code-execute-fixture"
SANDBOX_REPO="$PRODUCT_CODE_FIXTURE_ROOT/repo"
SANDBOX_MANIFEST="$PRODUCT_CODE_FIXTURE_ROOT/pulse-task-manifest.json"
PULSE_TASK_EXECUTOR_ROOT="$OUT_ROOT/pulse-task-executor"

rm -rf "$OUT_ROOT"
mkdir -p "$ALLOWED_OUTPUT" "$RESUME_FIXTURE" "$PULSE_RESUME_ROOT" "$PRODUCT_CODE_FIXTURE_ROOT"

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

AO2_PULSE_GENERATE_NEXT_REGISTER=0 \
AO2_PULSE_GENERATE_NEXT_ROOT="$PULSE_GENERATE_NEXT_ROOT" \
AO2_PULSE_GENERATE_NEXT_PACKET_ROOT="$PULSE_GENERATE_NEXT_PACKET_ROOT" \
AO2_PULSE_GENERATE_NEXT_CURSOR="$PULSE_GENERATE_NEXT_CURSOR" \
  npm run pulse:generate-next

python3 - "$SANDBOX_REPO" "$SANDBOX_MANIFEST" <<'PY'
import json
import subprocess
import sys
from pathlib import Path

repo = Path(sys.argv[1]).resolve()
manifest_path = Path(sys.argv[2]).resolve()
repo.mkdir(parents=True, exist_ok=True)
(repo / "allowed.txt").write_text("before\n", encoding="utf-8")
subprocess.run(["git", "init"], cwd=repo, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
subprocess.run(["git", "config", "user.email", "ao2@example.invalid"], cwd=repo, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
subprocess.run(["git", "add", "."], cwd=repo, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
subprocess.run(["git", "commit", "-m", "seed"], cwd=repo, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
payload = {
    "schema_version": "ao2.pulse-task-manifest.v1",
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_process_execution_and_packet_materialization",
    },
    "product_code_execution": {"enabled": True, "mode": "execute"},
    "tasks": [
        {
            "id": "sandbox-product-code-execute",
            "kind": "product_code",
            "title": "Sandbox product-code execute fixture",
            "objective": "Prove Pulse task executor can invoke the code-agent runner in execute mode inside a temporary git repo.",
            "repo": "ao2-pulse-sandbox",
            "repo_path": str(repo),
            "branch": "codex/sandbox-product-code-execute",
            "files": ["allowed.txt"],
            "acceptance": ["allowed.txt contains after."],
            "verification": [
                {
                    "command": "python3 -c \"from pathlib import Path; assert Path('allowed.txt').read_text() == 'after\\n'\"",
                    "expected_evidence": "ao2.pulse.sandbox-product-code-execute.allowed-file-updated",
                }
            ],
            "code_agent": {
                "command": "python3 -c \"from pathlib import Path; Path('allowed.txt').write_text('after\\\\n', encoding='utf-8')\""
            },
            "stop_conditions": ["Stop if unrelated dirty files are present."],
        }
    ],
}
manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

AO2_PULSE_CODE_AGENT_EXECUTE=1 \
AO2_PULSE_TASK_EXECUTOR_MANIFEST="$SANDBOX_MANIFEST" \
AO2_PULSE_TASK_EXECUTOR_ROOT="$PULSE_TASK_EXECUTOR_ROOT" \
  npm run pulse:task-executor

python3 - "$OUT_ROOT" "$SUMMARY" "$RESUME_JSON" "$PULSE_RESUME_ROOT/summary.json" "$ALLOWED_OUTPUT/contained-output.json" "$resume_command_digest" "$PULSE_GENERATE_NEXT_ROOT/summary.json" "$PULSE_TASK_EXECUTOR_ROOT/summary.json" "$SANDBOX_REPO" <<'PY'
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
pulse_generate_next_summary_path = Path(sys.argv[7]).resolve()
pulse_task_executor_summary_path = Path(sys.argv[8]).resolve()
sandbox_repo = Path(sys.argv[9]).resolve()

pulse_resume = json.loads(pulse_resume_summary_path.read_text(encoding="utf-8"))
contained_output = json.loads(contained_output_path.read_text(encoding="utf-8")) if contained_output_path.exists() else {}
pulse_generate_next = json.loads(pulse_generate_next_summary_path.read_text(encoding="utf-8"))
pulse_task_executor = json.loads(pulse_task_executor_summary_path.read_text(encoding="utf-8"))
product_result = pulse_task_executor.get("results", [{}])[0] if pulse_task_executor.get("results") else {}
code_agent_summary_path = Path(product_result.get("code_agent_summary", ""))
code_agent_summary = json.loads(code_agent_summary_path.read_text(encoding="utf-8")) if code_agent_summary_path.is_file() else {}
changed_files = code_agent_summary.get("workspace", {}).get("post_execution_dirty_files", [])
passed = (
    pulse_resume.get("status") == "passed"
    and pulse_resume.get("execute") is True
    and pulse_resume.get("sha256_matches") is True
    and contained_output.get("schema_version") == "ao2.pulse-contained-real-output.v1"
    and contained_output.get("status") == "passed"
    and pulse_generate_next.get("status") in {"passed", "ready"}
    and pulse_task_executor.get("status") == "passed"
    and product_result.get("status") == "code_agent_execute_passed"
    and code_agent_summary.get("status") == "passed"
    and code_agent_summary.get("mode") == "execute"
    and changed_files == [{"path": "allowed.txt", "status": " M"}]
)
payload = {
    "schema_version": "ao2.pulse-real-execute-containment.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if passed else "failed",
    "artifact_root": str(out_root),
    "resume_json": str(resume_json),
    "pulse_resume_summary": str(pulse_resume_summary_path),
    "pulse_generate_next_summary": str(pulse_generate_next_summary_path),
    "pulse_task_executor_summary": str(pulse_task_executor_summary_path),
    "contained_output": str(contained_output_path),
    "allowed_output_root": str(contained_output_path.parent),
    "sha256_matches": pulse_resume.get("sha256_matches"),
    "resume_command_digest": resume_command_digest,
    "product_code_execute_fixture": {
        "status": "passed" if (
            pulse_task_executor.get("status") == "passed"
            and product_result.get("status") == "code_agent_execute_passed"
            and code_agent_summary.get("status") == "passed"
            and changed_files == [{"path": "allowed.txt", "status": " M"}]
        ) else "failed",
        "sandbox_repo": str(sandbox_repo),
        "task_executor_result": product_result,
        "code_agent_summary": str(code_agent_summary_path),
        "changed_files": changed_files,
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "allowed-output-and-temporary-sandbox-repo-only",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
