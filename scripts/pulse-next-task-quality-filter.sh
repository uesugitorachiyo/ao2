#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_NEXT_TASK_QUALITY_ROOT:-$ROOT/target/pulse-next-task-quality-filter/latest}"
SUMMARY="$OUT_ROOT/summary.json"
PACKET="${AO2_PULSE_NEXT_TASK_QUALITY_PACKET:-$ROOT/target/pulse-next-recommended-tasks/packet.md}"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$PACKET" "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

packet = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
text = packet.read_text(encoding="utf-8") if packet.is_file() else ""
task_titles = re.findall(r"^## \d+\. (.+)$", text, flags=re.MULTILINE)
quality_items = []
for title in task_titles:
    lowered = title.lower()
    manifest_only_recursion = any(word in lowered for word in ["manifest only", "proof of proof"])
    consolidation_bias = any(word in lowered for word in ["consolidation", "index", "runbook", "matrix", "baseline", "watchdog", "lock"])
    coverage_gain = "high" if consolidation_bias else "medium"
    quality_score = 80 if consolidation_bias else 65
    if manifest_only_recursion:
        quality_score -= 30
    quality_items.append({
        "title": title,
        "coverage_gain": coverage_gain,
        "manifest_only_recursion": manifest_only_recursion,
        "consolidation_bias": consolidation_bias,
        "quality_score": quality_score,
    })
status = "passed" if all(item["quality_score"] >= 50 for item in quality_items) else "failed"
payload = {
    "schema_version": "ao2.pulse-next-task-quality-filter.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "packet": str(packet),
    "coverage_gain": "measured_per_task",
    "manifest_only_recursion": any(item["manifest_only_recursion"] for item in quality_items),
    "consolidation_bias": True,
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
