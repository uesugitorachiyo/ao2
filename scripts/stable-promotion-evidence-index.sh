#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ROOT="${AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ROOT:-$ROOT/target/stable-promotion-evidence-index/latest}"
AO2_STABLE_PROMOTION_EVIDENCE_INDEX_STABLE_PACKET_ROOT="${AO2_STABLE_PROMOTION_EVIDENCE_INDEX_STABLE_PACKET_ROOT:-$ROOT/target/stable-release-evidence-packet/latest}"
AO2_STABLE_PROMOTION_EVIDENCE_INDEX_PUBLIC_PAIR_DIGEST_ROOT="${AO2_STABLE_PROMOTION_EVIDENCE_INDEX_PUBLIC_PAIR_DIGEST_ROOT:-$ROOT/target/post-release-pair-digest-audit/latest}"
AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ARTIFACT_SIZE_BUDGET_ROOT="${AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ARTIFACT_SIZE_BUDGET_ROOT:-$ROOT/target/release-artifact-size-budget-audit/latest}"

rm -rf "$AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ROOT"
mkdir -p "$AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ROOT"

SUMMARY="$AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ROOT/summary.json"
INDEX="$AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ROOT/index.md"

python3 - "$AO2_STABLE_PROMOTION_EVIDENCE_INDEX_STABLE_PACKET_ROOT" \
  "$AO2_STABLE_PROMOTION_EVIDENCE_INDEX_PUBLIC_PAIR_DIGEST_ROOT" \
  "$AO2_STABLE_PROMOTION_EVIDENCE_INDEX_ARTIFACT_SIZE_BUDGET_ROOT" \
  "$SUMMARY" "$INDEX" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

STABLE_PACKET_SCHEMA = "ao2.stable-release-evidence-packet.v1"
POST_RELEASE_GATE_SCHEMA = "ao2.stable-promotion-evidence-gate.v1"
PUBLIC_PAIR_DIGEST_SCHEMA = "ao2.public-release-pair-digest-audit.v1"
ARTIFACT_SIZE_BUDGET_SCHEMA = "ao2.release-artifact-size-budget-audit.v1"
INDEX_SCHEMA = "ao2.stable-promotion-evidence-index.v1"

stable_packet_root = Path(sys.argv[1]).resolve()
public_pair_digest_root = Path(sys.argv[2]).resolve()
artifact_size_budget_root = Path(sys.argv[3]).resolve()
summary_path = Path(sys.argv[4]).resolve()
index_path = Path(sys.argv[5]).resolve()


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


def source_blocker(code: str, path: Path, schema: str):
    return {
        "code": code,
        "severity": "blocking",
        "path": str(path),
        "expected_schema": schema,
    }


blockers = []

stable_packet_path = find_schema(stable_packet_root, STABLE_PACKET_SCHEMA)
post_release_gate_path = find_schema(stable_packet_root, POST_RELEASE_GATE_SCHEMA)
public_pair_digest_path = find_schema(public_pair_digest_root, PUBLIC_PAIR_DIGEST_SCHEMA)
artifact_size_budget_path = find_schema(artifact_size_budget_root, ARTIFACT_SIZE_BUDGET_SCHEMA)

if stable_packet_path is None:
    blockers.append(source_blocker("stable_release_evidence_packet_missing", stable_packet_root, STABLE_PACKET_SCHEMA))
if post_release_gate_path is None:
    blockers.append(source_blocker("post_release_verification_gate_missing", stable_packet_root, POST_RELEASE_GATE_SCHEMA))
if public_pair_digest_path is None:
    blockers.append(source_blocker("public_pair_digest_audit_missing", public_pair_digest_root, PUBLIC_PAIR_DIGEST_SCHEMA))
if artifact_size_budget_path is None:
    blockers.append(source_blocker("artifact_size_budget_audit_missing", artifact_size_budget_root, ARTIFACT_SIZE_BUDGET_SCHEMA))

stable_packet = load_json(stable_packet_path) if stable_packet_path else {}
post_release_gate = load_json(post_release_gate_path) if post_release_gate_path else {}
public_pair_digest = load_json(public_pair_digest_path) if public_pair_digest_path else {}
artifact_size_budget = load_json(artifact_size_budget_path) if artifact_size_budget_path else {}

stable_packet_ready = (
    stable_packet.get("schema_version") == STABLE_PACKET_SCHEMA
    and stable_packet.get("status") == "passed"
    and stable_packet.get("stable_release_evidence_ready") is True
)
post_release_gate_ready = (
    post_release_gate.get("schema_version") == POST_RELEASE_GATE_SCHEMA
    and post_release_gate.get("status") == "passed"
    and post_release_gate.get("post_release_evidence_ready") is True
    and post_release_gate.get("passed_check_count") == post_release_gate.get("check_count")
)
public_pair_digest_ready = (
    public_pair_digest.get("schema_version") == PUBLIC_PAIR_DIGEST_SCHEMA
    and public_pair_digest.get("status") == "passed"
    and public_pair_digest.get("archive_parity", {}).get("status") == "passed"
)
artifact_size_budget_ready = (
    artifact_size_budget.get("schema_version") == ARTIFACT_SIZE_BUDGET_SCHEMA
    and artifact_size_budget.get("status") == "passed"
    and artifact_size_budget.get("failed_check_count") == 0
    and not artifact_size_budget.get("violations", [])
)

if stable_packet and not stable_packet_ready:
    blockers.append(
        {
            "code": "stable_release_evidence_packet_not_ready",
            "severity": "blocking",
            "status": stable_packet.get("status"),
            "stable_release_evidence_ready": stable_packet.get("stable_release_evidence_ready"),
        }
    )
if post_release_gate and not post_release_gate_ready:
    blockers.append(
        {
            "code": "post_release_verification_gate_not_ready",
            "severity": "blocking",
            "status": post_release_gate.get("status"),
            "post_release_evidence_ready": post_release_gate.get("post_release_evidence_ready"),
            "passed_check_count": post_release_gate.get("passed_check_count"),
            "check_count": post_release_gate.get("check_count"),
        }
    )
if public_pair_digest and not public_pair_digest_ready:
    blockers.append(
        {
            "code": "public_pair_digest_audit_not_ready",
            "severity": "blocking",
            "status": public_pair_digest.get("status"),
            "archive_parity_status": public_pair_digest.get("archive_parity", {}).get("status"),
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

source_trust = [
    stable_packet.get("trust_boundary", {}) if isinstance(stable_packet.get("trust_boundary"), dict) else {},
    post_release_gate.get("trust_boundary", {}) if isinstance(post_release_gate.get("trust_boundary"), dict) else {},
    public_pair_digest.get("trust_boundary", {}) if isinstance(public_pair_digest.get("trust_boundary"), dict) else {},
    artifact_size_budget.get("trust_boundary", {}) if isinstance(artifact_size_budget.get("trust_boundary"), dict) else {},
]
for index, trust in enumerate(source_trust):
    if trust.get("mutates_releases") is True or trust.get("mutates_github_releases") is True:
        blockers.append({"code": "source_mutates_releases", "severity": "blocking", "source_index": index, "trust_boundary": trust})
    if trust.get("stores_credentials") is True or trust.get("credential_material_included") is True:
        blockers.append({"code": "source_stores_credentials", "severity": "blocking", "source_index": index, "trust_boundary": trust})
    if trust.get("control_plane_approves_release") is True:
        blockers.append({"code": "source_control_plane_approves_release", "severity": "blocking", "source_index": index, "trust_boundary": trust})

ready = not blockers
payload = {
    "schema_version": INDEX_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if ready else "failed",
    "stable_promotion_evidence_index_ready": ready,
    "sources": {
        "stable_release_evidence_packet_root": str(stable_packet_root),
        "stable_release_evidence_packet_summary": str(stable_packet_path) if stable_packet_path else None,
        "post_release_verification_gate_summary": str(post_release_gate_path) if post_release_gate_path else None,
        "public_pair_digest_audit_root": str(public_pair_digest_root),
        "public_pair_digest_audit_summary": str(public_pair_digest_path) if public_pair_digest_path else None,
        "artifact_size_budget_audit_root": str(artifact_size_budget_root),
        "artifact_size_budget_audit_summary": str(artifact_size_budget_path) if artifact_size_budget_path else None,
    },
    "evidence": {
        "stable_release_evidence_packet": {
            "artifact": "ao2-stable-release-evidence-packet",
            "schema_version": stable_packet.get("schema_version"),
            "status": stable_packet.get("status"),
            "ready": stable_packet_ready,
            "stable_release_evidence_ready": stable_packet.get("stable_release_evidence_ready"),
        },
        "post_release_verification_gate": {
            "schema_version": post_release_gate.get("schema_version"),
            "status": post_release_gate.get("status"),
            "ready": post_release_gate_ready,
            "post_release_evidence_ready": post_release_gate.get("post_release_evidence_ready"),
            "check_count": post_release_gate.get("check_count"),
            "passed_check_count": post_release_gate.get("passed_check_count"),
        },
        "public_pair_digest_audit": {
            "artifact": "ao2-public-release-pair-digest-audit",
            "schema_version": public_pair_digest.get("schema_version"),
            "status": public_pair_digest.get("status"),
            "ready": public_pair_digest_ready,
            "archive_parity_status": public_pair_digest.get("archive_parity", {}).get("status"),
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
        "review_index",
        "review_operator_checklist",
        "verify_release_pages",
        "enter_confirmation_only_after_review",
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
    "# Stable Promotion Evidence Index",
    "",
    f"Status: {payload['status']}",
    f"Schema: `{INDEX_SCHEMA}`",
    "",
    "Source evidence:",
    "",
    f"- `ao2-stable-release-evidence-packet`: `{payload['sources']['stable_release_evidence_packet_summary']}`",
    f"- post-release verification evidence gate: `{payload['sources']['post_release_verification_gate_summary']}`",
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
index_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"index={index_path}")
print(f"status={payload['status']}")
print(f"stable_promotion_evidence_index_ready={str(ready).lower()}")
if not ready:
    for blocker in blockers:
        print(f"blocker={blocker['code']}", file=sys.stderr)
    raise SystemExit(1)
PY
