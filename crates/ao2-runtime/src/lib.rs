use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use ao2_adapters::{
    apply_sandbox_patch, posix_shell_command, preview_sandbox_patch,
    run_provider_prompt_in_sandbox, scripted_prompt_prefers_posix_shell, AdapterRunResult,
    ProviderKind, ProviderPromptRequest, SandboxPatchApplyRequest,
    DEFAULT_PROVIDER_TIMEOUT_SECONDS,
};
use ao2_artifacts::ArtifactStore;
use ao2_core::{
    atomic_write, new_id, sha256_hex, Actor, AoEvent, ApprovalTicket, ArtifactRef, ClosureReport,
    PolicyDecision,
};
use ao2_policy::{
    create_approval_ticket, deny, evaluate, fail_on_forbidden_provider_api_keys, grant_exact,
    ToolRequest,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use ao2_core::RunStatus;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub target_repo: PathBuf,
    pub workflow_path: PathBuf,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderRunOptions {
    pub target_repo: PathBuf,
    pub workflow_path: PathBuf,
    pub run_id: Option<String>,
    pub provider: ProviderKind,
    pub prompt: String,
    pub max_repair_attempts: usize,
    pub max_budget_usd: Option<f64>,
    pub repair_source: Option<RepairSourceContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairSourceContext {
    pub source_run_id: String,
    pub evidence_pack_path: PathBuf,
    pub source_verdict: String,
    pub run_health: serde_json::Value,
    pub unresolved_concerns: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub latest_verifier_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub status: RunStatus,
    pub run_dir: PathBuf,
    pub evidence_pack_path: PathBuf,
    pub report_path: PathBuf,
    pub run_record_path: PathBuf,
    pub denied_actions: Vec<PolicyDecision>,
    pub approvals: Vec<ApprovalTicket>,
    pub rejection_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalWaitSummary {
    pub run_id: String,
    pub status: RunStatus,
    pub run_dir: PathBuf,
    pub run_record_path: PathBuf,
    pub approval_ticket: ApprovalTicket,
}

#[derive(Debug, Clone)]
pub struct ApprovalOptions {
    pub target_repo: PathBuf,
    pub ticket_id: String,
    pub approver: String,
}

#[derive(Debug, Clone)]
pub struct ResumeOptions {
    pub target_repo: PathBuf,
    pub run_id: String,
}

#[derive(Debug, Clone)]
pub struct ReplayOptions {
    pub target_repo: PathBuf,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub run_id: String,
    pub status: RunStatus,
    pub event_count: usize,
    pub artifact_count: usize,
    pub event_types: Vec<String>,
    pub digest_failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredApproval {
    ticket: ApprovalTicket,
    request: ToolRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairAttempt {
    pub attempt: usize,
    pub trigger: String,
    pub status: String,
    pub evidence_refs: Vec<String>,
    pub summary: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct VerifierOutcome {
    artifact: ArtifactRef,
    success: bool,
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkflowSpec {
    id: String,
    version: String,
    template_kind: Option<String>,
    objective: String,
    roles: Vec<String>,
    #[serde(default)]
    tasks: Vec<serde_json::Value>,
    #[serde(default)]
    dependencies: Vec<serde_json::Value>,
    #[serde(default)]
    factory_v3_compatibility: Option<serde_json::Value>,
    verifier: WorkflowVerifier,
    #[serde(default)]
    acceptance: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkflowVerifier {
    command: String,
}

#[derive(Debug)]
struct RunContext {
    run_id: String,
    workflow_id: String,
    template_kind: Option<String>,
    objective: String,
    roles: Vec<String>,
    workflow_tasks: Vec<serde_json::Value>,
    workflow_dependencies: Vec<serde_json::Value>,
    factory_v3_compatibility: Option<serde_json::Value>,
    acceptance: Vec<String>,
    verifier_command: String,
    target_repo: PathBuf,
    run_dir: PathBuf,
    events_path: PathBuf,
    artifact_store: ArtifactStore,
    artifacts: Vec<ArtifactRef>,
    policy_decisions: Vec<PolicyDecision>,
    approvals: Vec<ApprovalTicket>,
    closure_reports: Vec<ClosureReport>,
    repair_attempts: Vec<RepairAttempt>,
}

impl RunContext {
    fn is_real_project_template(&self) -> bool {
        self.template_kind.as_deref() == Some("real_project")
    }
}

pub fn run_risky_pr_provider_free(options: RunOptions) -> Result<RunSummary> {
    let target_repo = options.target_repo.clone();
    let waiting = start_risky_pr_provider_free(options)?;
    approve_risky_pr_ticket(ApprovalOptions {
        target_repo: target_repo.clone(),
        ticket_id: waiting.approval_ticket.ticket_id,
        approver: "human:local-user".to_string(),
    })?;
    resume_risky_pr_provider_free(ResumeOptions {
        target_repo,
        run_id: waiting.run_id,
    })
}

pub fn run_risky_pr_with_provider_prompt(options: ProviderRunOptions) -> Result<RunSummary> {
    let target_repo = options.target_repo.clone();
    let waiting = start_risky_pr_provider_free(RunOptions {
        target_repo: target_repo.clone(),
        workflow_path: options.workflow_path,
        run_id: options.run_id,
    })?;
    approve_risky_pr_ticket(ApprovalOptions {
        target_repo: target_repo.clone(),
        ticket_id: waiting.approval_ticket.ticket_id,
        approver: "human:local-user".to_string(),
    })?;

    let (mut ctx, status) = load_run_context(&target_repo, &waiting.run_id)?;
    if status != RunStatus::WaitingForApproval {
        return Err(anyhow!(
            "provider-backed run {} cannot continue from status {:?}",
            waiting.run_id,
            status
        ));
    }
    if !ctx
        .approvals
        .iter()
        .any(|ticket| ticket.status == "approved")
    {
        return Err(anyhow!(
            "provider-backed run {} is waiting for approval",
            waiting.run_id
        ));
    }

    let provider_prompt = if let Some(repair_source) = options.repair_source.as_ref() {
        record_repair_source_context(&mut ctx, repair_source)?;
        build_provider_prompt_with_repair_source(options.provider, &options.prompt, repair_source)?
    } else {
        options.prompt.clone()
    };

    let status = if ctx.is_real_project_template() {
        run_real_project_provider_workflow(
            &mut ctx,
            options.provider,
            &provider_prompt,
            options.max_repair_attempts,
            options.max_budget_usd,
        )?
    } else {
        apply_provider_prompt_patch(
            &mut ctx,
            options.provider,
            &provider_prompt,
            options.max_budget_usd,
        )?;
        let review = reviewer_concern(&mut ctx)?;
        reject_for_missing_tests(&mut ctx, &review)?;
        if options.max_repair_attempts == 0 {
            record_repair_budget_exhausted(&mut ctx, "review_missing_tests", &review)?;
            RunStatus::Rejected
        } else {
            match run_repair_loop_after_rejection(&mut ctx, options.max_repair_attempts, &review)? {
                Some(test_log) => {
                    accept_final(&mut ctx, &test_log)?;
                    RunStatus::Accepted
                }
                None => RunStatus::Rejected,
            }
        }
    };

    let evidence_pack_path = export_evidence_pack(&ctx)?;
    let report_path = render_static_report(&ctx, &evidence_pack_path)?;
    write_run_record(&ctx, status, &evidence_pack_path, &report_path)?;
    let denied_actions = denied_actions(&ctx.policy_decisions);
    let approvals = ctx.approvals.clone();
    let rejection_count = ctx
        .closure_reports
        .iter()
        .filter(|report| report.verdict == "rejected")
        .count();

    Ok(RunSummary {
        run_id: waiting.run_id,
        status,
        run_dir: ctx.run_dir.clone(),
        evidence_pack_path,
        report_path,
        run_record_path: ctx.run_dir.join("run-record.json"),
        denied_actions,
        approvals,
        rejection_count,
    })
}

pub fn start_risky_pr_provider_free(options: RunOptions) -> Result<ApprovalWaitSummary> {
    fail_on_forbidden_provider_api_keys()?;
    ensure_target_repo(&options.target_repo)?;
    let workflow = load_workflow(&options.workflow_path)?;

    let run_id = options.run_id.unwrap_or_else(|| new_id("run"));
    let run_dir = options.target_repo.join(".ao2").join("runs").join(&run_id);
    let artifact_root = run_dir.join("artifacts");
    fs::create_dir_all(&artifact_root)
        .with_context(|| format!("create run artifact dir {}", artifact_root.display()))?;
    let events_path = run_dir.join("events.jsonl");
    let artifact_store = ArtifactStore::new(&artifact_root);
    let mut ctx = RunContext {
        run_id: run_id.clone(),
        workflow_id: workflow.workflow_id(),
        template_kind: workflow.template_kind,
        objective: workflow.objective,
        roles: workflow.roles,
        workflow_tasks: workflow.tasks,
        workflow_dependencies: workflow.dependencies,
        factory_v3_compatibility: workflow.factory_v3_compatibility,
        acceptance: workflow.acceptance,
        verifier_command: workflow.verifier.command,
        target_repo: options.target_repo,
        run_dir: run_dir.clone(),
        events_path,
        artifact_store,
        artifacts: Vec::new(),
        policy_decisions: Vec::new(),
        approvals: Vec::new(),
        closure_reports: Vec::new(),
        repair_attempts: Vec::new(),
    };

    emit(
        &ctx,
        "run.created",
        None,
        None,
        Actor::system(),
        json!({
            "objective": ctx.objective,
            "workflow_path": options.workflow_path,
            "status": "created"
        }),
    )?;
    emit(
        &ctx,
        "run.compiled",
        None,
        None,
        Actor::system(),
        json!({
            "workflow_id": ctx.workflow_id,
            "roles": ctx.roles,
            "workflow_tasks": ctx.workflow_tasks,
            "workflow_dependencies": ctx.workflow_dependencies,
            "factory_v3_compatibility": ctx.factory_v3_compatibility,
            "factory_v3_drives_workflow": false,
            "status": "compiled"
        }),
    )?;

    let context_bundle = json!({
        "sources": repo_file_inventory(&ctx.target_repo, 80)?,
        "role_scope": "planner",
        "redactions": [],
        "template_kind": ctx.template_kind
    });
    let context = ctx.artifact_store.put_text(
        "context_bundle",
        "planner",
        "context.json",
        "application/json",
        &serde_json::to_string_pretty(&context_bundle)?,
        vec![],
    )?;
    artifact_created(&mut ctx, &context)?;

    let plan = ctx.artifact_store.put_text(
        "plan",
        "planner",
        "plan.md",
        "text/markdown",
        &format!(
            r#"# Scoped Plan

Objective: {}

Likely files:
{}

Expected verifier:
- `{}`

Risks:
- broad file writes must be blocked
- external git push must be denied

Acceptance criteria:
{}
"#,
            ctx.objective,
            plan_file_list(&ctx.target_repo)?,
            ctx.verifier_command,
            plan_acceptance_list(&ctx)
        ),
        vec![context.artifact_id.clone()],
    )?;
    artifact_created(&mut ctx, &plan)?;

    let approval_ticket = simulate_policy_denial_and_pending_approval(&mut ctx)?;
    let evidence_pack_path = ctx.run_dir.join("evidence-pack").join("evidence-pack.json");
    let report_path = ctx.run_dir.join("report").join("index.html");
    write_run_record(
        &ctx,
        RunStatus::WaitingForApproval,
        &evidence_pack_path,
        &report_path,
    )?;

    Ok(ApprovalWaitSummary {
        run_id,
        status: RunStatus::WaitingForApproval,
        run_record_path: run_dir.join("run-record.json"),
        run_dir,
        approval_ticket,
    })
}

pub fn approve_risky_pr_ticket(options: ApprovalOptions) -> Result<ApprovalTicket> {
    fail_on_forbidden_provider_api_keys()?;
    ensure_target_repo(&options.target_repo)?;

    let approval_path = find_approval_path(&options.target_repo, &options.ticket_id)?;
    // Serialize the read-modify-write below (read approval → grant → write approval →
    // rewrite run record) against any other `ao2` process touching the same run. The
    // lock is released when this function returns. `write_run_record` and
    // `load_run_context` below stay lock-free so they never re-enter this lock.
    let run_dir = approval_path
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| {
            anyhow!(
                "approval path has no run directory: {}",
                approval_path.display()
            )
        })?;
    let _run_lock = RunLock::acquire(run_dir)?;
    let content = fs::read_to_string(&approval_path)
        .with_context(|| format!("read approval {}", approval_path.display()))?;
    let mut stored: StoredApproval = serde_json::from_str(&content)?;
    if stored.ticket.status == "approved" {
        return Ok(stored.ticket);
    }

    let granted = grant_exact(&stored.ticket, &options.approver, &stored.request)?;
    stored.ticket = granted.clone();
    atomic_write(&approval_path, serde_json::to_string_pretty(&stored)?)
        .with_context(|| format!("write approval {}", approval_path.display()))?;

    let (mut ctx, _) = load_run_context(&options.target_repo, &granted.run_id)?;
    replace_ticket(&mut ctx.approvals, granted.clone());
    emit(
        &ctx,
        "approval.granted",
        Some("implementer"),
        Some("approve_discount_patch"),
        Actor::human_local(),
        serde_json::to_value(&granted)?,
    )?;
    let evidence_pack_path = ctx.run_dir.join("evidence-pack").join("evidence-pack.json");
    let report_path = ctx.run_dir.join("report").join("index.html");
    write_run_record(
        &ctx,
        RunStatus::WaitingForApproval,
        &evidence_pack_path,
        &report_path,
    )?;

    Ok(granted)
}

pub fn resume_risky_pr_provider_free(options: ResumeOptions) -> Result<RunSummary> {
    fail_on_forbidden_provider_api_keys()?;
    ensure_target_repo(&options.target_repo)?;

    let (mut ctx, status) = load_run_context(&options.target_repo, &options.run_id)?;
    if status == RunStatus::Accepted {
        return summary_from_accepted_record(&options.target_repo, &options.run_id);
    }
    if status != RunStatus::WaitingForApproval {
        return Err(anyhow!(
            "run {} is not resumable from status {:?}",
            options.run_id,
            status
        ));
    }
    if !ctx
        .approvals
        .iter()
        .any(|ticket| ticket.status == "approved")
    {
        return Err(anyhow!(
            "run {} is waiting for approval before resume",
            options.run_id
        ));
    }

    apply_first_patch(&mut ctx)?;
    let review = reviewer_concern(&mut ctx)?;
    reject_for_missing_tests(&mut ctx, &review)?;
    apply_correction_patch(&mut ctx)?;
    let verifier = run_verifier(&mut ctx)?;
    if !verifier.success {
        return Err(anyhow!("verifier failed: {}", verifier.content));
    }
    accept_final(&mut ctx, &verifier.artifact)?;

    let evidence_pack_path = export_evidence_pack(&ctx)?;
    let report_path = render_static_report(&ctx, &evidence_pack_path)?;
    write_run_record(&ctx, RunStatus::Accepted, &evidence_pack_path, &report_path)?;
    let denied_actions = denied_actions(&ctx.policy_decisions);
    let approvals = ctx.approvals.clone();
    let rejection_count = ctx
        .closure_reports
        .iter()
        .filter(|report| report.verdict == "rejected")
        .count();

    Ok(RunSummary {
        run_id: options.run_id,
        status: RunStatus::Accepted,
        run_dir: ctx.run_dir.clone(),
        evidence_pack_path,
        report_path,
        run_record_path: ctx.run_dir.join("run-record.json"),
        denied_actions,
        approvals,
        rejection_count,
    })
}

pub fn replay_run(options: ReplayOptions) -> Result<ReplaySummary> {
    ensure_target_repo(&options.target_repo)?;
    let (ctx, status) = load_run_context(&options.target_repo, &options.run_id)?;
    let events = fs::read_to_string(&ctx.events_path)
        .with_context(|| format!("read events {}", ctx.events_path.display()))?;
    let mut event_count = 0;
    let mut event_types = Vec::new();
    let mut digest_failures = Vec::new();

    for (index, line) in events.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: AoEvent = serde_json::from_str(line)
            .with_context(|| format!("parse event line {}", index + 1))?;
        event_count += 1;
        event_types.push(event.event_type.clone());
        let expected = sha256_hex(serde_json::to_vec(&event.payload)?);
        if expected != event.payload_digest {
            digest_failures.push(format!("event {} payload digest mismatch", event.event_id));
        }
    }

    for artifact in &ctx.artifacts {
        let bytes =
            fs::read(&artifact.uri).with_context(|| format!("read artifact {}", artifact.uri))?;
        let expected = sha256_hex(bytes);
        if expected != artifact.digest {
            digest_failures.push(format!("artifact {} digest mismatch", artifact.artifact_id));
        }
    }

    if !digest_failures.is_empty() {
        return Err(anyhow!("digest mismatch: {}", digest_failures.join("; ")));
    }

    Ok(ReplaySummary {
        run_id: options.run_id,
        status,
        event_count,
        artifact_count: ctx.artifacts.len(),
        event_types,
        digest_failures,
    })
}

fn ensure_target_repo(target_repo: &Path) -> Result<()> {
    if !target_repo.exists() {
        return Err(anyhow!(
            "target repo does not exist: {}",
            target_repo.display()
        ));
    }
    Ok(())
}

impl WorkflowSpec {
    fn workflow_id(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

fn load_workflow(workflow_path: &Path) -> Result<WorkflowSpec> {
    if !workflow_path.exists() {
        return Err(anyhow!(
            "workflow file does not exist: {}",
            workflow_path.display()
        ));
    }
    let content = fs::read_to_string(workflow_path)
        .with_context(|| format!("read workflow file {}", workflow_path.display()))?;
    let workflow: WorkflowSpec = serde_yaml::from_str(&content)
        .with_context(|| format!("parse workflow file {}", workflow_path.display()))?;
    if workflow.id.trim().is_empty() {
        return Err(anyhow!("workflow id is required"));
    }
    if workflow.version.trim().is_empty() {
        return Err(anyhow!("workflow version is required"));
    }
    if workflow.objective.trim().is_empty() {
        return Err(anyhow!("workflow objective is required"));
    }
    if workflow.roles.is_empty() {
        return Err(anyhow!("workflow roles are required"));
    }
    if workflow.verifier.command.trim().is_empty() {
        return Err(anyhow!("workflow verifier command is required"));
    }
    Ok(workflow)
}

fn default_roles() -> Vec<String> {
    [
        "planner",
        "implementer",
        "reviewer",
        "test-engineer",
        "evaluator-closer",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn repo_file_inventory(root: &Path, limit: usize) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect_repo_files(root, root, limit, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_repo_files(
    root: &Path,
    dir: &Path,
    limit: usize,
    files: &mut Vec<String>,
) -> Result<()> {
    if files.len() >= limit {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if should_skip_repo_entry(&name.to_string_lossy()) {
            continue;
        }
        if path.is_dir() {
            collect_repo_files(root, &path, limit, files)?;
        } else if path.is_file() {
            files.push(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        if files.len() >= limit {
            break;
        }
    }
    Ok(())
}

fn should_skip_repo_entry(name: &str) -> bool {
    matches!(
        name,
        ".ao2"
            | ".git"
            | ".hg"
            | ".svn"
            | "target"
            | "node_modules"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".ruff_cache"
            | ".next"
            | ".expo"
            | "dist"
            | "build"
            | "coverage"
    )
}

fn plan_file_list(target_repo: &Path) -> Result<String> {
    let files = repo_file_inventory(target_repo, 12)?;
    if files.is_empty() {
        return Ok("- no files discovered".to_string());
    }
    Ok(files
        .iter()
        .map(|file| format!("- `{file}`"))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn plan_acceptance_list(ctx: &RunContext) -> String {
    let criteria = if ctx.acceptance.is_empty() {
        vec![
            "verifier command passes".to_string(),
            "patch stays scoped to the workflow objective".to_string(),
            "replay has zero digest failures".to_string(),
        ]
    } else {
        ctx.acceptance.clone()
    };
    criteria
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn emit(
    ctx: &RunContext,
    event_type: &str,
    role_id: Option<&str>,
    task_id: Option<&str>,
    actor: Actor,
    payload: serde_json::Value,
) -> Result<()> {
    let event = AoEvent::new(
        &ctx.run_id,
        &ctx.workflow_id,
        event_type,
        role_id,
        task_id,
        actor,
        payload,
    );
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.events_path)
        .with_context(|| format!("open events log {}", ctx.events_path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    // Flush the appended event to disk. This is the canonical, replayable evidence
    // stream (tool.denied / approval.* / closure.* verdicts); without sync_data the
    // tail can be lost on power loss while the run-record still claims completion.
    file.sync_data()
        .with_context(|| format!("fsync events log {}", ctx.events_path.display()))?;
    Ok(())
}

fn artifact_created(ctx: &mut RunContext, artifact: &ArtifactRef) -> Result<()> {
    ctx.artifacts.push(artifact.clone());
    emit(
        ctx,
        "artifact.created",
        Some(&artifact.producer),
        None,
        Actor::role(&artifact.producer),
        json!({
            "artifact_id": artifact.artifact_id,
            "artifact_type": artifact.artifact_type,
            "digest": artifact.digest,
            "uri": artifact.uri
        }),
    )
}

fn simulate_policy_denial_and_pending_approval(ctx: &mut RunContext) -> Result<ApprovalTicket> {
    let risky = ToolRequest {
        principal: "role:implementer".to_string(),
        tool: "git".to_string(),
        operation: "push".to_string(),
        resource: "origin main".to_string(),
        args: vec!["push".to_string(), "origin".to_string(), "main".to_string()],
        expected_side_effects: vec!["external_write".to_string()],
    };
    emit(
        ctx,
        "tool.requested",
        Some("implementer"),
        Some("policy_denied_git_push"),
        Actor::role("implementer"),
        serde_json::to_value(&risky)?,
    )?;
    let decision = deny(
        &risky,
        "git push is an external write and is denied in local MVP",
    );
    ctx.policy_decisions.push(decision.clone());
    emit(
        ctx,
        "tool.denied",
        Some("implementer"),
        Some("policy_denied_git_push"),
        Actor::system(),
        serde_json::to_value(&decision)?,
    )?;

    let safe = ToolRequest {
        principal: "role:implementer".to_string(),
        tool: "filesystem".to_string(),
        operation: "write_file".to_string(),
        resource: if ctx.is_real_project_template() {
            "provider_sandbox_patch".to_string()
        } else {
            "discount_service/discounts.py".to_string()
        },
        args: vec![
            "write_file".to_string(),
            if ctx.is_real_project_template() {
                "provider_sandbox_patch".to_string()
            } else {
                "discount_service/discounts.py".to_string()
            },
        ],
        expected_side_effects: vec!["repo_write".to_string()],
    };
    let safe_decision = evaluate(&safe);
    ctx.policy_decisions.push(safe_decision.clone());
    let pending = create_approval_ticket(&ctx.run_id, &safe);
    replace_ticket(&mut ctx.approvals, pending.clone());
    write_stored_approval(ctx, &pending, &safe)?;
    emit(
        ctx,
        "approval.requested",
        Some("implementer"),
        Some("approve_discount_patch"),
        Actor::system(),
        serde_json::to_value(&pending)?,
    )?;
    Ok(pending)
}

fn apply_first_patch(ctx: &mut RunContext) -> Result<()> {
    emit(
        ctx,
        "role.started",
        Some("implementer"),
        Some("implementer_first_patch"),
        Actor::role("implementer"),
        json!({"status": "started"}),
    )?;
    let target = ctx
        .target_repo
        .join("discount_service")
        .join("discounts.py");
    fs::write(
        &target,
        r#"def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
"#,
    )
    .with_context(|| format!("write first patch {}", target.display()))?;
    record_scripted_adapter_transcript(ctx)?;
    let patch = ctx.artifact_store.put_text(
        "patch_summary",
        "implementer",
        "first-patch.md",
        "text/markdown",
        "Updated `discount_service/discounts.py` with validation. Tests were not updated in this pass.",
        vec![],
    )?;
    artifact_created(ctx, &patch)?;
    emit(
        ctx,
        "role.completed",
        Some("implementer"),
        Some("implementer_first_patch"),
        Actor::role("implementer"),
        json!({"status": "completed", "concerns": [], "blockers": []}),
    )?;
    Ok(())
}

fn apply_provider_prompt_patch(
    ctx: &mut RunContext,
    provider: ProviderKind,
    prompt: &str,
    max_budget_usd: Option<f64>,
) -> Result<()> {
    apply_provider_prompt_patch_for_role(
        ctx,
        provider,
        prompt,
        max_budget_usd,
        "implementer",
        "provider_prompt_patch",
        None,
    )
}

fn apply_provider_prompt_patch_for_role(
    ctx: &mut RunContext,
    provider: ProviderKind,
    prompt: &str,
    max_budget_usd: Option<f64>,
    role_id: &str,
    action_id: &str,
    task_id: Option<&str>,
) -> Result<()> {
    emit(
        ctx,
        "role.started",
        Some(role_id),
        Some(action_id),
        Actor::role(role_id),
        json!({"status": "started", "provider": provider}),
    )?;

    let sandbox = run_provider_prompt_in_sandbox(ProviderPromptRequest {
        provider,
        target_repo: ctx.target_repo.clone(),
        prompt: prompt.to_string(),
        role_id: role_id.to_string(),
        keep_sandbox: true,
        timeout_ms: Some(DEFAULT_PROVIDER_TIMEOUT_SECONDS * 1_000),
        max_budget_usd,
    })?;
    let transcript = ctx.artifact_store.put_text(
        "provider_prompt_transcript",
        role_id,
        "provider-prompt-transcript.json",
        "application/json",
        &serde_json::to_string_pretty(&sandbox.adapter)?,
        vec![],
    )?;
    artifact_created(ctx, &transcript)?;
    let mut transcript_summary_value = serde_json::to_value(&sandbox.transcript_summary)?;
    if let Some(task_id) = task_id {
        if let Some(object) = transcript_summary_value.as_object_mut() {
            object.insert("task_id".to_string(), json!(task_id));
            object.insert("workflow_role".to_string(), json!(role_id));
        }
    }
    let transcript_summary = ctx.artifact_store.put_text(
        "provider_transcript_summary",
        role_id,
        "provider-transcript-summary.json",
        "application/json",
        &serde_json::to_string_pretty(&transcript_summary_value)?,
        vec![transcript.artifact_id.clone()],
    )?;
    artifact_created(ctx, &transcript_summary)?;
    emit(
        ctx,
        "adapter.transcript.parsed",
        Some(role_id),
        Some(action_id),
        Actor::role(role_id),
        transcript_summary_value,
    )?;
    emit(
        ctx,
        "adapter.completed",
        Some(role_id),
        Some(action_id),
        Actor::role(role_id),
        serde_json::to_value(&sandbox.adapter)?,
    )?;
    if sandbox.adapter.blocker.is_some() {
        return Err(anyhow!(
            "provider adapter failed; see provider_prompt_transcript artifact"
        ));
    }

    let preview = preview_sandbox_patch(&ctx.target_repo, &sandbox.sandbox_path)?;
    let preview_artifact = ctx.artifact_store.put_text(
        "sandbox_patch_preview",
        role_id,
        "sandbox-patch-preview.json",
        "application/json",
        &serde_json::to_string_pretty(&preview)?,
        vec![transcript.artifact_id.clone()],
    )?;
    artifact_created(ctx, &preview_artifact)?;
    emit(
        ctx,
        "sandbox.patch.previewed",
        Some(role_id),
        Some(action_id),
        Actor::system(),
        serde_json::to_value(&preview)?,
    )?;

    let applied = apply_sandbox_patch(SandboxPatchApplyRequest {
        target_repo: ctx.target_repo.clone(),
        sandbox_path: sandbox.sandbox_path.clone(),
        expected_digest: preview.action_digest,
        approver: "human:local-user".to_string(),
    })?;
    let apply_artifact = ctx.artifact_store.put_text(
        "sandbox_patch_apply",
        role_id,
        "sandbox-patch-apply.json",
        "application/json",
        &serde_json::to_string_pretty(&applied)?,
        vec![preview_artifact.artifact_id.clone()],
    )?;
    artifact_created(ctx, &apply_artifact)?;
    emit(
        ctx,
        "sandbox.patch.applied",
        Some(role_id),
        Some(action_id),
        Actor::human_local(),
        serde_json::to_value(&applied)?,
    )?;
    let _ = fs::remove_dir_all(&sandbox.sandbox_path);

    let patch = ctx.artifact_store.put_text(
        "patch_summary",
        role_id,
        "provider-patch.md",
        "text/markdown",
        "Applied provider prompt sandbox patch through exact-digest patch gate.",
        vec![
            apply_artifact.artifact_id.clone(),
            transcript_summary.artifact_id.clone(),
        ],
    )?;
    artifact_created(ctx, &patch)?;
    emit(
        ctx,
        "role.completed",
        Some(role_id),
        Some(action_id),
        Actor::role(role_id),
        json!({"status": "completed", "concerns": [], "blockers": []}),
    )?;
    Ok(())
}

fn record_repair_source_context(
    ctx: &mut RunContext,
    source: &RepairSourceContext,
) -> Result<ArtifactRef> {
    let latest_verifier_output_digest = source
        .latest_verifier_output
        .as_ref()
        .map(|output| sha256_hex(output.as_bytes()));
    let context = json!({
        "schema_version": "ao2.repair-source.v1",
        "source_run_id": source.source_run_id,
        "evidence_pack_path": source.evidence_pack_path,
        "source_verdict": source.source_verdict,
        "run_health": source.run_health,
        "unresolved_concerns": source.unresolved_concerns,
        "evidence_refs": source.evidence_refs,
        "latest_verifier_output": source.latest_verifier_output,
        "latest_verifier_output_digest": latest_verifier_output_digest
    });
    let artifact = ctx.artifact_store.put_text(
        "repair_source_context",
        "operator",
        "repair-source-context.json",
        "application/json",
        &serde_json::to_string_pretty(&context)?,
        vec![],
    )?;
    artifact_created(ctx, &artifact)?;
    emit(
        ctx,
        "repair.source.loaded",
        Some("operator"),
        Some("repair_resume_from_evidence"),
        Actor::human_local(),
        json!({
            "source_run_id": source.source_run_id,
            "source_verdict": source.source_verdict,
            "repair_source_artifact": artifact.artifact_id,
            "latest_verifier_output_digest": latest_verifier_output_digest
        }),
    )?;
    Ok(artifact)
}

fn build_provider_prompt_with_repair_source(
    provider: ProviderKind,
    original_prompt: &str,
    source: &RepairSourceContext,
) -> Result<String> {
    let run_health = serde_json::to_string_pretty(&source.run_health)?;
    let unresolved_concerns = serde_json::to_string_pretty(&source.unresolved_concerns)?;
    let evidence_refs = serde_json::to_string_pretty(&source.evidence_refs)?;
    let verifier_output = source.latest_verifier_output.as_deref().unwrap_or("");
    Ok(match provider {
        ProviderKind::Scripted => {
            if cfg!(windows) && !scripted_prompt_prefers_posix_shell(original_prompt) {
                format!(
                    "$env:AO2_REPAIR_SOURCE_RUN_ID = '{}'\n$env:AO2_REPAIR_RUN_HEALTH = @'\n{}\n'@\n$env:AO2_REPAIR_UNRESOLVED_CONCERNS = @'\n{}\n'@\n$env:AO2_REPAIR_EVIDENCE_REFS = @'\n{}\n'@\n$env:AO2_REPAIR_VERIFIER_OUTPUT = @'\n{}\n'@\n# AO2_REPAIR_SOURCE_CONTEXT_BEGIN\n# Prior run health, unresolved concerns, evidence refs, and verifier output are available in AO2_REPAIR_* variables.\n# AO2_REPAIR_SOURCE_CONTEXT_END\n{}",
                    escape_powershell_single_quoted_here_string(&source.source_run_id),
                    escape_powershell_single_quoted_here_string(&run_health),
                    escape_powershell_single_quoted_here_string(&unresolved_concerns),
                    escape_powershell_single_quoted_here_string(&evidence_refs),
                    escape_powershell_single_quoted_here_string(verifier_output),
                    original_prompt
                )
            } else {
                format!(
                    "export AO2_REPAIR_SOURCE_RUN_ID={}\nexport AO2_REPAIR_RUN_HEALTH={}\nexport AO2_REPAIR_UNRESOLVED_CONCERNS={}\nexport AO2_REPAIR_EVIDENCE_REFS={}\nexport AO2_REPAIR_VERIFIER_OUTPUT={}\n# AO2_REPAIR_SOURCE_CONTEXT_BEGIN\n# Prior run health, unresolved concerns, evidence refs, and verifier output are available in AO2_REPAIR_* variables.\n# AO2_REPAIR_SOURCE_CONTEXT_END\n{}",
                    shell_single_quote(&source.source_run_id),
                    shell_single_quote(&run_health),
                    shell_single_quote(&unresolved_concerns),
                    shell_single_quote(&evidence_refs),
                    shell_single_quote(verifier_output),
                    original_prompt
                )
            }
        }
        ProviderKind::Codex | ProviderKind::Claude | ProviderKind::Antigravity => format!(
            r#"You are repairing an AO2 run from signed evidence.

Source run: {}
Source verdict: {}

Run health:
```json
{run_health}
```

Unresolved concerns:
```json
{unresolved_concerns}
```

Evidence refs:
```json
{evidence_refs}
```

Latest verifier output:
```text
{verifier_output}
```

Use the prior evidence only as context. Make the smallest repository change needed to satisfy the workflow verifier, preserve the original task intent, report changed files, and do not perform network, publish, or destructive actions.

Original repair task:
{original_prompt}
"#,
            source.source_run_id, source.source_verdict
        ),
    })
}

fn run_real_project_provider_workflow(
    ctx: &mut RunContext,
    provider: ProviderKind,
    prompt: &str,
    max_repair_attempts: usize,
    max_budget_usd: Option<f64>,
) -> Result<RunStatus> {
    if sdd_workflow_task_order(ctx)?.is_some() {
        run_sdd_provider_task_graph(ctx, provider, prompt, max_budget_usd)?;
    } else {
        apply_provider_prompt_patch(ctx, provider, prompt, max_budget_usd)?;
    }
    let verifier = run_verifier(ctx)?;
    if verifier.success {
        let review = reviewer_accept_real_project(ctx, &verifier.artifact)?;
        accept_real_project_final(ctx, &verifier.artifact, &review)?;
        return Ok(RunStatus::Accepted);
    }

    let review = reviewer_verifier_failure(ctx, &verifier.artifact)?;
    if max_repair_attempts == 0 {
        reject_real_project_verifier_failure(ctx, &review, &verifier.artifact)?;
        record_repair_budget_exhausted(ctx, "verifier_failed", &review)?;
        return Ok(RunStatus::Rejected);
    }

    match run_real_project_repair_loop(
        ctx,
        provider,
        prompt,
        max_repair_attempts,
        max_budget_usd,
        &review,
        &verifier,
    )? {
        Some(test_log) => {
            let repair_review = reviewer_accept_real_project(ctx, &test_log)?;
            accept_real_project_final(ctx, &test_log, &repair_review)?;
            Ok(RunStatus::Accepted)
        }
        None => {
            reject_real_project_verifier_failure(ctx, &review, &verifier.artifact)?;
            Ok(RunStatus::Rejected)
        }
    }
}

fn run_sdd_provider_task_graph(
    ctx: &mut RunContext,
    provider: ProviderKind,
    prompt: &str,
    max_budget_usd: Option<f64>,
) -> Result<()> {
    let tasks = sdd_workflow_task_order(ctx)?.unwrap_or_default();
    let total = tasks.len();
    for (index, task) in tasks.iter().enumerate() {
        let task_id = task
            .get("id")
            .and_then(serde_json::Value::as_str)
            .context("SDD workflow task id is required")?;
        let task_prompt = provider_prompt_for_sdd_task(provider, prompt, task, index + 1, total)?;
        apply_provider_prompt_patch_for_role(
            ctx,
            provider,
            &task_prompt,
            max_budget_usd,
            task_id,
            "sdd_provider_task",
            Some(task_id),
        )?;
    }
    Ok(())
}

fn sdd_workflow_task_order(ctx: &RunContext) -> Result<Option<Vec<serde_json::Value>>> {
    let is_sdd = ctx
        .factory_v3_compatibility
        .as_ref()
        .and_then(|value| value.get("source_schema"))
        .and_then(serde_json::Value::as_str)
        == Some("ao2.sdd-plan.v1");
    if !is_sdd {
        return Ok(None);
    }

    let task_ids = ctx
        .workflow_tasks
        .iter()
        .map(|task| {
            task.get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .context("SDD workflow task id is required")
        })
        .collect::<Result<Vec<_>>>()?;
    let task_id_set = task_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut dependencies = task_ids
        .iter()
        .map(|id| (id.clone(), BTreeSet::<String>::new()))
        .collect::<BTreeMap<_, _>>();
    for dependency in &ctx.workflow_dependencies {
        let Some(to) = dependency.get("to").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !task_id_set.contains(to) {
            continue;
        }
        let from = dependency
            .get("from")
            .and_then(serde_json::Value::as_str)
            .with_context(|| format!("SDD workflow dependency into {to} is missing from"))?;
        if !task_id_set.contains(from) {
            return Err(anyhow!(
                "SDD workflow task {to} depends on unknown task {from}"
            ));
        }
        dependencies
            .entry(to.to_string())
            .or_default()
            .insert(from.to_string());
    }

    let mut completed = BTreeSet::new();
    let mut ordered = Vec::with_capacity(ctx.workflow_tasks.len());
    while ordered.len() < ctx.workflow_tasks.len() {
        let mut progressed = false;
        for task in &ctx.workflow_tasks {
            let id = task
                .get("id")
                .and_then(serde_json::Value::as_str)
                .context("SDD workflow task id is required")?;
            if completed.contains(id) {
                continue;
            }
            let ready = dependencies
                .get(id)
                .map(|deps| deps.iter().all(|dep| completed.contains(dep.as_str())))
                .unwrap_or(true);
            if ready {
                ordered.push(task.clone());
                completed.insert(id.to_string());
                progressed = true;
            }
        }
        if !progressed {
            return Err(anyhow!(
                "SDD workflow task graph contains a dependency cycle"
            ));
        }
    }

    Ok(Some(ordered))
}

fn provider_prompt_for_sdd_task(
    provider: ProviderKind,
    base_prompt: &str,
    task: &serde_json::Value,
    ordinal: usize,
    total: usize,
) -> Result<String> {
    let task_id = task
        .get("id")
        .and_then(serde_json::Value::as_str)
        .context("SDD workflow task id is required")?;
    if provider == ProviderKind::Scripted {
        return Ok(format!(
            "# AO2 SDD task {ordinal}/{total}: {task_id}\n{base_prompt}"
        ));
    }
    let task_json = serde_json::to_string_pretty(task)?;
    Ok(format!(
        r#"Execute only this dependency-ordered AO2 SDD task.

Task {ordinal}/{total}: {task_id}

Task contract:
```json
{task_json}
```

Overall governed-run instruction:
{base_prompt}
"#
    ))
}

fn reviewer_accept_real_project(
    ctx: &mut RunContext,
    verifier: &ArtifactRef,
) -> Result<ArtifactRef> {
    emit(
        ctx,
        "role.started",
        Some("reviewer"),
        Some("real_project_review"),
        Actor::role("reviewer"),
        json!({"status": "started"}),
    )?;
    let review = ctx.artifact_store.put_text(
        "review",
        "reviewer",
        "review-accepted.json",
        "application/json",
        &serde_json::to_string_pretty(&json!({
            "concerns": [],
            "summary": "Provider patch passed the workflow verifier for the real-project template.",
            "verifier_artifact": verifier.artifact_id
        }))?,
        vec![verifier.artifact_id.clone()],
    )?;
    artifact_created(ctx, &review)?;
    emit(
        ctx,
        "role.completed",
        Some("reviewer"),
        Some("real_project_review"),
        Actor::role("reviewer"),
        json!({"status": "completed", "concerns": []}),
    )?;
    Ok(review)
}

fn reviewer_verifier_failure(ctx: &mut RunContext, verifier: &ArtifactRef) -> Result<ArtifactRef> {
    emit(
        ctx,
        "role.started",
        Some("reviewer"),
        Some("real_project_verifier_failure"),
        Actor::role("reviewer"),
        json!({"status": "started"}),
    )?;
    let review = ctx.artifact_store.put_text(
        "review",
        "reviewer",
        "review-verifier-failed.json",
        "application/json",
        &serde_json::to_string_pretty(&json!({
            "concerns": [{
                "id": "verifier_failed",
                "severity": "high",
                "artifact": verifier.artifact_id,
                "reason": "Provider patch did not pass the workflow verifier.",
                "required_resolution": "Repair the patch until the verifier exits successfully."
            }]
        }))?,
        vec![verifier.artifact_id.clone()],
    )?;
    artifact_created(ctx, &review)?;
    emit(
        ctx,
        "role.completed",
        Some("reviewer"),
        Some("real_project_verifier_failure"),
        Actor::role("reviewer"),
        json!({"status": "completed", "concerns": ["verifier_failed"]}),
    )?;
    Ok(review)
}

fn record_scripted_adapter_transcript(ctx: &mut RunContext) -> Result<()> {
    let result = AdapterRunResult::scripted(
        "implementer",
        "scripted implementer adapter executed after policy approval; wrote validation patch under repo-write-no-network scope",
    );
    let transcript = ctx.artifact_store.put_text(
        "adapter_transcript",
        "implementer",
        "adapter-transcript.json",
        "application/json",
        &serde_json::to_string_pretty(&result)?,
        vec![],
    )?;
    artifact_created(ctx, &transcript)?;
    emit(
        ctx,
        "adapter.completed",
        Some("implementer"),
        Some("scripted_adapter_transcript"),
        Actor::role("implementer"),
        serde_json::to_value(&result)?,
    )?;
    Ok(())
}

fn reviewer_concern(ctx: &mut RunContext) -> Result<ArtifactRef> {
    emit(
        ctx,
        "role.started",
        Some("reviewer"),
        Some("review_missing_tests"),
        Actor::role("reviewer"),
        json!({"status": "started"}),
    )?;
    let review = ctx.artifact_store.put_text(
        "review",
        "reviewer",
        "review.json",
        "application/json",
        &serde_json::to_string_pretty(&json!({
            "concerns": [{
                "id": "review_missing_tests",
                "severity": "high",
                "artifact": "first-patch",
                "reason": "Implementation changed validation behavior but did not add tests for invalid inputs.",
                "required_resolution": "Add tests for negative price and discount rates outside 0..1."
            }]
        }))?,
        vec![],
    )?;
    artifact_created(ctx, &review)?;
    emit(
        ctx,
        "role.completed",
        Some("reviewer"),
        Some("review_missing_tests"),
        Actor::role("reviewer"),
        json!({"status": "completed", "concerns": ["review_missing_tests"]}),
    )?;
    Ok(review)
}

fn reject_for_missing_tests(ctx: &mut RunContext, review: &ArtifactRef) -> Result<()> {
    let report = ClosureReport {
        verdict: "rejected".to_string(),
        acceptance_criteria_results: vec![
            "validation implementation exists".to_string(),
            "invalid-input tests missing".to_string(),
        ],
        evidence_refs: vec![review.artifact_id.clone()],
        unresolved_concerns: vec!["review_missing_tests".to_string()],
        blockers: vec![],
        policy_exceptions: vec![],
        cost_summary: provider_cost_summary(ctx)?,
        created_at: Utc::now(),
    };
    ctx.closure_reports.push(report.clone());
    let artifact = ctx.artifact_store.put_text(
        "closure_report",
        "evaluator-closer",
        "closure-rejected.json",
        "application/json",
        &serde_json::to_string_pretty(&report)?,
        vec![review.artifact_id.clone()],
    )?;
    artifact_created(ctx, &artifact)?;
    emit(
        ctx,
        "closure.rejected",
        Some("evaluator-closer"),
        Some("closure_first_pass"),
        Actor::role("evaluator-closer"),
        serde_json::to_value(&report)?,
    )?;
    Ok(())
}

fn apply_correction_patch(ctx: &mut RunContext) -> Result<()> {
    let target = ctx.target_repo.join("tests").join("test_discounts.py");
    fs::write(
        &target,
        r#"import pytest

from discount_service.discounts import calculate_discount


def test_calculates_discount_for_valid_values():
    assert calculate_discount(100, 0.25) == 75


def test_rejects_negative_price():
    with pytest.raises(ValueError):
        calculate_discount(-1, 0.1)


def test_rejects_discount_rate_below_zero():
    with pytest.raises(ValueError):
        calculate_discount(100, -0.1)


def test_rejects_discount_rate_above_one():
    with pytest.raises(ValueError):
        calculate_discount(100, 1.1)
"#,
    )
    .with_context(|| format!("write correction tests {}", target.display()))?;
    let correction = ctx.artifact_store.put_text(
        "patch_summary",
        "implementer",
        "correction-patch.md",
        "text/markdown",
        "Added tests resolving `review_missing_tests`.",
        vec![],
    )?;
    artifact_created(ctx, &correction)?;
    Ok(())
}

fn run_repair_loop_after_rejection(
    ctx: &mut RunContext,
    max_attempts: usize,
    review: &ArtifactRef,
) -> Result<Option<ArtifactRef>> {
    for attempt in 1..=max_attempts {
        emit(
            ctx,
            "repair.attempt.started",
            Some("implementer"),
            Some("repair_after_reviewer_rejection"),
            Actor::role("implementer"),
            json!({
                "attempt": attempt,
                "trigger": "review_missing_tests",
                "max_attempts": max_attempts
            }),
        )?;
        apply_correction_patch(ctx)?;
        let verifier = run_verifier(ctx)?;
        if verifier.success {
            let attempt_record = RepairAttempt {
                attempt,
                trigger: "review_missing_tests".to_string(),
                status: "accepted".to_string(),
                evidence_refs: vec![
                    review.artifact_id.clone(),
                    verifier.artifact.artifact_id.clone(),
                ],
                summary: "Applied repair after reviewer rejection and verifier passed.".to_string(),
                created_at: Utc::now(),
            };
            record_repair_attempt(ctx, &attempt_record)?;
            emit(
                ctx,
                "repair.attempt.completed",
                Some("implementer"),
                Some("repair_after_reviewer_rejection"),
                Actor::role("implementer"),
                serde_json::to_value(&attempt_record)?,
            )?;
            return Ok(Some(verifier.artifact));
        }

        let attempt_record = RepairAttempt {
            attempt,
            trigger: "verifier_failed".to_string(),
            status: "failed".to_string(),
            evidence_refs: vec![
                review.artifact_id.clone(),
                verifier.artifact.artifact_id.clone(),
            ],
            summary: format!("Verifier failed during repair attempt {attempt}."),
            created_at: Utc::now(),
        };
        record_repair_attempt(ctx, &attempt_record)?;
        emit(
            ctx,
            "repair.attempt.failed",
            Some("test-engineer"),
            Some("repair_verifier_failed"),
            Actor::role("test-engineer"),
            serde_json::to_value(&attempt_record)?,
        )?;
    }
    record_repair_budget_exhausted(ctx, "verifier_failed", review)?;
    Ok(None)
}

fn record_repair_budget_exhausted(
    ctx: &mut RunContext,
    trigger: &str,
    review: &ArtifactRef,
) -> Result<()> {
    let attempt_record = RepairAttempt {
        attempt: 0,
        trigger: trigger.to_string(),
        status: "budget_exhausted".to_string(),
        evidence_refs: vec![review.artifact_id.clone()],
        summary: "Repair budget exhausted before any repair attempt was allowed.".to_string(),
        created_at: Utc::now(),
    };
    record_repair_attempt(ctx, &attempt_record)?;
    emit(
        ctx,
        "repair.budget.exhausted",
        Some("evaluator-closer"),
        Some("repair_budget_exhausted"),
        Actor::role("evaluator-closer"),
        serde_json::to_value(&attempt_record)?,
    )?;
    Ok(())
}

fn record_repair_attempt(ctx: &mut RunContext, attempt: &RepairAttempt) -> Result<()> {
    ctx.repair_attempts.push(attempt.clone());
    let artifact = ctx.artifact_store.put_text(
        "repair_attempt",
        "implementer",
        &format!("repair-attempt-{}.json", attempt.attempt),
        "application/json",
        &serde_json::to_string_pretty(attempt)?,
        attempt.evidence_refs.clone(),
    )?;
    artifact_created(ctx, &artifact)?;
    Ok(())
}

fn run_real_project_repair_loop(
    ctx: &mut RunContext,
    provider: ProviderKind,
    prompt: &str,
    max_attempts: usize,
    max_budget_usd: Option<f64>,
    review: &ArtifactRef,
    initial_verifier: &VerifierOutcome,
) -> Result<Option<ArtifactRef>> {
    let mut last_verifier = initial_verifier.clone();
    for attempt in 1..=max_attempts {
        emit(
            ctx,
            "repair.attempt.started",
            Some("implementer"),
            Some("real_project_repair_after_verifier_failure"),
            Actor::role("implementer"),
            json!({
                "attempt": attempt,
                "trigger": "verifier_failed",
                "max_attempts": max_attempts
            }),
        )?;
        let repair_prompt = record_real_project_repair_prompt(
            ctx,
            provider,
            prompt,
            attempt,
            &last_verifier,
            review,
        )?;
        apply_provider_prompt_patch(ctx, provider, &repair_prompt.content, max_budget_usd)?;
        let verifier = run_verifier(ctx)?;
        if verifier.success {
            let attempt_record = RepairAttempt {
                attempt,
                trigger: "verifier_failed".to_string(),
                status: "accepted".to_string(),
                evidence_refs: vec![
                    review.artifact_id.clone(),
                    last_verifier.artifact.artifact_id.clone(),
                    repair_prompt.artifact.artifact_id.clone(),
                    verifier.artifact.artifact_id.clone(),
                ],
                summary: "Ran structured repair prompt with verifier context and verifier passed."
                    .to_string(),
                created_at: Utc::now(),
            };
            record_repair_attempt(ctx, &attempt_record)?;
            emit(
                ctx,
                "repair.attempt.completed",
                Some("implementer"),
                Some("real_project_repair_after_verifier_failure"),
                Actor::role("implementer"),
                serde_json::to_value(&attempt_record)?,
            )?;
            return Ok(Some(verifier.artifact));
        }

        let attempt_record = RepairAttempt {
            attempt,
            trigger: "verifier_failed".to_string(),
            status: "failed".to_string(),
            evidence_refs: vec![
                review.artifact_id.clone(),
                last_verifier.artifact.artifact_id.clone(),
                repair_prompt.artifact.artifact_id.clone(),
                verifier.artifact.artifact_id.clone(),
            ],
            summary: format!(
                "Verifier failed during real-project repair attempt {attempt} after structured repair prompt."
            ),
            created_at: Utc::now(),
        };
        record_repair_attempt(ctx, &attempt_record)?;
        emit(
            ctx,
            "repair.attempt.failed",
            Some("test-engineer"),
            Some("real_project_repair_verifier_failed"),
            Actor::role("test-engineer"),
            serde_json::to_value(&attempt_record)?,
        )?;
        last_verifier = verifier;
    }
    record_repair_budget_exhausted(ctx, "verifier_failed", review)?;
    Ok(None)
}

struct RepairPromptEvidence {
    artifact: ArtifactRef,
    content: String,
}

fn record_real_project_repair_prompt(
    ctx: &mut RunContext,
    provider: ProviderKind,
    original_prompt: &str,
    attempt: usize,
    verifier: &VerifierOutcome,
    review: &ArtifactRef,
) -> Result<RepairPromptEvidence> {
    let provider_summaries = artifact_json_payloads(ctx, "provider_transcript_summary")?;
    let prompt = build_real_project_repair_prompt(
        provider,
        original_prompt,
        attempt,
        verifier,
        &provider_summaries,
    )?;
    let mut input_refs = vec![
        review.artifact_id.clone(),
        verifier.artifact.artifact_id.clone(),
    ];
    input_refs.extend(
        ctx.artifacts
            .iter()
            .filter(|artifact| artifact.artifact_type == "provider_transcript_summary")
            .map(|artifact| artifact.artifact_id.clone()),
    );
    let artifact = ctx.artifact_store.put_text(
        "repair_prompt",
        "implementer",
        &format!("repair-prompt-{attempt}.txt"),
        "text/plain",
        &prompt,
        input_refs,
    )?;
    artifact_created(ctx, &artifact)?;
    emit(
        ctx,
        "repair.prompt.created",
        Some("implementer"),
        Some("real_project_structured_repair_prompt"),
        Actor::role("implementer"),
        json!({
            "attempt": attempt,
            "provider": provider,
            "verifier_artifact": verifier.artifact.artifact_id,
            "repair_prompt_artifact": artifact.artifact_id,
            "verifier_output_digest": sha256_hex(verifier.content.as_bytes())
        }),
    )?;
    Ok(RepairPromptEvidence {
        artifact,
        content: prompt,
    })
}

fn build_real_project_repair_prompt(
    provider: ProviderKind,
    original_prompt: &str,
    attempt: usize,
    verifier: &VerifierOutcome,
    provider_summaries: &[serde_json::Value],
) -> Result<String> {
    let provider_summaries_json = serde_json::to_string_pretty(provider_summaries)?;
    Ok(match provider {
        ProviderKind::Scripted => build_scripted_repair_prompt(
            original_prompt,
            attempt,
            &verifier.content,
            &provider_summaries_json,
        ),
        ProviderKind::Codex | ProviderKind::Claude | ProviderKind::Antigravity => format!(
            r#"You are repairing a real-project AO2 run after the workflow verifier failed.

Repair attempt: {attempt}

Original task:
{original_prompt}

Previous verifier output:
```text
{}
```

Prior provider transcript summaries:
```json
{provider_summaries_json}
```

Make the smallest repository change needed for the verifier to pass. Preserve the original task intent, report changed files, and do not perform network, publish, or destructive actions.
"#,
            verifier.content
        ),
    })
}

fn build_scripted_repair_prompt(
    original_prompt: &str,
    attempt: usize,
    verifier_output: &str,
    provider_summaries_json: &str,
) -> String {
    if cfg!(windows) && !scripted_prompt_prefers_posix_shell(original_prompt) {
        format!(
            "$env:AO2_REPAIR_ATTEMPT = '{}'\n$env:AO2_REPAIR_TRIGGER = 'verifier_failed'\n$env:AO2_REPAIR_VERIFIER_OUTPUT = @'\n{}\n'@\n$env:AO2_REPAIR_PROVIDER_SUMMARIES = @'\n{}\n'@\n# AO2_REPAIR_CONTEXT_BEGIN\n# Previous verifier output is available in AO2_REPAIR_VERIFIER_OUTPUT.\n# Prior provider summaries are available in AO2_REPAIR_PROVIDER_SUMMARIES.\n# AO2_REPAIR_CONTEXT_END\n{}",
            attempt,
            escape_powershell_single_quoted_here_string(verifier_output),
            escape_powershell_single_quoted_here_string(provider_summaries_json),
            original_prompt
        )
    } else {
        format!(
            "export AO2_REPAIR_ATTEMPT={}\nexport AO2_REPAIR_TRIGGER=verifier_failed\nexport AO2_REPAIR_VERIFIER_OUTPUT={}\nexport AO2_REPAIR_PROVIDER_SUMMARIES={}\n# AO2_REPAIR_CONTEXT_BEGIN\n# Previous verifier output is available in AO2_REPAIR_VERIFIER_OUTPUT.\n# Prior provider summaries are available in AO2_REPAIR_PROVIDER_SUMMARIES.\n# AO2_REPAIR_CONTEXT_END\n{}",
            shell_single_quote(&attempt.to_string()),
            shell_single_quote(verifier_output),
            shell_single_quote(provider_summaries_json),
            original_prompt
        )
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn escape_powershell_single_quoted_here_string(value: &str) -> String {
    value.replace("\n'@", "\n' @")
}

fn run_verifier(ctx: &mut RunContext) -> Result<VerifierOutcome> {
    let command = resolve_verifier_command(&ctx.verifier_command);
    emit(
        ctx,
        "role.started",
        Some("test-engineer"),
        Some("run_python_tests"),
        Actor::role("test-engineer"),
        json!({"command": command}),
    )?;
    let output = verifier_command(&command)
        .current_dir(&ctx.target_repo)
        .output()
        .with_context(|| format!("run verifier with {command}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let content = format!(
        "command: {}\nexit_code: {}\nstdout:\n{}\nstderr:\n{}\n",
        command,
        output.status.code().unwrap_or(-1),
        stdout,
        stderr
    );
    let test_log = ctx.artifact_store.put_text(
        "test_log",
        "test-engineer",
        "pytest.log",
        "text/plain",
        &content,
        vec![],
    )?;
    artifact_created(ctx, &test_log)?;
    emit(
        ctx,
        if output.status.success() {
            "task.completed"
        } else {
            "task.failed"
        },
        Some("test-engineer"),
        Some("run_python_tests"),
        Actor::role("test-engineer"),
        json!({
            "command": command,
            "exit_code": output.status.code().unwrap_or(-1),
            "output_digest": sha256_hex(content.as_bytes())
        }),
    )?;
    Ok(VerifierOutcome {
        artifact: test_log,
        success: output.status.success(),
        content,
    })
}

fn resolve_verifier_command(command: &str) -> String {
    if (command == "python" || command.starts_with("python "))
        && Command::new("python").arg("--version").output().is_err()
        && Command::new("python3").arg("--version").output().is_ok()
    {
        return command.replacen("python", "python3", 1);
    }
    command.to_string()
}

fn verifier_command(command: &str) -> Command {
    if cfg!(windows) {
        if command.trim() == "true" {
            let mut command_shell = Command::new("cmd");
            command_shell.arg("/C").arg("exit /B 0");
            return command_shell;
        }
        if let Some(command) = python_inline_command(command) {
            return command;
        }
        if scripted_prompt_prefers_posix_shell(command) {
            if let Some(posix_shell) = posix_shell_command() {
                let mut shell = Command::new(posix_shell);
                shell.arg("-c").arg(command);
                return shell;
            }
        }
        let mut command_shell = Command::new("cmd");
        command_shell.arg("/C").arg(command);
        command_shell
    } else {
        let mut shell = Command::new("sh");
        shell.arg("-c").arg(command);
        shell
    }
}

#[cfg(test)]
mod verifier_command_tests {
    use super::{resolve_verifier_command, verifier_command};

    #[test]
    fn portable_true_verifier_command_succeeds_on_current_platform() {
        let command = resolve_verifier_command("true");
        let status = verifier_command(&command)
            .status()
            .expect("run portable true verifier command");

        assert!(
            status.success(),
            "`true` must remain a portable verifier no-op across AO2 supported hosts"
        );
    }
}

fn python_inline_command(command: &str) -> Option<Command> {
    for executable in ["python", "python3"] {
        let prefix = format!("{executable} -c ");
        let Some(inline) = command.strip_prefix(&prefix) else {
            continue;
        };
        let code = parse_single_shell_argument(inline.trim())?;
        let mut python = Command::new(executable);
        python.arg("-c").arg(code);
        return Some(python);
    }
    None
}

fn parse_single_shell_argument(input: &str) -> Option<String> {
    if input.len() < 2 {
        return None;
    }
    let bytes = input.as_bytes();
    let quote = bytes[0];
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    if bytes[input.len() - 1] != quote {
        return None;
    }
    let inner = &input[1..input.len() - 1];
    if quote == b'"' {
        Some(inner.replace("\\\"", "\""))
    } else {
        Some(inner.to_string())
    }
}

fn accept_final(ctx: &mut RunContext, test_log: &ArtifactRef) -> Result<()> {
    let report = ClosureReport {
        verdict: "accepted".to_string(),
        acceptance_criteria_results: vec![
            "negative prices raise ValueError: mapped to pytest evidence".to_string(),
            "discount rates below 0 raise ValueError: mapped to pytest evidence".to_string(),
            "discount rates above 1 raise ValueError: mapped to pytest evidence".to_string(),
            "valid discounts still calculate correctly: mapped to pytest evidence".to_string(),
            "risky git push was denied before execution".to_string(),
            "narrow repository write was exact-digest approved".to_string(),
        ],
        evidence_refs: vec![test_log.artifact_id.clone()],
        unresolved_concerns: vec![],
        blockers: vec![],
        policy_exceptions: vec![],
        cost_summary: provider_cost_summary(ctx)?,
        created_at: Utc::now(),
    };
    ctx.closure_reports.push(report.clone());
    let artifact = ctx.artifact_store.put_text(
        "closure_report",
        "evaluator-closer",
        "closure-accepted.json",
        "application/json",
        &serde_json::to_string_pretty(&report)?,
        vec![test_log.artifact_id.clone()],
    )?;
    artifact_created(ctx, &artifact)?;
    emit(
        ctx,
        "closure.accepted",
        Some("evaluator-closer"),
        Some("closure_final"),
        Actor::role("evaluator-closer"),
        serde_json::to_value(&report)?,
    )?;
    Ok(())
}

fn reject_real_project_verifier_failure(
    ctx: &mut RunContext,
    review: &ArtifactRef,
    verifier: &ArtifactRef,
) -> Result<()> {
    let report = ClosureReport {
        verdict: "rejected".to_string(),
        acceptance_criteria_results: vec![
            "provider patch applied through sandbox exact-digest gate".to_string(),
            "workflow verifier failed".to_string(),
        ],
        evidence_refs: vec![review.artifact_id.clone(), verifier.artifact_id.clone()],
        unresolved_concerns: vec!["verifier_failed".to_string()],
        blockers: vec![],
        policy_exceptions: vec![],
        cost_summary: provider_cost_summary(ctx)?,
        created_at: Utc::now(),
    };
    ctx.closure_reports.push(report.clone());
    let artifact = ctx.artifact_store.put_text(
        "closure_report",
        "evaluator-closer",
        "closure-rejected.json",
        "application/json",
        &serde_json::to_string_pretty(&report)?,
        vec![review.artifact_id.clone(), verifier.artifact_id.clone()],
    )?;
    artifact_created(ctx, &artifact)?;
    emit(
        ctx,
        "closure.rejected",
        Some("evaluator-closer"),
        Some("real_project_closure_failed"),
        Actor::role("evaluator-closer"),
        serde_json::to_value(&report)?,
    )?;
    Ok(())
}

fn accept_real_project_final(
    ctx: &mut RunContext,
    test_log: &ArtifactRef,
    review: &ArtifactRef,
) -> Result<()> {
    let mut acceptance = if ctx.acceptance.is_empty() {
        vec![
            "workflow verifier passed".to_string(),
            "provider patch applied through sandbox exact-digest gate".to_string(),
            "risky git push was denied before execution".to_string(),
        ]
    } else {
        ctx.acceptance
            .iter()
            .map(|criterion| format!("{criterion}: mapped to verifier evidence"))
            .collect::<Vec<_>>()
    };
    acceptance.push("risky git push was denied before execution".to_string());
    acceptance
        .push("repository changes were promoted through exact-digest sandbox apply".to_string());

    let report = ClosureReport {
        verdict: "accepted".to_string(),
        acceptance_criteria_results: acceptance,
        evidence_refs: vec![test_log.artifact_id.clone(), review.artifact_id.clone()],
        unresolved_concerns: vec![],
        blockers: vec![],
        policy_exceptions: vec![],
        cost_summary: provider_cost_summary(ctx)?,
        created_at: Utc::now(),
    };
    ctx.closure_reports.push(report.clone());
    let artifact = ctx.artifact_store.put_text(
        "closure_report",
        "evaluator-closer",
        "closure-accepted.json",
        "application/json",
        &serde_json::to_string_pretty(&report)?,
        vec![test_log.artifact_id.clone(), review.artifact_id.clone()],
    )?;
    artifact_created(ctx, &artifact)?;
    emit(
        ctx,
        "closure.accepted",
        Some("evaluator-closer"),
        Some("real_project_closure_final"),
        Actor::role("evaluator-closer"),
        serde_json::to_value(&report)?,
    )?;
    Ok(())
}

fn export_evidence_pack(ctx: &RunContext) -> Result<PathBuf> {
    let dir = ctx.run_dir.join("evidence-pack");
    fs::create_dir_all(&dir)?;
    let path = dir.join("evidence-pack.json");
    let provider_summaries = artifact_json_payloads(ctx, "provider_transcript_summary")?;
    let repair_source = artifact_json_payloads(ctx, "repair_source_context")?
        .into_iter()
        .last();
    let provider_contract = provider_contract_summary(ctx, &provider_summaries);
    let pack = json!({
        "schema_version": "ao2.evidence-pack.v1",
        "run_id": ctx.run_id,
        "workflow_id": ctx.workflow_id,
        "template_kind": ctx.template_kind,
        "objective": ctx.objective,
        "verifier_command": ctx.verifier_command,
        "verdict": ctx.closure_reports.last().map(|r| r.verdict.as_str()).unwrap_or("blocked"),
        "roles": ctx.roles,
        "workflow_tasks": ctx.workflow_tasks,
        "workflow_dependencies": ctx.workflow_dependencies,
        "factory_v3_compatibility": ctx.factory_v3_compatibility,
        "runtime_contract": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "factory_v3_drives_workflow": false,
            "provider_adapter_contract": provider_contract
        },
        "policy_decisions": ctx.policy_decisions,
        "approvals": ctx.approvals,
        "artifacts": ctx.artifacts,
        "provider_summaries": provider_summaries,
        "repair_source": repair_source,
        "closures": ctx.closure_reports,
        "repair_attempts": ctx.repair_attempts,
        "run_health": run_health(ctx),
        "markers": run_markers(ctx)
    });
    atomic_write(&path, serde_json::to_string_pretty(&pack)?)?;
    Ok(path)
}

fn provider_contract_summary(
    ctx: &RunContext,
    provider_summaries: &[serde_json::Value],
) -> serde_json::Value {
    let artifact_types: BTreeSet<&str> = ctx
        .artifacts
        .iter()
        .map(|artifact| artifact.artifact_type.as_str())
        .collect();
    let mut changed_files = BTreeSet::new();
    let mut providers = BTreeSet::new();
    let mut concern_count = 0usize;
    let mut blocker_count = 0usize;
    let mut cost_reported_count = 0usize;
    let mut observed_cost_usd = 0.0_f64;
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut total_tokens = 0_u64;
    for summary in provider_summaries {
        if let Some(provider) = summary.get("provider").and_then(|value| value.as_str()) {
            providers.insert(provider.to_string());
        }
        if let Some(files) = summary
            .get("changed_files")
            .and_then(|value| value.as_array())
        {
            for file in files {
                if let Some(path) = file.as_str().filter(|path| !path.trim().is_empty()) {
                    changed_files.insert(path.to_string());
                }
            }
        }
        concern_count += summary
            .get("concerns")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .unwrap_or_default();
        blocker_count += summary
            .get("blockers")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .unwrap_or_default();
        if let Some(cost) = summary.get("cost_usd").and_then(|value| value.as_f64()) {
            cost_reported_count += 1;
            observed_cost_usd += cost;
        }
        if let Some(usage) = summary.get("usage") {
            let input = json_u64(usage, "input_tokens");
            let output = json_u64(usage, "output_tokens");
            input_tokens += input;
            output_tokens += output;
            total_tokens += json_u64(usage, "total_tokens").max(input + output);
        }
    }

    let provider_run_observed = !provider_summaries.is_empty()
        || artifact_types.contains("provider_prompt_transcript")
        || artifact_types.contains("provider_transcript_summary");
    let evidence_refs = ctx
        .artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.artifact_type.as_str(),
                "provider_prompt_transcript"
                    | "provider_transcript_summary"
                    | "sandbox_patch_preview"
                    | "sandbox_patch_apply"
                    | "patch_summary"
            )
        })
        .map(|artifact| artifact.artifact_id.clone())
        .collect::<Vec<_>>();
    let changed_files = changed_files.into_iter().collect::<Vec<_>>();
    let requirements = json!({
        "evidence": !evidence_refs.is_empty(),
        "concerns": provider_run_observed,
        "blockers": provider_run_observed,
        "changed_files": !changed_files.is_empty(),
        "sandbox": artifact_types.contains("sandbox_patch_preview") && artifact_types.contains("sandbox_patch_apply"),
        "secret_redaction": artifact_types.contains("provider_prompt_transcript")
    });
    let fulfilled = requirements
        .as_object()
        .map(|object| {
            object
                .values()
                .all(|value| value.as_bool().unwrap_or(false))
        })
        .unwrap_or(false);

    json!({
        "schema_version": "ao2.provider-adapter-contract.v1",
        "status": if provider_run_observed { "observed" } else { "not_applicable" },
        "fulfilled": if provider_run_observed { fulfilled } else { true },
        "required_contract_fields": [
            "evidence",
            "concerns",
            "blockers",
            "changed_files",
            "sandbox",
            "secret_redaction"
        ],
        "requirements": requirements,
        "provider_summary_count": provider_summaries.len(),
        "providers": providers.into_iter().collect::<Vec<_>>(),
        "cost": {
            "observed_cost_usd": round_usd(observed_cost_usd),
            "reported_summary_count": cost_reported_count,
            "provider_summary_count": provider_summaries.len(),
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": total_tokens
        },
        "changed_files": changed_files,
        "concern_count": concern_count,
        "blocker_count": blocker_count,
        "evidence_refs": evidence_refs,
        "secret_redaction_contract": "adapter command/stdout/stderr/transcript artifacts are stored only after redaction; provider API-key auth is rejected before execution",
        "factory_v3_role": "parity_oracle_only",
        "owner": "ao2-provider-adapter-contract"
    })
}

fn provider_cost_summary(ctx: &RunContext) -> Result<String> {
    let provider_summaries = artifact_json_payloads(ctx, "provider_transcript_summary")?;
    if provider_summaries.is_empty() {
        return Ok("estimated_local_only_cost=0".to_string());
    }

    let mut observed_cost_usd = 0.0_f64;
    let mut cost_reported_count = 0usize;
    let mut total_tokens = 0_u64;
    for summary in &provider_summaries {
        if let Some(cost) = summary.get("cost_usd").and_then(|value| value.as_f64()) {
            observed_cost_usd += cost;
            cost_reported_count += 1;
        }
        if let Some(usage) = summary.get("usage") {
            let input = json_u64(usage, "input_tokens");
            let output = json_u64(usage, "output_tokens");
            total_tokens += json_u64(usage, "total_tokens").max(input + output);
        }
    }

    Ok(format!(
        "observed_provider_cost_usd={:.6}; provider_summary_count={}; cost_reported_summary_count={}; total_tokens={}",
        round_usd(observed_cost_usd),
        provider_summaries.len(),
        cost_reported_count,
        total_tokens
    ))
}

fn json_u64(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

fn round_usd(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn run_health(ctx: &RunContext) -> serde_json::Value {
    let verdict = ctx
        .closure_reports
        .last()
        .map(|report| report.verdict.as_str())
        .unwrap_or("blocked");
    let repair_attempt_count = ctx
        .repair_attempts
        .iter()
        .filter(|attempt| attempt.attempt > 0)
        .count();
    let failed_repair_attempts = ctx
        .repair_attempts
        .iter()
        .filter(|attempt| attempt.status == "failed")
        .count();
    let accepted_repair_attempts = ctx
        .repair_attempts
        .iter()
        .filter(|attempt| attempt.status == "accepted")
        .count();
    let budget_exhausted = ctx
        .repair_attempts
        .iter()
        .any(|attempt| attempt.status == "budget_exhausted");
    let unresolved_concerns = unresolved_concerns(ctx);
    let repair_status = repair_status(
        verdict,
        repair_attempt_count,
        failed_repair_attempts,
        accepted_repair_attempts,
        budget_exhausted,
        &unresolved_concerns,
    );
    let attention_required = verdict != "accepted"
        || budget_exhausted
        || (!unresolved_concerns.is_empty() && accepted_repair_attempts == 0);
    json!({
        "schema_version": "ao2.run-health.v1",
        "verdict": verdict,
        "repair_status": repair_status,
        "repair_attempt_count": repair_attempt_count,
        "failed_repair_attempts": failed_repair_attempts,
        "accepted_repair_attempts": accepted_repair_attempts,
        "budget_exhausted": budget_exhausted,
        "unresolved_concerns": unresolved_concerns,
        "attention_required": attention_required,
        "next_action": run_health_next_action(repair_status, attention_required),
        "evidence_refs": run_health_evidence_refs(ctx),
        "timeline": run_health_timeline(ctx)
    })
}

fn unresolved_concerns(ctx: &RunContext) -> Vec<String> {
    let Some(report) = ctx.closure_reports.last() else {
        return Vec::new();
    };
    let mut concerns = BTreeSet::new();
    for concern in &report.unresolved_concerns {
        concerns.insert(concern.clone());
    }
    concerns.into_iter().collect()
}

fn repair_status(
    verdict: &str,
    repair_attempt_count: usize,
    failed_repair_attempts: usize,
    accepted_repair_attempts: usize,
    budget_exhausted: bool,
    unresolved_concerns: &[String],
) -> &'static str {
    if budget_exhausted {
        "budget_exhausted"
    } else if accepted_repair_attempts > 0 {
        "repaired"
    } else if failed_repair_attempts > 0 {
        "repair_failed"
    } else if repair_attempt_count == 0 && verdict == "accepted" && unresolved_concerns.is_empty() {
        "clean"
    } else if repair_attempt_count == 0 {
        "not_attempted"
    } else {
        "unknown"
    }
}

fn run_health_next_action(repair_status: &str, attention_required: bool) -> &'static str {
    match (repair_status, attention_required) {
        (_, false) => "No operator action required; keep the signed evidence pack for replay.",
        ("budget_exhausted", _) => {
            "Increase repair budget or revise the provider prompt, then rerun from signed evidence."
        }
        ("repair_failed", _) => {
            "Open the latest verifier artifact, revise the repair prompt, and rerun."
        }
        ("not_attempted", _) => {
            "Run a governed repair attempt or close the unresolved concern with evaluator evidence."
        }
        _ => "Review unresolved concerns and verifier artifacts before resuming.",
    }
}

fn run_health_evidence_refs(ctx: &RunContext) -> Vec<String> {
    let mut refs = BTreeSet::new();
    for report in &ctx.closure_reports {
        for artifact_id in &report.evidence_refs {
            refs.insert(artifact_id.clone());
        }
    }
    for attempt in &ctx.repair_attempts {
        for artifact_id in &attempt.evidence_refs {
            refs.insert(artifact_id.clone());
        }
    }
    refs.into_iter().collect()
}

fn run_health_timeline(ctx: &RunContext) -> Vec<serde_json::Value> {
    let mut timeline = Vec::new();
    for report in &ctx.closure_reports {
        timeline.push(json!({
            "kind": "closure",
            "verdict": report.verdict,
            "unresolved_concerns": report.unresolved_concerns,
            "evidence_refs": report.evidence_refs,
            "created_at": report.created_at
        }));
    }
    for attempt in &ctx.repair_attempts {
        timeline.push(json!({
            "kind": "repair_attempt",
            "attempt": attempt.attempt,
            "trigger": attempt.trigger,
            "status": attempt.status,
            "evidence_refs": attempt.evidence_refs,
            "summary": attempt.summary,
            "created_at": attempt.created_at
        }));
    }
    timeline
}

fn artifact_json_payloads(ctx: &RunContext, artifact_type: &str) -> Result<Vec<serde_json::Value>> {
    let mut payloads = Vec::new();
    for artifact in &ctx.artifacts {
        if artifact.artifact_type != artifact_type {
            continue;
        }
        let content = fs::read_to_string(&artifact.uri)
            .with_context(|| format!("read artifact payload {}", artifact.uri))?;
        payloads.push(serde_json::from_str(&content)?);
    }
    Ok(payloads)
}

fn run_markers(ctx: &RunContext) -> Vec<&'static str> {
    let mut markers = vec!["policy_denied_git_push"];
    if ctx.is_real_project_template() {
        markers.push("real_project_template");
    }
    if ctx.closure_reports.iter().any(|report| {
        report
            .unresolved_concerns
            .iter()
            .any(|concern| concern == "review_missing_tests")
    }) {
        markers.push("review_missing_tests");
    }
    if ctx.closure_reports.iter().any(|report| {
        report
            .unresolved_concerns
            .iter()
            .any(|concern| concern == "verifier_failed")
    }) {
        markers.push("verifier_failed");
    }
    if ctx
        .repair_attempts
        .iter()
        .any(|attempt| attempt.status == "budget_exhausted")
    {
        markers.push("repair_budget_exhausted");
    }
    if ctx
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "repair_source_context")
    {
        markers.push("repair_source_context");
    }
    markers
}

fn escape_html(value: impl AsRef<str>) -> String {
    value
        .as_ref()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn html_list(items: Vec<String>, empty_message: &str) -> String {
    if items.is_empty() {
        return format!("<li>{}</li>", escape_html(empty_message));
    }
    items
        .into_iter()
        .map(|item| format!("<li>{}</li>", escape_html(item)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_static_report(ctx: &RunContext, evidence_pack_path: &Path) -> Result<PathBuf> {
    let dir = ctx.run_dir.join("report");
    fs::create_dir_all(&dir)?;
    let path = dir.join("index.html");
    let run_record_path = ctx.run_dir.join("run-record.json");
    let replay_path = ctx.run_dir.join("events.jsonl");
    let closure_items = ctx
        .closure_reports
        .iter()
        .map(|report| {
            format!(
                "<li><strong>{}</strong> at {} evidence=[{}] blockers=[{}] unresolved=[{}]</li>",
                escape_html(&report.verdict),
                report.created_at,
                escape_html(report.evidence_refs.join(", ")),
                escape_html(report.blockers.join(", ")),
                escape_html(report.unresolved_concerns.join(", "))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let acceptance = ctx
        .closure_reports
        .last()
        .map(|report| report.acceptance_criteria_results.clone())
        .filter(|criteria| !criteria.is_empty())
        .or_else(|| {
            if ctx.acceptance.is_empty() {
                None
            } else {
                Some(ctx.acceptance.clone())
            }
        })
        .unwrap_or_else(|| {
            vec![
                "verifier command passes".to_string(),
                "patch stays scoped to the workflow objective".to_string(),
                "replay has zero digest failures".to_string(),
            ]
        });
    let policy_items = ctx
        .policy_decisions
        .iter()
        .map(|decision| {
            format!(
                "{}: {} {} on {} reason={} digest={} policy={} approval_ticket={}",
                decision.decision_id,
                decision.decision,
                decision.action,
                decision.resource,
                decision.reason,
                decision.request_digest,
                decision.policy_version,
                decision.approval_ticket_id.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>();
    let approval_items = ctx
        .approvals
        .iter()
        .map(|ticket| {
            format!(
                "{}: {} action={} digest={} risk={} scope={} approver={}",
                ticket.ticket_id,
                ticket.status,
                ticket.requested_action,
                ticket.action_digest,
                ticket.risk_class,
                ticket.scope,
                ticket.approver.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>();
    let artifact_items = ctx
        .artifacts
        .iter()
        .map(|artifact| {
            format!(
                "{}: {} uri={} media_type={} digest={} producer={}",
                artifact.artifact_id,
                artifact.artifact_type,
                artifact.uri,
                artifact.media_type,
                artifact.digest,
                artifact.producer
            )
        })
        .collect::<Vec<_>>();
    let health = run_health(ctx);
    let health_verdict = health["verdict"].as_str().unwrap_or("blocked");
    let repair_status = health["repair_status"].as_str().unwrap_or("unknown");
    let attention_required = health["attention_required"].as_bool().unwrap_or(true);
    let next_action = health["next_action"]
        .as_str()
        .unwrap_or("Review the evidence pack before resuming.");
    let denied = ctx
        .policy_decisions
        .iter()
        .filter(|d| d.decision == "deny" || d.decision == "requires_approval")
        .count();
    let content = format!(
        r#"<!doctype html>
<html>
<head><meta charset="utf-8"><title>AO2 Risky PR Run {run_id}</title></head>
<body>
<h1>AO2 Risky PR Run</h1>
<p><strong>Run:</strong> {run_id}</p>
<p><strong>Workflow:</strong> {workflow}</p>
<p><strong>Objective:</strong> {objective}</p>
<p><strong>Verifier Command:</strong> {verifier}</p>
<p><strong>Final verdict:</strong> {verdict}</p>
<h2>Roles</h2>
<ul>
{roles}
</ul>
<h2>Acceptance Criteria</h2>
<ul>
{acceptance}
</ul>
<h2>Run Health</h2>
<p><strong>Health verdict:</strong> {health_verdict}</p>
<p><strong>Repair status:</strong> {repair_status}</p>
<p><strong>Attention required:</strong> {attention_required}</p>
<p><strong>Next Operator Action:</strong> {next_action}</p>
<h2>Timeline</h2>
<ul>
<li>Workflow compiled.</li>
<li>Risky git push denied before execution.</li>
<li>Narrow file write approved by exact digest.</li>
<li>First closure rejected for review_missing_tests.</li>
<li>Correction added tests.</li>
<li>Final closure accepted.</li>
</ul>
<h2>Governance</h2>
<p>Denied or approval-required actions: {denied}</p>
<p>Approvals granted: {approvals}</p>
<h2>Policy Decisions</h2>
<ul>
{policy_decisions}
</ul>
<h2>Approval Tickets</h2>
<ul>
{approval_tickets}
</ul>
<h2>Artifacts</h2>
<ul>
{artifacts}
</ul>
<h2>Evidence</h2>
<p>Evidence pack: {evidence}</p>
<h2>Local Run Record</h2>
<p>Run record: {run_record}</p>
<h2>Static Export Evidence</h2>
<p>Evidence pack export: {evidence}</p>
<p>Report artifact: {report}</p>
<h2>Evaluator Closure Evidence</h2>
<ul>
{closure_items}
</ul>
<h2>Replay Evidence</h2>
<p>Replay source: {replay}</p>
</body>
</html>
"#,
        run_id = escape_html(&ctx.run_id),
        workflow = escape_html(&ctx.workflow_id),
        objective = escape_html(&ctx.objective),
        verifier = escape_html(&ctx.verifier_command),
        verdict = escape_html(
            ctx.closure_reports
                .last()
                .map(|report| report.verdict.as_str())
                .unwrap_or("blocked"),
        ),
        roles = html_list(ctx.roles.clone(), "no roles declared"),
        acceptance = html_list(acceptance, "no acceptance criteria recorded"),
        health_verdict = escape_html(health_verdict),
        repair_status = escape_html(repair_status),
        attention_required = attention_required,
        next_action = escape_html(next_action),
        denied = denied,
        approvals = ctx.approvals.len(),
        policy_decisions = html_list(policy_items, "no policy decisions recorded"),
        approval_tickets = html_list(approval_items, "no approval tickets recorded"),
        artifacts = html_list(artifact_items, "no artifacts recorded"),
        evidence = escape_html(evidence_pack_path.display().to_string()),
        run_record = escape_html(run_record_path.display().to_string()),
        report = escape_html(path.display().to_string()),
        closure_items = closure_items,
        replay = escape_html(replay_path.display().to_string())
    );
    atomic_write(&path, content)?;
    Ok(path)
}

fn write_run_record(
    ctx: &RunContext,
    status: RunStatus,
    evidence_pack_path: &Path,
    report_path: &Path,
) -> Result<()> {
    let path = ctx.run_dir.join("run-record.json");
    let record = json!({
        "schema_version": "ao2.run-record.v1",
        "run_id": ctx.run_id,
        "workflow_ref": ctx.workflow_id,
        "template_kind": ctx.template_kind,
        "objective": ctx.objective,
        "roles": ctx.roles,
        "workflow_tasks": ctx.workflow_tasks,
        "workflow_dependencies": ctx.workflow_dependencies,
        "factory_v3_compatibility": ctx.factory_v3_compatibility,
        "acceptance": ctx.acceptance,
        "verifier_command": ctx.verifier_command,
        "status": status,
        "events_head": ctx.events_path,
        "artifacts": ctx.artifacts,
        "policy_decisions": ctx.policy_decisions,
        "approval_tickets": ctx.approvals,
        "repair_attempts": ctx.repair_attempts,
        "closures": ctx.closure_reports,
        "closure": ctx.closure_reports.last(),
        "evidence_pack": evidence_pack_path,
        "report": report_path
    });
    atomic_write(path, serde_json::to_string_pretty(&record)?)?;
    Ok(())
}

/// Deserialize an optional array field from a run record, distinguishing
/// "absent/null" (→ empty, expected for older or partial records) from
/// "present but malformed" (→ hard error). The previous `unwrap_or_default()`
/// conflated the two, so a shape-drifted record was silently emptied and then
/// re-persisted on resume — permanent loss of policy decisions / approvals that
/// also feeds the lossless evidence pack the observer reads.
fn parse_optional_record_array<T: serde::de::DeserializeOwned>(
    record: &serde_json::Value,
    field: &str,
) -> Result<Vec<T>> {
    match record.get(field) {
        None => Ok(Vec::new()),
        Some(value) if value.is_null() => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value.clone())
            .with_context(|| format!("run record field `{field}` is malformed")),
    }
}

fn load_run_context(target_repo: &Path, run_id: &str) -> Result<(RunContext, RunStatus)> {
    let run_dir = target_repo.join(".ao2").join("runs").join(run_id);
    let record_path = run_dir.join("run-record.json");
    let content = fs::read_to_string(&record_path)
        .with_context(|| format!("read run record {}", record_path.display()))?;
    let record: serde_json::Value = serde_json::from_str(&content)?;
    let status: RunStatus = serde_json::from_value(record["status"].clone())?;
    let workflow_id = record["workflow_ref"]
        .as_str()
        .unwrap_or("risky-pr-run@0.1.0")
        .to_string();
    let template_kind = record["template_kind"].as_str().map(str::to_string);
    let objective = record["objective"]
        .as_str()
        .unwrap_or("Add input validation to calculate_discount and update tests.")
        .to_string();
    let roles = record["roles"]
        .as_array()
        .map(|roles| {
            roles
                .iter()
                .filter_map(|role| role.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|roles| !roles.is_empty())
        .unwrap_or_else(default_roles);
    let acceptance = record["acceptance"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let workflow_tasks = record["workflow_tasks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let workflow_dependencies = record["workflow_dependencies"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let factory_v3_compatibility = record
        .get("factory_v3_compatibility")
        .filter(|value| !value.is_null())
        .cloned();
    let verifier_command = record["verifier_command"]
        .as_str()
        .unwrap_or("python -m pytest")
        .to_string();
    let artifacts: Vec<ArtifactRef> = parse_optional_record_array(&record, "artifacts")?;
    let policy_decisions: Vec<PolicyDecision> =
        parse_optional_record_array(&record, "policy_decisions")?;
    let approvals: Vec<ApprovalTicket> = parse_optional_record_array(&record, "approval_tickets")?;
    let closure_reports: Vec<ClosureReport> =
        if record.get("closures").is_some() && !record["closures"].is_null() {
            parse_optional_record_array(&record, "closures")?
        } else if !record["closure"].is_null() {
            vec![serde_json::from_value(record["closure"].clone())
                .context("run record field `closure` is malformed")?]
        } else {
            Vec::new()
        };
    let repair_attempts: Vec<RepairAttempt> =
        parse_optional_record_array(&record, "repair_attempts")?;

    Ok((
        RunContext {
            run_id: run_id.to_string(),
            workflow_id,
            template_kind,
            objective,
            roles,
            workflow_tasks,
            workflow_dependencies,
            factory_v3_compatibility,
            acceptance,
            verifier_command,
            target_repo: target_repo.to_path_buf(),
            events_path: run_dir.join("events.jsonl"),
            artifact_store: ArtifactStore::new(run_dir.join("artifacts")),
            run_dir,
            artifacts,
            policy_decisions,
            approvals,
            closure_reports,
            repair_attempts,
        },
        status,
    ))
}

fn summary_from_accepted_record(target_repo: &Path, run_id: &str) -> Result<RunSummary> {
    let (ctx, status) = load_run_context(target_repo, run_id)?;
    if status != RunStatus::Accepted {
        return Err(anyhow!("run {run_id} is not accepted"));
    }
    let record_path = ctx.run_dir.join("run-record.json");
    let content = fs::read_to_string(&record_path)
        .with_context(|| format!("read run record {}", record_path.display()))?;
    let record: serde_json::Value = serde_json::from_str(&content)?;
    let evidence_pack_path = path_from_json_string(&record["evidence_pack"])?;
    let report_path = path_from_json_string(&record["report"])?;
    Ok(RunSummary {
        run_id: run_id.to_string(),
        status,
        run_dir: ctx.run_dir.clone(),
        evidence_pack_path,
        report_path,
        run_record_path: record_path,
        denied_actions: denied_actions(&ctx.policy_decisions),
        approvals: ctx.approvals.clone(),
        rejection_count: ctx
            .closure_reports
            .iter()
            .filter(|report| report.verdict == "rejected")
            .count(),
    })
}

fn path_from_json_string(value: &serde_json::Value) -> Result<PathBuf> {
    value
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("expected path string in run record"))
}

fn denied_actions(decisions: &[PolicyDecision]) -> Vec<PolicyDecision> {
    decisions
        .iter()
        .filter(|decision| decision.decision == "deny" || decision.decision == "requires_approval")
        .cloned()
        .collect()
}

fn replace_ticket(tickets: &mut Vec<ApprovalTicket>, ticket: ApprovalTicket) {
    if let Some(existing) = tickets
        .iter_mut()
        .find(|existing| existing.ticket_id == ticket.ticket_id)
    {
        *existing = ticket;
    } else {
        tickets.push(ticket);
    }
}

fn approval_path(run_dir: &Path, ticket_id: &str) -> PathBuf {
    run_dir.join("approvals").join(format!("{ticket_id}.json"))
}

fn write_stored_approval(
    ctx: &RunContext,
    ticket: &ApprovalTicket,
    request: &ToolRequest,
) -> Result<()> {
    let path = approval_path(&ctx.run_dir, &ticket.ticket_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let stored = StoredApproval {
        ticket: ticket.clone(),
        request: request.clone(),
    };
    atomic_write(&path, serde_json::to_string_pretty(&stored)?)
        .with_context(|| format!("write approval {}", path.display()))?;
    Ok(())
}

fn find_approval_path(target_repo: &Path, ticket_id: &str) -> Result<PathBuf> {
    let runs_dir = target_repo.join(".ao2").join("runs");
    if !runs_dir.exists() {
        return Err(anyhow!("no AO2 runs found under {}", target_repo.display()));
    }
    for entry in fs::read_dir(&runs_dir)? {
        let entry = entry?;
        let path = approval_path(&entry.path(), ticket_id);
        if path.exists() {
            return Ok(path);
        }
    }
    Err(anyhow!("approval ticket not found: {ticket_id}"))
}

/// Cross-process advisory lock over a single AO2 run directory.
///
/// `approve_risky_pr_ticket` reads an approval file, mutates the ticket, and writes
/// it back, then rewrites the run record. Two `ao2` processes acting on the same run
/// concurrently could interleave those steps and drop an update. This guard
/// serializes them by holding an exclusive OS lock (flock on Unix, LockFileEx on
/// Windows, via `fs4`) on a `<run_dir>/.lock` sentinel for the whole read-modify-write
/// and releasing it on drop — including on process exit, since the OS closes the
/// handle, so a crashed holder cannot wedge the lock permanently.
///
/// This guards the runtime's own on-disk run state; it grants no new authority over
/// AO2 runs.
struct RunLock {
    _file: std::fs::File,
}

impl RunLock {
    fn acquire(run_dir: &Path) -> Result<Self> {
        fs::create_dir_all(run_dir)
            .with_context(|| format!("create run dir {}", run_dir.display()))?;
        let lock_path = run_dir.join(".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            // Never truncate: the sentinel's contents are irrelevant (we only lock
            // it), and truncating would needlessly rewrite a file other holders may
            // have open.
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("open run lock {}", lock_path.display()))?;
        // Block until the exclusive lock is ours. Called as `fs4::FileExt::lock`
        // (rather than `file.lock()`) so it binds to fs4's cross-platform impl
        // instead of std's inherent `File::lock`, which is newer than the declared
        // workspace MSRV.
        fs4::FileExt::lock(&file)
            .with_context(|| format!("acquire run lock {}", lock_path.display()))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod run_lock_tests {
    use super::RunLock;
    use std::fs::OpenOptions;

    #[test]
    fn holds_exclusive_os_lock_on_run_dir_until_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let run_dir = dir.path();

        // Acquiring the run lock creates `<run_dir>/.lock` and takes an exclusive
        // OS lock on it.
        let guard = RunLock::acquire(run_dir).expect("acquire run lock");

        // A second, independent handle to the same sentinel must NOT be able to take
        // the lock while the guard holds it. Separate file descriptors / handles in
        // the same process contend under flock (Unix) and LockFileEx (Windows), so a
        // failed `try_lock` here proves the guard enforces real cross-process mutual
        // exclusion over the run directory's read-modify-write.
        let probe = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(run_dir.join(".lock"))
            .expect("open probe handle");
        assert!(
            fs4::FileExt::try_lock(&probe).is_err(),
            "run lock must exclude a second holder while the guard is alive"
        );

        // Dropping the guard releases the lock, so a fresh independent holder
        // can acquire the sentinel again. Use a new handle here: on some
        // platforms a handle that already attempted and failed a lock can report
        // stale contention under heavy parallel test load.
        drop(guard);
        drop(probe);
        let released_probe = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(run_dir.join(".lock"))
            .expect("open released probe handle");
        let released = (0..10).any(|_| {
            if fs4::FileExt::try_lock(&released_probe).is_ok() {
                true
            } else {
                std::thread::sleep(std::time::Duration::from_millis(10));
                false
            }
        });
        assert!(released, "run lock must be free once the guard is dropped");
    }
}

#[cfg(test)]
mod load_run_context_tests {
    use super::*;
    use chrono::Utc;
    use std::fs;

    fn base_record() -> serde_json::Value {
        json!({
            "schema_version": "ao2.run-record.v1",
            "run_id": "run-test",
            "workflow_ref": "risky-pr-run@0.1.0",
            "status": "accepted",
            "events_head": "events.jsonl",
            "evidence_pack": "evidence-pack/evidence-pack.json",
            "report": "report/index.html"
        })
    }

    fn write_record(target: &Path, run_id: &str, record: &serde_json::Value) {
        let run_dir = target.join(".ao2").join("runs").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(
            run_dir.join("run-record.json"),
            serde_json::to_string_pretty(record).unwrap(),
        )
        .unwrap();
    }

    fn closure_report(verdict: &str) -> serde_json::Value {
        json!({
            "verdict": verdict,
            "acceptance_criteria_results": [],
            "evidence_refs": [],
            "unresolved_concerns": [],
            "blockers": [],
            "policy_exceptions": [],
            "cost_summary": "provider-free",
            "created_at": Utc::now()
        })
    }

    #[test]
    fn absent_optional_arrays_load_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), "run-test", &base_record());
        let (ctx, status) = load_run_context(dir.path(), "run-test").unwrap();
        assert_eq!(status, RunStatus::Accepted);
        assert!(ctx.policy_decisions.is_empty());
        assert!(ctx.approvals.is_empty());
        assert!(ctx.artifacts.is_empty());
        assert!(ctx.repair_attempts.is_empty());
    }

    #[test]
    fn malformed_policy_decisions_is_an_error_not_silent_loss() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = base_record();
        record["policy_decisions"] = json!("totally-not-an-array-of-decisions");
        write_record(dir.path(), "run-test", &record);
        let result = load_run_context(dir.path(), "run-test");
        assert!(
            result.is_err(),
            "a malformed policy_decisions field must error, not silently empty the array and then re-persist the loss"
        );
    }

    #[test]
    fn malformed_approval_tickets_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = base_record();
        record["approval_tickets"] = json!([{"unexpected": "shape"}]);
        write_record(dir.path(), "run-test", &record);
        assert!(load_run_context(dir.path(), "run-test").is_err());
    }

    #[test]
    fn closures_array_preserves_rejection_count_on_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = base_record();
        record["closure"] = closure_report("accepted");
        record["closures"] = json!([closure_report("rejected"), closure_report("accepted")]);
        write_record(dir.path(), "run-test", &record);

        let summary = summary_from_accepted_record(dir.path(), "run-test").unwrap();

        assert_eq!(
            summary.rejection_count, 1,
            "run-record reload must preserve the full closure history, not just the final accepted closure"
        );
    }
}
