#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT="${AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT:-$ROOT/target/rsi-improvement-evidence-gate/latest}"
AO2_RSI_IMPROVEMENT_BASELINE_CHECK_COUNT="${AO2_RSI_IMPROVEMENT_BASELINE_CHECK_COUNT:-6}"
AO2_RSI_IMPROVEMENT_TARGET_PERCENT="${AO2_RSI_IMPROVEMENT_TARGET_PERCENT:-5}"
AO2_RSI_IMPROVEMENT_LIVE_SUMMARY="${AO2_RSI_IMPROVEMENT_LIVE_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/live-self-change-rehearsal/summary.json}"
AO2_RSI_IMPROVEMENT_READBACK_SUMMARY="${AO2_RSI_IMPROVEMENT_READBACK_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/control-plane-readback/summary.json}"
AO2_RSI_IMPROVEMENT_READBACK_INDEX_SUMMARY="${AO2_RSI_IMPROVEMENT_READBACK_INDEX_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/readback-index/summary.json}"
AO2_RSI_IMPROVEMENT_CLAIM_READINESS_SUMMARY="${AO2_RSI_IMPROVEMENT_CLAIM_READINESS_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/claim-readiness/summary.json}"
AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY="${AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/covenant-gate/summary.json}"
AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE="${AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE:-$ROOT/target/rsi-cross-repo-e2e/latest/logs/covenant_gate_schema_validate.log.exit-code}"

SUMMARY="$AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT/summary.json"

rm -rf "$AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT"
mkdir -p "$AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT"

python3 - "$SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_BASELINE_CHECK_COUNT" \
  "$AO2_RSI_IMPROVEMENT_TARGET_PERCENT" \
  "$AO2_RSI_IMPROVEMENT_LIVE_SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_READBACK_SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_READBACK_INDEX_SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_CLAIM_READINESS_SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_path = Path(sys.argv[1]).resolve()
baseline_check_count = int(sys.argv[2])
target_percent = float(sys.argv[3])
live_path = Path(sys.argv[4]).resolve()
readback_path = Path(sys.argv[5]).resolve()
readback_index_path = Path(sys.argv[6]).resolve()
claim_path = Path(sys.argv[7]).resolve()
covenant_gate_path = Path(sys.argv[8]).resolve()
covenant_schema_exit_path = Path(sys.argv[9]).resolve()


def load_json(path: Path) -> dict:
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def check(name: str, path: Path, passed: bool, details: dict) -> dict:
    return {
        "name": name,
        "status": "passed" if passed else "failed",
        "path": str(path),
        "details": details,
    }


live = load_json(live_path)
readback = load_json(readback_path)
readback_index = load_json(readback_index_path)
claim = load_json(claim_path)
covenant_gate = load_json(covenant_gate_path)
covenant_schema_exit = (
    covenant_schema_exit_path.read_text(encoding="utf-8").strip()
    if covenant_schema_exit_path.is_file()
    else "missing"
)

evidence_checks = [
    check(
        "live_self_change_rehearsal",
        live_path,
        live.get("schema_version") == "ao2.rsi-live-self-change-rehearsal.v1"
        and live.get("status") == "live_rehearsal_passed"
        and live.get("self_change", {}).get("repository_restored") is True,
        {
            "schema_version": live.get("schema_version"),
            "status": live.get("status"),
            "repository_restored": live.get("self_change", {}).get("repository_restored"),
        },
    ),
    check(
        "control_plane_readback",
        readback_path,
        readback.get("schema_version") == "ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1"
        and readback.get("status") == "passed",
        {
            "schema_version": readback.get("schema_version"),
            "status": readback.get("status"),
        },
    ),
    check(
        "readback_index",
        readback_index_path,
        readback_index.get("schema_version") == "ao2.rsi-live-self-change-readback-evidence-index.v1"
        and readback_index.get("status") == "passed",
        {
            "schema_version": readback_index.get("schema_version"),
            "status": readback_index.get("status"),
        },
    ),
    check(
        "claim_readiness",
        claim_path,
        claim.get("schema_version") == "ao2.rsi-claim-readiness-audit.v1"
        and claim.get("status") == "claim_boundary_enforced",
        {
            "schema_version": claim.get("schema_version"),
            "status": claim.get("status"),
        },
    ),
    check(
        "covenant_claim_publish_gate",
        covenant_gate_path,
        covenant_gate.get("schema_version") == "covenant.rsi-claim-publish-gate.v1"
        and covenant_gate.get("status") == "denied"
        and covenant_gate.get("decision") == "deny"
        and covenant_gate.get("publish_authority") is False,
        {
            "schema_version": covenant_gate.get("schema_version"),
            "status": covenant_gate.get("status"),
            "decision": covenant_gate.get("decision"),
            "publish_authority": covenant_gate.get("publish_authority"),
        },
    ),
    check(
        "covenant_gate_schema_validate",
        covenant_schema_exit_path,
        covenant_schema_exit == "0",
        {"exit_code": covenant_schema_exit},
    ),
]

observed_check_count = len(evidence_checks) + 1
measured_improvement_percent = (
    ((observed_check_count - baseline_check_count) / baseline_check_count) * 100.0
    if baseline_check_count > 0
    else 0.0
)
claim_publish_authority = bool(covenant_gate.get("publish_authority"))
claim_publish_decision = covenant_gate.get("decision", "missing")
blockers = []

if baseline_check_count <= 0:
    blockers.append({"code": "invalid_baseline_check_count", "severity": "blocking"})
if measured_improvement_percent < target_percent:
    blockers.append(
        {
            "code": "improvement_below_target",
            "severity": "blocking",
            "target_percent": target_percent,
            "measured_improvement_percent": measured_improvement_percent,
        }
    )
if any(item["status"] != "passed" for item in evidence_checks):
    blockers.append({"code": "evidence_check_failed", "severity": "blocking"})
if claim_publish_decision != "deny" or claim_publish_authority is not False:
    blockers.append(
        {
            "code": "claim_publish_boundary_not_denied",
            "severity": "blocking",
            "claim_publish_decision": claim_publish_decision,
            "claim_publish_authority": claim_publish_authority,
        }
    )

ready = not blockers
payload = {
    "schema_version": "ao2.rsi-improvement-evidence-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "improvement_ready": ready,
    "claim_level": "full_autonomous_self_mutating_rsi",
    "claim_publish_decision": claim_publish_decision,
    "claim_publish_authority": claim_publish_authority,
    "metric": {
        "unit": "enforced_rsi_evidence_checks",
        "baseline_check_count": baseline_check_count,
        "observed_check_count": observed_check_count,
        "target_percent": target_percent,
        "measured_improvement_percent": round(measured_improvement_percent, 4),
    },
    "evidence_checks": evidence_checks,
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"rsi_improvement_evidence_gate={payload['status']}")
print(
    "measured_improvement_percent="
    f"{payload['metric']['measured_improvement_percent']} "
    f"target_percent={payload['metric']['target_percent']}"
)
print(
    "claim_level=full_autonomous_self_mutating_rsi "
    f"decision={claim_publish_decision} "
    f"publish_authority={str(claim_publish_authority).lower()}"
)
if not ready:
    for blocker in blockers:
        print(f"blocker={blocker['code']}", file=sys.stderr)
    raise SystemExit(1)
PY
