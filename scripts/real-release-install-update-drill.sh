#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_REAL_RELEASE_INSTALL_UPDATE_ROOT:-$ROOT/target/real-release-install-update-drill/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
ENABLE_DOWNLOAD="${AO2_REAL_RELEASE_DRILL_ENABLE_DOWNLOAD:-0}"

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

run_step fixture_install_update \
  env AO2_RELEASE_INSTALL_UPDATE_ROOT="$OUT_ROOT/release-install-update-fixture" \
    npm run release:install-update-fixture

if [ "$ENABLE_DOWNLOAD" = "1" ]; then
  run_step release_download_verify \
    env AO2_RELEASE_DOWNLOAD_DIR="$OUT_ROOT/release-download" \
      AO2_RELEASE_ROLLBACK_VERIFY="${AO2_REAL_RELEASE_ROLLBACK_VERIFY:-0}" \
      npm run release:download-verify
else
  printf "real release download skipped; set AO2_REAL_RELEASE_DRILL_ENABLE_DOWNLOAD=1\n" >"$LOG_DIR/release_download_verify.log"
  printf "0\n" >"$LOG_DIR/release_download_verify.log.exit-code"
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$ENABLE_DOWNLOAD" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
enable_download = sys.argv[3] == "1"
log_dir = out_root / "logs"
checks = []
for name in ["fixture_install_update", "release_download_verify"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.real-release-install-update-drill.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "download_mode": "real_release_download" if enable_download else "fixture_plus_download_contract",
    "checks": checks,
    "component_summaries": {
        "release_install_update_fixture": str(out_root / "release-install-update-fixture" / "summary.json"),
        "release_download_verify": str(out_root / "release-download" / "release-rollback-summary.json"),
    },
    "publish_guards": {
        "tag_push_publish_deploy": "not executed",
        "gh_release_create": "not executed",
        "release:download-verify": "optional download/verify only",
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
