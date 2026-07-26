use crate::cli::RepairCommand;
use crate::cli_util::{json_array, json_string, read_prompt, run_dir};
use crate::provider_ops::materialize_template_workflow;
use crate::run_reporting::print_run_summary;
use anyhow::{Context, Result};
use ao2_adapters::parse_provider;
use ao2_runtime::{
    approve_risky_pr_ticket, replay_run, resume_risky_pr_provider_free,
    run_risky_pr_with_provider_prompt, ApprovalOptions, ProviderRunOptions, RepairSourceContext,
    ReplayOptions, ResumeOptions,
};
use std::path::{Path, PathBuf};
use std::{collections::BTreeSet, fs};

pub(crate) fn approve_and_resume_persisted_sandbox_patches(
    target: &Path,
    run_id: &str,
    approver: &str,
) -> Result<Option<ao2_runtime::RunSummary>> {
    let mut latest = None;
    loop {
        let evidence_pack_path = run_dir(target, run_id)
            .join("evidence-pack")
            .join("evidence-pack.json");
        let evidence = match fs::read_to_string(&evidence_pack_path) {
            Ok(content) => serde_json::from_str::<serde_json::Value>(&content)
                .with_context(|| format!("parse {}", evidence_pack_path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(latest),
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", evidence_pack_path.display()))
            }
        };
        let pending_ticket_id = json_array(&evidence, "approvals")
            .iter()
            .find(|ticket| {
                json_string(ticket, "status") == "pending"
                    && json_string(ticket, "requested_action") == "sandbox:apply"
            })
            .map(|ticket| json_string(ticket, "ticket_id"))
            .filter(|ticket_id| !ticket_id.is_empty());
        let Some(ticket_id) = pending_ticket_id else {
            return Ok(latest);
        };
        approve_risky_pr_ticket(ApprovalOptions {
            target_repo: target.to_path_buf(),
            ticket_id,
            approver: approver.to_string(),
        })?;
        let summary = resume_risky_pr_provider_free(ResumeOptions {
            target_repo: target.to_path_buf(),
            run_id: run_id.to_string(),
        })?;
        let still_waiting = summary.status == ao2_runtime::RunStatus::WaitingForApproval;
        latest = Some(summary);
        if !still_waiting {
            return Ok(latest);
        }
    }
}

pub(crate) struct ApprovalRecoveryContext {
    ticket_id: String,
    run_id: String,
    action_digest: String,
    evidence_dir: PathBuf,
    target: PathBuf,
}

fn read_approval_recovery_context(
    target: &Path,
    approval_path: &Path,
) -> Option<ApprovalRecoveryContext> {
    let text = fs::read_to_string(approval_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ticket = value.get("ticket")?;
    let ticket_id = ticket.get("ticket_id")?.as_str()?.to_string();
    let run_id = ticket.get("run_id")?.as_str()?.to_string();
    let action_digest = ticket.get("action_digest")?.as_str()?.to_string();
    let evidence_dir = approval_path.parent()?.parent()?.to_path_buf();
    Some(ApprovalRecoveryContext {
        ticket_id,
        run_id,
        action_digest,
        evidence_dir,
        target: target.to_path_buf(),
    })
}

fn approval_recovery_context_by_ticket(
    target: &Path,
    ticket_id: &str,
) -> Option<ApprovalRecoveryContext> {
    let runs_dir = target.join(".ao2").join("runs");
    for entry in fs::read_dir(runs_dir).ok()? {
        let run_dir = entry.ok()?.path();
        let approval_path = run_dir.join("approvals").join(format!("{ticket_id}.json"));
        if approval_path.is_file() {
            return read_approval_recovery_context(target, &approval_path);
        }
    }
    None
}

pub(crate) fn pending_approval_recovery_context(
    target: &Path,
    run_id: &str,
) -> Option<ApprovalRecoveryContext> {
    let approvals_dir = target
        .join(".ao2")
        .join("runs")
        .join(run_id)
        .join("approvals");
    for entry in fs::read_dir(approvals_dir).ok()? {
        let approval_path = entry.ok()?.path();
        if approval_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&approval_path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        if value["ticket"]["status"].as_str() == Some("pending") {
            return read_approval_recovery_context(target, &approval_path);
        }
    }
    None
}

pub(crate) fn print_approval_recovery_context(
    context: &ApprovalRecoveryContext,
    approval_status: &str,
    digest_failure: Option<&str>,
) {
    eprintln!("approval_status={approval_status}");
    eprintln!("required_digest_field=action_digest");
    eprintln!("action_digest={}", context.action_digest);
    if let Some(digest_failure) = digest_failure {
        eprintln!("digest_failure={digest_failure}");
    }
    eprintln!("replay_state=waiting_for_approval");
    eprintln!("evidence_dir={}", context.evidence_dir.display());
    eprintln!(
        "next_step=ao2 approve {} --target {} --approver <operator>; ao2 run --resume {} --target {}",
        context.ticket_id,
        context.target.display(),
        context.run_id,
        context.target.display()
    );
    eprintln!("recovery=preserve the failing state and compare the required action_digest before retrying");
}

pub(crate) fn approve(target: PathBuf, ticket_id: String, approver: String) -> Result<()> {
    match approve_risky_pr_ticket(ApprovalOptions {
        target_repo: target.clone(),
        ticket_id: ticket_id.clone(),
        approver,
    }) {
        Ok(approval) => {
            println!("ticket_id={}", approval.ticket_id);
            println!("status={}", approval.status);
            println!("approver={}", approval.approver.unwrap_or_default());
            Ok(())
        }
        Err(error) => {
            if error.to_string().contains("approval digest mismatch") {
                if let Some(context) = approval_recovery_context_by_ticket(&target, &ticket_id) {
                    print_approval_recovery_context(
                        &context,
                        "rejected",
                        Some("approval digest mismatch"),
                    );
                }
            }
            Err(error)
        }
    }
}

pub(crate) fn replay(target: PathBuf, run_id: String) -> Result<()> {
    let summary = replay_run(ReplayOptions {
        target_repo: target,
        run_id,
    })?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub(crate) fn repair(command: RepairCommand) -> Result<()> {
    match command {
        RepairCommand::Resume {
            evidence_pack,
            workflow,
            template,
            target,
            run_id,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            max_repair_attempts,
            json,
        } => {
            let workflow = workflow.map(Ok).unwrap_or_else(|| {
                let template = template
                    .as_deref()
                    .context("--workflow or --template is required")?;
                materialize_template_workflow(&target, template)
            })?;
            let provider = parse_provider(provider.as_deref().unwrap_or("scripted"))?;
            let prompt = read_prompt(provider_prompt, provider_prompt_file)?;
            let repair_source = repair_source_context_from_evidence_pack(&evidence_pack)?;
            let source_run_id = repair_source.source_run_id.clone();
            let summary = run_risky_pr_with_provider_prompt(ProviderRunOptions {
                target_repo: target,
                workflow_path: workflow,
                run_id,
                provider,
                prompt,
                max_repair_attempts,
                max_budget_usd: provider_max_budget_usd,
                repair_source: Some(repair_source),
            })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "ao2.repair-resume.v1",
                        "source_run_id": source_run_id,
                        "run_id": summary.run_id,
                        "status": summary.status,
                        "evidence_pack": summary.evidence_pack_path,
                        "report": summary.report_path,
                        "rejection_count": summary.rejection_count
                    }))?
                );
            } else {
                println!("source_run_id={source_run_id}");
                print_run_summary(&summary);
            }
            Ok(())
        }
    }
}

pub(crate) fn repair_source_context_from_evidence_pack(path: &Path) -> Result<RepairSourceContext> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let evidence: serde_json::Value =
        serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))?;
    let schema_version = json_string(&evidence, "schema_version");
    if schema_version != "ao2.evidence-pack.v1" {
        anyhow::bail!("repair resume requires ao2.evidence-pack.v1, got {schema_version}");
    }
    let source_verdict = json_string(&evidence, "verdict");
    if source_verdict == "accepted" {
        anyhow::bail!("repair resume requires a non-accepted source evidence pack");
    }
    let source_run_id = json_string(&evidence, "run_id");
    if source_run_id.is_empty() {
        anyhow::bail!("repair resume source evidence pack is missing run_id");
    }
    let run_health = evidence
        .get("run_health")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "schema_version": "ao2.run-health.v1" }));
    let mut unresolved_concerns = string_values(
        run_health
            .get("unresolved_concerns")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    );
    if unresolved_concerns.is_empty() {
        unresolved_concerns = unresolved_concerns_from_closures(&evidence);
    }
    let evidence_refs = string_values(
        run_health
            .get("evidence_refs")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    );
    Ok(RepairSourceContext {
        source_run_id,
        evidence_pack_path: path.to_path_buf(),
        source_verdict,
        run_health,
        unresolved_concerns,
        evidence_refs,
        latest_verifier_output: latest_artifact_content(&evidence, "test_log"),
    })
}

fn unresolved_concerns_from_closures(evidence: &serde_json::Value) -> Vec<String> {
    let mut concerns = BTreeSet::new();
    for closure in json_array(evidence, "closures") {
        for concern in json_array(closure, "unresolved_concerns") {
            if let Some(text) = concern.as_str() {
                concerns.insert(text.to_string());
            }
        }
    }
    concerns.into_iter().collect()
}

fn string_values(values: &[serde_json::Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn latest_artifact_content(evidence: &serde_json::Value, artifact_type: &str) -> Option<String> {
    json_array(evidence, "artifacts")
        .iter()
        .rev()
        .find(|artifact| json_string(artifact, "artifact_type") == artifact_type)
        .and_then(|artifact| {
            let uri = json_string(artifact, "uri");
            if uri.is_empty() {
                None
            } else {
                fs::read_to_string(uri).ok()
            }
        })
}
