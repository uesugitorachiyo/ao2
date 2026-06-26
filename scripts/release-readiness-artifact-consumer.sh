#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONSUMER_ROOT="${AO2_RELEASE_READINESS_CONSUMER_ROOT:-$ROOT/target/release-readiness-consumer}"
SUMMARY="$CONSUMER_ROOT/summary.json"
DASHBOARD="$CONSUMER_ROOT/dashboard.html"

python3 - "$CONSUMER_ROOT" "$SUMMARY" "$DASHBOARD" <<'PY'
import html
import json
import sys
from pathlib import Path

consumer_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
dashboard_path = Path(sys.argv[3])

def load_json(relative_path: str):
    path = consumer_root / relative_path
    if not path.is_file():
        raise SystemExit(f"missing required artifact file: {path}")
    return path, json.loads(path.read_text(encoding="utf-8"))

def require(condition, message, payload=None):
    if not condition:
        detail = f": {json.dumps(payload, sort_keys=True)}" if payload is not None else ""
        raise SystemExit(message + detail)

summary_path_source, summary = load_json("ao2-release-readiness/summary.json")
require(summary.get("schema_version") == "ao2.release-readiness-local.v1", "unexpected release readiness schema", summary)
require(summary.get("status") == "passed", "release readiness did not pass", summary)
closure_index_path, closure_index = load_json("ao2-release-readiness/artifact-closure-index.json")
require(closure_index.get("schema_version") == "ao2.release-artifact-closure-index.v1", "unexpected release readiness artifact closure schema", closure_index)
require(closure_index.get("status") == "passed", "release readiness artifact closure did not pass", closure_index)
public_pair_digest_gate = closure_index.get("public_pair_digest_gate", {})
require(
    public_pair_digest_gate.get("schema_version") == "ao2.public-release-pair-digest-audit.v1"
    and public_pair_digest_gate.get("status") == "passed"
    and public_pair_digest_gate.get("archive_parity_status") == "passed"
    and public_pair_digest_gate.get("required_summary_field") == "public_pair_digest_audit"
    and public_pair_digest_gate.get("required_archive_scope") == "full_archive_parity"
    and public_pair_digest_gate.get("required_check") == "release_public_pair_digest_audit_contract"
    and public_pair_digest_gate.get("required_artifact") == "ao2-public-release-pair-digest-audit",
    "release readiness public pair digest gate was not ready",
    closure_index,
)

hosted_gate_summary_path, hosted_gate_summary = load_json("ao2-release-readiness-hosted-artifact-gate/summary.json")
require(
    hosted_gate_summary.get("schema_version") == "ao2.release-readiness-regression-gate.v1",
    "unexpected release-readiness hosted artifact gate schema",
    hosted_gate_summary,
)
hosted_gate_nested = hosted_gate_summary.get("hosted_release_readiness_artifact_gate", {})
require(
    hosted_gate_summary.get("status") == "passed"
    and hosted_gate_nested.get("schema_version") == "ao2.release-readiness-hosted-artifact-gate.v1"
    and hosted_gate_nested.get("status") == "passed",
    "release-readiness hosted artifact gate did not pass",
    hosted_gate_summary,
)
hosted_gate_detail_path, hosted_gate_detail = load_json(
    "ao2-release-readiness-hosted-artifact-gate/hosted-release-readiness-artifact-gate/summary.json"
)
require(
    hosted_gate_detail.get("schema_version") == "ao2.release-readiness-hosted-artifact-gate.v1",
    "unexpected hosted release-readiness artifact gate detail schema",
    hosted_gate_detail,
)
hosted_public_pair_digest_gate = hosted_gate_detail.get("public_pair_digest_gate", {})
require(
    hosted_gate_detail.get("status") == "passed"
    and hosted_gate_detail.get("required") is True
    and hosted_gate_detail.get("readiness_schema_version") == "ao2.release-readiness-local.v1"
    and hosted_gate_detail.get("artifact_closure_schema_version") == "ao2.release-artifact-closure-index.v1"
    and hosted_public_pair_digest_gate.get("schema_version") == "ao2.public-release-pair-digest-audit.v1"
    and hosted_public_pair_digest_gate.get("status") == "passed"
    and hosted_public_pair_digest_gate.get("archive_parity_status") == "passed"
    and hosted_public_pair_digest_gate.get("required_summary_field") == "public_pair_digest_audit"
    and hosted_public_pair_digest_gate.get("required_archive_scope") == "full_archive_parity"
    and hosted_public_pair_digest_gate.get("required_check") == "release_public_pair_digest_audit_contract"
    and hosted_public_pair_digest_gate.get("required_artifact") == "ao2-public-release-pair-digest-audit",
    "hosted release-readiness public pair digest gate was not ready",
    hosted_gate_detail,
)
require(
    hosted_gate_detail.get("trust_boundary", {}).get("stores_credentials") is False
    and hosted_gate_detail.get("trust_boundary", {}).get("source") == "github_actions_artifact_download",
    "hosted release-readiness artifact gate trust boundary was not ready",
    hosted_gate_detail,
)

bridge_summary_path, bridge_summary = load_json("ao2-release-train-control-plane-bridge/latest/summary.json")
require(bridge_summary.get("schema_version") == "ao2.release-train-control-plane-bridge.v1", "unexpected release train bridge schema", bridge_summary)
require(bridge_summary.get("status") == "passed", "release train bridge did not pass", bridge_summary)
require(bridge_summary.get("control_plane", {}).get("smoke") == "passed", "release train control-plane smoke did not pass", bridge_summary)

task_board_bridge_summary_path, task_board_bridge_summary = load_json("ao2-ai-task-board-control-plane-bridge/latest/summary.json")
require(task_board_bridge_summary.get("schema_version") == "ao2.ai-task-board-control-plane-bridge.v1", "unexpected AI task-board bridge schema", task_board_bridge_summary)
require(task_board_bridge_summary.get("status") == "passed", "AI task-board bridge did not pass", task_board_bridge_summary)
require(task_board_bridge_summary.get("control_plane", {}).get("smoke") == "passed", "AI task-board control-plane smoke did not pass", task_board_bridge_summary)

_, task_board_smoke = load_json("ao2-ai-task-board-control-plane-bridge/latest/control-plane-smoke/summary.json")
require(task_board_smoke.get("latest", {}).get("schema_version") == "ao2.cp-ai-task-board-readback.v1", "unexpected task-board readback schema", task_board_smoke)
require(task_board_smoke.get("dashboard", {}).get("schema_version") == "ao2.cp-ai-task-board-dashboard.v1", "unexpected task-board dashboard schema", task_board_smoke)

pulse_task_board_closure_summary_path, pulse_task_board_closure_summary = load_json("ao2-pulse-task-board-closure-packet/latest/summary.json")
require(pulse_task_board_closure_summary.get("schema_version") == "ao2.pulse-task-board-closure-packet.v1", "unexpected Pulse task-board closure schema", pulse_task_board_closure_summary)
require(pulse_task_board_closure_summary.get("status") == "passed", "Pulse task-board closure did not pass", pulse_task_board_closure_summary)
require(pulse_task_board_closure_summary.get("alignment", {}).get("task_ids_match") is True, "Pulse task-board closure task ids did not match", pulse_task_board_closure_summary)
require(pulse_task_board_closure_summary.get("alignment", {}).get("safety_fields_preserved") is True, "Pulse task-board closure safety fields were not preserved", pulse_task_board_closure_summary)
require(
    pulse_task_board_closure_summary.get("checks", {}).get("control_plane_fixture_consumer", {}).get("operator_task_board_view_status") == "passed",
    "Pulse task-board closure control-plane fixture consumer did not pass",
    pulse_task_board_closure_summary,
)

pulse_ao2_smoke_summary_path, pulse_ao2_smoke_summary = load_json("ao2-pulse-ao2-event-loop-smoke/latest/summary.json")
require(pulse_ao2_smoke_summary.get("schema_version") == "ao2.pulse-event-loop-smoke.v1", "unexpected Pulse AO2 smoke schema", pulse_ao2_smoke_summary)
require(pulse_ao2_smoke_summary.get("status") == "passed", "Pulse AO2 smoke did not pass", pulse_ao2_smoke_summary)
require(pulse_ao2_smoke_summary.get("ao2", {}).get("decision_source") == "file", "Pulse AO2 smoke did not use file decision source", pulse_ao2_smoke_summary)
require(pulse_ao2_smoke_summary.get("ao2", {}).get("run_loop_schema") == "ao2.pulse-event-loop-run.v1", "unexpected AO2 run-loop schema", pulse_ao2_smoke_summary)
require(pulse_ao2_smoke_summary.get("ao2", {}).get("decision_schema") == "ao2.pulse-event-loop-decision.v1", "unexpected AO2 decision schema", pulse_ao2_smoke_summary)
require(pulse_ao2_smoke_summary.get("ao2", {}).get("decision_metadata_schema") == "ao2.pulse-event-loop-decision-metadata.v1", "unexpected AO2 decision metadata schema", pulse_ao2_smoke_summary)
require(pulse_ao2_smoke_summary.get("trust_boundary", {}).get("provider_execution") is False, "Pulse AO2 smoke must not execute providers", pulse_ao2_smoke_summary)
pulse_generate_next_rel = "ao2-pulse-ao2-event-loop-smoke/latest/pulse-generate-next/summary.json"
ao2_decision_rel = "ao2-pulse-ao2-event-loop-smoke/latest/pulse-next-recommended-tasks/ao2-event-loop-decision.json"
ao2_stdout_rel = "ao2-pulse-ao2-event-loop-smoke/latest/ao2-run-loop.stdout"
for rel_path in [pulse_generate_next_rel, ao2_decision_rel, ao2_stdout_rel]:
    require((consumer_root / rel_path).is_file(), f"missing Pulse AO2 smoke file {rel_path}")
pulse_generate_next_summary_path, pulse_generate_next_summary = load_json(pulse_generate_next_rel)
require(pulse_generate_next_summary.get("schema_version") == "ao2.pulse-generate-next.v1", "unexpected Pulse generate-next schema", pulse_generate_next_summary)
require(pulse_generate_next_summary.get("status") == "ready", "Pulse generate-next was not ready", pulse_generate_next_summary)
ao2_decision_path, ao2_decision = load_json(ao2_decision_rel)
require(ao2_decision.get("schema_version") == "ao2.pulse-event-loop-decision.v1", "unexpected AO2 decision file schema", ao2_decision)
require(ao2_decision.get("ao2", {}).get("schema_version") == "ao2.pulse-event-loop-decision-metadata.v1", "unexpected AO2 decision metadata file schema", ao2_decision)

dual_repo_summary_path, dual_repo_summary = load_json("ao2-dual-repo-installed-release-smoke/latest/summary.json")
require(dual_repo_summary.get("schema_version") == "ao2.dual-repo-installed-release-smoke.v1", "unexpected dual-repo installed smoke schema", dual_repo_summary)
require(dual_repo_summary.get("status") == "passed", "dual-repo installed smoke did not pass", dual_repo_summary)
require(dual_repo_summary.get("archives", {}).get("ao2", {}).get("manifest_schema") == "ao2.release-manifest.v1", "unexpected AO2 archive manifest schema", dual_repo_summary)
require(dual_repo_summary.get("archives", {}).get("ao2_control_plane", {}).get("manifest_schema") == "ao2-control-plane.release-manifest.v1", "unexpected control-plane archive manifest schema", dual_repo_summary)
require(dual_repo_summary.get("trust_boundary", {}).get("auth_value_stored") is False, "dual-repo installed smoke stored auth value", dual_repo_summary)

rsi_cross_repo_summary_path, rsi_cross_repo_summary = load_json("ao2-rsi-cross-repo-e2e/latest/summary.json")
require(rsi_cross_repo_summary.get("schema_version") == "ao2.rsi-cross-repo-e2e.v1", "unexpected RSI cross-repo E2E schema", rsi_cross_repo_summary)
require(rsi_cross_repo_summary.get("status") == "passed", "RSI cross-repo E2E did not pass", rsi_cross_repo_summary)
rsi_covenant_gate_summary_path, rsi_covenant_gate_summary = load_json("ao2-rsi-cross-repo-e2e/latest/covenant-gate/summary.json")
require(
    rsi_cross_repo_summary.get("claim_publish_decision") == "deny"
    and rsi_cross_repo_summary.get("claim_publish_authority") is False
    and rsi_cross_repo_summary.get("observed_evidence", {}).get("covenant_gate_schema_version") == "covenant.rsi-claim-publish-gate.v1"
    and rsi_cross_repo_summary.get("observed_evidence", {}).get("covenant_gate_status") == "denied"
    and rsi_cross_repo_summary.get("trust_boundary", {}).get("requires_provider_api_key") is False
    and rsi_cross_repo_summary.get("trust_boundary", {}).get("stores_credentials") is False
    and rsi_cross_repo_summary.get("trust_boundary", {}).get("publishes_claims") is False
    and rsi_cross_repo_summary.get("trust_boundary", {}).get("approves_rsi_claims") is False,
    "RSI cross-repo E2E claim publish boundary was not denied",
    rsi_cross_repo_summary,
)
require(
    rsi_covenant_gate_summary.get("schema_version") == "covenant.rsi-claim-publish-gate.v1"
    and rsi_covenant_gate_summary.get("status") == "denied"
    and rsi_covenant_gate_summary.get("decision") == "deny"
    and rsi_covenant_gate_summary.get("publish_authority") is False,
    "RSI Covenant claim-publish gate did not deny publish authority",
    rsi_covenant_gate_summary,
)

rsi_baseline_packet_path, rsi_baseline_packet = load_json("ao2-rsi-baseline-packet/packet/summary.json")
require(
    rsi_baseline_packet.get("schema_version") == "ao2.rsi-baseline-packet.v1",
    "unexpected RSI baseline packet schema",
    rsi_baseline_packet,
)
require(
    rsi_baseline_packet.get("status") == "passed"
    and rsi_baseline_packet.get("rsi_baseline_ready") is True,
    "RSI baseline packet was not ready",
    rsi_baseline_packet,
)
baseline_packet_rsi = rsi_baseline_packet.get("rsi_cross_repo_e2e", {})
require(
    baseline_packet_rsi.get("schema_version") == "ao2.rsi-cross-repo-e2e.v1"
    and baseline_packet_rsi.get("status") == "passed"
    and baseline_packet_rsi.get("claim_publish_decision") == "deny"
    and baseline_packet_rsi.get("claim_publish_authority") is False
    and baseline_packet_rsi.get("covenant_gate_schema_version") == "covenant.rsi-claim-publish-gate.v1"
    and baseline_packet_rsi.get("covenant_gate_status") == "denied",
    "RSI baseline packet claim-publish boundary was not denied",
    rsi_baseline_packet,
)
baseline_packet_blueprint = rsi_baseline_packet.get("rsi_blueprint_authorization", {})
require(
    baseline_packet_blueprint.get("schema_version") == "ao2.rsi-blueprint-authorization-gate.v1"
    and baseline_packet_blueprint.get("status") == "passed"
    and baseline_packet_blueprint.get("blueprint_authorization_ready") is True
    and baseline_packet_blueprint.get("gate_model") == "tiered"
    and baseline_packet_blueprint.get("source") == "ao-blueprint"
    and baseline_packet_blueprint.get("self_authorized_by_rsi") is False
    and baseline_packet_blueprint.get("authorizes_claim_publication") is False
    and baseline_packet_blueprint.get("authorizes_ao_blueprint_self_change") is False,
    "RSI baseline packet Blueprint authorization was not ready",
    rsi_baseline_packet,
)
baseline_packet_improvement = rsi_baseline_packet.get("rsi_improvement_evidence", {})
require(
    baseline_packet_improvement.get("schema_version") == "ao2.rsi-improvement-evidence-gate.v1"
    and baseline_packet_improvement.get("status") == "passed"
    and baseline_packet_improvement.get("improvement_ready") is True
    and baseline_packet_improvement.get("measured_improvement_percent", 0) >= 5
    and baseline_packet_improvement.get("claim_publish_decision") == "deny"
    and baseline_packet_improvement.get("claim_publish_authority") is False,
    "RSI baseline packet improvement evidence was not ready",
    rsi_baseline_packet,
)
baseline_packet_trend = rsi_baseline_packet.get("rsi_improvement_trend", {})
require(
    baseline_packet_trend.get("schema_version") == "ao2.rsi-improvement-trend.v1"
    and baseline_packet_trend.get("status") == "passed"
    and baseline_packet_trend.get("trend_ready") is True
    and baseline_packet_trend.get("current_measured_improvement_percent", 0) >= 5
    and "delta_from_previous_percent" in baseline_packet_trend
    and baseline_packet_trend.get("claim_publish_decision") == "deny"
    and baseline_packet_trend.get("claim_publish_authority") is False,
    "RSI baseline packet improvement trend was not ready",
    rsi_baseline_packet,
)
require(
    rsi_baseline_packet.get("trust_boundary", {}).get("mutates_repositories") is False
    and rsi_baseline_packet.get("trust_boundary", {}).get("publishes_claims") is False
    and rsi_baseline_packet.get("trust_boundary", {}).get("stores_credentials") is False,
    "RSI baseline packet trust boundary was not ready",
    rsi_baseline_packet,
)
require((consumer_root / "ao2-rsi-baseline-packet/packet/dashboard.html").is_file(), "missing RSI baseline packet dashboard")

rsi_eligibility_packet_path, rsi_eligibility_packet = load_json("ao2-rsi-eligibility-packet/packet/summary.json")
require(
    rsi_eligibility_packet.get("schema_version") == "ao2.rsi-eligibility-packet.v1",
    "unexpected RSI eligibility packet schema",
    rsi_eligibility_packet,
)
eligibility_blueprint = rsi_eligibility_packet.get("blueprint_authorization", {})
eligibility_improvement = rsi_eligibility_packet.get("improvement_evidence", {})
eligibility_trust = rsi_eligibility_packet.get("trust_boundary", {})
require(
    rsi_eligibility_packet.get("status") == "passed"
    and rsi_eligibility_packet.get("rsi_eligibility_ready") is True
    and rsi_eligibility_packet.get("baseline_count", 0) >= rsi_eligibility_packet.get("minimum_baseline_count", 2)
    and rsi_eligibility_packet.get("minimum_baseline_count", 0) >= 2
    and rsi_eligibility_packet.get("claim_publish_decision") == "deny"
    and rsi_eligibility_packet.get("claim_publish_authority") is False,
    "RSI eligibility packet was not ready",
    rsi_eligibility_packet,
)
require(
    eligibility_blueprint.get("source") == "ao-blueprint"
    and eligibility_blueprint.get("self_authorized_by_rsi") is False
    and eligibility_blueprint.get("authorizes_claim_publication") is False
    and eligibility_blueprint.get("authorizes_ao_blueprint_self_change") is False,
    "RSI eligibility packet Blueprint boundary was not ready",
    rsi_eligibility_packet,
)
require(
    eligibility_improvement.get("minimum_target_percent", 0) >= 5
    and eligibility_improvement.get("minimum_measured_improvement_percent", 0)
    >= eligibility_improvement.get("minimum_target_percent", 5),
    "RSI eligibility packet improvement evidence was not ready",
    rsi_eligibility_packet,
)
require(
    eligibility_trust.get("publishes_claims") is False
    and eligibility_trust.get("approves_rsi_claims") is False
    and eligibility_trust.get("mutates_repositories") is False
    and eligibility_trust.get("requires_provider_api_key") is False,
    "RSI eligibility packet trust boundary was not ready",
    rsi_eligibility_packet,
)
require((consumer_root / "ao2-rsi-eligibility-packet/packet/dashboard.html").is_file(), "missing RSI eligibility packet dashboard")

publication_closure_summary_path, publication_closure_summary = load_json("ao2-release-publication-closure/summary.json")
require(publication_closure_summary.get("schema_version") == "ao2.release-publication-dry-run-closure.v1", "unexpected release publication closure schema", publication_closure_summary)
require(publication_closure_summary.get("status") == "passed", "release publication closure did not pass", publication_closure_summary)
require(publication_closure_summary.get("publication_ready") is True, "release publication closure not publication-ready", publication_closure_summary)
require(publication_closure_summary.get("stable_release_ready") is True, "release publication closure not stable-release-ready", publication_closure_summary)
require(publication_closure_summary.get("publication_state", {}).get("dry_run") is True, "release publication closure was not a dry run", publication_closure_summary)
require(publication_closure_summary.get("publication_state", {}).get("upload_status") == "not_attempted", "release publication closure attempted upload", publication_closure_summary)
require(publication_closure_summary.get("trust_boundary", {}).get("mutates_releases") is False, "release publication closure mutated releases", publication_closure_summary)
require(publication_closure_summary.get("trust_boundary", {}).get("stores_credentials") is False, "release publication closure stored credentials", publication_closure_summary)

dual_repo_publication_closure_summary_path, dual_repo_publication_closure_summary = load_json("ao2-dual-repo-release-publication-closure-index/summary.json")
require(dual_repo_publication_closure_summary.get("schema_version") == "ao2.dual-repo-release-publication-closure-index.v1", "unexpected dual-repo publication closure index schema", dual_repo_publication_closure_summary)
require(dual_repo_publication_closure_summary.get("status") == "passed", "dual-repo publication closure index did not pass", dual_repo_publication_closure_summary)
require(dual_repo_publication_closure_summary.get("ao2", {}).get("schema_version") == "ao2.release-publication-dry-run-closure.v1", "unexpected AO2 publication closure schema in dual-repo index", dual_repo_publication_closure_summary)
require(dual_repo_publication_closure_summary.get("control_plane", {}).get("schema_version") == "ao2.cp-release-publication-closure.v1", "unexpected control-plane publication closure schema", dual_repo_publication_closure_summary)
require(dual_repo_publication_closure_summary.get("control_plane", {}).get("checksum_verified") is True, "control-plane publication closure checksum not verified", dual_repo_publication_closure_summary)
control_plane_assets = dual_repo_publication_closure_summary.get("control_plane", {}).get("assets", [])
control_plane_archive_assets = [
    asset
    for asset in control_plane_assets
    if isinstance(asset, dict)
    and isinstance(asset.get("name"), str)
    and asset["name"].startswith("ao2-control-plane-")
    and asset["name"].endswith(".tar.gz")
]
require(control_plane_archive_assets, "control-plane publication closure missing release archive asset", dual_repo_publication_closure_summary.get("control_plane", {}))
for asset in control_plane_archive_assets:
    sha256 = asset.get("sha256")
    size_bytes = asset.get("size_bytes")
    require(
        isinstance(sha256, str)
        and len(sha256) == 64
        and all(char in "0123456789abcdef" for char in sha256.lower())
        and isinstance(size_bytes, int)
        and size_bytes > 0,
        "control-plane publication closure archive missing digest evidence",
        asset,
    )
require(dual_repo_publication_closure_summary.get("trust_boundary", {}).get("mutates_releases") is False, "dual-repo publication closure mutated releases", dual_repo_publication_closure_summary)
require(dual_repo_publication_closure_summary.get("trust_boundary", {}).get("mutates_github_releases") is False, "dual-repo publication closure mutated GitHub releases", dual_repo_publication_closure_summary)

stable_release_evidence_packet_path, stable_release_evidence_packet = load_json("ao2-stable-release-evidence-packet/packet/summary.json")
require(stable_release_evidence_packet.get("schema_version") == "ao2.stable-release-evidence-packet.v1", "unexpected stable release evidence packet schema", stable_release_evidence_packet)
require(stable_release_evidence_packet.get("status") == "passed", "stable release evidence packet did not pass", stable_release_evidence_packet)
require(stable_release_evidence_packet.get("stable_release_evidence_ready") is True, "stable release evidence packet was not ready", stable_release_evidence_packet)
require(stable_release_evidence_packet.get("stable_promotion", {}).get("schema_version") == "ao2.stable-promotion-workflow.v1", "unexpected stable promotion schema in stable evidence packet", stable_release_evidence_packet)
require(stable_release_evidence_packet.get("operator_evidence", {}).get("schema_version") == "ao2.operator-release-evidence-bundle.v1", "unexpected operator evidence schema in stable evidence packet", stable_release_evidence_packet)
require(stable_release_evidence_packet.get("operator_evidence", {}).get("operator_release_evidence_ready") is True, "operator evidence was not ready in stable evidence packet", stable_release_evidence_packet)
public_pair_digest_audit = stable_release_evidence_packet.get("public_pair_digest_audit", {})
require(public_pair_digest_audit.get("schema_version") == "ao2.public-release-pair-digest-audit.v1", "unexpected public pair digest audit schema in stable evidence packet", stable_release_evidence_packet)
require(
    public_pair_digest_audit.get("status") == "passed" and public_pair_digest_audit.get("archive_parity_status") == "passed",
    "stable release evidence packet public pair digest audit was not ready",
    stable_release_evidence_packet,
)
stable_packet_rsi = stable_release_evidence_packet.get("rsi_cross_repo_e2e", {})
require(
    stable_packet_rsi.get("schema_version") == "ao2.rsi-cross-repo-e2e.v1"
    and stable_packet_rsi.get("status") == "passed"
    and stable_packet_rsi.get("claim_publish_decision") == "deny"
    and stable_packet_rsi.get("claim_publish_authority") is False
    and stable_packet_rsi.get("covenant_gate_schema_version") == "covenant.rsi-claim-publish-gate.v1"
    and stable_packet_rsi.get("covenant_gate_status") == "denied",
    "stable release evidence packet RSI claim-publish boundary was not denied",
    stable_release_evidence_packet,
)
stable_packet_improvement = stable_release_evidence_packet.get("rsi_improvement_evidence", {})
require(
    stable_packet_improvement.get("schema_version") == "ao2.rsi-improvement-evidence-gate.v1"
    and stable_packet_improvement.get("status") == "passed"
    and stable_packet_improvement.get("improvement_ready") is True
    and stable_packet_improvement.get("measured_improvement_percent", 0) >= 5
    and stable_packet_improvement.get("claim_publish_decision") == "deny"
    and stable_packet_improvement.get("claim_publish_authority") is False,
    "stable release evidence packet RSI improvement evidence was not ready",
    stable_release_evidence_packet,
)
stable_packet_blueprint_authorization = stable_release_evidence_packet.get("rsi_blueprint_authorization", {})
require(
    stable_packet_blueprint_authorization.get("schema_version") == "ao2.rsi-blueprint-authorization-gate.v1"
    and stable_packet_blueprint_authorization.get("status") == "passed"
    and stable_packet_blueprint_authorization.get("blueprint_authorization_ready") is True
    and stable_packet_blueprint_authorization.get("gate_model") == "tiered"
    and stable_packet_blueprint_authorization.get("source") == "ao-blueprint"
    and stable_packet_blueprint_authorization.get("self_authorized_by_rsi") is False
    and stable_packet_blueprint_authorization.get("authorizes_claim_publication") is False
    and stable_packet_blueprint_authorization.get("authorizes_ao_blueprint_self_change") is False,
    "stable release evidence packet RSI Blueprint authorization was not ready",
    stable_release_evidence_packet,
)
stable_packet_trend = stable_release_evidence_packet.get("rsi_improvement_trend", {})
require(
    stable_packet_trend.get("schema_version") == "ao2.rsi-improvement-trend.v1"
    and stable_packet_trend.get("status") == "passed"
    and stable_packet_trend.get("trend_ready") is True
    and stable_packet_trend.get("current_measured_improvement_percent", 0) >= 5
    and stable_packet_trend.get("claim_publish_decision") == "deny"
    and stable_packet_trend.get("claim_publish_authority") is False,
    "stable release evidence packet RSI improvement trend was not ready",
    stable_release_evidence_packet,
)
stable_packet_eligibility = stable_release_evidence_packet.get("rsi_eligibility_packet", {})
require(
    stable_packet_eligibility.get("schema_version") == "ao2.rsi-eligibility-packet.v1"
    and stable_packet_eligibility.get("status") == "passed"
    and stable_packet_eligibility.get("rsi_eligibility_ready") is True
    and stable_packet_eligibility.get("baseline_count", 0) >= stable_packet_eligibility.get("minimum_baseline_count", 2)
    and stable_packet_eligibility.get("claim_publish_decision") == "deny"
    and stable_packet_eligibility.get("claim_publish_authority") is False,
    "stable release evidence packet RSI eligibility packet was not ready",
    stable_release_evidence_packet,
)
require(stable_release_evidence_packet.get("trust_boundary", {}).get("mutates_releases") is False, "stable release evidence packet mutated releases", stable_release_evidence_packet)
require(stable_release_evidence_packet.get("trust_boundary", {}).get("stores_credentials") is False, "stable release evidence packet stored credentials", stable_release_evidence_packet)
require((consumer_root / "ao2-stable-release-evidence-packet/packet/dashboard.html").is_file(), "missing stable release evidence packet dashboard")

required_checks = [
    "ci_job_required_os:verify",
    "ci_job_required_os:release-archive-hosted-smoke",
    "ci_job_required_os:workbench-operator-packet-control-plane-smoke",
    "ci_release_readiness_static_artifact_job",
    "ci_release_readiness_hosted_artifact_gate_job",
    "ci_release_train_control_plane_bridge_artifact_job",
    "ci_ai_task_board_control_plane_bridge_artifact_job",
    "ci_pulse_task_board_closure_packet_artifact_job",
    "ci_pulse_ao2_event_loop_smoke_artifact_job",
    "ci_rsi_cross_repo_e2e_artifact_job",
    "ci_dual_repo_installed_release_smoke_artifact_job",
    "ci_release_publication_closure_artifact_job",
    "ci_dual_repo_release_publication_closure_index_job",
    "ci_stable_release_evidence_packet_artifact_job",
]
checks = {item.get("name"): item for item in summary.get("checks", [])}
missing = [
    name
    for name in required_checks
    if checks.get(name, {}).get("status") != "passed"
]
if missing:
    raise SystemExit(f"release-readiness artifact missing passed checks: {missing}")

consumer_summary = {
    "schema_version": "ao2.release-readiness-artifact-consumer.v1",
    "status": "passed",
    "dashboard": str(dashboard_path),
    "source_artifacts": [
        "ao2-release-readiness",
        "ao2-release-readiness-hosted-artifact-gate",
        "ao2-release-train-control-plane-bridge",
        "ao2-ai-task-board-control-plane-bridge",
        "ao2-pulse-task-board-closure-packet",
        "ao2-pulse-ao2-event-loop-smoke",
        "ao2-rsi-cross-repo-e2e",
        "ao2-rsi-baseline-packet",
        "ao2-rsi-eligibility-packet",
        "ao2-dual-repo-installed-release-smoke",
        "ao2-release-publication-closure",
        "ao2-dual-repo-release-publication-closure-index",
        "ao2-stable-release-evidence-packet",
    ],
    "source_summaries": [
        str(summary_path_source),
        str(closure_index_path),
        str(hosted_gate_summary_path),
        str(hosted_gate_detail_path),
        str(bridge_summary_path),
        str(task_board_bridge_summary_path),
        str(pulse_task_board_closure_summary_path),
        str(pulse_ao2_smoke_summary_path),
        str(pulse_generate_next_summary_path),
        str(ao2_decision_path),
        str(dual_repo_summary_path),
        str(rsi_cross_repo_summary_path),
        str(rsi_covenant_gate_summary_path),
        str(rsi_baseline_packet_path),
        str(rsi_eligibility_packet_path),
        str(publication_closure_summary_path),
        str(dual_repo_publication_closure_summary_path),
        str(stable_release_evidence_packet_path),
    ],
    "rsi_baseline_packet": {
        "schema_version": rsi_baseline_packet.get("schema_version"),
        "status": rsi_baseline_packet.get("status"),
        "rsi_baseline_ready": rsi_baseline_packet.get("rsi_baseline_ready"),
        "rsi_cross_repo_e2e": baseline_packet_rsi,
        "rsi_blueprint_authorization": baseline_packet_blueprint,
        "rsi_improvement_evidence": baseline_packet_improvement,
        "rsi_improvement_trend": baseline_packet_trend,
    },
    "rsi_eligibility_packet": {
        "schema_version": rsi_eligibility_packet.get("schema_version"),
        "status": rsi_eligibility_packet.get("status"),
        "rsi_eligibility_ready": rsi_eligibility_packet.get("rsi_eligibility_ready"),
        "baseline_count": rsi_eligibility_packet.get("baseline_count"),
        "minimum_baseline_count": rsi_eligibility_packet.get("minimum_baseline_count"),
        "claim_publish_decision": rsi_eligibility_packet.get("claim_publish_decision"),
        "claim_publish_authority": rsi_eligibility_packet.get("claim_publish_authority"),
        "blueprint_authorization": {
            "source": eligibility_blueprint.get("source"),
            "self_authorized_by_rsi": eligibility_blueprint.get("self_authorized_by_rsi"),
            "authorizes_claim_publication": eligibility_blueprint.get("authorizes_claim_publication"),
            "authorizes_ao_blueprint_self_change": eligibility_blueprint.get("authorizes_ao_blueprint_self_change"),
        },
        "improvement_evidence": {
            "minimum_target_percent": eligibility_improvement.get("minimum_target_percent"),
            "minimum_measured_improvement_percent": eligibility_improvement.get("minimum_measured_improvement_percent"),
        },
    },
    "stable_release_evidence_packet": {
        "schema_version": stable_release_evidence_packet.get("schema_version"),
        "status": stable_release_evidence_packet.get("status"),
        "stable_release_evidence_ready": stable_release_evidence_packet.get("stable_release_evidence_ready"),
        "public_pair_digest_audit": {
            "artifact": public_pair_digest_audit.get("artifact"),
            "schema_version": public_pair_digest_audit.get("schema_version"),
            "status": public_pair_digest_audit.get("status"),
            "archive_parity_status": public_pair_digest_audit.get("archive_parity_status"),
            "summary": public_pair_digest_audit.get("summary"),
        },
        "rsi_cross_repo_e2e": {
            "schema_version": stable_packet_rsi.get("schema_version"),
            "status": stable_packet_rsi.get("status"),
            "claim_publish_decision": stable_packet_rsi.get("claim_publish_decision"),
            "claim_publish_authority": stable_packet_rsi.get("claim_publish_authority"),
            "covenant_gate_schema_version": stable_packet_rsi.get("covenant_gate_schema_version"),
            "covenant_gate_status": stable_packet_rsi.get("covenant_gate_status"),
        },
        "rsi_improvement_evidence": {
            "schema_version": stable_packet_improvement.get("schema_version"),
            "status": stable_packet_improvement.get("status"),
            "improvement_ready": stable_packet_improvement.get("improvement_ready"),
            "measured_improvement_percent": stable_packet_improvement.get("measured_improvement_percent"),
            "target_percent": stable_packet_improvement.get("target_percent"),
            "claim_publish_decision": stable_packet_improvement.get("claim_publish_decision"),
            "claim_publish_authority": stable_packet_improvement.get("claim_publish_authority"),
        },
        "rsi_blueprint_authorization": {
            "schema_version": stable_packet_blueprint_authorization.get("schema_version"),
            "status": stable_packet_blueprint_authorization.get("status"),
            "blueprint_authorization_ready": stable_packet_blueprint_authorization.get("blueprint_authorization_ready"),
            "gate_model": stable_packet_blueprint_authorization.get("gate_model"),
            "candidate_id": stable_packet_blueprint_authorization.get("candidate_id"),
            "source": stable_packet_blueprint_authorization.get("source"),
            "self_authorized_by_rsi": stable_packet_blueprint_authorization.get("self_authorized_by_rsi"),
            "authorizes_claim_publication": stable_packet_blueprint_authorization.get("authorizes_claim_publication"),
            "authorizes_ao_blueprint_self_change": stable_packet_blueprint_authorization.get("authorizes_ao_blueprint_self_change"),
        },
        "rsi_improvement_trend": {
            "schema_version": stable_packet_trend.get("schema_version"),
            "status": stable_packet_trend.get("status"),
            "trend_ready": stable_packet_trend.get("trend_ready"),
            "run_count": stable_packet_trend.get("run_count"),
            "current_measured_improvement_percent": stable_packet_trend.get("current_measured_improvement_percent"),
            "delta_from_previous_percent": stable_packet_trend.get("delta_from_previous_percent"),
            "target_percent": stable_packet_trend.get("target_percent"),
            "claim_publish_decision": stable_packet_trend.get("claim_publish_decision"),
            "claim_publish_authority": stable_packet_trend.get("claim_publish_authority"),
        },
        "rsi_eligibility_packet": {
            "schema_version": stable_packet_eligibility.get("schema_version"),
            "status": stable_packet_eligibility.get("status"),
            "rsi_eligibility_ready": stable_packet_eligibility.get("rsi_eligibility_ready"),
            "baseline_count": stable_packet_eligibility.get("baseline_count"),
            "minimum_baseline_count": stable_packet_eligibility.get("minimum_baseline_count"),
            "claim_publish_decision": stable_packet_eligibility.get("claim_publish_decision"),
            "claim_publish_authority": stable_packet_eligibility.get("claim_publish_authority"),
        },
    },
    "public_pair_digest_gate": public_pair_digest_gate,
    "hosted_release_readiness_artifact_gate": {
        "schema_version": hosted_gate_detail.get("schema_version"),
        "status": hosted_gate_detail.get("status"),
        "required": hosted_gate_detail.get("required"),
        "readiness_schema_version": hosted_gate_detail.get("readiness_schema_version"),
        "artifact_closure_schema_version": hosted_gate_detail.get("artifact_closure_schema_version"),
        "public_pair_digest_gate": hosted_public_pair_digest_gate,
    },
    "rsi_cross_repo_e2e": {
        "schema_version": rsi_cross_repo_summary.get("schema_version"),
        "status": rsi_cross_repo_summary.get("status"),
        "claim_publish_decision": rsi_cross_repo_summary.get("claim_publish_decision"),
        "claim_publish_authority": rsi_cross_repo_summary.get("claim_publish_authority"),
        "covenant_gate_schema_version": rsi_cross_repo_summary.get("observed_evidence", {}).get("covenant_gate_schema_version"),
        "covenant_gate_status": rsi_cross_repo_summary.get("observed_evidence", {}).get("covenant_gate_status"),
    },
    "required_checks": required_checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "source": "github_actions_artifact_download",
    },
}

def render_value(value):
    if isinstance(value, bool):
        return "True" if value else "False"
    return str(value)

def render_rows(rows):
    return "\n".join(
        "<tr><th scope=\"row\">{}</th><td>{}</td></tr>".format(
            html.escape(label),
            html.escape(render_value(value)),
        )
        for label, value in rows
    )

rsi_eligibility_rows = [
    ("schema_version", rsi_eligibility_packet.get("schema_version")),
    ("status", rsi_eligibility_packet.get("status")),
    ("rsi_eligibility_ready", rsi_eligibility_packet.get("rsi_eligibility_ready")),
    ("baseline_count", rsi_eligibility_packet.get("baseline_count")),
    ("minimum_baseline_count", rsi_eligibility_packet.get("minimum_baseline_count")),
    ("claim_publish_decision", rsi_eligibility_packet.get("claim_publish_decision")),
    ("claim_publish_authority", rsi_eligibility_packet.get("claim_publish_authority")),
    ("source", eligibility_blueprint.get("source")),
    ("self_authorized_by_rsi", eligibility_blueprint.get("self_authorized_by_rsi")),
    ("authorizes_claim_publication", eligibility_blueprint.get("authorizes_claim_publication")),
    ("authorizes_ao_blueprint_self_change", eligibility_blueprint.get("authorizes_ao_blueprint_self_change")),
    ("minimum_target_percent", eligibility_improvement.get("minimum_target_percent")),
    (
        "minimum_measured_improvement_percent",
        eligibility_improvement.get("minimum_measured_improvement_percent"),
    ),
    ("publishes_claims", eligibility_trust.get("publishes_claims")),
    ("approves_rsi_claims", eligibility_trust.get("approves_rsi_claims")),
    ("mutates_repositories", eligibility_trust.get("mutates_repositories")),
    ("requires_provider_api_key", eligibility_trust.get("requires_provider_api_key")),
]

release_rows = [
    ("schema_version", consumer_summary.get("schema_version")),
    ("status", consumer_summary.get("status")),
    ("release_readiness", summary.get("status")),
    ("hosted_artifact_gate", hosted_gate_detail.get("status")),
    ("stable_release_evidence_ready", stable_release_evidence_packet.get("stable_release_evidence_ready")),
    ("public_pair_digest_archive_parity", public_pair_digest_gate.get("archive_parity_status")),
    ("trust_boundary.source", consumer_summary["trust_boundary"]["source"]),
    ("trust_boundary.stores_credentials", consumer_summary["trust_boundary"]["stores_credentials"]),
]

dashboard_html = """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Release Readiness Artifact Consumer</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 2rem; color: #1f2933; background: #f8fafc; }}
    main {{ max-width: 960px; margin: 0 auto; }}
    h1, h2 {{ color: #102a43; }}
    table {{ width: 100%; border-collapse: collapse; margin: 1rem 0 2rem; background: #fff; }}
    th, td {{ border: 1px solid #d9e2ec; padding: 0.625rem 0.75rem; text-align: left; vertical-align: top; }}
    th {{ width: 34%; background: #edf2f7; font-weight: 600; }}
    code {{ background: #edf2f7; padding: 0.1rem 0.25rem; border-radius: 3px; }}
  </style>
</head>
<body>
<main>
  <h1>Release Readiness Artifact Consumer</h1>
  <p>Schema: <code>{consumer_schema}</code></p>
  <section>
    <h2>Release Readiness Readback</h2>
    <table>
      <tbody>
{release_rows}
      </tbody>
    </table>
  </section>
  <section>
    <h2>RSI Eligibility Readback</h2>
    <p>This readback preserves the denial boundary: repeated 5 percent evidence is visible, but autonomous RSI claim publication authority remains denied.</p>
    <table>
      <tbody>
{rsi_eligibility_rows}
      </tbody>
    </table>
  </section>
</main>
</body>
</html>
""".format(
    consumer_schema=html.escape(consumer_summary["schema_version"]),
    release_rows=render_rows(release_rows),
    rsi_eligibility_rows=render_rows(rsi_eligibility_rows),
)

dashboard_path.write_text(dashboard_html, encoding="utf-8")
summary_path.write_text(json.dumps(consumer_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
PY
