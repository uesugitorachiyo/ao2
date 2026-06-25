#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT="${AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT:-$ROOT/target/rsi-blueprint-authorization-gate/latest}"
AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY="${AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY:-$ROOT/fixtures/rsi-blueprint-authorization/build-authorization.json}"

SUMMARY="$AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT/summary.json"

rm -rf "$AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT"
mkdir -p "$AO2_RSI_BLUEPRINT_AUTHORIZATION_GATE_ROOT"

python3 - "$AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

authorization_path = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()

blockers = []


def blocker(code, detail=None):
    item = {"code": code, "severity": "blocking"}
    if detail is not None:
        item["detail"] = detail
    blockers.append(item)


def load_json(path):
    if not path.is_file():
        blocker("missing_blueprint_authorization", str(path))
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        blocker("invalid_blueprint_authorization_json", str(exc))
        return {}


authorization = load_json(authorization_path)
scope = authorization.get("authorization_scope", {})
boundary = authorization.get("authority_boundary", {})

if authorization.get("schema") != "ao.blueprint.build-authorization.v0.1":
    blocker("blueprint_authorization_schema_mismatch", authorization.get("schema"))
if authorization.get("status") != "ready":
    blocker("blueprint_authorization_not_ready", authorization.get("status"))
if authorization.get("score") != 100:
    blocker("blueprint_authorization_score_not_100", authorization.get("score"))
if authorization.get("approved_by_user") is not True:
    blocker("blueprint_authorization_not_user_approved", authorization.get("approved_by_user"))
if authorization.get("blocking_assumptions") not in ([], None):
    blocker("blueprint_authorization_has_blockers", authorization.get("blocking_assumptions"))
if authorization.get("next_allowed_action") not in ("ao-foundry", "ao-forge"):
    blocker("blueprint_authorization_next_action_not_downstream", authorization.get("next_allowed_action"))

if scope.get("domain") != "rsi":
    blocker("blueprint_authorization_scope_not_rsi", scope.get("domain"))
if scope.get("gate_model") != "tiered":
    blocker("blueprint_authorization_gate_model_not_tiered", scope.get("gate_model"))
if not isinstance(scope.get("candidate_id"), str) or not scope.get("candidate_id").strip():
    blocker("blueprint_authorization_missing_candidate_id")

if boundary.get("source") != "ao-blueprint":
    blocker("blueprint_authorization_wrong_source", boundary.get("source"))
if boundary.get("downstream_of_operator_intent") is not True:
    blocker(
        "blueprint_authorization_not_downstream_of_operator_intent",
        boundary.get("downstream_of_operator_intent"),
    )
if boundary.get("self_authorized_by_rsi") is not False:
    blocker("blueprint_self_authorized_by_rsi", boundary.get("self_authorized_by_rsi"))
if boundary.get("authorizes_implementation") is not True:
    blocker("blueprint_authorization_does_not_authorize_implementation", boundary.get("authorizes_implementation"))
if boundary.get("authorizes_claim_publication") is not False:
    blocker("blueprint_authorization_claim_publication_authority", boundary.get("authorizes_claim_publication"))
if boundary.get("authorizes_ao_blueprint_self_change") is not False:
    blocker(
        "blueprint_authorization_self_change_authority",
        boundary.get("authorizes_ao_blueprint_self_change"),
    )

ready = not blockers
payload = {
    "schema_version": "ao2.rsi-blueprint-authorization-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "blueprint_authorization_ready": ready,
    "source_authorization_path": str(authorization_path),
    "source_authorization": {
        "schema": authorization.get("schema"),
        "project_id": authorization.get("project_id"),
        "status": authorization.get("status"),
        "score": authorization.get("score"),
        "approved_by_user": authorization.get("approved_by_user"),
        "next_allowed_action": authorization.get("next_allowed_action"),
    },
    "authorization_scope": {
        "domain": scope.get("domain"),
        "gate_model": scope.get("gate_model"),
        "candidate_id": scope.get("candidate_id"),
        "requires_new_blueprint_for": scope.get("requires_new_blueprint_for", []),
    },
    "authority_boundary": {
        "source": boundary.get("source"),
        "downstream_of_operator_intent": boundary.get("downstream_of_operator_intent"),
        "self_authorized_by_rsi": boundary.get("self_authorized_by_rsi"),
        "authorizes_implementation": boundary.get("authorizes_implementation"),
        "authorizes_claim_publication": boundary.get("authorizes_claim_publication"),
        "authorizes_ao_blueprint_self_change": boundary.get("authorizes_ao_blueprint_self_change"),
    },
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "executes_rsi_work": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"rsi_blueprint_authorization_gate={payload['status']}")
if not ready:
    for item in blockers:
        print(f"blocker={item['code']}", file=sys.stderr)
    raise SystemExit(1)
PY
