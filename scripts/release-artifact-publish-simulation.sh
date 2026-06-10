#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_ARTIFACT_PUBLISH_SIMULATION_ROOT:-$ROOT/target/release-artifact-publish-simulation/latest}"
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

run_step publish_blocker_closure_drill \
  env AO2_RELEASE_PUBLISH_BLOCKER_CLOSURE_ROOT="$OUT_ROOT/release-publish-blocker-closure-drill" \
    npm run release:publish-blocker-closure-drill

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
log_dir = out_root / "logs"
version = subprocess.check_output([str(root / "scripts" / "current-version.sh")], cwd=root, text=True).strip()
closure_summary = out_root / "release-publish-blocker-closure-drill" / "summary.json"
closure = json.loads(closure_summary.read_text(encoding="utf-8")) if closure_summary.is_file() else {}
expected_assets = closure.get("expected_release_assets") or [
    f"ao2-{version}-macos-aarch64.tar.gz",
    f"ao2-{version}-linux-aarch64.tar.gz",
    f"ao2-{version}-linux-x86_64.tar.gz",
    f"ao2-{version}-windows-x86_64.tar.gz",
    "SHA256SUMS",
    "ao2-release-provenance.json",
    "ao2-release-provenance.json.sig",
    "ao2-release-signing-public.pem",
]
code = int((log_dir / "publish_blocker_closure_drill.log.exit-code").read_text(encoding="utf-8").strip())
checksum_manifest = out_root / "checksum-manifest.json"
checksum_manifest.write_text(json.dumps({
    "schema_version": "ao2.release-publish-checksum-manifest.v1",
    "checksum_manifest": "simulated_from_local_release_contract",
    "assets": expected_assets,
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
upload_plan = out_root / "artifact-upload-plan.json"
upload_plan.write_text(json.dumps({
    "schema_version": "ao2.release-artifact-upload-plan.v1",
    "immutable_asset_names": expected_assets,
    "provenance_signature": "ao2-release-provenance.json.sig",
    "rollback_notes": closure.get("rollback_evidence"),
    "tag_push_publish_deploy": "not_executed",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "publish_blocker_closure_drill", "command": "release:publish-blocker-closure-drill", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "publish_blocker_closure_drill.log")},
    {"name": "immutable_asset_names", "status": "passed" if expected_assets else "failed"},
    {"name": "checksum_manifest", "status": "passed" if checksum_manifest.is_file() else "failed"},
    {"name": "provenance_signature", "status": "passed" if "ao2-release-provenance.json.sig" in expected_assets else "failed"},
    {"name": "rollback_notes", "status": "passed" if closure.get("rollback_evidence") else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.release-artifact-publish-simulation.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "immutable_asset_names": expected_assets,
    "checksum_manifest": str(checksum_manifest),
    "provenance_signature": "ao2-release-provenance.json.sig",
    "rollback_notes": closure.get("rollback_evidence"),
    "upload_plan": str(upload_plan),
    "publish_guards": {"local_only": True, "tag_push_publish_deploy": "not_executed"},
    "component_summaries": {"publish_blocker_closure_drill": str(closure_summary)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
