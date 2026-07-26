use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ao2_runtime::{
    approve_risky_pr_ticket, replay_run, resume_risky_pr_provider_free, ApprovalOptions,
    ReplayOptions, ResumeOptions,
};

use crate::cli_util::{json_array, json_string, run_dir};

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
