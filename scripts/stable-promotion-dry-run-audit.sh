#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_STABLE_PROMOTION_DRY_RUN_ARTIFACT_ROOT="${AO2_STABLE_PROMOTION_DRY_RUN_ARTIFACT_ROOT:-$ROOT/target/stable-release-promotion-dry-run/latest}"
AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_ROOT="${AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_ROOT:-$ROOT/target/stable-promotion-dry-run-audit/latest}"

rm -rf "$AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_ROOT"
mkdir -p "$AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_ROOT"

SUMMARY="$AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_ROOT/summary.json"

python3 - "$AO2_STABLE_PROMOTION_DRY_RUN_ARTIFACT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

artifact_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])

failures = []


def fail(code, message, details=None):
    failures.append({"code": code, "message": message, "details": details or {}})


def load_json(relative):
    path = artifact_root / relative
    if not path.is_file():
        fail("missing_file", f"missing {relative}", {"path": str(path)})
        return path, {}
    try:
        return path, json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail("invalid_json", f"invalid JSON at {relative}", {"path": str(path), "error": str(exc)})
        return path, {}


workflow_path, workflow = load_json("workflow/summary.json")
evidence_path, evidence = load_json("workflow/post-release-verification-evidence/summary.json")
packet_path, packet = load_json("stable-release-evidence-packet/packet/summary.json")

if workflow.get("schema_version") != "ao2.stable-promotion-workflow.v1":
    fail("workflow_schema_mismatch", "unexpected stable promotion workflow schema", {"observed": workflow.get("schema_version")})
if workflow.get("dry_run") is not True or workflow.get("confirmed") is not False:
    fail(
        "stable_promotion_not_dry_run",
        "stable promotion workflow was not a dry-run",
        {"dry_run": workflow.get("dry_run"), "confirmed": workflow.get("confirmed")},
    )
if workflow.get("promotion_status") != "not_attempted":
    fail("promotion_was_attempted", "stable promotion attempted release mutation", {"promotion_status": workflow.get("promotion_status")})
if workflow.get("trust_boundary", {}).get("mutates_releases") is not False:
    fail("workflow_mutates_releases", "stable promotion workflow trust boundary mutates releases", workflow.get("trust_boundary", {}))
if workflow.get("trust_boundary", {}).get("stores_credentials") is not False:
    fail("workflow_stores_credentials", "stable promotion workflow trust boundary stores credentials", workflow.get("trust_boundary", {}))
if workflow.get("post_release_evidence_ready") is not True or workflow.get("evidence_gate_status") != "passed":
    fail(
        "workflow_evidence_not_ready",
        "stable promotion workflow evidence gate was not ready",
        {
            "post_release_evidence_ready": workflow.get("post_release_evidence_ready"),
            "evidence_gate_status": workflow.get("evidence_gate_status"),
        },
    )

if evidence.get("schema_version") != "ao2.stable-promotion-evidence-gate.v1":
    fail("evidence_schema_mismatch", "unexpected stable promotion evidence gate schema", {"observed": evidence.get("schema_version")})
if evidence.get("status") != "passed" or evidence.get("post_release_evidence_ready") is not True:
    fail(
        "post_release_evidence_not_ready",
        "post-release evidence gate was not ready",
        {"status": evidence.get("status"), "post_release_evidence_ready": evidence.get("post_release_evidence_ready")},
    )
if evidence.get("trust_boundary", {}).get("mutates_releases") is not False:
    fail("evidence_mutates_releases", "post-release evidence gate trust boundary mutates releases", evidence.get("trust_boundary", {}))
if evidence.get("trust_boundary", {}).get("stores_credentials") is not False:
    fail("evidence_stores_credentials", "post-release evidence gate trust boundary stores credentials", evidence.get("trust_boundary", {}))

checks = evidence.get("checks", [])
passed_check_count = sum(1 for check in checks if check.get("status") == "passed")
if not checks or passed_check_count != len(checks):
    fail("evidence_checks_not_all_passed", "not all post-release evidence checks passed", {"passed": passed_check_count, "total": len(checks)})

if packet.get("schema_version") != "ao2.stable-release-evidence-packet.v1":
    fail("packet_schema_mismatch", "unexpected stable release evidence packet schema", {"observed": packet.get("schema_version")})
if packet.get("status") != "passed" or packet.get("stable_release_evidence_ready") is not True:
    fail(
        "stable_release_evidence_packet_not_ready",
        "stable release evidence packet was not ready",
        {"status": packet.get("status"), "stable_release_evidence_ready": packet.get("stable_release_evidence_ready")},
    )
if packet.get("stable_promotion", {}).get("evidence_gate_status") != "passed":
    fail("packet_stable_promotion_gate_not_passed", "packet stable promotion gate was not passed", packet.get("stable_promotion", {}))
if packet.get("operator_evidence", {}).get("operator_release_evidence_ready") is not True:
    fail("packet_operator_evidence_not_ready", "packet operator evidence was not ready", packet.get("operator_evidence", {}))
public_pair_digest_audit = packet.get("public_pair_digest_audit", {})
if public_pair_digest_audit.get("schema_version") != "ao2.public-release-pair-digest-audit.v1":
    fail(
        "packet_public_pair_digest_schema_mismatch",
        "packet public pair digest audit schema was not present",
        public_pair_digest_audit,
    )
if public_pair_digest_audit.get("status") != "passed" or public_pair_digest_audit.get("archive_parity_status") != "passed":
    fail(
        "packet_public_pair_digest_not_ready",
        "packet public pair digest audit archive parity was not ready",
        public_pair_digest_audit,
    )
rsi_cross_repo_e2e = packet.get("rsi_cross_repo_e2e", {})
if not (
    rsi_cross_repo_e2e.get("schema_version") == "ao2.rsi-cross-repo-e2e.v1"
    and rsi_cross_repo_e2e.get("status") == "passed"
    and rsi_cross_repo_e2e.get("claim_publish_decision") == "deny"
    and rsi_cross_repo_e2e.get("claim_publish_authority") is False
    and rsi_cross_repo_e2e.get("covenant_gate_schema_version") == "covenant.rsi-claim-publish-gate.v1"
    and rsi_cross_repo_e2e.get("covenant_gate_status") == "denied"
):
    fail(
        "packet_rsi_claim_publish_boundary_not_denied",
        "packet RSI claim-publish boundary was not denied",
        rsi_cross_repo_e2e,
    )
rsi_improvement_evidence = packet.get("rsi_improvement_evidence", {})
if not (
    rsi_improvement_evidence.get("schema_version") == "ao2.rsi-improvement-evidence-gate.v1"
    and rsi_improvement_evidence.get("status") == "passed"
    and rsi_improvement_evidence.get("improvement_ready") is True
    and rsi_improvement_evidence.get("measured_improvement_percent", 0) >= rsi_improvement_evidence.get("target_percent", 5)
    and rsi_improvement_evidence.get("target_percent", 0) >= 5
    and rsi_improvement_evidence.get("claim_publish_decision") == "deny"
    and rsi_improvement_evidence.get("claim_publish_authority") is False
):
    fail(
        "packet_rsi_improvement_evidence_not_ready",
        "packet RSI improvement evidence was not ready",
        rsi_improvement_evidence,
    )
rsi_blueprint_authorization = packet.get("rsi_blueprint_authorization", {})
if not (
    rsi_blueprint_authorization.get("schema_version") == "ao2.rsi-blueprint-authorization-gate.v1"
    and rsi_blueprint_authorization.get("status") == "passed"
    and rsi_blueprint_authorization.get("blueprint_authorization_ready") is True
    and rsi_blueprint_authorization.get("gate_model") == "tiered"
    and rsi_blueprint_authorization.get("source") == "ao-blueprint"
    and rsi_blueprint_authorization.get("self_authorized_by_rsi") is False
    and rsi_blueprint_authorization.get("authorizes_claim_publication") is False
    and rsi_blueprint_authorization.get("authorizes_ao_blueprint_self_change") is False
):
    fail(
        "packet_rsi_blueprint_authorization_not_ready",
        "packet RSI Blueprint authorization was not ready",
        rsi_blueprint_authorization,
    )
rsi_improvement_trend = packet.get("rsi_improvement_trend", {})
if not (
    rsi_improvement_trend.get("schema_version") == "ao2.rsi-improvement-trend.v1"
    and rsi_improvement_trend.get("status") == "passed"
    and rsi_improvement_trend.get("trend_ready") is True
    and rsi_improvement_trend.get("current_measured_improvement_percent", 0) >= rsi_improvement_trend.get("target_percent", 5)
    and rsi_improvement_trend.get("target_percent", 0) >= 5
    and rsi_improvement_trend.get("claim_publish_decision") == "deny"
    and rsi_improvement_trend.get("claim_publish_authority") is False
):
    fail(
        "packet_rsi_improvement_trend_not_ready",
        "packet RSI improvement trend was not ready",
        rsi_improvement_trend,
    )
if packet.get("trust_boundary", {}).get("mutates_releases") is not False:
    fail("packet_mutates_releases", "stable release evidence packet trust boundary mutates releases", packet.get("trust_boundary", {}))
if packet.get("trust_boundary", {}).get("stores_credentials") is not False:
    fail("packet_stores_credentials", "stable release evidence packet trust boundary stores credentials", packet.get("trust_boundary", {}))

ready = not failures
payload = {
    "schema_version": "ao2.stable-promotion-dry-run-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "dry_run_audit_ready": ready,
    "artifact_root": str(artifact_root),
    "sources": {
        "workflow_summary": str(workflow_path),
        "evidence_gate_summary": str(evidence_path),
        "stable_release_evidence_packet": str(packet_path),
    },
    "workflow": {
        "schema_version": workflow.get("schema_version"),
        "status": workflow.get("status"),
        "dry_run": workflow.get("dry_run"),
        "confirmed": workflow.get("confirmed"),
        "promotion_status": workflow.get("promotion_status"),
        "post_release_evidence_ready": workflow.get("post_release_evidence_ready"),
        "evidence_gate_status": workflow.get("evidence_gate_status"),
    },
    "evidence_gate": {
        "schema_version": evidence.get("schema_version"),
        "status": evidence.get("status"),
        "post_release_evidence_ready": evidence.get("post_release_evidence_ready"),
        "check_count": len(checks),
        "passed_check_count": passed_check_count,
    },
    "stable_release_evidence_packet": {
        "schema_version": packet.get("schema_version"),
        "status": packet.get("status"),
        "stable_release_evidence_ready": packet.get("stable_release_evidence_ready"),
        "operator_release_evidence_ready": packet.get("operator_evidence", {}).get("operator_release_evidence_ready"),
        "public_pair_digest_audit": {
            "artifact": public_pair_digest_audit.get("artifact"),
            "schema_version": public_pair_digest_audit.get("schema_version"),
            "status": public_pair_digest_audit.get("status"),
            "archive_parity_status": public_pair_digest_audit.get("archive_parity_status"),
            "summary": public_pair_digest_audit.get("summary"),
        },
        "rsi_cross_repo_e2e": {
            "schema_version": rsi_cross_repo_e2e.get("schema_version"),
            "status": rsi_cross_repo_e2e.get("status"),
            "claim_publish_decision": rsi_cross_repo_e2e.get("claim_publish_decision"),
            "claim_publish_authority": rsi_cross_repo_e2e.get("claim_publish_authority"),
            "covenant_gate_schema_version": rsi_cross_repo_e2e.get("covenant_gate_schema_version"),
            "covenant_gate_status": rsi_cross_repo_e2e.get("covenant_gate_status"),
        },
        "rsi_improvement_evidence": {
            "schema_version": rsi_improvement_evidence.get("schema_version"),
            "status": rsi_improvement_evidence.get("status"),
            "improvement_ready": rsi_improvement_evidence.get("improvement_ready"),
            "target_percent": rsi_improvement_evidence.get("target_percent"),
            "measured_improvement_percent": rsi_improvement_evidence.get("measured_improvement_percent"),
            "claim_publish_decision": rsi_improvement_evidence.get("claim_publish_decision"),
            "claim_publish_authority": rsi_improvement_evidence.get("claim_publish_authority"),
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
        "rsi_improvement_trend": {
            "schema_version": rsi_improvement_trend.get("schema_version"),
            "status": rsi_improvement_trend.get("status"),
            "trend_ready": rsi_improvement_trend.get("trend_ready"),
            "run_count": rsi_improvement_trend.get("run_count"),
            "previous_measured_improvement_percent": rsi_improvement_trend.get("previous_measured_improvement_percent"),
            "current_measured_improvement_percent": rsi_improvement_trend.get("current_measured_improvement_percent"),
            "delta_from_previous_percent": rsi_improvement_trend.get("delta_from_previous_percent"),
            "target_percent": rsi_improvement_trend.get("target_percent"),
            "claim_publish_decision": rsi_improvement_trend.get("claim_publish_decision"),
            "claim_publish_authority": rsi_improvement_trend.get("claim_publish_authority"),
        },
    },
    "failures": failures,
    "trust_boundary": {
        "local_only": True,
        "mutates_releases": False,
        "stores_credentials": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"status={payload['status']}")
print(f"dry_run_audit_ready={str(ready).lower()}")
if failures:
    for item in failures:
        print(f"failure={item['code']} {item['message']}", file=sys.stderr)
    raise SystemExit(1)
PY
