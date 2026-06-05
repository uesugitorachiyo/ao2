#!/bin/sh
set -eu

# Dogfood the factory-facing app replacement entry point:
# ao2 factory app-run

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
AO2_FACTORY_APP_RUN_ROOT="${AO2_FACTORY_APP_RUN_ROOT:-$repo_root/target/factory-app-run-smoke/$timestamp}"
AO2_BIN="${AO2_BIN:-$repo_root/target/release/ao2}"

ao2_cmd() {
  if [ -x "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  else
    cargo run -p ao2-cli --quiet -- "$@"
  fi
}

mkdir -p "$AO2_FACTORY_APP_RUN_ROOT"
AO2_FACTORY_APP_RUN_ROOT=$(CDPATH= cd -- "$AO2_FACTORY_APP_RUN_ROOT" && pwd)

target="$AO2_FACTORY_APP_RUN_ROOT/missed-call-recovery-target"
spec="$AO2_FACTORY_APP_RUN_ROOT/factory-app-missed-call-recovery.md"
prompt="$AO2_FACTORY_APP_RUN_ROOT/provider-prompt.sh"
signing_key="$AO2_FACTORY_APP_RUN_ROOT/factory-app-signing-key.pem"
run_out="$AO2_FACTORY_APP_RUN_ROOT/run"
summary_json="$AO2_FACTORY_APP_RUN_ROOT/factory-app-run-summary.json"
run_json="$AO2_FACTORY_APP_RUN_ROOT/factory-app-run.json"
bundle_archive="$AO2_FACTORY_APP_RUN_ROOT/app-run-evidence-bundle.tgz"
bundle_json="$AO2_FACTORY_APP_RUN_ROOT/app-run-evidence-bundle.json"

rm -rf "$target" "$run_out"
mkdir -p "$target" "$run_out"
cp -R "$repo_root/fixtures/missed-call-recovery/." "$target/"

cat > "$target/tests/test_recovery_workflow.py" <<'PY'
from missed_call_recovery.workflow import LeadCapture, build_recovery_message, classify_lead


def lead(**overrides):
    data = {
        "customer_name": "Jordan",
        "phone": "530-555-0188",
        "missed_at_minutes_ago": 8,
        "repeat_calls_24h": 2,
        "service_requested": "emergency leak repair",
        "business_name": "Missed Call Recovery",
        "consent_to_text": True,
    }
    data.update(overrides)
    return LeadCapture(**data)


def test_recovery_message_mentions_customer_and_business():
    message = build_recovery_message(lead())
    assert "Jordan" in message
    assert "Missed Call Recovery" in message
    assert "emergency leak repair" in message
    assert "reply" in message.lower()


def test_hot_lead_score_prioritizes_recent_repeat_callers():
    classification = classify_lead(lead())
    assert classification["priority"] == "hot"
    assert classification["score"] >= 85
    assert "repeat caller" in classification["reason"].lower()


def test_opt_out_contact_gets_no_text_message():
    assert build_recovery_message(lead(consent_to_text=False)) == ""
PY

cat > "$spec" <<'MD'
# Factory App Missed Call Recovery

Build a production app workflow from a plain greenfield spec.

Acceptance:
- The implementation models a missed-call lead as a LeadCapture record.
- The recovery message mentions the customer, business, requested service, and a reply path.
- Recent repeat callers are classified as hot with a score of at least 85.
- Leads without text consent return no recovery text.
- The verifier can run with `python -m pytest -q`.
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
    elif capture.repeat_calls_24h == 1:
        score += 10
        reasons.append("called once today")
    if any(word in capture.service_requested.lower() for word in ["emergency", "leak", "no heat", "water heater"]):
        score += 15
        reasons.append("urgent service request")
    score = min(score, 100)
    if score >= 85:
        priority = "hot"
    elif score >= 65:
        priority = "warm"
    else:
        priority = "standard"
    return {
        "priority": priority,
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
printf 'Summary: factory app run implemented missed-call recovery scoring and reply copy\n'
printf 'Changed files: missed_call_recovery/workflow.py\n'
printf 'Input tokens: 43\n'
SH

ao2_cmd workbench support-keygen --out "$signing_key" --bits 2048 >/dev/null

ao2_cmd factory app-run \
  --spec "$spec" \
  --target "$target" \
  --run-id factory-app-smoke \
  --verifier-command "python -m pytest -q" \
  --provider scripted \
  --provider-prompt-file "$prompt" \
  --signing-key "$signing_key" \
  --signer-id factory-app-smoke \
  --out-dir "$run_out" \
  --json > "$run_json"

ao2_cmd factory app-run-bundle \
  --app-run "$run_json" \
  --out "$bundle_archive" \
  --json > "$bundle_json"

node - "$run_json" "$bundle_json" "$summary_json" "$AO2_FACTORY_APP_RUN_ROOT" <<'NODE'
const fs = require('fs');
const [runPath, bundlePath, summaryPath, root] = process.argv.slice(2);
const run = JSON.parse(fs.readFileSync(runPath, 'utf8'));
const bundle = JSON.parse(fs.readFileSync(bundlePath, 'utf8'));

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

assert(run.schema_version === 'ao2.factory-app-run.v1', 'unexpected factory app schema');
assert(run.status === 'accepted', 'factory app run must be accepted');
assert(run.factory_replacement_boundary.factory_v3_drives_workflow === false, 'factory-v3 must not drive workflow');
assert(run.factory_replacement_boundary.factory_v3_role === 'parity_oracle_only', 'factory-v3 role mismatch');
assert(run.factory_replacement_boundary.control_plane_role === 'read_only_observer_after_signed_evidence', 'control-plane role mismatch');
assert(run.factory_replacement_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'release owner mismatch');
assert(run.factory_replacement_boundary.control_plane_approves_release === false, 'control plane must not approve release');
assert(run.factory_replacement_boundary.mutates_ao_artifacts === false, 'must not mutate AO artifacts');
assert(typeof run.rubric_sha256 === 'string' && run.rubric_sha256.length === 64, 'rubric_sha256 missing');
assert(run.release_review.rubric_sha256 === run.rubric_sha256, 'release review rubric digest mismatch');
assert(run.app_run_checklist.ao2_derived_signed_evaluator_rubric === true, 'signed evaluator rubric not derived');
assert(run.app_run_checklist.verifier_outputs_reference_rubric_sha256 === true, 'verifier rubric digest reference missing');
assert(run.app_run_checklist.closer_outputs_reference_rubric_sha256 === true, 'closer rubric digest reference missing');
assert(run.release_review.downstream_contract.verifier_outputs_must_reference === 'rubric_sha256', 'verifier rubric contract missing');
assert(run.release_review.downstream_contract.closer_outputs_must_reference === 'rubric_sha256', 'closer rubric contract missing');
assert(run.app_run_checklist.ao2_executed_generated_governed_plan === true, 'governed plan not executed');
assert(run.app_run_checklist.release_review_artifacts_ready === true, 'release review artifacts not ready');
assert(run.app.governed_run.evaluator_decision_verification.status === 'accepted', 'evaluator decision not accepted');

for (const key of ['factory_app_run', 'evaluator_rubric', 'greenfield_governed_run', 'governed_run', 'evidence_pack', 'evaluator_decision']) {
  assert(fs.existsSync(run.artifacts[key]), `missing artifact ${key}: ${run.artifacts[key]}`);
}

assert(JSON.stringify(run).indexOf('Bearer ') === -1, 'bearer token leaked into artifact');
assert(bundle.schema_version === 'ao2.factory-app-run-bundle.v1', 'unexpected app-run bundle schema');
assert(bundle.status === 'bundled', 'app-run bundle must be bundled');
assert(fs.existsSync(bundle.archive), `missing bundle archive: ${bundle.archive}`);
assert(bundle.artifact_count === 8, 'app-run bundle artifact count mismatch');
assert(bundle.trust_boundary.control_plane_role === 'read_only_observer_after_signed_evidence', 'bundle control-plane role mismatch');
assert(bundle.trust_boundary.control_plane_approves_release === false, 'bundle control plane must not approve release');
assert(bundle.trust_boundary.mutates_ao_artifacts === false, 'bundle must not mutate AO artifacts');
assert(JSON.stringify(bundle).indexOf('Bearer ') === -1, 'bearer token leaked into bundle metadata');

const summary = {
  schema_version: 'ao2.factory-app-run-smoke.v1',
  status: 'passed',
  root,
  product_fixture: 'missed-call-recovery',
  product_domain: 'missed-call revenue recovery',
  run_id: run.run_id,
  run_status: run.status,
  factory_app_schema: run.schema_version,
  factory_v3_drives_workflow: false,
  factory_v3_role: 'parity_oracle_only',
  control_plane_role: run.factory_replacement_boundary.control_plane_role,
  release_acceptance_owner: run.factory_replacement_boundary.release_acceptance_owner,
  control_plane_approves_release: run.factory_replacement_boundary.control_plane_approves_release,
  mutates_ao_artifacts: run.factory_replacement_boundary.mutates_ao_artifacts,
  release_review_artifacts_ready: run.app_run_checklist.release_review_artifacts_ready,
  app_run_bundle: bundle.archive,
  app_run_bundle_status: bundle.status,
  rubric_sha256: run.rubric_sha256,
  evaluator_decision_status: run.app.governed_run.evaluator_decision_verification.status,
  artifacts: run.artifacts
};
fs.writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
NODE

rm -f "$signing_key"

printf "factory_app_run_root=%s\n" "$AO2_FACTORY_APP_RUN_ROOT"
printf "factory_app_run_summary=%s\n" "$summary_json"
printf "factory_app_run_bundle=%s\n" "$bundle_archive"
printf "factory_app_run=passed\n"
printf "app_run_bundle=passed\n"
