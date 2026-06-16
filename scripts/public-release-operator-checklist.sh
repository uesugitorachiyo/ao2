#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_PUBLIC_RELEASE_OPERATOR_READINESS_SUMMARY="${AO2_PUBLIC_RELEASE_OPERATOR_READINESS_SUMMARY:-$ROOT/target/operator-readiness-summary/latest/summary.json}"
AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_ROOT="${AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_ROOT:-$ROOT/target/public-release-operator-checklist/latest}"
AO2_PUBLIC_RELEASE_OPERATOR_REQUIRED_CONFIRM="${AO2_PUBLIC_RELEASE_OPERATOR_REQUIRED_CONFIRM:-public-release-reviewed-v0.4.80-v0.1.13}"

if [ "${OPENAI_API_KEY+x}" = "x" ] || [ "${ANTHROPIC_API_KEY+x}" = "x" ]; then
  echo "provider API keys are not accepted by public release operator checklist" >&2
  exit 1
fi

rm -rf "$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_ROOT"
mkdir -p "$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_ROOT"

SUMMARY="$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_ROOT/summary.json"
CHECKLIST="$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_ROOT/checklist.md"

python3 - "$AO2_PUBLIC_RELEASE_OPERATOR_READINESS_SUMMARY" "$SUMMARY" "$CHECKLIST" "$AO2_PUBLIC_RELEASE_OPERATOR_REQUIRED_CONFIRM" <<'PY'
import json
import hashlib
import sys
from datetime import datetime, timezone
from pathlib import Path

readiness_summary_path = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
checklist_path = Path(sys.argv[3]).resolve()
required_confirmation = sys.argv[4]

failures = []
readiness_bytes = b""


def fail(code, message, details=None):
    failures.append({"code": code, "message": message, "details": details or {}})


if not readiness_summary_path.is_file():
    fail("operator_readiness_summary_missing", "missing operator readiness summary", {"path": str(readiness_summary_path)})
    readiness = {}
else:
    try:
        readiness_bytes = readiness_summary_path.read_bytes()
        readiness = json.loads(readiness_bytes.decode("utf-8"))
    except json.JSONDecodeError as exc:
        fail("operator_readiness_summary_invalid_json", "invalid operator readiness summary JSON", {"error": str(exc)})
        readiness = {}
        readiness_bytes = b""

evidence = readiness.get("evidence", {}) if isinstance(readiness.get("evidence"), dict) else {}
public_pair_digest = evidence.get("public_pair_digest_audit", {}) if isinstance(evidence.get("public_pair_digest_audit"), dict) else {}
artifact_size_budget = evidence.get("artifact_size_budget_audit", {}) if isinstance(evidence.get("artifact_size_budget_audit"), dict) else {}
trust_boundary = readiness.get("trust_boundary", {}) if isinstance(readiness.get("trust_boundary"), dict) else {}

if readiness.get("schema_version") != "ao2.operator-readiness-summary.v1":
    fail("operator_readiness_summary_schema_mismatch", "unexpected operator readiness summary schema", {"observed": readiness.get("schema_version")})
if readiness.get("status") != "passed" or readiness.get("operator_readiness_ready") is not True or readiness.get("release_go_no_go") != "go":
    fail(
        "operator_readiness_summary_not_go",
        "operator readiness summary was not go",
        {
            "status": readiness.get("status"),
            "operator_readiness_ready": readiness.get("operator_readiness_ready"),
            "release_go_no_go": readiness.get("release_go_no_go"),
        },
    )

for name in [
    "release_readiness_final_closure",
    "stable_promotion_evidence_index",
    "public_pair_digest_audit",
    "artifact_size_budget_audit",
]:
    block = evidence.get(name, {})
    if not isinstance(block, dict) or block.get("ready") is not True:
        fail(f"{name}_not_ready", f"{name} was not ready", block if isinstance(block, dict) else {})

if public_pair_digest.get("archive_parity_status") != "passed":
    fail("public_pair_digest_archive_parity_not_passed", "public pair digest archive parity was not passed", public_pair_digest)
if public_pair_digest.get("required_archive_scope") != "full_archive_parity":
    fail("public_pair_digest_not_full_archive_parity", "public pair digest audit did not require full archive parity", public_pair_digest)
if artifact_size_budget.get("failed_check_count") not in (None, 0):
    fail("artifact_size_budget_failed_checks", "artifact size budget audit had failed checks", artifact_size_budget)
if trust_boundary.get("mutates_releases") is not False:
    fail("operator_readiness_mutates_releases", "operator readiness summary trust boundary mutates releases", trust_boundary)
if trust_boundary.get("stores_credentials") is not False:
    fail("operator_readiness_stores_credentials", "operator readiness summary trust boundary stores credentials", trust_boundary)
if trust_boundary.get("control_plane_approves_release") is not False:
    fail("operator_readiness_control_plane_approves_release", "operator readiness summary lets control plane approve release", trust_boundary)

ready = not failures
payload = {
    "schema_version": "ao2.public-release-operator-checklist.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "operator_checklist_ready": ready,
    "source": {
        "operator_readiness_summary": str(readiness_summary_path),
        "source_sha256": hashlib.sha256(readiness_bytes).hexdigest(),
        "schema_version": readiness.get("schema_version"),
        "status": readiness.get("status"),
        "release_go_no_go": readiness.get("release_go_no_go"),
        "operator_readiness_ready": readiness.get("operator_readiness_ready"),
    },
    "operator_decision": {
        "required_confirmation": required_confirmation,
        "go_no_go_reviewed": False,
        "release_pages_reviewed": False,
        "artifact_digests_reviewed": False,
        "approval_confirmation_entered": False,
    },
    "evidence": {
        "archive_parity_status": public_pair_digest.get("archive_parity_status"),
        "required_archive_scope": public_pair_digest.get("required_archive_scope"),
        "artifact_size_budget_failed_check_count": artifact_size_budget.get("failed_check_count"),
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

lines = [
    "# Public Release Operator Checklist",
    "",
    f"Status: `{payload['status']}`",
    f"Release go/no-go: `{readiness.get('release_go_no_go')}`",
    f"Operator readiness summary: `{readiness_summary_path}`",
    "",
    "Operator decision fields:",
    "",
    f"- go_no_go_reviewed: `{str(payload['operator_decision']['go_no_go_reviewed']).lower()}`",
    f"- release_pages_reviewed: `{str(payload['operator_decision']['release_pages_reviewed']).lower()}`",
    f"- artifact_digests_reviewed: `{str(payload['operator_decision']['artifact_digests_reviewed']).lower()}`",
    f"- approval_confirmation_entered: `{str(payload['operator_decision']['approval_confirmation_entered']).lower()}`",
    "",
    f"Required confirmation: `{required_confirmation}`",
    "",
    "Review before approval:",
    "",
    "- Confirm the operator readiness summary says `release_go_no_go=go`.",
    "- Confirm public AO2 and ao2-control-plane release pages expose the expected archive assets.",
    "- Confirm digest parity and full archive parity evidence are present.",
    "- Confirm artifact size budget evidence passed for lightweight approval packets.",
    "- Do not enter the confirmation string until every review field is true.",
    "",
    "Trust boundary:",
    "",
    "- mutates_releases: `false`",
    "- stores_credentials: `false`",
    "- control_plane_approves_release: `false`",
]
if failures:
    lines.extend(["", "Failures:", ""])
    lines.extend(f"- `{item['code']}`: {item['message']}" for item in failures)
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
