#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RSI_ELIGIBILITY_PACKET_ROOT="${AO2_RSI_ELIGIBILITY_PACKET_ROOT:-$ROOT/target/rsi-eligibility-packet/latest}"
AO2_RSI_ELIGIBILITY_PACKET_CURRENT_BASELINE="${AO2_RSI_ELIGIBILITY_PACKET_CURRENT_BASELINE:-$ROOT/target/rsi-baseline-packet/latest/summary.json}"
AO2_RSI_ELIGIBILITY_PACKET_PREVIOUS_BASELINE="${AO2_RSI_ELIGIBILITY_PACKET_PREVIOUS_BASELINE:-$ROOT/target/rsi-baseline-packet/previous/summary.json}"

SUMMARY="$AO2_RSI_ELIGIBILITY_PACKET_ROOT/summary.json"
DASHBOARD="$AO2_RSI_ELIGIBILITY_PACKET_ROOT/dashboard.html"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --out-root)
      AO2_RSI_ELIGIBILITY_PACKET_ROOT="${2:-}"
      if [ -z "$AO2_RSI_ELIGIBILITY_PACKET_ROOT" ]; then
        echo "--out-root requires a path" >&2
        exit 2
      fi
      SUMMARY="$AO2_RSI_ELIGIBILITY_PACKET_ROOT/summary.json"
      DASHBOARD="$AO2_RSI_ELIGIBILITY_PACKET_ROOT/dashboard.html"
      shift 2
      ;;
    --current-baseline)
      AO2_RSI_ELIGIBILITY_PACKET_CURRENT_BASELINE="${2:-}"
      if [ -z "$AO2_RSI_ELIGIBILITY_PACKET_CURRENT_BASELINE" ]; then
        echo "--current-baseline requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --previous-baseline)
      AO2_RSI_ELIGIBILITY_PACKET_PREVIOUS_BASELINE="${2:-}"
      if [ -z "$AO2_RSI_ELIGIBILITY_PACKET_PREVIOUS_BASELINE" ]; then
        echo "--previous-baseline requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--out-root <path>] [--current-baseline <path>] [--previous-baseline <path>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$AO2_RSI_ELIGIBILITY_PACKET_ROOT"
mkdir -p "$AO2_RSI_ELIGIBILITY_PACKET_ROOT"

python3 - "$AO2_RSI_ELIGIBILITY_PACKET_CURRENT_BASELINE" "$AO2_RSI_ELIGIBILITY_PACKET_PREVIOUS_BASELINE" "$SUMMARY" "$DASHBOARD" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

PACKET_SCHEMA = "ao2.rsi-eligibility-packet.v1"
BASELINE_SCHEMA = "ao2.rsi-baseline-packet.v1"
RSI_SCHEMA = "ao2.rsi-cross-repo-e2e.v1"
BLUEPRINT_SCHEMA = "ao2.rsi-blueprint-authorization-gate.v1"
IMPROVEMENT_SCHEMA = "ao2.rsi-improvement-evidence-gate.v1"
TREND_SCHEMA = "ao2.rsi-improvement-trend.v1"
COVENANT_SCHEMA = "covenant.rsi-claim-publish-gate.v1"

current_path = Path(sys.argv[1]).resolve()
previous_path = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
dashboard_path = Path(sys.argv[4]).resolve()


def load_json(path: Path, source: str, blockers: list[dict]) -> dict:
    if not path.is_file():
        blockers.append(
            {
                "code": "baseline_packet_missing",
                "severity": "blocking",
                "source": source,
                "path": str(path),
            }
        )
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        blockers.append(
            {
                "code": "baseline_packet_invalid_json",
                "severity": "blocking",
                "source": source,
                "path": str(path),
                "detail": str(exc),
            }
        )
        return {}


def add_blocker(blockers: list[dict], source: str, code: str, **details) -> None:
    item = {"code": code, "severity": "blocking", "source": source}
    item.update(details)
    blockers.append(item)


def validate_baseline(source: str, path: Path, packet: dict, blockers: list[dict]) -> dict:
    rsi = packet.get("rsi_cross_repo_e2e") if isinstance(packet.get("rsi_cross_repo_e2e"), dict) else {}
    blueprint = (
        packet.get("rsi_blueprint_authorization")
        if isinstance(packet.get("rsi_blueprint_authorization"), dict)
        else {}
    )
    improvement = (
        packet.get("rsi_improvement_evidence")
        if isinstance(packet.get("rsi_improvement_evidence"), dict)
        else {}
    )
    trend = (
        packet.get("rsi_improvement_trend")
        if isinstance(packet.get("rsi_improvement_trend"), dict)
        else {}
    )
    trust = packet.get("trust_boundary") if isinstance(packet.get("trust_boundary"), dict) else {}

    if packet.get("schema_version") != BASELINE_SCHEMA:
        add_blocker(
            blockers,
            source,
            "baseline_packet_schema_mismatch",
            expected=BASELINE_SCHEMA,
            actual=packet.get("schema_version"),
        )
    if packet.get("status") != "passed" or packet.get("rsi_baseline_ready") is not True:
        add_blocker(
            blockers,
            source,
            "baseline_packet_not_ready",
            status=packet.get("status"),
            rsi_baseline_ready=packet.get("rsi_baseline_ready"),
        )
    if (
        rsi.get("schema_version") != RSI_SCHEMA
        or rsi.get("status") != "passed"
        or rsi.get("claim_publish_decision") != "deny"
        or rsi.get("claim_publish_authority") is not False
        or rsi.get("covenant_gate_schema_version") != COVENANT_SCHEMA
        or rsi.get("covenant_gate_status") != "denied"
    ):
        add_blocker(
            blockers,
            source,
            "claim_publish_boundary_not_denied",
            schema_version=rsi.get("schema_version"),
            status=rsi.get("status"),
            claim_publish_decision=rsi.get("claim_publish_decision"),
            claim_publish_authority=rsi.get("claim_publish_authority"),
            covenant_gate_schema_version=rsi.get("covenant_gate_schema_version"),
            covenant_gate_status=rsi.get("covenant_gate_status"),
        )
    if (
        blueprint.get("schema_version") != BLUEPRINT_SCHEMA
        or blueprint.get("status") != "passed"
        or blueprint.get("blueprint_authorization_ready") is not True
        or blueprint.get("gate_model") != "tiered"
        or blueprint.get("source") != "ao-blueprint"
    ):
        add_blocker(
            blockers,
            source,
            "blueprint_authorization_not_ready",
            schema_version=blueprint.get("schema_version"),
            status=blueprint.get("status"),
            blueprint_authorization_ready=blueprint.get("blueprint_authorization_ready"),
            gate_model=blueprint.get("gate_model"),
            source_authority=blueprint.get("source"),
        )
    if blueprint.get("self_authorized_by_rsi") is not False:
        add_blocker(
            blockers,
            source,
            "blueprint_authorization_self_authorized_by_rsi",
            self_authorized_by_rsi=blueprint.get("self_authorized_by_rsi"),
        )
    if blueprint.get("authorizes_claim_publication") is not False:
        add_blocker(
            blockers,
            source,
            "blueprint_authorization_claim_publication_authority",
            authorizes_claim_publication=blueprint.get("authorizes_claim_publication"),
        )
    if blueprint.get("authorizes_ao_blueprint_self_change") is not False:
        add_blocker(
            blockers,
            source,
            "blueprint_authorization_self_change_authority",
            authorizes_ao_blueprint_self_change=blueprint.get("authorizes_ao_blueprint_self_change"),
        )

    measured = improvement.get("measured_improvement_percent")
    target = improvement.get("target_percent")
    if (
        improvement.get("schema_version") != IMPROVEMENT_SCHEMA
        or improvement.get("status") != "passed"
        or improvement.get("improvement_ready") is not True
        or not isinstance(measured, (int, float))
        or not isinstance(target, (int, float))
        or measured < target
        or target < 5
        or improvement.get("claim_publish_decision") != "deny"
        or improvement.get("claim_publish_authority") is not False
    ):
        add_blocker(
            blockers,
            source,
            "improvement_evidence_not_ready",
            schema_version=improvement.get("schema_version"),
            status=improvement.get("status"),
            improvement_ready=improvement.get("improvement_ready"),
            measured_improvement_percent=measured,
            target_percent=target,
            claim_publish_decision=improvement.get("claim_publish_decision"),
            claim_publish_authority=improvement.get("claim_publish_authority"),
        )

    trend_current = trend.get("current_measured_improvement_percent")
    trend_target = trend.get("target_percent")
    if (
        trend.get("schema_version") != TREND_SCHEMA
        or trend.get("status") != "passed"
        or trend.get("trend_ready") is not True
        or not isinstance(trend_current, (int, float))
        or not isinstance(trend_target, (int, float))
        or trend_current < trend_target
        or trend_target < 5
        or trend.get("claim_publish_decision") != "deny"
        or trend.get("claim_publish_authority") is not False
    ):
        add_blocker(
            blockers,
            source,
            "improvement_trend_not_ready",
            schema_version=trend.get("schema_version"),
            status=trend.get("status"),
            trend_ready=trend.get("trend_ready"),
            current_measured_improvement_percent=trend_current,
            target_percent=trend_target,
            claim_publish_decision=trend.get("claim_publish_decision"),
            claim_publish_authority=trend.get("claim_publish_authority"),
        )

    if (
        trust.get("publishes_claims") is not False
        or trust.get("approves_rsi_claims") is not False
        or trust.get("stores_credentials") is not False
        or trust.get("requires_provider_api_key") is not False
        or trust.get("mutates_repositories") is not False
    ):
        add_blocker(
            blockers,
            source,
            "baseline_trust_boundary_not_local_readback",
            trust_boundary=trust,
        )

    return {
        "source": source,
        "path": str(path),
        "schema_version": packet.get("schema_version"),
        "status": packet.get("status"),
        "rsi_baseline_ready": packet.get("rsi_baseline_ready"),
        "claim_publish_decision": rsi.get("claim_publish_decision"),
        "claim_publish_authority": rsi.get("claim_publish_authority"),
        "blueprint_gate_model": blueprint.get("gate_model"),
        "blueprint_source": blueprint.get("source"),
        "blueprint_self_authorized_by_rsi": blueprint.get("self_authorized_by_rsi"),
        "authorizes_claim_publication": blueprint.get("authorizes_claim_publication"),
        "authorizes_ao_blueprint_self_change": blueprint.get("authorizes_ao_blueprint_self_change"),
        "measured_improvement_percent": measured,
        "target_percent": target,
        "trend_current_measured_improvement_percent": trend_current,
        "trend_delta_from_previous_percent": trend.get("delta_from_previous_percent"),
    }


blockers: list[dict] = []
current = load_json(current_path, "current", blockers)
previous = load_json(previous_path, "previous", blockers)
baseline_summaries = [
    validate_baseline("current", current_path, current, blockers),
    validate_baseline("previous", previous_path, previous, blockers),
]

ready = not blockers
measured_values = [
    item.get("measured_improvement_percent")
    for item in baseline_summaries
    if isinstance(item.get("measured_improvement_percent"), (int, float))
]
target_values = [
    item.get("target_percent")
    for item in baseline_summaries
    if isinstance(item.get("target_percent"), (int, float))
]

payload = {
    "schema_version": PACKET_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "rsi_eligibility_ready": ready,
    "baseline_count": 2,
    "minimum_baseline_count": 2,
    "sources": {
        "current_baseline": str(current_path),
        "previous_baseline": str(previous_path),
    },
    "baseline_summaries": baseline_summaries,
    "claim_publish_decision": "deny" if ready else "blocked",
    "claim_publish_authority": False,
    "blueprint_authorization": {
        "schema_version": BLUEPRINT_SCHEMA,
        "source": "ao-blueprint",
        "gate_model": "tiered",
        "self_authorized_by_rsi": False,
        "authorizes_claim_publication": False,
        "authorizes_ao_blueprint_self_change": False,
    },
    "improvement_evidence": {
        "schema_version": IMPROVEMENT_SCHEMA,
        "minimum_target_percent": min(target_values) if target_values else None,
        "minimum_measured_improvement_percent": min(measured_values) if measured_values else None,
        "baseline_count": 2,
    },
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "reads_local_evidence_only": True,
        "uses_network": False,
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

blocker_rows = []
for blocker in blockers:
    blocker_rows.append(
        "<tr>"
        f"<td>{html.escape(str(blocker.get('source', '')))}</td>"
        f"<td>{html.escape(str(blocker.get('code', '')))}</td>"
        f"<td><code>{html.escape(json.dumps(blocker, sort_keys=True))}</code></td>"
        "</tr>"
    )
if not blocker_rows:
    blocker_rows.append('<tr><td colspan="3">No blockers</td></tr>')

baseline_rows = []
for item in baseline_summaries:
    baseline_rows.append(
        "<tr>"
        f"<td>{html.escape(str(item.get('source')))}</td>"
        f"<td><code>{html.escape(str(item.get('path')))}</code></td>"
        f"<td>{html.escape(str(item.get('status')))}</td>"
        f"<td>{html.escape(str(item.get('claim_publish_decision')))}</td>"
        f"<td>{html.escape(str(item.get('claim_publish_authority')))}</td>"
        f"<td>{html.escape(str(item.get('blueprint_source')))}</td>"
        f"<td>{html.escape(str(item.get('blueprint_self_authorized_by_rsi')))}</td>"
        f"<td>{html.escape(str(item.get('measured_improvement_percent')))}</td>"
        "</tr>"
    )

dashboard_path.write_text(
    f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>RSI Eligibility Packet</title>
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
  <h1>RSI Eligibility Packet</h1>
  <p><code>{PACKET_SCHEMA}</code></p>
  <p class="status">Status: {html.escape(payload["status"])}</p>
  <p>Eligibility ready: {str(ready).lower()}</p>
  <p>claim-publish boundary: {html.escape(str(payload["claim_publish_decision"]))}</p>
  <p>Publish authority: {html.escape(str(payload["claim_publish_authority"]))}</p>
  <p>Blueprint source: ao-blueprint; self-authorized by RSI False</p>
  <h2>Baseline Packets</h2>
  <table>
    <tr><th>Source</th><th>Path</th><th>Status</th><th>Decision</th><th>Authority</th><th>Blueprint</th><th>Self-authorized</th><th>Measured %</th></tr>
    {''.join(baseline_rows)}
  </table>
  <h2>Blockers</h2>
  <table>
    <tr><th>Source</th><th>Code</th><th>Details</th></tr>
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
print(f"rsi_eligibility_ready={str(ready).lower()}")
if not ready:
    for item in blockers:
        print(f"blocker={item['source']}:{item['code']}", file=sys.stderr)
    raise SystemExit(1)
PY
