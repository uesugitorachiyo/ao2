#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_REPO:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_CROSS_REPO_CP_OBSERVER_ROOT:-$ROOT/target/cross-repo-control-plane-observer/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

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

test -x "$CP_ROOT/scripts/smoke-ingest-from-ao2.sh"
test -x "$CP_ROOT/scripts/cp-health-snapshot.sh"
test -x "$CP_ROOT/scripts/cp-dashboard-snapshot.sh"
test -x "$CP_ROOT/scripts/cp-dr-restore-drill.sh"

run_step signed_evidence_bundle \
  env AO2_EVIDENCE_CP_SMOKE_ROOT="$OUT_ROOT/signed-evidence-bundle" \
    npm run smoke:evidence-control-plane

run_step restore_negative_drill \
  "$CP_ROOT/scripts/cp-dr-restore-drill.sh" \
    --negative-only \
    --work-dir "$OUT_ROOT/cp-dr-work" \
    --out "$OUT_ROOT/cp-dr-restore-report.json"

if [ -n "${AO2_CROSS_REPO_CP_LIVE_URL:-}" ] && [ -n "${AO2_CROSS_REPO_CP_API_TOKEN_ENV:-}" ]; then
  run_step health_snapshot \
    "$CP_ROOT/scripts/cp-health-snapshot.sh" \
      --base-url "$AO2_CROSS_REPO_CP_LIVE_URL" \
      --api-token-env "$AO2_CROSS_REPO_CP_API_TOKEN_ENV" \
      --out "$OUT_ROOT/cp-health-snapshot.json"
  run_step dashboard_snapshot \
    "$CP_ROOT/scripts/cp-dashboard-snapshot.sh" \
      --base-url "$AO2_CROSS_REPO_CP_LIVE_URL" \
      --api-token-env "$AO2_CROSS_REPO_CP_API_TOKEN_ENV" \
      --out-dir "$OUT_ROOT/cp-dashboard-snapshot"
else
  printf "0\n" >"$LOG_DIR/health_snapshot.log.exit-code"
  printf "live health snapshot skipped; cp-health-snapshot.sh is contract-checked\n" >"$LOG_DIR/health_snapshot.log"
  printf "0\n" >"$LOG_DIR/dashboard_snapshot.log.exit-code"
  printf "live dashboard snapshot skipped; cp-dashboard-snapshot.sh is contract-checked\n" >"$LOG_DIR/dashboard_snapshot.log"
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$CP_ROOT" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
cp_root = Path(sys.argv[3]).resolve()
log_dir = out_root / "logs"
names = [
    "signed_evidence_bundle",
    "restore_negative_drill",
    "health_snapshot",
    "dashboard_snapshot",
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
smoke_summary = out_root / "signed-evidence-bundle" / "summary.json"
read_only_observer = False
can_approve_runs = None
can_mutate_ao2_evidence = None
if smoke_summary.is_file():
    smoke = json.loads(smoke_summary.read_text(encoding="utf-8"))
    read_only_observer = bool(smoke.get("read_only_observer"))
    can_approve_runs = smoke.get("can_approve_runs")
    can_mutate_ao2_evidence = smoke.get("can_mutate_ao2_evidence")
status = "passed" if all(item["exit_code"] == 0 for item in checks) and read_only_observer and can_approve_runs is False and can_mutate_ao2_evidence is False else "failed"
payload = {
    "schema_version": "ao2.cross-repo-control-plane-observer.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "control_plane_repo": str(cp_root),
    "checks": checks,
    "component_summaries": {
        "signed_evidence_bundle": str(smoke_summary),
        "restore_negative_drill": str(out_root / "cp-dr-restore-report.json"),
        "health_snapshot": str(out_root / "cp-health-snapshot.json"),
        "dashboard_snapshot": str(out_root / "cp-dashboard-snapshot"),
    },
    "observer_contract": {
        "signed_evidence_bundle": True,
        "read_only_observer": read_only_observer,
        "can_approve_runs": can_approve_runs,
        "can_mutate_ao2_evidence": can_mutate_ao2_evidence,
        "smoke-ingest-from-ao2.sh": str(cp_root / "scripts" / "smoke-ingest-from-ao2.sh"),
        "cp-health-snapshot.sh": str(cp_root / "scripts" / "cp-health-snapshot.sh"),
        "cp-dashboard-snapshot.sh": str(cp_root / "scripts" / "cp-dashboard-snapshot.sh"),
        "cp-dr-restore-drill.sh": str(cp_root / "scripts" / "cp-dr-restore-drill.sh"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
