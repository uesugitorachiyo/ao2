#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_ROOT="${AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_ROOT:-$ROOT/target/public-release-operator-checklist-closure/latest}"
AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_SUMMARY="${AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_SUMMARY:-$ROOT/target/public-release-operator-checklist/latest/summary.json}"
AO2_PUBLIC_RELEASE_OPERATOR_READINESS_SUMMARY="${AO2_PUBLIC_RELEASE_OPERATOR_READINESS_SUMMARY:-$ROOT/target/operator-readiness-summary/latest/summary.json}"

if [ "${OPENAI_API_KEY+x}" = "x" ] || [ "${ANTHROPIC_API_KEY+x}" = "x" ]; then
  echo "provider API keys are not accepted by public release operator checklist closure" >&2
  exit 1
fi

rm -rf "$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_ROOT"
mkdir -p "$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_ROOT"

SUMMARY="$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_ROOT/summary.json"
REPORT="$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_ROOT/report.md"

python3 - "$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_SUMMARY" "$AO2_PUBLIC_RELEASE_OPERATOR_READINESS_SUMMARY" "$SUMMARY" "$REPORT" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

checklist_path = Path(sys.argv[1]).resolve()
readiness_path = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
report_path = Path(sys.argv[4]).resolve()

failures = []


def fail(code, message, details=None):
    failures.append({"code": code, "message": message, "details": details or {}})


def load_json(path, missing_code, invalid_code):
    if not path.is_file():
        fail(missing_code, "missing JSON input", {"path": str(path)})
        return {}, b""
    data = path.read_bytes()
    try:
        return json.loads(data.decode("utf-8")), data
    except json.JSONDecodeError as exc:
        fail(invalid_code, "invalid JSON input", {"path": str(path), "error": str(exc)})
        return {}, data


checklist, checklist_bytes = load_json(
    checklist_path,
    "public_release_operator_checklist_missing",
    "public_release_operator_checklist_invalid_json",
)
readiness, readiness_bytes = load_json(
    readiness_path,
    "operator_readiness_summary_missing",
    "operator_readiness_summary_invalid_json",
)

checklist_sha = hashlib.sha256(checklist_bytes).hexdigest()
readiness_sha = hashlib.sha256(readiness_bytes).hexdigest()

checklist_source = checklist.get("source", {}) if isinstance(checklist.get("source"), dict) else {}
operator_decision = (
    checklist.get("operator_decision", {}) if isinstance(checklist.get("operator_decision"), dict) else {}
)
checklist_trust = (
    checklist.get("trust_boundary", {}) if isinstance(checklist.get("trust_boundary"), dict) else {}
)
readiness_trust = (
    readiness.get("trust_boundary", {}) if isinstance(readiness.get("trust_boundary"), dict) else {}
)

if checklist.get("schema_version") != "ao2.public-release-operator-checklist.v1":
    fail(
        "public_release_operator_checklist_schema_mismatch",
        "unexpected public release operator checklist schema",
        {"observed": checklist.get("schema_version")},
    )
if readiness.get("schema_version") != "ao2.operator-readiness-summary.v1":
    fail(
        "operator_readiness_summary_schema_mismatch",
        "unexpected operator readiness summary schema",
        {"observed": readiness.get("schema_version")},
    )
if checklist.get("status") != "passed" or checklist.get("operator_checklist_ready") is not True:
    fail(
        "public_release_operator_checklist_not_ready",
        "public release operator checklist was not ready",
        {
            "status": checklist.get("status"),
            "operator_checklist_ready": checklist.get("operator_checklist_ready"),
        },
    )
if readiness.get("status") != "passed" or readiness.get("operator_readiness_ready") is not True:
    fail(
        "operator_readiness_summary_not_ready",
        "operator readiness summary was not ready",
        {
            "status": readiness.get("status"),
            "operator_readiness_ready": readiness.get("operator_readiness_ready"),
        },
    )
if checklist_source.get("source_sha256") != readiness_sha:
    fail(
        "operator_readiness_source_digest_mismatch",
        "checklist source digest does not match operator readiness summary",
        {
            "checklist_source_sha256": checklist_source.get("source_sha256"),
            "operator_readiness_summary_sha256": readiness_sha,
        },
    )
for field in ["schema_version", "status", "release_go_no_go", "operator_readiness_ready"]:
    if checklist_source.get(field) != readiness.get(field):
        fail(
            f"operator_readiness_source_{field}_mismatch",
            f"checklist source {field} does not match operator readiness summary",
            {"checklist": checklist_source.get(field), "readiness": readiness.get(field)},
        )

approval_fields = [
    "go_no_go_reviewed",
    "release_pages_reviewed",
    "artifact_digests_reviewed",
    "approval_confirmation_entered",
]
operator_decision_fields_remain_unapproved = all(
    operator_decision.get(field) is False for field in approval_fields
)
if not operator_decision_fields_remain_unapproved:
    fail(
        "operator_decision_already_approved",
        "operator decision fields must remain unapproved for closure evidence",
        {field: operator_decision.get(field) for field in approval_fields},
    )

for label, trust in [
    ("public_release_operator_checklist", checklist_trust),
    ("operator_readiness_summary", readiness_trust),
]:
    if trust.get("mutates_releases") is not False:
        fail(f"{label}_mutates_releases", f"{label} trust boundary mutates releases", trust)
    if trust.get("stores_credentials") is not False:
        fail(f"{label}_stores_credentials", f"{label} trust boundary stores credentials", trust)
    if trust.get("control_plane_approves_release") is not False:
        fail(
            f"{label}_control_plane_approves_release",
            f"{label} trust boundary lets control plane approve release",
            trust,
        )

ready = not failures
payload = {
    "schema_version": "ao2.public-release-operator-checklist-closure.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "public_operator_checklist_closure_ready": ready,
    "operator_decision_fields_remain_unapproved": operator_decision_fields_remain_unapproved,
    "sources": {
        "operator_readiness_summary": {
            "path": str(readiness_path),
            "source_sha256": readiness_sha,
            "schema_version": readiness.get("schema_version"),
            "status": readiness.get("status"),
            "release_go_no_go": readiness.get("release_go_no_go"),
            "operator_readiness_ready": readiness.get("operator_readiness_ready"),
        },
        "public_release_operator_checklist": {
            "path": str(checklist_path),
            "source_sha256": checklist_sha,
            "schema_version": checklist.get("schema_version"),
            "status": checklist.get("status"),
            "operator_checklist_ready": checklist.get("operator_checklist_ready"),
            "embedded_operator_readiness_source_sha256": checklist_source.get("source_sha256"),
        },
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
    "# Public Release Operator Checklist Closure",
    "",
    f"Status: `{payload['status']}`",
    f"Operator decision fields remain unapproved: `{str(operator_decision_fields_remain_unapproved).lower()}`",
    f"Operator readiness summary SHA256: `{readiness_sha}`",
    f"Public release operator checklist SHA256: `{checklist_sha}`",
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
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"status={payload['status']}")
print(f"public_operator_checklist_closure_ready={str(ready).lower()}")
if failures:
    for item in failures:
        print(f"failure={item['code']} {item['message']}", file=sys.stderr)
    raise SystemExit(1)
PY
