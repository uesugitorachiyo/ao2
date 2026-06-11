#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_LOCAL_CANARY_ROOT:-$ROOT/target/local-canary/latest}"
SUMMARY="$OUT_ROOT/local-canary-summary.json"
LOG_DIR="$OUT_ROOT/logs"
PULSE_SOURCE="$OUT_ROOT/pulse-source"
CP_RESTORE_ROOT="${AO2_LOCAL_CANARY_CP_RESTORE_ROOT:-$CP_ROOT/target/dr-restore-drill/local-canary}"
STEP_TIMEOUT_SECONDS="${AO2_LOCAL_CANARY_STEP_TIMEOUT_SECONDS:-900}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR" "$PULSE_SOURCE/loop-000"

printf '{}\n' > "$PULSE_SOURCE/executor-evidence.json"
printf '# Packet\n' > "$PULSE_SOURCE/packet.md"
printf '# Board\n' > "$PULSE_SOURCE/board.md"
printf '{"schema_version":"ao2.pulse-eval-loop.v1","status":"passed"}\n' > "$PULSE_SOURCE/loop-000/pulse-eval-loop.json"

run_step() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  set +e
  python3 - "$STEP_TIMEOUT_SECONDS" "$log" "$@" <<'PY'
import os
import signal
import subprocess
import sys
import time

timeout_seconds = float(sys.argv[1])
log_path = sys.argv[2]
cmd = sys.argv[3:]
started = time.monotonic()
with open(log_path, "w", encoding="utf-8") as log:
    try:
        kwargs = {
            "stdout": log,
            "stderr": subprocess.STDOUT,
            "text": True,
        }
        if os.name == "nt":
            kwargs["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
        else:
            kwargs["start_new_session"] = True
        proc = subprocess.Popen(cmd, **kwargs)
        code = proc.wait(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        elapsed = round(time.monotonic() - started, 3)
        log.write(
            f"step timed out after {int(timeout_seconds)}s "
            f"elapsed_seconds={elapsed}: {' '.join(cmd)}\n"
        )
        if os.name == "nt":
            proc.kill()
        else:
            os.killpg(proc.pid, signal.SIGTERM)
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            if os.name == "nt":
                proc.kill()
            else:
                os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
        code = 124
    except FileNotFoundError as exc:
        log.write(f"failed to start command: {exc}\n")
        code = 127
raise SystemExit(code)
PY
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

run_step release_artifact_consumer_smoke \
  env AO2_RELEASE_ARTIFACT_CONSUMER_ROOT="$OUT_ROOT/release-artifact-consumer-smoke" \
  npm run release:artifact-consumer-smoke -- --dry-run

run_step ci_artifact_download_contract \
  env AO2_CI_ARTIFACT_DOWNLOAD_ROOT="$ROOT/target/ci-artifacts/latest" \
  npm run artifacts:ci-download-contract

run_step pulse_local_mirror \
  env AO2_PULSE_LOCAL_MIRROR_SOURCE="$PULSE_SOURCE" \
  npm run pulse:local-mirror

run_step pulse_resume_dry_run \
  env AO2_PULSE_RESUME_ROOT="$OUT_ROOT/pulse-resume" \
  npm run pulse:resume -- --dry-run

# Mirrors: ../ao2-control-plane/scripts/cp-dr-restore-drill.sh --negative-only
run_step control_plane_restore_negative \
  "$CP_ROOT/scripts/cp-dr-restore-drill.sh" \
  --negative-only \
  --work-dir "$CP_RESTORE_ROOT" \
  --out "$CP_RESTORE_ROOT/dr-restore-report.json"

run_step artifact_index \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" \
  npm run artifacts:index

run_step artifact_health \
  env \
    AO2_ARTIFACT_HEALTH_INDEX="$OUT_ROOT/artifact-index/artifact-index.json" \
    AO2_ARTIFACT_HEALTH_ROOT="$OUT_ROOT/artifact-health" \
    AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS="ao2/target/ci-artifacts ao2/.ao2-local/pulse/latest ao2-control-plane/target/ci-artifacts ao2-control-plane/target/dr-restore-drill" \
    AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS="ao2/target/release-readiness-regression-gate ao2/target/release-readiness-ci ao2/target/release-evidence-closure ao2/target/phase1-promotion-golden ao2/target/pulse-real-execute-containment" \
    AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1 \
    npm run artifacts:health

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$CP_RESTORE_ROOT" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
cp_restore_root = Path(sys.argv[4]).resolve()
log_dir = out_root / "logs"

steps = [
    "release_artifact_consumer_smoke",
    "ci_artifact_download_contract",
    "pulse_local_mirror",
    "pulse_resume_dry_run",
    "control_plane_restore_negative",
    "artifact_index",
    "artifact_health",
]

step_results = []
for name in steps:
    log = log_dir / f"{name}.log"
    exit_code_path = log_dir / f"{name}.log.exit-code"
    exit_code = int(exit_code_path.read_text(encoding="utf-8").strip())
    step_results.append(
        {
            "name": name,
            "status": "passed" if exit_code == 0 else "failed",
            "exit_code": exit_code,
            "log": str(log),
        }
    )

payload = {
    "schema_version": "ao2.local-canary-run.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if all(item["exit_code"] == 0 for item in step_results) else "failed",
    "artifact_root": str(out_root),
    "step_results": step_results,
    "evidence": {
        "release_artifact_consumer_smoke": str(out_root / "release-artifact-consumer-smoke" / "summary.json"),
        "ci_artifact_download_contract": str(root / "target/ci-artifacts/latest/summary.json"),
        "pulse_resume": str(out_root / "pulse-resume" / "summary.json"),
        "control_plane_restore": str(cp_restore_root / "dr-restore-report.json"),
        "artifact_index": str(out_root / "artifact-index" / "artifact-index.json"),
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
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
