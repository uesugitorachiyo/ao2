#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT:-$ROOT/target/stable-release-evidence-packet/latest}"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY:-$ROOT/target/stable-promotion-workflow/latest/summary.json}"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY:-$ROOT/target/operator-release-evidence-bundle/latest/summary.json}"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_SUMMARY="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/summary.json}"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_ELIGIBILITY_SUMMARY="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_ELIGIBILITY_SUMMARY:-$ROOT/target/rsi-eligibility-packet/latest/summary.json}"
SUMMARY="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/summary.json"
DASHBOARD="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/dashboard.html"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --out-root)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT" ]; then
        echo "--out-root requires a path" >&2
        exit 2
      fi
      SUMMARY="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/summary.json"
      DASHBOARD="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/dashboard.html"
      shift 2
      ;;
    --stable-summary)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY" ]; then
        echo "--stable-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --operator-summary)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY" ]; then
        echo "--operator-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --rsi-summary)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_SUMMARY="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_SUMMARY" ]; then
        echo "--rsi-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --rsi-eligibility-summary)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_ELIGIBILITY_SUMMARY="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_ELIGIBILITY_SUMMARY" ]; then
        echo "--rsi-eligibility-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--out-root <path>] [--stable-summary <path>] [--operator-summary <path>] [--rsi-summary <path>] [--rsi-eligibility-summary <path>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT"
mkdir -p "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT"

python3 - "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY" \
  "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY" \
  "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_SUMMARY" \
  "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_ELIGIBILITY_SUMMARY" \
  "$SUMMARY" "$DASHBOARD" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

STABLE_SCHEMA = "ao2.stable-promotion-workflow.v1"
OPERATOR_SCHEMA = "ao2.operator-release-evidence-bundle.v1"
RSI_SCHEMA = "ao2.rsi-cross-repo-e2e.v1"
RSI_BLUEPRINT_AUTHORIZATION_SCHEMA = "ao2.rsi-blueprint-authorization-gate.v1"
RSI_IMPROVEMENT_SCHEMA = "ao2.rsi-improvement-evidence-gate.v1"
RSI_IMPROVEMENT_TREND_SCHEMA = "ao2.rsi-improvement-trend.v1"
RSI_ELIGIBILITY_SCHEMA = "ao2.rsi-eligibility-packet.v1"
RSI_COVENANT_GATE_SCHEMA = "covenant.rsi-claim-publish-gate.v1"
PACKET_SCHEMA = "ao2.stable-release-evidence-packet.v1"

stable_summary_path = Path(sys.argv[1]).resolve()
operator_summary_path = Path(sys.argv[2]).resolve()
rsi_summary_path = Path(sys.argv[3]).resolve()
rsi_eligibility_summary_path = Path(sys.argv[4]).resolve()
summary_path = Path(sys.argv[5]).resolve()
dashboard_path = Path(sys.argv[6]).resolve()


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


blockers = []
stable = {}
operator = {}
rsi = {}
rsi_eligibility = {}

if not stable_summary_path.is_file():
    blockers.append(
        {
            "code": "stable_promotion_summary_missing",
            "severity": "blocking",
            "path": str(stable_summary_path),
        }
    )
else:
    stable = load_json(stable_summary_path)

if not operator_summary_path.is_file():
    blockers.append(
        {
            "code": "operator_evidence_summary_missing",
            "severity": "blocking",
            "path": str(operator_summary_path),
        }
    )
else:
    operator = load_json(operator_summary_path)

if not rsi_summary_path.is_file():
    blockers.append(
        {
            "code": "rsi_cross_repo_e2e_summary_missing",
            "severity": "blocking",
            "path": str(rsi_summary_path),
        }
    )
else:
    rsi = load_json(rsi_summary_path)

if not rsi_eligibility_summary_path.is_file():
    blockers.append(
        {
            "code": "rsi_eligibility_summary_missing",
            "severity": "blocking",
            "path": str(rsi_eligibility_summary_path),
        }
    )
else:
    rsi_eligibility = load_json(rsi_eligibility_summary_path)

stable_schema_ok = stable.get("schema_version") == STABLE_SCHEMA
stable_status = stable.get("status")
stable_ready = (
    stable_schema_ok
    and stable_status in {"already_stable", "ready_to_promote"}
    and stable.get("post_release_evidence_ready") is True
    and stable.get("evidence_gate_status") == "passed"
)
operator_schema_ok = operator.get("schema_version") == OPERATOR_SCHEMA
operator_checks = operator.get("checks") if isinstance(operator.get("checks"), list) else []
passed_operator_checks = [
    check for check in operator_checks if check.get("status") == "passed"
]
public_pair_digest_check = next(
    (
        check
        for check in operator_checks
        if check.get("artifact") == "ao2-public-release-pair-digest-audit"
    ),
    {},
)
public_pair_digest_ready = (
    public_pair_digest_check.get("status") == "passed"
    and public_pair_digest_check.get("schema_version")
    == "ao2.public-release-pair-digest-audit.v1"
    and public_pair_digest_check.get("archive_parity_status") == "passed"
)
operator_ready = (
    operator_schema_ok
    and operator.get("status") == "passed"
    and operator.get("operator_release_evidence_ready") is True
    and len(passed_operator_checks) == len(operator_checks)
)
rsi_schema_ok = rsi.get("schema_version") == RSI_SCHEMA
rsi_claim_publish_denied = (
    rsi_schema_ok
    and rsi.get("status") == "passed"
    and rsi.get("claim_publish_decision") == "deny"
    and rsi.get("claim_publish_authority") is False
    and rsi.get("observed_evidence", {}).get("covenant_gate_schema_version")
    == RSI_COVENANT_GATE_SCHEMA
    and rsi.get("observed_evidence", {}).get("covenant_gate_status") == "denied"
    and rsi.get("trust_boundary", {}).get("requires_provider_api_key") is False
    and rsi.get("trust_boundary", {}).get("stores_credentials") is False
    and rsi.get("trust_boundary", {}).get("publishes_claims") is False
    and rsi.get("trust_boundary", {}).get("approves_rsi_claims") is False
)
rsi_improvement = (
    rsi.get("improvement_evidence")
    if isinstance(rsi.get("improvement_evidence"), dict)
    else {}
)
rsi_blueprint_authorization = (
    rsi.get("blueprint_authorization")
    if isinstance(rsi.get("blueprint_authorization"), dict)
    else {}
)
rsi_blueprint_authorization_ready = (
    rsi_schema_ok
    and rsi_blueprint_authorization.get("schema_version") == RSI_BLUEPRINT_AUTHORIZATION_SCHEMA
    and rsi_blueprint_authorization.get("status") == "passed"
    and rsi_blueprint_authorization.get("blueprint_authorization_ready") is True
    and rsi_blueprint_authorization.get("gate_model") == "tiered"
    and rsi_blueprint_authorization.get("source") == "ao-blueprint"
    and rsi_blueprint_authorization.get("self_authorized_by_rsi") is False
    and rsi_blueprint_authorization.get("authorizes_claim_publication") is False
    and rsi_blueprint_authorization.get("authorizes_ao_blueprint_self_change") is False
)
rsi_improvement_metric_ok = (
    isinstance(rsi_improvement.get("measured_improvement_percent"), (int, float))
    and isinstance(rsi_improvement.get("target_percent"), (int, float))
    and rsi_improvement.get("measured_improvement_percent")
    >= rsi_improvement.get("target_percent")
    and rsi_improvement.get("target_percent") >= 5
)
rsi_improvement_ready = (
    rsi_schema_ok
    and rsi_improvement.get("schema_version") == RSI_IMPROVEMENT_SCHEMA
    and rsi_improvement.get("status") == "passed"
    and rsi_improvement.get("improvement_ready") is True
    and rsi_improvement.get("unit") == "enforced_rsi_evidence_checks"
    and rsi_improvement_metric_ok
    and rsi_improvement.get("claim_publish_decision") == "deny"
    and rsi_improvement.get("claim_publish_authority") is False
)
rsi_improvement_trend = (
    rsi.get("improvement_trend")
    if isinstance(rsi.get("improvement_trend"), dict)
    else {}
)
rsi_improvement_trend_ready = (
    rsi_schema_ok
    and rsi_improvement_trend.get("schema_version") == RSI_IMPROVEMENT_TREND_SCHEMA
    and rsi_improvement_trend.get("status") == "passed"
    and rsi_improvement_trend.get("trend_ready") is True
    and isinstance(rsi_improvement_trend.get("current_measured_improvement_percent"), (int, float))
    and isinstance(rsi_improvement_trend.get("target_percent"), (int, float))
    and rsi_improvement_trend.get("current_measured_improvement_percent")
    >= rsi_improvement_trend.get("target_percent")
    and rsi_improvement_trend.get("target_percent") >= 5
    and rsi_improvement_trend.get("claim_publish_decision") == "deny"
    and rsi_improvement_trend.get("claim_publish_authority") is False
)
rsi_eligibility_blueprint = (
    rsi_eligibility.get("blueprint_authorization")
    if isinstance(rsi_eligibility.get("blueprint_authorization"), dict)
    else {}
)
rsi_eligibility_improvement = (
    rsi_eligibility.get("improvement_evidence")
    if isinstance(rsi_eligibility.get("improvement_evidence"), dict)
    else {}
)
rsi_eligibility_trust = (
    rsi_eligibility.get("trust_boundary")
    if isinstance(rsi_eligibility.get("trust_boundary"), dict)
    else {}
)
rsi_eligibility_ready = (
    rsi_eligibility.get("schema_version") == RSI_ELIGIBILITY_SCHEMA
    and rsi_eligibility.get("status") == "passed"
    and rsi_eligibility.get("rsi_eligibility_ready") is True
    and isinstance(rsi_eligibility.get("baseline_count"), int)
    and isinstance(rsi_eligibility.get("minimum_baseline_count"), int)
    and rsi_eligibility.get("baseline_count") >= rsi_eligibility.get("minimum_baseline_count")
    and rsi_eligibility.get("minimum_baseline_count") >= 2
    and rsi_eligibility.get("claim_publish_decision") == "deny"
    and rsi_eligibility.get("claim_publish_authority") is False
    and rsi_eligibility_blueprint.get("source") == "ao-blueprint"
    and rsi_eligibility_blueprint.get("self_authorized_by_rsi") is False
    and rsi_eligibility_blueprint.get("authorizes_claim_publication") is False
    and rsi_eligibility_blueprint.get("authorizes_ao_blueprint_self_change") is False
    and isinstance(rsi_eligibility_improvement.get("minimum_target_percent"), (int, float))
    and isinstance(rsi_eligibility_improvement.get("minimum_measured_improvement_percent"), (int, float))
    and rsi_eligibility_improvement.get("minimum_target_percent") >= 5
    and rsi_eligibility_improvement.get("minimum_measured_improvement_percent")
    >= rsi_eligibility_improvement.get("minimum_target_percent")
    and rsi_eligibility_trust.get("publishes_claims") is False
    and rsi_eligibility_trust.get("approves_rsi_claims") is False
    and rsi_eligibility_trust.get("mutates_repositories") is False
    and rsi_eligibility_trust.get("requires_provider_api_key") is False
)

if stable and not stable_schema_ok:
    blockers.append(
        {
            "code": "stable_promotion_schema_mismatch",
            "severity": "blocking",
            "expected": STABLE_SCHEMA,
            "actual": stable.get("schema_version"),
        }
    )
if stable and not stable_ready:
    blockers.append(
        {
            "code": "stable_promotion_not_ready",
            "severity": "blocking",
            "status": stable_status,
            "post_release_evidence_ready": stable.get("post_release_evidence_ready"),
            "evidence_gate_status": stable.get("evidence_gate_status"),
        }
    )
if operator and not operator_schema_ok:
    blockers.append(
        {
            "code": "operator_evidence_schema_mismatch",
            "severity": "blocking",
            "expected": OPERATOR_SCHEMA,
            "actual": operator.get("schema_version"),
        }
    )
if operator and not operator_ready:
    blockers.append(
        {
            "code": "operator_evidence_not_ready",
            "severity": "blocking",
            "status": operator.get("status"),
            "operator_release_evidence_ready": operator.get("operator_release_evidence_ready"),
            "passed_check_count": len(passed_operator_checks),
            "check_count": len(operator_checks),
        }
    )
if operator and not public_pair_digest_ready:
    blockers.append(
        {
            "code": "public_pair_digest_audit_not_ready",
            "severity": "blocking",
            "artifact": public_pair_digest_check.get("artifact"),
            "status": public_pair_digest_check.get("status"),
            "schema_version": public_pair_digest_check.get("schema_version"),
            "archive_parity_status": public_pair_digest_check.get("archive_parity_status"),
        }
    )
if rsi and not rsi_schema_ok:
    blockers.append(
        {
            "code": "rsi_cross_repo_e2e_schema_mismatch",
            "severity": "blocking",
            "expected": RSI_SCHEMA,
            "actual": rsi.get("schema_version"),
        }
    )
if rsi and not rsi_claim_publish_denied:
    blockers.append(
        {
            "code": "rsi_claim_publish_boundary_not_denied",
            "severity": "blocking",
            "status": rsi.get("status"),
            "claim_publish_decision": rsi.get("claim_publish_decision"),
            "claim_publish_authority": rsi.get("claim_publish_authority"),
            "covenant_gate_schema_version": rsi.get("observed_evidence", {}).get("covenant_gate_schema_version"),
            "covenant_gate_status": rsi.get("observed_evidence", {}).get("covenant_gate_status"),
        }
    )
if rsi and not rsi_blueprint_authorization_ready:
    blockers.append(
        {
            "code": "rsi_blueprint_authorization_not_ready",
            "severity": "blocking",
            "schema_version": rsi_blueprint_authorization.get("schema_version"),
            "status": rsi_blueprint_authorization.get("status"),
            "gate_model": rsi_blueprint_authorization.get("gate_model"),
            "self_authorized_by_rsi": rsi_blueprint_authorization.get("self_authorized_by_rsi"),
            "authorizes_claim_publication": rsi_blueprint_authorization.get("authorizes_claim_publication"),
            "authorizes_ao_blueprint_self_change": rsi_blueprint_authorization.get("authorizes_ao_blueprint_self_change"),
        }
    )
if rsi and not rsi_improvement_ready:
    blockers.append(
        {
            "code": "rsi_improvement_evidence_not_ready",
            "severity": "blocking",
            "schema_version": rsi_improvement.get("schema_version"),
            "status": rsi_improvement.get("status"),
            "improvement_ready": rsi_improvement.get("improvement_ready"),
            "target_percent": rsi_improvement.get("target_percent"),
            "measured_improvement_percent": rsi_improvement.get("measured_improvement_percent"),
            "claim_publish_decision": rsi_improvement.get("claim_publish_decision"),
            "claim_publish_authority": rsi_improvement.get("claim_publish_authority"),
        }
    )
if rsi and not rsi_improvement_trend_ready:
    blockers.append(
        {
            "code": "rsi_improvement_trend_not_ready",
            "severity": "blocking",
            "schema_version": rsi_improvement_trend.get("schema_version"),
            "status": rsi_improvement_trend.get("status"),
            "trend_ready": rsi_improvement_trend.get("trend_ready"),
            "target_percent": rsi_improvement_trend.get("target_percent"),
            "current_measured_improvement_percent": rsi_improvement_trend.get("current_measured_improvement_percent"),
            "claim_publish_decision": rsi_improvement_trend.get("claim_publish_decision"),
            "claim_publish_authority": rsi_improvement_trend.get("claim_publish_authority"),
        }
    )
if rsi_eligibility and not rsi_eligibility_ready:
    blockers.append(
        {
            "code": "rsi_eligibility_packet_not_ready",
            "severity": "blocking",
            "schema_version": rsi_eligibility.get("schema_version"),
            "status": rsi_eligibility.get("status"),
            "rsi_eligibility_ready": rsi_eligibility.get("rsi_eligibility_ready"),
            "baseline_count": rsi_eligibility.get("baseline_count"),
            "minimum_baseline_count": rsi_eligibility.get("minimum_baseline_count"),
            "claim_publish_decision": rsi_eligibility.get("claim_publish_decision"),
            "claim_publish_authority": rsi_eligibility.get("claim_publish_authority"),
        }
    )

source_trust = [
    stable.get("trust_boundary", {}) if isinstance(stable.get("trust_boundary"), dict) else {},
    operator.get("trust_boundary", {}) if isinstance(operator.get("trust_boundary"), dict) else {},
    rsi.get("trust_boundary", {}) if isinstance(rsi.get("trust_boundary"), dict) else {},
    rsi_eligibility.get("trust_boundary", {}) if isinstance(rsi_eligibility.get("trust_boundary"), dict) else {},
]
stable_release_evidence_ready = (
    stable_ready
    and operator_ready
    and rsi_claim_publish_denied
    and rsi_blueprint_authorization_ready
    and rsi_improvement_ready
    and rsi_improvement_trend_ready
    and rsi_eligibility_ready
    and not blockers
)
payload = {
    "schema_version": PACKET_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if stable_release_evidence_ready else "failed",
    "stable_release_evidence_ready": stable_release_evidence_ready,
    "sources": {
        "stable_promotion_summary": str(stable_summary_path),
        "operator_evidence_summary": str(operator_summary_path),
        "rsi_cross_repo_e2e_summary": str(rsi_summary_path),
        "rsi_eligibility_summary": str(rsi_eligibility_summary_path),
    },
    "stable_promotion": {
        "schema_version": stable.get("schema_version"),
        "status": stable_status,
        "post_release_evidence_ready": stable.get("post_release_evidence_ready"),
        "evidence_gate_status": stable.get("evidence_gate_status"),
        "promotion_status": stable.get("promotion_status"),
        "blocker_count": len(stable.get("blockers", []))
        if isinstance(stable.get("blockers"), list)
        else None,
        "components": stable.get("components", []),
    },
    "operator_evidence": {
        "schema_version": operator.get("schema_version"),
        "status": operator.get("status"),
        "operator_release_evidence_ready": operator.get("operator_release_evidence_ready"),
        "check_count": len(operator_checks),
        "passed_check_count": len(passed_operator_checks),
        "checks": operator_checks,
    },
    "public_pair_digest_audit": {
        "artifact": public_pair_digest_check.get("artifact"),
        "schema_version": public_pair_digest_check.get("schema_version"),
        "status": public_pair_digest_check.get("status"),
        "archive_parity_status": public_pair_digest_check.get("archive_parity_status"),
        "summary": public_pair_digest_check.get("summary"),
    },
    "rsi_cross_repo_e2e": {
        "schema_version": rsi.get("schema_version"),
        "status": rsi.get("status"),
        "claim_level": rsi.get("claim_level"),
        "claim_publish_decision": rsi.get("claim_publish_decision"),
        "claim_publish_authority": rsi.get("claim_publish_authority"),
        "covenant_gate_schema_version": rsi.get("observed_evidence", {}).get("covenant_gate_schema_version"),
        "covenant_gate_status": rsi.get("observed_evidence", {}).get("covenant_gate_status"),
    },
    "rsi_blueprint_authorization": {
        "schema_version": rsi_blueprint_authorization.get("schema_version"),
        "status": rsi_blueprint_authorization.get("status"),
        "blueprint_authorization_ready": rsi_blueprint_authorization.get("blueprint_authorization_ready"),
        "gate_model": rsi_blueprint_authorization.get("gate_model"),
        "candidate_id": rsi_blueprint_authorization.get("candidate_id"),
        "source": rsi_blueprint_authorization.get("source"),
        "self_authorized_by_rsi": rsi_blueprint_authorization.get("self_authorized_by_rsi"),
        "authorizes_claim_publication": rsi_blueprint_authorization.get("authorizes_claim_publication"),
        "authorizes_ao_blueprint_self_change": rsi_blueprint_authorization.get("authorizes_ao_blueprint_self_change"),
    },
    "rsi_improvement_evidence": {
        "schema_version": rsi_improvement.get("schema_version"),
        "status": rsi_improvement.get("status"),
        "improvement_ready": rsi_improvement.get("improvement_ready"),
        "unit": rsi_improvement.get("unit"),
        "baseline_check_count": rsi_improvement.get("baseline_check_count"),
        "observed_check_count": rsi_improvement.get("observed_check_count"),
        "target_percent": rsi_improvement.get("target_percent"),
        "measured_improvement_percent": rsi_improvement.get("measured_improvement_percent"),
        "claim_publish_decision": rsi_improvement.get("claim_publish_decision"),
        "claim_publish_authority": rsi_improvement.get("claim_publish_authority"),
    },
    "rsi_improvement_trend": {
        "schema_version": rsi_improvement_trend.get("schema_version"),
        "status": rsi_improvement_trend.get("status"),
        "trend_ready": rsi_improvement_trend.get("trend_ready"),
        "history_path": rsi_improvement_trend.get("history_path"),
        "run_count": rsi_improvement_trend.get("run_count"),
        "previous_measured_improvement_percent": rsi_improvement_trend.get("previous_measured_improvement_percent"),
        "current_measured_improvement_percent": rsi_improvement_trend.get("current_measured_improvement_percent"),
        "delta_from_previous_percent": rsi_improvement_trend.get("delta_from_previous_percent"),
        "target_percent": rsi_improvement_trend.get("target_percent"),
        "claim_publish_decision": rsi_improvement_trend.get("claim_publish_decision"),
        "claim_publish_authority": rsi_improvement_trend.get("claim_publish_authority"),
    },
    "rsi_eligibility_packet": {
        "schema_version": rsi_eligibility.get("schema_version"),
        "status": rsi_eligibility.get("status"),
        "rsi_eligibility_ready": rsi_eligibility.get("rsi_eligibility_ready"),
        "baseline_count": rsi_eligibility.get("baseline_count"),
        "minimum_baseline_count": rsi_eligibility.get("minimum_baseline_count"),
        "claim_publish_decision": rsi_eligibility.get("claim_publish_decision"),
        "claim_publish_authority": rsi_eligibility.get("claim_publish_authority"),
        "blueprint_authorization": {
            "source": rsi_eligibility_blueprint.get("source"),
            "self_authorized_by_rsi": rsi_eligibility_blueprint.get("self_authorized_by_rsi"),
            "authorizes_claim_publication": rsi_eligibility_blueprint.get("authorizes_claim_publication"),
            "authorizes_ao_blueprint_self_change": rsi_eligibility_blueprint.get("authorizes_ao_blueprint_self_change"),
        },
        "improvement_evidence": {
            "minimum_target_percent": rsi_eligibility_improvement.get("minimum_target_percent"),
            "minimum_measured_improvement_percent": rsi_eligibility_improvement.get("minimum_measured_improvement_percent"),
        },
    },
    "blockers": blockers,
    "trust_boundary": {
        "mutates_releases": False,
        "stores_credentials": False,
        "reads_local_evidence_only": True,
        "source_mutates_releases": [
            trust.get("mutates_releases", trust.get("mutates_github_releases"))
            for trust in source_trust
            if "mutates_releases" in trust or "mutates_github_releases" in trust
        ],
        "source_stores_credentials": [
            trust.get("stores_credentials", trust.get("credential_material_included"))
            for trust in source_trust
            if "stores_credentials" in trust or "credential_material_included" in trust
        ],
    },
    "dashboard": str(dashboard_path),
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

rows = []
for check in operator_checks:
    rows.append(
        "<tr>"
        f"<td>{html.escape(str(check.get('component', '')))}</td>"
        f"<td>{html.escape(str(check.get('platform', '')))}</td>"
        f"<td>{html.escape(str(check.get('artifact', '')))}</td>"
        f"<td>{html.escape(str(check.get('status', '')))}</td>"
        "</tr>"
    )
blocker_rows = []
for blocker in blockers:
    blocker_rows.append(
        "<tr>"
        f"<td>{html.escape(str(blocker.get('code', '')))}</td>"
        f"<td>{html.escape(str(blocker.get('severity', '')))}</td>"
        f"<td><code>{html.escape(json.dumps(blocker, sort_keys=True))}</code></td>"
        "</tr>"
    )
if not blocker_rows:
    blocker_rows.append('<tr><td colspan="3">No blockers</td></tr>')

dashboard_path.write_text(
    f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Stable Release Evidence Packet</title>
  <style>
    body {{ font-family: system-ui, sans-serif; margin: 2rem; color: #111827; }}
    h1, h2 {{ margin-bottom: 0.4rem; }}
    code {{ background: #f3f4f6; padding: 0.1rem 0.25rem; border-radius: 4px; }}
    table {{ border-collapse: collapse; width: 100%; margin: 1rem 0 2rem; }}
    th, td {{ border: 1px solid #d1d5db; padding: 0.5rem; text-align: left; vertical-align: top; }}
    th {{ background: #f9fafb; }}
    .status {{ font-weight: 700; }}
  </style>
</head>
<body>
  <h1>Stable Release Evidence Packet</h1>
  <p><code>{PACKET_SCHEMA}</code></p>
  <p class="status">Status: {html.escape(payload["status"])}</p>
  <p>Stable release evidence ready: {str(stable_release_evidence_ready).lower()}</p>
  <p>Archive parity: {html.escape(str(payload["public_pair_digest_audit"]["archive_parity_status"]))}</p>
  <p>RSI claim-publish boundary: {html.escape(str(payload["rsi_cross_repo_e2e"]["claim_publish_decision"]))}</p>
  <p>RSI Blueprint authorization: {html.escape(str(payload["rsi_blueprint_authorization"]["gate_model"]))} / self-authorized by RSI {html.escape(str(payload["rsi_blueprint_authorization"]["self_authorized_by_rsi"]))}</p>
  <p>RSI improvement evidence: {html.escape(str(payload["rsi_improvement_evidence"]["measured_improvement_percent"]))}% / target {html.escape(str(payload["rsi_improvement_evidence"]["target_percent"]))}%</p>
  <p>RSI improvement trend: current {html.escape(str(payload["rsi_improvement_trend"]["current_measured_improvement_percent"]))}% / delta {html.escape(str(payload["rsi_improvement_trend"]["delta_from_previous_percent"]))}%</p>
  <p>RSI eligibility packet: {html.escape(str(payload["rsi_eligibility_packet"]["rsi_eligibility_ready"]))} / publish authority {html.escape(str(payload["rsi_eligibility_packet"]["claim_publish_authority"]))}</p>
  <h2>Source Summaries</h2>
  <table>
    <tr><th>Source</th><th>Path</th><th>Status</th></tr>
    <tr><td>Stable promotion</td><td><code>{html.escape(str(stable_summary_path))}</code></td><td>{html.escape(str(stable_status))}</td></tr>
    <tr><td>Operator evidence</td><td><code>{html.escape(str(operator_summary_path))}</code></td><td>{html.escape(str(operator.get("status")))}</td></tr>
    <tr><td>RSI cross-repo E2E</td><td><code>{html.escape(str(rsi_summary_path))}</code></td><td>{html.escape(str(rsi.get("status")))}</td></tr>
    <tr><td>RSI eligibility packet</td><td><code>{html.escape(str(rsi_eligibility_summary_path))}</code></td><td>{html.escape(str(rsi_eligibility.get("status")))}</td></tr>
  </table>
  <h2>RSI Claim-Publish Boundary</h2>
  <table>
    <tr><th>Schema</th><th>Decision</th><th>Publish authority</th><th>Covenant gate</th></tr>
    <tr>
      <td><code>{html.escape(str(payload["rsi_cross_repo_e2e"]["schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_cross_repo_e2e"]["claim_publish_decision"]))}</td>
      <td>{html.escape(str(payload["rsi_cross_repo_e2e"]["claim_publish_authority"]))}</td>
      <td><code>{html.escape(str(payload["rsi_cross_repo_e2e"]["covenant_gate_schema_version"]))}</code></td>
    </tr>
  </table>
  <h2>RSI Blueprint Authorization</h2>
  <table>
    <tr><th>Schema</th><th>Status</th><th>Gate model</th><th>Source</th><th>Self-authorized by RSI</th><th>Claim publication</th><th>Blueprint self-change</th></tr>
    <tr>
      <td><code>{html.escape(str(payload["rsi_blueprint_authorization"]["schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["status"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["gate_model"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["source"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["self_authorized_by_rsi"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["authorizes_claim_publication"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["authorizes_ao_blueprint_self_change"]))}</td>
    </tr>
  </table>
  <h2>RSI Improvement Evidence</h2>
  <table>
    <tr><th>Schema</th><th>Status</th><th>Unit</th><th>Observed</th><th>Baseline</th><th>Measured</th><th>Target</th></tr>
    <tr>
      <td><code>{html.escape(str(payload["rsi_improvement_evidence"]["schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_improvement_evidence"]["status"]))}</td>
      <td>{html.escape(str(payload["rsi_improvement_evidence"]["unit"]))}</td>
      <td>{html.escape(str(payload["rsi_improvement_evidence"]["observed_check_count"]))}</td>
      <td>{html.escape(str(payload["rsi_improvement_evidence"]["baseline_check_count"]))}</td>
      <td>{html.escape(str(payload["rsi_improvement_evidence"]["measured_improvement_percent"]))}%</td>
      <td>{html.escape(str(payload["rsi_improvement_evidence"]["target_percent"]))}%</td>
    </tr>
  </table>
  <h2>RSI Improvement Trend</h2>
  <table>
    <tr><th>Schema</th><th>Status</th><th>Runs</th><th>Previous</th><th>Current</th><th>Delta</th><th>Target</th></tr>
    <tr>
      <td><code>{html.escape(str(payload["rsi_improvement_trend"]["schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_improvement_trend"]["status"]))}</td>
      <td>{html.escape(str(payload["rsi_improvement_trend"]["run_count"]))}</td>
      <td>{html.escape(str(payload["rsi_improvement_trend"]["previous_measured_improvement_percent"]))}%</td>
      <td>{html.escape(str(payload["rsi_improvement_trend"]["current_measured_improvement_percent"]))}%</td>
      <td>{html.escape(str(payload["rsi_improvement_trend"]["delta_from_previous_percent"]))}%</td>
      <td>{html.escape(str(payload["rsi_improvement_trend"]["target_percent"]))}%</td>
    </tr>
  </table>
  <h2>RSI Eligibility Packet</h2>
  <table>
    <tr><th>Schema</th><th>Status</th><th>Ready</th><th>Baselines</th><th>Decision</th><th>Publish authority</th><th>Minimum improvement</th></tr>
    <tr>
      <td><code>{html.escape(str(payload["rsi_eligibility_packet"]["schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_eligibility_packet"]["status"]))}</td>
      <td>{html.escape(str(payload["rsi_eligibility_packet"]["rsi_eligibility_ready"]))}</td>
      <td>{html.escape(str(payload["rsi_eligibility_packet"]["baseline_count"]))} / {html.escape(str(payload["rsi_eligibility_packet"]["minimum_baseline_count"]))}</td>
      <td>{html.escape(str(payload["rsi_eligibility_packet"]["claim_publish_decision"]))}</td>
      <td>{html.escape(str(payload["rsi_eligibility_packet"]["claim_publish_authority"]))}</td>
      <td>{html.escape(str(payload["rsi_eligibility_packet"]["improvement_evidence"]["minimum_measured_improvement_percent"]))}%</td>
    </tr>
  </table>
  <h2>Operator Checks</h2>
  <table>
    <tr><th>Component</th><th>Platform</th><th>Artifact</th><th>Status</th></tr>
    {''.join(rows)}
  </table>
  <h2>Blockers</h2>
  <table>
    <tr><th>Code</th><th>Severity</th><th>Details</th></tr>
    {''.join(blocker_rows)}
  </table>
</body>
</html>
""",
    encoding="utf-8",
)

print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
print(f"status={payload['status']}")
print(f"stable_release_evidence_ready={str(stable_release_evidence_ready).lower()}")
if not stable_release_evidence_ready:
    raise SystemExit(1)
PY
