#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_ROOT:-$ROOT/target/pulse-task-board-closure-packet}"
LATEST_ROOT="$OUT_ROOT/latest"
LOG_DIR="$LATEST_ROOT/logs"
TASK_BOARD_ROOT="$LATEST_ROOT/task-board"
TASK_BOARD="$TASK_BOARD_ROOT/summary.json"
NEXT_ACTIONS_ROOT="$LATEST_ROOT/next-actions"
TASK_BOARD_STATE_ROOT="$LATEST_ROOT/task-board-state"
CONTROL_PLANE_FIXTURE_ROOT="$LATEST_ROOT/control-plane-fixture-consumer-smoke"
SUMMARY="$LATEST_ROOT/summary.json"
MARKDOWN="$LATEST_ROOT/closure-packet.md"

mkdir -p "$LOG_DIR" "$TASK_BOARD_ROOT" "$NEXT_ACTIONS_ROOT" "$TASK_BOARD_STATE_ROOT" "$CONTROL_PLANE_FIXTURE_ROOT"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" pulse_generate_next \
  env AO2_PULSE_GENERATE_NEXT_REGISTER=0 \
    AO2_PULSE_TASK_BOARD_ROOT="$TASK_BOARD_ROOT" \
    npm run pulse:generate-next

ao2_gate_run_step "$LOG_DIR" pulse_next_actions \
  env AO2_PULSE_NEXT_ACTIONS_BOARD="$TASK_BOARD" \
    AO2_PULSE_NEXT_ACTIONS_ROOT="$NEXT_ACTIONS_ROOT" \
    npm run pulse:next-actions

ao2_gate_run_step "$LOG_DIR" pulse_task_board_state \
  env AO2_PULSE_TASK_BOARD_STATE_BOARD="$TASK_BOARD" \
    AO2_PULSE_TASK_BOARD_STATE_ROOT="$TASK_BOARD_STATE_ROOT" \
    npm run pulse:task-board-state

ao2_gate_run_step "$LOG_DIR" control_plane_fixture_consumer_smoke \
  env AO2_CP_FIXTURE_CONSUMER_TASK_BOARD="$TASK_BOARD" \
    AO2_CP_FIXTURE_CONSUMER_SMOKE_ROOT="$CONTROL_PLANE_FIXTURE_ROOT" \
    npm run control-plane:fixture-consumer-smoke

python3 - "$LATEST_ROOT" "$LOG_DIR" "$TASK_BOARD" "$NEXT_ACTIONS_ROOT/summary.json" "$TASK_BOARD_STATE_ROOT/summary.json" "$CONTROL_PLANE_FIXTURE_ROOT/summary.json" "$SUMMARY" "$MARKDOWN" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

latest_root = Path(sys.argv[1]).resolve()
log_dir = Path(sys.argv[2]).resolve()
task_board_path = Path(sys.argv[3]).resolve()
next_actions_path = Path(sys.argv[4]).resolve()
task_board_state_path = Path(sys.argv[5]).resolve()
control_plane_path = Path(sys.argv[6]).resolve()
summary_path = Path(sys.argv[7]).resolve()
markdown_path = Path(sys.argv[8]).resolve()

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def load_json(path: Path) -> dict:
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))

def exit_code(name: str) -> int:
    path = log_dir / f"{name}.log.exit-code"
    if not path.is_file():
        return 999
    return int(path.read_text(encoding="utf-8").strip())

task_board = load_json(task_board_path)
next_actions = load_json(next_actions_path)
task_board_state = load_json(task_board_state_path)
control_plane = load_json(control_plane_path)

tasks = [item for item in task_board.get("tasks", []) if isinstance(item, dict)]
board_ids = {str(item.get("task_id")) for item in tasks if item.get("task_id")}
next_action_items = [
    item for item in next_actions.get("next_actions", []) if isinstance(item, dict)
]
next_action_ids = {
    str(item.get("task_id")) for item in next_action_items if item.get("task_id")
}
state_items = [
    item for item in task_board_state.get("next_actions", []) if isinstance(item, dict)
]
state_ids = {str(item.get("task_id")) for item in state_items if item.get("task_id")}

action_summaries = []
for item in next_action_items:
    required_evidence = item.get("required_evidence")
    stop_conditions = item.get("stop_conditions")
    action_summaries.append({
        "task_id": item.get("task_id"),
        "stable_task_id": item.get("stable_task_id"),
        "status": item.get("status"),
        "next_action": item.get("next_action"),
        "required_evidence_count": len(required_evidence) if isinstance(required_evidence, list) else 0,
        "stop_conditions_count": len(stop_conditions) if isinstance(stop_conditions, list) else 0,
    })

step_exit_codes = {
    "pulse_generate_next": exit_code("pulse_generate_next"),
    "pulse_next_actions": exit_code("pulse_next_actions"),
    "pulse_task_board_state": exit_code("pulse_task_board_state"),
    "control_plane_fixture_consumer_smoke": exit_code("control_plane_fixture_consumer_smoke"),
}
safety_fields_preserved = bool(action_summaries) and all(
    item["required_evidence_count"] > 0 and item["stop_conditions_count"] > 0
    for item in action_summaries
)
task_ids_match = (
    bool(board_ids)
    and next_action_ids.issubset(board_ids)
    and state_ids.issubset(board_ids)
)
schemas_ok = (
    task_board.get("schema_version") == "ao2.ai-task-board.v1"
    and next_actions.get("schema_version") == "ao2.pulse-next-actions.v1"
    and task_board_state.get("schema_version") == "ao2.pulse-task-board-state.v1"
    and control_plane.get("schema_version") == "ao2.control-plane-fixture-consumer-smoke.v1"
)
component_statuses_ok = (
    next_actions.get("status") == "passed"
    and task_board_state.get("status") == "passed"
    and control_plane.get("status") == "passed"
)
operator_view = control_plane.get("operator_task_board_view", {})
control_plane_readback = control_plane.get("task_board_readback", {})
control_plane_ok = (
    control_plane_readback.get("status") == "passed"
    and operator_view.get("status") == "passed"
)
steps_ok = all(code == 0 for code in step_exit_codes.values())
status = "passed" if all([
    steps_ok,
    schemas_ok,
    component_statuses_ok,
    task_ids_match,
    safety_fields_preserved,
    control_plane_ok,
]) else "failed"

payload = {
    "schema_version": "ao2.pulse-task-board-closure-packet.v1",
    "generated_at_utc": utc_now(),
    "status": status,
    "artifact_root": str(latest_root),
    "task_count": len(tasks),
    "next_actions": action_summaries,
    "alignment": {
        "task_ids_match": task_ids_match,
        "board_task_ids": sorted(board_ids),
        "next_action_task_ids": sorted(next_action_ids),
        "state_task_ids": sorted(state_ids),
        "safety_fields_preserved": safety_fields_preserved,
    },
    "checks": {
        "step_exit_codes": step_exit_codes,
        "task_board": {
            "schema_version": task_board.get("schema_version"),
            "status": "passed" if task_board.get("schema_version") == "ao2.ai-task-board.v1" else "failed",
            "path": str(task_board_path),
        },
        "next_actions": {
            "schema_version": next_actions.get("schema_version"),
            "status": next_actions.get("status"),
            "path": str(next_actions_path),
        },
        "task_board_state": {
            "schema_version": task_board_state.get("schema_version"),
            "status": task_board_state.get("status"),
            "path": str(task_board_state_path),
        },
        "control_plane_fixture_consumer": {
            "schema_version": control_plane.get("schema_version"),
            "status": control_plane.get("status"),
            "task_board_readback_status": control_plane_readback.get("status"),
            "operator_task_board_view_status": operator_view.get("status"),
            "path": str(control_plane_path),
        },
    },
    "exports": {
        "markdown": str(markdown_path),
        "task_board": str(task_board_path),
        "next_actions": str(next_actions_path),
        "task_board_state": str(task_board_state_path),
        "control_plane_fixture_consumer": str(control_plane_path),
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "mutates_releases": False,
        "control_plane_approves_release": False,
    },
}

lines = [
    "# Pulse Task Board Closure Packet",
    "",
    f"Status: `{status}`",
    f"Task count: `{len(tasks)}`",
    "",
    "## Checks",
]
for name, code in step_exit_codes.items():
    lines.append(f"- `{name}` exit code: `{code}`")
lines.extend([
    f"- Task IDs match: `{task_ids_match}`",
    f"- Safety fields preserved: `{safety_fields_preserved}`",
    f"- Control-plane readback: `{control_plane_readback.get('status')}`",
    f"- Operator task-board view: `{operator_view.get('status')}`",
    "",
    "## Next Actions",
])
for item in action_summaries:
    lines.append(
        f"- `{item.get('task_id')}` next_action=`{item.get('next_action')}` "
        f"required_evidence={item['required_evidence_count']} "
        f"stop_conditions={item['stop_conditions_count']}"
    )

markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
