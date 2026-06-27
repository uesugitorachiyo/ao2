#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_TASK_BOARD_STATE_ROOT:-$ROOT/target/pulse-task-board-state/latest}"
BOARD="${AO2_PULSE_TASK_BOARD_STATE_BOARD:-$ROOT/target/pulse-task-board/latest/summary.json}"
SUMMARY="$OUT_ROOT/summary.json"

mkdir -p "$OUT_ROOT"

python3 - "$BOARD" "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

board_path = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

payload = {
    "schema_version": "ao2.pulse-task-board-state.v1",
    "generated_at_utc": utc_now(),
    "status": "failed",
    "artifact_root": str(out_root),
    "task_board": str(board_path),
    "state_summary": None,
    "task_count": 0,
    "status_counts": {},
    "status_transition_source": None,
    "next_actions": [],
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}

if not board_path.is_file():
    payload["reason"] = "task_board_missing"
else:
    try:
        board = json.loads(board_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        board = {}
        payload["reason"] = f"task_board_invalid_json:{exc.lineno}"
    if board and board.get("schema_version") != "ao2.ai-task-board.v1":
        payload["reason"] = "task_board_schema_invalid"
    elif board:
        exports = board.get("exports") if isinstance(board.get("exports"), dict) else {}
        state_summary_path = Path(str(exports.get("state_summary") or board_path.with_name("board-state-summary.json"))).expanduser().resolve()
        state_summary = {}
        if state_summary_path.is_file():
            state_summary = json.loads(state_summary_path.read_text(encoding="utf-8"))
        tasks = [item for item in board.get("tasks", []) if isinstance(item, dict)]
        status_counts = {}
        next_actions = []
        for item in tasks:
            status = str(item.get("status") or "unknown")
            status_counts[status] = status_counts.get(status, 0) + 1
            next_actions.append({
                "task_id": item.get("task_id"),
                "stable_task_id": item.get("stable_task_id"),
                "title": item.get("title"),
                "status": status,
                "next_action": item.get("next_action"),
            })
        status_transition_source = (
            state_summary.get("status_transition_source")
            if isinstance(state_summary.get("status_transition_source"), dict)
            else board.get("status_transition_source")
        )
        payload.update({
            "status": "passed",
            "reason": "task_board_state_read",
            "state_summary": str(state_summary_path) if state_summary_path.is_file() else None,
            "task_count": len(tasks),
            "status_counts": state_summary.get("status_counts") or status_counts,
            "status_transition_source": status_transition_source
            if isinstance(status_transition_source, dict)
            else None,
            "next_actions": state_summary.get("next_actions") or next_actions,
        })

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if isinstance(payload.get("status_transition_source"), dict):
    source = payload["status_transition_source"]
    line = f"status_transition_source={source.get('status', 'unknown')}"
    if source.get("task_board_generation") is not None:
        line += f" evidence_generation={source.get('task_board_generation')}"
    if source.get("current_generation") is not None:
        line += f" board_generation={source.get('current_generation')}"
    if source.get("updates_applied") is not None:
        line += f" updates_applied={source.get('updates_applied')}"
    print(line)
if payload["status"] != "passed":
    raise SystemExit(1)
PY
