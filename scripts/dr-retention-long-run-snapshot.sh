#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_REPO:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_DR_RETENTION_SNAPSHOT_ROOT:-$ROOT/target/dr-retention-long-run-snapshot/latest}"
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

run_step restore_drill_evidence \
  "$CP_ROOT/scripts/cp-dr-restore-drill.sh" \
    --work-dir "$OUT_ROOT/cp-dr-work" \
    --out "$OUT_ROOT/cp-dr-restore-report.json"

run_step retention_preflight_evidence \
  env AO2_RELEASE_RETENTION_PRUNE=0 npm run release:retention-preflight

run_step artifact_index \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" npm run artifacts:index

run_step artifact_health_evidence \
  env AO2_ARTIFACT_HEALTH_INDEX="$OUT_ROOT/artifact-index/artifact-index.json" \
    AO2_ARTIFACT_HEALTH_ROOT="$OUT_ROOT/artifact-health" \
    AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS="target/ci-artifacts target/release-readiness-regression-gate target/release-readiness-ci target/release-evidence-closure target/phase1-promotion-golden target/pulse-real-execute-containment .ao2-local/pulse/latest" \
    npm run artifacts:health

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
names = ["restore_drill_evidence", "retention_preflight_evidence", "artifact_index", "artifact_health_evidence"]
checks = []
for name in names:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
fixture_snapshot_manifest = out_root / "fixture-snapshot-manifest.json"
manifest = {
    "schema_version": "ao2.dr-retention-long-run-snapshot.manifest.v1",
    "restore_drill_evidence": str(out_root / "cp-dr-restore-report.json"),
    "retention_preflight_evidence": str(log_dir / "retention_preflight_evidence.log"),
    "artifact_health_evidence": str(out_root / "artifact-health" / "summary.json"),
}
fixture_snapshot_manifest.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.dr-retention-long-run-snapshot.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "fixture_snapshot_manifest": str(fixture_snapshot_manifest),
    "restore_drill_evidence": manifest["restore_drill_evidence"],
    "retention_preflight_evidence": manifest["retention_preflight_evidence"],
    "artifact_health_evidence": manifest["artifact_health_evidence"],
    "component_summaries": manifest,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
