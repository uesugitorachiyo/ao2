use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Subcommand;
use serde::Serialize;

use ao2_runtime::github_issue_publication::{
    apply_publication_plan, decode_publication_plan_strict, verify_publication_plan,
};

#[derive(Debug, Subcommand)]
pub(crate) enum PublishCommand {
    /// Verify a publication plan without contacting GitHub or changing Git.
    Verify {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        expected_push_action_digest: String,
        #[arg(long)]
        expected_draft_action_digest: String,
        #[arg(long)]
        json: bool,
    },
    /// Apply a verified plan using ambient GitHub and Git authentication.
    Apply {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        repository: PathBuf,
        #[arg(long)]
        expected_push_action_digest: String,
        #[arg(long)]
        expected_draft_action_digest: String,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: PublishCommand) -> Result<()> {
    let (plan_path, push, draft, json, repository) = match command {
        PublishCommand::Verify {
            plan,
            expected_push_action_digest,
            expected_draft_action_digest,
            json,
        } => (
            plan,
            expected_push_action_digest,
            expected_draft_action_digest,
            json,
            None,
        ),
        PublishCommand::Apply {
            plan,
            repository,
            expected_push_action_digest,
            expected_draft_action_digest,
            json,
        } => (
            plan,
            expected_push_action_digest,
            expected_draft_action_digest,
            json,
            Some(repository),
        ),
    };
    let bytes = crate::github_issue_draft::read_bounded_bytes(&plan_path)
        .context("publication plan is malformed, unsafe, or oversized")?;
    let plan = decode_publication_plan_strict(&bytes)
        .context("publication plan contains invalid, unknown, or duplicate fields")?;
    let now = Utc::now();
    match repository {
        Some(root) => emit(
            &apply_publication_plan(&plan, &root, &push, &draft, now)?,
            json,
        ),
        None => emit(&verify_publication_plan(&plan, &push, &draft, now)?, json),
    }
}

fn emit(value: &impl Serialize, pretty: bool) -> Result<()> {
    println!(
        "{}",
        if pretty {
            serde_json::to_string_pretty(value)?
        } else {
            serde_json::to_string(value)?
        }
    );
    Ok(())
}
