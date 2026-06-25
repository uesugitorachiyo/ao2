#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RSI_IMPROVEMENT_TREND_ROOT="${AO2_RSI_IMPROVEMENT_TREND_ROOT:-$ROOT/target/rsi-improvement-trend/latest}"
AO2_RSI_IMPROVEMENT_TREND_CURRENT_SUMMARY="${AO2_RSI_IMPROVEMENT_TREND_CURRENT_SUMMARY:-$ROOT/target/rsi-improvement-evidence-gate/latest/summary.json}"
AO2_RSI_IMPROVEMENT_TREND_HISTORY="${AO2_RSI_IMPROVEMENT_TREND_HISTORY:-$ROOT/target/rsi-improvement-trend/history.jsonl}"

SUMMARY="$AO2_RSI_IMPROVEMENT_TREND_ROOT/summary.json"

rm -rf "$AO2_RSI_IMPROVEMENT_TREND_ROOT"
mkdir -p "$AO2_RSI_IMPROVEMENT_TREND_ROOT" "$(dirname "$AO2_RSI_IMPROVEMENT_TREND_HISTORY")"

python3 - "$SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_TREND_CURRENT_SUMMARY" \
  "$AO2_RSI_IMPROVEMENT_TREND_HISTORY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_path = Path(sys.argv[1]).resolve()
current_summary_path = Path(sys.argv[2]).resolve()
history_path = Path(sys.argv[3]).resolve()


def load_json(path: Path) -> dict:
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def read_history(path: Path) -> list[dict]:
    if not path.is_file():
        return []
    rows = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError as exc:
            raise SystemExit(f"invalid trend history JSONL at line {line_number}: {exc}") from exc
    return rows


current = load_json(current_summary_path)
metric = current.get("metric") if isinstance(current.get("metric"), dict) else {}
history = read_history(history_path)
previous = history[-1] if history else None
current_measured = metric.get("measured_improvement_percent")
target_percent = metric.get("target_percent")
previous_measured = (
    previous.get("measured_improvement_percent")
    if isinstance(previous, dict)
    else None
)
delta = (
    round(float(current_measured) - float(previous_measured), 4)
    if isinstance(current_measured, (int, float))
    and isinstance(previous_measured, (int, float))
    else None
)
claim_publish_decision = current.get("claim_publish_decision", "missing")
claim_publish_authority = bool(current.get("claim_publish_authority"))

blockers = []
if current.get("schema_version") != "ao2.rsi-improvement-evidence-gate.v1":
    blockers.append(
        {
            "code": "improvement_gate_schema_mismatch",
            "severity": "blocking",
            "actual": current.get("schema_version"),
        }
    )
if current.get("status") != "passed" or current.get("improvement_ready") is not True:
    blockers.append(
        {
            "code": "improvement_gate_not_ready",
            "severity": "blocking",
            "status": current.get("status"),
            "improvement_ready": current.get("improvement_ready"),
        }
    )
if (
    not isinstance(current_measured, (int, float))
    or not isinstance(target_percent, (int, float))
    or current_measured < target_percent
    or target_percent < 5
):
    blockers.append(
        {
            "code": "improvement_metric_not_ready",
            "severity": "blocking",
            "target_percent": target_percent,
            "measured_improvement_percent": current_measured,
        }
    )
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
recorded_at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
record = {
    "schema_version": "ao2.rsi-improvement-trend-record.v1",
    "recorded_at_utc": recorded_at,
    "source_summary": str(current_summary_path),
    "measured_improvement_percent": current_measured,
    "target_percent": target_percent,
    "baseline_check_count": metric.get("baseline_check_count"),
    "observed_check_count": metric.get("observed_check_count"),
    "claim_publish_decision": claim_publish_decision,
    "claim_publish_authority": claim_publish_authority,
}

if ready:
    with history_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, sort_keys=True) + "\n")
    history = [*history, record]

payload = {
    "schema_version": "ao2.rsi-improvement-trend.v1",
    "generated_at_utc": recorded_at,
    "status": "passed" if ready else "failed",
    "trend_ready": ready,
    "history_path": str(history_path),
    "source_summary": str(current_summary_path),
    "run_count": len(history),
    "previous_measured_improvement_percent": previous_measured,
    "current_measured_improvement_percent": current_measured,
    "delta_from_previous_percent": delta,
    "target_percent": target_percent,
    "claim_level": "full_autonomous_self_mutating_rsi",
    "claim_publish_decision": claim_publish_decision,
    "claim_publish_authority": claim_publish_authority,
    "latest_record": record if ready else None,
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "writes_local_history": True,
        "publishes_claims": False,
        "approves_rsi_claims": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"history={history_path}")
print(f"rsi_improvement_trend={payload['status']}")
print(
    "current_measured_improvement_percent="
    f"{payload['current_measured_improvement_percent']} "
    f"delta_from_previous_percent={payload['delta_from_previous_percent']}"
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
