#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_OPERATOR_READINESS_SUMMARY_ROOT="${AO2_OPERATOR_READINESS_SUMMARY_ROOT:-$ROOT/target/operator-readiness-summary/latest}"
AO2_OPERATOR_READINESS_FINAL_CLOSURE_ROOT="${AO2_OPERATOR_READINESS_FINAL_CLOSURE_ROOT:-$ROOT/target/release-readiness-final-closure-verifier}"
AO2_OPERATOR_READINESS_STABLE_PROMOTION_INDEX_ROOT="${AO2_OPERATOR_READINESS_STABLE_PROMOTION_INDEX_ROOT:-$ROOT/target/stable-promotion-evidence-index/latest}"
AO2_OPERATOR_READINESS_PUBLIC_PAIR_DIGEST_ROOT="${AO2_OPERATOR_READINESS_PUBLIC_PAIR_DIGEST_ROOT:-$ROOT/target/post-release-pair-digest-audit/latest}"
AO2_OPERATOR_READINESS_ARTIFACT_SIZE_BUDGET_ROOT="${AO2_OPERATOR_READINESS_ARTIFACT_SIZE_BUDGET_ROOT:-$ROOT/target/release-artifact-size-budget-audit/latest}"

if [ "${OPENAI_API_KEY+x}" = "x" ] || [ "${ANTHROPIC_API_KEY+x}" = "x" ]; then
  echo "provider API keys are not accepted by operator readiness summary" >&2
  exit 1
fi

rm -rf "$AO2_OPERATOR_READINESS_SUMMARY_ROOT"
mkdir -p "$AO2_OPERATOR_READINESS_SUMMARY_ROOT"

SUMMARY="$AO2_OPERATOR_READINESS_SUMMARY_ROOT/summary.json"
REPORT="$AO2_OPERATOR_READINESS_SUMMARY_ROOT/report.md"

python3 - "$AO2_OPERATOR_READINESS_FINAL_CLOSURE_ROOT" \
  "$AO2_OPERATOR_READINESS_STABLE_PROMOTION_INDEX_ROOT" \
  "$AO2_OPERATOR_READINESS_PUBLIC_PAIR_DIGEST_ROOT" \
  "$AO2_OPERATOR_READINESS_ARTIFACT_SIZE_BUDGET_ROOT" \
  "$SUMMARY" "$REPORT" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

SUMMARY_SCHEMA = "ao2.operator-readiness-summary.v1"
FINAL_CLOSURE_SCHEMA = "ao2.release-readiness-final-closure-verifier.v1"
STABLE_PROMOTION_INDEX_SCHEMA = "ao2.stable-promotion-evidence-index.v1"
PUBLIC_PAIR_DIGEST_SCHEMA = "ao2.public-release-pair-digest-audit.v1"
ARTIFACT_SIZE_BUDGET_SCHEMA = "ao2.release-artifact-size-budget-audit.v1"

final_closure_root = Path(sys.argv[1]).resolve()
stable_promotion_index_root = Path(sys.argv[2]).resolve()
public_pair_digest_root = Path(sys.argv[3]).resolve()
artifact_size_budget_root = Path(sys.argv[4]).resolve()
summary_path = Path(sys.argv[5]).resolve()
report_path = Path(sys.argv[6]).resolve()


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def find_schema(root: Path, schema: str) -> Optional[Path]:
    if root.is_file():
        try:
            payload = load_json(root)
        except Exception:
            return None
        return root if payload.get("schema_version") == schema else None
    if not root.is_dir():
        return None
    for path in sorted(root.rglob("*.json")):
        try:
            payload = load_json(path)
        except Exception:
            continue
        if payload.get("schema_version") == schema:
            return path
    return None


def source_blocker(code: str, path: Path, schema: str) -> dict:
    return {
        "code": code,
        "severity": "blocking",
        "path": str(path),
        "expected_schema": schema,
    }


def trust_boundary_blockers(source_name: str, payload: dict) -> list[dict]:
    trust = payload.get("trust_boundary", {})
    if not isinstance(trust, dict):
        return [{"code": "source_trust_boundary_missing", "severity": "blocking", "source": source_name}]
    blockers = []
    if trust.get("mutates_releases") is True or trust.get("mutates_github_releases") is True:
        blockers.append({"code": "source_mutates_releases", "severity": "blocking", "source": source_name})
    if trust.get("stores_credentials") is True or trust.get("credential_material_included") is True:
        blockers.append({"code": "source_stores_credentials", "severity": "blocking", "source": source_name})
    if trust.get("control_plane_approves_release") is True:
        blockers.append({"code": "source_control_plane_approves_release", "severity": "blocking", "source": source_name})
    return blockers


blockers = []

final_closure_path = find_schema(final_closure_root, FINAL_CLOSURE_SCHEMA)
stable_promotion_index_path = find_schema(stable_promotion_index_root, STABLE_PROMOTION_INDEX_SCHEMA)
public_pair_digest_path = find_schema(public_pair_digest_root, PUBLIC_PAIR_DIGEST_SCHEMA)
artifact_size_budget_path = find_schema(artifact_size_budget_root, ARTIFACT_SIZE_BUDGET_SCHEMA)

if final_closure_path is None:
    blockers.append(source_blocker("release_readiness_final_closure_missing", final_closure_root, FINAL_CLOSURE_SCHEMA))
if stable_promotion_index_path is None:
    blockers.append(source_blocker("stable_promotion_evidence_index_missing", stable_promotion_index_root, STABLE_PROMOTION_INDEX_SCHEMA))
if public_pair_digest_path is None:
    blockers.append(source_blocker("public_pair_digest_audit_missing", public_pair_digest_root, PUBLIC_PAIR_DIGEST_SCHEMA))
if artifact_size_budget_path is None:
    blockers.append(source_blocker("artifact_size_budget_audit_missing", artifact_size_budget_root, ARTIFACT_SIZE_BUDGET_SCHEMA))

final_closure = load_json(final_closure_path) if final_closure_path else {}
stable_promotion_index = load_json(stable_promotion_index_path) if stable_promotion_index_path else {}
public_pair_digest = load_json(public_pair_digest_path) if public_pair_digest_path else {}
artifact_size_budget = load_json(artifact_size_budget_path) if artifact_size_budget_path else {}

final_closure_ready = (
    final_closure.get("schema_version") == FINAL_CLOSURE_SCHEMA
    and final_closure.get("status") == "passed"
    and final_closure.get("source_artifact") == "ao2-release-readiness-consumer"
)
stable_promotion_index_ready = (
    stable_promotion_index.get("schema_version") == STABLE_PROMOTION_INDEX_SCHEMA
    and stable_promotion_index.get("status") == "passed"
    and stable_promotion_index.get("stable_promotion_evidence_index_ready") is True
)
archive_parity_status = public_pair_digest.get("archive_parity_status") or public_pair_digest.get("archive_parity", {}).get("status")
public_pair_digest_ready = (
    public_pair_digest.get("schema_version") == PUBLIC_PAIR_DIGEST_SCHEMA
    and public_pair_digest.get("status") == "passed"
    and archive_parity_status == "passed"
    and public_pair_digest.get("required_archive_scope") == "full_archive_parity"
)
artifact_size_budget_ready = (
    artifact_size_budget.get("schema_version") == ARTIFACT_SIZE_BUDGET_SCHEMA
    and artifact_size_budget.get("status") == "passed"
    and artifact_size_budget.get("failed_check_count") == 0
    and not artifact_size_budget.get("violations", [])
)

if final_closure and not final_closure_ready:
    blockers.append({"code": "release_readiness_final_closure_not_ready", "severity": "blocking", "status": final_closure.get("status")})
if stable_promotion_index and not stable_promotion_index_ready:
    blockers.append({"code": "stable_promotion_evidence_index_not_ready", "severity": "blocking", "status": stable_promotion_index.get("status")})
if public_pair_digest and not public_pair_digest_ready:
    blockers.append(
        {
            "code": "public_pair_digest_audit_not_ready",
            "severity": "blocking",
            "status": public_pair_digest.get("status"),
            "archive_parity_status": archive_parity_status,
        }
    )
if artifact_size_budget and not artifact_size_budget_ready:
    blockers.append(
        {
            "code": "artifact_size_budget_audit_not_ready",
            "severity": "blocking",
            "status": artifact_size_budget.get("status"),
            "failed_check_count": artifact_size_budget.get("failed_check_count"),
            "violations": artifact_size_budget.get("violations", []),
        }
    )

for source_name, payload in [
    ("release_readiness_final_closure", final_closure),
    ("stable_promotion_evidence_index", stable_promotion_index),
    ("public_pair_digest_audit", public_pair_digest),
    ("artifact_size_budget_audit", artifact_size_budget),
]:
    if payload:
        blockers.extend(trust_boundary_blockers(source_name, payload))

ready = not blockers
payload = {
    "schema_version": SUMMARY_SCHEMA,
    "generated_at_utc": now_iso(),
    "status": "passed" if ready else "failed",
    "release_go_no_go": "go" if ready else "no_go",
    "operator_readiness_ready": ready,
    "sources": {
        "release_readiness_final_closure_root": str(final_closure_root),
        "release_readiness_final_closure_summary": str(final_closure_path) if final_closure_path else None,
        "stable_promotion_evidence_index_root": str(stable_promotion_index_root),
        "stable_promotion_evidence_index_summary": str(stable_promotion_index_path) if stable_promotion_index_path else None,
        "public_pair_digest_audit_root": str(public_pair_digest_root),
        "public_pair_digest_audit_summary": str(public_pair_digest_path) if public_pair_digest_path else None,
        "artifact_size_budget_audit_root": str(artifact_size_budget_root),
        "artifact_size_budget_audit_summary": str(artifact_size_budget_path) if artifact_size_budget_path else None,
    },
    "evidence": {
        "release_readiness_final_closure": {
            "artifact": "ao2-release-readiness-final-closure-verifier",
            "schema_version": final_closure.get("schema_version"),
            "status": final_closure.get("status"),
            "ready": final_closure_ready,
            "source_artifact": final_closure.get("source_artifact"),
        },
        "stable_promotion_evidence_index": {
            "artifact": "ao2-stable-promotion-evidence-index",
            "schema_version": stable_promotion_index.get("schema_version"),
            "status": stable_promotion_index.get("status"),
            "ready": stable_promotion_index_ready,
            "stable_promotion_evidence_index_ready": stable_promotion_index.get("stable_promotion_evidence_index_ready"),
        },
        "public_pair_digest_audit": {
            "artifact": "ao2-public-release-pair-digest-audit",
            "schema_version": public_pair_digest.get("schema_version"),
            "status": public_pair_digest.get("status"),
            "ready": public_pair_digest_ready,
            "archive_parity_status": archive_parity_status,
            "required_archive_scope": public_pair_digest.get("required_archive_scope"),
        },
        "artifact_size_budget_audit": {
            "artifact": "ao2-release-artifact-size-budget-audit",
            "schema_version": artifact_size_budget.get("schema_version"),
            "status": artifact_size_budget.get("status"),
            "ready": artifact_size_budget_ready,
            "check_count": artifact_size_budget.get("check_count"),
            "passed_check_count": artifact_size_budget.get("passed_check_count"),
            "failed_check_count": artifact_size_budget.get("failed_check_count"),
            "violations": artifact_size_budget.get("violations", []),
        },
    },
    "required_operator_actions": [
        "review_final_closure_verifier",
        "review_stable_promotion_evidence_index",
        "review_public_pair_digest_audit",
        "review_artifact_size_budget_audit",
        "perform_manual_release_page_review",
    ],
    "blockers": blockers,
    "trust_boundary": {
        "local_only": True,
        "mutates_releases": False,
        "stores_credentials": False,
        "control_plane_approves_release": False,
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# Operator Readiness Summary",
    "",
    f"Status: `{payload['status']}`",
    f"Release go/no-go: `{payload['release_go_no_go']}`",
    f"Schema: `{SUMMARY_SCHEMA}`",
    "",
    "Evidence:",
    "",
    f"- `ao2-release-readiness-final-closure-verifier`: `{payload['sources']['release_readiness_final_closure_summary']}`",
    f"- `ao2-stable-promotion-evidence-index`: `{payload['sources']['stable_promotion_evidence_index_summary']}`",
    f"- `ao2-public-release-pair-digest-audit`: `{payload['sources']['public_pair_digest_audit_summary']}`",
    f"- `ao2-release-artifact-size-budget-audit`: `{payload['sources']['artifact_size_budget_audit_summary']}`",
    "",
    "Operator actions:",
    "",
]
lines.extend(f"- `{item}`" for item in payload["required_operator_actions"])
lines.extend(
    [
        "",
        "Trust boundary:",
        "",
        "- `mutates_releases=false`",
        "- `stores_credentials=false`",
        "- `control_plane_approves_release=false`",
    ]
)
if blockers:
    lines.extend(["", "Blockers:", ""])
    lines.extend(f"- `{item['code']}`" for item in blockers)
else:
    lines.extend(["", "Blockers: none"])
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"release_go_no_go={payload['release_go_no_go']}")
if not ready:
    for blocker in blockers:
        print(f"blocker={blocker['code']}", file=sys.stderr)
    raise SystemExit(1)
PY
