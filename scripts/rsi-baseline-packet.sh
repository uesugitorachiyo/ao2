#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RSI_BASELINE_PACKET_ROOT="${AO2_RSI_BASELINE_PACKET_ROOT:-$ROOT/target/rsi-baseline-packet/latest}"
AO2_RSI_BASELINE_PACKET_RSI_SUMMARY="${AO2_RSI_BASELINE_PACKET_RSI_SUMMARY:-$ROOT/target/rsi-cross-repo-e2e/latest/summary.json}"
SUMMARY="$AO2_RSI_BASELINE_PACKET_ROOT/summary.json"
DASHBOARD="$AO2_RSI_BASELINE_PACKET_ROOT/dashboard.html"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --out-root)
      AO2_RSI_BASELINE_PACKET_ROOT="${2:-}"
      if [ -z "$AO2_RSI_BASELINE_PACKET_ROOT" ]; then
        echo "--out-root requires a path" >&2
        exit 2
      fi
      SUMMARY="$AO2_RSI_BASELINE_PACKET_ROOT/summary.json"
      DASHBOARD="$AO2_RSI_BASELINE_PACKET_ROOT/dashboard.html"
      shift 2
      ;;
    --rsi-summary)
      AO2_RSI_BASELINE_PACKET_RSI_SUMMARY="${2:-}"
      if [ -z "$AO2_RSI_BASELINE_PACKET_RSI_SUMMARY" ]; then
        echo "--rsi-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--out-root <path>] [--rsi-summary <path>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$AO2_RSI_BASELINE_PACKET_ROOT"
mkdir -p "$AO2_RSI_BASELINE_PACKET_ROOT"

python3 - "$AO2_RSI_BASELINE_PACKET_RSI_SUMMARY" "$SUMMARY" "$DASHBOARD" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

RSI_SCHEMA = "ao2.rsi-cross-repo-e2e.v1"
RSI_BLUEPRINT_AUTHORIZATION_SCHEMA = "ao2.rsi-blueprint-authorization-gate.v1"
RSI_IMPROVEMENT_SCHEMA = "ao2.rsi-improvement-evidence-gate.v1"
RSI_IMPROVEMENT_TREND_SCHEMA = "ao2.rsi-improvement-trend.v1"
RSI_COVENANT_GATE_SCHEMA = "covenant.rsi-claim-publish-gate.v1"
PACKET_SCHEMA = "ao2.rsi-baseline-packet.v1"

rsi_summary_path = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
dashboard_path = Path(sys.argv[3]).resolve()


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


blockers = []
rsi = {}
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

rsi_schema_ok = rsi.get("schema_version") == RSI_SCHEMA
rsi_trust = rsi.get("trust_boundary") if isinstance(rsi.get("trust_boundary"), dict) else {}
rsi_claim_publish_denied = (
    rsi_schema_ok
    and rsi.get("status") == "passed"
    and rsi.get("claim_publish_decision") == "deny"
    and rsi.get("claim_publish_authority") is False
    and rsi.get("observed_evidence", {}).get("covenant_gate_schema_version")
    == RSI_COVENANT_GATE_SCHEMA
    and rsi.get("observed_evidence", {}).get("covenant_gate_status") == "denied"
    and rsi_trust.get("requires_provider_api_key") is False
    and rsi_trust.get("stores_credentials") is False
    and rsi_trust.get("publishes_claims") is False
    and rsi_trust.get("approves_rsi_claims") is False
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
rsi_trend = (
    rsi.get("improvement_trend")
    if isinstance(rsi.get("improvement_trend"), dict)
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
rsi_improvement_ready = (
    rsi_schema_ok
    and rsi_improvement.get("schema_version") == RSI_IMPROVEMENT_SCHEMA
    and rsi_improvement.get("status") == "passed"
    and rsi_improvement.get("improvement_ready") is True
    and rsi_improvement.get("unit") == "enforced_rsi_evidence_checks"
    and isinstance(rsi_improvement.get("measured_improvement_percent"), (int, float))
    and isinstance(rsi_improvement.get("target_percent"), (int, float))
    and rsi_improvement.get("measured_improvement_percent")
    >= rsi_improvement.get("target_percent")
    and rsi_improvement.get("target_percent") >= 5
    and rsi_improvement.get("claim_publish_decision") == "deny"
    and rsi_improvement.get("claim_publish_authority") is False
)
rsi_trend_ready = (
    rsi_schema_ok
    and rsi_trend.get("schema_version") == RSI_IMPROVEMENT_TREND_SCHEMA
    and rsi_trend.get("status") == "passed"
    and rsi_trend.get("trend_ready") is True
    and isinstance(rsi_trend.get("current_measured_improvement_percent"), (int, float))
    and isinstance(rsi_trend.get("target_percent"), (int, float))
    and rsi_trend.get("current_measured_improvement_percent")
    >= rsi_trend.get("target_percent")
    and rsi_trend.get("target_percent") >= 5
    and "delta_from_previous_percent" in rsi_trend
    and rsi_trend.get("claim_publish_decision") == "deny"
    and rsi_trend.get("claim_publish_authority") is False
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
if rsi and not rsi_trend_ready:
    blockers.append(
        {
            "code": "rsi_improvement_trend_not_ready",
            "severity": "blocking",
            "schema_version": rsi_trend.get("schema_version"),
            "status": rsi_trend.get("status"),
            "trend_ready": rsi_trend.get("trend_ready"),
            "target_percent": rsi_trend.get("target_percent"),
            "current_measured_improvement_percent": rsi_trend.get("current_measured_improvement_percent"),
            "delta_from_previous_percent": rsi_trend.get("delta_from_previous_percent"),
            "claim_publish_decision": rsi_trend.get("claim_publish_decision"),
            "claim_publish_authority": rsi_trend.get("claim_publish_authority"),
        }
    )

ready = (
    rsi_claim_publish_denied
    and rsi_blueprint_authorization_ready
    and rsi_improvement_ready
    and rsi_trend_ready
    and not blockers
)
payload = {
    "schema_version": PACKET_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "rsi_baseline_ready": ready,
    "sources": {
        "rsi_cross_repo_e2e_summary": str(rsi_summary_path),
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
        "schema_version": rsi_trend.get("schema_version"),
        "status": rsi_trend.get("status"),
        "trend_ready": rsi_trend.get("trend_ready"),
        "history_path": rsi_trend.get("history_path"),
        "run_count": rsi_trend.get("run_count"),
        "previous_measured_improvement_percent": rsi_trend.get("previous_measured_improvement_percent"),
        "current_measured_improvement_percent": rsi_trend.get("current_measured_improvement_percent"),
        "delta_from_previous_percent": rsi_trend.get("delta_from_previous_percent"),
        "target_percent": rsi_trend.get("target_percent"),
        "claim_publish_decision": rsi_trend.get("claim_publish_decision"),
        "claim_publish_authority": rsi_trend.get("claim_publish_authority"),
    },
    "component_summaries": rsi.get("component_summaries", {}),
    "checks": rsi.get("checks", []),
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "reads_local_evidence_only": True,
        "requires_provider_api_key": False,
        "stores_credentials": False,
        "mutates_repositories": False,
        "mutates_releases": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
        "authorizes_ao_blueprint_self_change": False,
    },
    "dashboard": str(dashboard_path),
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

check_rows = []
for check in payload["checks"]:
    check_rows.append(
        "<tr>"
        f"<td>{html.escape(str(check.get('name', '')))}</td>"
        f"<td>{html.escape(str(check.get('status', '')))}</td>"
        f"<td>{html.escape(str(check.get('exit_code', '')))}</td>"
        f"<td><code>{html.escape(str(check.get('log', '')))}</code></td>"
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
  <title>RSI Baseline Packet</title>
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
  <h1>RSI Baseline Packet</h1>
  <p><code>{PACKET_SCHEMA}</code></p>
  <p class="status">Status: {html.escape(payload["status"])}</p>
  <p>Baseline ready: {str(ready).lower()}</p>
  <p>RSI claim-publish boundary: {html.escape(str(payload["rsi_cross_repo_e2e"]["claim_publish_decision"]))}</p>
  <p>Publish authority: {html.escape(str(payload["rsi_cross_repo_e2e"]["claim_publish_authority"]))}</p>
  <p>Blueprint gate: {html.escape(str(payload["rsi_blueprint_authorization"]["gate_model"]))} / self-authorized by RSI {html.escape(str(payload["rsi_blueprint_authorization"]["self_authorized_by_rsi"]))}</p>
  <p>Improvement evidence: {html.escape(str(payload["rsi_improvement_evidence"]["measured_improvement_percent"]))}% / target {html.escape(str(payload["rsi_improvement_evidence"]["target_percent"]))}%</p>
  <p>Improvement trend: run count {html.escape(str(payload["rsi_improvement_trend"]["run_count"]))}, current {html.escape(str(payload["rsi_improvement_trend"]["current_measured_improvement_percent"]))}%, delta {html.escape(str(payload["rsi_improvement_trend"]["delta_from_previous_percent"]))}%</p>
  <h2>Source Summary</h2>
  <table>
    <tr><th>Source</th><th>Path</th><th>Status</th></tr>
    <tr><td>RSI cross-repo E2E</td><td><code>{html.escape(str(rsi_summary_path))}</code></td><td>{html.escape(str(rsi.get("status")))}</td></tr>
  </table>
  <h2>Claim-Publish Boundary</h2>
  <table>
    <tr><th>Schema</th><th>Decision</th><th>Publish authority</th><th>Covenant gate</th><th>Covenant status</th></tr>
    <tr>
      <td><code>{html.escape(str(payload["rsi_cross_repo_e2e"]["schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_cross_repo_e2e"]["claim_publish_decision"]))}</td>
      <td>{html.escape(str(payload["rsi_cross_repo_e2e"]["claim_publish_authority"]))}</td>
      <td><code>{html.escape(str(payload["rsi_cross_repo_e2e"]["covenant_gate_schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_cross_repo_e2e"]["covenant_gate_status"]))}</td>
    </tr>
  </table>
  <h2>Blueprint Authorization</h2>
  <table>
    <tr><th>Schema</th><th>Status</th><th>Gate model</th><th>Candidate</th><th>Source</th><th>Self-authorized by RSI</th><th>Claim publication</th><th>Blueprint self-change</th></tr>
    <tr>
      <td><code>{html.escape(str(payload["rsi_blueprint_authorization"]["schema_version"]))}</code></td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["status"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["gate_model"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["candidate_id"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["source"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["self_authorized_by_rsi"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["authorizes_claim_publication"]))}</td>
      <td>{html.escape(str(payload["rsi_blueprint_authorization"]["authorizes_ao_blueprint_self_change"]))}</td>
    </tr>
  </table>
  <h2>Improvement Evidence</h2>
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
  <h2>Improvement Trend</h2>
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
  <h2>E2E Checks</h2>
  <table>
    <tr><th>Name</th><th>Status</th><th>Exit code</th><th>Log</th></tr>
    {''.join(check_rows)}
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
print(f"rsi_baseline_ready={str(ready).lower()}")
if not ready:
    raise SystemExit(1)
PY
