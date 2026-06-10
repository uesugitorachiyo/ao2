#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_RELEASE_READINESS_REGRESSION_ROOT:-$ROOT/target/release-readiness-regression-gate/$(date -u +%Y%m%dT%H%M%SZ)}"
SUMMARY="$OUT_ROOT/summary.json"
# Default control-plane smoke: ../ao2-control-plane/scripts/smoke-long-lived-dev.sh

mkdir -p "$OUT_ROOT"

run_step() {
  name="$1"
  log="$2"
  shift 2
  echo "step=$name"
  set +e
  "$@" >"$log" 2>&1
  status="$?"
  set -e
  printf "%s\n" "$status" >"$log.exit-code"
  return 0
}

run_step release_readiness_static "$OUT_ROOT/release-readiness-static.log" \
  env AO2_RELEASE_READINESS_ROOT="$OUT_ROOT/release-readiness-static" npm run release:readiness:static
release_status="$(cat "$OUT_ROOT/release-readiness-static.log.exit-code")"

run_step phase1_operator_golden "$OUT_ROOT/phase1-operator-golden.log" \
  env AO2_PHASE1_OPERATOR_SMOKE_ROOT="$OUT_ROOT/phase1-operator-golden" npm run smoke:phase1-operator-golden
phase1_status="$(cat "$OUT_ROOT/phase1-operator-golden.log.exit-code")"

run_step pulse_local_mirror "$OUT_ROOT/pulse-local-mirror.log" \
  env AO2_PULSE_LOCAL_MIRROR_DEST="$ROOT/.ao2-local/pulse/latest" npm run pulse:local-mirror
pulse_status="$(cat "$OUT_ROOT/pulse-local-mirror.log.exit-code")"

run_step pulse_resume_dry_run "$OUT_ROOT/pulse-resume-dry-run.log" \
  env AO2_PULSE_RESUME_JSON="$ROOT/.ao2-local/pulse/latest/resume.json" \
    AO2_PULSE_RESUME_ROOT="$OUT_ROOT/pulse-resume" \
    npm run pulse:resume -- --dry-run
pulse_resume_status="$(cat "$OUT_ROOT/pulse-resume-dry-run.log.exit-code")"

run_step ci_artifact_download_contract "$OUT_ROOT/ci-artifact-download-contract.log" \
  env AO2_CI_ARTIFACT_DOWNLOAD_ROOT="$ROOT/target/ci-artifacts/latest" \
    npm run artifacts:ci-download-contract
ci_artifact_status="$(cat "$OUT_ROOT/ci-artifact-download-contract.log.exit-code")"

run_step artifact_index "$OUT_ROOT/artifact-index.log" \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" npm run artifacts:index
artifact_status="$(cat "$OUT_ROOT/artifact-index.log.exit-code")"

run_step artifact_health "$OUT_ROOT/artifact-health.log" \
  env \
    AO2_ARTIFACT_HEALTH_INDEX="$OUT_ROOT/artifact-index/artifact-index.json" \
    AO2_ARTIFACT_HEALTH_ROOT="$OUT_ROOT/artifact-health" \
    AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS="ao2/target/ci-artifacts ao2/.ao2-local/pulse/latest ao2-control-plane/target/ci-artifacts" \
    AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS="ao2/target/release-readiness-ci ao2/target/release-evidence-closure ao2/target/release-readiness-regression-gate ao2/target/phase1-promotion-golden ao2/target/pulse-real-execute-containment ao2-control-plane/target/dr-restore-drill" \
    AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1 \
    npm run artifacts:health
artifact_health_status="$(cat "$OUT_ROOT/artifact-health.log.exit-code")"

run_step release_artifact_consumer_smoke "$OUT_ROOT/release-artifact-consumer-smoke.log" \
  env AO2_RELEASE_ARTIFACT_CONSUMER_ROOT="$OUT_ROOT/release-artifact-consumer-smoke" \
    npm run release:artifact-consumer-smoke -- --dry-run
consumer_status="$(cat "$OUT_ROOT/release-artifact-consumer-smoke.log.exit-code")"

if [ -x "$CP_ROOT/scripts/smoke-long-lived-dev.sh" ]; then
  run_step control_plane_long_lived_smoke "$OUT_ROOT/control-plane-long-lived-smoke.log" \
    env AO2_CP_LONG_LIVED_SMOKE_ROOT="$OUT_ROOT/control-plane-long-lived-smoke" \
      "$CP_ROOT/scripts/smoke-long-lived-dev.sh"
  cp_status="$(cat "$OUT_ROOT/control-plane-long-lived-smoke.log.exit-code")"
else
  cp_status="127"
  printf "missing control-plane smoke script: %s\n" "$CP_ROOT/scripts/smoke-long-lived-dev.sh" \
    >"$OUT_ROOT/control-plane-long-lived-smoke.log"
  printf "%s\n" "$cp_status" >"$OUT_ROOT/control-plane-long-lived-smoke.log.exit-code"
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$release_status" "$phase1_status" "$pulse_status" "$pulse_resume_status" "$ci_artifact_status" "$artifact_status" "$artifact_health_status" "$consumer_status" "$cp_status" <<'PY'
import json
import sys
from pathlib import Path

out_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
codes = {
    "release_readiness_static": int(sys.argv[3]),
    "phase1_operator_golden": int(sys.argv[4]),
    "pulse_local_mirror": int(sys.argv[5]),
    "pulse_resume_dry_run": int(sys.argv[6]),
    "ci_artifact_download_contract": int(sys.argv[7]),
    "artifact_index": int(sys.argv[8]),
    "artifact_health": int(sys.argv[9]),
    "release_artifact_consumer_smoke": int(sys.argv[10]),
    "control_plane_long_lived_smoke": int(sys.argv[11]),
}

checks = []
for name, exit_code in codes.items():
    log = out_root / f"{name.replace('_', '-')}.log"
    checks.append({
        "name": name,
        "status": "passed" if exit_code == 0 else "failed",
        "exit_code": exit_code,
        "log": str(log),
    })

payload = {
    "schema_version": "ao2.release-readiness-regression-gate.v1",
    "status": "passed" if all(code == 0 for code in codes.values()) else "failed",
    "artifact_root": str(out_root),
    "checks": checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
