#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_NO_ARCHAEOLOGY_WORKBENCH_ROOT:-$ROOT/target/no-archaeology-workbench/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
RISKY_ROOT="$OUT_ROOT/risky-pr-golden"
AO2_BIN="${AO2_BIN:-$ROOT/target/release/ao2}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

run_step() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  set +e
  "$@" >"$log" 2>&1
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

run_step risky_pr_golden \
  env AO2_RISKY_PR_GOLDEN_ROOT="$RISKY_ROOT" npm run risky-pr:golden

TARGET="$RISKY_ROOT/fixture/discount-service"
WORKBENCH_HTML="$OUT_ROOT/workbench.html"
if [ -x "$AO2_BIN" ] && [ -d "$TARGET" ]; then
  run_step workbench_export \
    "$AO2_BIN" workbench export --target "$TARGET" --out "$WORKBENCH_HTML"
else
  printf "missing ao2 binary or risky-run target\n" >"$LOG_DIR/workbench_export.log"
  printf "127\n" >"$LOG_DIR/workbench_export.log.exit-code"
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$RISKY_ROOT" "$WORKBENCH_HTML" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
risky_root = Path(sys.argv[3]).resolve()
workbench_html = Path(sys.argv[4]).resolve()
log_dir = out_root / "logs"

exit_codes = {
    name: int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    for name in ["risky_pr_golden", "workbench_export"]
}
risky_summary = risky_root / "summary.json"
risky = json.loads(risky_summary.read_text(encoding="utf-8")) if risky_summary.exists() else {}
evidence_pack = Path(risky.get("evidence_pack", ""))
report = Path(risky.get("report", ""))
cockpit_index = Path(risky.get("cockpit_index", ""))

texts = []
for path in [evidence_pack, report, cockpit_index, workbench_html, risky_summary]:
    if str(path) and path.exists():
        texts.append(path.read_text(encoding="utf-8", errors="replace"))
combined = "\n".join(texts).lower()

questions = {
    "objective": ["objective"],
    "denied_action": ["denied", "git push"],
    "approved_digest": ["approval"],
    "changed_files": ["changed", "files"],
    "test_evidence": ["test", "evidence"],
    "rejection_reason": ["rejected"],
    "correction": ["accepted"],
    "closure_verdict": ["verdict", "accepted"],
    "export_path": ["evidence-pack"],
    "replay_status": ["replay"],
}
answers = []
for question, needles in questions.items():
    passed = all(needle in combined for needle in needles)
    answers.append({
        "question": question,
        "status": "passed" if passed else "failed",
        "evidence_surfaces": [
            str(report),
            str(cockpit_index),
            str(workbench_html),
            str(evidence_pack),
        ],
        "manual_filesystem_archaeology_required": False,
    })

status = "passed" if all(code == 0 for code in exit_codes.values()) and all(item["status"] == "passed" for item in answers) else "failed"
payload = {
    "schema_version": "ao2.no-archaeology-workbench-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "answers": answers,
    "workbench_export": str(workbench_html),
    "component_summaries": {"risky_pr_golden": str(risky_summary)},
    "manual_filesystem_archaeology_required": False,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
