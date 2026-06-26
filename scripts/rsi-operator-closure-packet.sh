#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RSI_OPERATOR_CLOSURE_ROOT="${AO2_RSI_OPERATOR_CLOSURE_ROOT:-$ROOT/target/rsi-operator-closure-packet/latest}"
AO2_RSI_OPERATOR_CLOSURE_RSI_SUMMARY="${AO2_RSI_OPERATOR_CLOSURE_RSI_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/summary.json}"
AO2_RSI_OPERATOR_CLOSURE_CONTROL_PLANE_READBACK="${AO2_RSI_OPERATOR_CLOSURE_CONTROL_PLANE_READBACK:-$ROOT/../ao2-control-plane/target/ao-stack-rsi-chain-binding-readback/latest/summary.json}"

SUMMARY="$AO2_RSI_OPERATOR_CLOSURE_ROOT/summary.json"
CLOSURE="$AO2_RSI_OPERATOR_CLOSURE_ROOT/closure.md"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --out-root)
      AO2_RSI_OPERATOR_CLOSURE_ROOT="${2:-}"
      if [ -z "$AO2_RSI_OPERATOR_CLOSURE_ROOT" ]; then
        echo "--out-root requires a path" >&2
        exit 2
      fi
      SUMMARY="$AO2_RSI_OPERATOR_CLOSURE_ROOT/summary.json"
      CLOSURE="$AO2_RSI_OPERATOR_CLOSURE_ROOT/closure.md"
      shift 2
      ;;
    --rsi-summary)
      AO2_RSI_OPERATOR_CLOSURE_RSI_SUMMARY="${2:-}"
      if [ -z "$AO2_RSI_OPERATOR_CLOSURE_RSI_SUMMARY" ]; then
        echo "--rsi-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --control-plane-readback)
      AO2_RSI_OPERATOR_CLOSURE_CONTROL_PLANE_READBACK="${2:-}"
      if [ -z "$AO2_RSI_OPERATOR_CLOSURE_CONTROL_PLANE_READBACK" ]; then
        echo "--control-plane-readback requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--out-root <path>] [--rsi-summary <path>] [--control-plane-readback <path>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$AO2_RSI_OPERATOR_CLOSURE_ROOT"
mkdir -p "$AO2_RSI_OPERATOR_CLOSURE_ROOT"

python3 - "$AO2_RSI_OPERATOR_CLOSURE_RSI_SUMMARY" \
  "$AO2_RSI_OPERATOR_CLOSURE_CONTROL_PLANE_READBACK" \
  "$SUMMARY" \
  "$CLOSURE" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

PACKET_SCHEMA = "ao2.rsi-operator-closure-packet.v1"
RSI_SCHEMA = "ao2.rsi-cross-repo-e2e.v1"
CONTROL_PLANE_SCHEMA = "ao2.cp-ao-stack-rsi-chain-binding-readback.v1"
FOUNDRY_PACKET_SCHEMA = "ao.foundry.rsi-control-surface-packet.v0.1"
COVENANT_SCHEMA = "covenant.rsi-claim-publish-gate.v1"

rsi_path = Path(sys.argv[1]).resolve()
control_plane_path = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
closure_path = Path(sys.argv[4]).resolve()


def load_json(path: Path, code: str, blockers: list[dict]) -> dict:
    if not path.is_file():
        blockers.append({"code": code, "severity": "blocking", "path": str(path)})
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        blockers.append(
            {
                "code": f"{code}_invalid_json",
                "severity": "blocking",
                "path": str(path),
                "detail": str(exc),
            }
        )
        return {}


def chain_stage(summary: dict, stage: str) -> dict:
    chain = summary.get("chain_binding")
    if not isinstance(chain, list):
        return {}
    for item in chain:
        if isinstance(item, dict) and item.get("stage") == stage:
            return item
    return {}


blockers: list[dict] = []
rsi = load_json(rsi_path, "ao2_rsi_cross_repo_summary_missing", blockers)
control_plane = load_json(
    control_plane_path, "control_plane_chain_binding_readback_missing", blockers
)

rsi_trust = rsi.get("trust_boundary") if isinstance(rsi.get("trust_boundary"), dict) else {}
foundry_readback = (
    rsi.get("control_plane_foundry_packet_readback")
    if isinstance(rsi.get("control_plane_foundry_packet_readback"), dict)
    else {}
)
cp_trust = (
    control_plane.get("trust_boundary")
    if isinstance(control_plane.get("trust_boundary"), dict)
    else {}
)
cp_foundry = chain_stage(control_plane, "foundry_control_surface_packet")
cp_covenant = chain_stage(control_plane, "covenant_claim_decision")
cp_ao2 = chain_stage(control_plane, "ao2_execution_evidence")
cp_readback = chain_stage(control_plane, "control_plane_readback")

ao2_boundary_ok = (
    rsi.get("schema_version") == RSI_SCHEMA
    and rsi.get("status") == "passed"
    and rsi.get("claim_publish_decision") == "deny"
    and rsi.get("claim_publish_authority") is False
    and rsi_trust.get("publishes_claims") is False
    and rsi_trust.get("approves_rsi_claims") is False
    and rsi_trust.get("requires_provider_api_key") is False
    and rsi_trust.get("stores_credentials") is False
)
if rsi and not ao2_boundary_ok:
    blockers.append(
        {
            "code": "ao2_claim_publish_boundary_not_denied",
            "severity": "blocking",
            "schema_version": rsi.get("schema_version"),
            "status": rsi.get("status"),
            "claim_publish_decision": rsi.get("claim_publish_decision"),
            "claim_publish_authority": rsi.get("claim_publish_authority"),
        }
    )

foundry_readback_ok = (
    foundry_readback.get("schema_version") == CONTROL_PLANE_SCHEMA
    and foundry_readback.get("status") == "observer_supported"
    and foundry_readback.get("foundry_packet_schema_version") == FOUNDRY_PACKET_SCHEMA
    and foundry_readback.get("foundry_control_surface_packet_consumed_by_control_plane")
    is True
    and foundry_readback.get("control_plane_observer_only") is True
    and foundry_readback.get("claim_publish_decision") == "deny"
    and foundry_readback.get("claim_publish_authority") is False
    and foundry_readback.get("approves_rsi_claims") is False
    and foundry_readback.get("publishes_claims") is False
)
if rsi and not foundry_readback_ok:
    blockers.append(
        {
            "code": "ao2_foundry_packet_readback_not_observer_supported",
            "severity": "blocking",
            "schema_version": foundry_readback.get("schema_version"),
            "status": foundry_readback.get("status"),
            "foundry_packet_schema_version": foundry_readback.get(
                "foundry_packet_schema_version"
            ),
            "control_plane_observer_only": foundry_readback.get(
                "control_plane_observer_only"
            ),
        }
    )

control_plane_boundary_ok = (
    control_plane.get("schema_version") == CONTROL_PLANE_SCHEMA
    and control_plane.get("status") == "passed"
    and cp_foundry.get("schema_version") == FOUNDRY_PACKET_SCHEMA
    and cp_foundry.get("bounded_governed_rsi") == "supported"
    and cp_foundry.get("full_autonomous_self_mutating_rsi") == "denied"
    and cp_foundry.get("control_plane_observer_only") is True
    and cp_foundry.get("publishes_full_autonomous_rsi_claim") is False
    and cp_covenant.get("schema_version") == COVENANT_SCHEMA
    and cp_covenant.get("decision") == "deny"
    and cp_covenant.get("publish_authority") is False
    and cp_ao2.get("schema_version") == RSI_SCHEMA
    and cp_ao2.get("status") == "passed"
    and cp_readback.get("status") == "passed"
    and cp_readback.get("control_plane_approves_rsi_claims") is False
    and cp_readback.get("publishes_claims") is False
    and cp_trust.get("control_plane_mutates_repositories") is False
    and cp_trust.get("control_plane_publishes_claims") is False
    and cp_trust.get("control_plane_approves_rsi_claims") is False
    and cp_trust.get("control_plane_executes_ao_work") is False
    and cp_trust.get("provider_api_keys_allowed") is False
)
if control_plane and not control_plane_boundary_ok:
    blockers.append(
        {
            "code": "control_plane_chain_binding_not_observer_only",
            "severity": "blocking",
            "schema_version": control_plane.get("schema_version"),
            "status": control_plane.get("status"),
            "foundry_bounded_governed_rsi": cp_foundry.get("bounded_governed_rsi"),
            "foundry_full_autonomous_self_mutating_rsi": cp_foundry.get(
                "full_autonomous_self_mutating_rsi"
            ),
            "covenant_decision": cp_covenant.get("decision"),
            "control_plane_readback_status": cp_readback.get("status"),
        }
    )

ready = not blockers
stable_boundary = {
    "bounded_governed_rsi": "supported" if ready else "unknown",
    "full_autonomous_self_mutating_rsi": "denied"
    if ready or rsi.get("claim_publish_decision") == "deny"
    else "unknown",
    "claim_publish_decision": rsi.get("claim_publish_decision", "missing"),
    "claim_publish_authority": bool(rsi.get("claim_publish_authority")),
    "control_plane_observer_only": bool(
        foundry_readback.get("control_plane_observer_only")
        and cp_foundry.get("control_plane_observer_only")
    ),
}

payload = {
    "schema_version": PACKET_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "operator_closure_ready": ready,
    "stable_boundary": stable_boundary,
    "sources": {
        "ao2_cross_repo_e2e_summary": str(rsi_path),
        "control_plane_chain_binding_readback": str(control_plane_path),
    },
    "source_readbacks": {
        "ao2_cross_repo_e2e": {
            "schema_version": rsi.get("schema_version"),
            "status": rsi.get("status"),
            "claim_publish_decision": rsi.get("claim_publish_decision"),
            "claim_publish_authority": rsi.get("claim_publish_authority"),
            "measured_improvement_percent": rsi.get("improvement_evidence", {}).get(
                "measured_improvement_percent"
            )
            if isinstance(rsi.get("improvement_evidence"), dict)
            else None,
            "target_percent": rsi.get("improvement_evidence", {}).get("target_percent")
            if isinstance(rsi.get("improvement_evidence"), dict)
            else None,
        },
        "foundry_control_surface_packet": {
            "schema_version": cp_foundry.get("schema_version")
            or foundry_readback.get("foundry_packet_schema_version"),
            "bounded_governed_rsi": cp_foundry.get("bounded_governed_rsi"),
            "full_autonomous_self_mutating_rsi": cp_foundry.get(
                "full_autonomous_self_mutating_rsi"
            ),
            "control_plane_observer_only": cp_foundry.get("control_plane_observer_only"),
        },
        "control_plane_chain_binding": {
            "schema_version": control_plane.get("schema_version"),
            "status": control_plane.get("status"),
            "control_plane_readback_status": cp_readback.get("status"),
            "control_plane_approves_rsi_claims": cp_readback.get(
                "control_plane_approves_rsi_claims"
            ),
            "publishes_claims": cp_readback.get("publishes_claims"),
        },
    },
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
        "executes_ao_work": False,
        "control_plane_observer_only": True,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# AO2 RSI Operator Closure Packet",
    "",
    f"Status: {payload['status']}",
    "",
    "- bounded governed RSI is supported when AO2, Foundry, Covenant, and control-plane readbacks all agree.",
    "- full autonomous RSI publication remains denied by policy and evidence boundaries.",
    "- control-plane remains observer-only; it does not approve RSI claims, publish claims, execute AO work, or mutate repositories.",
    "",
    "## Sources",
    "",
    f"- AO2 cross-repo E2E: `{rsi_path}`",
    f"- control-plane chain-binding readback: `{control_plane_path}`",
    "",
    "## Stable Boundary",
    "",
    f"- bounded_governed_rsi: `{stable_boundary['bounded_governed_rsi']}`",
    f"- full_autonomous_self_mutating_rsi: `{stable_boundary['full_autonomous_self_mutating_rsi']}`",
    f"- claim_publish_decision: `{stable_boundary['claim_publish_decision']}`",
    f"- claim_publish_authority: `{str(stable_boundary['claim_publish_authority']).lower()}`",
    f"- control_plane_observer_only: `{str(stable_boundary['control_plane_observer_only']).lower()}`",
]
if blockers:
    lines.extend(["", "## Blockers", ""])
    lines.extend(f"- {item['code']}" for item in blockers)
closure_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"closure={closure_path}")
print(f"rsi_operator_closure_packet={payload['status']}")
print(
    "bounded_governed_rsi="
    f"{stable_boundary['bounded_governed_rsi']} "
    "full_autonomous_self_mutating_rsi="
    f"{stable_boundary['full_autonomous_self_mutating_rsi']} "
    "claim_publish_decision="
    f"{stable_boundary['claim_publish_decision']} "
    "control_plane_observer_only="
    f"{str(stable_boundary['control_plane_observer_only']).lower()}"
)
if not ready:
    for blocker in blockers:
        print(f"blocker={blocker['code']}", file=sys.stderr)
    raise SystemExit(1)
PY
