#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RSI_LIVE_SELF_CHANGE_READBACK_INDEX_ROOT:-$ROOT/target/rsi-live-self-change-readback-index/latest}"
LIVE_SUMMARY="${AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_SUMMARY:-$ROOT/target/rsi-live-self-change-rehearsal/latest/summary.json}"
READBACK_SUMMARY="${AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_READBACK_SUMMARY:-$ROOT/../ao2-control-plane/target/ao2-rsi-live-self-change-rehearsal-readback/summary.json}"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

SUMMARY="$OUT_ROOT/summary.json"
INDEX="$OUT_ROOT/index.md"

python3 - "$LIVE_SUMMARY" "$READBACK_SUMMARY" "$SUMMARY" "$INDEX" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

INDEX_SCHEMA = "ao2.rsi-live-self-change-readback-evidence-index.v1"
LIVE_SCHEMA = "ao2.rsi-live-self-change-rehearsal.v1"
READBACK_SCHEMA = "ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1"
READBACK_ARTIFACT = "ao2-control-plane-ao2-rsi-live-self-change-rehearsal-readback"
EXPECTED_EVIDENCE_PATHS = [
    "summary.json",
    "proposed-live-self-change.patch",
    "rollback-live-self-change.patch",
]

live_summary_path = Path(sys.argv[1])
readback_summary_path = Path(sys.argv[2])
summary_path = Path(sys.argv[3])
index_path = Path(sys.argv[4])


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def add_blocker(blockers: list[dict], code: str, **extra) -> None:
    payload = {"code": code, "severity": "blocking"}
    payload.update(extra)
    blockers.append(payload)


blockers: list[dict] = []
live_summary = {}
readback_summary = {}

try:
    live_summary = load_json(live_summary_path)
except Exception:
    add_blocker(blockers, "live_rehearsal_summary_unreadable")

try:
    readback_summary = load_json(readback_summary_path)
except Exception:
    add_blocker(blockers, "control_plane_readback_summary_unreadable")

live_self_change = live_summary.get("self_change", {}) if isinstance(live_summary.get("self_change"), dict) else {}
live_rollback = live_summary.get("rollback", {}) if isinstance(live_summary.get("rollback"), dict) else {}
live_evidence = (
    live_summary.get("live_self_change_evidence", {})
    if isinstance(live_summary.get("live_self_change_evidence"), dict)
    else {}
)
live_claim_boundary = (
    live_summary.get("claim_boundary", {}) if isinstance(live_summary.get("claim_boundary"), dict) else {}
)
live_trust = live_summary.get("trust_boundary", {}) if isinstance(live_summary.get("trust_boundary"), dict) else {}
expected_live_trust = {
    "local_only": True,
    "uses_network": False,
    "requires_provider_api_key": False,
    "stores_credentials": False,
    "mutates_repositories": True,
    "applies_patch": True,
    "rollback_applied": True,
    "publishes_claims": False,
}
live_ready = (
    live_summary.get("schema_version") == LIVE_SCHEMA
    and live_summary.get("status") == "live_rehearsal_passed"
    and live_claim_boundary.get("full_autonomous_self_mutating_rsi") == "denied"
    and live_self_change.get("mode") == "live_rehearsal"
    and live_self_change.get("repository") == "ao2"
    and live_self_change.get("repository_restored") is True
    and live_rollback.get("mode") == "live_rehearsal"
    and live_rollback.get("status") == "passed"
    and live_rollback.get("same_change_class") is True
    and live_evidence.get("status") == "passed"
    and live_evidence.get("evidence_paths") == EXPECTED_EVIDENCE_PATHS
    and live_trust == expected_live_trust
)
if live_summary and not live_ready:
    add_blocker(
        blockers,
        "live_rehearsal_not_ready",
        status=live_summary.get("status"),
        schema_version=live_summary.get("schema_version"),
    )

readback_trust = (
    readback_summary.get("trust_boundary", {})
    if isinstance(readback_summary.get("trust_boundary"), dict)
    else {}
)
expected_readback_trust = {
    "downloads_github_actions_artifacts": False,
    "control_plane_approves_rsi_claims": False,
    "mutates_ao_artifacts": False,
    "applies_ao_patches": False,
    "mutates_github_repositories": False,
    "mutates_observer_storage": False,
    "publishes_claims": False,
    "credential_material_included": False,
    "provider_api_keys_allowed": False,
}
if readback_summary.get("schema_version") != READBACK_SCHEMA:
    add_blocker(
        blockers,
        "control_plane_readback_schema_mismatch",
        schema_version=readback_summary.get("schema_version"),
    )
if readback_summary.get("status") != "passed":
    add_blocker(blockers, "control_plane_readback_not_passed", status=readback_summary.get("status"))
if (
    readback_summary.get("producer_schema_version") != LIVE_SCHEMA
    or readback_summary.get("producer_status") != "live_rehearsal_passed"
):
    add_blocker(
        blockers,
        "control_plane_readback_producer_mismatch",
        producer_schema_version=readback_summary.get("producer_schema_version"),
        producer_status=readback_summary.get("producer_status"),
    )
if readback_summary.get("gaps", []) != []:
    gaps = readback_summary.get("gaps")
    add_blocker(
        blockers,
        "control_plane_readback_reported_gaps",
        gap_count=len(gaps) if isinstance(gaps, list) else 1,
    )
if readback_trust and readback_trust != expected_readback_trust:
    add_blocker(blockers, "control_plane_readback_trust_boundary_drift")

ready = not blockers
live_summary_sha = sha256_file(live_summary_path) if live_summary_path.is_file() else None
readback_summary_sha = sha256_file(readback_summary_path) if readback_summary_path.is_file() else None

payload = {
    "schema_version": INDEX_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "retained_claim_level_evidence": {
        "status": "present" if ready else "blocked",
        "artifact": READBACK_ARTIFACT,
        "schema_version": READBACK_SCHEMA,
        "summary_sha256": readback_summary_sha,
    },
    "sources": {
        "live_rehearsal": {
            "schema_version": live_summary.get("schema_version"),
            "status": live_summary.get("status"),
            "summary_sha256": live_summary_sha,
            "evidence_paths": live_evidence.get("evidence_paths", []),
        },
        "control_plane_readback": {
            "schema_version": readback_summary.get("schema_version"),
            "status": readback_summary.get("status"),
            "producer_schema_version": readback_summary.get("producer_schema_version"),
            "producer_status": readback_summary.get("producer_status"),
            "summary_sha256": readback_summary_sha,
        },
    },
    "claim_boundary": {
        "bounded_governed_rsi": "allowed",
        "full_autonomous_self_mutating_rsi": "denied",
    },
    "full_claim_boundary": {
        "decision": "denied",
        "remaining_blockers": [
            "covenant_claim_publish_approval",
            "rehearsal_not_claim_publish_evidence",
        ],
    },
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "mutates_control_plane_artifacts": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# RSI Live Self-Change Readback Evidence Index",
    "",
    f"Status: `{payload['status']}`",
    f"Schema: `{INDEX_SCHEMA}`",
    "",
    "Sources:",
    "",
    f"- live rehearsal: `{LIVE_SCHEMA}` sha256 `{live_summary_sha}`",
    f"- control-plane readback: `{READBACK_SCHEMA}` sha256 `{readback_summary_sha}`",
    "",
    "Claim boundary:",
    "",
    "- `full_autonomous_self_mutating_rsi=denied`",
    "- `covenant_claim_publish_approval` remains required",
    "- `rehearsal_not_claim_publish_evidence` remains required",
    "",
    "Trust boundary:",
    "",
    "- `mutates_repositories=false`",
    "- `mutates_control_plane_artifacts=false`",
    "- `publishes_claims=false`",
    "- `approves_rsi_claims=false`",
]
if blockers:
    lines.extend(["", "Blockers:", ""])
    lines.extend(f"- `{blocker['code']}`" for blocker in blockers)
else:
    lines.extend(["", "Blockers: none"])
index_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"index={index_path}")
print(f"rsi_live_self_change_readback_index={payload['status']}")
print("claim_level=full_autonomous_self_mutating_rsi decision=denied")
if not ready:
    for blocker in blockers:
        print(f"blocker={blocker['code']}", file=sys.stderr)
    raise SystemExit(1)
PY
