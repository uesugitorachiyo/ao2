#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
eval "$("$ROOT/scripts/release-train-env.sh" "${AO2_RELEASE_TRAIN:-stable}")"
AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_SUMMARY="${AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_SUMMARY:-$ROOT/target/stable-promotion-dry-run-audit/latest/summary.json}"
AO2_STABLE_PROMOTION_OPERATOR_CHECKLIST_ROOT="${AO2_STABLE_PROMOTION_OPERATOR_CHECKLIST_ROOT:-$ROOT/target/stable-promotion-operator-checklist/latest}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-$AO2_RELEASE_TRAIN_AO2_TAG}"
AO2_CONTROL_PLANE_RELEASE_TAG="${AO2_CONTROL_PLANE_RELEASE_TAG:-$AO2_RELEASE_TRAIN_CP_TAG}"
# Current stable default resolves from docs/release/release-train.json.
AO2_STABLE_PROMOTION_REQUIRED_CONFIRM="${AO2_STABLE_PROMOTION_REQUIRED_CONFIRM:-promote-stable-$AO2_RELEASE_TAG-$AO2_CONTROL_PLANE_RELEASE_TAG}"

rm -rf "$AO2_STABLE_PROMOTION_OPERATOR_CHECKLIST_ROOT"
mkdir -p "$AO2_STABLE_PROMOTION_OPERATOR_CHECKLIST_ROOT"

SUMMARY="$AO2_STABLE_PROMOTION_OPERATOR_CHECKLIST_ROOT/summary.json"
CHECKLIST="$AO2_STABLE_PROMOTION_OPERATOR_CHECKLIST_ROOT/checklist.md"

python3 - "$AO2_STABLE_PROMOTION_DRY_RUN_AUDIT_SUMMARY" "$SUMMARY" "$CHECKLIST" "$AO2_STABLE_PROMOTION_REQUIRED_CONFIRM" "$AO2_RELEASE_TAG" "$AO2_CONTROL_PLANE_RELEASE_TAG" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

audit_summary_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
checklist_path = Path(sys.argv[3])
required_confirmation = sys.argv[4]
ao2_release_tag = sys.argv[5]
control_plane_release_tag = sys.argv[6]

failures = []


def fail(code, message, details=None):
    failures.append({"code": code, "message": message, "details": details or {}})


if not audit_summary_path.is_file():
    fail("missing_dry_run_audit_summary", "missing stable promotion dry-run audit summary", {"path": str(audit_summary_path)})
    audit = {}
else:
    try:
        audit = json.loads(audit_summary_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail("invalid_dry_run_audit_summary", "invalid stable promotion dry-run audit summary JSON", {"path": str(audit_summary_path), "error": str(exc)})
        audit = {}

workflow = audit.get("workflow", {})
evidence_gate = audit.get("evidence_gate", {})
packet = audit.get("stable_release_evidence_packet", {})
trust_boundary = audit.get("trust_boundary", {})

if audit.get("schema_version") != "ao2.stable-promotion-dry-run-audit.v1":
    fail("dry_run_audit_schema_mismatch", "unexpected stable promotion dry-run audit schema", {"observed": audit.get("schema_version")})
if audit.get("status") != "passed" or audit.get("dry_run_audit_ready") is not True:
    fail(
        "dry_run_audit_not_ready",
        "stable promotion dry-run audit was not ready",
        {"status": audit.get("status"), "dry_run_audit_ready": audit.get("dry_run_audit_ready")},
    )
if workflow.get("schema_version") != "ao2.stable-promotion-workflow.v1":
    fail("workflow_schema_mismatch", "unexpected stable promotion workflow schema", {"observed": workflow.get("schema_version")})
if workflow.get("dry_run") is not True or workflow.get("confirmed") is not False:
    fail(
        "workflow_not_unconfirmed_dry_run",
        "stable promotion workflow was not an unconfirmed dry-run",
        {"dry_run": workflow.get("dry_run"), "confirmed": workflow.get("confirmed")},
    )
if workflow.get("promotion_status") != "not_attempted":
    fail("promotion_was_attempted", "stable promotion dry-run attempted promotion", {"promotion_status": workflow.get("promotion_status")})
if workflow.get("post_release_evidence_ready") is not True or workflow.get("evidence_gate_status") != "passed":
    fail(
        "workflow_evidence_not_ready",
        "stable promotion workflow evidence gate was not ready",
        {
            "post_release_evidence_ready": workflow.get("post_release_evidence_ready"),
            "evidence_gate_status": workflow.get("evidence_gate_status"),
        },
    )
if evidence_gate.get("schema_version") != "ao2.stable-promotion-evidence-gate.v1":
    fail("evidence_gate_schema_mismatch", "unexpected stable promotion evidence gate schema", {"observed": evidence_gate.get("schema_version")})
if evidence_gate.get("status") != "passed" or evidence_gate.get("post_release_evidence_ready") is not True:
    fail(
        "evidence_gate_not_ready",
        "stable promotion evidence gate was not ready",
        {"status": evidence_gate.get("status"), "post_release_evidence_ready": evidence_gate.get("post_release_evidence_ready")},
    )
if evidence_gate.get("check_count", 0) <= 0 or evidence_gate.get("passed_check_count") != evidence_gate.get("check_count"):
    fail(
        "evidence_gate_checks_not_all_passed",
        "stable promotion evidence gate checks were not all passed",
        {"passed": evidence_gate.get("passed_check_count"), "total": evidence_gate.get("check_count")},
    )
if packet.get("schema_version") != "ao2.stable-release-evidence-packet.v1":
    fail("packet_schema_mismatch", "unexpected stable release evidence packet schema", {"observed": packet.get("schema_version")})
if packet.get("status") != "passed" or packet.get("stable_release_evidence_ready") is not True:
    fail(
        "stable_release_packet_not_ready",
        "stable release evidence packet was not ready",
        {"status": packet.get("status"), "stable_release_evidence_ready": packet.get("stable_release_evidence_ready")},
    )
if packet.get("operator_release_evidence_ready") is not True:
    fail("operator_evidence_not_ready", "operator release evidence was not ready", {"operator_release_evidence_ready": packet.get("operator_release_evidence_ready")})
public_pair_digest_audit = packet.get("public_pair_digest_audit", {})
if public_pair_digest_audit.get("schema_version") != "ao2.public-release-pair-digest-audit.v1":
    fail(
        "public_pair_digest_audit_schema_mismatch",
        "public pair digest audit schema was not present in stable release evidence packet",
        public_pair_digest_audit,
    )
if public_pair_digest_audit.get("status") != "passed" or public_pair_digest_audit.get("archive_parity_status") != "passed":
    fail(
        "public_pair_digest_audit_not_ready",
        "public pair digest audit archive parity was not ready",
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
        "rsi_claim_publish_boundary_not_denied",
        "RSI claim-publish boundary was not denied in the stable release evidence packet",
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
        "rsi_improvement_evidence_not_ready",
        "RSI improvement evidence was not ready in the stable release evidence packet",
        rsi_improvement_evidence,
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
        "rsi_improvement_trend_not_ready",
        "RSI improvement trend was not ready in the stable release evidence packet",
        rsi_improvement_trend,
    )
if trust_boundary.get("mutates_releases") is not False:
    fail("dry_run_audit_mutates_releases", "stable promotion dry-run audit trust boundary mutates releases", trust_boundary)
if trust_boundary.get("stores_credentials") is not False:
    fail("dry_run_audit_stores_credentials", "stable promotion dry-run audit trust boundary stores credentials", trust_boundary)

ready = not failures
payload = {
    "schema_version": "ao2.stable-promotion-operator-checklist.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "operator_checklist_ready": ready,
    "required_confirmation": required_confirmation,
    "release_targets": {
        "ao2": ao2_release_tag,
        "ao2_control_plane": control_plane_release_tag,
    },
    "sources": {
        "dry_run_audit_summary": str(audit_summary_path),
    },
    "dry_run_audit": {
        "schema_version": audit.get("schema_version"),
        "status": audit.get("status"),
        "dry_run_audit_ready": audit.get("dry_run_audit_ready"),
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
            "schema_version": evidence_gate.get("schema_version"),
            "status": evidence_gate.get("status"),
            "post_release_evidence_ready": evidence_gate.get("post_release_evidence_ready"),
            "check_count": evidence_gate.get("check_count"),
            "passed_check_count": evidence_gate.get("passed_check_count"),
        },
        "stable_release_evidence_packet": {
            "schema_version": packet.get("schema_version"),
            "status": packet.get("status"),
            "stable_release_evidence_ready": packet.get("stable_release_evidence_ready"),
            "operator_release_evidence_ready": packet.get("operator_release_evidence_ready"),
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
    },
    "operator_decision": {
        "confirmation_required": required_confirmation,
        "confirmation_entered": False,
        "must_verify_release_pages": True,
        "must_verify_no_provider_api_keys": True,
        "must_verify_dry_run_was_non_mutating": True,
    },
    "failures": failures,
    "trust_boundary": {
        "local_only": True,
        "mutates_releases": False,
        "stores_credentials": False,
        "control_plane_approves_release": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

status_label = "passed" if ready else "failed"
lines = [
    "# Stable Promotion Operator Checklist",
    "",
    f"Status: {status_label}",
    "",
    f"AO2 release target: `{ao2_release_tag}`",
    f"ao2-control-plane release target: `{control_plane_release_tag}`",
    f"Dry-run audit summary: `{audit_summary_path}`",
    "",
    "Do not enter the confirmation string unless this checklist status is passed.",
    "",
    f"Required confirmation string: `{required_confirmation}`",
    "",
    "Operator decision inputs:",
    "",
    "- Confirm the stable promotion dry-run audit passed.",
    "- Confirm the dry-run workflow was unconfirmed and did not attempt promotion.",
    "- Confirm post-release evidence and the stable release evidence packet passed.",
    f"- Archive parity status: `{public_pair_digest_audit.get('archive_parity_status')}`.",
    f"- RSI claim-publish decision: `{rsi_cross_repo_e2e.get('claim_publish_decision')}`.",
    f"- RSI improvement measured: `{rsi_improvement_evidence.get('measured_improvement_percent')}`.",
    f"- RSI improvement trend delta: `{rsi_improvement_trend.get('delta_from_previous_percent')}`.",
    f"- RSI improvement trend runs: `{rsi_improvement_trend.get('run_count')}`.",
    "- Confirm the public GitHub Release pages show the intended AO2 and ao2-control-plane assets.",
    "- No provider API keys are required or accepted.",
    "- The control plane records evidence; it does not approve the release.",
    "",
    "Trust boundary:",
    "",
    f"- mutates_releases: `{str(payload['trust_boundary']['mutates_releases']).lower()}`",
    f"- stores_credentials: `{str(payload['trust_boundary']['stores_credentials']).lower()}`",
    f"- control_plane_approves_release: `{str(payload['trust_boundary']['control_plane_approves_release']).lower()}`",
]
if failures:
    lines.extend(["", "Failures:", ""])
    for item in failures:
        lines.append(f"- `{item['code']}`: {item['message']}")
checklist_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"checklist={checklist_path}")
print(f"status={payload['status']}")
print(f"operator_checklist_ready={str(ready).lower()}")
if failures:
    for item in failures:
        print(f"failure={item['code']} {item['message']}", file=sys.stderr)
    raise SystemExit(1)
PY
