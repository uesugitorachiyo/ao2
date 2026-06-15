#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FINAL_ROOT="${AO2_RELEASE_READINESS_FINAL_CLOSURE_ROOT:-$ROOT/target/release-readiness-final-closure-verifier}"
SUMMARY="$FINAL_ROOT/summary.json"

python3 - "$FINAL_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from pathlib import Path

final_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])

def load_json(relative_path: str):
    path = final_root / relative_path
    if not path.is_file():
        raise SystemExit(f"missing required artifact file: {path}")
    return path, json.loads(path.read_text(encoding="utf-8"))

def require(condition, message, payload=None):
    if not condition:
        detail = f": {json.dumps(payload, sort_keys=True)}" if payload is not None else ""
        raise SystemExit(message + detail)

consumer_summary_path, consumer = load_json("ao2-release-readiness-consumer/summary.json")
require(
    consumer.get("schema_version") == "ao2.release-readiness-artifact-consumer.v1",
    "unexpected release-readiness consumer schema",
    consumer,
)
require(consumer.get("status") == "passed", "release-readiness consumer did not pass", consumer)

required_source_artifacts = [
    "ao2-release-readiness",
    "ao2-release-readiness-hosted-artifact-gate",
    "ao2-release-train-control-plane-bridge",
    "ao2-ai-task-board-control-plane-bridge",
    "ao2-pulse-task-board-closure-packet",
    "ao2-pulse-codex-cron-event-loop-smoke",
    "ao2-dual-repo-installed-release-smoke",
    "ao2-release-publication-closure",
    "ao2-dual-repo-release-publication-closure-index",
    "ao2-stable-release-evidence-packet",
]
source_artifacts = set(consumer.get("source_artifacts", []))
missing_source_artifacts = [
    artifact for artifact in required_source_artifacts if artifact not in source_artifacts
]
require(
    not missing_source_artifacts,
    "release-readiness consumer missing source artifacts",
    {"missing": missing_source_artifacts, "source_artifacts": consumer.get("source_artifacts")},
)

required_consumer_checks = [
    "ci_release_readiness_hosted_artifact_gate_job",
]
consumer_checks = set(consumer.get("required_checks", []))
missing_consumer_checks = [
    check for check in required_consumer_checks if check not in consumer_checks
]
require(
    not missing_consumer_checks,
    "release-readiness consumer missing required checks",
    {"missing": missing_consumer_checks, "required_checks": consumer.get("required_checks")},
)

public_pair_digest_gate = consumer.get("public_pair_digest_gate", {})
require(
    public_pair_digest_gate.get("schema_version") == "ao2.public-release-pair-digest-audit.v1"
    and public_pair_digest_gate.get("status") == "passed"
    and public_pair_digest_gate.get("archive_parity_status") == "passed"
    and public_pair_digest_gate.get("required_archive_scope") == "full_archive_parity",
    "release-readiness consumer public pair digest gate was not ready",
    consumer,
)

hosted_gate = consumer.get("hosted_release_readiness_artifact_gate", {})
hosted_public_pair_digest_gate = hosted_gate.get("public_pair_digest_gate", {})
require(
    hosted_gate.get("schema_version") == "ao2.release-readiness-hosted-artifact-gate.v1"
    and hosted_gate.get("status") == "passed"
    and hosted_public_pair_digest_gate.get("schema_version") == "ao2.public-release-pair-digest-audit.v1"
    and hosted_public_pair_digest_gate.get("status") == "passed"
    and hosted_public_pair_digest_gate.get("archive_parity_status") == "passed",
    "release-readiness hosted gate evidence was not ready",
    consumer,
)

stable_release_evidence_packet = consumer.get("stable_release_evidence_packet", {})
stable_public_pair_digest_audit = stable_release_evidence_packet.get("public_pair_digest_audit", {})
require(
    stable_release_evidence_packet.get("schema_version") == "ao2.stable-release-evidence-packet.v1"
    and stable_release_evidence_packet.get("status") == "passed"
    and stable_release_evidence_packet.get("stable_release_evidence_ready") is True
    and stable_public_pair_digest_audit.get("schema_version") == "ao2.public-release-pair-digest-audit.v1"
    and stable_public_pair_digest_audit.get("status") == "passed"
    and stable_public_pair_digest_audit.get("archive_parity_status") == "passed",
    "release-readiness stable evidence packet was not ready",
    consumer,
)

require(
    consumer.get("trust_boundary", {}).get("stores_credentials") is False
    and consumer.get("trust_boundary", {}).get("source") == "github_actions_artifact_download",
    "release-readiness consumer trust boundary was not ready",
    consumer,
)

payload = {
    "schema_version": "ao2.release-readiness-final-closure-verifier.v1",
    "status": "passed",
    "source_artifact": "ao2-release-readiness-consumer",
    "source_summary": str(consumer_summary_path),
    "consumer": {
        "schema_version": consumer.get("schema_version"),
        "status": consumer.get("status"),
        "source_artifacts": consumer.get("source_artifacts", []),
    },
    "public_pair_digest_gate": public_pair_digest_gate,
    "hosted_release_readiness_artifact_gate": hosted_gate,
    "stable_release_evidence_packet": stable_release_evidence_packet,
    "required_checks": ["ci_release_readiness_artifact_consumer_job"],
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "source": "github_actions_artifact_download",
    },
}
summary_path.parent.mkdir(parents=True, exist_ok=True)
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
PY
