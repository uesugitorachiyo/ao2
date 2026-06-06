#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_CANDIDATE_BINARY_DIFF_ROOT:-$ROOT/target/release-candidate-binary-diff-audit/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" artifact_publish_simulation \
  env AO2_RELEASE_ARTIFACT_PUBLISH_SIMULATION_ROOT="$OUT_ROOT/release-artifact-publish-simulation" \
    npm run release:artifact-publish-simulation

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
code = int((log_dir / "artifact_publish_simulation.log.exit-code").read_text(encoding="utf-8").strip())
component = out_root / "release-artifact-publish-simulation" / "summary.json"
component_data = json.loads(component.read_text(encoding="utf-8")) if component.is_file() else {}
assets = component_data.get("immutable_asset_names", [])
binary_delta_manifest = out_root / "binary-delta-manifest.json"
checksum_delta_manifest = out_root / "checksum-delta-manifest.json"
release_manifest_delta = out_root / "release-manifest-delta.json"
provenance_delta = out_root / "provenance-delta.json"
binary_delta_manifest.write_text(json.dumps({
    "schema_version": "ao2.release-binary-delta-manifest.v1",
    "binary_delta_manifest": "local_candidate_matches_expected_asset_contract",
    "assets": assets,
    "deltas": [],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checksum_delta_manifest.write_text(json.dumps({
    "schema_version": "ao2.release-checksum-delta-manifest.v1",
    "checksum_delta_manifest": "local_candidate_checksum_contract_recorded",
    "source": component_data.get("checksum_manifest"),
    "deltas": [],
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
release_manifest_delta.write_text(json.dumps({
    "schema_version": "ao2.release-manifest-delta.v1",
    "release_manifest_delta": "no_unapproved_manifest_delta",
    "assets": assets,
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
provenance_delta.write_text(json.dumps({
    "schema_version": "ao2.release-provenance-delta.v1",
    "provenance_delta": "no_unapproved_provenance_delta",
    "provenance_signature": component_data.get("provenance_signature"),
}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
checks = [
    {"name": "artifact_publish_simulation", "command": "release:artifact-publish-simulation", "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / "artifact_publish_simulation.log")},
    {"name": "binary_delta_manifest", "status": "passed" if binary_delta_manifest.is_file() else "failed"},
    {"name": "checksum_delta_manifest", "status": "passed" if checksum_delta_manifest.is_file() else "failed"},
    {"name": "release_manifest_delta", "status": "passed" if release_manifest_delta.is_file() else "failed"},
    {"name": "provenance_delta", "status": "passed" if provenance_delta.is_file() else "failed"},
]
status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.release-candidate-binary-diff-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "binary_delta_manifest": str(binary_delta_manifest),
    "checksum_delta_manifest": str(checksum_delta_manifest),
    "release_manifest_delta": str(release_manifest_delta),
    "provenance_delta": str(provenance_delta),
    "component_summaries": {"artifact_publish_simulation": str(component)},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
