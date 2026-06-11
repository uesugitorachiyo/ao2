#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_ASSET_PUBLICATION_READINESS_ROOT:-$ROOT/target/release-asset-publication-readiness/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="${AO2_RELEASE_ASSET_PUBLICATION_FIXTURE_DIR:-$OUT_ROOT/release-artifact-fixture}"
CI_SAFE="${AO2_RELEASE_ASSET_PUBLICATION_READINESS_CI_SAFE:-0}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR" "$FIXTURE_DIR/uesugitorachiyo-ao2/ao2-python-guard"

cat >"$FIXTURE_DIR/uesugitorachiyo-ao2/ao2-python-guard/summary.json" <<'JSON'
{
  "schema_version": "ao2.python-guard-ci-artifacts.v1",
  "status": "passed",
  "fixture": "release-artifact-fixture",
  "trust_boundary": {
    "local_only": true,
    "stores_credentials": false
  }
}
JSON

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" cross_os_attestation \
  env AO2_CROSS_OS_ATTESTATION_ROOT="$OUT_ROOT/cross-os-release-artifact-attestation" \
    npm run release:cross-os-attestation

ao2_gate_run_step "$LOG_DIR" public_ship_dry_run \
  env \
    AO2_PUBLIC_SHIP_DRY_RUN_ROOT="$OUT_ROOT/public-ship-dry-run" \
    AO2_PUBLIC_SHIP_DRY_RUN_FIXTURE_DIR="$FIXTURE_DIR" \
    AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR="$FIXTURE_DIR" \
    AO2_PUBLIC_SHIP_DRY_RUN_CI_SAFE="$CI_SAFE" \
    npm run release:public-ship-dry-run

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$FIXTURE_DIR" "$CI_SAFE" <<'PY'
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
fixture_dir = Path(sys.argv[4]).resolve()
ci_safe = sys.argv[5] == "1"
log_dir = out_root / "logs"
version = subprocess.check_output([str(root / "scripts" / "current-version.sh")], cwd=root, text=True).strip()
tag = f"v{version}"
expected_release_assets = [
    f"ao2-{version}-macos-aarch64.tar.gz",
    f"ao2-{version}-linux-aarch64.tar.gz",
    f"ao2-{version}-linux-x86_64.tar.gz",
    f"ao2-{version}-windows-x86_64.tar.gz",
    "SHA256SUMS",
    "ao2-release-provenance.json",
    "ao2-release-provenance.json.sig",
    "ao2-release-signing-public.pem",
]
checks = []
for name in ["cross_os_attestation", "public_ship_dry_run"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
asset_contract = out_root / "expected-release-assets.json"
asset_contract.write_text(json.dumps({
    "schema_version": "ao2.release-asset-publication-contract.v1",
    "release_tag": tag,
    "version": version,
    "expected_release_assets": expected_release_assets,
    "release_not_found_gap": {
        "status": "tracked",
        "blocked_by_missing_public_release": True,
        "readiness_gate_blocks_publish": False,
    },
    "publish_guards": {"tag_push_publish_deploy": "not executed"},
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.release-asset-publication-readiness.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "release_tag": tag,
    "expected_release_assets": expected_release_assets,
    "asset_contract": str(asset_contract),
    "release_artifact_fixture": str(fixture_dir),
    "ci_safe_mode": ci_safe,
    "release_not_found_gap": {
        "status": "tracked",
        "next_action": "publish release assets only after explicit human approval",
    },
    "checks": checks,
    "component_summaries": {
        "cross_os_attestation": str(out_root / "cross-os-release-artifact-attestation" / "summary.json"),
        "public_ship_dry_run": str(out_root / "public-ship-dry-run" / "summary.json"),
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
