#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONSUMER_ROOT="${AO2_RELEASE_READINESS_CONSUMER_ROOT:-$ROOT/target/release-readiness-consumer}"
SUMMARY="$CONSUMER_ROOT/summary.json"

python3 - "$CONSUMER_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from pathlib import Path

consumer_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])

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

pulse_codex_cron_smoke_summary_path, pulse_codex_cron_smoke_summary = load_json("ao2-pulse-codex-cron-event-loop-smoke/latest/summary.json")
require(pulse_codex_cron_smoke_summary.get("schema_version") == "ao2.pulse-codex-cron-event-loop-smoke.v1", "unexpected Pulse codex-cron smoke schema", pulse_codex_cron_smoke_summary)
require(pulse_codex_cron_smoke_summary.get("status") == "passed", "Pulse codex-cron smoke did not pass", pulse_codex_cron_smoke_summary)
require(pulse_codex_cron_smoke_summary.get("codex_cron", {}).get("decision_source") == "file", "Pulse codex-cron smoke did not use file decision source", pulse_codex_cron_smoke_summary)
require(pulse_codex_cron_smoke_summary.get("ao2", {}).get("decision_schema") == "codex-cron.event-loop-decision.v1", "unexpected codex-cron decision schema", pulse_codex_cron_smoke_summary)
require(pulse_codex_cron_smoke_summary.get("ao2", {}).get("ao2_decision_schema") == "ao2.pulse-codex-cron-event-loop-decision.v1", "unexpected AO2 codex-cron decision schema", pulse_codex_cron_smoke_summary)
require(pulse_codex_cron_smoke_summary.get("trust_boundary", {}).get("provider_execution") is False, "Pulse codex-cron smoke must not execute providers", pulse_codex_cron_smoke_summary)
pulse_generate_next_rel = "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-generate-next/summary.json"
codex_cron_decision_rel = "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-next-recommended-tasks/codex-cron-event-loop-decision.json"
codex_cron_stdout_rel = "ao2-pulse-codex-cron-event-loop-smoke/latest/codex-cron-run-loop.stdout"
for rel_path in [pulse_generate_next_rel, codex_cron_decision_rel, codex_cron_stdout_rel]:
    require((consumer_root / rel_path).is_file(), f"missing Pulse codex-cron smoke file {rel_path}")
pulse_generate_next_summary_path, pulse_generate_next_summary = load_json(pulse_generate_next_rel)
require(pulse_generate_next_summary.get("schema_version") == "ao2.pulse-generate-next.v1", "unexpected Pulse generate-next schema", pulse_generate_next_summary)
require(pulse_generate_next_summary.get("status") == "ready", "Pulse generate-next was not ready", pulse_generate_next_summary)
codex_cron_decision_path, codex_cron_decision = load_json(codex_cron_decision_rel)
require(codex_cron_decision.get("schema_version") == "codex-cron.event-loop-decision.v1", "unexpected codex-cron decision file schema", codex_cron_decision)
require(codex_cron_decision.get("ao2", {}).get("schema_version") == "ao2.pulse-codex-cron-event-loop-decision.v1", "unexpected AO2 codex-cron decision file schema", codex_cron_decision)

dual_repo_summary_path, dual_repo_summary = load_json("ao2-dual-repo-installed-release-smoke/latest/summary.json")
require(dual_repo_summary.get("schema_version") == "ao2.dual-repo-installed-release-smoke.v1", "unexpected dual-repo installed smoke schema", dual_repo_summary)
require(dual_repo_summary.get("status") == "passed", "dual-repo installed smoke did not pass", dual_repo_summary)
require(dual_repo_summary.get("archives", {}).get("ao2", {}).get("manifest_schema") == "ao2.release-manifest.v1", "unexpected AO2 archive manifest schema", dual_repo_summary)
require(dual_repo_summary.get("archives", {}).get("ao2_control_plane", {}).get("manifest_schema") == "ao2-control-plane.release-manifest.v1", "unexpected control-plane archive manifest schema", dual_repo_summary)
require(dual_repo_summary.get("trust_boundary", {}).get("auth_value_stored") is False, "dual-repo installed smoke stored auth value", dual_repo_summary)

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
    asset.get("name", "")
    for asset in control_plane_assets
    if isinstance(asset, dict)
    and isinstance(asset.get("name"), str)
    and asset["name"].startswith("ao2-control-plane-")
    and asset["name"].endswith(".tar.gz")
]
require(control_plane_archive_assets, "control-plane publication closure missing release archive asset", dual_repo_publication_closure_summary.get("control_plane", {}))
require(dual_repo_publication_closure_summary.get("trust_boundary", {}).get("mutates_releases") is False, "dual-repo publication closure mutated releases", dual_repo_publication_closure_summary)
require(dual_repo_publication_closure_summary.get("trust_boundary", {}).get("mutates_github_releases") is False, "dual-repo publication closure mutated GitHub releases", dual_repo_publication_closure_summary)

required_checks = [
    "ci_job_required_os:verify",
    "ci_job_required_os:release-archive-hosted-smoke",
    "ci_job_required_os:workbench-operator-packet-control-plane-smoke",
    "ci_release_readiness_static_artifact_job",
    "ci_release_train_control_plane_bridge_artifact_job",
    "ci_ai_task_board_control_plane_bridge_artifact_job",
    "ci_pulse_task_board_closure_packet_artifact_job",
    "ci_pulse_codex_cron_event_loop_smoke_artifact_job",
    "ci_dual_repo_installed_release_smoke_artifact_job",
    "ci_release_publication_closure_artifact_job",
    "ci_dual_repo_release_publication_closure_index_job",
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
    "source_artifacts": [
        "ao2-release-readiness",
        "ao2-release-train-control-plane-bridge",
        "ao2-ai-task-board-control-plane-bridge",
        "ao2-pulse-task-board-closure-packet",
        "ao2-pulse-codex-cron-event-loop-smoke",
        "ao2-dual-repo-installed-release-smoke",
        "ao2-release-publication-closure",
        "ao2-dual-repo-release-publication-closure-index",
    ],
    "source_summaries": [
        str(summary_path_source),
        str(bridge_summary_path),
        str(task_board_bridge_summary_path),
        str(pulse_task_board_closure_summary_path),
        str(pulse_codex_cron_smoke_summary_path),
        str(pulse_generate_next_summary_path),
        str(codex_cron_decision_path),
        str(dual_repo_summary_path),
        str(publication_closure_summary_path),
        str(dual_repo_publication_closure_summary_path),
    ],
    "required_checks": required_checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "source": "github_actions_artifact_download",
    },
}
summary_path.write_text(json.dumps(consumer_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
PY
