#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_EXECUTE_SAFETY_CORPUS_ROOT:-$ROOT/target/pulse-execute-safety-corpus/latest}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" <<'PY'
import hashlib
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()

def write_case(
    name: str,
    *,
    sha_override = None,
    simulation: bool = True,
    simulation_output_path = None,
    simulated_exit_code: int = 0,
    args = None,
) -> dict[str, object]:
    case_root = out_root / "cases" / name
    loop_dir = case_root / "loop-000"
    loop_dir.mkdir(parents=True, exist_ok=True)
    eval_loop = loop_dir / "pulse-eval-loop.json"
    eval_loop.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-eval-loop.v1",
                "status": "passed",
                "case": name,
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    digest = hashlib.sha256(eval_loop.read_bytes()).hexdigest()
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "pulse_eval_loop_path": "loop-000/pulse-eval-loop.json",
        "pulse_eval_loop_sha256": sha_override or digest,
        "resume_command": f"ao2 pulse eval-loop run --chain target/pulse-execute-safety-corpus/latest/cases/{name}",
        "simulation": simulation,
    }
    if simulation_output_path is not None:
        resume["simulation_output_path"] = simulation_output_path
    if simulated_exit_code:
        resume["simulated_exit_code"] = simulated_exit_code
        resume["simulation_reason"] = "simulated failure"
    resume_json = case_root / "resume.json"
    resume_json.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return {
        "case_root": case_root,
        "resume_json": resume_json,
        "args": args or ["--resume-json", str(resume_json), "--execute"],
    }

cases = [
    {
        "name": "hash_mismatch",
        "fixture": write_case(
            "hash_mismatch",
            sha_override="0" * 64,
            simulation_output_path="target/pulse-execute-safety-corpus/latest/cases/hash_mismatch/simulation-output.json",
        ),
        "expected_exit_code": 1,
        "expected_status": "failed",
        "expected_reason": "hash_mismatch",
    },
    {
        "name": "unsafe_output_path",
        "fixture": write_case(
            "unsafe_output_path",
            simulation_output_path="../unsafe-output.json",
        ),
        "expected_exit_code": 1,
        "expected_status": "failed",
        "expected_reason": "unsafe simulation_output_path",
    },
    {
        "name": "missing_simulation_output_path",
        "fixture": write_case("missing_simulation_output_path"),
        "expected_exit_code": 1,
        "expected_status": "failed",
        "expected_reason": "simulation_output_path missing",
    },
    {
        "name": "failing_simulated_command",
        "fixture": write_case(
            "failing_simulated_command",
            simulation_output_path="target/pulse-execute-safety-corpus/latest/cases/failing_simulated_command/simulation-output.json",
            simulated_exit_code=7,
        ),
        "expected_exit_code": 1,
        "expected_status": "failed",
        "expected_reason": "simulated failure",
    },
    {
        "name": "dry_run_execute_conflict",
        "fixture": write_case(
            "dry_run_execute_conflict",
            simulation_output_path="target/pulse-execute-safety-corpus/latest/cases/dry_run_execute_conflict/simulation-output.json",
            args=[],
        ),
        "expected_exit_code": 2,
        "expected_status": None,
        "expected_reason": "mutually exclusive",
    },
]

results = []
for case in cases:
    name = str(case["name"])
    fixture = case["fixture"]
    case_root = Path(fixture["case_root"])
    if name == "dry_run_execute_conflict":
        command = [
            str(root / "scripts" / "pulse-resume.sh"),
            "--resume-json",
            str(fixture["resume_json"]),
            "--dry-run",
            "--execute",
        ]
    else:
        command = [str(root / "scripts" / "pulse-resume.sh"), *fixture["args"]]
    completed = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    (case_root / "stdout.log").write_text(completed.stdout, encoding="utf-8")
    (case_root / "stderr.log").write_text(completed.stderr, encoding="utf-8")

    resume_summary = root / "target" / "pulse-resume" / "latest" / "summary.json"
    observed_status = None
    observed_reason = completed.stderr.strip() or completed.stdout.strip()
    if resume_summary.is_file() and name != "dry_run_execute_conflict":
        observed = json.loads(resume_summary.read_text(encoding="utf-8"))
        observed_status = observed.get("status")
        observed_reason = observed.get("reason")
        (case_root / "pulse-resume-summary.json").write_text(
            json.dumps(observed, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    expected_reason = str(case["expected_reason"])
    reason_matches = observed_reason is not None and expected_reason in str(observed_reason)
    status_matches = case["expected_status"] is None or observed_status == case["expected_status"]
    exit_matches = completed.returncode == case["expected_exit_code"]
    results.append(
        {
            "name": name,
            "status": "passed" if exit_matches and status_matches and reason_matches else "failed",
            "exit_code": completed.returncode,
            "expected_exit_code": case["expected_exit_code"],
            "observed_status": observed_status,
            "expected_status": case["expected_status"],
            "observed_reason": observed_reason,
            "expected_reason": expected_reason,
            "stdout": str(case_root / "stdout.log"),
            "stderr": str(case_root / "stderr.log"),
        }
    )

payload = {
    "schema_version": "ao2.pulse-execute-safety-corpus.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if all(item["status"] == "passed" for item in results) else "failed",
    "artifact_root": str(out_root),
    "summary_path": "target/pulse-execute-safety-corpus/latest/summary.json",
    "case_results": results,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "simulation_evidence_only",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
