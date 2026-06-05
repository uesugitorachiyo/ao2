#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_NEXT_LENGTHY_GATE_ROOT:-$ROOT/target/next-lengthy-gate/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="${AO2_NEXT_LENGTHY_GATE_FIXTURE_DIR:-}"

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

run_step mvp_acceptance_matrix \
  env AO2_MVP_ACCEPTANCE_MATRIX_ROOT="$OUT_ROOT/mvp-acceptance-matrix" \
    npm run mvp:acceptance-matrix-gate

run_step no_archaeology_workbench \
  env AO2_NO_ARCHAEOLOGY_WORKBENCH_ROOT="$OUT_ROOT/no-archaeology-workbench" \
    npm run workbench:no-archaeology-audit

run_step control_plane_observer \
  env AO2_CP_OBSERVER_HARDENING_ROOT="$OUT_ROOT/control-plane-observer-hardening" \
    npm run control-plane:observer-hardening

run_step provider_phase2_contract \
  env AO2_PROVIDER_PHASE2_HARDENING_ROOT="$OUT_ROOT/provider-phase2-contract-hardening" \
    npm run provider:phase2-contract-hardening

release_env=(env AO2_PUBLIC_RELEASE_TRAIN_DRILL_ROOT="$OUT_ROOT/public-release-train-drill")
if [ -n "$FIXTURE_DIR" ]; then
  release_env+=(AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR="$FIXTURE_DIR")
fi
run_step public_release_train \
  "${release_env[@]}" npm run release:train-drill

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
components = [
    ("mvp_acceptance_matrix", "mvp-acceptance-matrix/summary.json"),
    ("no_archaeology_workbench", "no-archaeology-workbench/summary.json"),
    ("control_plane_observer", "control-plane-observer-hardening/summary.json"),
    ("provider_phase2_contract", "provider-phase2-contract-hardening/summary.json"),
    ("public_release_train", "public-release-train-drill/summary.json"),
]
checks = []
component_summaries = {}
for name, rel_summary in components:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    summary = out_root / rel_summary
    component_summaries[name] = str(summary)
    checks.append({
        "name": name,
        "status": "passed" if code == 0 else "failed",
        "exit_code": code,
        "log": str(log_dir / f"{name}.log"),
        "summary": str(summary),
    })
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.next-lengthy-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "component_summaries": component_summaries,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
