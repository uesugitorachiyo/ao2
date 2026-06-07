#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_CP_OBSERVER_HARDENING_ROOT:-$ROOT/target/control-plane-observer-hardening/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
CP_RESTORE_ROOT="$CP_ROOT/target/dr-restore-drill/control-plane-observer-hardening"

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

run_step evidence_control_plane_smoke \
  env AO2_EVIDENCE_CP_SMOKE_ROOT="$OUT_ROOT/evidence-control-plane-smoke" \
    npm run smoke:evidence-control-plane

run_step operator_packet_control_plane_smoke \
  env AO2_OPERATOR_PACKET_CP_SMOKE_ROOT="$OUT_ROOT/operator-packet-control-plane-smoke" \
    npm run smoke:operator-packet-control-plane

run_step workbench_operator_packet_control_plane_smoke \
  env AO2_WORKBENCH_OPERATOR_PACKET_CP_SMOKE_ROOT="$OUT_ROOT/workbench-operator-packet-control-plane-smoke" \
    npm run smoke:workbench-operator-packet-control-plane

run_step negative_restore_drill \
  "$CP_ROOT/scripts/cp-dr-restore-drill.sh" \
    --negative-only \
    --work-dir "$CP_RESTORE_ROOT" \
    --out "$CP_RESTORE_ROOT/dr-restore-report.json"

if [ -x "$CP_ROOT/scripts/smoke-long-lived-dev.sh" ]; then
  run_step long_lived_smoke \
    env AO2_CP_LONG_LIVED_SMOKE_ROOT="$OUT_ROOT/long-lived-smoke" \
      "$CP_ROOT/scripts/smoke-long-lived-dev.sh"
else
  printf "missing control-plane long-lived smoke\n" >"$LOG_DIR/long_lived_smoke.log"
  printf "127\n" >"$LOG_DIR/long_lived_smoke.log.exit-code"
fi

run_step artifact_index \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" npm run artifacts:index

run_step artifact_health \
  env \
    AO2_ARTIFACT_HEALTH_INDEX="$OUT_ROOT/artifact-index/artifact-index.json" \
    AO2_ARTIFACT_HEALTH_ROOT="$OUT_ROOT/artifact-health" \
    AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS="ao2-control-plane/target/dr-restore-drill" \
    AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS="ao2/target/ci-artifacts ao2/target/release-readiness-ci ao2/target/release-readiness-regression-gate ao2/target/release-evidence-closure ao2/target/phase1-promotion-golden ao2/target/pulse-real-execute-containment ao2/.ao2-local/pulse/latest ao2-control-plane/target/ci-artifacts" \
    AO2_ARTIFACT_HEALTH_STALE_AFTER_SECONDS=315360000 \
    AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1 \
    npm run artifacts:health

python3 - "$OUT_ROOT" "$SUMMARY" "$CP_RESTORE_ROOT" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
cp_restore_root = Path(sys.argv[3]).resolve()
log_dir = out_root / "logs"
names = [
    "evidence_control_plane_smoke",
    "operator_packet_control_plane_smoke",
    "workbench_operator_packet_control_plane_smoke",
    "negative_restore_drill",
    "long_lived_smoke",
    "artifact_index",
    "artifact_health",
]
checks = []
for name in names:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({
        "name": name,
        "status": "passed" if code == 0 else "failed",
        "exit_code": code,
        "log": str(log_dir / f"{name}.log"),
    })

smoke_summary = out_root / "evidence-control-plane-smoke" / "summary.json"
smoke = json.loads(smoke_summary.read_text(encoding="utf-8")) if smoke_summary.exists() else {}
operator_packet_summary = out_root / "operator-packet-control-plane-smoke" / "summary.json"
operator_packet_smoke = json.loads(operator_packet_summary.read_text(encoding="utf-8")) if operator_packet_summary.exists() else {}
workbench_operator_packet_summary = out_root / "workbench-operator-packet-control-plane-smoke" / "summary.json"
workbench_operator_packet_smoke = json.loads(workbench_operator_packet_summary.read_text(encoding="utf-8")) if workbench_operator_packet_summary.exists() else {}
observer_checks = {
    "dashboard_schema_stability": smoke.get("dashboard_schema_version") == "ao2.cp-evidence-pack-dashboard.v1",
    "operator_packet_dashboard_schema_stability": operator_packet_smoke.get("contract_schemas", {}).get("dashboard") == "ao2.cp-operator-packet-dashboard.v1",
    "operator_packet_read_only_observer": operator_packet_smoke.get("read_only_observer") is True,
    "operator_packet_can_approve_runs": operator_packet_smoke.get("can_approve_runs") is False,
    "operator_packet_can_mutate_ao2_evidence": operator_packet_smoke.get("can_mutate_ao2_evidence") is False,
    "workbench_operator_packet_dashboard_schema_stability": workbench_operator_packet_smoke.get("contract_schemas", {}).get("dashboard") == "ao2.cp-operator-packet-dashboard.v1",
    "workbench_operator_packet_read_only_observer": workbench_operator_packet_smoke.get("read_only_observer") is True,
    "workbench_operator_packet_can_approve_runs": workbench_operator_packet_smoke.get("can_approve_runs") is False,
    "workbench_operator_packet_can_mutate_ao2_evidence": workbench_operator_packet_smoke.get("can_mutate_ao2_evidence") is False,
    "workbench_operator_packet_real_run_evidence": workbench_operator_packet_smoke.get("operator_packet", {}).get("evidence_pack_schema_version") == "ao2.evidence-pack.v1",
    "read_only_observer": smoke.get("read_only_observer") is True,
    "can_approve_runs": smoke.get("can_approve_runs") is False,
    "can_mutate_ao2_evidence": smoke.get("can_mutate_ao2_evidence") is False,
    "restore_drill_evidence": (cp_restore_root / "dr-restore-report.json").exists(),
}
status = "passed" if all(item["exit_code"] == 0 for item in checks) and all(observer_checks.values()) else "failed"
payload = {
    "schema_version": "ao2.control-plane-observer-hardening.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "observer_checks": observer_checks,
    "component_summaries": {
        "evidence_control_plane_smoke": str(smoke_summary),
        "operator_packet_control_plane_smoke": str(operator_packet_summary),
        "workbench_operator_packet_control_plane_smoke": str(workbench_operator_packet_summary),
        "negative_restore_drill": str(cp_restore_root / "dr-restore-report.json"),
        "artifact_health": str(out_root / "artifact-health" / "summary.json"),
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
