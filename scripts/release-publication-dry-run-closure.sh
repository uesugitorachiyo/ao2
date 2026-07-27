#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_PUBLICATION_DRY_RUN_CLOSURE_ROOT:-$ROOT/target/release-publication-dry-run-closure/latest}"
CANONICAL_HOSTED_SUMMARY="${AO2_RELEASE_CANONICAL_HOSTED_SUMMARY:-}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
. "$ROOT/scripts/lib/pulse-gate-lib.sh"

ao2_gate_run_step "$LOG_DIR" asset_publication_readiness \
  env AO2_RELEASE_ASSET_PUBLICATION_READINESS_ROOT="$OUT_ROOT/release-asset-publication-readiness" \
    npm run release:asset-publication-readiness

ao2_gate_run_step "$LOG_DIR" sync_provenance_assets_dry_run \
  env \
    AO2_RELEASE_SYNC_ROOT="$OUT_ROOT/release-sync-provenance-assets" \
    AO2_RELEASE_SYNC_CONFIRM= \
    npm run release:sync-provenance-assets

ao2_gate_run_step "$LOG_DIR" stable_readiness \
  env \
    AO2_STABLE_RELEASE_READINESS_ROOT="$OUT_ROOT/stable-release-readiness" \
    AO2_STABLE_PROMOTION_CONFIRM= \
    npm run release:stable-readiness

python3 - "$OUT_ROOT" "$SUMMARY" "$CANONICAL_HOSTED_SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
canonical_summary_path = Path(sys.argv[3]).resolve() if sys.argv[3] else None
log_dir = out_root / "logs"

components = {
    "asset_publication_readiness": {
        "schema_version": "ao2.release-asset-publication-readiness.v1",
        "summary": out_root / "release-asset-publication-readiness" / "summary.json",
        "log_name": "asset_publication_readiness",
    },
    "sync_provenance_assets": {
        "schema_version": "ao2.release-sync-provenance-assets.v1",
        "summary": out_root / "release-sync-provenance-assets" / "summary.json",
        "log_name": "sync_provenance_assets_dry_run",
    },
    "stable_readiness": {
        "schema_version": "ao2.stable-release-readiness.v1",
        "summary": out_root / "stable-release-readiness" / "summary.json",
        "log_name": "stable_readiness",
    },
}

checks = []
component_payloads = {}
blockers = []
for name, component in components.items():
    exit_code = int((log_dir / f"{component['log_name']}.log.exit-code").read_text(encoding="utf-8").strip())
    log_path = log_dir / f"{component['log_name']}.log"
    summary_file = component["summary"]
    summary_exists = summary_file.is_file()
    payload = json.loads(summary_file.read_text(encoding="utf-8")) if summary_exists else {}
    schema_matches = payload.get("schema_version") == component["schema_version"]
    check_passed = exit_code == 0 and summary_exists and schema_matches
    checks.append(
        {
            "name": name,
            "status": "passed" if check_passed else "failed",
            "exit_code": exit_code,
            "summary": str(summary_file),
            "log": str(log_path),
            "expected_schema_version": component["schema_version"],
            "observed_schema_version": payload.get("schema_version"),
        }
    )
    component_payloads[name] = payload
    for blocker in payload.get("blockers", []) or payload.get("promotion_blockers", []):
        blockers.append({"component": name, **blocker})

asset_payload = component_payloads["asset_publication_readiness"]
sync_payload = component_payloads["sync_provenance_assets"]
stable_payload = component_payloads["stable_readiness"]
sync_trust_boundary = sync_payload.get("trust_boundary", {})
stable_trust_boundary = stable_payload.get("trust_boundary", {})

canonical_payload = {}
canonical_ready = False
if canonical_summary_path is not None:
    canonical_payload = json.loads(canonical_summary_path.read_text(encoding="utf-8"))
    version = canonical_payload.get("version")
    canonical_ready = (
        canonical_payload.get("schema_version")
        == "ao2.hosted-release-public-verification.v1"
        and canonical_payload.get("status") == "passed"
        and isinstance(version, str)
        and canonical_payload.get("tag") == f"v{version}"
        and canonical_payload.get("assets")
        == sorted(
            [
                f"ao2-{version}-linux-x86_64.tar.gz",
                f"ao2-{version}-macos-aarch64.tar.gz",
                f"ao2-{version}-windows-x86_64.tar.gz",
                "promotion-plan.json",
                "SHA256SUMS",
            ]
        )
    )

dry_run = sync_payload.get("dry_run") is True
upload_status = sync_payload.get("upload_status")
legacy_publication_ready = (
    asset_payload.get("status") == "passed"
    and sync_payload.get("status") in {"already_synced", "ready_to_upload", "release_missing"}
    and dry_run
    and upload_status == "not_attempted"
    and sync_trust_boundary.get("mutates_releases") is False
)
publication_ready = legacy_publication_ready or canonical_ready
publication_model = (
    "canonical_hosted_five_asset"
    if canonical_ready
    else "legacy_provenance_sidecars"
)
stable_release_ready = stable_payload.get("stable_release_ready") is True
commands_passed = all(check["status"] == "passed" for check in checks)
if canonical_ready:
    commands_passed = all(
        check["status"] == "passed"
        for check in checks
        if check["name"] not in {"asset_publication_readiness", "sync_provenance_assets"}
    )
mutation_guard_ok = (
    dry_run
    and upload_status == "not_attempted"
    and sync_trust_boundary.get("mutates_releases") is False
    and stable_trust_boundary.get("mutates_releases") is False
)

payload = {
    "schema_version": "ao2.release-publication-dry-run-closure.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if commands_passed and mutation_guard_ok else "failed",
    "artifact_root": str(out_root),
    "publication_ready": publication_ready,
    "stable_release_ready": stable_release_ready,
    "release_tag": asset_payload.get("release_tag") or sync_payload.get("release_tag"),
    "publication_model": publication_model,
    "expected_release_assets": (
        canonical_payload["assets"]
        if canonical_ready
        else asset_payload.get("expected_release_assets", [])
    ),
    "checks": checks,
    "component_summaries": {
        "asset_publication_readiness": str(components["asset_publication_readiness"]["summary"]),
        "sync_provenance_assets": str(components["sync_provenance_assets"]["summary"]),
        "stable_readiness": str(components["stable_readiness"]["summary"]),
    },
    "publication_state": {
        "asset_publication_status": (
            "not_applicable_canonical_hosted_release"
            if canonical_ready
            else asset_payload.get("status")
        ),
        "sync_provenance_status": (
            "not_applicable_canonical_hosted_release"
            if canonical_ready
            else sync_payload.get("status")
        ),
        "stable_readiness_status": stable_payload.get("status"),
        "dry_run": dry_run,
        "upload_status": upload_status,
        "release_publish": "not executed",
        "tag_push_publish_deploy": "not executed",
        "release_not_found_gap": sync_payload.get("release_not_found_gap"),
    },
    "blockers": [] if canonical_ready else blockers,
    "canonical_hosted_release_verification": {
        "path": str(canonical_summary_path) if canonical_summary_path else None,
        "schema_version": canonical_payload.get("schema_version"),
        "status": canonical_payload.get("status"),
    },
    "trust_boundary": {
        "local_only": True,
        "queries_public_releases": True,
        "mutates_releases": False,
        "stores_credentials": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
print(f"publication_ready={str(publication_ready).lower()}")
print(f"stable_release_ready={str(stable_release_ready).lower()}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
