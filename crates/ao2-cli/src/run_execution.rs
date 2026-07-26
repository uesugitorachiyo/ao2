use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_adapters::{parse_provider, ProviderKind};
use ao2_runtime::{
    resume_risky_pr_provider_free, run_risky_pr_provider_free, run_risky_pr_with_provider_prompt,
    start_risky_pr_provider_free, ProviderRunOptions, ResumeOptions, RunOptions,
};

use crate::cli_util::{
    atomic_write_text, json_array, json_string, read_prompt, sanitize_greenfield_id,
    sha256_bytes_hex,
};
use crate::provider_ops::{materialize_template_workflow, read_optional_json};
use crate::run_resume::{
    approve_and_resume_persisted_sandbox_patches, pending_approval_recovery_context,
    print_approval_recovery_context,
};

pub(crate) struct CliRunOptions {
    pub(crate) workflow: Option<PathBuf>,
    pub(crate) spec: Option<PathBuf>,
    pub(crate) dry_run: bool,
    pub(crate) template: Option<String>,
    pub(crate) target: Option<PathBuf>,
    pub(crate) run_id: Option<String>,
    pub(crate) pause_for_approval: bool,
    pub(crate) resume: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt: Option<String>,
    pub(crate) provider_prompt_file: Option<PathBuf>,
    pub(crate) provider_max_budget_usd: Option<f64>,
    pub(crate) max_repair_attempts: usize,
}

struct Ao2RunSpec {
    path: PathBuf,
    spec_sha256: String,
    value: serde_json::Value,
    api_version: String,
    plan_id: String,
    source_schema: String,
    target_repo: PathBuf,
    control_plane_role: String,
    mutates_ao_artifacts: bool,
    tasks: Vec<serde_json::Value>,
}

#[derive(Clone, Copy)]
enum Ao2RunSpecExecutionMode {
    ProviderFree,
    AggregateProvider,
}

impl Ao2RunSpecExecutionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ProviderFree => "provider_free",
            Self::AggregateProvider => "aggregate_provider",
        }
    }
}

pub(crate) fn run(options: CliRunOptions) -> Result<()> {
    if let Some(spec) = options.spec.clone() {
        if options.workflow.is_some() || options.template.is_some() || options.resume.is_some() {
            return Err(anyhow!(
                "ao2 run --spec cannot be combined with workflow/template/resume options"
            ));
        }
        if options.dry_run {
            if options.provider.is_some()
                || options.provider_prompt.is_some()
                || options.provider_prompt_file.is_some()
            {
                return Err(anyhow!(
                    "ao2 run --dry-run --spec cannot be combined with provider options"
                ));
            }
            return run_ao2_run_spec_dry_run(&spec, options.target.as_deref());
        }
        return run_ao2_run_spec_governed(options, &spec);
    }
    let target = options
        .target
        .clone()
        .context("--target is required unless --spec is used")?;
    if options.dry_run {
        return Err(anyhow!("--dry-run requires --spec"));
    }
    if let Some(resume_run_id) = options.resume {
        match resume_risky_pr_provider_free(ResumeOptions {
            target_repo: target.clone(),
            run_id: resume_run_id.clone(),
        }) {
            Ok(summary) => {
                crate::run_reporting::print_run_summary(&summary);
                return Ok(());
            }
            Err(error) => {
                if error
                    .to_string()
                    .contains("waiting for approval before resume")
                {
                    if let Some(context) =
                        pending_approval_recovery_context(&target, &resume_run_id)
                    {
                        print_approval_recovery_context(&context, "pending", None);
                    }
                }
                return Err(error);
            }
        }
    }

    let workflow = options.workflow.map(Ok).unwrap_or_else(|| {
        let template = options
            .template
            .as_deref()
            .context("workflow path or --template is required unless --resume is used")?;
        materialize_template_workflow(&target, template)
    })?;
    if options.provider.is_some()
        || options.provider_prompt.is_some()
        || options.provider_prompt_file.is_some()
    {
        let provider = parse_provider(options.provider.as_deref().unwrap_or("scripted"))?;
        let prompt = read_prompt(options.provider_prompt, options.provider_prompt_file)?;
        let summary = run_risky_pr_with_provider_prompt(ProviderRunOptions {
            target_repo: target,
            workflow_path: workflow,
            run_id: options.run_id,
            provider,
            prompt,
            max_repair_attempts: options.max_repair_attempts,
            max_budget_usd: options.provider_max_budget_usd,
            repair_source: None,
        })?;
        crate::run_reporting::print_run_summary(&summary);
        return Ok(());
    }

    if options.pause_for_approval {
        let summary = start_risky_pr_provider_free(RunOptions {
            target_repo: target.clone(),
            workflow_path: workflow,
            run_id: options.run_id,
        })?;
        println!("run_id={}", summary.run_id);
        println!("status={:?}", summary.status);
        println!("approval_required=true");
        println!("run_record={}", summary.run_record_path.display());
        println!("evidence_dir={}", summary.run_dir.display());
        println!("approval_ticket_id={}", summary.approval_ticket.ticket_id);
        println!("approval_status={}", summary.approval_ticket.status);
        println!("required_digest_field=action_digest");
        println!("action_digest={}", summary.approval_ticket.action_digest);
        println!("replay_state=waiting_for_approval");
        println!(
            "next_step=ao2 approve {} --target {} --approver <operator>; ao2 run --resume {} --target {}",
            summary.approval_ticket.ticket_id,
            target.display(),
            summary.run_id,
            target.display()
        );
        return Ok(());
    }

    let summary = run_risky_pr_provider_free(RunOptions {
        target_repo: target,
        workflow_path: workflow,
        run_id: options.run_id,
    })?;
    crate::run_reporting::print_run_summary(&summary);
    Ok(())
}

fn load_ao2_run_spec(spec: &Path, target_override: Option<&Path>) -> Result<Ao2RunSpec> {
    let spec_text = fs::read_to_string(spec).with_context(|| format!("read {}", spec.display()))?;
    let value: serde_json::Value = serde_yaml::from_str(&spec_text)
        .with_context(|| format!("parse ao2 run spec {}", spec.display()))?;
    let api_version = value
        .get("apiVersion")
        .and_then(serde_json::Value::as_str)
        .context("ao2 run spec apiVersion is required")?;
    if api_version != "ao2.run/v1" {
        return Err(anyhow!(
            "unsupported ao2 run spec apiVersion: {api_version}"
        ));
    }
    let kind = value
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .context("ao2 run spec kind is required")?;
    if kind != "Run" {
        return Err(anyhow!("unsupported ao2 run spec kind: {kind}"));
    }
    let spec_body = value
        .get("spec")
        .and_then(serde_json::Value::as_object)
        .context("ao2 run spec body is required")?;
    let source = spec_body
        .get("source")
        .and_then(serde_json::Value::as_object)
        .context("ao2 run spec source is required")?;
    let plan_id = source
        .get("plan_id")
        .and_then(serde_json::Value::as_str)
        .context("ao2 run spec source.plan_id is required")?;
    let source_schema = source
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        .context("ao2 run spec source.schema_version is required")?;
    if source_schema != "ao2.sdd-plan.v1" {
        return Err(anyhow!(
            "ao2 run spec source.schema_version must be ao2.sdd-plan.v1"
        ));
    }
    let target = spec_body
        .get("target")
        .and_then(serde_json::Value::as_object)
        .context("ao2 run spec target is required")?;
    let target_repo = target
        .get("repo_path")
        .and_then(serde_json::Value::as_str)
        .context("ao2 run spec target.repo_path is required")?;
    if let Some(override_path) = target_override {
        let override_text = override_path.display().to_string();
        if override_text != target_repo {
            return Err(anyhow!(
                "--target override {} does not match ao2 run spec target.repo_path {}",
                override_path.display(),
                target_repo
            ));
        }
    }
    let trust_boundary = spec_body
        .get("trust_boundary")
        .and_then(serde_json::Value::as_object)
        .context("ao2 run spec trust_boundary is required")?;
    let control_plane_role = trust_boundary
        .get("control_plane_role")
        .and_then(serde_json::Value::as_str)
        .context("ao2 run spec trust_boundary.control_plane_role is required")?;
    if control_plane_role != "read_only_observer" {
        return Err(anyhow!(
            "ao2 run spec trust_boundary.control_plane_role must be read_only_observer"
        ));
    }
    let mutates_ao_artifacts = trust_boundary
        .get("mutates_ao_artifacts")
        .and_then(serde_json::Value::as_bool)
        .context("ao2 run spec trust_boundary.mutates_ao_artifacts is required")?;
    if mutates_ao_artifacts {
        return Err(anyhow!(
            "ao2 run spec trust_boundary.mutates_ao_artifacts must be false"
        ));
    }
    let tasks = spec_body
        .get("tasks")
        .and_then(serde_json::Value::as_array)
        .context("ao2 run spec tasks are required")?;
    if tasks.is_empty() {
        return Err(anyhow!("ao2 run spec tasks must not be empty"));
    }
    let spec_sha256 = sha256_bytes_hex(spec_text.as_bytes());
    let api_version = api_version.to_string();
    let plan_id = plan_id.to_string();
    let source_schema = source_schema.to_string();
    let target_repo = PathBuf::from(target_repo);
    let control_plane_role = control_plane_role.to_string();
    let tasks = tasks.clone();
    Ok(Ao2RunSpec {
        path: spec.to_path_buf(),
        spec_sha256,
        value,
        api_version,
        plan_id,
        source_schema,
        target_repo,
        control_plane_role,
        mutates_ao_artifacts,
        tasks,
    })
}

fn run_ao2_run_spec_dry_run(spec: &Path, target_override: Option<&Path>) -> Result<()> {
    let loaded = load_ao2_run_spec(spec, target_override)?;
    println!("status=dry_run_accepted");
    println!("schema_version={}", loaded.api_version);
    println!("plan_id={}", loaded.plan_id);
    println!("task_count={}", loaded.tasks.len());
    println!("target_repo={}", loaded.target_repo.display());
    println!("control_plane_role={}", loaded.control_plane_role);
    println!("mutates_ao_artifacts={}", loaded.mutates_ao_artifacts);
    println!("factory_v3_drives_workflow=false");
    println!("spec={}", spec.display());
    println!("spec_sha256={}", loaded.spec_sha256);
    Ok(())
}

fn run_ao2_run_spec_governed(options: CliRunOptions, spec: &Path) -> Result<()> {
    let loaded = load_ao2_run_spec(spec, options.target.as_deref())?;
    let workflow = materialize_ao2_run_spec_workflow(&loaded)?;
    let provider_backed = options.provider.is_some()
        || options.provider_prompt.is_some()
        || options.provider_prompt_file.is_some();
    println!(
        "status={}",
        if provider_backed {
            "governed_provider_run_started"
        } else {
            "governed_run_started"
        }
    );
    println!("schema_version={}", loaded.api_version);
    println!("plan_id={}", loaded.plan_id);
    println!("target_repo={}", loaded.target_repo.display());
    println!("workflow={}", workflow.display());
    let (mut summary, execution_mode) = if provider_backed {
        let provider = parse_provider(options.provider.as_deref().unwrap_or("scripted"))?;
        let operator_prompt =
            if options.provider_prompt.is_some() || options.provider_prompt_file.is_some() {
                Some(read_prompt(
                    options.provider_prompt,
                    options.provider_prompt_file,
                )?)
            } else {
                None
            };
        let prompt = ao2_run_spec_provider_prompt(&loaded, provider, operator_prompt.as_deref())?;
        (
            run_risky_pr_with_provider_prompt(ProviderRunOptions {
                target_repo: loaded.target_repo.clone(),
                workflow_path: workflow,
                run_id: options.run_id,
                provider,
                prompt,
                max_repair_attempts: options.max_repair_attempts,
                max_budget_usd: options.provider_max_budget_usd,
                repair_source: None,
            })?,
            Ao2RunSpecExecutionMode::AggregateProvider,
        )
    } else {
        (
            run_risky_pr_provider_free(RunOptions {
                target_repo: loaded.target_repo.clone(),
                workflow_path: workflow,
                run_id: options.run_id,
            })?,
            Ao2RunSpecExecutionMode::ProviderFree,
        )
    };
    if provider_backed && summary.status == ao2_runtime::RunStatus::WaitingForApproval {
        if let Some(resumed) = approve_and_resume_persisted_sandbox_patches(
            &loaded.target_repo,
            &summary.run_id,
            "human:sdd-run-operator",
        )? {
            summary = resumed;
        }
    }
    let task_graph_path = write_ao2_run_spec_task_graph_sidecar(&loaded, &summary, execution_mode)?;
    crate::run_reporting::print_run_summary(&summary);
    println!("sdd_task_graph={}", task_graph_path.display());
    Ok(())
}

fn write_ao2_run_spec_task_graph_sidecar(
    spec: &Ao2RunSpec,
    summary: &ao2_runtime::RunSummary,
    execution_mode: Ao2RunSpecExecutionMode,
) -> Result<PathBuf> {
    let run_dir = summary
        .evidence_pack_path
        .parent()
        .and_then(Path::parent)
        .context("evidence pack path must live under run/evidence-pack")?;
    let path = run_dir.join("sdd-task-graph.json");
    let ordered_tasks = ao2_run_spec_ordered_tasks(&spec.tasks)?;
    let task_graph = ordered_tasks
        .iter()
        .map(ao2_run_spec_task_graph_entry)
        .collect::<Vec<_>>();
    let task_executions =
        ao2_run_spec_task_execution_records(spec, summary, run_dir, &ordered_tasks)?;
    write_ao2_run_spec_task_executions_to_evidence_pack(
        &summary.evidence_pack_path,
        &task_executions,
    )?;
    let payload = serde_json::json!({
        "schema_version": "ao2.sdd-task-graph-execution.v1",
        "run_id": summary.run_id,
        "status": format!("{:?}", summary.status),
        "plan_id": spec.plan_id,
        "source_schema": spec.source_schema,
        "source_spec": spec.path,
        "source_spec_sha256": spec.spec_sha256,
        "execution_mode": execution_mode.as_str(),
        "task_count": task_graph.len(),
        "tasks": task_graph,
        "task_executions": task_executions,
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        },
        "next_maturity_step": "execute each task with dependency-aware provider evidence and verifier closure"
    });
    atomic_write_text(&path, &serde_json::to_string_pretty(&payload)?)?;
    Ok(path)
}

fn ao2_run_spec_task_graph_entry(task: &serde_json::Value) -> serde_json::Value {
    let id = task
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let kind = task
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("agent");
    serde_json::json!({
        "id": id,
        "kind": kind,
        "role": task.get("role").cloned().unwrap_or_else(|| serde_json::json!("implementer")),
        "depends_on": ao2_run_spec_task_dependencies(task),
        "acceptance": task.get("acceptance").cloned().unwrap_or_else(|| serde_json::json!([])),
        "writes": task.get("writes").cloned().unwrap_or_else(|| serde_json::json!([])),
        "provider_contract": {
            "evidence": true,
            "concerns": true,
            "blockers": true,
            "changed_files": true,
            "sandbox": true,
            "secret_redaction": true
        }
    })
}

fn ao2_run_spec_ordered_tasks(tasks: &[serde_json::Value]) -> Result<Vec<serde_json::Value>> {
    let task_ids = tasks
        .iter()
        .map(|task| {
            task.get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .context("ao2 run spec task.id is required")
        })
        .collect::<Result<Vec<_>>>()?;
    let task_id_set = task_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut completed = BTreeSet::<String>::new();
    let mut ordered = Vec::with_capacity(tasks.len());

    while ordered.len() < tasks.len() {
        let mut progressed = false;
        for task in tasks {
            let id = task
                .get("id")
                .and_then(serde_json::Value::as_str)
                .context("ao2 run spec task.id is required")?;
            if completed.contains(id) {
                continue;
            }
            let deps = ao2_run_spec_task_dependency_strings(task);
            for dep in &deps {
                if !task_id_set.contains(dep) {
                    return Err(anyhow!(
                        "ao2 run spec task {id} depends on unknown task {dep}"
                    ));
                }
            }
            if deps.iter().all(|dep| completed.contains(dep)) {
                ordered.push(task.clone());
                completed.insert(id.to_string());
                progressed = true;
            }
        }
        if !progressed {
            return Err(anyhow!(
                "ao2 run spec task graph contains a dependency cycle"
            ));
        }
    }

    Ok(ordered)
}

fn ao2_run_spec_task_dependencies(task: &serde_json::Value) -> serde_json::Value {
    serde_json::Value::Array(
        ao2_run_spec_task_dependency_strings(task)
            .into_iter()
            .map(serde_json::Value::String)
            .collect(),
    )
}

fn ao2_run_spec_task_dependency_strings(task: &serde_json::Value) -> Vec<String> {
    task.get("depends_on")
        .or_else(|| task.get("deps"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}

fn ao2_run_spec_task_execution_records(
    _spec: &Ao2RunSpec,
    summary: &ao2_runtime::RunSummary,
    run_dir: &Path,
    ordered_tasks: &[serde_json::Value],
) -> Result<Vec<serde_json::Value>> {
    let evidence_pack = read_optional_json(&summary.evidence_pack_path)?;
    let events = read_ao2_run_spec_events(&run_dir.join("events.jsonl"))?;
    let closure_status = json_string(&evidence_pack, "verdict");
    let verifier_refs = ao2_run_spec_artifact_refs_by_type(&evidence_pack, |artifact_type| {
        artifact_type.contains("verifier") || artifact_type == "test_log"
    });
    let mut records = Vec::with_capacity(ordered_tasks.len());

    for task in ordered_tasks {
        let task_id = task
            .get("id")
            .and_then(serde_json::Value::as_str)
            .context("ao2 run spec task.id is required")?;
        let task_events = events
            .iter()
            .filter(|event| json_string(event, "role_id") == task_id)
            .collect::<Vec<_>>();
        let event_refs = task_events
            .iter()
            .map(|event| json_string(event, "event_id"))
            .filter(|event_id| !event_id.is_empty())
            .collect::<Vec<_>>();
        let started_at = task_events
            .iter()
            .map(|event| json_string(event, "timestamp"))
            .find(|timestamp| !timestamp.is_empty());
        let completed_at = task_events
            .iter()
            .rev()
            .map(|event| json_string(event, "timestamp"))
            .find(|timestamp| !timestamp.is_empty());
        let provider_summary_refs = ao2_run_spec_artifact_refs_for_task(
            &evidence_pack,
            task_id,
            &["provider_transcript_summary"],
        );
        let sandbox_patch_refs = ao2_run_spec_artifact_refs_for_task(
            &evidence_pack,
            task_id,
            &["sandbox_patch_preview", "sandbox_patch_apply"],
        );
        let provider_free_command_refs = ao2_run_spec_artifact_refs_for_task(
            &evidence_pack,
            task_id,
            &["provider_free_command_log"],
        );
        let provider_free_command_count = provider_free_command_refs.len();
        let status = if closure_status == "accepted" {
            "accepted"
        } else if task_events.iter().any(|event| {
            json_string(event, "event_type") == "role.completed"
                || json_string(event, "event_type") == "sandbox.patch.applied"
        }) {
            "completed"
        } else if task_events
            .iter()
            .any(|event| json_string(event, "event_type") == "role.started")
        {
            "started"
        } else {
            "not_observed"
        };

        records.push(serde_json::json!({
            "task_id": task_id,
            "status": status,
            "dependency_prerequisites": ao2_run_spec_task_dependencies(task),
            "started_at": started_at,
            "completed_at": completed_at,
            "provider_summary_refs": provider_summary_refs,
            "provider_summary_count": provider_summary_refs.len(),
            "sandbox_patch_refs": sandbox_patch_refs,
            "sandbox_patch_count": sandbox_patch_refs.len(),
            "provider_free_command_refs": provider_free_command_refs,
            "provider_free_command_count": provider_free_command_count,
            "verifier_refs": verifier_refs,
            "verifier_ref_count": verifier_refs.len(),
            "closure_status": closure_status,
            "event_refs": event_refs,
            "event_count": event_refs.len(),
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false
            }
        }));
    }

    Ok(records)
}

fn read_ao2_run_spec_events(path: &Path) -> Result<Vec<serde_json::Value>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut events = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        events.push(serde_json::from_str(trimmed).with_context(|| {
            format!("parse {} line {}", path.display(), index.saturating_add(1))
        })?);
    }
    Ok(events)
}

fn ao2_run_spec_artifact_refs_for_task(
    evidence_pack: &serde_json::Value,
    task_id: &str,
    artifact_types: &[&str],
) -> Vec<String> {
    json_array(evidence_pack, "artifacts")
        .iter()
        .filter(|artifact| json_string(artifact, "producer") == task_id)
        .filter(|artifact| {
            artifact_types.contains(&json_string(artifact, "artifact_type").as_str())
        })
        .map(|artifact| json_string(artifact, "artifact_id"))
        .filter(|artifact_id| !artifact_id.is_empty())
        .collect()
}

fn ao2_run_spec_artifact_refs_by_type<F>(
    evidence_pack: &serde_json::Value,
    matches_type: F,
) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    json_array(evidence_pack, "artifacts")
        .iter()
        .filter(|artifact| matches_type(&json_string(artifact, "artifact_type")))
        .map(|artifact| json_string(artifact, "artifact_id"))
        .filter(|artifact_id| !artifact_id.is_empty())
        .collect()
}

fn write_ao2_run_spec_task_executions_to_evidence_pack(
    evidence_pack_path: &Path,
    task_executions: &[serde_json::Value],
) -> Result<()> {
    let mut evidence_pack = read_optional_json(evidence_pack_path)?;
    if let Some(object) = evidence_pack.as_object_mut() {
        object.insert(
            "task_executions".to_string(),
            serde_json::Value::Array(task_executions.to_vec()),
        );
        atomic_write_text(
            evidence_pack_path,
            &serde_json::to_string_pretty(&evidence_pack)?,
        )?;
    }
    Ok(())
}

fn ao2_run_spec_provider_prompt(
    spec: &Ao2RunSpec,
    provider: ProviderKind,
    operator_prompt: Option<&str>,
) -> Result<String> {
    match provider {
        ProviderKind::Scripted => Ok(ao2_run_spec_scripted_provider_prompt(spec, operator_prompt)),
        ProviderKind::Codex | ProviderKind::Claude | ProviderKind::Antigravity => {
            ao2_run_spec_agent_provider_prompt(spec, operator_prompt)
        }
    }
}

fn ao2_run_spec_scripted_provider_prompt(
    spec: &Ao2RunSpec,
    operator_prompt: Option<&str>,
) -> String {
    let mut script = String::new();
    script.push_str("set -eu\n");
    script.push_str(&format!(
        "printf 'Summary: AO2 scripted provider accepted SDD task graph {}\\n'\n",
        cli_shell_single_quote(&spec.plan_id)
    ));
    script.push_str("printf 'Changed files: none\\n'\n");
    script.push_str("printf 'Concern: none\\n'\n");
    script.push_str("printf 'Blocker: none\\n'\n");
    script.push_str(&format!(
        "printf 'ao2_plan_id=%s\\n' {}\n",
        cli_shell_single_quote(&spec.plan_id)
    ));
    script.push_str(&format!(
        "printf 'ao2_task_count=%s\\n' {}\n",
        cli_shell_single_quote(&spec.tasks.len().to_string())
    ));
    script.push_str("# AO2 SDD task graph follows as shell comments.\n");
    for task in &spec.tasks {
        let id = task
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let kind = task
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("agent");
        script.push_str(&format!("# task id={} kind={}\n", id, kind));
        if let Some(rationale) = task.get("rationale").and_then(serde_json::Value::as_str) {
            script.push_str(&format!("# rationale: {}\n", rationale.replace('\n', " ")));
        }
    }
    if let Some(operator_prompt) = operator_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        script.push_str("# Operator-supplied scripted extension begins.\n");
        script.push_str(operator_prompt);
        if !operator_prompt.ends_with('\n') {
            script.push('\n');
        }
    }
    script
}

fn cli_shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn ao2_run_spec_agent_provider_prompt(
    spec: &Ao2RunSpec,
    operator_prompt: Option<&str>,
) -> Result<String> {
    let spec_json = serde_json::to_string_pretty(&spec.value)?;
    let operator_section = operator_prompt
        .filter(|prompt| !prompt.trim().is_empty())
        .map(|prompt| format!("\nAdditional operator instruction:\n{prompt}\n"))
        .unwrap_or_default();
    Ok(format!(
        r#"You are executing an AO2 SDD task graph inside the governed AO2 runtime.

Trust boundary:
- AO2 owns execution, memory, replay, and signed evidence.
- ao2-control-plane is read-only observer only.
- factory-v3 evaluator-closer remains the release acceptance owner.
- Do not use provider API keys. Use local OAuth CLI only.
- Do not expose bearer tokens, cookies, credentials, or secrets in outputs.

Plan id: {plan_id}
Source schema: {source_schema}
Target repository: {target_repo}
Task count: {task_count}

Execute the task graph below in dependency order. Keep changes scoped to the listed paths and acceptance criteria. Run the verifier when possible. Report:
Summary: <short summary>
Changed files: <comma-separated files>
Concern: <severity - message, only if any>
Blocker: <message, only if any>

AO2 run spec:
```json
{spec_json}
```
{operator_section}"#,
        plan_id = spec.plan_id,
        source_schema = spec.source_schema,
        target_repo = spec.target_repo.display(),
        task_count = spec.tasks.len(),
        spec_json = spec_json,
        operator_section = operator_section
    ))
}

fn materialize_ao2_run_spec_workflow(spec: &Ao2RunSpec) -> Result<PathBuf> {
    let workflow_dir = spec.target_repo.join(".ao2").join("generated-workflows");
    fs::create_dir_all(&workflow_dir)
        .with_context(|| format!("create {}", workflow_dir.display()))?;
    let workflow_path = workflow_dir.join(format!(
        "{}-sdd-run.yaml",
        sanitize_greenfield_id(&spec.plan_id)
    ));
    let body = spec
        .value
        .get("spec")
        .and_then(serde_json::Value::as_object)
        .context("ao2 run spec body is required")?;
    let goal = body
        .get("goal")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Execute AO2 SDD plan through governed AO2 runtime.");
    let verifier_command = body
        .get("exit_criteria")
        .and_then(|exit| exit.get("tests"))
        .and_then(serde_json::Value::as_array)
        .and_then(|tests| tests.iter().find_map(serde_json::Value::as_str))
        .or_else(|| {
            body.get("exit_criteria")
                .and_then(|exit| exit.get("gates"))
                .and_then(serde_json::Value::as_array)
                .and_then(|gates| gates.iter().find_map(serde_json::Value::as_str))
        })
        .context("ao2 run spec exit_criteria.tests or exit_criteria.gates is required")?;
    let workflow_tasks = spec
        .tasks
        .iter()
        .map(ao2_run_spec_workflow_task)
        .collect::<Result<Vec<_>>>()?;
    let mut roles = workflow_tasks
        .iter()
        .filter_map(|task| task.get("role").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !roles.iter().any(|role| role == "evaluator-closer") {
        roles.push("evaluator-closer".to_string());
    }
    let dependencies = spec
        .tasks
        .iter()
        .flat_map(ao2_run_spec_workflow_dependencies)
        .collect::<Vec<_>>();
    let acceptance = ao2_run_spec_acceptance(body, &spec.tasks);
    let workflow = serde_json::json!({
        "id": format!("ao2-sdd-{}", sanitize_greenfield_id(&spec.plan_id)),
        "name": format!("AO2 SDD governed run {}", spec.plan_id),
        "version": "0.1.0",
        "template_kind": "real_project",
        "objective": goal,
        "roles": roles,
        "tasks": workflow_tasks,
        "dependencies": dependencies,
        "factory_v3_compatibility": {
            "source_schema": spec.source_schema,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "input_spec": spec.path.display().to_string(),
            "input_spec_sha256": spec.spec_sha256
        },
        "verifier": {
            "command": verifier_command
        },
        "acceptance": acceptance,
        "policy": {
            "deny_by_default": true,
            "approval_mode": "exact_action_digest",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    atomic_write_text(&workflow_path, &serde_yaml::to_string(&workflow)?)?;
    Ok(workflow_path)
}

fn ao2_run_spec_workflow_task(task: &serde_json::Value) -> Result<serde_json::Value> {
    let id = task
        .get("id")
        .and_then(serde_json::Value::as_str)
        .context("ao2 run spec task.id is required")?;
    let mut workflow_task = serde_json::json!({
        "id": id,
        "role": id,
        "kind": task.get("kind").and_then(serde_json::Value::as_str).unwrap_or("agent"),
        "provider": "scripted",
        "paths": task.get("paths").cloned().unwrap_or_else(|| serde_json::json!([])),
        "rationale": task.get("rationale").and_then(serde_json::Value::as_str).unwrap_or(""),
        "acceptance": task.get("acceptance").cloned().unwrap_or_else(|| serde_json::json!([])),
        "policy_profile": "ao2-sdd-run-task"
    });
    if let Some(provider_free) = task.get("provider_free") {
        workflow_task["provider_free"] = provider_free.clone();
    }
    Ok(workflow_task)
}

fn ao2_run_spec_workflow_dependencies(task: &serde_json::Value) -> Vec<serde_json::Value> {
    let Some(to) = task.get("id").and_then(serde_json::Value::as_str) else {
        return Vec::new();
    };
    task.get("deps")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(|from| {
            serde_json::json!({
                "from": from,
                "to": to,
                "source": "ao2.run/v1 task deps"
            })
        })
        .collect()
}

fn ao2_run_spec_acceptance(
    body: &serde_json::Map<String, serde_json::Value>,
    tasks: &[serde_json::Value],
) -> Vec<String> {
    let mut acceptance = Vec::new();
    if let Some(exit) = body.get("exit_criteria") {
        for key in ["tests", "gates", "manual"] {
            if let Some(values) = exit.get(key).and_then(serde_json::Value::as_array) {
                for value in values {
                    if let Some(value) = value.as_str() {
                        acceptance.push(format!("{key}: {value}"));
                    }
                }
            }
        }
    }
    for task in tasks {
        if let Some(values) = task.get("acceptance").and_then(serde_json::Value::as_array) {
            for value in values {
                if let Some(value) = value.as_str() {
                    acceptance.push(value.to_string());
                }
            }
        }
    }
    if acceptance.is_empty() {
        acceptance.push("verifier command passes".to_string());
        acceptance.push("replay has zero digest failures".to_string());
    }
    acceptance
}
