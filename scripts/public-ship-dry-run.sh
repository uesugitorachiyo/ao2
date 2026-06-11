#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_SHIP_DRY_RUN_ROOT:-$ROOT/target/public-ship-dry-run/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="${AO2_PUBLIC_SHIP_DRY_RUN_FIXTURE_DIR:-}"

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

run_step public_ship_rehearsal \
  env \
    AO2_PUBLIC_SHIP_REHEARSAL_ROOT="$OUT_ROOT/public-ship-rehearsal" \
    AO2_PUBLIC_SHIP_REHEARSAL_FIXTURE_DIR="$FIXTURE_DIR" \
    AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR="$FIXTURE_DIR" \
    npm run release:public-ship-rehearsal

run_step install_update_rollback_contract \
  env AO2_REAL_RELEASE_INSTALL_UPDATE_ROOT="$OUT_ROOT/real-release-install-update-drill" \
    npm run release:real-install-update-drill

python3 - "$OUT_ROOT" "$SUMMARY" "$FIXTURE_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
fixture_dir = sys.argv[3] or None
log_dir = out_root / "logs"
checks = []
for name in ["public_ship_rehearsal", "install_update_rollback_contract"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
rollback_manifest = out_root / "rollback_manifest.json"
rollback_manifest.write_text(json.dumps({
    "schema_version": "ao2.public-ship-dry-run.rollback-manifest.v1",
    "status": "passed",
    "rollback_sources": [
        str(out_root / "real-release-install-update-drill" / "summary.json"),
        str(out_root / "public-ship-rehearsal" / "summary.json"),
    ],
    "publish_side_effects": "not executed",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.public-ship-dry-run.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "release_artifact_fixture": fixture_dir,
    "checks": checks,
    "rollback_manifest": str(rollback_manifest),
    "component_summaries": {
        "public_ship_rehearsal": str(out_root / "public-ship-rehearsal" / "summary.json"),
        "install_update_rollback_contract": str(out_root / "real-release-install-update-drill" / "summary.json"),
    },
    "publish_guards": {"tag_push_publish_deploy": "not executed", "release_publish": "not executed"},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
