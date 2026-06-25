#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RSI_CLAIM_READINESS_ROOT:-$ROOT/target/rsi-claim-readiness/latest}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$SUMMARY" <<'PY'
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
self_change_dry_run_summary = Path(
    os.environ.get(
        "AO2_RSI_SELF_CHANGE_DRY_RUN_SUMMARY",
        root / "target" / "rsi-self-change-dry-run" / "latest" / "summary.json",
    )
)

bounded_required = [
    "scripts/pulse-auto-advance.sh",
    "scripts/pulse-generate-next.sh",
    "scripts/pulse-real-execute-containment.sh",
    "scripts/pulse-resume.sh",
    "tests/test_public_stabilization.py",
    "docs/VERIFICATION.md",
]
bounded_present = [
    path for path in bounded_required if (root / path).is_file()
]
bounded_missing = [
    path for path in bounded_required if not (root / path).is_file()
]

full_blockers = [
    {
        "id": "mutation_authority",
        "evidence_state": "missing",
        "required_evidence": "an explicit governed authority path for AO2 to mutate its own implementation repositories",
    },
    {
        "id": "rollback_evidence",
        "evidence_state": "missing",
        "required_evidence": "a proven rollback path for failed self-change attempts, not only release install rollback",
    },
    {
        "id": "live_self_change_evidence",
        "evidence_state": "missing",
        "required_evidence": "a completed AO2-originated change to AO2 with replayable before/after evidence",
    },
    {
        "id": "observer_readback",
        "evidence_state": "missing",
        "required_evidence": "ao2-control-plane readback for the self-change and rollback evidence packet",
    },
    {
        "id": "covenant_claim_publish_approval",
        "evidence_state": "missing",
        "required_evidence": "Covenant approval to publish the full autonomous self-mutating RSI claim",
    },
]

def read_self_change_dry_run_evidence(path):
    if not path.is_file():
        return {
            "evidence_state": "missing",
            "schema_version": None,
            "status": "missing",
        }
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {
            "evidence_state": "invalid",
            "schema_version": None,
            "status": "invalid_json",
        }

    trust_boundary = payload.get("trust_boundary", {})
    rollback_rehearsal = payload.get("rollback_rehearsal", {})
    expected_trust_boundary = {
        "local_only": True,
        "uses_network": False,
        "requires_provider_api_key": False,
        "stores_credentials": False,
        "mutates_repositories": False,
        "applies_patch": False,
        "emits_authority_packet_candidate": True,
        "publishes_claims": False,
    }
    mutation_authority_packet = payload.get("mutation_authority_packet", {})
    evidence_present = (
        payload.get("schema_version") == "ao2.rsi-governed-self-change-dry-run.v1"
        and payload.get("status") == "dry_run_evidence_ready"
        and payload.get("self_change", {}).get("mode") == "dry_run"
        and payload.get("rollback", {}).get("mode") == "dry_run"
        and mutation_authority_packet.get("mode") == "dry_run_candidate"
        and mutation_authority_packet.get("schema_version") == "covenant.live-self-change-authority.v1"
        and mutation_authority_packet.get("schema_valid_for_claim_publish") is False
        and rollback_rehearsal.get("mode") == "executed_in_temporary_workspace"
        and rollback_rehearsal.get("status") == "passed"
        and rollback_rehearsal.get("same_change_class") is True
        and trust_boundary == expected_trust_boundary
    )
    return {
        "evidence_state": "present" if evidence_present else "invalid",
        "schema_version": payload.get("schema_version"),
        "mutation_authority_packet": mutation_authority_packet.get("mode", "missing"),
        "rollback_rehearsal_status": rollback_rehearsal.get("status", "missing"),
        "status": payload.get("status", "missing"),
    }

bounded_allowed = not bounded_missing
self_change_dry_run_evidence = read_self_change_dry_run_evidence(self_change_dry_run_summary)
payload = {
    "schema_version": "ao2.rsi-claim-readiness-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "claim_boundary_enforced" if bounded_allowed else "failed",
    "claim_boundary": {
        "bounded_governed_rsi": "allowed" if bounded_allowed else "insufficient_evidence",
        "full_autonomous_self_mutating_rsi": "denied",
    },
    "claims": {
        "bounded_governed_rsi": {
            "decision": "allowed" if bounded_allowed else "insufficient_evidence",
            "evidence_state": "present" if bounded_allowed else "missing_required_evidence",
            "evidence": bounded_present,
            "missing_evidence": bounded_missing,
        },
        "full_autonomous_self_mutating_rsi": {
            "decision": "denied",
            "evidence_state": "missing_required_evidence",
            "partial_evidence": {
                "governed_self_change_dry_run": self_change_dry_run_evidence,
            },
            "blockers": full_blockers,
        },
    },
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "publishes_claims": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "claim_boundary_enforced":
    raise SystemExit(1)
PY
