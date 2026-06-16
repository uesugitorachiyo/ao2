#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_DUAL_REPO_PUBLIC_APPROVAL_CLOSURE_ROOT="${AO2_DUAL_REPO_PUBLIC_APPROVAL_CLOSURE_ROOT:-$ROOT/target/dual-repo-public-approval-closure/latest}"
AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_SUMMARY="${AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_SUMMARY:-$ROOT/target/public-release-operator-checklist-closure/latest/summary.json}"
AO2_CP_PUBLIC_RELEASE_PAIR_VERIFICATION_SUMMARY="${AO2_CP_PUBLIC_RELEASE_PAIR_VERIFICATION_SUMMARY:-$ROOT/../ao2-control-plane/target/public-release-pair-verification/summary.json}"
AO2_CP_STABLE_PROMOTION_EVIDENCE_INDEX_READBACK_SUMMARY="${AO2_CP_STABLE_PROMOTION_EVIDENCE_INDEX_READBACK_SUMMARY:-$ROOT/../ao2-control-plane/target/ao2-stable-promotion-evidence-index-readback/summary.json}"

if [ "${OPENAI_API_KEY+x}" = "x" ] || [ "${ANTHROPIC_API_KEY+x}" = "x" ]; then
  echo "provider API keys are not accepted by dual-repo public approval closure" >&2
  exit 1
fi

rm -rf "$AO2_DUAL_REPO_PUBLIC_APPROVAL_CLOSURE_ROOT"
mkdir -p "$AO2_DUAL_REPO_PUBLIC_APPROVAL_CLOSURE_ROOT"

SUMMARY="$AO2_DUAL_REPO_PUBLIC_APPROVAL_CLOSURE_ROOT/summary.json"
REPORT="$AO2_DUAL_REPO_PUBLIC_APPROVAL_CLOSURE_ROOT/report.md"

python3 - "$AO2_PUBLIC_RELEASE_OPERATOR_CHECKLIST_CLOSURE_SUMMARY" "$AO2_CP_PUBLIC_RELEASE_PAIR_VERIFICATION_SUMMARY" "$AO2_CP_STABLE_PROMOTION_EVIDENCE_INDEX_READBACK_SUMMARY" "$SUMMARY" "$REPORT" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

ao2_checklist_closure_path = Path(sys.argv[1]).resolve()
cp_pair_path = Path(sys.argv[2]).resolve()
cp_readback_path = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
report_path = Path(sys.argv[5]).resolve()

failures = []


def fail(code, message, details=None):
    failures.append({"code": code, "message": message, "details": details or {}})


def load_json(path, label):
    if not path.is_file():
        fail(f"{label}_missing", "missing JSON input", {"path": str(path)})
        return {}, b""
    data = path.read_bytes()
    try:
        return json.loads(data.decode("utf-8")), data
    except json.JSONDecodeError as exc:
        fail(f"{label}_invalid_json", "invalid JSON input", {"path": str(path), "error": str(exc)})
        return {}, data


ao2_checklist_closure, ao2_checklist_closure_bytes = load_json(
    ao2_checklist_closure_path,
    "ao2_public_release_operator_checklist_closure",
)
cp_pair, cp_pair_bytes = load_json(
    cp_pair_path,
    "control_plane_public_release_pair_verification",
)
cp_readback, cp_readback_bytes = load_json(
    cp_readback_path,
    "control_plane_ao2_stable_promotion_evidence_index_readback",
)

ao2_checklist_closure_sha = hashlib.sha256(ao2_checklist_closure_bytes).hexdigest()
cp_pair_sha = hashlib.sha256(cp_pair_bytes).hexdigest()
cp_readback_sha = hashlib.sha256(cp_readback_bytes).hexdigest()

if ao2_checklist_closure.get("schema_version") != "ao2.public-release-operator-checklist-closure.v1":
    fail(
        "ao2_public_release_operator_checklist_closure_schema_mismatch",
        "unexpected AO2 public release operator checklist closure schema",
        {"observed": ao2_checklist_closure.get("schema_version")},
    )
if ao2_checklist_closure.get("status") != "passed" or ao2_checklist_closure.get("public_operator_checklist_closure_ready") is not True:
    fail(
        "ao2_public_release_operator_checklist_closure_not_ready",
        "AO2 public release operator checklist closure was not ready",
        {
            "status": ao2_checklist_closure.get("status"),
            "public_operator_checklist_closure_ready": ao2_checklist_closure.get("public_operator_checklist_closure_ready"),
        },
    )
operator_decision_fields_remain_unapproved = (
    ao2_checklist_closure.get("operator_decision_fields_remain_unapproved") is True
)
if not operator_decision_fields_remain_unapproved:
    fail(
        "operator_decision_fields_not_unapproved",
        "AO2 operator decision fields must remain unapproved before final public approval",
        {"operator_decision_fields_remain_unapproved": ao2_checklist_closure.get("operator_decision_fields_remain_unapproved")},
    )

if cp_pair.get("schema_version") != "ao2.cp-public-release-pair-verification.v1":
    fail(
        "control_plane_public_release_pair_schema_mismatch",
        "unexpected control-plane public release pair verification schema",
        {"observed": cp_pair.get("schema_version")},
    )
if cp_pair.get("status") != "passed":
    fail(
        "control_plane_public_release_pair_not_passed",
        "control-plane public release pair verification did not pass",
        {"status": cp_pair.get("status")},
    )
if cp_pair.get("gaps") not in ([], None):
    fail(
        "control_plane_public_release_pair_gaps_present",
        "control-plane public release pair verification reported gaps",
        {"gaps": cp_pair.get("gaps")},
    )
if cp_pair.get("common_platforms") != ["linux-x86_64", "macos-aarch64", "windows-x86_64"]:
    fail(
        "control_plane_public_release_pair_platform_scope_mismatch",
        "control-plane public release pair verification did not cover required public platforms",
        {"common_platforms": cp_pair.get("common_platforms")},
    )

if cp_readback.get("schema_version") != "ao2.cp-ao2-stable-promotion-evidence-index-readback.v1":
    fail(
        "control_plane_stable_promotion_readback_schema_mismatch",
        "unexpected control-plane AO2 stable promotion evidence index readback schema",
        {"observed": cp_readback.get("schema_version")},
    )
if cp_readback.get("status") != "passed" or cp_readback.get("producer_ready") is not True:
    fail(
        "control_plane_stable_promotion_readback_not_ready",
        "control-plane AO2 stable promotion evidence index readback was not ready",
        {"status": cp_readback.get("status"), "producer_ready": cp_readback.get("producer_ready")},
    )
if cp_readback.get("gaps") not in ([], None):
    fail(
        "control_plane_stable_promotion_readback_gaps_present",
        "control-plane AO2 stable promotion evidence index readback reported gaps",
        {"gaps": cp_readback.get("gaps")},
    )
required_evidence = cp_readback.get("required_evidence")
expected_required_evidence = [
    "artifact_size_budget_audit",
    "post_release_verification_gate",
    "public_pair_digest_audit",
    "stable_release_evidence_packet",
]
if required_evidence != expected_required_evidence:
    fail(
        "control_plane_stable_promotion_readback_evidence_scope_mismatch",
        "control-plane AO2 stable promotion readback evidence scope changed",
        {"required_evidence": required_evidence},
    )


def require_false(trust, label, key):
    if trust.get(key) is not False:
        fail(f"{label}_{key}", f"{label} trust boundary {key} must be false", trust)


ao2_trust = (
    ao2_checklist_closure.get("trust_boundary", {})
    if isinstance(ao2_checklist_closure.get("trust_boundary"), dict)
    else {}
)
cp_pair_trust = (
    cp_pair.get("trust_boundary", {}) if isinstance(cp_pair.get("trust_boundary"), dict) else {}
)
cp_readback_trust = (
    cp_readback.get("trust_boundary", {}) if isinstance(cp_readback.get("trust_boundary"), dict) else {}
)

for label, trust in [
    ("ao2_public_release_operator_checklist_closure", ao2_trust),
    ("control_plane_public_release_pair_verification", cp_pair_trust),
    ("control_plane_ao2_stable_promotion_evidence_index_readback", cp_readback_trust),
]:
    require_false(trust, label, "control_plane_approves_release")

for key in ["mutates_releases", "stores_credentials"]:
    require_false(ao2_trust, "ao2_public_release_operator_checklist_closure", key)
for key in ["mutates_ao_artifacts", "mutates_github_releases", "credential_material_included"]:
    require_false(cp_pair_trust, "control_plane_public_release_pair_verification", key)
for key in [
    "mutates_ao_artifacts",
    "mutates_github_releases",
    "credential_material_included",
    "provider_api_keys_allowed",
]:
    require_false(cp_readback_trust, "control_plane_ao2_stable_promotion_evidence_index_readback", key)

ready = not failures
payload = {
    "schema_version": "ao2.dual-repo-public-approval-closure.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "dual_repo_public_approval_closure_ready": ready,
    "release_go_no_go": "go" if ready else "no_go",
    "operator_decision_fields_remain_unapproved": operator_decision_fields_remain_unapproved,
    "source_artifacts": [
        "ao2-public-release-operator-checklist-closure",
        "ao2-control-plane-public-release-pair-verification",
        "ao2-control-plane-ao2-stable-promotion-evidence-index-readback",
    ],
    "sources": {
        "ao2_public_release_operator_checklist_closure": {
            "path": str(ao2_checklist_closure_path),
            "source_sha256": ao2_checklist_closure_sha,
            "schema_version": ao2_checklist_closure.get("schema_version"),
            "status": ao2_checklist_closure.get("status"),
            "ready": ao2_checklist_closure.get("public_operator_checklist_closure_ready"),
        },
        "control_plane_public_release_pair_verification": {
            "path": str(cp_pair_path),
            "source_sha256": cp_pair_sha,
            "schema_version": cp_pair.get("schema_version"),
            "status": cp_pair.get("status"),
            "common_platforms": cp_pair.get("common_platforms"),
        },
        "control_plane_ao2_stable_promotion_evidence_index_readback": {
            "path": str(cp_readback_path),
            "source_sha256": cp_readback_sha,
            "schema_version": cp_readback.get("schema_version"),
            "status": cp_readback.get("status"),
            "producer_ready": cp_readback.get("producer_ready"),
            "required_evidence": cp_readback.get("required_evidence"),
        },
    },
    "failures": failures,
    "trust_boundary": {
        "local_only": True,
        "mutates_releases": False,
        "stores_credentials": False,
        "control_plane_approves_release": False,
        "mutates_ao_artifacts": False,
        "mutates_github_releases": False,
        "credential_material_included": False,
        "provider_api_keys_allowed": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# Dual Repo Public Approval Closure",
    "",
    f"Status: `{payload['status']}`",
    f"Release go/no-go: `{payload['release_go_no_go']}`",
    f"AO2 public checklist closure SHA256: `{ao2_checklist_closure_sha}`",
    f"Control-plane public release pair verification SHA256: `{cp_pair_sha}`",
    f"Control-plane AO2 stable promotion readback SHA256: `{cp_readback_sha}`",
    "",
    "Trust boundary:",
    "",
    "- control_plane_approves_release: `false`",
    "- mutates_releases: `false`",
    "- mutates_ao_artifacts: `false`",
    "- mutates_github_releases: `false`",
    "- stores_credentials: `false`",
    "- provider_api_keys_allowed: `false`",
]
if failures:
    lines.extend(["", "Failures:", ""])
    lines.extend(f"- `{item['code']}`: {item['message']}" for item in failures)
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"status={payload['status']}")
print(f"release_go_no_go={payload['release_go_no_go']}")
print(f"dual_repo_public_approval_closure_ready={str(ready).lower()}")
if failures:
    for item in failures:
        print(f"failure={item['code']} {item['message']}", file=sys.stderr)
    raise SystemExit(1)
PY
