#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PYTHON_GUARD_ARTIFACT_ROOT:-$ROOT/target/ci-artifacts/python-guard-tests}"
LOG="$OUT_ROOT/pytest.log"
SUMMARY="$OUT_ROOT/summary.json"

mkdir -p "$OUT_ROOT"

set +e
PYTHONDONTWRITEBYTECODE=1 python3 -m pytest \
  tests/test_public_stabilization.py \
  tests/test_phase1_promote_wrapper.py \
  tests/test_ao2_native_runtime_platform_evidence.py \
  -q 2>&1 | tee "$LOG"
status="${PIPESTATUS[0]}"
set -e

python3 - "$OUT_ROOT" "$LOG" "$SUMMARY" "$status" <<'PY'
import json
import sys
from pathlib import Path

out_root = Path(sys.argv[1])
log = Path(sys.argv[2])
summary = Path(sys.argv[3])
status_code = int(sys.argv[4])

payload = {
    "schema_version": "ao2.python-guard-ci-artifacts.v1",
    "status": "passed" if status_code == 0 else "failed",
    "command": "python3 -m pytest tests/test_public_stabilization.py tests/test_phase1_promote_wrapper.py tests/test_ao2_native_runtime_platform_evidence.py -q",
    "log": str(log),
    "log_bytes": log.stat().st_size if log.exists() else 0,
    "artifact_root": str(out_root),
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

exit "$status"
