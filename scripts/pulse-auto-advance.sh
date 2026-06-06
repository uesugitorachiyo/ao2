#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RESUME_JSON="${AO2_PULSE_RESUME_JSON:-$ROOT/.ao2-local/pulse/latest/resume.json}"
OUT_ROOT="${AO2_PULSE_AUTO_ADVANCE_ROOT:-$ROOT/target/pulse-auto-advance/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
LEDGER="${AO2_PULSE_AUTO_ADVANCE_LEDGER:-$ROOT/.ao2-local/pulse/pulse-auto-advance-ledger.jsonl}"
STOP_FILE="${AO2_PULSE_AUTO_ADVANCE_STOP_FILE:-$ROOT/.ao2-local/pulse/STOP}"
MAX_ITERATIONS="${AO2_PULSE_AUTO_ADVANCE_MAX_ITERATIONS:-1}"
MAX_ITERATIONS_EXPLICIT=0
ALLOW_DUPLICATE="${AO2_PULSE_AUTO_ADVANCE_ALLOW_DUPLICATE:-0}"
FOREVER=0
SLEEP_SECONDS="${AO2_PULSE_AUTO_ADVANCE_SLEEP_SECONDS:-30}"
GENERATE_NEXT="${AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT:-1}"
GENERATE_NEXT_SLEEP_SECONDS="${AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT_SLEEP_SECONDS:-$SLEEP_SECONDS}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --forever)
      FOREVER=1
      shift
      ;;
    --max-iterations)
      MAX_ITERATIONS="${2:-}"
      MAX_ITERATIONS_EXPLICIT=1
      if [ -z "$MAX_ITERATIONS" ]; then
        echo "--max-iterations requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    --allow-duplicate)
      ALLOW_DUPLICATE=1
      shift
      ;;
    --sleep-seconds)
      SLEEP_SECONDS="${2:-}"
      if [ -z "$SLEEP_SECONDS" ]; then
        echo "--sleep-seconds requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--forever] [--max-iterations <n>] [--allow-duplicate] [--sleep-seconds <n>]" >&2
      exit 2
      ;;
  esac
done

if [ "$FOREVER" = "1" ] && [ "$MAX_ITERATIONS_EXPLICIT" = "0" ] && [ -z "${AO2_PULSE_AUTO_ADVANCE_MAX_ITERATIONS:-}" ]; then
  MAX_ITERATIONS=0
fi

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR" "$(dirname "$LEDGER")"

python3 - "$ROOT" "$RESUME_JSON" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$LEDGER" "$STOP_FILE" "$MAX_ITERATIONS" "$ALLOW_DUPLICATE" "$FOREVER" "$SLEEP_SECONDS" "$GENERATE_NEXT" "$GENERATE_NEXT_SLEEP_SECONDS" <<'PY'
import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
resume_json = Path(sys.argv[2]).resolve()
out_root = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
log_dir = Path(sys.argv[5]).resolve()
ledger = Path(sys.argv[6]).resolve()
stop_file = Path(sys.argv[7]).resolve()
max_iterations = int(sys.argv[8])
allow_duplicate = sys.argv[9] == "1"
forever = sys.argv[10] == "1"
sleep_seconds = float(sys.argv[11])
GENERATE_NEXT = sys.argv[12]
generate_next_sleep_seconds = float(sys.argv[13])

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def sha256_path(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

def write_summary(payload: dict) -> None:
    summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

payload = {
    "schema_version": "ao2.pulse-auto-advance-run.v1",
    "generated_at_utc": utc_now(),
    "status": "failed",
    "resume_json": str(resume_json),
    "max_iterations": max_iterations,
    "forever": forever,
    "sleep_seconds": sleep_seconds,
    "completed_iterations": 0,
    "heartbeat_count": 0,
    "stop_file": str(stop_file),
    "ledger": str(ledger),
    "results": [],
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}

def load_seen() -> set[str]:
    seen = set()
    if ledger.is_file():
        for line in ledger.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            digest = item.get("pulse_eval_loop_sha256")
            if digest:
                seen.add(str(digest))
    return seen

def write_heartbeat(reason: str, resume: dict | None = None, digest: str | None = None) -> None:
    payload["schema_version"] = "ao2.pulse-auto-advance-heartbeat.v1"
    payload["status"] = "waiting"
    payload["reason"] = reason
    payload["generated_at_utc"] = utc_now()
    payload["heartbeat_count"] = int(payload.get("heartbeat_count", 0)) + 1
    if resume is not None:
        payload["auto_advance"] = resume.get("auto_advance", {})
    if digest:
        payload["observed_eval_loop_sha256"] = digest
    write_summary(payload)

def pulse_generate_next(reason: str) -> bool:
    if not forever or GENERATE_NEXT != "1":
        return False
    log_path = log_dir / f"pulse_generate_next-{int(time.time())}.log"
    with log_path.open("w", encoding="utf-8") as log:
        log.write("$ npm run pulse:generate-next\n")
        log.write(f"reason={reason}\n")
        log.flush()
        result = subprocess.run("npm run pulse:generate-next", cwd=root, shell=True, stdout=log, stderr=subprocess.STDOUT)
    payload["pulse_generate_next"] = {
        "command": "pulse:generate-next",
        "status": "passed" if result.returncode == 0 else "failed",
        "exit_code": int(result.returncode),
        "log": str(log_path),
        "reason": reason,
        "sleep_seconds": generate_next_sleep_seconds,
    }
    payload["generated_next_packet"] = result.returncode == 0
    payload["register_next_packet"] = result.returncode == 0
    payload["status"] = "waiting" if result.returncode == 0 else "failed"
    payload["reason"] = "generated_next_packet" if result.returncode == 0 else "generate_next_failed"
    payload["generated_at_utc"] = utc_now()
    write_summary(payload)
    return result.returncode == 0

if stop_file.exists():
    payload["status"] = "stopped"
    payload["reason"] = "stop_file_present"
    write_summary(payload)
    print(f"summary={summary_path}")
    print("status=stopped")
    raise SystemExit(0)

if not resume_json.is_file():
    payload["reason"] = f"resume_json_missing: {resume_json}"
    write_summary(payload)
    print(f"summary={summary_path}")
    print("status=failed")
    raise SystemExit(1)

iteration = 0
while True:
    if stop_file.exists():
        payload["status"] = "stopped"
        payload["reason"] = "stop_file_present"
        break
    if not resume_json.is_file():
        if forever:
            write_heartbeat("waiting_for_resume_json")
            time.sleep(sleep_seconds)
            continue
        payload["reason"] = f"resume_json_missing: {resume_json}"
        break

    resume = json.loads(resume_json.read_text(encoding="utf-8"))
    eval_loop_path = (resume_json.parent / str(resume["pulse_eval_loop_path"])).resolve()
    eval_loop_sha256 = sha256_path(eval_loop_path)
    expected_eval_loop_sha256 = str(resume["pulse_eval_loop_sha256"])
    operator_prompt_path = (resume_json.parent / str(resume.get("operator_prompt_path", "operator-prompt.txt"))).resolve()
    operator_prompt_sha256 = sha256_path(operator_prompt_path) if operator_prompt_path.is_file() else None
    payload.update({
        "schema_version": "ao2.pulse-auto-advance-run.v1",
        "status": "failed",
        "pulse_eval_loop_path": str(eval_loop_path),
        "pulse_eval_loop_sha256": expected_eval_loop_sha256,
        "observed_eval_loop_sha256": eval_loop_sha256,
        "sha256_matches": eval_loop_sha256 == expected_eval_loop_sha256,
        "operator_prompt_path": str(operator_prompt_path),
        "operator_prompt_sha256": resume.get("operator_prompt_sha256"),
        "operator_prompt_observed_sha256": operator_prompt_sha256,
        "operator_prompt_sha256_matches": operator_prompt_sha256 == resume.get("operator_prompt_sha256"),
        "auto_advance": resume.get("auto_advance", {}),
    })

    if payload["sha256_matches"] is not True:
        payload["reason"] = "eval_loop_hash_mismatch"
        break
    if payload["operator_prompt_sha256_matches"] is not True:
        payload["reason"] = "operator_prompt_hash_mismatch"
        break
    if not resume.get("auto_advance", {}).get("continue_until_stopped"):
        payload["reason"] = "auto_advance_continue_until_stopped_missing"
        break

    seen = load_seen()
    if eval_loop_sha256 in seen and not allow_duplicate:
        if forever:
            if pulse_generate_next("duplicate_eval_loop_digest"):
                time.sleep(generate_next_sleep_seconds)
                continue
            write_heartbeat("waiting_for_new_eval_loop_digest", resume, eval_loop_sha256)
            time.sleep(sleep_seconds)
            continue
        payload["status"] = "stopped"
        payload["reason"] = "duplicate_eval_loop_digest"
        break

    eval_loop = json.loads(eval_loop_path.read_text(encoding="utf-8"))
    tasks = eval_loop.get("recommended_tasks", [])
    if not isinstance(tasks, list) or not tasks:
        payload["reason"] = "recommended_tasks_missing"
        break

    iteration += 1
    payload["schema_version"] = "ao2.pulse-auto-advance-run.v1"
    payload["status"] = "running"
    payload["reason"] = "executing_recommended_tasks"
    payload["generated_at_utc"] = utc_now()
    payload["current_iteration"] = iteration
    payload["current_task_count"] = len(tasks)
    write_summary(payload)
    iteration_results = []
    for index, task in enumerate(tasks, start=1):
        task_id = str(task.get("id") or f"task-{index}")
        command = str(task.get("command") or "")
        if not command:
            iteration_results.append({"index": index, "id": task_id, "status": "failed", "reason": "command_missing"})
            break
        safe_id = "".join(ch if ch.isalnum() or ch in "._-" else "_" for ch in task_id)
        log_path = log_dir / f"iteration-{iteration:02d}-{index:02d}-{safe_id}.log"
        with log_path.open("w", encoding="utf-8") as log:
            log.write(f"$ {command}\n")
            log.flush()
            result = subprocess.run(command, cwd=root, shell=True, stdout=log, stderr=subprocess.STDOUT)
        status = "passed" if result.returncode == 0 else "failed"
        iteration_results.append({
            "iteration": iteration,
            "index": index,
            "id": task_id,
            "title": task.get("title"),
            "command": command,
            "expected_evidence": task.get("expected_evidence"),
            "status": status,
            "exit_code": int(result.returncode),
            "log": str(log_path),
        })
        if result.returncode != 0:
            break
    payload["results"].extend(iteration_results)
    if all(item.get("status") == "passed" for item in iteration_results) and len(iteration_results) == len(tasks):
        payload["completed_iterations"] = iteration
    else:
        payload["status"] = "failed"
        payload["reason"] = "task_failed"
        break
    ledger_entry = {
        "schema_version": "ao2.pulse-auto-advance-ledger-entry.v1",
        "generated_at_utc": utc_now(),
        "pulse_eval_loop_path": str(eval_loop_path),
        "pulse_eval_loop_sha256": eval_loop_sha256,
        "status": "passed",
        "task_count": len(tasks),
    }
    with ledger.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(ledger_entry, sort_keys=True) + "\n")
    if not forever:
        payload["status"] = "passed"
        break
    if max_iterations > 0 and iteration >= max_iterations:
        payload["status"] = "passed"
        payload["reason"] = "max_iterations_reached"
        break
    if pulse_generate_next("completed_iteration"):
        time.sleep(generate_next_sleep_seconds)
        continue
    payload["status"] = "waiting"
    payload["reason"] = "waiting_for_new_eval_loop_digest"
    write_summary(payload)
    time.sleep(sleep_seconds)

write_summary(payload)
print(f"summary={summary_path}")
print(f"status={payload['status']}")
raise SystemExit(0 if payload["status"] in {"passed", "stopped", "waiting"} else 1)
PY
