#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_NEXT_TASK_QUALITY_ROOT:-$ROOT/target/pulse-next-task-quality-filter/latest}"
SUMMARY="$OUT_ROOT/summary.json"
PACKET="${AO2_PULSE_NEXT_TASK_QUALITY_PACKET:-$ROOT/target/pulse-next-recommended-tasks/packet.md}"
TASK_BOARD="${AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD:-$(dirname "$PACKET")/task-board.json}"
STATUS_EVIDENCE="${AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE:-}"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$PACKET" "$TASK_BOARD" "$STATUS_EVIDENCE" "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

packet = Path(sys.argv[1]).resolve()
task_board_path = Path(sys.argv[2]).resolve()
status_evidence_arg = sys.argv[3]
status_evidence_path = Path(status_evidence_arg).resolve() if status_evidence_arg else None
out_root = Path(sys.argv[4]).resolve()
summary_path = Path(sys.argv[5]).resolve()
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
task_board = {}
task_board_task_ids = set()
task_board_task_id_matches = {}
task_board_generation = None
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
    source_recommendation = task_board.get("source_recommendation")
    if isinstance(source_recommendation, dict):
        task_board_generation = source_recommendation.get("generation")
    tasks = task_board.get("tasks")
    if not isinstance(tasks, list) or not tasks:
        task_board_blockers.append("task_board_tasks_missing")
    else:
        for index, task in enumerate(tasks, start=1):
            if not isinstance(task, dict):
                task_board_blockers.append(f"task_not_object:{index}")
                continue
            task_id = str(task.get("task_id") or task.get("id") or f"index-{index}")
            task_board_task_ids.add(task_id)
            task_board_task_id_matches[task_id] = {
                "task_id": task_id,
                "stable_task_id": str(task.get("stable_task_id") or "").strip() or None,
                "matched_by": "task_id",
            }
            stable_task_id = str(task.get("stable_task_id") or "").strip()
            if stable_task_id:
                task_board_task_ids.add(stable_task_id)
                task_board_task_id_matches[stable_task_id] = {
                    "task_id": task_id,
                    "stable_task_id": stable_task_id,
                    "matched_by": "stable_task_id",
                }
            required_evidence = task.get("required_evidence") or task.get("evidence_requirements")
            stop_conditions = task.get("stop_conditions")
            if not isinstance(required_evidence, list) or not any(str(item).strip() for item in required_evidence):
                task_board_blockers.append(f"task_missing_required_evidence:{task_id}")
            if not isinstance(stop_conditions, list) or not any(str(item).strip() for item in stop_conditions):
                task_board_blockers.append(f"task_missing_stop_conditions:{task_id}")
    if task_board_blockers:
        task_board_drift_gate = "failed"
status_evidence_blockers = []
status_evidence_gate = "skipped"
status_evidence_matches = []
status_evidence_match_counts = {"task_id": 0, "stable_task_id": 0}
if status_evidence_path and status_evidence_path.is_file():
    status_evidence_gate = "passed"
    try:
        status_evidence = json.loads(status_evidence_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        status_evidence = {}
        status_evidence_blockers.append(f"status_evidence_invalid_json:{exc.lineno}")
    if status_evidence.get("schema_version") != "ao2.ai-task-board-status-evidence.v1":
        status_evidence_blockers.append("status_evidence_schema_invalid")
    evidence_generation = status_evidence.get("task_board_generation")
    if (
        task_board_generation is not None
        and evidence_generation != task_board_generation
    ):
        status_evidence_blockers.append(
            f"status_evidence_stale_generation:{evidence_generation}!={task_board_generation}"
        )
    task_statuses = status_evidence.get("task_statuses")
    if not isinstance(task_statuses, dict) or not task_statuses:
        status_evidence_blockers.append("status_evidence_task_statuses_missing")
    else:
        for task_id in sorted(str(key) for key in task_statuses):
            if task_board_task_ids and task_id not in task_board_task_ids:
                status_evidence_blockers.append(f"status_evidence_unknown_task_id:{task_id}")
            elif task_id in task_board_task_id_matches:
                match = dict(task_board_task_id_matches[task_id])
                match["evidence_task_id"] = task_id
                status_evidence_matches.append(match)
                matched_by = str(match["matched_by"])
                status_evidence_match_counts[matched_by] = status_evidence_match_counts.get(matched_by, 0) + 1
    if status_evidence_blockers:
        status_evidence_gate = "failed"
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
    "status_evidence": str(status_evidence_path) if status_evidence_path else None,
    "task_board_drift_gate": task_board_drift_gate,
    "task_board_blockers": task_board_blockers,
    "status_evidence_gate": status_evidence_gate,
    "status_evidence_blockers": status_evidence_blockers,
    "status_evidence_matches": status_evidence_matches,
    "status_evidence_match_counts": status_evidence_match_counts,
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
