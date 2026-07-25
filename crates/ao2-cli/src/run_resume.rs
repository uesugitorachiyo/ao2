use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use ao2_runtime::{
    approve_risky_pr_ticket, resume_risky_pr_provider_free, ApprovalOptions, ResumeOptions,
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
