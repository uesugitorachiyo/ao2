#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${AO2_PULSE_TASK_EXECUTOR_MANIFEST:-$ROOT/.ao2-local/pulse/latest/pulse-task-manifest.json}"
OUT_ROOT="${AO2_PULSE_TASK_EXECUTOR_ROOT:-$ROOT/target/pulse-task-executor/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
PACKET_DIR="$OUT_ROOT/implementation-packets"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR" "$PACKET_DIR"

python3 - "$ROOT" "$MANIFEST" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$PACKET_DIR" <<'PY'
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
manifest_path = Path(sys.argv[2]).resolve()
out_root = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
log_dir = Path(sys.argv[5]).resolve()
packet_dir = Path(sys.argv[6]).resolve()

ALLOWED_KINDS = {"product_code", "evidence_gate", "verification"}
EXECUTABLE_KINDS = {"evidence_gate", "verification"}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


payload = {
    "schema_version": "ao2.pulse-task-executor.v1",
    "generated_at_utc": utc_now(),
    "status": "failed",
    "manifest": str(manifest_path),
    "artifact_root": str(out_root),
    "implementation_packet_dir": str(packet_dir),
    "counts": {"product_code": 0, "evidence_gate": 0, "verification": 0},
    "results": [],
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}


def write_summary() -> None:
    summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def fail(reason: str, code: int = 1) -> None:
    payload["status"] = "failed"
    payload["reason"] = reason
    write_summary()
    print(f"summary={summary_path}")
    print("status=failed")
    raise SystemExit(code)


def require_string(value: object, reason: str) -> str:
    if not isinstance(value, str) or not value.strip():
        fail(reason)
    return value.strip()


def require_string_list(value: object, reason: str) -> list[str]:
    if not isinstance(value, list) or not value or not all(isinstance(item, str) and item.strip() for item in value):
        fail(reason)
    return [item.strip() for item in value]


def slug(value: str) -> str:
    clean = re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-")
    return clean or "task"


def render_verification(items: object) -> list[str]:
    if not isinstance(items, list):
        return []
    rendered = []
    for item in items:
        if isinstance(item, dict):
            command = item.get("command")
            expected = item.get("expected_evidence")
            if isinstance(command, str) and command.strip():
                if isinstance(expected, str) and expected.strip():
                    rendered.append(f"- `{command.strip()}` => {expected.strip()}")
                else:
                    rendered.append(f"- `{command.strip()}`")
        elif isinstance(item, str) and item.strip():
            rendered.append(f"- {item.strip()}")
    return rendered


def has_product_verification_evidence(items: object) -> bool:
    if not isinstance(items, list):
        return False
    for item in items:
        if not isinstance(item, dict):
            continue
        command = item.get("command")
        expected = item.get("expected_evidence")
        if (
            isinstance(command, str)
            and command.strip()
            and isinstance(expected, str)
            and expected.strip()
        ):
            return True
    return False


def materialize_product_packet(task: dict) -> dict:
    task_id = require_string(task.get("id"), "product_code_id_missing")
    title = require_string(task.get("title"), "product_code_title_missing")
    objective = require_string(task.get("objective"), "product_code_objective_missing")
    files = require_string_list(task.get("files"), "product_code_files_missing")
    acceptance = require_string_list(task.get("acceptance"), "product_code_acceptance_missing")
    stop_conditions = task.get("stop_conditions") if isinstance(task.get("stop_conditions"), list) else []
    stop_lines = [str(item).strip() for item in stop_conditions if str(item).strip()]
    verification = render_verification(task.get("verification"))

    packet_path = packet_dir / f"{slug(task_id)}.md"
    lines = [
        f"# {title}",
        "",
        f"task_id: {task_id}",
        "kind: product_code",
        "",
        "## Objective",
        objective,
        "",
        "## Files",
        *[f"- {item}" for item in files],
        "",
        "## Acceptance",
        *[f"- {item}" for item in acceptance],
        "",
        "## Verification",
        *(verification or ["- Verification command must be selected before implementation closure."]),
        "",
        "## Stop Conditions",
        *(["- " + item for item in stop_lines] or ["- Stop if the task requires provider API keys or credential storage."]),
        "",
    ]
    packet_path.write_text("\n".join(lines), encoding="utf-8")
    return {
        "id": task_id,
        "kind": "product_code",
        "title": title,
        "status": "packet_materialized",
        "packet": str(packet_path),
        "files": files,
    }


def run_executable_task(task: dict, index: int) -> dict:
    task_id = require_string(task.get("id"), "executable_task_id_missing")
    kind = require_string(task.get("kind"), "executable_task_kind_missing")
    command = require_string(task.get("command"), "executable_task_command_missing")
    log_path = log_dir / f"{index:02d}-{slug(task_id)}.log"
    env = dict(os.environ)
    env.setdefault("AO2_PULSE_LOCAL_MIRROR_DEST", str((out_root / "task-executor-local-mirror").resolve()))
    with log_path.open("w", encoding="utf-8") as log:
        log.write(f"$ {command}\n")
        log.write(f"AO2_PULSE_LOCAL_MIRROR_DEST={env['AO2_PULSE_LOCAL_MIRROR_DEST']}\n")
        log.flush()
        result = subprocess.run(command, cwd=root, shell=True, env=env, stdout=log, stderr=subprocess.STDOUT)
    status = "passed" if result.returncode == 0 else "failed"
    return {
        "id": task_id,
        "kind": kind,
        "title": task.get("title", task_id),
        "status": status,
        "exit_code": int(result.returncode),
        "command": command,
        "isolated_pulse_local_mirror_dest": env["AO2_PULSE_LOCAL_MIRROR_DEST"],
        "expected_evidence": task.get("expected_evidence"),
        "log": str(log_path),
    }


if not manifest_path.is_file():
    fail("manifest_missing")

try:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
except json.JSONDecodeError:
    fail("manifest_json_invalid")

if manifest.get("schema_version") != "ao2.pulse-task-manifest.v1":
    fail("manifest_schema_version_unsupported")

trust_boundary = manifest.get("trust_boundary")
if not isinstance(trust_boundary, dict):
    fail("trust_boundary_missing")
payload["trust_boundary"] = trust_boundary
if trust_boundary.get("local_only") is not True:
    fail("non_local_manifest_rejected")
if trust_boundary.get("stores_credentials") is not False:
    fail("credential_storing_manifest_rejected")

tasks = manifest.get("tasks")
if not isinstance(tasks, list) or not tasks:
    fail("tasks_missing")

seen_ids: set[str] = set()
for index, task in enumerate(tasks, start=1):
    if not isinstance(task, dict):
        fail("task_not_object")
    task_id = require_string(task.get("id"), "task_id_missing")
    if task_id in seen_ids:
        fail("duplicate_task_id")
    seen_ids.add(task_id)
    kind = require_string(task.get("kind"), "task_kind_missing")
    if kind not in ALLOWED_KINDS:
        fail("task_kind_unsupported")
    payload["counts"][kind] += 1
    if kind == "product_code":
        if not has_product_verification_evidence(task.get("verification")):
            payload["results"].append({
                "id": task_id,
                "kind": "product_code",
                "title": task.get("title", task_id),
                "status": "failed",
                "reason": "product_code_verification_evidence_missing",
            })
            fail("product_code_verification_evidence_missing")
        payload["results"].append(materialize_product_packet(task))
    elif kind in EXECUTABLE_KINDS:
        result = run_executable_task(task, index)
        payload["results"].append(result)
        if result["status"] != "passed":
            payload["status"] = "failed"
            payload["reason"] = "executable_task_failed"
            write_summary()
            print(f"summary={summary_path}")
            print("status=failed")
            raise SystemExit(int(result["exit_code"]) or 1)

payload["status"] = "passed"
payload["reason"] = "all_tasks_processed"
write_summary()
print(f"summary={summary_path}")
print("status=passed")
PY
