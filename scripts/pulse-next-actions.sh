#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_NEXT_ACTIONS_ROOT:-$ROOT/target/pulse-next-actions/latest}"
BOARD="${AO2_PULSE_NEXT_ACTIONS_BOARD:-$ROOT/target/pulse-task-board/latest/summary.json}"
STATUS_FILTER="${AO2_PULSE_NEXT_ACTIONS_STATUS:-}"
SUMMARY="$OUT_ROOT/summary.json"
MARKDOWN="$OUT_ROOT/next-actions.md"

mkdir -p "$OUT_ROOT"

python3 - "$BOARD" "$OUT_ROOT" "$SUMMARY" "$MARKDOWN" "$STATUS_FILTER" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

board_path = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
markdown_path = Path(sys.argv[4]).resolve()
status_filter_raw = sys.argv[5]

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

payload = {
    "schema_version": "ao2.pulse-next-actions.v1",
    "generated_at_utc": utc_now(),
    "status": "failed",
    "artifact_root": str(out_root),
    "task_board": str(board_path),
    "status_filter": [
        value.strip().lower().replace("-", "_")
        for value in status_filter_raw.split(",")
        if value.strip()
    ],
    "next_actions": [],
    "exports": {"markdown": str(markdown_path)},
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
        actions = []
        status_filter = set(payload["status_filter"])
        rsi_operator_closure_readback = board.get("rsi_operator_closure_readback")
        if not isinstance(rsi_operator_closure_readback, dict):
            rsi_operator_closure_readback = {}
        rsi_claim_boundary = board.get("rsi_claim_boundary")
        if not isinstance(rsi_claim_boundary, dict):
            rsi_claim_boundary = {}
        for item in board.get("tasks", []):
            if not isinstance(item, dict):
                continue
            status = str(item.get("status") or "unknown").lower().replace("-", "_")
            if status_filter and status not in status_filter:
                continue
            actions.append({
                "task_id": item.get("task_id"),
                "stable_task_id": item.get("stable_task_id"),
                "title": item.get("title"),
                "status": status,
                "next_action": item.get("next_action"),
                "rationale": item.get("rationale"),
                "required_evidence": item.get("required_evidence") if isinstance(item.get("required_evidence"), list) else [],
                "stop_conditions": item.get("stop_conditions") if isinstance(item.get("stop_conditions"), list) else [],
                "rsi_operator_closure_readback": item.get("rsi_operator_closure_readback") if isinstance(item.get("rsi_operator_closure_readback"), dict) else rsi_operator_closure_readback,
                "rsi_claim_boundary": item.get("rsi_claim_boundary") if isinstance(item.get("rsi_claim_boundary"), dict) else rsi_claim_boundary,
            })
        payload.update({
            "status": "passed",
            "reason": "next_actions_read",
            "task_count": len(actions),
            "next_actions": actions,
            "rsi_operator_closure_readback": rsi_operator_closure_readback,
            "rsi_claim_boundary": rsi_claim_boundary,
        })

lines = ["# Next Actions", ""]
if payload["status"] != "passed":
    lines.append(f"Reason: {payload.get('reason')}")
    lines.append("")
elif payload.get("rsi_claim_boundary"):
    boundary = payload["rsi_claim_boundary"]
    lines.extend([
        "## RSI Claim Boundary",
        "",
        f"- bounded_governed_rsi: `{boundary.get('bounded_governed_rsi')}`",
        f"- full_autonomous_self_mutating_rsi: `{boundary.get('full_autonomous_self_mutating_rsi')}`",
        f"- claim_publish_decision: `{boundary.get('claim_publish_decision')}`",
        f"- claim_publish_authority: `{str(boundary.get('claim_publish_authority')).lower()}`",
        f"- operator_closure_is_publication_authority: `{str(boundary.get('operator_closure_is_publication_authority')).lower()}`",
        "",
    ])
for item in payload["next_actions"]:
    lines.append(
        f"- `{item.get('task_id')}` [{item.get('status')}]: "
        f"{item.get('title')} -> next_action: `{item.get('next_action')}`"
    )
    if item.get("rationale"):
        lines.append(f"  - Rationale: {item.get('rationale')}")
    if item.get("required_evidence"):
        lines.append("  - Required evidence:")
        for evidence in item.get("required_evidence") or []:
            lines.append(f"    - `{evidence}`")
    if item.get("stop_conditions"):
        lines.append("  - Stop conditions:")
        for condition in item.get("stop_conditions") or []:
            lines.append(f"    - {condition}")
markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
for item in payload["next_actions"]:
    print(
        f"next_action {item.get('task_id')} {item.get('status')} "
        f"{item.get('next_action')}"
    )
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
