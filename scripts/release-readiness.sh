#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_RELEASE_READINESS_ROOT:-$ROOT/target/release-readiness/$(date -u +%Y%m%dT%H%M%SZ)}"
MODE="default"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --static-only)
      MODE="static-only"
      shift
      ;;
    --full)
      MODE="full"
      shift
      ;;
    *)
      echo "usage: scripts/release-readiness.sh [--static-only|--full]" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$OUT_ROOT"
SUMMARY="$OUT_ROOT/summary.json"

echo "release_readiness_root=$OUT_ROOT"
echo "mode=$MODE"

python3 - "$ROOT" "$CP_ROOT" "$MODE" "$SUMMARY" <<'PY'
import json
import html
import os
import re
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1])
cp_root = Path(sys.argv[2])
mode = sys.argv[3]
summary_path = Path(sys.argv[4])

checks = []

def add(name, status, detail=""):
    checks.append({"name": name, "status": status, "detail": detail})

def read(path):
    return (root / path).read_text(encoding="utf-8")

def run(args, cwd=root):
    return subprocess.run(args, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

package = json.loads(read("package.json"))
scripts = package.get("scripts", {})
for name in [
    "risky-pr:golden",
    "release:readiness",
    "release:readiness:static",
    "release:readiness:artifact-consumer",
    "release:readiness:regression-gate",
    "release:metadata-drift-audit",
    "smoke:evidence-control-plane",
]:
    add(f"package_script:{name}", "passed" if name in scripts else "failed", scripts.get(name, "missing"))

ci = read(".github/workflows/ci.yml")
add("ci_pull_request_enabled", "passed" if re.search(r"(?m)^\s*pull_request:\s*$", ci) else "failed")
add("ci_main_push_enabled", "passed" if re.search(r"(?m)^\s*branches:\s*\[\s*main\s*\]\s*$", ci) else "failed")
ci_read_only_permissions_ok = (
    "permissions:" in ci
    and "  actions: read" in ci
    and "  contents: read" in ci
    and "  contents: write" not in ci
    and "  actions: write" not in ci
)
add("ci_read_only_permissions", "passed" if ci_read_only_permissions_ok else "failed")

required_ci_os = ["ubuntu-latest", "macos-latest", "windows-latest"]

def workflow_job_block(job_name):
    match = re.search(
        rf"(?ms)^  {re.escape(job_name)}:\n(?P<body>.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)",
        ci,
    )
    return match.group("body") if match else None

def add_job_matrix_os_check(job_name, expected_os):
    block = workflow_job_block(job_name)
    if block is None:
        add(f"ci_job_required_os:{job_name}", "failed", "job_missing")
        return
    missing = [os_name for os_name in expected_os if os_name not in block]
    add(
        f"ci_job_required_os:{job_name}",
        "passed" if not missing else "failed",
        "required_os=" + ",".join(expected_os) + (" missing=" + ",".join(missing) if missing else ""),
    )

add_job_matrix_os_check("verify", required_ci_os)
add_job_matrix_os_check("release-archive-hosted-smoke", required_ci_os)
add_job_matrix_os_check("workbench-operator-packet-control-plane-smoke", required_ci_os)
add_job_matrix_os_check("non_approval_required_check_compat", ["macos-latest", "windows-latest"])

operator_index = workflow_job_block("workbench-operator-packet-control-plane-smoke-index")
operator_index_ok = (
    operator_index is not None
    and "needs: workbench-operator-packet-control-plane-smoke" in operator_index
    and "AO2_WORKBENCH_OPERATOR_PACKET_CP_INDEX_REQUIRED_OS: ubuntu-latest,macos-latest,windows-latest" in operator_index
)
add(
    "ci_workbench_operator_packet_smoke_index_requires_all_os",
    "passed" if operator_index_ok else "failed",
    "requires ubuntu-latest,macos-latest,windows-latest uploaded smoke artifacts",
)

release_readiness_artifacts = workflow_job_block("release-readiness-artifacts")
release_readiness_artifacts_ok = (
    release_readiness_artifacts is not None
    and "scripts/release-readiness.sh --static-only" in release_readiness_artifacts
    and "ao2-release-readiness" in release_readiness_artifacts
    and "target/release-readiness-ci" in release_readiness_artifacts
)
add(
    "ci_release_readiness_static_artifact_job",
    "passed" if release_readiness_artifacts_ok else "failed",
    "runs static release readiness and uploads target/release-readiness-ci",
)

release_train_bridge_artifacts = workflow_job_block("release-train-control-plane-bridge-artifacts")
release_train_bridge_artifacts_ok = (
    release_train_bridge_artifacts is not None
    and "ao2-release-train-control-plane-bridge" in release_train_bridge_artifacts
    and "target/release-train-control-plane-bridge-ci" in release_train_bridge_artifacts
    and "ao2.release-train-control-plane-bridge.v1" in release_train_bridge_artifacts
    and "ao2.cp-release-train-bridge-smoke.v1" in release_train_bridge_artifacts
)
add(
    "ci_release_train_control_plane_bridge_artifact_job",
    "passed" if release_train_bridge_artifacts_ok else "failed",
    "runs release train control-plane bridge and uploads read-only bridge evidence",
)

ai_task_board_bridge_artifacts = workflow_job_block("ai-task-board-control-plane-bridge-artifacts")
ai_task_board_bridge_artifacts_ok = (
    ai_task_board_bridge_artifacts is not None
    and "ao2-ai-task-board-control-plane-bridge" in ai_task_board_bridge_artifacts
    and "target/ai-task-board-control-plane-bridge-ci" in ai_task_board_bridge_artifacts
    and "ao2.ai-task-board-control-plane-bridge.v1" in ai_task_board_bridge_artifacts
    and "ao2.cp-ai-task-board-readback.v1" in ai_task_board_bridge_artifacts
    and "ao2.cp-ai-task-board-dashboard.v1" in ai_task_board_bridge_artifacts
)
add(
    "ci_ai_task_board_control_plane_bridge_artifact_job",
    "passed" if ai_task_board_bridge_artifacts_ok else "failed",
    "runs AI task board control-plane bridge and uploads read-only bridge evidence",
)

pulse_task_board_closure_packet_artifacts = workflow_job_block("pulse-task-board-closure-packet-artifacts")
pulse_task_board_closure_packet_artifacts_ok = (
    pulse_task_board_closure_packet_artifacts is not None
    and "ao2-pulse-task-board-closure-packet" in pulse_task_board_closure_packet_artifacts
    and "target/pulse-task-board-closure-packet-ci" in pulse_task_board_closure_packet_artifacts
    and "npm run pulse:task-board-closure-packet" in pulse_task_board_closure_packet_artifacts
    and "ao2.pulse-task-board-closure-packet.v1" in pulse_task_board_closure_packet_artifacts
    and "ao2.pulse-next-actions.v1" in pulse_task_board_closure_packet_artifacts
    and "ao2.pulse-task-board-state.v1" in pulse_task_board_closure_packet_artifacts
    and "ao2.control-plane-fixture-consumer-smoke.v1" in pulse_task_board_closure_packet_artifacts
    and "safety_fields_preserved" in pulse_task_board_closure_packet_artifacts
)
add(
    "ci_pulse_task_board_closure_packet_artifact_job",
    "passed" if pulse_task_board_closure_packet_artifacts_ok else "failed",
    "runs Pulse task-board closure packet and uploads aligned next-actions/state/control-plane evidence",
)

pulse_codex_cron_event_loop_smoke_artifacts = workflow_job_block("pulse-codex-cron-event-loop-smoke-artifacts")
pulse_codex_cron_event_loop_smoke_artifacts_ok = (
    pulse_codex_cron_event_loop_smoke_artifacts is not None
    and "ao2-pulse-codex-cron-event-loop-smoke" in pulse_codex_cron_event_loop_smoke_artifacts
    and "target/pulse-codex-cron-event-loop-smoke-ci" in pulse_codex_cron_event_loop_smoke_artifacts
    and "npm run pulse:codex-cron-event-loop-smoke" in pulse_codex_cron_event_loop_smoke_artifacts
    and "ao2.pulse-codex-cron-event-loop-smoke.v1" in pulse_codex_cron_event_loop_smoke_artifacts
    and "codex-cron.event-loop-decision.v1" in pulse_codex_cron_event_loop_smoke_artifacts
    and "ao2.pulse-codex-cron-event-loop-decision.v1" in pulse_codex_cron_event_loop_smoke_artifacts
    and "decision_source" in pulse_codex_cron_event_loop_smoke_artifacts
    and "provider_execution" in pulse_codex_cron_event_loop_smoke_artifacts
)
add(
    "ci_pulse_codex_cron_event_loop_smoke_artifact_job",
    "passed" if pulse_codex_cron_event_loop_smoke_artifacts_ok else "failed",
    "runs Pulse through codex-cron event-loop decision-file handoff and uploads non-provider smoke evidence",
)

dual_repo_installed_smoke_artifacts = workflow_job_block("dual-repo-installed-release-smoke-artifacts")
dual_repo_installed_smoke_artifacts_ok = (
    dual_repo_installed_smoke_artifacts is not None
    and "ao2-dual-repo-installed-release-smoke" in dual_repo_installed_smoke_artifacts
    and "target/dual-repo-installed-release-smoke-ci" in dual_repo_installed_smoke_artifacts
    and "ao2.dual-repo-installed-release-smoke.v1" in dual_repo_installed_smoke_artifacts
    and "ao2.release-manifest.v1" in dual_repo_installed_smoke_artifacts
    and "ao2-control-plane.release-manifest.v1" in dual_repo_installed_smoke_artifacts
    and "ao2.cp-ai-task-board-readback.v1" in dual_repo_installed_smoke_artifacts
    and "ao2.cp-ai-task-board-dashboard.v1" in dual_repo_installed_smoke_artifacts
)
add(
    "ci_dual_repo_installed_release_smoke_artifact_job",
    "passed" if dual_repo_installed_smoke_artifacts_ok else "failed",
    "runs AO2 plus ao2-control-plane installed release archive smoke and uploads token-free evidence",
)

release_publication_closure_artifacts = workflow_job_block("release-publication-closure-artifacts")
release_publication_closure_artifacts_ok = (
    release_publication_closure_artifacts is not None
    and "ao2-release-publication-closure" in release_publication_closure_artifacts
    and "target/release-publication-closure-ci" in release_publication_closure_artifacts
    and "dtolnay/rust-toolchain@stable" in release_publication_closure_artifacts
    and "Download published provenance sidecars" in release_publication_closure_artifacts
    and "gh release download" in release_publication_closure_artifacts
    and "AO2_RELEASE_PROVENANCE_DIR=target/release-publication-provenance" in release_publication_closure_artifacts
    and "AO2_RELEASE_ASSET_PUBLICATION_READINESS_CI_SAFE=1" in release_publication_closure_artifacts
    and "AO2_RELEASE_PUBLICATION_DRY_RUN_CLOSURE_ROOT=target/release-publication-closure-ci" in release_publication_closure_artifacts
    and "npm run release:publication-dry-run-closure" in release_publication_closure_artifacts
    and "if: always()" in release_publication_closure_artifacts
    and "ao2.release-publication-dry-run-closure.v1" in release_publication_closure_artifacts
    and "publication_ready" in release_publication_closure_artifacts
    and "stable_release_ready" in release_publication_closure_artifacts
    and "upload_status" in release_publication_closure_artifacts
    and "not_attempted" in release_publication_closure_artifacts
    and "mutates_releases" in release_publication_closure_artifacts
)
add(
    "ci_release_publication_closure_artifact_job",
    "passed" if release_publication_closure_artifacts_ok else "failed",
    "runs release publication dry-run closure and uploads non-mutating release publication evidence",
)

dual_repo_release_publication_closure_index = workflow_job_block("dual-repo-release-publication-closure-index")
dual_repo_release_publication_closure_index_ok = (
    dual_repo_release_publication_closure_index is not None
    and "needs: release-publication-closure-artifacts" in dual_repo_release_publication_closure_index
    and "ao2-dual-repo-release-publication-closure-index" in dual_repo_release_publication_closure_index
    and "ao2-control-plane-release-publication-closure" in dual_repo_release_publication_closure_index
    and "Download AO2 public archive assets for closure index" in dual_repo_release_publication_closure_index
    and "target/dual-repo-release-publication-closure-index/ao2-release-archives" in dual_repo_release_publication_closure_index
    and "gh release download v0.4.80" in dual_repo_release_publication_closure_index
    and "ao2_archive_assets" in dual_repo_release_publication_closure_index
    and "hashlib.sha256(path.read_bytes()).hexdigest()" in dual_repo_release_publication_closure_index
    and "ao2-0.4.80-linux-aarch64.tar.gz" in dual_repo_release_publication_closure_index
    and "ao2-0.4.80-linux-x86_64.tar.gz" in dual_repo_release_publication_closure_index
    and "ao2-0.4.80-macos-aarch64.tar.gz" in dual_repo_release_publication_closure_index
    and "ao2-0.4.80-windows-x86_64.tar.gz" in dual_repo_release_publication_closure_index
    and "target/dual-repo-release-publication-closure-index/ao2-release-publication-closure" in dual_repo_release_publication_closure_index
    and "target/dual-repo-release-publication-closure-index/ao2-control-plane-release-publication-closure" in dual_repo_release_publication_closure_index
    and "gh run list --repo uesugitorachiyo/ao2-control-plane --branch main --workflow CI" in dual_repo_release_publication_closure_index
    and 'gh run download "$candidate_run_id" --repo uesugitorachiyo/ao2-control-plane' in dual_repo_release_publication_closure_index
    and "ao2.dual-repo-release-publication-closure-index.v1" in dual_repo_release_publication_closure_index
    and "ao2.release-publication-dry-run-closure.v1" in dual_repo_release_publication_closure_index
    and "ao2.cp-release-publication-closure.v1" in dual_repo_release_publication_closure_index
    and "checksum_verified" in dual_repo_release_publication_closure_index
    and "ao2-control-plane-" in dual_repo_release_publication_closure_index
    and ".tar.gz" in dual_repo_release_publication_closure_index
    and "sha256" in dual_repo_release_publication_closure_index
    and "size_bytes" in dual_repo_release_publication_closure_index
    and "mutates_github_releases" in dual_repo_release_publication_closure_index
)
add(
    "ci_dual_repo_release_publication_closure_index_job",
    "passed" if dual_repo_release_publication_closure_index_ok else "failed",
    "downloads AO2 and ao2-control-plane release publication closure artifacts and uploads a combined closure index",
)

stable_release_evidence_packet_artifacts = workflow_job_block("stable-release-evidence-packet-artifacts")
stable_release_evidence_packet_artifacts_ok = (
    stable_release_evidence_packet_artifacts is not None
    and "name: Stable release evidence packet artifacts" in stable_release_evidence_packet_artifacts
    and "GH_TOKEN: ${{ github.token }}" in stable_release_evidence_packet_artifacts
    and "AO2_STABLE_PROMOTION_ROOT=target/stable-release-evidence-packet-ci/stable-promotion-workflow" in stable_release_evidence_packet_artifacts
    and "npm run release:stable-promotion-workflow" in stable_release_evidence_packet_artifacts
    and "AO2_OPERATOR_RELEASE_EVIDENCE_ROOT=target/stable-release-evidence-packet-ci/operator-release-evidence-bundle" in stable_release_evidence_packet_artifacts
    and "npm run release:operator-evidence-bundle" in stable_release_evidence_packet_artifacts
    and "AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT=target/stable-release-evidence-packet-ci/packet" in stable_release_evidence_packet_artifacts
    and "AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY=target/stable-release-evidence-packet-ci/stable-promotion-workflow/summary.json" in stable_release_evidence_packet_artifacts
    and "AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY=target/stable-release-evidence-packet-ci/operator-release-evidence-bundle/summary.json" in stable_release_evidence_packet_artifacts
    and "npm run release:stable-evidence-packet" in stable_release_evidence_packet_artifacts
    and "ao2.stable-release-evidence-packet.v1" in stable_release_evidence_packet_artifacts
    and "stable_release_evidence_ready" in stable_release_evidence_packet_artifacts
    and "mutates_releases" in stable_release_evidence_packet_artifacts
    and "stores_credentials" in stable_release_evidence_packet_artifacts
    and "name: ao2-stable-release-evidence-packet" in stable_release_evidence_packet_artifacts
    and "target/stable-release-evidence-packet-ci" in stable_release_evidence_packet_artifacts
)
add(
    "ci_stable_release_evidence_packet_artifact_job",
    "passed" if stable_release_evidence_packet_artifacts_ok else "failed",
    "runs stable promotion and operator evidence baselines, composes the stable release evidence packet, and uploads non-mutating evidence",
)

stable_release_promotion_workflow = read(".github/workflows/stable-release-promotion.yml")
stable_release_promotion_workflow_ok = (
    "name: Stable Release Promotion" in stable_release_promotion_workflow
    and "workflow_dispatch:" in stable_release_promotion_workflow
    and "stable_release_evidence_run_id:" in stable_release_promotion_workflow
    and "promotion_confirm:" in stable_release_promotion_workflow
    and "actions: read" in stable_release_promotion_workflow
    and "contents: write" in stable_release_promotion_workflow
    and "GH_TOKEN: ${{ github.token }}" in stable_release_promotion_workflow
    and "STABLE_RELEASE_EVIDENCE_RUN_ID: ${{ inputs.stable_release_evidence_run_id }}" in stable_release_promotion_workflow
    and "PROMOTION_CONFIRM_INPUT: ${{ inputs.promotion_confirm }}" in stable_release_promotion_workflow
    and "ao2-stable-release-evidence-packet" in stable_release_promotion_workflow
    and "target/stable-release-promotion/stable-release-evidence-packet" in stable_release_promotion_workflow
    and "ao2.stable-release-evidence-packet.v1" in stable_release_promotion_workflow
    and "stable_release_evidence_ready" in stable_release_promotion_workflow
    and "operator_release_evidence_ready" in stable_release_promotion_workflow
    and "AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR=target/stable-release-promotion/stable-release-evidence-packet/stable-promotion-workflow/post-release-verification-evidence" in stable_release_promotion_workflow
    and 'AO2_STABLE_PROMOTION_CONFIRM="$PROMOTION_CONFIRM_INPUT"' in stable_release_promotion_workflow
    and "npm run release:stable-promotion-workflow" in stable_release_promotion_workflow
    and "promote-stable-v0.4.80-v0.1.13" in stable_release_promotion_workflow
    and "refusing stable promotion because workflow input did not match required confirmation" in stable_release_promotion_workflow
    and "actions/upload-artifact@v7.0.1" in stable_release_promotion_workflow
    and "ao2-stable-release-promotion-workflow" in stable_release_promotion_workflow
    and "OPENAI_API_KEY:" not in stable_release_promotion_workflow
    and "ANTHROPIC_API_KEY:" not in stable_release_promotion_workflow
    and "AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD=1" not in stable_release_promotion_workflow
)
add(
    "ci_stable_release_promotion_workflow_dispatch",
    "passed" if stable_release_promotion_workflow_ok else "failed",
    "manual stable release promotion consumes the hosted stable evidence packet and requires exact confirmation before release mutation",
)

stable_release_promotion_dry_run_audit = read(".github/workflows/stable-release-promotion-dry-run-audit.yml")
stable_release_promotion_dry_run_audit_ok = (
    "name: Stable Release Promotion Dry-Run Audit" in stable_release_promotion_dry_run_audit
    and "workflow_dispatch:" in stable_release_promotion_dry_run_audit
    and "stable_promotion_run_id:" in stable_release_promotion_dry_run_audit
    and "actions: read" in stable_release_promotion_dry_run_audit
    and "contents: read" in stable_release_promotion_dry_run_audit
    and "GH_TOKEN: ${{ github.token }}" in stable_release_promotion_dry_run_audit
    and "STABLE_PROMOTION_RUN_ID: ${{ inputs.stable_promotion_run_id }}" in stable_release_promotion_dry_run_audit
    and "ao2-stable-release-promotion-workflow" in stable_release_promotion_dry_run_audit
    and "target/stable-promotion-dry-run-audit/artifact" in stable_release_promotion_dry_run_audit
    and "npm run release:stable-promotion-dry-run-audit" in stable_release_promotion_dry_run_audit
    and "ao2-stable-release-promotion-dry-run-audit" in stable_release_promotion_dry_run_audit
    and scripts.get("release:stable-promotion-dry-run-audit") == "node scripts/run-sh-script.js scripts/stable-promotion-dry-run-audit.sh"
)
add(
    "ci_stable_release_promotion_dry_run_audit_workflow",
    "passed" if stable_release_promotion_dry_run_audit_ok else "failed",
    "manual post-dispatch audit downloads a stable promotion dry-run artifact and validates that no release mutation was attempted",
)

stable_promotion_operator_checklist = read(".github/workflows/stable-promotion-operator-checklist.yml")
stable_promotion_operator_checklist_script = read("scripts/stable-promotion-operator-checklist.sh")
stable_promotion_operator_checklist_ok = (
    "name: Stable Promotion Operator Checklist" in stable_promotion_operator_checklist
    and "workflow_dispatch:" in stable_promotion_operator_checklist
    and "stable_promotion_dry_run_audit_run_id:" in stable_promotion_operator_checklist
    and "actions: read" in stable_promotion_operator_checklist
    and "contents: read" in stable_promotion_operator_checklist
    and "GH_TOKEN: ${{ github.token }}" in stable_promotion_operator_checklist
    and "STABLE_PROMOTION_DRY_RUN_AUDIT_RUN_ID: ${{ inputs.stable_promotion_dry_run_audit_run_id }}" in stable_promotion_operator_checklist
    and "ao2-stable-release-promotion-dry-run-audit" in stable_promotion_operator_checklist
    and "target/stable-promotion-operator-checklist/dry-run-audit" in stable_promotion_operator_checklist
    and "npm run release:stable-promotion-operator-checklist" in stable_promotion_operator_checklist
    and "ao2-stable-promotion-operator-checklist" in stable_promotion_operator_checklist
    and scripts.get("release:stable-promotion-operator-checklist") == "node scripts/run-sh-script.js scripts/stable-promotion-operator-checklist.sh"
    and "ao2.stable-promotion-operator-checklist.v1" in stable_promotion_operator_checklist_script
    and "promote-stable-v0.4.80-v0.1.13" in stable_promotion_operator_checklist_script
    and "No provider API keys are required or accepted" in stable_promotion_operator_checklist_script
)
add(
    "ci_stable_promotion_operator_checklist_workflow",
    "passed" if stable_promotion_operator_checklist_ok else "failed",
    "manual operator checklist converts a passed dry-run audit into a non-mutating human approval packet before stable promotion",
)

stable_promotion_dry_run_checklist = read(".github/workflows/stable-promotion-dry-run-checklist.yml")
stable_promotion_dry_run_checklist_ok = (
    "name: Stable Promotion Dry-Run Checklist" in stable_promotion_dry_run_checklist
    and "workflow_dispatch:" in stable_promotion_dry_run_checklist
    and "stable_release_evidence_run_id:" in stable_promotion_dry_run_checklist
    and "actions: read" in stable_promotion_dry_run_checklist
    and "contents: read" in stable_promotion_dry_run_checklist
    and "GH_TOKEN: ${{ github.token }}" in stable_promotion_dry_run_checklist
    and "STABLE_RELEASE_EVIDENCE_RUN_ID: ${{ inputs.stable_release_evidence_run_id }}" in stable_promotion_dry_run_checklist
    and "ao2-stable-release-evidence-packet" in stable_promotion_dry_run_checklist
    and "target/stable-promotion-dry-run-checklist/stable-release-evidence-packet" in stable_promotion_dry_run_checklist
    and "AO2_STABLE_PROMOTION_ROOT=target/stable-promotion-dry-run-checklist/workflow" in stable_promotion_dry_run_checklist
    and "AO2_STABLE_PROMOTION_CONFIRM=\"\"" in stable_promotion_dry_run_checklist
    and "npm run release:stable-promotion-workflow" in stable_promotion_dry_run_checklist
    and "npm run release:stable-promotion-dry-run-audit" in stable_promotion_dry_run_checklist
    and "npm run release:stable-promotion-operator-checklist" in stable_promotion_dry_run_checklist
    and "ao2.stable-promotion-dry-run-audit.v1" in stable_promotion_dry_run_checklist
    and "ao2.stable-promotion-operator-checklist.v1" in stable_promotion_dry_run_checklist
    and "Assemble lightweight dry-run checklist artifact" in stable_promotion_dry_run_checklist
    and "target/stable-promotion-dry-run-checklist/checklist-artifact" in stable_promotion_dry_run_checklist
    and "stable-release-evidence-packet/packet/summary.json" in stable_promotion_dry_run_checklist
    and "workflow/post-release-verification-evidence/summary.json" in stable_promotion_dry_run_checklist
    and "operator_checklist_ready" in stable_promotion_dry_run_checklist
    and "confirmation_entered" in stable_promotion_dry_run_checklist
    and "ao2-stable-promotion-dry-run-checklist" in stable_promotion_dry_run_checklist
    and "OPENAI_API_KEY" in stable_promotion_dry_run_checklist
    and "ANTHROPIC_API_KEY" in stable_promotion_dry_run_checklist
    and "contents: write" not in stable_promotion_dry_run_checklist
)
add(
    "ci_stable_promotion_dry_run_checklist_workflow",
    "passed" if stable_promotion_dry_run_checklist_ok else "failed",
    "manual dry-run checklist downloads the hosted stable evidence packet, reruns dry-run promotion, audits it, and emits the non-mutating operator checklist",
)

release_readiness_artifact_consumer = workflow_job_block("release-readiness-artifact-consumer")
release_readiness_artifact_consumer_script = read("scripts/release-readiness-artifact-consumer.sh")
release_readiness_artifact_consumer_ok = (
    release_readiness_artifact_consumer is not None
    and "needs: [release-readiness-artifacts, release-train-control-plane-bridge-artifacts, ai-task-board-control-plane-bridge-artifacts, pulse-task-board-closure-packet-artifacts, pulse-codex-cron-event-loop-smoke-artifacts, dual-repo-installed-release-smoke-artifacts, release-publication-closure-artifacts, dual-repo-release-publication-closure-index, stable-release-evidence-packet-artifacts]" in release_readiness_artifact_consumer
    and "actions/download-artifact@v8.0.1" in release_readiness_artifact_consumer
    and "npm run release:readiness:artifact-consumer" in release_readiness_artifact_consumer
    and scripts.get("release:readiness:artifact-consumer") == "node scripts/run-sh-script.js scripts/release-readiness-artifact-consumer.sh"
    and "name: ao2-release-readiness" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-release-readiness" in release_readiness_artifact_consumer
    and "name: ao2-release-train-control-plane-bridge" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-release-train-control-plane-bridge" in release_readiness_artifact_consumer
    and "name: ao2-ai-task-board-control-plane-bridge" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-ai-task-board-control-plane-bridge" in release_readiness_artifact_consumer
    and "name: ao2-pulse-task-board-closure-packet" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-pulse-task-board-closure-packet" in release_readiness_artifact_consumer
    and "name: ao2-pulse-codex-cron-event-loop-smoke" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-pulse-codex-cron-event-loop-smoke" in release_readiness_artifact_consumer
    and "name: ao2-dual-repo-installed-release-smoke" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-dual-repo-installed-release-smoke" in release_readiness_artifact_consumer
    and "name: ao2-release-publication-closure" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-release-publication-closure" in release_readiness_artifact_consumer
    and "name: ao2-dual-repo-release-publication-closure-index" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-dual-repo-release-publication-closure-index" in release_readiness_artifact_consumer
    and "name: ao2-stable-release-evidence-packet" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-stable-release-evidence-packet" in release_readiness_artifact_consumer
    and "ao2.release-readiness-local.v1" in release_readiness_artifact_consumer_script
    and "ao2.release-train-control-plane-bridge.v1" in release_readiness_artifact_consumer_script
    and "ao2.ai-task-board-control-plane-bridge.v1" in release_readiness_artifact_consumer_script
    and "ao2.pulse-task-board-closure-packet.v1" in release_readiness_artifact_consumer_script
    and "ao2.pulse-codex-cron-event-loop-smoke.v1" in release_readiness_artifact_consumer_script
    and "codex-cron.event-loop-decision.v1" in release_readiness_artifact_consumer_script
    and "ao2.dual-repo-installed-release-smoke.v1" in release_readiness_artifact_consumer_script
    and "ao2.release-publication-dry-run-closure.v1" in release_readiness_artifact_consumer_script
    and "ao2.dual-repo-release-publication-closure-index.v1" in release_readiness_artifact_consumer_script
    and "ao2.cp-release-publication-closure.v1" in release_readiness_artifact_consumer_script
    and "ao2.stable-release-evidence-packet.v1" in release_readiness_artifact_consumer_script
    and "stable_release_evidence_ready" in release_readiness_artifact_consumer_script
    and "stable_release_evidence_packet" in release_readiness_artifact_consumer_script
    and "ao2-control-plane-" in release_readiness_artifact_consumer_script
    and ".tar.gz" in release_readiness_artifact_consumer_script
    and "sha256" in release_readiness_artifact_consumer_script
    and "size_bytes" in release_readiness_artifact_consumer_script
    and "publication_ready" in release_readiness_artifact_consumer_script
    and "stable_release_ready" in release_readiness_artifact_consumer_script
    and "ci_job_required_os:verify" in release_readiness_artifact_consumer_script
    and "ci_job_required_os:release-archive-hosted-smoke" in release_readiness_artifact_consumer_script
    and "ci_job_required_os:workbench-operator-packet-control-plane-smoke" in release_readiness_artifact_consumer_script
    and "ci_release_readiness_static_artifact_job" in release_readiness_artifact_consumer_script
    and "ci_release_train_control_plane_bridge_artifact_job" in release_readiness_artifact_consumer_script
    and "ci_ai_task_board_control_plane_bridge_artifact_job" in release_readiness_artifact_consumer_script
    and "ci_pulse_task_board_closure_packet_artifact_job" in release_readiness_artifact_consumer_script
    and "ci_pulse_codex_cron_event_loop_smoke_artifact_job" in release_readiness_artifact_consumer_script
    and "ci_dual_repo_installed_release_smoke_artifact_job" in release_readiness_artifact_consumer_script
    and "ci_release_publication_closure_artifact_job" in release_readiness_artifact_consumer_script
    and "ci_dual_repo_release_publication_closure_index_job" in release_readiness_artifact_consumer_script
    and "ci_stable_release_evidence_packet_artifact_job" in release_readiness_artifact_consumer_script
)
add(
    "ci_release_readiness_artifact_consumer_job",
    "passed" if release_readiness_artifact_consumer_ok else "failed",
    "downloads release-readiness plus control-plane bridge artifacts and validates schema/status/core cross-OS checks",
)

release_metadata_drift_audit_script = read("scripts/release-metadata-drift-audit.sh")
release_metadata_drift_audit_contract_ok = (
    "ao2.release-metadata-drift-audit.v1" in release_metadata_drift_audit_script
    and "gh" in release_metadata_drift_audit_script
    and "release" in release_metadata_drift_audit_script
    and "view" in release_metadata_drift_audit_script
    and "release_name_drift" in release_metadata_drift_audit_script
    and "release_channel_drift" in release_metadata_drift_audit_script
    and "doc_channel_drift" in release_metadata_drift_audit_script
    and "mutates_releases" in release_metadata_drift_audit_script
    and "stores_credentials" in release_metadata_drift_audit_script
)
add(
    "release_metadata_drift_audit_contract",
    "passed" if release_metadata_drift_audit_contract_ok else "failed",
    "validates release names, stable/prerelease channel flags, and public docs without mutation",
)

release_public_pair_digest_audit_script = read("scripts/public-release-pair-digest-audit.sh")
release_public_pair_digest_audit_contract_ok = (
    scripts.get("release:public-pair-digest-audit")
    == "node scripts/run-sh-script.js scripts/public-release-pair-digest-audit.sh"
    and "ao2.public-release-pair-digest-audit.v1" in release_public_pair_digest_audit_script
    and "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON" in release_public_pair_digest_audit_script
    and "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON" in release_public_pair_digest_audit_script
    and "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON" in release_public_pair_digest_audit_script
    and "gh" in release_public_pair_digest_audit_script
    and "release" in release_public_pair_digest_audit_script
    and "view" in release_public_pair_digest_audit_script
    and "required_archive_names" in release_public_pair_digest_audit_script
    and "required_archive_presence" in release_public_pair_digest_audit_script
    and "closure_archive_assets" in release_public_pair_digest_audit_script
    and "full_archive_parity" in release_public_pair_digest_audit_script
    and "ao2-0.4.80-linux-aarch64.tar.gz" in release_public_pair_digest_audit_script
    and "ao2-0.4.80-linux-x86_64.tar.gz" in release_public_pair_digest_audit_script
    and "ao2-0.4.80-macos-aarch64.tar.gz" in release_public_pair_digest_audit_script
    and "ao2-0.4.80-windows-x86_64.tar.gz" in release_public_pair_digest_audit_script
    and "ao2-control-plane-0.1.13-linux-x86_64.tar.gz" in release_public_pair_digest_audit_script
    and "ao2-control-plane-0.1.13-macos-aarch64.tar.gz" in release_public_pair_digest_audit_script
    and "ao2-control-plane-0.1.13-windows-x86_64.tar.gz" in release_public_pair_digest_audit_script
    and "dual_repo_closure_digest_match" in release_public_pair_digest_audit_script
    and "published_asset_digest_present" in release_public_pair_digest_audit_script
    and "published_asset_size_match" in release_public_pair_digest_audit_script
    and "sha256" in release_public_pair_digest_audit_script
    and "size_bytes" in release_public_pair_digest_audit_script
    and "mutates_releases" in release_public_pair_digest_audit_script
    and "stores_credentials" in release_public_pair_digest_audit_script
)
add(
    "release_public_pair_digest_audit_contract",
    "passed" if release_public_pair_digest_audit_contract_ok else "failed",
    "compares dual-repo closure archive digest metadata with published release asset metadata without mutation",
)

post_release_pair_digest_audit_workflow = read(".github/workflows/post-release-pair-digest-audit.yml")
post_release_pair_digest_audit_forbidden = [
    "pull_request:",
    "push:",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "/Users/torachiyouesugi/Documents/private",
    "target/long-lived-control-plane/api-token",
    "gh release create",
    "gh release edit",
    "gh release upload",
    "git push origin",
    "npm publish",
]
post_release_pair_digest_audit_workflow_ok = (
    "name: Post Release Pair Digest Audit" in post_release_pair_digest_audit_workflow
    and re.search(r"(?m)^\s*workflow_dispatch:\s*$", post_release_pair_digest_audit_workflow)
    and "permissions:" in post_release_pair_digest_audit_workflow
    and "  contents: read" in post_release_pair_digest_audit_workflow
    and "  actions: read" in post_release_pair_digest_audit_workflow
    and "uses: actions/checkout@v6.0.3" in post_release_pair_digest_audit_workflow
    and "uses: actions/setup-node@v6.4.0" in post_release_pair_digest_audit_workflow
    and 'node-version: "22"' in post_release_pair_digest_audit_workflow
    and "gh run list --repo uesugitorachiyo/ao2 --branch main --workflow CI --status success" in post_release_pair_digest_audit_workflow
    and 'gh run download "$run_id" --repo uesugitorachiyo/ao2' in post_release_pair_digest_audit_workflow
    and "--name ao2-dual-repo-release-publication-closure-index" in post_release_pair_digest_audit_workflow
    and "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT=target/post-release-pair-digest-audit" in post_release_pair_digest_audit_workflow
    and "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON=target/post-release-pair-digest-audit-input/summary.json" in post_release_pair_digest_audit_workflow
    and "npm run release:public-pair-digest-audit" in post_release_pair_digest_audit_workflow
    and "ao2.public-release-pair-digest-audit.v1" in post_release_pair_digest_audit_workflow
    and "target/post-release-pair-digest-audit/summary.json" in post_release_pair_digest_audit_workflow
    and "mutates_releases" in post_release_pair_digest_audit_workflow
    and "stores_credentials" in post_release_pair_digest_audit_workflow
    and "uses: actions/upload-artifact@v7.0.1" in post_release_pair_digest_audit_workflow
    and "name: ao2-public-release-pair-digest-audit" in post_release_pair_digest_audit_workflow
    and all(forbidden not in post_release_pair_digest_audit_workflow for forbidden in post_release_pair_digest_audit_forbidden)
)
add(
    "post_release_pair_digest_audit_workflow",
    "passed" if post_release_pair_digest_audit_workflow_ok else "failed",
    "manual read-only workflow downloads the dual-repo closure index and uploads ao2.public-release-pair-digest-audit.v1 evidence",
)

for workflow in [".github/workflows/release-gate.yml", ".github/workflows/public-release-build.yml"]:
    text = read(workflow)
    manual_only = (
        re.search(r"(?m)^\s*workflow_dispatch:\s*$", text)
        and not re.search(r"(?m)^\s*pull_request:\s*$", text)
        and not re.search(r"(?m)^\s*push:\s*$", text)
    )
    add(f"manual_release_workflow:{workflow}", "passed" if manual_only else "failed")

for script in [
    "scripts/risky-pr-golden-path.sh",
    "scripts/release-readiness.sh",
    "scripts/release-readiness-artifact-consumer.sh",
    "scripts/smoke-evidence-pack-control-plane.sh",
    "scripts/release-metadata-drift-audit.sh",
]:
    path = root / script
    add(f"script_present:{script}", "passed" if path.is_file() else "failed")
    add(f"script_executable:{script}", "passed" if path.exists() and path.stat().st_mode & 0o100 else "failed")

for forbidden in ["OPENAI_API_" + "KEY=", "ANTHROPIC_API_" + "KEY=", "cat target/long-lived-control-plane/" + "api-token"]:
    combined = "\n".join((root / path).read_text(encoding="utf-8", errors="replace") for path in [
        "scripts/risky-pr-golden-path.sh",
        "scripts/release-readiness.sh",
        "scripts/smoke-evidence-pack-control-plane.sh",
        "scripts/release-metadata-drift-audit.sh",
    ])
    add(f"provider_key_or_token_literal_absent:{forbidden}", "passed" if forbidden not in combined else "failed")

if mode != "static-only":
    release_metadata_drift_audit_summary = summary_path.with_name("release-metadata-drift-audit") / "summary.json"
    audit_env = os.environ.copy()
    audit_env["AO2_RELEASE_METADATA_DRIFT_AUDIT_ROOT"] = str(release_metadata_drift_audit_summary.parent)
    result = subprocess.run(
        ["npm", "run", "release:metadata-drift-audit"],
        cwd=root,
        env=audit_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    release_metadata_drift_audit_status = "missing"
    if release_metadata_drift_audit_summary.is_file():
        release_metadata_drift_audit_payload = json.loads(
            release_metadata_drift_audit_summary.read_text(encoding="utf-8")
        )
        release_metadata_drift_audit_status = str(
            release_metadata_drift_audit_payload.get("status")
        )
    add(
        "release_metadata_drift_audit",
        "passed"
        if result.returncode == 0 and release_metadata_drift_audit_status == "passed"
        else "failed",
        f"release_metadata_drift_audit_summary={release_metadata_drift_audit_summary} "
        f"release_metadata_drift_audit_status={release_metadata_drift_audit_status}",
    )

    for repo, expected_min in [("uesugitorachiyo/ao2", 1), ("uesugitorachiyo/ao2-control-plane", 1)]:
        result = run(["gh", "api", f"repos/{repo}/branches/main/protection"])
        if result.returncode != 0:
            add(f"branch_protection:{repo}", "failed", result.stderr.strip() or result.stdout.strip())
            continue
        protection = json.loads(result.stdout)
        contexts = protection.get("required_status_checks", {}).get("contexts") or []
        force_pushes = protection.get("allow_force_pushes", {}).get("enabled")
        deletions = protection.get("allow_deletions", {}).get("enabled")
        ok = len(contexts) >= expected_min and force_pushes is False and deletions is False
        add(f"branch_protection:{repo}", "passed" if ok else "failed", f"contexts={len(contexts)} force_pushes={force_pushes} deletions={deletions}")

    for repo in ["uesugitorachiyo/ao2", "uesugitorachiyo/ao2-control-plane"]:
        result = run(["gh", "run", "list", "--repo", repo, "--branch", "main", "--workflow", "CI", "--limit", "1", "--json", "databaseId,status,conclusion,headSha,url"])
        if result.returncode != 0:
            add(f"latest_main_ci:{repo}", "failed", result.stderr.strip() or result.stdout.strip())
            continue
        runs = json.loads(result.stdout)
        latest = runs[0] if runs else {}
        ok = latest.get("status") == "completed" and latest.get("conclusion") == "success"
        add(f"latest_main_ci:{repo}", "passed" if ok else "failed", json.dumps(latest, sort_keys=True))

if mode == "full":
    full_commands = [
        ["npm", "run", "risky-pr:golden"],
        ["npm", "run", "smoke:evidence-control-plane"],
        ["npm", "run", "verify:no-factory-v3"],
    ]
    for command in full_commands:
        result = run(command)
        add("full_command:" + " ".join(command), "passed" if result.returncode == 0 else "failed", (result.stdout + "\n" + result.stderr)[-4000:])

status = "passed" if all(check["status"] == "passed" for check in checks) else "failed"
report_md_path = summary_path.with_name("report.md")
report_html_path = summary_path.with_name("report.html")
artifact_closure_index_path = summary_path.with_name("artifact-closure-index.json")
artifact_closure_index = {
    "schema_version": "ao2.release-artifact-closure-index.v1",
    "status": status,
    "source_summary": str(summary_path),
    "required_artifacts": [
        {
            "id": "release_readiness",
            "artifact_name": "ao2-release-readiness",
            "producer_job": "release-readiness-artifacts",
            "required_files": ["summary.json", "report.md", "report.html"],
            "schema_versions": ["ao2.release-readiness-local.v1"],
            "required_checks": ["ci_release_readiness_static_artifact_job"],
        },
        {
            "id": "release_train_control_plane_bridge",
            "artifact_name": "ao2-release-train-control-plane-bridge",
            "producer_job": "release-train-control-plane-bridge-artifacts",
            "required_files": [
                "latest/summary.json",
                "latest/release-train-summary.json",
                "latest/control-plane.env",
                "latest/control-plane-smoke/summary.json",
                "latest/control-plane-smoke/release-train-readback.json",
                "latest/control-plane-smoke/release-train-readback.html",
            ],
            "schema_versions": [
                "ao2.release-train-control-plane-bridge.v1",
                "ao2.cp-release-train-bridge-smoke.v1",
            ],
            "required_checks": ["ci_release_train_control_plane_bridge_artifact_job"],
        },
        {
            "id": "ai_task_board_control_plane_bridge",
            "artifact_name": "ao2-ai-task-board-control-plane-bridge",
            "producer_job": "ai-task-board-control-plane-bridge-artifacts",
            "required_files": [
                "latest/summary.json",
                "latest/task-board.json",
                "latest/control-plane-smoke/summary.json",
                "latest/control-plane-smoke/ingest-receipt.json",
                "latest/control-plane-smoke/task-board-readback.json",
                "latest/control-plane-smoke/task-board-dashboard.json",
            ],
            "schema_versions": [
                "ao2.ai-task-board-control-plane-bridge.v1",
                "ao2.ai-task-board-control-plane-bridge-smoke.v1",
                "ao2.cp-ai-task-board-readback.v1",
                "ao2.cp-ai-task-board-dashboard.v1",
            ],
            "required_checks": ["ci_ai_task_board_control_plane_bridge_artifact_job"],
        },
        {
            "id": "pulse_task_board_closure_packet",
            "artifact_name": "ao2-pulse-task-board-closure-packet",
            "producer_job": "pulse-task-board-closure-packet-artifacts",
            "required_files": [
                "latest/summary.json",
                "latest/closure-packet.md",
                "latest/task-board/summary.json",
                "latest/next-actions/summary.json",
                "latest/task-board-state/summary.json",
                "latest/control-plane-fixture-consumer-smoke/summary.json",
            ],
            "schema_versions": [
                "ao2.pulse-task-board-closure-packet.v1",
                "ao2.pulse-next-actions.v1",
                "ao2.pulse-task-board-state.v1",
                "ao2.control-plane-fixture-consumer-smoke.v1",
            ],
            "required_checks": ["ci_pulse_task_board_closure_packet_artifact_job"],
        },
        {
            "id": "pulse_codex_cron_event_loop_smoke",
            "artifact_name": "ao2-pulse-codex-cron-event-loop-smoke",
            "producer_job": "pulse-codex-cron-event-loop-smoke-artifacts",
            "required_files": [
                "latest/summary.json",
                "latest/pulse-generate-next/summary.json",
                "latest/pulse-next-recommended-tasks/codex-cron-event-loop-decision.json",
                "latest/codex-cron-run-loop.stdout",
            ],
            "schema_versions": [
                "ao2.pulse-codex-cron-event-loop-smoke.v1",
                "codex-cron.event-loop-decision.v1",
                "ao2.pulse-codex-cron-event-loop-decision.v1",
                "ao2.pulse-generate-next.v1",
            ],
            "required_checks": ["ci_pulse_codex_cron_event_loop_smoke_artifact_job"],
        },
        {
            "id": "dual_repo_installed_release_smoke",
            "artifact_name": "ao2-dual-repo-installed-release-smoke",
            "producer_job": "dual-repo-installed-release-smoke-artifacts",
            "required_files": [
                "latest/summary.json",
                "latest/smoke/ao2-version.json",
                "latest/smoke/task-board.json",
                "latest/smoke/ingest-receipt.json",
                "latest/smoke/task-board-readback.json",
                "latest/smoke/task-board-dashboard.json",
            ],
            "schema_versions": [
                "ao2.dual-repo-installed-release-smoke.v1",
                "ao2.release-manifest.v1",
                "ao2-control-plane.release-manifest.v1",
                "ao2.cp-ai-task-board-readback.v1",
                "ao2.cp-ai-task-board-dashboard.v1",
            ],
            "required_checks": ["ci_dual_repo_installed_release_smoke_artifact_job"],
        },
        {
            "id": "release_publication_closure",
            "artifact_name": "ao2-release-publication-closure",
            "producer_job": "release-publication-closure-artifacts",
            "required_files": [
                "summary.json",
                "release-asset-publication-readiness/summary.json",
                "release-sync-provenance-assets/summary.json",
                "stable-release-readiness/summary.json",
            ],
            "schema_versions": [
                "ao2.release-publication-dry-run-closure.v1",
                "ao2.release-asset-publication-readiness.v1",
                "ao2.release-sync-provenance-assets.v1",
                "ao2.stable-release-readiness.v1",
            ],
            "required_checks": ["ci_release_publication_closure_artifact_job"],
        },
        {
            "id": "dual_repo_release_publication_closure_index",
            "artifact_name": "ao2-dual-repo-release-publication-closure-index",
            "producer_job": "dual-repo-release-publication-closure-index",
            "required_files": [
                "summary.json",
                "ao2-release-publication-closure/summary.json",
                "ao2-control-plane-release-publication-closure/summary.json",
            ],
            "schema_versions": [
                "ao2.dual-repo-release-publication-closure-index.v1",
                "ao2.release-publication-dry-run-closure.v1",
                "ao2.cp-release-publication-closure.v1",
            ],
            "required_checks": ["ci_dual_repo_release_publication_closure_index_job"],
            "source_artifacts": [
                "ao2-release-publication-closure",
                "ao2-control-plane-release-publication-closure",
            ],
        },
        {
            "id": "stable_release_evidence_packet",
            "artifact_name": "ao2-stable-release-evidence-packet",
            "producer_job": "stable-release-evidence-packet-artifacts",
            "required_files": [
                "packet/summary.json",
                "packet/dashboard.html",
                "stable-promotion-workflow/summary.json",
                "operator-release-evidence-bundle/summary.json",
            ],
            "schema_versions": [
                "ao2.stable-release-evidence-packet.v1",
                "ao2.stable-promotion-workflow.v1",
                "ao2.operator-release-evidence-bundle.v1",
            ],
            "required_checks": ["ci_stable_release_evidence_packet_artifact_job"],
            "source_artifacts": [
                "stable-promotion-workflow",
                "operator-release-evidence-bundle",
            ],
        },
        {
            "id": "stable_release_promotion_workflow_dispatch",
            "artifact_name": "ao2-stable-release-promotion-workflow",
            "producer_job": "Stable Release Promotion / stable-release-promotion",
            "required_files": [
                "stable-release-evidence-packet/packet/summary.json",
                "stable-release-evidence-packet/packet/dashboard.html",
                "workflow/summary.json",
                "workflow/post-release-verification-evidence/summary.json",
            ],
            "schema_versions": [
                "ao2.stable-release-evidence-packet.v1",
                "ao2.stable-promotion-workflow.v1",
                "ao2.stable-promotion-evidence-gate.v1",
            ],
            "required_checks": ["ci_stable_release_promotion_workflow_dispatch"],
            "source_artifacts": ["ao2-stable-release-evidence-packet"],
        },
        {
            "id": "stable_release_promotion_dry_run_audit",
            "artifact_name": "ao2-stable-release-promotion-dry-run-audit",
            "producer_job": "Stable Release Promotion Dry-Run Audit / stable-release-promotion-dry-run-audit",
            "required_files": [
                "artifact/workflow/summary.json",
                "artifact/workflow/post-release-verification-evidence/summary.json",
                "artifact/stable-release-evidence-packet/packet/summary.json",
                "report/summary.json",
            ],
            "schema_versions": [
                "ao2.stable-promotion-dry-run-audit.v1",
                "ao2.stable-promotion-workflow.v1",
                "ao2.stable-promotion-evidence-gate.v1",
                "ao2.stable-release-evidence-packet.v1",
            ],
            "required_checks": ["ci_stable_release_promotion_dry_run_audit_workflow"],
            "source_artifacts": ["ao2-stable-release-promotion-workflow"],
        },
        {
            "id": "stable_promotion_operator_checklist",
            "artifact_name": "ao2-stable-promotion-operator-checklist",
            "producer_job": "Stable Promotion Operator Checklist / stable-promotion-operator-checklist",
            "required_files": [
                "dry-run-audit/report/summary.json",
                "report/summary.json",
                "report/checklist.md",
            ],
            "schema_versions": [
                "ao2.stable-promotion-operator-checklist.v1",
                "ao2.stable-promotion-dry-run-audit.v1",
            ],
            "required_checks": ["ci_stable_promotion_operator_checklist_workflow"],
            "source_artifacts": ["ao2-stable-release-promotion-dry-run-audit"],
        },
        {
            "id": "stable_promotion_dry_run_checklist",
            "artifact_name": "ao2-stable-promotion-dry-run-checklist",
            "producer_job": "Stable Promotion Dry-Run Checklist / stable-promotion-dry-run-checklist",
            "required_files": [
                "stable-release-evidence-packet/packet/summary.json",
                "workflow/summary.json",
                "workflow/post-release-verification-evidence/summary.json",
                "dry-run-audit/summary.json",
                "operator-checklist/summary.json",
                "operator-checklist/checklist.md",
            ],
            "schema_versions": [
                "ao2.stable-release-evidence-packet.v1",
                "ao2.stable-promotion-workflow.v1",
                "ao2.stable-promotion-evidence-gate.v1",
                "ao2.stable-promotion-dry-run-audit.v1",
                "ao2.stable-promotion-operator-checklist.v1",
            ],
            "required_checks": ["ci_stable_promotion_dry_run_checklist_workflow"],
            "source_artifacts": ["ao2-stable-release-evidence-packet"],
        },
        {
            "id": "release_readiness_artifact_consumer",
            "artifact_name": "ao2-release-readiness-consumer",
            "producer_job": "release-readiness-artifact-consumer",
            "required_files": ["summary.json"],
            "schema_versions": ["ao2.release-readiness-artifact-consumer.v1"],
            "required_checks": ["ci_release_readiness_artifact_consumer_job"],
            "consumes": [
                "ao2-release-readiness",
                "ao2-release-train-control-plane-bridge",
                "ao2-ai-task-board-control-plane-bridge",
                "ao2-pulse-task-board-closure-packet",
                "ao2-pulse-codex-cron-event-loop-smoke",
                "ao2-dual-repo-installed-release-smoke",
                "ao2-release-publication-closure",
                "ao2-dual-repo-release-publication-closure-index",
                "ao2-stable-release-evidence-packet",
            ],
        },
    ],
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_approves_release": False,
    },
}
artifact_closure_index_path.write_text(
    json.dumps(artifact_closure_index, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
summary = {
    "schema_version": "ao2.release-readiness-local.v1",
    "status": status,
    "mode": mode,
    "ao2_root": str(root),
    "control_plane_root_exists": cp_root.is_dir(),
    "report_md": str(report_md_path),
    "report_html": str(report_html_path),
    "artifact_closure_index": str(artifact_closure_index_path),
    "checks": checks,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

def compact(value, limit=600):
    value = str(value or "").replace("\n", " ").strip()
    if len(value) > limit:
        return value[: limit - 3] + "..."
    return value

lines = [
    "# AO2 Release Readiness",
    "",
    f"- Schema: `{summary['schema_version']}`",
    f"- Status: `{status}`",
    f"- Mode: `{mode}`",
    f"- AO2 root: `{root}`",
    f"- Control-plane root exists: `{cp_root.is_dir()}`",
    "",
    "| Check | Status | Detail |",
    "| --- | --- | --- |",
]
for check in checks:
    name = compact(check["name"]).replace("|", "\\|")
    check_status = compact(check["status"]).replace("|", "\\|")
    detail = compact(check.get("detail", "")).replace("|", "\\|")
    lines.append(f"| `{name}` | `{check_status}` | {detail} |")
report_md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

rows = []
for check in checks:
    rows.append(
        "<tr>"
        f"<td><code>{html.escape(compact(check['name']))}</code></td>"
        f"<td><code>{html.escape(compact(check['status']))}</code></td>"
        f"<td>{html.escape(compact(check.get('detail', '')))}</td>"
        "</tr>"
    )
report_html_path.write_text(
    "<!doctype html>\n"
    "<html><head><meta charset=\"utf-8\"><title>AO2 Release Readiness</title>"
    "<style>body{font-family:system-ui,sans-serif;margin:2rem;line-height:1.45}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #ddd;padding:.4rem;text-align:left}"
    "th{background:#f5f5f5}code{white-space:pre-wrap}</style></head><body>"
    "<h1>AO2 Release Readiness</h1>"
    f"<p><strong>Status:</strong> <code>{html.escape(status)}</code></p>"
    f"<p><strong>Mode:</strong> <code>{html.escape(mode)}</code></p>"
    f"<p><strong>Schema:</strong> <code>{html.escape(summary['schema_version'])}</code></p>"
    "<table><thead><tr><th>Check</th><th>Status</th><th>Detail</th></tr></thead><tbody>"
    + "".join(rows)
    + "</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"report_md={report_md_path}")
print(f"report_html={report_html_path}")
print(f"artifact_closure_index={artifact_closure_index_path}")
print(f"status={status}")
if status != "passed":
    for check in checks:
        if check["status"] != "passed":
            print(f"failed={check['name']} {check.get('detail', '')}", file=sys.stderr)
    raise SystemExit(1)
PY
