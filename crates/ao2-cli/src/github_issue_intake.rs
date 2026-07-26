use anyhow::Result;
use serde::Serialize;

use super::{canonical_json_sha256, github_issue_draft};
use crate::cli::IssueCommand;

#[derive(Debug, Serialize)]
struct GitHubIssueIntakeReadback {
    schema_version: &'static str,
    state: String,
    input_url: String,
    canonical_url: Option<String>,
    host: Option<String>,
    owner: Option<String>,
    repo: Option<String>,
    issue_number: Option<u64>,
    rejection: Option<String>,
    command_policy_class: &'static str,
    github_read_performed: bool,
    github_write_performed: bool,
    feature_generated_pr_opened: bool,
    issue_write_performed: bool,
    maintainer_contacted: bool,
}

pub(super) fn issue(command: IssueCommand) -> Result<()> {
    match command {
        IssueCommand::Intake { url, json } => issue_intake(&url, json),
        IssueCommand::Acquire {
            url,
            upstream_url,
            default_branch,
            target_commit,
            json,
        } => issue_acquire(&url, &upstream_url, &default_branch, &target_commit, json),
        IssueCommand::DraftPr { command } => {
            github_issue_draft::run(command, canonical_json_sha256)
        }
    }
}

fn issue_intake(url: &str, json: bool) -> Result<()> {
    let readback = canonicalize_github_issue_url(url);
    if json {
        println!("{}", serde_json::to_string_pretty(&readback)?);
    } else if let Some(canonical_url) = &readback.canonical_url {
        println!("state={}", readback.state);
        println!("canonical_url={canonical_url}");
    } else {
        println!("state={}", readback.state);
        if let Some(rejection) = &readback.rejection {
            println!("rejection={rejection}");
        }
    }
    Ok(())
}

fn canonicalize_github_issue_url(input: &str) -> GitHubIssueIntakeReadback {
    let mut readback = GitHubIssueIntakeReadback {
        schema_version: "ao2.github-issue-intake.v0.1",
        state: "invalid_url".to_string(),
        input_url: input.to_string(),
        canonical_url: None,
        host: None,
        owner: None,
        repo: None,
        issue_number: None,
        rejection: None,
        command_policy_class: "safe_read_only_discovery",
        github_read_performed: false,
        github_write_performed: false,
        feature_generated_pr_opened: false,
        issue_write_performed: false,
        maintainer_contacted: false,
    };
    let trimmed = input.trim();
    let without_fragment = trimmed.split_once('#').map_or(trimmed, |(base, _)| base);
    let without_query = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(base, _)| base);
    let Some(after_scheme) = without_query
        .strip_prefix("https://")
        .or_else(|| without_query.strip_prefix("http://"))
    else {
        readback.rejection = Some("missing_or_unsupported_scheme".to_string());
        return readback;
    };
    let (host, path) = match after_scheme.split_once('/') {
        Some((host, path)) => (host.to_ascii_lowercase(), path),
        None => (after_scheme.to_ascii_lowercase(), ""),
    };
    readback.host = Some(host.clone());
    if host != "github.com" {
        readback.state = "unsupported_host".to_string();
        readback.rejection = Some("unsupported_host".to_string());
        return readback;
    }
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments
        .iter()
        .any(|segment| *segment == "." || *segment == "..")
    {
        readback.rejection = Some("traversal".to_string());
        return readback;
    }
    if segments.len() != 4 || segments[2] != "issues" {
        readback.rejection = Some(
            match segments.get(2).copied() {
                Some("pull") => "pull_request_url",
                Some("discussions") => "discussion_url",
                Some("actions") => "actions_url",
                _ => "not_a_github_issue_url",
            }
            .to_string(),
        );
        return readback;
    }
    let Ok(issue_number) = segments[3].parse::<u64>() else {
        readback.rejection = Some("malformed_number".to_string());
        return readback;
    };
    if issue_number == 0 {
        readback.rejection = Some("malformed_number".to_string());
        return readback;
    }
    let owner = segments[0].to_string();
    let repo = segments[1].to_string();
    readback.state = "intake_validated".to_string();
    readback.owner = Some(owner.clone());
    readback.repo = Some(repo.clone());
    readback.issue_number = Some(issue_number);
    readback.canonical_url = Some(format!(
        "https://github.com/{owner}/{repo}/issues/{issue_number}"
    ));
    readback
}

#[derive(Debug, Serialize)]
struct GitHubIssueAcquisitionReadback {
    schema_version: &'static str,
    state: String,
    issue: GitHubIssueIntakeReadback,
    repository_identity: GitHubIssueRepositoryIdentity,
    acquisition: GitHubIssueAcquisitionPolicy,
    denied_actions: GitHubIssueAcquisitionDeniedActions,
    operator_next_action: &'static str,
}

#[derive(Debug, Serialize)]
struct GitHubIssueRepositoryIdentity {
    upstream_url: String,
    default_branch: String,
    target_commit: String,
    target_commit_valid: bool,
    upstream_matches_issue_repository: bool,
}

#[derive(Debug, Serialize)]
struct GitHubIssueAcquisitionPolicy {
    hooks_disabled: bool,
    safe_git_config: bool,
    controlled_remotes: bool,
    dependency_network_policy: &'static str,
    sanitized_environment: bool,
    log_capture: bool,
    size_policy: &'static str,
    archive_policy: &'static str,
    path_policy: &'static str,
    symlink_policy: &'static str,
    binary_policy: &'static str,
    lfs_policy: &'static str,
    submodule_policy: &'static str,
    mutation_policy: &'static str,
}

#[derive(Debug, Serialize)]
struct GitHubIssueAcquisitionDeniedActions {
    github_write_performed: bool,
    feature_generated_pr_opened: bool,
    issue_write_performed: bool,
    maintainer_contacted: bool,
    provider_call_performed: bool,
    repository_mutated: bool,
}

fn issue_acquire(
    url: &str,
    upstream_url: &str,
    default_branch: &str,
    target_commit: &str,
    json: bool,
) -> Result<()> {
    let readback = plan_github_issue_acquisition(url, upstream_url, default_branch, target_commit);
    if json {
        println!("{}", serde_json::to_string_pretty(&readback)?);
    } else {
        println!("state={}", readback.state);
        println!(
            "target_commit={}",
            readback.repository_identity.target_commit
        );
        println!(
            "upstream_matches_issue_repository={}",
            readback
                .repository_identity
                .upstream_matches_issue_repository
        );
        println!("mutation_policy={}", readback.acquisition.mutation_policy);
    }
    Ok(())
}

fn plan_github_issue_acquisition(
    url: &str,
    upstream_url: &str,
    default_branch: &str,
    target_commit: &str,
) -> GitHubIssueAcquisitionReadback {
    let issue = canonicalize_github_issue_url(url);
    let expected_upstream = issue
        .owner
        .as_ref()
        .zip(issue.repo.as_ref())
        .map(|(owner, repo)| format!("https://github.com/{owner}/{repo}.git"));
    let target_commit_valid = is_full_hex_sha(target_commit);
    let upstream_matches_issue_repository =
        expected_upstream.as_deref() == Some(upstream_url.trim());
    let state = if issue.state == "intake_validated"
        && target_commit_valid
        && upstream_matches_issue_repository
        && !default_branch.trim().is_empty()
    {
        "acquisition_planned"
    } else {
        "policy_blocked"
    };
    GitHubIssueAcquisitionReadback {
        schema_version: "ao2.github-issue-acquisition-plan.v0.1",
        state: state.to_string(),
        issue,
        repository_identity: GitHubIssueRepositoryIdentity {
            upstream_url: upstream_url.to_string(),
            default_branch: default_branch.to_string(),
            target_commit: target_commit.to_string(),
            target_commit_valid,
            upstream_matches_issue_repository,
        },
        acquisition: GitHubIssueAcquisitionPolicy {
            hooks_disabled: true,
            safe_git_config: true,
            controlled_remotes: true,
            dependency_network_policy: "deny_by_default_operator_approved_allowlist_only",
            sanitized_environment: true,
            log_capture: true,
            size_policy: "bounded_before_checkout",
            archive_policy: "no_untrusted_archive_extraction_without_manifest",
            path_policy: "reject_absolute_parent_traversal_and_control_paths",
            symlink_policy: "inspect_before_following",
            binary_policy: "do_not_execute_untrusted_binary",
            lfs_policy: "disabled_until_operator_allows",
            submodule_policy: "disabled_until_operator_allows",
            mutation_policy: "read_only_acquisition_plan_no_clone_or_checkout_performed_by_this_readback",
        },
        denied_actions: GitHubIssueAcquisitionDeniedActions {
            github_write_performed: false,
            feature_generated_pr_opened: false,
            issue_write_performed: false,
            maintainer_contacted: false,
            provider_call_performed: false,
            repository_mutated: false,
        },
        operator_next_action:
            "review acquisition policy, then run reproduction only in an isolated workspace with hooks disabled",
    }
}

fn is_full_hex_sha(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}
