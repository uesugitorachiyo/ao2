#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_POST_MERGE_CANARY_ROOT:-$ROOT/target/post-merge-canary/$(date -u +%Y%m%dT%H%M%SZ)}"
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
}

run_step artifact_index "$OUT_ROOT/artifact-index.log" \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" npm run artifacts:index

run_step release_artifact_consumer_smoke "$OUT_ROOT/release-artifact-consumer-smoke.log" \
  env AO2_RELEASE_ARTIFACT_CONSUMER_ROOT="$OUT_ROOT/release-artifact-consumer-smoke" \
    npm run release:artifact-consumer-smoke -- --dry-run

run_step pulse_local_mirror "$OUT_ROOT/pulse-local-mirror.log" \
  env AO2_PULSE_LOCAL_MIRROR_DEST="$OUT_ROOT/pulse-local-mirror" npm run pulse:local-mirror

run_step pulse_resume "$OUT_ROOT/pulse-resume.log" \
  env AO2_PULSE_RESUME_JSON="$OUT_ROOT/pulse-local-mirror/resume.json" \
    AO2_PULSE_RESUME_ROOT="$OUT_ROOT/pulse-resume" \
    npm run pulse:resume -- --dry-run

if [ -x "$CP_ROOT/scripts/smoke-long-lived-dev.sh" ]; then
  run_step control_plane_long_lived_smoke "$OUT_ROOT/control-plane-long-lived-smoke.log" \
    env AO2_CP_LONG_LIVED_SMOKE_ROOT="$OUT_ROOT/control-plane-long-lived-smoke" \
      "$CP_ROOT/scripts/smoke-long-lived-dev.sh"
else
  printf "missing control-plane smoke script: %s\n" "$CP_ROOT/scripts/smoke-long-lived-dev.sh" \
    >"$OUT_ROOT/control-plane-long-lived-smoke.log"
  printf "127\n" >"$OUT_ROOT/control-plane-long-lived-smoke.log.exit-code"
fi

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
checks = []
for path in sorted(out_root.glob("*.log.exit-code")):
    name = path.name.removesuffix(".log.exit-code").replace("-", "_")
    exit_code = int(path.read_text(encoding="utf-8").strip())
    checks.append({
        "name": name,
        "status": "passed" if exit_code == 0 else "failed",
        "exit_code": exit_code,
        "log": str(out_root / path.name.removesuffix(".exit-code")),
    })

payload = {
    "schema_version": "ao2.post-merge-canary.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if all(item["exit_code"] == 0 for item in checks) else "failed",
    "artifact_root": str(out_root),
    "checks": checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
