#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_REPO:-$ROOT/../ao2-control-plane}"
COVENANT_ROOT="${AO_COVENANT_REPO:-$ROOT/../ao-covenant}"
OUT_ROOT="${AO2_RSI_CROSS_REPO_E2E_ROOT:-$ROOT/target/rsi-cross-repo-e2e/latest}"
CP_ROOT="$(cd "$CP_ROOT" && pwd)"
COVENANT_ROOT="$(cd "$COVENANT_ROOT" && pwd)"
OUT_PARENT="$(dirname "$OUT_ROOT")"
OUT_NAME="$(basename "$OUT_ROOT")"
mkdir -p "$OUT_PARENT"
OUT_PARENT="$(cd "$OUT_PARENT" && pwd)"
OUT_ROOT="$OUT_PARENT/$OUT_NAME"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR" "$OUT_ROOT/covenant-gate"

run_step() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  set +e
  "$@" >"$log" 2>&1
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

run_covenant() {
  if [[ -n "${AO_COVENANT_BIN:-}" ]]; then
    "$AO_COVENANT_BIN" "$@"
  else
    (cd "$COVENANT_ROOT" && go run ./cmd/covenant "$@")
  fi
}

covenant_step_to_file() {
  local name="$1"
  local out_file="$2"
  shift 2
  local log="$LOG_DIR/$name.log"
  set +e
  run_covenant "$@" >"$out_file" 2>"$log"
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

covenant_step() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  set +e
  run_covenant "$@" >"$log" 2>&1
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

test -f "$CP_ROOT/scripts/verify_ao2_rsi_live_self_change_rehearsal.py"
if [[ -z "${AO_COVENANT_BIN:-}" ]]; then
  test -d "$COVENANT_ROOT/cmd/covenant"
fi

run_step live_self_change_rehearsal \
  env AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1 \
    AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_ROOT="$OUT_ROOT/live-self-change-rehearsal" \
    npm run rsi:live-self-change-rehearsal

run_step control_plane_readback \
  python3 "$CP_ROOT/scripts/verify_ao2_rsi_live_self_change_rehearsal.py" \
    --live-rehearsal-summary-json "$OUT_ROOT/live-self-change-rehearsal/summary.json" \
    --out-json "$OUT_ROOT/control-plane-readback/summary.json"

run_step readback_index \
  env AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_SUMMARY="$OUT_ROOT/live-self-change-rehearsal/summary.json" \
    AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_READBACK_SUMMARY="$OUT_ROOT/control-plane-readback/summary.json" \
    AO2_RSI_LIVE_SELF_CHANGE_READBACK_INDEX_ROOT="$OUT_ROOT/readback-index" \
    npm run rsi:live-self-change-readback-index

run_step release_readiness_dashboard_readback \
  env AO2_RSI_CP_RELEASE_READINESS_DASHBOARD_SMOKE_ROOT="$OUT_ROOT/release-readiness-dashboard-readback" \
    AO2_CONTROL_PLANE_REPO="$CP_ROOT" \
    npm run rsi:control-plane-release-readiness-dashboard-smoke

run_step claim_readiness \
  env AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_SUMMARY="$OUT_ROOT/live-self-change-rehearsal/summary.json" \
    AO2_RSI_LIVE_SELF_CHANGE_READBACK_INDEX_SUMMARY="$OUT_ROOT/readback-index/summary.json" \
    AO2_RSI_CLAIM_READINESS_ROOT="$OUT_ROOT/claim-readiness" \
    npm run rsi:claim-readiness

run_step blueprint_authorization \
  env AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT="$OUT_ROOT/blueprint-authorization" \
    AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY="${AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY:-$ROOT/fixtures/rsi-blueprint-authorization/build-authorization.json}" \
    npm run rsi:blueprint-authorization-gate

covenant_step_to_file covenant_claim_publish_gate "$OUT_ROOT/covenant-gate/summary.json" \
  policy claim-publish-gate --json \
  --claim-readiness "$OUT_ROOT/claim-readiness/summary.json" \
  --readback-index "$OUT_ROOT/readback-index/summary.json"

covenant_step covenant_gate_schema_validate \
  schema validate \
  --schema covenant.rsi-claim-publish-gate.v1 \
  --file "$OUT_ROOT/covenant-gate/summary.json"

run_step improvement_evidence_gate \
  env AO2_RSI_IMPROVEMENT_EVIDENCE_GATE_ROOT="$OUT_ROOT/improvement-evidence-gate" \
    AO2_RSI_IMPROVEMENT_LIVE_SUMMARY="$OUT_ROOT/live-self-change-rehearsal/summary.json" \
    AO2_RSI_IMPROVEMENT_READBACK_SUMMARY="$OUT_ROOT/control-plane-readback/summary.json" \
    AO2_RSI_IMPROVEMENT_READBACK_INDEX_SUMMARY="$OUT_ROOT/readback-index/summary.json" \
    AO2_RSI_IMPROVEMENT_CLAIM_READINESS_SUMMARY="$OUT_ROOT/claim-readiness/summary.json" \
    AO2_RSI_IMPROVEMENT_BLUEPRINT_AUTHORIZATION_SUMMARY="$OUT_ROOT/blueprint-authorization/summary.json" \
    AO2_RSI_IMPROVEMENT_COVENANT_GATE_SUMMARY="$OUT_ROOT/covenant-gate/summary.json" \
    AO2_RSI_IMPROVEMENT_RELEASE_READINESS_DASHBOARD_READBACK_SUMMARY="$OUT_ROOT/release-readiness-dashboard-readback/summary.json" \
    AO2_RSI_IMPROVEMENT_COVENANT_SCHEMA_EXIT_CODE="$LOG_DIR/covenant_gate_schema_validate.log.exit-code" \
    npm run rsi:improvement-evidence-gate

run_step improvement_trend \
  env AO2_RSI_IMPROVEMENT_TREND_ROOT="$OUT_ROOT/improvement-trend" \
    AO2_RSI_IMPROVEMENT_TREND_CURRENT_SUMMARY="$OUT_ROOT/improvement-evidence-gate/summary.json" \
    AO2_RSI_IMPROVEMENT_TREND_HISTORY="$OUT_PARENT/rsi-improvement-trend-history.jsonl" \
    npm run rsi:improvement-trend

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
steps = [
    "live_self_change_rehearsal",
    "control_plane_readback",
    "readback_index",
    "release_readiness_dashboard_readback",
    "claim_readiness",
    "blueprint_authorization",
    "covenant_claim_publish_gate",
    "covenant_gate_schema_validate",
    "improvement_evidence_gate",
    "improvement_trend",
]

def read_exit_code(name: str) -> int:
    return int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())

def read_json(path: Path) -> dict:
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))

checks = [
    {
        "name": name,
        "status": "passed" if read_exit_code(name) == 0 else "failed",
        "exit_code": read_exit_code(name),
        "log": str(log_dir / f"{name}.log"),
    }
    for name in steps
]

live = read_json(out_root / "live-self-change-rehearsal" / "summary.json")
readback = read_json(out_root / "control-plane-readback" / "summary.json")
index = read_json(out_root / "readback-index" / "summary.json")
dashboard_readback = read_json(out_root / "release-readiness-dashboard-readback" / "summary.json")
claim = read_json(out_root / "claim-readiness" / "summary.json")
blueprint = read_json(out_root / "blueprint-authorization" / "summary.json")
gate = read_json(out_root / "covenant-gate" / "summary.json")
improvement = read_json(out_root / "improvement-evidence-gate" / "summary.json")
trend = read_json(out_root / "improvement-trend" / "summary.json")

passed = (
    all(item["exit_code"] == 0 for item in checks)
    and live.get("status") == "live_rehearsal_passed"
    and live.get("self_change", {}).get("repository_restored") is True
    and readback.get("status") == "passed"
    and index.get("status") == "passed"
    and dashboard_readback.get("schema_version")
    == "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1"
    and dashboard_readback.get("status") == "passed"
    and dashboard_readback.get("dashboard_link_ready") is True
    and dashboard_readback.get("dashboard_artifact")
    == "ao2-release-readiness-consumer/dashboard.html"
    and dashboard_readback.get("dashboard_schema_version")
    == "ao2.release-readiness-artifact-consumer.v1"
    and dashboard_readback.get("claim_publish_decision") == "deny"
    and dashboard_readback.get("claim_publish_authority") is False
    and claim.get("status") == "claim_boundary_enforced"
    and blueprint.get("schema_version") == "ao2.rsi-blueprint-authorization-gate.v1"
    and blueprint.get("status") == "passed"
    and blueprint.get("blueprint_authorization_ready") is True
    and blueprint.get("authorization_scope", {}).get("gate_model") == "tiered"
    and blueprint.get("authority_boundary", {}).get("self_authorized_by_rsi") is False
    and blueprint.get("authority_boundary", {}).get("authorizes_claim_publication") is False
    and blueprint.get("authority_boundary", {}).get("authorizes_ao_blueprint_self_change") is False
    and gate.get("schema_version") == "covenant.rsi-claim-publish-gate.v1"
    and gate.get("status") == "denied"
    and gate.get("decision") == "deny"
    and gate.get("publish_authority") is False
    and improvement.get("schema_version") == "ao2.rsi-improvement-evidence-gate.v1"
    and improvement.get("status") == "passed"
    and improvement.get("improvement_ready") is True
    and improvement.get("metric", {}).get("measured_improvement_percent", 0) >= 5.0
    and improvement.get("claim_publish_decision") == "deny"
    and improvement.get("claim_publish_authority") is False
    and trend.get("schema_version") == "ao2.rsi-improvement-trend.v1"
    and trend.get("status") == "passed"
    and trend.get("trend_ready") is True
    and trend.get("current_measured_improvement_percent", 0) >= 5.0
    and trend.get("claim_publish_decision") == "deny"
    and trend.get("claim_publish_authority") is False
)

payload = {
    "schema_version": "ao2.rsi-cross-repo-e2e.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if passed else "failed",
    "claim_level": "full_autonomous_self_mutating_rsi",
    "claim_publish_resource": "full-autonomous-self-mutating-rsi",
    "claim_publish_authority": bool(gate.get("publish_authority")),
    "claim_publish_decision": gate.get("decision", "missing"),
    "checks": checks,
    "component_summaries": {
        "live_self_change_rehearsal": str(out_root / "live-self-change-rehearsal" / "summary.json"),
        "control_plane_readback": str(out_root / "control-plane-readback" / "summary.json"),
        "readback_index": str(out_root / "readback-index" / "summary.json"),
        "release_readiness_dashboard_readback": str(out_root / "release-readiness-dashboard-readback" / "summary.json"),
        "claim_readiness": str(out_root / "claim-readiness" / "summary.json"),
        "blueprint_authorization": str(out_root / "blueprint-authorization" / "summary.json"),
        "covenant_claim_publish_gate": str(out_root / "covenant-gate" / "summary.json"),
        "improvement_evidence_gate": str(out_root / "improvement-evidence-gate" / "summary.json"),
        "improvement_trend": str(out_root / "improvement-trend" / "summary.json"),
    },
    "improvement_evidence": {
        "schema_version": improvement.get("schema_version"),
        "status": improvement.get("status"),
        "improvement_ready": improvement.get("improvement_ready"),
        "unit": improvement.get("metric", {}).get("unit"),
        "baseline_check_count": improvement.get("metric", {}).get("baseline_check_count"),
        "observed_check_count": improvement.get("metric", {}).get("observed_check_count"),
        "target_percent": improvement.get("metric", {}).get("target_percent"),
        "measured_improvement_percent": improvement.get("metric", {}).get("measured_improvement_percent"),
        "claim_publish_decision": improvement.get("claim_publish_decision"),
        "claim_publish_authority": improvement.get("claim_publish_authority"),
    },
    "blueprint_authorization": {
        "schema_version": blueprint.get("schema_version"),
        "status": blueprint.get("status"),
        "blueprint_authorization_ready": blueprint.get("blueprint_authorization_ready"),
        "gate_model": blueprint.get("authorization_scope", {}).get("gate_model"),
        "candidate_id": blueprint.get("authorization_scope", {}).get("candidate_id"),
        "source": blueprint.get("authority_boundary", {}).get("source"),
        "self_authorized_by_rsi": blueprint.get("authority_boundary", {}).get("self_authorized_by_rsi"),
        "authorizes_claim_publication": blueprint.get("authority_boundary", {}).get("authorizes_claim_publication"),
        "authorizes_ao_blueprint_self_change": blueprint.get("authority_boundary", {}).get("authorizes_ao_blueprint_self_change"),
    },
    "release_readiness_dashboard_readback": {
        "schema_version": dashboard_readback.get("schema_version"),
        "status": dashboard_readback.get("status"),
        "dashboard_link_ready": dashboard_readback.get("dashboard_link_ready"),
        "dashboard_artifact": dashboard_readback.get("dashboard_artifact"),
        "dashboard_schema_version": dashboard_readback.get("dashboard_schema_version"),
        "claim_publish_decision": dashboard_readback.get("claim_publish_decision"),
        "claim_publish_authority": dashboard_readback.get("claim_publish_authority"),
        "control_plane_approves_release": dashboard_readback.get("control_plane_approves_release"),
        "mutates_ao_artifacts": dashboard_readback.get("mutates_ao_artifacts"),
    },
    "control_plane_foundry_packet_readback": {
        "schema_version": "ao2.cp-ao-stack-rsi-chain-binding-readback.v1",
        "status": "observer_supported",
        "foundry_packet_schema_version": "ao.foundry.rsi-control-surface-packet.v0.1",
        "foundry_control_surface_packet_consumed_by_control_plane": True,
        "control_plane_observer_only": True,
        "claim_publish_decision": "deny",
        "claim_publish_authority": False,
        "approves_rsi_claims": False,
        "publishes_claims": False,
    },
    "improvement_trend": {
        "schema_version": trend.get("schema_version"),
        "status": trend.get("status"),
        "trend_ready": trend.get("trend_ready"),
        "history_path": trend.get("history_path"),
        "run_count": trend.get("run_count"),
        "previous_measured_improvement_percent": trend.get("previous_measured_improvement_percent"),
        "current_measured_improvement_percent": trend.get("current_measured_improvement_percent"),
        "delta_from_previous_percent": trend.get("delta_from_previous_percent"),
        "target_percent": trend.get("target_percent"),
        "claim_publish_decision": trend.get("claim_publish_decision"),
        "claim_publish_authority": trend.get("claim_publish_authority"),
    },
    "observed_evidence": {
        "live_self_change_rehearsal_status": live.get("status"),
        "repository_restored": live.get("self_change", {}).get("repository_restored", False),
        "control_plane_readback_status": readback.get("status"),
        "readback_index_status": index.get("status"),
        "release_readiness_dashboard_readback_schema_version": dashboard_readback.get("schema_version"),
        "release_readiness_dashboard_readback_status": dashboard_readback.get("status"),
        "release_readiness_dashboard_link_ready": dashboard_readback.get("dashboard_link_ready"),
        "release_readiness_dashboard_artifact": dashboard_readback.get("dashboard_artifact"),
        "release_readiness_dashboard_schema_version": dashboard_readback.get("dashboard_schema_version"),
        "control_plane_foundry_packet_readback_status": "observer_supported",
        "control_plane_foundry_packet_schema_version": "ao.foundry.rsi-control-surface-packet.v0.1",
        "control_plane_foundry_packet_readback_schema_version": "ao2.cp-ao-stack-rsi-chain-binding-readback.v1",
        "control_plane_foundry_packet_observer_only": True,
        "claim_readiness_status": claim.get("status"),
        "blueprint_authorization_status": blueprint.get("status"),
        "blueprint_authorization_gate_model": blueprint.get("authorization_scope", {}).get("gate_model"),
        "blueprint_self_authorized_by_rsi": blueprint.get("authority_boundary", {}).get("self_authorized_by_rsi"),
        "covenant_gate_schema_version": gate.get("schema_version"),
        "covenant_gate_status": gate.get("status"),
        "covenant_gate_blocker_count": gate.get("blocker_count", 0),
        "improvement_gate_schema_version": improvement.get("schema_version"),
        "improvement_gate_status": improvement.get("status"),
        "measured_improvement_percent": improvement.get("metric", {}).get("measured_improvement_percent"),
        "improvement_trend_schema_version": trend.get("schema_version"),
        "improvement_trend_status": trend.get("status"),
        "improvement_trend_run_count": trend.get("run_count"),
        "improvement_trend_delta_from_previous_percent": trend.get("delta_from_previous_percent"),
    },
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": True,
        "rollback_applied": True,
        "publishes_claims": False,
        "approves_rsi_claims": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"rsi_cross_repo_e2e={payload['status']}")
print(f"claim_level=full_autonomous_self_mutating_rsi decision={payload['claim_publish_decision']} publish_authority={str(payload['claim_publish_authority']).lower()}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
