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
