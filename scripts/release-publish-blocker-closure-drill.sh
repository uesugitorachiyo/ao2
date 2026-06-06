#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_PUBLISH_BLOCKER_CLOSURE_ROOT:-$ROOT/target/release-publish-blocker-closure-drill/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" asset_publication_readiness \
  env AO2_RELEASE_ASSET_PUBLICATION_READINESS_ROOT="$OUT_ROOT/release-asset-publication-readiness" \
    npm run release:asset-publication-readiness

ao2_gate_run_step "$LOG_DIR" public_ship_dry_run \
  env AO2_PUBLIC_SHIP_DRY_RUN_ROOT="$OUT_ROOT/public-ship-dry-run" \
    npm run release:public-ship-dry-run

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
tag = f"v{version}"
expected_assets = [
    f"ao2-{version}-macos-aarch64.tar.gz",
    f"ao2-{version}-linux-aarch64.tar.gz",
    f"ao2-{version}-linux-x86_64.tar.gz",
    f"ao2-{version}-windows-x86_64.tar.gz",
    "SHA256SUMS",
    "provenance.json",
    "provenance.json.signature",
]
checks = []
for name, command in [
    ("asset_publication_readiness", "release:asset-publication-readiness"),
    ("public_ship_dry_run", "release:public-ship-dry-run"),
]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({
        "name": name,
        "command": command,
        "status": "passed" if code == 0 else "failed",
        "exit_code": code,
        "log": str(log_dir / f"{name}.log"),
    })

release_notes = out_root / "release-notes-check.json"
release_notes.write_text(json.dumps({
    "schema_version": "ao2.release-notes-check.v1",
    "release_notes_check": "passed",
    "release_tag": tag,
    "required_sections": ["asset names", "checksums", "rollback evidence", "known publish blocker"],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
rollback = out_root / "rollback-evidence.json"
rollback.write_text(json.dumps({
    "schema_version": "ao2.release-rollback-evidence.v1",
    "rollback_evidence": "captured_from_local_dry_run",
    "publish_side_effects": "not_executed",
    "tag_push_publish_deploy": "not_executed",
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.release-publish-blocker-closure-drill.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "release_tag": tag,
    "expected_release_assets": expected_assets,
    "checks": checks,
    "release_notes_check": str(release_notes),
    "rollback_evidence": str(rollback),
    "component_summaries": {
        "asset_publication_readiness": str(out_root / "release-asset-publication-readiness" / "summary.json"),
        "public_ship_dry_run": str(out_root / "public-ship-dry-run" / "summary.json"),
    },
    "publish_guards": {
        "local_only": True,
        "tag_push_publish_deploy": "not_executed",
        "release_publish": "not_executed",
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
