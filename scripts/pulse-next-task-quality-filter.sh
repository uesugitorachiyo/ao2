#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_NEXT_TASK_QUALITY_ROOT:-$ROOT/target/pulse-next-task-quality-filter/latest}"
SUMMARY="$OUT_ROOT/summary.json"
PACKET="${AO2_PULSE_NEXT_TASK_QUALITY_PACKET:-$ROOT/target/pulse-next-recommended-tasks/packet.md}"
TASK_BOARD="${AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD:-$(dirname "$PACKET")/task-board.json}"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$PACKET" "$TASK_BOARD" "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

packet = Path(sys.argv[1]).resolve()
task_board_path = Path(sys.argv[2]).resolve()
out_root = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
text = packet.read_text(encoding="utf-8") if packet.is_file() else ""
task_titles = re.findall(r"^## \d+\. (.+)$", text, flags=re.MULTILINE)
task_titles.extend(re.findall(r"^- `[^`]+`: ([^-]+?) - .+$", text, flags=re.MULTILINE))
product_slice_keywords = [
    "risky-pr",
    "risky pr",
    "product loop",
    "local run record",
    "static report",
    "static report/export",
    "evaluator closure",
    "release readiness",
    "cross-os",
    "ubuntu",
    "macos",
    "windows",
    "ci",
    "control-plane",
    "operator cockpit",
    "evidence dashboard",
    "workbench",
    "provider contract",
    "artifact",
]
script_wrapper_keywords = [
    "shell wrapper",
    "wrapper",
    "script tracking",
    "runbook",
    "matrix",
    "baseline",
    "watchdog",
    "lock",
    "consolidation",
    "manifest only",
    "proof of proof",
    "lengthy gate",
]
lowered_packet = text.lower()
has_product_slice = any(keyword in lowered_packet for keyword in product_slice_keywords)
quality_items = []
for title in task_titles:
    lowered = title.lower()
    manifest_only_recursion = any(word in lowered for word in ["manifest only", "proof of proof"])
    consolidation_bias = any(word in lowered for word in ["consolidation", "index", "runbook", "matrix", "baseline", "watchdog", "lock"])
    script_wrapper_bias = any(word in lowered for word in script_wrapper_keywords)
    product_slice = any(word in lowered for word in product_slice_keywords)
    coverage_gain = "high" if product_slice else ("medium" if consolidation_bias or script_wrapper_bias else "low")
    quality_score = 85 if product_slice else (55 if script_wrapper_bias and has_product_slice else (45 if script_wrapper_bias else 65))
    if manifest_only_recursion:
        quality_score -= 30
    quality_items.append({
        "title": title,
        "coverage_gain": coverage_gain,
        "manifest_only_recursion": manifest_only_recursion,
        "consolidation_bias": consolidation_bias,
        "script_wrapper_bias": script_wrapper_bias,
        "product_slice": product_slice,
        "quality_score": quality_score,
    })
script_wrapper_recursion_block = bool(task_titles) and not has_product_slice and any(
    item["script_wrapper_bias"] or item["consolidation_bias"] or item["manifest_only_recursion"]
    for item in quality_items
)
task_board_blockers = []
task_board_drift_gate = "skipped"
if task_board_path.is_file():
    task_board_drift_gate = "passed"
    try:
        task_board = json.loads(task_board_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        task_board = {}
        task_board_blockers.append(f"task_board_invalid_json:{exc.lineno}")
    if task_board.get("schema_version") != "ao2.ai-task-board.v1":
        task_board_blockers.append("task_board_schema_invalid")
    if not str(task_board.get("release_objective") or "").strip():
        task_board_blockers.append("release_objective_missing")
    tasks = task_board.get("tasks")
    if not isinstance(tasks, list) or not tasks:
        task_board_blockers.append("task_board_tasks_missing")
    else:
        for index, task in enumerate(tasks, start=1):
            if not isinstance(task, dict):
                task_board_blockers.append(f"task_not_object:{index}")
                continue
            task_id = str(task.get("task_id") or task.get("id") or f"index-{index}")
            required_evidence = task.get("required_evidence") or task.get("evidence_requirements")
            stop_conditions = task.get("stop_conditions")
            if not isinstance(required_evidence, list) or not any(str(item).strip() for item in required_evidence):
                task_board_blockers.append(f"task_missing_required_evidence:{task_id}")
            if not isinstance(stop_conditions, list) or not any(str(item).strip() for item in stop_conditions):
                task_board_blockers.append(f"task_missing_stop_conditions:{task_id}")
    if task_board_blockers:
        task_board_drift_gate = "failed"
status = "passed" if (
    bool(task_titles)
    and has_product_slice
    and not script_wrapper_recursion_block
    and all(item["quality_score"] >= 50 for item in quality_items)
    and task_board_drift_gate != "failed"
) else "failed"
payload = {
    "schema_version": "ao2.pulse-next-task-quality-filter.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "packet": str(packet),
    "task_board": str(task_board_path),
    "task_board_drift_gate": task_board_drift_gate,
    "task_board_blockers": task_board_blockers,
    "coverage_gain": "measured_per_task",
    "manifest_only_recursion": any(item["manifest_only_recursion"] for item in quality_items),
    "consolidation_bias": any(item["consolidation_bias"] for item in quality_items),
    "script_wrapper_recursion_block": script_wrapper_recursion_block,
    "product_slice_coverage": "present" if has_product_slice else "missing",
    "quality_score": min([item["quality_score"] for item in quality_items] or [0]),
    "tasks": quality_items,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
