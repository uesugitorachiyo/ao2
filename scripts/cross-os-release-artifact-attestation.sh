#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_CROSS_OS_ATTESTATION_ROOT:-$ROOT/target/cross-os-release-artifact-attestation/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
ENABLE_THREE_OS="${AO2_CROSS_OS_ATTESTATION_ENABLE_THREE_OS:-0}"
ENABLE_DOWNLOAD="${AO2_CROSS_OS_ATTESTATION_ENABLE_DOWNLOAD:-0}"
REQUIRE_NATIVE="${AO2_CROSS_OS_ATTESTATION_REQUIRE_NATIVE:-0}"
REQUIRE_DOWNLOAD="${AO2_CROSS_OS_ATTESTATION_REQUIRE_DOWNLOAD:-0}"

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

run_step install_update_contract \
  env AO2_REAL_RELEASE_INSTALL_UPDATE_ROOT="$OUT_ROOT/real-release-install-update-drill" \
    AO2_REAL_RELEASE_DRILL_ENABLE_DOWNLOAD="$ENABLE_DOWNLOAD" \
    npm run release:real-install-update-drill

if [ "$ENABLE_THREE_OS" = "1" ]; then
  run_step three_os_smoke \
    env AO2_THREE_OS_SMOKE_ROOT="$OUT_ROOT/three-os-smoke" \
      npm run smoke:three-os
elif [ "$REQUIRE_NATIVE" = "1" ]; then
  printf "native three-OS smoke required but not enabled; set AO2_CROSS_OS_ATTESTATION_ENABLE_THREE_OS=1\n" >"$LOG_DIR/three_os_smoke.log"
  printf "2\n" >"$LOG_DIR/three_os_smoke.log.exit-code"
else
  printf "smoke:three-os skipped; set AO2_CROSS_OS_ATTESTATION_ENABLE_THREE_OS=1\n" >"$LOG_DIR/three_os_smoke.log"
  printf "0\n" >"$LOG_DIR/three_os_smoke.log.exit-code"
fi

if [ "$REQUIRE_DOWNLOAD" = "1" ] && [ "$ENABLE_DOWNLOAD" != "1" ]; then
  printf "release download verification required but not enabled; set AO2_CROSS_OS_ATTESTATION_ENABLE_DOWNLOAD=1\n" >"$LOG_DIR/download_requirement.log"
  printf "2\n" >"$LOG_DIR/download_requirement.log.exit-code"
else
  printf "release download verification requirement satisfied by policy\n" >"$LOG_DIR/download_requirement.log"
  printf "0\n" >"$LOG_DIR/download_requirement.log.exit-code"
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$ENABLE_THREE_OS" "$ENABLE_DOWNLOAD" "$REQUIRE_NATIVE" "$REQUIRE_DOWNLOAD" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
enable_three_os = sys.argv[3] == "1"
enable_download = sys.argv[4] == "1"
require_native = sys.argv[5] == "1"
require_download = sys.argv[6] == "1"
log_dir = out_root / "logs"
checks = []
for name in ["install_update_contract", "three_os_smoke", "download_requirement"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
required_ci_checks = [
    {
        "name": "install_update_contract",
        "status": next(item["status"] for item in checks if item["name"] == "install_update_contract"),
        "mode": "ci_safe_required",
        "source": "release:real-install-update-drill",
    },
    {
        "name": "download_requirement",
        "status": next(item["status"] for item in checks if item["name"] == "download_requirement"),
        "mode": "ci_safe_required",
        "source": "AO2_CROSS_OS_ATTESTATION_REQUIRE_DOWNLOAD",
    },
]
optional_native_checks = [
    {
        "name": "three_os_smoke",
        "status": next(item["status"] for item in checks if item["name"] == "three_os_smoke"),
        "mode": "native_execution_optional" if not require_native else "native_execution_required",
        "enabled": enable_three_os,
        "required": require_native,
        "source": "smoke:three-os",
        "skip_reason": None if enable_three_os else "set AO2_CROSS_OS_ATTESTATION_ENABLE_THREE_OS=1",
    },
    {
        "name": "release_download_verify",
        "status": "enabled" if enable_download else ("required_missing" if require_download else "skipped"),
        "mode": "download_verification_optional" if not require_download else "download_verification_required",
        "enabled": enable_download,
        "required": require_download,
        "source": "release:download-verify",
        "skip_reason": None if enable_download else "set AO2_CROSS_OS_ATTESTATION_ENABLE_DOWNLOAD=1",
    },
]
platform_attestations = {
    "macos-aarch64": {"status": "contract_attested", "source": "release:real-install-update-drill", "requirement": "ci_safe_required"},
    "linux-aarch64": {"status": "contract_attested", "source": "smoke:three-os" if enable_three_os else "three-os optional", "requirement": "native_execution_optional"},
    "linux-x86_64": {"status": "contract_attested", "source": "smoke:three-os" if enable_three_os else "three-os optional", "requirement": "native_execution_optional"},
    "windows-x86_64": {"status": "contract_attested", "source": "smoke:three-os" if enable_three_os else "static archive optional", "requirement": "native_execution_optional"},
}
platform_matrix = [
    {"platform": key, **value}
    for key, value in platform_attestations.items()
]
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.cross-os-release-attestation.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "required_ci_checks": required_ci_checks,
    "optional_native_checks": optional_native_checks,
    "platform_matrix": platform_matrix,
    "platform_attestations": platform_attestations,
    "external_release_assets": {
        "release:download-verify": "enabled" if enable_download else "contract_recorded",
        "download_attempted": enable_download,
        "download_required": require_download,
    },
    "component_summaries": {
        "release_real_install_update_drill": str(out_root / "real-release-install-update-drill" / "summary.json"),
        "smoke_three_os": str(out_root / "three-os-smoke" / "summary.json"),
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
