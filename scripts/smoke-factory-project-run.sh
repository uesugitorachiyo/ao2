#!/bin/sh
set -eu

# Dogfood the factory-facing project replacement entry points:
# ao2 factory project-plan
# ao2 factory project-start
# ao2 factory project-start-bundle-verify
# ao2 factory queue-run-next auto project-start closure packaging
# ao2 factory queue-run-next auto project-start closure verification
# ao2 factory queue-run-next auto replacement packet packaging
# ao2 factory queue-run-next auto replacement packet verification
# ao2 factory queue-project-start-completion-summary Hermes packet handoff readback
# ao2 factory replacement-packet
# ao2 factory replacement-packet-verify
# ao2 factory queue-project-start-complete one-shot Hermes backend driver
# ao2 factory queue-project-start-next-action read-only Hermes action preview
# ao2 factory project-run
# ao2 factory project-acceptance-review

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
AO2_FACTORY_PROJECT_RUN_ROOT="${AO2_FACTORY_PROJECT_RUN_ROOT:-$repo_root/target/factory-project-run-smoke/$timestamp}"
AO2_BIN="${AO2_BIN:-$repo_root/target/release/ao2}"

ao2_cmd() {
  if [ -x "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  else
    cargo run -p ao2-cli --quiet -- "$@"
  fi
}

mkdir -p "$AO2_FACTORY_PROJECT_RUN_ROOT"
AO2_FACTORY_PROJECT_RUN_ROOT=$(CDPATH= cd -- "$AO2_FACTORY_PROJECT_RUN_ROOT" && pwd)

project_spec="$AO2_FACTORY_PROJECT_RUN_ROOT/missed-call-recovery-project.md"
queued_project_workspace="$AO2_FACTORY_PROJECT_RUN_ROOT/queued-generated-project"
queued_provider_prompt_dir="$queued_project_workspace/provider-prompts"
queued_project_start_out="$AO2_FACTORY_PROJECT_RUN_ROOT/queued-project-start"
queued_project_start_submit_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-submit.json"
queued_project_start_run_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-run-next.json"
queued_project_start_status_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-status.json"
queued_project_start_latest_status_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-latest-status.json"
queued_project_start_completion_contract_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-completion-contract.json"
queued_project_start_completion_contract_consumer_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-completion-contract-consumer.json"
queued_project_start_completion_summary_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-completion-summary.json"
queued_replacement_packet="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-replacement-packet.tgz"
queued_replacement_packet_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-replacement-packet.json"
queued_replacement_packet_verify_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-replacement-packet-verification.json"
one_shot_project_workspace="$AO2_FACTORY_PROJECT_RUN_ROOT/one-shot-generated-project"
one_shot_provider_prompt_dir="$one_shot_project_workspace/provider-prompts"
one_shot_project_start_out="$AO2_FACTORY_PROJECT_RUN_ROOT/one-shot-project-start"
one_shot_project_start_complete_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-complete.json"
one_shot_project_start_complete_status_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-complete-status.json"
one_shot_project_start_next_action_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-queue-project-start-next-action.json"
project_workspace="$AO2_FACTORY_PROJECT_RUN_ROOT/generated-project"
provider_prompt_dir="$project_workspace/provider-prompts"
project_plan="$AO2_FACTORY_PROJECT_RUN_ROOT/missed-call-recovery-project-plan.json"
project_plan_generated="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-plan.json"
project_plan_validation="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-plan-validation.json"
signing_key="$AO2_FACTORY_PROJECT_RUN_ROOT/project-run-signing-key.pem"
project_start_out="$AO2_FACTORY_PROJECT_RUN_ROOT/project-start"
project_start_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-start.json"
project_start_bundle="$AO2_FACTORY_PROJECT_RUN_ROOT/project-start-handoff.tgz"
project_start_bundle_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-start-bundle.json"
project_start_bundle_verify_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-start-bundle-verification.json"
project_start_summary_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-start-operator-summary.json"
project_start_summary_md="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-start-operator-summary.md"
failed_project_out="$AO2_FACTORY_PROJECT_RUN_ROOT/project-run-failed"
failed_project_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-run-failed.json"
project_out="$AO2_FACTORY_PROJECT_RUN_ROOT/project-run"
project_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-run.json"
project_acceptance_review_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-acceptance-review.json"
summary_json="$AO2_FACTORY_PROJECT_RUN_ROOT/factory-project-run-summary.json"

rm -rf "$queued_project_workspace" "$queued_project_start_out" "$one_shot_project_workspace" "$one_shot_project_start_out" "$project_start_out" "$failed_project_out" "$project_out"
mkdir -p "$queued_project_workspace" "$queued_project_start_out" "$one_shot_project_workspace" "$one_shot_project_start_out" "$project_start_out" "$failed_project_out" "$project_out"

cat > "$project_spec" <<'MD'
# Missed Call Recovery Project

Build a governed missed-call revenue recovery project from AO2-dispatched app steps.

Acceptance:
- AO2 generates a deterministic project plan from this human project spec.
- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
- AO2 dispatches app-run steps from a project plan.
- AO2 collects each app-run evidence bundle into one project release-review package.
- Factory-v3 remains evaluator-closer owner and does not drive execution.
- AO2 Control Plane remains read-only observer after signed evidence exists.

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
MD

ao2_cmd workbench support-keygen --out "$signing_key" --bits 2048 >/dev/null

ao2_cmd factory queue-submit-project-start \
  --target "$queued_project_workspace" \
  --project-spec "$project_spec" \
  --project-root "$queued_project_workspace" \
  --run-id missed-call-recovery-project-queued \
  --provider scripted \
  --provider-prompt-dir "$queued_provider_prompt_dir" \
  --verifier-command "true" \
  --signing-key "$signing_key" \
  --signer-id factory-project-start-queue-smoke \
  --out-dir "$queued_project_start_out" \
  --json > "$queued_project_start_submit_json"

ao2_cmd factory queue-run-next \
  --target "$queued_project_workspace" \
  --json > "$queued_project_start_run_json"

ao2_cmd factory queue-status \
  --target "$queued_project_workspace" \
  --run-id missed-call-recovery-project-queued \
  --json > "$queued_project_start_status_json"

ao2_cmd factory queue-status \
  --target "$queued_project_workspace" \
  --latest-completed-project-start \
  --json > "$queued_project_start_latest_status_json"

ao2_cmd factory queue-completion-contract \
  --target "$queued_project_workspace" \
  --latest-completed-project-start \
  --json > "$queued_project_start_completion_contract_json"

ao2_cmd factory queue-completion-contract-consume \
  --contract "$queued_project_start_completion_contract_json" \
  --json > "$queued_project_start_completion_contract_consumer_json"

ao2_cmd factory queue-project-start-completion-summary \
  --target "$queued_project_workspace" \
  --run-id missed-call-recovery-project-queued \
  --json > "$queued_project_start_completion_summary_json"

queued_project_start_closure_bundle=$(node -e "const fs=require('fs'); const run=JSON.parse(fs.readFileSync(process.argv[1], 'utf8')); process.stdout.write(run.entry.project_start_closure);" "$queued_project_start_run_json")
queued_project_start_closure_verification=$(node -e "const fs=require('fs'); const run=JSON.parse(fs.readFileSync(process.argv[1], 'utf8')); process.stdout.write(run.entry.project_start_closure_verification);" "$queued_project_start_run_json")

ao2_cmd factory replacement-packet \
  --queue-status "$queued_project_start_status_json" \
  --latest-queue-status "$queued_project_start_latest_status_json" \
  --closure "$queued_project_start_closure_bundle" \
  --closure-verification "$queued_project_start_closure_verification" \
  --out "$queued_replacement_packet" \
  --json > "$queued_replacement_packet_json"

ao2_cmd factory replacement-packet-verify \
  --bundle "$queued_replacement_packet" \
  --json > "$queued_replacement_packet_verify_json"

ao2_cmd factory queue-project-start-complete \
  --target "$one_shot_project_workspace" \
  --project-spec "$project_spec" \
  --project-root "$one_shot_project_workspace" \
  --run-id missed-call-recovery-project-one-shot \
  --provider scripted \
  --provider-prompt-dir "$one_shot_provider_prompt_dir" \
  --verifier-command "true" \
  --signing-key "$signing_key" \
  --signer-id factory-project-start-one-shot-smoke \
  --out-dir "$one_shot_project_start_out" \
  --json > "$one_shot_project_start_complete_json"

ao2_cmd factory queue-project-start-complete-status \
  --target "$one_shot_project_workspace" \
  --run-id missed-call-recovery-project-one-shot \
  --out-dir "$one_shot_project_start_out" \
  --json > "$one_shot_project_start_complete_status_json"

ao2_cmd factory queue-project-start-next-action \
  --target "$one_shot_project_workspace" \
  --run-id missed-call-recovery-project-one-shot \
  --out-dir "$one_shot_project_start_out" \
  --contract "$repo_root/docs/contracts/hermes-project-start-poll-act-contract.v1.json" \
  --json > "$one_shot_project_start_next_action_json"

ao2_cmd factory project-start \
  --project-spec "$project_spec" \
  --project-root "$project_workspace" \
  --run-id missed-call-recovery-project-start \
  --provider scripted \
  --provider-prompt-dir "$provider_prompt_dir" \
  --verifier-command "true" \
  --signing-key "$signing_key" \
  --signer-id factory-project-start-smoke \
  --out-dir "$project_start_out" \
  --handoff-bundle-out "$project_start_bundle" \
  --handoff-bundle-report "$project_start_bundle_json" \
  --json > "$project_start_json"

ao2_cmd factory project-start-bundle-verify \
  --bundle "$project_start_bundle" \
  --json > "$project_start_bundle_verify_json"

ao2_cmd factory project-start-summary \
  --project-start "$project_start_out/missed-call-recovery-project-start-factory-project-start.json" \
  --bundle-verification "$project_start_bundle_verify_json" \
  --out "$project_start_summary_json" \
  --markdown "$project_start_summary_md" \
  --json > /dev/null

ao2_cmd factory project-plan \
  --project-spec "$project_spec" \
  --project-root "$project_workspace" \
  --run-id missed-call-recovery-project \
  --provider scripted \
  --provider-prompt-dir "$provider_prompt_dir" \
  --verifier-command "python -m pytest -q" \
  --signing-key "$signing_key" \
  --signer-id factory-project-plan-smoke \
  --out "$project_plan" \
  --json > "$project_plan_generated"

write_step_fixture() {
  label="$1"
  target="$project_workspace/apps/$label"
  spec="$project_workspace/specs/$label.md"
  prompt="$provider_prompt_dir/$label-provider-prompt.sh"
  rm -rf "$target"
  mkdir -p "$target"
  cp -R "$repo_root/fixtures/missed-call-recovery/." "$target/"
  cat > "$target/tests/test_project_step.py" <<'PY'
from missed_call_recovery.workflow import LeadCapture, build_recovery_message, classify_lead


def lead(**overrides):
    data = {
        "customer_name": "Riley",
        "phone": "530-555-0133",
        "missed_at_minutes_ago": 7,
        "repeat_calls_24h": 2,
        "service_requested": "emergency leak repair",
        "business_name": "Missed Call Recovery",
        "consent_to_text": True,
    }
    data.update(overrides)
    return LeadCapture(**data)


def test_project_step_message_and_score():
    message = build_recovery_message(lead())
    assert "Riley" in message
    assert "Missed Call Recovery" in message
    assert "emergency leak repair" in message
    assert "reply" in message.lower()
    classification = classify_lead(lead())
    assert classification["priority"] == "hot"
    assert classification["score"] >= 85


def test_project_step_opt_out():
    assert build_recovery_message(lead(consent_to_text=False)) == ""
PY
  cat > "$spec" <<MD
# $label Missed Call Step

Acceptance:
- The implementation models a missed-call lead as a LeadCapture record.
- The recovery message mentions the customer, business, requested service, and a reply path.
- Recent repeat callers are classified as hot with a score of at least 85.
- Leads without text consent return no recovery text.
- The verifier can run with \`python -m pytest -q\`.
MD
  cat > "$prompt" <<'SH'
cat > missed_call_recovery/workflow.py <<'PY'
from dataclasses import dataclass


@dataclass(frozen=True)
class LeadCapture:
    customer_name: str
    phone: str
    missed_at_minutes_ago: int
    repeat_calls_24h: int
    service_requested: str
    business_name: str
    consent_to_text: bool


def classify_lead(capture: LeadCapture) -> dict:
    score = 40
    reasons = []
    if capture.missed_at_minutes_ago <= 15:
        score += 30
        reasons.append("recent missed call")
    if capture.repeat_calls_24h >= 2:
        score += 30
        reasons.append("repeat caller")
    if any(word in capture.service_requested.lower() for word in ["emergency", "leak", "no heat", "water heater"]):
        score += 15
        reasons.append("urgent service request")
    score = min(score, 100)
    return {
        "priority": "hot" if score >= 85 else "standard",
        "score": score,
        "reason": ", ".join(reasons) or "baseline missed-call follow-up",
    }


def build_recovery_message(capture: LeadCapture) -> str:
    if not capture.consent_to_text:
        return ""
    return (
        f"Hi {capture.customer_name}, this is {capture.business_name}. "
        f"Sorry we missed your call about {capture.service_requested}. "
        "Reply here with a good time and our team will follow up."
    )
PY
printf 'Summary: project-run app step implemented missed-call recovery workflow\n'
printf 'Changed files: missed_call_recovery/workflow.py\n'
printf 'Input tokens: 37\n'
SH
}

write_step_fixture intake
write_step_fixture messaging

ao2_cmd factory project-plan-validate \
  --project-plan "$project_plan" \
  --project-root "$project_workspace" \
  --out "$project_plan_validation" \
  --json > /dev/null

cat > "$provider_prompt_dir/messaging-provider-prompt.sh" <<'SH'
printf 'Summary: intentionally leaving messaging implementation broken for resume smoke\n'
printf 'Changed files: none\n'
printf 'Input tokens: 11\n'
SH

ao2_cmd factory project-run \
  --project-spec "$project_spec" \
  --project-plan "$project_plan" \
  --run-id missed-call-recovery-project \
  --signing-key "$signing_key" \
  --signer-id factory-project-smoke \
  --out-dir "$failed_project_out" \
  --json > "$failed_project_json"

resume_state=$(node - "$failed_project_json" <<'NODE'
const fs = require('fs');
const [projectPath] = process.argv.slice(2);
const project = JSON.parse(fs.readFileSync(projectPath, 'utf8'));
function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}
assert(project.schema_version === 'ao2.factory-project-run.v1', 'unexpected failed project-run schema');
assert(project.status === 'rejected', 'failed project-run must be rejected');
assert(project.app_run_count === 1, 'failed project-run must preserve one accepted app-run');
assert(project.failed_step_count === 1, 'failed project-run must record one failed step');
assert(project.release_review.ready === false, 'failed project-run must not be release-ready');
assert(project.project_run_checklist.ao2_preserved_partial_evidence === true, 'partial evidence not preserved');
assert(project.project_run_checklist.release_review_package_ready === false, 'failed project-run must not package release review');
assert(project.artifacts.release_review_package === null, 'failed project-run must not emit release package');
assert(project.project_steps.some((step) => step.id === 'intake' && step.status === 'accepted'), 'accepted intake step missing');
assert(project.project_steps.some((step) => step.id === 'messaging' && step.status === 'rejected'), 'rejected messaging step missing');
assert(fs.existsSync(project.artifacts.factory_project_run_state), 'missing resumable project state');
assert(JSON.stringify(project).indexOf('Bearer ') === -1, 'bearer token leaked into failed project metadata');
process.stdout.write(project.artifacts.factory_project_run_state);
NODE
)

write_step_fixture messaging

ao2_cmd factory project-run \
  --project-spec "$project_spec" \
  --project-plan "$project_plan" \
  --resume-from "$resume_state" \
  --run-id missed-call-recovery-project \
  --signing-key "$signing_key" \
  --signer-id factory-project-smoke \
  --out-dir "$project_out" \
  --json > "$project_json"

ao2_cmd factory project-acceptance-review \
  --project-run "$project_json" \
  --signing-key "$signing_key" \
  --signer-id factory-project-acceptance-review-smoke \
  --out "$project_acceptance_review_json" \
  --json > /dev/null

node - "$project_json" "$summary_json" "$AO2_FACTORY_PROJECT_RUN_ROOT" <<'NODE'
const fs = require('fs');
const crypto = require('crypto');
const [projectPath, summaryPath, root] = process.argv.slice(2);
const project = JSON.parse(fs.readFileSync(projectPath, 'utf8'));

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function sha256File(path) {
  return crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex');
}

assert(project.schema_version === 'ao2.factory-project-run.v1', 'unexpected project-run schema');
assert(project.status === 'accepted', 'project-run must be accepted');
assert(project.app_run_count === 2, 'project-run must include two app-runs');
assert(project.failed_step_count === 0, 'resumed project-run must have zero failed steps');
assert(project.project_plan.schema_version === 'ao2.factory-project-plan.v1', 'project plan not embedded');
assert(project.project_plan.factory_replacement_boundary.factory_v3_role === 'parity_oracle_only', 'generated plan must keep factory-v3 as oracle');
assert(project.project_plan.factory_replacement_boundary.control_plane_approves_release === false, 'generated plan must not make control plane approve release');
assert(project.project_plan.acceptance_rubric.schema_version === 'ao2.factory-acceptance-rubric.v1', 'project plan missing AO2 acceptance rubric');
assert(project.project_plan.acceptance_rubric.signature.signature_status === 'signed', 'acceptance rubric must be signed');
assert(project.project_plan.acceptance_rubric.signature.signature_verified === true, 'acceptance rubric signature must verify');
assert(project.acceptance_rubric.accepted === true, 'project-run must validate signed acceptance rubric');
assert(project.artifacts.acceptance_rubric_sha256 === project.acceptance_rubric.sha256, 'project-run rubric digest mismatch');
const projectAcceptanceReview = JSON.parse(fs.readFileSync(`${root}/factory-project-acceptance-review.json`, 'utf8'));
assert(projectAcceptanceReview.schema_version === 'ao2.factory-project-acceptance-review.v1', 'project acceptance review schema mismatch');
assert(projectAcceptanceReview.status === 'accepted', 'project acceptance review must be accepted');
assert(projectAcceptanceReview.recommended_decision === 'accept', 'project acceptance review must recommend accept');
assert(projectAcceptanceReview.rubric_sha256 === project.artifacts.acceptance_rubric_sha256, 'project acceptance review rubric digest mismatch');
assert(projectAcceptanceReview.must_have_artifacts_present === true, 'project acceptance review missing artifacts');
assert(projectAcceptanceReview.thresholds_satisfied === true, 'project acceptance review thresholds not satisfied');
assert(projectAcceptanceReview.signature.signature_status === 'signed', 'project acceptance review must be signed');
assert(projectAcceptanceReview.signature.signature_verified === true, 'project acceptance review signature must verify');
assert(projectAcceptanceReview.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'project acceptance review release owner mismatch');
assert(projectAcceptanceReview.trust_boundary.control_plane_approves_release === false, 'project acceptance review must not approve release through control plane');
assert(projectAcceptanceReview.trust_boundary.mutates_ao_artifacts === false, 'project acceptance review must not mutate AO artifacts');
const validation = JSON.parse(fs.readFileSync(`${root}/factory-project-plan-validation.json`, 'utf8'));
assert(validation.schema_version === 'ao2.factory-project-plan-validation.v1', 'project plan validation schema mismatch');
assert(validation.status === 'accepted', 'project plan validation must accept generated plan');
assert(validation.checks.all_paths_within_project_root === true, 'project plan paths must stay inside project root');
assert(validation.checks.control_plane_remains_observer === true, 'control plane observer invariant must validate');
assert(validation.checks.signed_acceptance_rubric === true, 'project plan validation must verify signed rubric');
const queuedProjectStartSubmit = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-submit.json`, 'utf8'));
const queuedProjectStartRun = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-run-next.json`, 'utf8'));
const queuedProjectStartStatus = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-status.json`, 'utf8'));
const queuedProjectStartLatestStatus = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-latest-status.json`, 'utf8'));
const queuedProjectStartCompletionContract = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-completion-contract.json`, 'utf8'));
const queuedProjectStartCompletionContractConsumer = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-completion-contract-consumer.json`, 'utf8'));
const queuedProjectStartCompletionSummary = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-completion-summary.json`, 'utf8'));
const queuedReplacementPacket = JSON.parse(fs.readFileSync(`${root}/factory-replacement-packet.json`, 'utf8'));
const queuedReplacementPacketVerification = JSON.parse(fs.readFileSync(`${root}/factory-replacement-packet-verification.json`, 'utf8'));
const oneShotProjectStartComplete = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-complete.json`, 'utf8'));
const oneShotProjectStartCompleteStatus = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-complete-status.json`, 'utf8'));
const oneShotProjectStartNextAction = JSON.parse(fs.readFileSync(`${root}/factory-queue-project-start-next-action.json`, 'utf8'));
const queuedProjectStartClosure = JSON.parse(fs.readFileSync(queuedProjectStartRun.entry.project_start_closure_json, 'utf8'));
const queuedProjectStartClosureVerification = JSON.parse(fs.readFileSync(queuedProjectStartRun.entry.project_start_closure_verification, 'utf8'));
assert(queuedProjectStartSubmit.schema_version === 'ao2.factory-project-start-workbench-queue-submit.v1', 'queued project-start submit schema mismatch');
assert(queuedProjectStartSubmit.job_kind === 'factory_project_start', 'queued project-start job kind mismatch');
assert(queuedProjectStartRun.schema_version === 'ao2.factory-project-start-workbench-queue-run-next.v1', 'queued project-start run-next schema mismatch');
assert(queuedProjectStartRun.status === 'accepted', 'queued project-start must be accepted');
assert(queuedProjectStartRun.entry.status === 'accepted', 'queued project-start entry must be accepted');
assert(queuedProjectStartRun.entry.hermes_queue_handoff.schema_version === 'ao2.hermes-project-start-handoff.v1', 'queued project-start missing Hermes handoff');
assert(queuedProjectStartRun.entry.replacement_packet_status === 'packaged', 'queued project-start entry must package replacement packet');
assert(queuedProjectStartRun.entry.replacement_packet_verification_status === 'accepted', 'queued project-start entry must verify replacement packet');
assert(queuedProjectStartRun.entry.replacement_packet_verification_checks.checksums_verified === true, 'queued project-start replacement packet checksum verification mismatch');
assert(queuedProjectStartRun.entry.replacement_packet_verification_checks.trust_boundary_verified === true, 'queued project-start replacement packet trust boundary verification mismatch');
assert(queuedProjectStartRun.entry.replacement_packet_verification_checks.ao2_replacement_driver_verified === true, 'queued project-start replacement packet driver verification mismatch');
assert(queuedProjectStartRun.entry.replacement_packet_verification_checks.factory_v3_evaluator_closer_verified === true, 'queued project-start replacement packet factory-v3 role verification mismatch');
assert(queuedProjectStartRun.entry.hermes_queue_handoff.project_start_bundle === queuedProjectStartRun.entry.project_start_bundle, 'queued project-start handoff bundle path mismatch');
assert(fs.existsSync(queuedProjectStartRun.entry.project_start_bundle), `missing queued project-start bundle ${queuedProjectStartRun.entry.project_start_bundle}`);
assert(queuedProjectStartRun.entry.project_acceptance_review_status === 'accepted', 'queued project-start acceptance review failed');
assert(queuedProjectStartRun.entry.project_acceptance_review_recommended_decision === 'accept', 'queued project-start acceptance review must recommend accept');
assert(queuedProjectStartRun.entry.project_start_result.project_acceptance_review.schema_version === 'ao2.factory-project-acceptance-review.v1', 'queued project-start acceptance review schema mismatch');
assert(queuedProjectStartRun.entry.project_start_result.project_acceptance_review.signature.signature_status === 'signed', 'queued project-start acceptance review must be signed');
assert(fs.existsSync(queuedProjectStartRun.entry.project_acceptance_review), `missing queued project acceptance review ${queuedProjectStartRun.entry.project_acceptance_review}`);
assert(queuedProjectStartRun.entry.project_start_bundle_verification_status === 'accepted', 'queued project-start bundle verification failed');
assert(fs.existsSync(queuedProjectStartRun.entry.project_start_bundle_verification), `missing queued project-start bundle verification ${queuedProjectStartRun.entry.project_start_bundle_verification}`);
assert(queuedProjectStartRun.entry.project_start_bundle_verification_result.schema_version === 'ao2.factory-project-start-bundle-verification.v1', 'queued project-start bundle verification schema mismatch');
assert(queuedProjectStartRun.entry.project_start_bundle_verification_result.checks.project_acceptance_review_signature_verified === true, 'queued project-start bundle verification must verify review signature');
assert(queuedProjectStartRun.entry.project_start_bundle_verification_result.checks.review_rubric_digest_matches === true, 'queued project-start bundle verification must bind review to rubric');
assert(queuedProjectStartRun.entry.project_start_bundle_verification_result.checks.review_project_run_digest_matches === true, 'queued project-start bundle verification must bind review to project-run');
assert(queuedProjectStartRun.parity_checklist_progress.ao2_queue_executes_project_start_handoff_job === true, 'queued project-start parity checklist missing');
assert(queuedProjectStartRun.parity_checklist_progress.ao2_queue_verifies_project_start_handoff_bundle === true, 'queued project-start bundle verification checklist missing');
assert(queuedProjectStartRun.entry.project_start_operator_summary_status === 'accepted', 'queued project-start operator summary failed');
assert(fs.existsSync(queuedProjectStartRun.entry.project_start_operator_summary), `missing queued project-start operator summary ${queuedProjectStartRun.entry.project_start_operator_summary}`);
assert(fs.existsSync(queuedProjectStartRun.entry.project_start_operator_summary_markdown), `missing queued project-start operator summary markdown ${queuedProjectStartRun.entry.project_start_operator_summary_markdown}`);
assert(queuedProjectStartRun.entry.project_start_operator_summary_result.schema_version === 'ao2.factory-project-start-operator-summary.v1', 'queued project-start operator summary schema mismatch');
assert(queuedProjectStartRun.entry.project_start_operator_summary_result.checks.project_start_accepted === true, 'queued project-start operator summary must verify project-start acceptance');
assert(queuedProjectStartRun.entry.project_start_operator_summary_result.checks.bundle_verification_accepted === true, 'queued project-start operator summary must verify bundle verification acceptance');
assert(queuedProjectStartRun.entry.project_start_operator_summary_result.checks.bundle_digest_matches === true, 'queued project-start operator summary must bind bundle digest');
assert(queuedProjectStartRun.entry.project_start_operator_summary_result.trust_boundary.control_plane_approves_release === false, 'queued project-start operator summary must not approve release');
assert(queuedProjectStartRun.entry.project_start_operator_summary_result.trust_boundary.mutates_ao_artifacts === false, 'queued project-start operator summary must not mutate AO artifacts');
assert(queuedProjectStartRun.parity_checklist_progress.ao2_queue_summarizes_project_start_handoff === true, 'queued project-start operator summary checklist missing');
assert(queuedProjectStartRun.parity_checklist_progress.factory_v3_drives_workflow === false, 'queued project-start must not use factory-v3 driver');
assert(queuedProjectStartStatus.schema_version === 'ao2.factory-queue-status.v1', 'queued project-start queue-status schema mismatch');
assert(queuedProjectStartStatus.status === 'accepted', 'queued project-start queue-status must be accepted');
assert(queuedProjectStartStatus.entry.run_id === queuedProjectStartRun.entry.run_id, 'queued project-start queue-status run id mismatch');
assert(queuedProjectStartStatus.entry.project_start_operator_summary_sha256 === queuedProjectStartRun.entry.project_start_operator_summary_sha256, 'queued project-start queue-status summary digest mismatch');
assert(queuedProjectStartStatus.entry.project_start_operator_summary_status === 'accepted', 'queued project-start queue-status summary status mismatch');
assert(queuedProjectStartStatus.entry.project_start_bundle_verification_status === 'accepted', 'queued project-start queue-status verifier status mismatch');
assert(queuedProjectStartStatus.entry.project_start_closure_status === 'packaged', 'queued project-start queue-status closure status mismatch');
assert(queuedProjectStartStatus.entry.project_start_closure_verification_status === 'accepted', 'queued project-start queue-status closure verification status mismatch');
assert(queuedProjectStartStatus.trust_boundary.control_plane_approves_release === false, 'queued project-start queue-status must not approve release');
assert(queuedProjectStartStatus.trust_boundary.mutates_ao_artifacts === false, 'queued project-start queue-status must not mutate AO artifacts');
assert(queuedProjectStartStatus.parity_checklist_progress.ao2_queue_status_detail_is_read_only === true, 'queued project-start queue-status must be read-only detail');
assert(queuedProjectStartLatestStatus.schema_version === 'ao2.factory-queue-status.v1', 'queued project-start latest queue-status schema mismatch');
assert(queuedProjectStartLatestStatus.status === queuedProjectStartStatus.status, 'queued project-start latest queue-status status mismatch');
assert(queuedProjectStartLatestStatus.run_id === queuedProjectStartStatus.run_id, 'queued project-start latest queue-status run id mismatch');
assert(queuedProjectStartLatestStatus.entry.run_id === queuedProjectStartStatus.entry.run_id, 'queued project-start latest queue-status entry mismatch');
assert(queuedProjectStartLatestStatus.entry.project_start_operator_summary_sha256 === queuedProjectStartStatus.entry.project_start_operator_summary_sha256, 'queued project-start latest queue-status summary digest mismatch');
assert(queuedProjectStartLatestStatus.entry.project_start_closure_sha256 === queuedProjectStartStatus.entry.project_start_closure_sha256, 'queued project-start latest queue-status closure digest mismatch');
assert(queuedProjectStartLatestStatus.entry.project_start_closure_verification_sha256 === queuedProjectStartStatus.entry.project_start_closure_verification_sha256, 'queued project-start latest queue-status closure verification digest mismatch');
assert(queuedProjectStartLatestStatus.parity_checklist_progress.ao2_queue_status_detail_is_read_only === true, 'queued project-start latest queue-status must be read-only detail');
assert(queuedProjectStartLatestStatus.trust_boundary.control_plane_approves_release === false, 'queued project-start latest queue-status must not approve release');
assert(queuedProjectStartLatestStatus.trust_boundary.mutates_ao_artifacts === false, 'queued project-start latest queue-status must not mutate AO artifacts');
assert(queuedProjectStartClosure.schema_version === 'ao2.factory-project-start-closure.v1', 'queued project-start closure schema mismatch');
assert(queuedProjectStartClosure.status === 'packaged', 'queued project-start closure must package evidence');
assert(queuedProjectStartClosure.run_id === queuedProjectStartStatus.run_id, 'queued project-start closure run id mismatch');
assert(queuedProjectStartRun.entry.project_start_closure_status === 'packaged', 'queued project-start auto closure status mismatch');
assert(queuedProjectStartClosure.queue_status === 'accepted', 'queued project-start closure queue status mismatch');
assert(queuedProjectStartClosure.latest_queue_status === 'accepted', 'queued project-start closure latest status mismatch');
assert(queuedProjectStartClosure.latest_selector_matches_run_id_selector === true, 'queued project-start closure latest selector mismatch');
assert(queuedProjectStartClosure.manifest_entry === 'manifest.json', 'queued project-start closure manifest entry mismatch');
assert(queuedProjectStartClosure.checksum_entry === 'SHA256SUMS', 'queued project-start closure checksum entry mismatch');
assert(fs.existsSync(queuedProjectStartClosure.archive), `missing queued project-start closure ${queuedProjectStartClosure.archive}`);
assert(fs.existsSync(queuedProjectStartRun.entry.project_start_closure), `missing queued project-start closure bundle ${queuedProjectStartRun.entry.project_start_closure}`);
assert(fs.existsSync(queuedProjectStartRun.entry.project_start_closure_json), `missing queued project-start closure json ${queuedProjectStartRun.entry.project_start_closure_json}`);
assert(queuedProjectStartRun.entry.project_start_closure === queuedProjectStartClosure.archive, 'queued project-start auto closure bundle path mismatch');
assert(queuedProjectStartRun.entry.project_start_closure_sha256 === queuedProjectStartClosure.sha256, 'queued project-start auto closure digest mismatch');
assert(queuedProjectStartRun.entry.project_start_closure_sha256 === sha256File(queuedProjectStartRun.entry.project_start_closure), 'queued project-start auto closure digest does not match file');
assert(queuedProjectStartRun.entry.project_start_closure_json_sha256 === sha256File(queuedProjectStartRun.entry.project_start_closure_json), 'queued project-start auto closure json digest does not match file');
assert(queuedProjectStartClosure.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'queued project-start closure release owner mismatch');
assert(queuedProjectStartClosure.trust_boundary.control_plane_approves_release === false, 'queued project-start closure must not approve release');
assert(queuedProjectStartClosure.trust_boundary.mutates_ao_artifacts === false, 'queued project-start closure must not mutate AO artifacts');
assert(queuedProjectStartClosureVerification.schema_version === 'ao2.factory-project-start-closure-verification.v1', 'queued project-start closure verification schema mismatch');
assert(queuedProjectStartClosureVerification.status === 'accepted', 'queued project-start closure verification must accept');
assert(queuedProjectStartClosureVerification.run_id === queuedProjectStartClosure.run_id, 'queued project-start closure verification run id mismatch');
assert(queuedProjectStartRun.entry.project_start_closure_verification_status === 'accepted', 'queued project-start auto closure verification status mismatch');
assert(queuedProjectStartRun.entry.project_start_closure_verification_sha256 === sha256File(queuedProjectStartRun.entry.project_start_closure_verification), 'queued project-start auto closure verification digest does not match file');
assert(queuedProjectStartRun.entry.project_start_closure_verification_checks.checksums_verified === true, 'queued project-start auto closure verification must expose checksum check');
assert(queuedProjectStartRun.entry.project_start_closure_verification_checks.trust_boundary_verified === true, 'queued project-start auto closure verification must expose trust-boundary check');
assert(queuedProjectStartClosureVerification.checks.manifest_verified === true, 'queued project-start closure verification must verify manifest');
assert(queuedProjectStartClosureVerification.checks.checksums_verified === true, 'queued project-start closure verification must verify checksums');
assert(queuedProjectStartClosureVerification.checks.closure_verified === true, 'queued project-start closure verification must verify closure');
assert(queuedProjectStartClosureVerification.checks.latest_selector_matches_run_id_selector === true, 'queued project-start closure verification selector mismatch');
assert(queuedProjectStartClosureVerification.checks.trust_boundary_verified === true, 'queued project-start closure verification trust boundary failed');
assert(queuedProjectStartClosureVerification.checks.secret_scan_passed === true, 'queued project-start closure verification secret scan failed');
assert(queuedProjectStartClosureVerification.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'queued project-start closure verification release owner mismatch');
assert(queuedProjectStartClosureVerification.trust_boundary.control_plane_approves_release === false, 'queued project-start closure verification must not approve release');
assert(queuedProjectStartClosureVerification.trust_boundary.mutates_ao_artifacts === false, 'queued project-start closure verification must not mutate AO artifacts');
assert(queuedProjectStartRun.parity_checklist_progress.ao2_queue_packages_project_start_closure === true, 'queued project-start auto closure packaging checklist missing');
assert(queuedProjectStartRun.parity_checklist_progress.ao2_queue_verifies_project_start_closure === true, 'queued project-start auto closure verification checklist missing');
assert(queuedReplacementPacket.schema_version === 'ao2.factory-replacement-packet.v1', 'factory replacement packet schema mismatch');
assert(queuedReplacementPacket.status === 'packaged', 'factory replacement packet must package replacement evidence');
assert(queuedReplacementPacket.run_id === queuedProjectStartStatus.run_id, 'factory replacement packet run id mismatch');
assert(queuedReplacementPacket.checks.queue_status_accepted === true, 'factory replacement packet must accept queue status');
assert(queuedReplacementPacket.checks.latest_selector_matches_run_id_selector === true, 'factory replacement packet selector mismatch');
assert(queuedReplacementPacket.checks.closure_verification_accepted === true, 'factory replacement packet closure verification missing');
assert(queuedReplacementPacket.replacement_summary.ao2_replaces_factory_v3_workflow_driver === true, 'factory replacement packet must declare AO2 workflow-driver replacement');
assert(queuedReplacementPacket.replacement_summary.factory_v3_role === 'evaluator_closer_and_sampling_auditor', 'factory replacement packet factory-v3 role mismatch');
assert(queuedReplacementPacket.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'factory replacement packet release owner mismatch');
assert(queuedReplacementPacket.trust_boundary.control_plane_approves_release === false, 'factory replacement packet must not approve release through control plane');
assert(queuedReplacementPacket.trust_boundary.mutates_ao_artifacts === false, 'factory replacement packet must not mutate AO artifacts');
assert(fs.existsSync(queuedReplacementPacket.archive), `missing factory replacement packet ${queuedReplacementPacket.archive}`);
assert(queuedReplacementPacketVerification.schema_version === 'ao2.factory-replacement-packet-verification.v1', 'factory replacement packet verification schema mismatch');
assert(queuedReplacementPacketVerification.status === 'accepted', 'factory replacement packet verification must accept');
assert(queuedReplacementPacketVerification.run_id === queuedReplacementPacket.run_id, 'factory replacement packet verification run id mismatch');
assert(queuedReplacementPacketVerification.bundle_sha256 === queuedReplacementPacket.sha256, 'factory replacement packet verification digest mismatch');
assert(queuedReplacementPacketVerification.checks.checksums_verified === true, 'factory replacement packet verification must verify checksums');
assert(queuedReplacementPacketVerification.checks.manifest_verified === true, 'factory replacement packet verification must verify manifest');
assert(queuedReplacementPacketVerification.checks.packet_verified === true, 'factory replacement packet verification must verify packet');
assert(queuedReplacementPacketVerification.checks.trust_boundary_verified === true, 'factory replacement packet verification must verify trust boundary');
assert(queuedReplacementPacketVerification.checks.secret_scan_passed === true, 'factory replacement packet verification must pass secret scan');
assert(queuedReplacementPacketVerification.checks.ao2_replacement_driver_verified === true, 'factory replacement packet verification must verify AO2 replacement driver');
assert(queuedReplacementPacketVerification.checks.factory_v3_evaluator_closer_verified === true, 'factory replacement packet verification must verify factory-v3 evaluator closer role');
assert(queuedProjectStartCompletionContract.schema_version === 'ao2.factory-project-start-queue-completion-contract.v1', 'queued project-start completion contract schema mismatch');
assert(queuedProjectStartCompletionContract.status === 'accepted', 'queued project-start completion contract must be accepted');
assert(queuedProjectStartCompletionContract.run_id === queuedProjectStartRun.entry.run_id, 'queued project-start completion contract run id mismatch');
assert(queuedProjectStartCompletionContract.source_queue_status.schema_version === 'ao2.factory-queue-status.v1', 'queued project-start completion contract source queue-status schema mismatch');
assert(queuedProjectStartCompletionContract.artifacts.project_start_bundle === queuedProjectStartRun.entry.project_start_bundle, 'queued project-start completion contract bundle mismatch');
assert(queuedProjectStartCompletionContract.artifacts.project_start_closure === queuedProjectStartRun.entry.project_start_closure, 'queued project-start completion contract closure mismatch');
assert(queuedProjectStartCompletionContract.artifacts.project_start_closure_sha256 === queuedProjectStartRun.entry.project_start_closure_sha256, 'queued project-start completion contract closure digest mismatch');
assert(queuedProjectStartCompletionContract.artifacts.replacement_packet === queuedProjectStartRun.entry.replacement_packet, 'queued project-start completion contract replacement packet mismatch');
assert(queuedProjectStartCompletionContract.artifacts.replacement_packet_archive === queuedProjectStartRun.entry.replacement_packet_archive, 'queued project-start completion contract replacement packet archive mismatch');
assert(queuedProjectStartCompletionContract.artifacts.replacement_packet_verification === queuedProjectStartRun.entry.replacement_packet_verification, 'queued project-start completion contract replacement packet verification mismatch');
assert(queuedProjectStartCompletionContract.checks.project_start_closure_status === 'packaged', 'queued project-start completion contract closure status mismatch');
assert(queuedProjectStartCompletionContract.checks.project_start_closure_verification_status === 'accepted', 'queued project-start completion contract closure verification status mismatch');
assert(queuedProjectStartCompletionContract.checks.project_start_closure_verification_checksums_verified === true, 'queued project-start completion contract checksum check mismatch');
assert(queuedProjectStartCompletionContract.checks.replacement_packet_status === 'packaged', 'queued project-start completion contract replacement packet status mismatch');
assert(queuedProjectStartCompletionContract.checks.replacement_packet_verification_status === 'accepted', 'queued project-start completion contract replacement packet verification status mismatch');
assert(queuedProjectStartCompletionContract.checks.replacement_packet_verification_checksums_verified === true, 'queued project-start completion contract replacement checksum check mismatch');
assert(queuedProjectStartCompletionContract.checks.replacement_packet_verification_trust_boundary_verified === true, 'queued project-start completion contract replacement trust-boundary check mismatch');
assert(queuedProjectStartCompletionContract.checks.replacement_packet_verification_ao2_replacement_driver_verified === true, 'queued project-start completion contract replacement driver check mismatch');
assert(queuedProjectStartCompletionContract.checks.replacement_packet_verification_factory_v3_evaluator_closer_verified === true, 'queued project-start completion contract factory-v3 role check mismatch');
assert(queuedProjectStartCompletionContract.hermes_contract.front_end_reads_single_completion_record === true, 'queued project-start completion contract must be one-read Hermes contract');
assert(queuedProjectStartCompletionContract.hermes_contract.requires_manual_closure_commands === false, 'queued project-start completion contract must not require manual closure commands');
assert(queuedProjectStartCompletionContract.hermes_contract.requires_manual_packet_commands === false, 'queued project-start completion contract must not require manual replacement packet commands');
assert(queuedProjectStartCompletionContract.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'queued project-start completion contract release owner mismatch');
assert(queuedProjectStartCompletionContract.trust_boundary.control_plane_approves_release === false, 'queued project-start completion contract must not approve release');
assert(queuedProjectStartCompletionContract.trust_boundary.mutates_ao_artifacts === false, 'queued project-start completion contract must not mutate AO artifacts');
assert(queuedProjectStartCompletionContractConsumer.schema_version === 'ao2.factory-project-start-queue-completion-contract-consumption.v1', 'queued project-start completion contract consumer schema mismatch');
assert(queuedProjectStartCompletionContractConsumer.status === 'accepted', 'queued project-start completion contract consumer must accept');
assert(queuedProjectStartCompletionContractConsumer.ready_for_operator_review === true, 'queued project-start completion contract consumer must mark ready');
assert(queuedProjectStartCompletionContractConsumer.run_id === queuedProjectStartCompletionContract.run_id, 'queued project-start completion contract consumer run id mismatch');
assert(queuedProjectStartCompletionContractConsumer.source_contract_schema === queuedProjectStartCompletionContract.schema_version, 'queued project-start completion contract consumer source schema mismatch');
assert(queuedProjectStartCompletionContractConsumer.checks.replacement_packet_verification_status === 'accepted', 'queued project-start completion contract consumer replacement verification status mismatch');
assert(queuedProjectStartCompletionContractConsumer.checks.replacement_packet_verification_ao2_replacement_driver_verified === true, 'queued project-start completion contract consumer replacement driver check mismatch');
assert(queuedProjectStartCompletionContractConsumer.hermes_contract.consumed_contract_only === true, 'queued project-start completion contract consumer must consume contract only');
assert(queuedProjectStartCompletionContractConsumer.hermes_contract.front_end_reads_single_completion_record === true, 'queued project-start completion contract consumer must preserve one-read contract');
assert(queuedProjectStartCompletionContractConsumer.hermes_contract.requires_manual_closure_commands === false, 'queued project-start completion contract consumer must not require manual closure commands');
assert(queuedProjectStartCompletionContractConsumer.hermes_contract.requires_manual_packet_commands === false, 'queued project-start completion contract consumer must not require manual replacement packet commands');
assert(queuedProjectStartCompletionContractConsumer.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'queued project-start completion contract consumer release owner mismatch');
assert(queuedProjectStartCompletionContractConsumer.trust_boundary.control_plane_approves_release === false, 'queued project-start completion contract consumer must not approve release');
assert(queuedProjectStartCompletionContractConsumer.trust_boundary.mutates_ao_artifacts === false, 'queued project-start completion contract consumer must not mutate AO artifacts');
assert(queuedProjectStartCompletionSummary.schema_version === 'ao2.factory-project-start-completion-summary.v1', 'queued project-start completion summary schema mismatch');
assert(queuedProjectStartCompletionSummary.status === 'accepted', 'queued project-start completion summary status mismatch');
assert(queuedProjectStartCompletionSummary.replacement_packet_handoff.status === 'ready_for_operator_review', 'queued project-start completion summary replacement handoff must be ready');
assert(queuedProjectStartCompletionSummary.replacement_packet_handoff.requires_manual_packet_verify_command === false, 'queued project-start completion summary must not require manual packet verification');
assert(queuedProjectStartCompletionSummary.replacement_packet_handoff.verification === queuedProjectStartRun.entry.replacement_packet_verification, 'queued project-start completion summary replacement verification mismatch');
assert(queuedProjectStartCompletionSummary.replacement_packet_handoff.verification_sha256 === queuedProjectStartRun.entry.replacement_packet_verification_sha256, 'queued project-start completion summary replacement verification digest mismatch');
assert(queuedProjectStartCompletionSummary.replacement_packet_handoff.ao2_replaces_factory_v3_workflow_driver === true, 'queued project-start completion summary replacement driver mismatch');
assert(queuedProjectStartCompletionSummary.hermes_memory.next_recommended_action === 'record_replacement_packet_completion_summary', 'queued project-start completion summary next action mismatch');
assert(queuedProjectStartCompletionSummary.side_effects.would_execute_queue === false, 'queued project-start completion summary must not execute queue');
assert(oneShotProjectStartComplete.schema_version === 'ao2.factory-project-start-queue-complete.v1', 'one-shot project-start schema mismatch');
assert(oneShotProjectStartComplete.status === 'accepted', 'one-shot project-start must accept');
assert(oneShotProjectStartComplete.ready_for_operator_review === true, 'one-shot project-start must be ready for operator review');
assert(oneShotProjectStartComplete.run_id === 'missed-call-recovery-project-one-shot', 'one-shot project-start run id mismatch');
assert(oneShotProjectStartComplete.queue_run_next_status === 'accepted', 'one-shot project-start run-next status mismatch');
assert(oneShotProjectStartComplete.completion_contract_status === 'accepted', 'one-shot project-start completion contract status mismatch');
assert(oneShotProjectStartComplete.completion_contract_consumer_status === 'accepted', 'one-shot project-start consumer status mismatch');
assert(oneShotProjectStartComplete.hermes_contract.backend_used_bounded_ao2_queue === true, 'one-shot project-start must use bounded AO2 queue');
assert(oneShotProjectStartComplete.hermes_contract.requires_manual_command_sequence === false, 'one-shot project-start must not require manual command sequence');
assert(oneShotProjectStartComplete.completion_contract_consumer.schema_version === 'ao2.factory-project-start-queue-completion-contract-consumption.v1', 'one-shot project-start consumer schema mismatch');
assert(oneShotProjectStartComplete.completion_contract_consumer.hermes_contract.consumed_contract_only === true, 'one-shot project-start consumer must consume contract only');
assert(oneShotProjectStartComplete.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'one-shot project-start release owner mismatch');
assert(oneShotProjectStartComplete.trust_boundary.control_plane_approves_release === false, 'one-shot project-start must not approve release through control plane');
assert(oneShotProjectStartComplete.trust_boundary.mutates_ao_artifacts === false, 'one-shot project-start must not mutate AO artifacts');
for (const artifact of ['queue_submit', 'queue_run_next', 'completion_contract', 'completion_contract_consumer', 'project_start_bundle', 'project_start_closure_verification']) {
  assert(fs.existsSync(oneShotProjectStartComplete.artifacts[artifact]), `missing one-shot artifact ${artifact}: ${oneShotProjectStartComplete.artifacts[artifact]}`);
}
assert(oneShotProjectStartCompleteStatus.schema_version === 'ao2.factory-project-start-queue-complete-status.v1', 'one-shot project-start status probe schema mismatch');
assert(oneShotProjectStartCompleteStatus.status === 'accepted', 'one-shot project-start status probe must accept');
assert(oneShotProjectStartCompleteStatus.completion_record_state === 'complete', 'one-shot project-start status probe must see complete record');
assert(oneShotProjectStartCompleteStatus.run_id === oneShotProjectStartComplete.run_id, 'one-shot project-start status probe run id mismatch');
assert(oneShotProjectStartCompleteStatus.read_only === true, 'one-shot project-start status probe must be read-only');
assert(oneShotProjectStartCompleteStatus.would_execute_queue === false, 'one-shot project-start status probe must not execute queue');
assert(oneShotProjectStartCompleteStatus.would_rebuild_wrappers === false, 'one-shot project-start status probe must not rebuild wrappers');
assert(oneShotProjectStartCompleteStatus.ready_for_operator_review === true, 'one-shot project-start status probe must mark ready');
assert(oneShotProjectStartCompleteStatus.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'one-shot project-start status probe release owner mismatch');
assert(oneShotProjectStartCompleteStatus.trust_boundary.control_plane_approves_release === false, 'one-shot project-start status probe must not approve release');
assert(oneShotProjectStartCompleteStatus.trust_boundary.mutates_ao_artifacts === false, 'one-shot project-start status probe must not mutate AO artifacts');
assert(oneShotProjectStartNextAction.schema_version === 'ao2.factory-project-start-next-action.v1', 'one-shot project-start next-action schema mismatch');
assert(oneShotProjectStartNextAction.status === 'ready', 'one-shot project-start next-action must be ready');
assert(oneShotProjectStartNextAction.next_action === 'publish_operator_record', 'one-shot project-start next-action must publish operator record');
assert(oneShotProjectStartNextAction.read_only === true, 'one-shot project-start next-action must be read-only');
assert(oneShotProjectStartNextAction.would_execute_queue === false, 'one-shot project-start next-action must not execute queue');
assert(oneShotProjectStartNextAction.would_submit_queue_entry === false, 'one-shot project-start next-action must not submit queue entry');
assert(oneShotProjectStartNextAction.would_rebuild_wrappers === false, 'one-shot project-start next-action must not rebuild wrappers');
assert(oneShotProjectStartNextAction.status_probe.completion_record_state === 'complete', 'one-shot project-start next-action status probe mismatch');
assert(oneShotProjectStartNextAction.trust_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'one-shot project-start next-action release owner mismatch');
assert(oneShotProjectStartNextAction.trust_boundary.control_plane_approves_release === false, 'one-shot project-start next-action must not approve release');
assert(oneShotProjectStartNextAction.trust_boundary.mutates_ao_artifacts === false, 'one-shot project-start next-action must not mutate AO artifacts');
const projectStart = JSON.parse(fs.readFileSync(`${root}/factory-project-start.json`, 'utf8'));
assert(projectStart.schema_version === 'ao2.factory-project-start.v1', 'project-start schema mismatch');
assert(projectStart.status === 'accepted', 'project-start must be accepted');
assert(projectStart.checks.project_plan_validation_status === 'accepted', 'project-start validation chain failed');
assert(projectStart.checks.project_run_status === 'accepted', 'project-start project-run chain failed');
assert(projectStart.checks.release_review_package_ready === true, 'project-start release package not ready');
assert(projectStart.checks.project_acceptance_review_status === 'accepted', 'project-start acceptance review failed');
assert(projectStart.checks.project_acceptance_review_recommended_decision === 'accept', 'project-start acceptance review must recommend accept');
assert(projectStart.project_acceptance_review.schema_version === 'ao2.factory-project-acceptance-review.v1', 'project-start acceptance review schema mismatch');
assert(projectStart.project_acceptance_review.status === 'accepted', 'project-start embedded acceptance review must be accepted');
assert(projectStart.project_acceptance_review.signature.signature_status === 'signed', 'project-start acceptance review must be signed');
assert(projectStart.project_acceptance_review.signature.signature_verified === true, 'project-start acceptance review signature must verify');
assert(fs.existsSync(projectStart.artifacts.project_acceptance_review), `missing project-start acceptance review ${projectStart.artifacts.project_acceptance_review}`);
assert(projectStart.factory_replacement_boundary.factory_v3_drives_workflow === false, 'project-start must not use factory-v3 driver');
assert(projectStart.factory_replacement_boundary.control_plane_approves_release === false, 'project-start must not make control plane approve release');
assert(projectStart.factory_replacement_boundary.mutates_ao_artifacts === false, 'project-start must not mutate AO artifacts');
const projectStartBundle = JSON.parse(fs.readFileSync(`${root}/factory-project-start-bundle.json`, 'utf8'));
assert(projectStartBundle.schema_version === 'ao2.factory-project-start-bundle.v1', 'project-start bundle schema mismatch');
assert(projectStartBundle.status === 'bundled', 'project-start bundle must be bundled');
assert(fs.existsSync(projectStartBundle.archive), `missing project-start bundle ${projectStartBundle.archive}`);
assert(projectStartBundle.artifacts.some((artifact) => artifact.label === 'project-acceptance-review'), 'project-start bundle missing project acceptance review');
assert(projectStartBundle.artifacts.some((artifact) => artifact.label === 'project-acceptance-review-signed-payload'), 'project-start bundle missing project acceptance review signed payload');
assert(projectStartBundle.artifacts.some((artifact) => artifact.label === 'acceptance-rubric-signed-payload'), 'project-start bundle missing acceptance rubric signed payload');
assert(projectStartBundle.trust_boundary.control_plane_approves_release === false, 'project-start bundle must not approve release');
assert(projectStartBundle.trust_boundary.mutates_ao_artifacts === false, 'project-start bundle must not mutate AO artifacts');
const projectStartBundleVerification = JSON.parse(fs.readFileSync(`${root}/factory-project-start-bundle-verification.json`, 'utf8'));
assert(projectStartBundleVerification.schema_version === 'ao2.factory-project-start-bundle-verification.v1', 'project-start bundle verification schema mismatch');
assert(projectStartBundleVerification.status === 'accepted', 'project-start bundle verification must accept');
assert(projectStartBundleVerification.checks.project_start_verified === true, 'project-start bundle verification must verify project-start');
assert(projectStartBundleVerification.checks.project_run_verified === true, 'project-start bundle verification must verify project-run');
assert(projectStartBundleVerification.checks.acceptance_rubric_signature_verified === true, 'project-start bundle verification must verify rubric signature');
assert(projectStartBundleVerification.checks.project_acceptance_review_signature_verified === true, 'project-start bundle verification must verify review signature');
assert(projectStartBundleVerification.checks.review_rubric_digest_matches === true, 'project-start bundle verification must bind review to rubric');
assert(projectStartBundleVerification.checks.review_project_run_digest_matches === true, 'project-start bundle verification must bind review to project-run');
const projectStartOperatorSummary = JSON.parse(fs.readFileSync(`${root}/factory-project-start-operator-summary.json`, 'utf8'));
assert(projectStartOperatorSummary.schema_version === 'ao2.factory-project-start-operator-summary.v1', 'project-start operator summary schema mismatch');
assert(projectStartOperatorSummary.status === 'accepted', 'project-start operator summary must accept');
assert(projectStartOperatorSummary.bundle_verification_status === 'accepted', 'project-start operator summary must bind accepted bundle verification');
assert(projectStartOperatorSummary.artifacts.project_start_bundle_verification.status === 'accepted', 'project-start operator summary missing bundle verification status');
assert(projectStartOperatorSummary.artifacts.project_start_bundle.sha256 === projectStartOperatorSummary.artifacts.project_start_bundle.expected_sha256, 'project-start operator summary bundle digest mismatch');
assert(projectStartOperatorSummary.trust_boundary.control_plane_approves_release === false, 'project-start operator summary must not approve release');
assert(projectStartOperatorSummary.trust_boundary.mutates_ao_artifacts === false, 'project-start operator summary must not mutate AO artifacts');
assert(fs.existsSync(`${root}/factory-project-start-operator-summary.md`), 'missing project-start operator summary markdown');
assert(projectStart.project_start_bundle.schema_version === 'ao2.factory-project-start-bundle.v1', 'project-start output missing bundle summary');
assert(projectStart.hermes_queue_handoff.schema_version === 'ao2.hermes-project-start-handoff.v1', 'project-start output missing Hermes queue handoff');
assert(projectStart.hermes_queue_handoff.status === 'ready', 'Hermes queue handoff must be ready');
assert(projectStart.hermes_queue_handoff.project_start_bundle === projectStartBundle.archive, 'Hermes queue handoff must point to bundle archive');
assert(projectStart.hermes_queue_handoff.project_start_bundle_sha256 === projectStartBundle.sha256, 'Hermes queue handoff bundle digest mismatch');
assert(projectStart.hermes_queue_handoff.handoff_entry === 'handoff.json', 'Hermes queue handoff entry mismatch');
assert(projectStart.hermes_queue_handoff.manifest_entry === 'manifest.json', 'Hermes queue manifest entry mismatch');
assert(projectStart.hermes_queue_handoff.checksum_entry === 'SHA256SUMS', 'Hermes queue checksum entry mismatch');
assert(projectStart.hermes_queue_handoff.factory_v3_role === 'parity_oracle_only', 'Hermes queue factory role mismatch');
assert(projectStart.hermes_queue_handoff.control_plane_role === 'read_only_observer_after_signed_evidence', 'Hermes queue control-plane role mismatch');
assert(projectStart.hermes_queue_handoff.release_acceptance_owner === 'factory-v3 evaluator-closer', 'Hermes queue release owner mismatch');
assert(project.project_run_checklist.ao2_ingested_project_spec === true, 'project spec not ingested');
assert(project.project_run_checklist.ao2_dispatched_project_plan === true, 'project plan not dispatched');
assert(project.project_run_checklist.ao2_reused_resume_state === true, 'resume state not reused');
assert(project.project_run_checklist.ao2_preserved_partial_evidence === false, 'accepted resume should not be partial');
assert(project.project_run_checklist.ao2_collected_app_run_bundles === true, 'app-run bundles not collected');
assert(project.project_run_checklist.release_review_package_ready === true, 'release-review package not ready');
assert(project.factory_replacement_boundary.factory_v3_drives_workflow === false, 'factory-v3 must not drive workflow');
assert(project.factory_replacement_boundary.factory_v3_role === 'parity_oracle_only', 'factory-v3 role mismatch');
assert(project.factory_replacement_boundary.control_plane_role === 'read_only_observer_after_signed_evidence', 'control-plane role mismatch');
assert(project.factory_replacement_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'release owner mismatch');
assert(project.factory_replacement_boundary.control_plane_approves_release === false, 'control plane must not approve release');
assert(project.factory_replacement_boundary.mutates_ao_artifacts === false, 'must not mutate AO artifacts');
assert(project.release_review_package.status === 'packaged', 'release package must be packaged');
assert(fs.existsSync(project.artifacts.factory_project_run), `missing project run: ${project.artifacts.factory_project_run}`);
assert(fs.existsSync(project.artifacts.release_review_package), `missing release package: ${project.artifacts.release_review_package}`);
for (const item of project.app_runs) {
  assert(fs.existsSync(item.app_run), `missing dispatched app-run ${item.app_run}`);
  assert(fs.existsSync(item.bundle), `missing dispatched app-run bundle ${item.bundle}`);
}
assert(project.project_steps.some((step) => step.id === 'intake' && step.reused_from_resume === true), 'intake step should be reused from resume state');
assert(project.project_steps.every((step) => step.status === 'accepted'), 'all resumed project steps must be accepted');
assert(JSON.stringify(project).indexOf('Bearer ') === -1, 'bearer token leaked into project metadata');

const summary = {
  schema_version: 'ao2.factory-project-run-smoke.v1',
  status: 'passed',
  root,
  product_fixture: 'missed-call-recovery',
  product_domain: 'missed-call revenue recovery',
  run_id: project.run_id,
  run_status: project.status,
  factory_project_schema: project.schema_version,
  app_run_count: project.app_run_count,
  project_plan_dispatched: project.project_run_checklist.ao2_dispatched_project_plan,
  project_plan_generated_by_ao2: true,
  project_plan_generated: `${root}/factory-project-plan.json`,
  acceptance_rubric: project.artifacts.acceptance_rubric,
  acceptance_rubric_sha256: project.artifacts.acceptance_rubric_sha256,
  acceptance_rubric_status: project.acceptance_rubric.accepted ? 'accepted' : 'rejected',
  project_acceptance_review: `${root}/factory-project-acceptance-review.json`,
  project_acceptance_review_status: projectAcceptanceReview.status,
  project_acceptance_review_recommended_decision: projectAcceptanceReview.recommended_decision,
  project_acceptance_review_signature_status: projectAcceptanceReview.signature.signature_status,
  project_plan_validation: `${root}/factory-project-plan-validation.json`,
  project_plan_validation_status: validation.status,
  queued_project_start_submit: `${root}/factory-queue-project-start-submit.json`,
  queued_project_start_run_next: `${root}/factory-queue-project-start-run-next.json`,
  queued_project_start_queue_status_detail: `${root}/factory-queue-project-start-status.json`,
  queued_project_start_queue_status_schema: queuedProjectStartStatus.schema_version,
  queued_project_start_queue_status: queuedProjectStartStatus.status,
  queued_project_start_queue_status_read_only: queuedProjectStartStatus.parity_checklist_progress.ao2_queue_status_detail_is_read_only,
  queued_project_start_latest_queue_status_detail: `${root}/factory-queue-project-start-latest-status.json`,
  queued_project_start_latest_queue_status_schema: queuedProjectStartLatestStatus.schema_version,
  queued_project_start_latest_queue_status: queuedProjectStartLatestStatus.status,
  queued_project_start_latest_queue_status_run_id: queuedProjectStartLatestStatus.run_id,
  queued_project_start_latest_queue_status_matches_run_id_selector: queuedProjectStartLatestStatus.entry.run_id === queuedProjectStartStatus.entry.run_id && queuedProjectStartLatestStatus.entry.project_start_operator_summary_sha256 === queuedProjectStartStatus.entry.project_start_operator_summary_sha256,
  queued_project_start_latest_queue_status_read_only: queuedProjectStartLatestStatus.parity_checklist_progress.ao2_queue_status_detail_is_read_only,
  queued_project_start_completion_contract: `${root}/factory-queue-project-start-completion-contract.json`,
  queued_project_start_completion_contract_schema: queuedProjectStartCompletionContract.schema_version,
  queued_project_start_completion_contract_status: queuedProjectStartCompletionContract.status,
  queued_project_start_completion_contract_one_read: queuedProjectStartCompletionContract.hermes_contract.front_end_reads_single_completion_record,
  queued_project_start_completion_contract_requires_manual_closure_commands: queuedProjectStartCompletionContract.hermes_contract.requires_manual_closure_commands,
  queued_project_start_completion_contract_requires_manual_packet_commands: queuedProjectStartCompletionContract.hermes_contract.requires_manual_packet_commands,
  queued_project_start_completion_contract_consumer: `${root}/factory-queue-project-start-completion-contract-consumer.json`,
  queued_project_start_completion_contract_consumer_schema: queuedProjectStartCompletionContractConsumer.schema_version,
  queued_project_start_completion_contract_consumer_status: queuedProjectStartCompletionContractConsumer.status,
  queued_project_start_completion_contract_consumer_ready: queuedProjectStartCompletionContractConsumer.ready_for_operator_review,
  queued_project_start_completion_contract_consumer_contract_only: queuedProjectStartCompletionContractConsumer.hermes_contract.consumed_contract_only,
  queued_project_start_completion_summary: `${root}/factory-queue-project-start-completion-summary.json`,
  queued_project_start_completion_summary_schema: queuedProjectStartCompletionSummary.schema_version,
  queued_project_start_completion_summary_status: queuedProjectStartCompletionSummary.status,
  queued_project_start_completion_summary_handoff_status: queuedProjectStartCompletionSummary.replacement_packet_handoff.status,
  queued_project_start_completion_summary_next_action: queuedProjectStartCompletionSummary.replacement_packet_handoff.next_recommended_action,
  queued_project_start_completion_summary_requires_manual_packet_verify_command: queuedProjectStartCompletionSummary.replacement_packet_handoff.requires_manual_packet_verify_command,
  one_shot_project_start_complete: `${root}/factory-queue-project-start-complete.json`,
  one_shot_project_start_schema: oneShotProjectStartComplete.schema_version,
  one_shot_project_start_status: oneShotProjectStartComplete.status,
  one_shot_project_start_ready_for_operator_review: oneShotProjectStartComplete.ready_for_operator_review,
  one_shot_project_start_contract_status: oneShotProjectStartComplete.completion_contract_status,
  one_shot_project_start_contract_consumer_status: oneShotProjectStartComplete.completion_contract_consumer_status,
  one_shot_project_start_contract_only: oneShotProjectStartComplete.completion_contract_consumer.hermes_contract.consumed_contract_only,
  one_shot_project_start_manual_sequence_required: oneShotProjectStartComplete.hermes_contract.requires_manual_command_sequence,
  one_shot_project_start_complete_status: `${root}/factory-queue-project-start-complete-status.json`,
  one_shot_project_start_probe_schema: oneShotProjectStartCompleteStatus.schema_version,
  one_shot_project_start_probe_status: oneShotProjectStartCompleteStatus.status,
  one_shot_project_start_probe_record_state: oneShotProjectStartCompleteStatus.completion_record_state,
  one_shot_project_start_probe_read_only: oneShotProjectStartCompleteStatus.read_only,
  one_shot_project_start_probe_would_execute_queue: oneShotProjectStartCompleteStatus.would_execute_queue,
  one_shot_project_start_probe_would_rebuild_wrappers: oneShotProjectStartCompleteStatus.would_rebuild_wrappers,
  one_shot_project_start_next_action: `${root}/factory-queue-project-start-next-action.json`,
  one_shot_project_start_next_action_schema: oneShotProjectStartNextAction.schema_version,
  one_shot_project_start_next_action_status: oneShotProjectStartNextAction.status,
  one_shot_project_start_next_action_value: oneShotProjectStartNextAction.next_action,
  one_shot_project_start_next_action_read_only: oneShotProjectStartNextAction.read_only,
  one_shot_project_start_next_action_probe_state: oneShotProjectStartNextAction.status_probe.completion_record_state,
  queued_project_start_closure: queuedProjectStartRun.entry.project_start_closure_json,
  queued_project_start_closure_schema: queuedProjectStartClosure.schema_version,
  queued_project_start_closure_status: queuedProjectStartClosure.status,
  queued_project_start_closure_bundle: queuedProjectStartClosure.archive,
  queued_project_start_closure_sha256: queuedProjectStartClosure.sha256,
  queued_project_start_closure_latest_selector_matches_run_id_selector: queuedProjectStartClosure.latest_selector_matches_run_id_selector,
  queued_project_start_closure_verification: queuedProjectStartRun.entry.project_start_closure_verification,
  queued_project_start_closure_verification_schema: queuedProjectStartClosureVerification.schema_version,
  queued_project_start_closure_verification_status: queuedProjectStartClosureVerification.status,
  queued_project_start_closure_verification_checksums_verified: queuedProjectStartClosureVerification.checks.checksums_verified,
  queued_project_start_closure_verification_trust_boundary_verified: queuedProjectStartClosureVerification.checks.trust_boundary_verified,
  queued_auto_replacement_packet: queuedProjectStartRun.entry.replacement_packet,
  queued_auto_replacement_packet_archive: queuedProjectStartRun.entry.replacement_packet_archive,
  queued_auto_replacement_packet_status: queuedProjectStartRun.entry.replacement_packet_status,
  queued_auto_replacement_packet_verification: queuedProjectStartRun.entry.replacement_packet_verification,
  queued_auto_replacement_packet_verification_status: queuedProjectStartRun.entry.replacement_packet_verification_status,
  queued_auto_replacement_packet_verification_checksums_verified: queuedProjectStartRun.entry.replacement_packet_verification_checks.checksums_verified,
  queued_auto_replacement_packet_verification_trust_boundary_verified: queuedProjectStartRun.entry.replacement_packet_verification_checks.trust_boundary_verified,
  queued_replacement_packet: `${root}/factory-replacement-packet.json`,
  queued_replacement_packet_archive: queuedReplacementPacket.archive,
  queued_replacement_packet_schema: queuedReplacementPacket.schema_version,
  queued_replacement_packet_status: queuedReplacementPacket.status,
  queued_replacement_packet_sha256: queuedReplacementPacket.sha256,
  queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver: queuedReplacementPacket.replacement_summary.ao2_replaces_factory_v3_workflow_driver,
  queued_replacement_packet_factory_v3_role: queuedReplacementPacket.replacement_summary.factory_v3_role,
  queued_replacement_packet_verification: `${root}/factory-replacement-packet-verification.json`,
  queued_replacement_packet_verification_schema: queuedReplacementPacketVerification.schema_version,
  queued_replacement_packet_verification_status: queuedReplacementPacketVerification.status,
  queued_replacement_packet_verification_checksums_verified: queuedReplacementPacketVerification.checks.checksums_verified,
  queued_replacement_packet_verification_trust_boundary_verified: queuedReplacementPacketVerification.checks.trust_boundary_verified,
  queued_replacement_packet_verification_ao2_replacement_driver_verified: queuedReplacementPacketVerification.checks.ao2_replacement_driver_verified,
  queued_replacement_packet_verification_factory_v3_evaluator_closer_verified: queuedReplacementPacketVerification.checks.factory_v3_evaluator_closer_verified,
  queued_project_start_status: queuedProjectStartRun.status,
  queued_project_start_bundle: queuedProjectStartRun.entry.project_start_bundle,
  queued_project_start_bundle_sha256: queuedProjectStartRun.entry.project_start_bundle_sha256,
  queued_project_start_handoff_schema: queuedProjectStartRun.entry.hermes_queue_handoff.schema_version,
  queued_project_acceptance_review_status: queuedProjectStartRun.entry.project_acceptance_review_status,
  queued_project_acceptance_review_recommended_decision: queuedProjectStartRun.entry.project_acceptance_review_recommended_decision,
  queued_project_start_bundle_verification: queuedProjectStartRun.entry.project_start_bundle_verification,
  queued_project_start_bundle_verification_status: queuedProjectStartRun.entry.project_start_bundle_verification_status,
  queued_project_start_bundle_review_signature_verified: queuedProjectStartRun.entry.project_start_bundle_verification_checks.project_acceptance_review_signature_verified,
  queued_project_start_operator_summary: queuedProjectStartRun.entry.project_start_operator_summary,
  queued_project_start_operator_summary_markdown: queuedProjectStartRun.entry.project_start_operator_summary_markdown,
  queued_project_start_operator_summary_status: queuedProjectStartRun.entry.project_start_operator_summary_status,
  queued_project_start_operator_summary_bundle_digest_matches: queuedProjectStartRun.entry.project_start_operator_summary_checks.bundle_digest_matches,
  project_start: `${root}/factory-project-start.json`,
  project_start_status: projectStart.status,
  project_start_release_review_package_ready: projectStart.checks.release_review_package_ready,
  project_start_acceptance_review_status: projectStart.checks.project_acceptance_review_status,
  project_start_acceptance_review_recommended_decision: projectStart.checks.project_acceptance_review_recommended_decision,
  project_start_acceptance_review_signature_status: projectStart.project_acceptance_review.signature.signature_status,
  project_start_bundle: projectStartBundle.archive,
  project_start_bundle_status: projectStartBundle.status,
  project_start_bundle_verification: `${root}/factory-project-start-bundle-verification.json`,
  project_start_bundle_verification_status: projectStartBundleVerification.status,
  project_start_bundle_review_signature_verified: projectStartBundleVerification.checks.project_acceptance_review_signature_verified,
  project_start_operator_summary: `${root}/factory-project-start-operator-summary.json`,
  project_start_operator_summary_markdown: `${root}/factory-project-start-operator-summary.md`,
  project_start_operator_summary_status: projectStartOperatorSummary.status,
  project_resume_state_reused: project.project_run_checklist.ao2_reused_resume_state,
  partial_evidence_preserved_before_resume: true,
  factory_v3_drives_workflow: false,
  factory_v3_role: 'parity_oracle_only',
  control_plane_role: project.factory_replacement_boundary.control_plane_role,
  release_acceptance_owner: project.factory_replacement_boundary.release_acceptance_owner,
  control_plane_approves_release: project.factory_replacement_boundary.control_plane_approves_release,
  mutates_ao_artifacts: project.factory_replacement_boundary.mutates_ao_artifacts,
  release_review_package_ready: project.project_run_checklist.release_review_package_ready,
  release_review_package: project.artifacts.release_review_package,
  release_review_package_status: project.release_review_package.status,
  artifacts: project.artifacts
};
fs.writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
NODE

rm -f "$signing_key"

printf "factory_project_run_root=%s\n" "$AO2_FACTORY_PROJECT_RUN_ROOT"
printf "factory_project_run_summary=%s\n" "$summary_json"
printf "factory_project_run_package=%s\n" "$project_out/missed-call-recovery-project-release-review-package.tgz"
printf "factory_project_acceptance_review=%s\n" "$project_acceptance_review_json"
printf "factory_project_run=passed\n"
printf "project_run_package=passed\n"
