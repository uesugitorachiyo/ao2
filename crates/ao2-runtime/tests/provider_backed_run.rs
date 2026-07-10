use std::fs;
use std::path::Path;
use std::sync::Mutex;

use ao2_adapters::ProviderKind;
use ao2_runtime::{
    approve_risky_pr_ticket, resume_risky_pr_provider_free, run_risky_pr_with_provider_prompt,
    ApprovalOptions, ProviderRunOptions, ResumeOptions, RunStatus, RunSummary,
};

mod support;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn run_provider_backed_end_to_end(options: ProviderRunOptions) -> anyhow::Result<RunSummary> {
    let repo = options.target_repo.clone();
    let mut summary = run_risky_pr_with_provider_prompt(options)?;
    while summary.status == RunStatus::WaitingForApproval {
        let pending = summary
            .approvals
            .iter()
            .find(|t| t.status == "pending" && t.requested_action == "sandbox:apply")
            .cloned();
        if let Some(ticket) = pending {
            approve_risky_pr_ticket(ApprovalOptions {
                target_repo: repo.clone(),
                ticket_id: ticket.ticket_id,
                approver: "human:test-operator".to_string(),
            })?;
            summary = resume_risky_pr_provider_free(ResumeOptions {
                target_repo: repo.clone(),
                run_id: summary.run_id.clone(),
            })?;
        } else {
            break;
        }
    }
    Ok(summary)
}

fn copy_fixture(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
    support::commit_fixture(dst);
}

#[test]
fn provider_backed_risky_run_uses_sandbox_patch_gate_for_implementer() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let prompt = r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: added validation around discount math\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 10\n'
printf 'Output tokens: 20\n'
printf 'Cost: $0.001\n'
"#;

    let summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("provider-run".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 1,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 1);
    assert!(summary.evidence_pack_path.exists());

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    assert!(evidence.contains("provider_prompt_transcript"));
    assert!(evidence.contains("provider_transcript_summary"));
    assert!(evidence.contains("added validation around discount math"));
    assert!(evidence.contains("\"input_tokens\": 10"));
    assert!(evidence.contains("\"cost_usd\": 0.001"));
    assert!(evidence.contains("sandbox_patch_preview"));
    assert!(evidence.contains("sandbox_patch_apply"));
    let evidence_json: serde_json::Value = serde_json::from_str(&evidence).unwrap();
    let preview_ref = evidence_json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["artifact_type"] == "sandbox_patch_preview")
        .unwrap();
    let preview: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(preview_ref["uri"].as_str().unwrap()).unwrap())
            .unwrap();
    let sandbox_ticket = summary
        .approvals
        .iter()
        .find(|ticket| ticket.requested_action == "sandbox:apply")
        .unwrap();
    assert_eq!(
        preview["approval_subject"]["schema_version"],
        "ao2.sandbox-patch-approval-subject.v1"
    );
    assert_eq!(sandbox_ticket.action_digest, preview["action_digest"]);
    assert!(matches!(
        preview["approval_subject"]["base_commit"]
            .as_str()
            .unwrap()
            .len(),
        40 | 64
    ));
    let provider_contract = &evidence_json["runtime_contract"]["provider_adapter_contract"];
    assert_eq!(
        provider_contract["schema_version"],
        "ao2.provider-adapter-contract.v1"
    );
    assert_eq!(provider_contract["status"], "observed");
    assert_eq!(provider_contract["fulfilled"], true);
    for field in [
        "evidence",
        "concerns",
        "blockers",
        "changed_files",
        "sandbox",
        "secret_redaction",
    ] {
        assert_eq!(
            provider_contract["requirements"][field], true,
            "provider adapter contract field {field} was not satisfied"
        );
    }
    assert_eq!(
        provider_contract["changed_files"][0],
        "discount_service/discounts.py"
    );
    assert_eq!(provider_contract["cost"]["observed_cost_usd"], 0.001);
    assert_eq!(provider_contract["cost"]["reported_summary_count"], 1);
    assert_eq!(provider_contract["cost"]["input_tokens"], 10);
    assert_eq!(provider_contract["cost"]["output_tokens"], 20);
    assert_eq!(provider_contract["cost"]["total_tokens"], 30);
    assert_eq!(provider_contract["factory_v3_role"], "parity_oracle_only");
    assert!(evidence_json["closures"][0]["cost_summary"]
        .as_str()
        .unwrap()
        .contains("observed_provider_cost_usd=0.001000"));

    let events = fs::read_to_string(repo.join(".ao2/runs/provider-run/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"adapter.completed\""));
    assert!(events.contains("\"event_type\":\"adapter.transcript.parsed\""));
    assert!(events.contains("\"event_type\":\"sandbox.patch.previewed\""));
    assert!(events.contains("\"event_type\":\"sandbox.patch.applied\""));

    let replay = ao2_runtime::replay_run(ao2_runtime::ReplayOptions {
        target_repo: repo,
        run_id: "provider-run".to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Accepted);
    assert!(replay.digest_failures.is_empty());

    env.restore();
}

#[test]
fn approved_sandbox_patch_rejects_target_head_drift_before_resume_apply() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = "printf 'changed after approval test\\n' > approval-drift.txt\n";

    let waiting = run_risky_pr_with_provider_prompt(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("sandbox-stale-base-run".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 1,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();
    assert_eq!(waiting.status, RunStatus::WaitingForApproval);
    let ticket = waiting
        .approvals
        .iter()
        .find(|ticket| ticket.status == "pending" && ticket.requested_action == "sandbox:apply")
        .unwrap();
    approve_risky_pr_ticket(ApprovalOptions {
        target_repo: repo.clone(),
        ticket_id: ticket.ticket_id.clone(),
        approver: "human:test-operator".to_string(),
    })
    .unwrap();

    fs::write(repo.join("base-drift.txt"), "new target base\n").unwrap();
    support::commit_all(&repo, "advance target base after sandbox approval");

    let error = resume_risky_pr_provider_free(ResumeOptions {
        target_repo: repo.clone(),
        run_id: "sandbox-stale-base-run".to_string(),
    })
    .unwrap_err();
    assert!(
        error.to_string().contains("approval") && error.to_string().contains("changed"),
        "{error:#}"
    );
    assert!(!repo.join("approval-drift.txt").exists());

    env.restore();
}

#[test]
fn provider_backed_run_stops_rejected_when_repair_budget_is_zero() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let prompt = r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: added validation without tests\n'
printf 'Changed files: discount_service/discounts.py\n'
"#;

    let summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("provider-budget-zero".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 0,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Rejected);
    assert_eq!(summary.rejection_count, 1);
    assert!(summary.evidence_pack_path.exists());

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    assert!(evidence.contains("repair_budget_exhausted"));
    assert!(evidence.contains("repair_attempts"));
    assert!(!evidence.contains("correction-patch.md"));

    let events =
        fs::read_to_string(repo.join(".ao2/runs/provider-budget-zero/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"closure.rejected\""));
    assert!(events.contains("\"event_type\":\"repair.budget.exhausted\""));

    let replay = ao2_runtime::replay_run(ao2_runtime::ReplayOptions {
        target_repo: repo,
        run_id: "provider-budget-zero".to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Rejected);
    assert!(replay.digest_failures.is_empty());

    env.restore();
}

#[test]
fn provider_backed_run_retries_after_verifier_failure_until_budget_accepts() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let workflow = temp.path().join("retry-verifier.yaml");
    fs::write(
        &workflow,
        r#"id: retry-verifier
version: 0.1.0
objective: Verify autonomous repair retry evidence.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: if [ -f verifier-ok ]; then exit 0; else touch verifier-ok; exit 1; fi
policy:
  deny_by_default: true
  approval_mode: exact_action_digest
"#,
    )
    .unwrap();

    let prompt = r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: added validation before retrying verifier\n'
printf 'Changed files: discount_service/discounts.py\n'
"#;

    let summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: workflow,
        run_id: Some("provider-retry-verifier".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 2,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 1);

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    assert!(evidence.contains("\"attempt\": 1"));
    assert!(evidence.contains("\"status\": \"failed\""));
    assert!(evidence.contains("\"attempt\": 2"));
    assert!(evidence.contains("\"status\": \"accepted\""));

    let events =
        fs::read_to_string(repo.join(".ao2/runs/provider-retry-verifier/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"repair.attempt.failed\""));
    assert!(events.contains("\"event_type\":\"repair.attempt.completed\""));

    env.restore();
}

#[test]
fn provider_backed_real_project_template_accepts_without_discount_assumptions() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project");
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(repo.join("README.md"), "real project\n").unwrap();
    support::commit_fixture(&repo);
    let workflow = temp.path().join("test-generation.yaml");
    fs::write(
        &workflow,
        r#"id: test-generation
version: 0.1.0
template_kind: real_project
objective: Add a deterministic smoke artifact for a real repository pilot.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test -f docs/ao2-pilot-smoke.txt
acceptance:
  - Smoke artifact exists.
  - Replay has zero digest failures.
"#,
    )
    .unwrap();

    let prompt = r#"printf 'pilot ok\n' > docs/ao2-pilot-smoke.txt
printf 'Summary: added deterministic real-project pilot artifact\n'
printf 'Changed files: docs/ao2-pilot-smoke.txt\n'
"#;

    let summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: workflow,
        run_id: Some("real-project-template".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 1,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 0);
    assert_eq!(
        fs::read_to_string(repo.join("docs/ao2-pilot-smoke.txt")).unwrap(),
        "pilot ok\n"
    );

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    let evidence_json: serde_json::Value = serde_json::from_str(&evidence).unwrap();
    assert_eq!(evidence_json["template_kind"], "real_project");
    assert!(evidence.contains("real_project_template"));
    assert!(!evidence.contains("review_missing_tests"));
    assert!(!repo.join("discount_service/discounts.py").exists());
    assert!(!repo.join("tests/test_discounts.py").exists());

    let replay = ao2_runtime::replay_run(ao2_runtime::ReplayOptions {
        target_repo: repo,
        run_id: "real-project-template".to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Accepted);
    assert!(replay.digest_failures.is_empty());

    env.restore();
}

#[test]
fn provider_backed_real_project_template_repairs_after_verifier_failure() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project-repair");
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(repo.join("README.md"), "real project\n").unwrap();
    support::commit_fixture(&repo);
    let workflow = temp.path().join("bug-fix.yaml");
    fs::write(
        &workflow,
        r#"id: bug-fix
version: 0.1.0
template_kind: real_project
objective: Repair a failed real-project verifier with a second provider pass.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test -f docs/fixed.txt
acceptance:
  - Fixed artifact exists after repair.
  - Replay has zero digest failures.
"#,
    )
    .unwrap();

    let prompt = r#"if [ -f docs/first-attempt.txt ]; then
  printf 'fixed\n' > docs/fixed.txt
else
  printf 'needs repair\n' > docs/first-attempt.txt
fi
printf 'Summary: attempted real-project repairable change\n'
printf 'Changed files: docs/first-attempt.txt, docs/fixed.txt\n'
"#;

    let summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: workflow,
        run_id: Some("real-project-repair".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 2,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 0);
    assert_eq!(
        fs::read_to_string(repo.join("docs/fixed.txt")).unwrap(),
        "fixed\n"
    );

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    println!("EVIDENCE CONTENT FOR DEBUGGING:\n{}", evidence);
    let evidence_json: serde_json::Value = serde_json::from_str(&evidence).unwrap();
    assert!(evidence.contains("\"attempt\": 1"));
    assert!(evidence.contains("\"status\": \"accepted\""));
    assert!(evidence.contains("real_project_template"));
    assert!(!evidence.contains("repair_budget_exhausted"));
    assert_eq!(
        evidence_json["run_health"]["schema_version"],
        "ao2.run-health.v1"
    );
    assert_eq!(evidence_json["run_health"]["repair_status"], "repaired");
    assert_eq!(evidence_json["run_health"]["repair_attempt_count"], 1);
    assert_eq!(evidence_json["run_health"]["accepted_repair_attempts"], 1);
    assert_eq!(evidence_json["run_health"]["attention_required"], false);
    assert!(evidence_json["run_health"]["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "repair_attempt" && entry["status"] == "accepted"));

    let events =
        fs::read_to_string(repo.join(".ao2/runs/real-project-repair/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"repair.attempt.started\""));
    assert!(events.contains("\"event_type\":\"repair.attempt.completed\""));

    let replay = ao2_runtime::replay_run(ao2_runtime::ReplayOptions {
        target_repo: repo,
        run_id: "real-project-repair".to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Accepted);
    assert!(replay.digest_failures.is_empty());

    env.restore();
}

#[test]
fn provider_backed_real_project_template_accepts_node_verifier_commands() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();

    let npm_test_repo = temp.path().join("node-npm-test");
    create_node_package_repo(&npm_test_repo);
    let npm_test_workflow = write_real_project_workflow(
        temp.path(),
        "node-npm-test.yaml",
        "node-npm-test",
        "Validate npm test verifier for a Node real-project template.",
        "npm test",
    );
    let npm_test_prompt = r#"cat > src/math.mjs <<'JS'
export function add(left, right) {
  return left + right;
}
JS
mkdir -p test
cat > test/math.test.mjs <<'JS'
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { add } from '../src/math.mjs';

test('adds two numbers', () => {
  assert.equal(add(2, 3), 5);
});
JS
printf 'Summary: added npm test coverage for Node math module\n'
printf 'Changed files: src/math.mjs, test/math.test.mjs\n'
"#;
    let npm_test_summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: npm_test_repo.clone(),
        workflow_path: npm_test_workflow,
        run_id: Some("node-npm-test".to_string()),
        provider: ProviderKind::Scripted,
        prompt: npm_test_prompt.to_string(),
        max_repair_attempts: 0,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();
    assert_node_run_accepted(
        &npm_test_repo,
        &npm_test_summary,
        "node-npm-test",
        "npm test",
    );

    let typecheck_repo = temp.path().join("node-typecheck");
    create_node_package_repo(&typecheck_repo);
    let typecheck_workflow = write_real_project_workflow(
        temp.path(),
        "node-typecheck.yaml",
        "node-typecheck",
        "Validate npm run typecheck verifier for a Node real-project template.",
        "npm run typecheck",
    );
    let typecheck_prompt = r#"mkdir -p src scripts
cat > src/math.mjs <<'JS'
export function add(left, right) {
  return left + right;
}
JS
cat > scripts/typecheck.mjs <<'JS'
import { readFileSync } from 'node:fs';

const source = readFileSync('src/math.mjs', 'utf8');
if (!source.includes('export function add')) {
  throw new Error('expected add export');
}
JS
printf 'Summary: added dependency-free Node typecheck script\n'
printf 'Changed files: src/math.mjs, scripts/typecheck.mjs\n'
"#;
    let typecheck_summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: typecheck_repo.clone(),
        workflow_path: typecheck_workflow,
        run_id: Some("node-typecheck".to_string()),
        provider: ProviderKind::Scripted,
        prompt: typecheck_prompt.to_string(),
        max_repair_attempts: 0,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();
    assert_node_run_accepted(
        &typecheck_repo,
        &typecheck_summary,
        "node-typecheck",
        "npm run typecheck",
    );

    let workspace_repo = temp.path().join("node-workspace");
    create_node_workspace_repo(&workspace_repo);
    let workspace_workflow = write_real_project_workflow(
        temp.path(),
        "node-workspace.yaml",
        "node-workspace",
        "Validate npm workspace verifier for a Node real-project template.",
        "npm test --workspace @ao2/node-pilot",
    );
    let workspace_prompt = r#"mkdir -p packages/app/src packages/app/test
cat > packages/app/src/value.mjs <<'JS'
export const value = 42;
JS
cat > packages/app/test/value.test.mjs <<'JS'
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { value } from '../src/value.mjs';

test('exports workspace value', () => {
  assert.equal(value, 42);
});
JS
printf 'Summary: added workspace test coverage for Node package\n'
printf 'Changed files: packages/app/src/value.mjs, packages/app/test/value.test.mjs\n'
"#;
    let workspace_summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: workspace_repo.clone(),
        workflow_path: workspace_workflow,
        run_id: Some("node-workspace".to_string()),
        provider: ProviderKind::Scripted,
        prompt: workspace_prompt.to_string(),
        max_repair_attempts: 0,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();
    assert_node_run_accepted(
        &workspace_repo,
        &workspace_summary,
        "node-workspace",
        "npm test --workspace @ao2/node-pilot",
    );

    env.restore();
}

#[test]
fn provider_backed_real_project_repair_prompt_includes_verifier_context_for_node() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("node-repair-context");
    create_node_package_repo(&repo);
    let workflow = write_real_project_workflow(
        temp.path(),
        "node-repair-context.yaml",
        "node-repair-context",
        "Repair a failing Node verifier using structured verifier context.",
        "npm test",
    );

    let prompt = r#"mkdir -p src test
if printf '%s' "$AO2_REPAIR_VERIFIER_OUTPUT" | grep -q 'expected 42 after repair'; then
  cat > src/value.mjs <<'JS'
export const value = 42;
JS
else
  cat > src/value.mjs <<'JS'
export const value = 41;
JS
fi
cat > test/value.test.mjs <<'JS'
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { value } from '../src/value.mjs';

test('uses verifier context to repair value', () => {
  assert.equal(value, 42, 'expected 42 after repair');
});
JS
printf 'Summary: repaired Node value when verifier context was available\n'
printf 'Changed files: src/value.mjs, test/value.test.mjs\n'
"#;

    let summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: workflow,
        run_id: Some("node-repair-context".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 1,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 0);
    assert_eq!(
        fs::read_to_string(repo.join("src/value.mjs")).unwrap(),
        "export const value = 42;\n"
    );

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    assert!(evidence.contains("repair_prompt"));
    assert!(evidence.contains("repaired Node value when verifier context was available"));
    let repair_prompt = read_artifact_content(&evidence, "repair_prompt");
    assert!(repair_prompt.contains("AO2_REPAIR_VERIFIER_OUTPUT"));
    assert!(repair_prompt.contains("expected 42 after repair"));

    let events =
        fs::read_to_string(repo.join(".ao2/runs/node-repair-context/events.jsonl")).unwrap();
    println!("EVENTS LOG FOR DEBUGGING:\n{}", events);
    assert!(events.contains("\"event_type\":\"repair.prompt.created\""));
    assert!(events.contains("\"event_type\":\"repair.attempt.completed\""));

    let replay = ao2_runtime::replay_run(ao2_runtime::ReplayOptions {
        target_repo: repo,
        run_id: "node-repair-context".to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Accepted);
    assert!(replay.digest_failures.is_empty());

    env.restore();
}

#[test]
fn provider_backed_real_project_repair_prompt_includes_verifier_context_for_python() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("python-repair-context");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("README.md"), "python repair context\n").unwrap();
    support::commit_fixture(&repo);
    let workflow = write_real_project_workflow(
        temp.path(),
        "python-repair-context.yaml",
        "python-repair-context",
        "Repair a failing Python verifier using structured verifier context.",
        "python -c \"from pathlib import Path; assert Path('src/value.txt').read_text().strip() == 'ok', 'expected ok after repair'\"",
    );

    let prompt = r#"mkdir -p src
if printf '%s' "$AO2_REPAIR_VERIFIER_OUTPUT" | grep -q 'expected ok after repair'; then
  printf 'ok\n' > src/value.txt
else
  printf 'bad\n' > src/value.txt
fi
printf 'Summary: repaired Python value when verifier context was available\n'
printf 'Changed files: src/value.txt\n'
"#;

    let summary = run_provider_backed_end_to_end(ProviderRunOptions {
        target_repo: repo.clone(),
        workflow_path: workflow,
        run_id: Some("python-repair-context".to_string()),
        provider: ProviderKind::Scripted,
        prompt: prompt.to_string(),
        max_repair_attempts: 1,
        max_budget_usd: None,
        repair_source: None,
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 0);
    assert_eq!(
        fs::read_to_string(repo.join("src/value.txt")).unwrap(),
        "ok\n"
    );

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    assert!(evidence.contains("repair_prompt"));
    assert!(evidence.contains("repaired Python value when verifier context was available"));
    let repair_prompt = read_artifact_content(&evidence, "repair_prompt");
    assert!(repair_prompt.contains("AO2_REPAIR_VERIFIER_OUTPUT"));
    assert!(repair_prompt.contains("expected ok after repair"));

    let events =
        fs::read_to_string(repo.join(".ao2/runs/python-repair-context/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"repair.prompt.created\""));
    assert!(events.contains("\"event_type\":\"repair.attempt.completed\""));

    let replay = ao2_runtime::replay_run(ao2_runtime::ReplayOptions {
        target_repo: repo,
        run_id: "python-repair-context".to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Accepted);
    assert!(replay.digest_failures.is_empty());

    env.restore();
}

fn create_node_package_repo(repo: &Path) {
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{
  "name": "ao2-node-pilot",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "node --test",
    "typecheck": "node scripts/typecheck.mjs"
  }
}
"#,
    )
    .unwrap();
    fs::write(repo.join("src/.gitkeep"), "").unwrap();
    support::commit_fixture(repo);
}

fn create_node_workspace_repo(repo: &Path) {
    fs::create_dir_all(repo.join("packages/app/src")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{
  "name": "ao2-node-workspace-pilot",
  "version": "0.1.0",
  "private": true,
  "workspaces": ["packages/app"]
}
"#,
    )
    .unwrap();
    fs::write(
        repo.join("packages/app/package.json"),
        r#"{
  "name": "@ao2/node-pilot",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "node --test"
  }
}
"#,
    )
    .unwrap();
    fs::write(repo.join("packages/app/src/.gitkeep"), "").unwrap();
    support::commit_fixture(repo);
}

fn write_real_project_workflow(
    root: &Path,
    filename: &str,
    id: &str,
    objective: &str,
    verifier: &str,
) -> std::path::PathBuf {
    let workflow = root.join(filename);
    fs::write(
        &workflow,
        format!(
            r#"id: {id}
version: 0.1.0
template_kind: real_project
objective: {objective}
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: {verifier}
acceptance:
  - Node verifier exits successfully.
  - Replay has zero digest failures.
"#
        ),
    )
    .unwrap();
    workflow
}

fn assert_node_run_accepted(
    repo: &Path,
    summary: &ao2_runtime::RunSummary,
    run_id: &str,
    verifier: &str,
) {
    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 0);
    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    let evidence_json: serde_json::Value = serde_json::from_str(&evidence).unwrap();
    assert_eq!(evidence_json["template_kind"], "real_project");
    assert!(evidence.contains("real_project_template"));
    assert!(evidence.contains(verifier));

    let replay = ao2_runtime::replay_run(ao2_runtime::ReplayOptions {
        target_repo: repo.to_path_buf(),
        run_id: run_id.to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Accepted);
    assert!(replay.digest_failures.is_empty());
}

fn read_artifact_content(evidence: &str, artifact_type: &str) -> String {
    let evidence_json: serde_json::Value = serde_json::from_str(evidence).unwrap();
    let uri = evidence_json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["artifact_type"] == artifact_type)
        .and_then(|artifact| artifact["uri"].as_str())
        .unwrap();
    fs::read_to_string(uri).unwrap()
}

struct EnvSnapshot {
    openai: Option<std::ffi::OsString>,
    anthropic: Option<std::ffi::OsString>,
}

impl EnvSnapshot {
    fn clear_for_runtime() -> Self {
        let snapshot = Self {
            openai: std::env::var_os("OPENAI_API_KEY"),
            anthropic: std::env::var_os("ANTHROPIC_API_KEY"),
        };
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
        snapshot
    }

    fn restore(self) {
        restore_env("OPENAI_API_KEY", self.openai);
        restore_env("ANTHROPIC_API_KEY", self.anthropic);
    }
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}
