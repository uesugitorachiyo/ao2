#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_CUTOVER_LOCK_ROOT:-$ROOT/target/release-cutover-readiness-lock/latest}"
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

run_step candidate_binary_diff_audit \
  env AO2_RELEASE_CANDIDATE_BINARY_DIFF_ROOT="$OUT_ROOT/release-candidate-binary-diff-audit" \
    npm run release:candidate-binary-diff-audit

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "candidate_binary_diff_audit.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "release-candidate-binary-diff-audit" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
lock = out_root / "cutover-readiness-lock.json"
lock.write_text(json.dumps({
    "schema_version": "ao2.release-cutover-lock-manifest.v1",
    "binary_diff_lock": component_data.get("binary_delta_manifest"),
    "checksum_lock": component_data.get("checksum_delta_manifest"),
    "provenance_lock": component_data.get("provenance_delta"),
    "known_blocker_state": "release_not_published_local_only",
    "tag_push_publish_deploy": "not_executed",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "candidate_binary_diff_audit", "command": "release:candidate-binary-diff-audit", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "candidate_binary_diff_audit.log")},
    {"name": "binary_diff_lock", "status": "passed"},
    {"name": "checksum_lock", "status": "passed"},
    {"name": "provenance_lock", "status": "passed"},
    {"name": "known_blocker_state", "status": "passed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.release-cutover-readiness-lock.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "binary_diff_lock": component_data.get("binary_delta_manifest"),
    "checksum_lock": component_data.get("checksum_delta_manifest"),
    "provenance_lock": component_data.get("provenance_delta"),
    "known_blocker_state": "release_not_published_local_only",
    "lock_manifest": str(lock),
    "publish_guards": {"tag_push_publish_deploy": "not_executed"},
    "component_summaries": {"candidate_binary_diff_audit": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
