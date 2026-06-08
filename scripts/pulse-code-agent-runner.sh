#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TASK_PATH="${AO2_PULSE_CODE_AGENT_TASK:-}"
OUT_ROOT="${AO2_PULSE_CODE_AGENT_RUNNER_ROOT:-$ROOT/target/pulse-code-agent-runner/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
DRY_RUN=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --task)
      TASK_PATH="${2:-}"
      if [ -z "$TASK_PATH" ]; then
        echo "--task requires a value" >&2
        exit 2
      fi
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    *)
      echo "usage: $0 --task <ao2.pulse-code-agent-task.v1.json> --dry-run" >&2
      exit 2
      ;;
  esac
done

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

python3 - "$ROOT" "$TASK_PATH" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$DRY_RUN" <<'PY'
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
task_path_arg = sys.argv[2]
out_root = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
log_dir = Path(sys.argv[5]).resolve()
dry_run = sys.argv[6] == "1"

# workspace guard: git status --porcelain
FORBIDDEN_TOKENS = [
    "OPENAI" + "_API_KEY",
    "ANTHROPIC" + "_API_KEY",
    "git push" + " origin",
    "gh pr" + " create",
    "gh release" + " create",
]


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


payload = {
    "schema_version": "ao2.pulse-code-agent-runner.v1",
    "generated_at_utc": utc_now(),
    "status": "failed",
    "mode": "dry_run" if dry_run else "execute",
    "task_path": task_path_arg,
    "artifact_root": str(out_root),
    "logs": str(log_dir),
    "task": {},
    "workspace": {"git_status_checked": False},
    "verification": [],
    "execution": {
        "would_invoke_code_agent": False,
        "invoked_code_agent": False,
        "pushes": False,
        "opens_pr": False,
        "publishes_release": False,
        "stores_credentials": False,
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}


def write_summary() -> None:
    summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def finish(status: str, reason: str, code: int) -> None:
    payload["status"] = status
    payload["reason"] = reason
    payload["generated_at_utc"] = utc_now()
    write_summary()
    print(f"summary={summary_path}")
    print(f"status={status}")
    raise SystemExit(code)


def require_string(value: object, reason: str) -> str:
    if not isinstance(value, str) or not value.strip():
        finish("failed", reason, 1)
    return value.strip()


def require_string_list(value: object, reason: str) -> list[str]:
    if not isinstance(value, list) or not value:
        finish("failed", reason, 1)
    result = []
    for item in value:
        if not isinstance(item, str) or not item.strip():
            finish("failed", reason, 1)
        result.append(item.strip())
    return result


def require_verification(value: object) -> list[dict]:
    if not isinstance(value, list) or not value:
        finish("failed", "verification_missing", 1)
    result = []
    for item in value:
        if not isinstance(item, dict):
            finish("failed", "verification_item_invalid", 1)
        command = require_string(item.get("command"), "verification_command_missing")
        expected = require_string(item.get("expected_evidence"), "verification_expected_evidence_missing")
        result.append({"command": command, "expected_evidence": expected})
    return result


def validate_relative_path(value: str) -> None:
    path = Path(value)
    if path.is_absolute() or ".." in path.parts or ".git" in path.parts:
        finish("failed", "allowed_file_path_unsafe", 1)


def resolve_repo_path(task: dict) -> Path:
    repo_path = task.get("repo_path")
    if isinstance(repo_path, str) and repo_path.strip():
        return Path(repo_path).expanduser().resolve()
    repo_name = require_string(task.get("repo"), "repo_missing")
    if repo_name == root.name:
        return root
    sibling = (root.parent / repo_name).resolve()
    if sibling.exists():
        return sibling
    return root


def git_status(repo_path: Path) -> list[dict]:
    log_path = log_dir / "git-status.log"
    with log_path.open("w", encoding="utf-8") as log:
        log.write("$ git status --porcelain\n")
        log.flush()
        result = subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=repo_path,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            check=False,
        )
        log.write(result.stdout)
    payload["workspace"]["git_status_log"] = str(log_path)
    if result.returncode != 0:
        finish("failed", "git_status_failed", 1)
    entries = []
    for line in result.stdout.splitlines():
        if not line:
            continue
        status = line[:2]
        path = line[3:] if len(line) > 3 else ""
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        entries.append({"status": status, "path": path})
    return entries


if not dry_run:
    finish("failed", "execute_mode_not_implemented", 1)

if not task_path_arg:
    finish("failed", "task_path_missing", 1)

task_path = Path(task_path_arg).expanduser().resolve()
if not task_path.is_file():
    finish("failed", "task_missing", 1)

try:
    task_text = task_path.read_text(encoding="utf-8")
    task = json.loads(task_text)
except json.JSONDecodeError:
    finish("failed", "task_json_invalid", 1)

for token in FORBIDDEN_TOKENS:
    if token in task_text:
        finish("failed", "forbidden_token_present", 1)

if task.get("schema_version") != "ao2.pulse-code-agent-task.v1":
    finish("failed", "task_schema_version_unsupported", 1)

trust_boundary = task.get("trust_boundary")
if not isinstance(trust_boundary, dict):
    finish("failed", "trust_boundary_missing", 1)
payload["trust_boundary"] = trust_boundary
if trust_boundary.get("local_only") is not True:
    finish("failed", "non_local_task_rejected", 1)
if trust_boundary.get("stores_credentials") is not False:
    finish("failed", "credential_storing_task_rejected", 1)

task_id = require_string(task.get("id"), "task_id_missing")
title = require_string(task.get("title"), "title_missing")
objective = require_string(task.get("objective"), "objective_missing")
repo = require_string(task.get("repo"), "repo_missing")
branch = require_string(task.get("branch"), "branch_missing")
allowed_files = require_string_list(task.get("allowed_files"), "allowed_files_missing")
acceptance = require_string_list(task.get("acceptance"), "acceptance_missing")
stop_conditions = require_string_list(task.get("stop_conditions"), "stop_conditions_missing")
verification = require_verification(task.get("verification"))

if not re.fullmatch(r"[A-Za-z0-9._/-]+", branch) or branch.startswith("-"):
    finish("failed", "branch_name_unsafe", 1)

for item in allowed_files:
    validate_relative_path(item)

repo_path = resolve_repo_path(task)
if not repo_path.is_dir():
    finish("failed", "repo_path_missing", 1)
if not (repo_path / ".git").exists():
    finish("failed", "repo_not_git_worktree", 1)

status_entries = git_status(repo_path)
payload["workspace"].update({
    "repo_path": str(repo_path),
    "git_status_checked": True,
    "dirty_files": status_entries,
})
allowed_set = set(allowed_files)
unrelated_dirty = [entry for entry in status_entries if entry["path"] not in allowed_set]
payload["workspace"]["unrelated_dirty_files"] = unrelated_dirty
if unrelated_dirty:
    finish("failed", "unrelated_dirty_files_present", 1)

payload["task"] = {
    "id": task_id,
    "title": title,
    "objective": objective,
    "repo": repo,
    "branch": branch,
    "allowed_files": allowed_files,
    "acceptance": acceptance,
    "stop_conditions": stop_conditions,
}
payload["verification"] = verification
payload["execution"].update({
    "would_invoke_code_agent": True,
    "invoked_code_agent": False,
    "pushes": False,
    "opens_pr": False,
    "publishes_release": False,
    "stores_credentials": False,
})

finish("passed", "dry_run_validated_code_agent_task", 0)
PY
