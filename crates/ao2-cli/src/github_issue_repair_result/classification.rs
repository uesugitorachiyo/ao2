use super::github_issue_draft::read_bounded_bytes;
use crate::cli::RepairResultCommand;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_AGE_DAYS: i64 = 7;
const MAX_FUTURE_SKEW_MINUTES: i64 = 5;
const MAX_FAILURES: usize = 256;
const MAX_IDENTIFIER_BYTES: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Verification {
    schema_version: String,
    role: String,
    repository: String,
    issue_number: u64,
    baseline_source_sha: String,
    source_sha: String,
    candidate_sha: Option<String>,
    command_sha256: String,
    toolchain: Toolchain,
    completed_at: String,
    exit_code: u8,
    output_sha256: String,
    failures: Vec<Failure>,
    safety: Safety,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Toolchain {
    name: String,
    version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    identifier: String,
    signature_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Safety {
    network: String,
    credentials_present: bool,
    git_history_present: bool,
    oracle_present: bool,
    external_effects: u64,
}

#[derive(Debug, Serialize)]
struct ChangedFailure {
    identifier: String,
    baseline_signature_sha256: String,
    candidate_signature_sha256: String,
}

#[derive(Debug, Serialize)]
struct Readback<'a> {
    schema_version: &'static str,
    classification: &'static str,
    repository: &'a str,
    issue_number: u64,
    baseline_source_sha: &'a str,
    candidate_sha: &'a str,
    command_sha256: &'a str,
    baseline_evidence_sha256: String,
    candidate_evidence_sha256: String,
    baseline_exit_code: u8,
    candidate_exit_code: u8,
    baseline_failures_retained: bool,
    candidate_regression: bool,
    shared_failures: Vec<Failure>,
    resolved_failures: Vec<Failure>,
    changed_failures: Vec<ChangedFailure>,
    candidate_only_failures: Vec<Failure>,
    network_performed: bool,
    git_performed: bool,
    github_performed: bool,
    provider_calls: u8,
    repair_performed: bool,
    mutation_performed: bool,
    approval_granted: bool,
    release_performed: bool,
    deployment_performed: bool,
    publication_performed: bool,
}

pub(crate) fn run(command: RepairResultCommand) -> Result<()> {
    match command {
        RepairResultCommand::Classify {
            baseline,
            candidate,
            json,
        } => classify(&baseline, &candidate, json),
    }
}

fn classify(baseline_path: &Path, candidate_path: &Path, json: bool) -> Result<()> {
    let baseline_bytes = read_bounded_bytes(baseline_path)?;
    let candidate_bytes = read_bounded_bytes(candidate_path)?;
    let baseline: Verification = serde_json::from_slice(&baseline_bytes).with_context(|| {
        format!(
            "parse strict baseline JSON from {}",
            baseline_path.display()
        )
    })?;
    let candidate: Verification = serde_json::from_slice(&candidate_bytes).with_context(|| {
        format!(
            "parse strict candidate JSON from {}",
            candidate_path.display()
        )
    })?;

    validate(&baseline, "baseline")?;
    validate(&candidate, "candidate")?;
    if baseline.repository != candidate.repository
        || baseline.issue_number != candidate.issue_number
        || baseline.baseline_source_sha != candidate.baseline_source_sha
        || baseline.command_sha256 != candidate.command_sha256
        || baseline.toolchain != candidate.toolchain
    {
        bail!("baseline and candidate comparison identities do not match");
    }

    let baseline_failures = failure_map(&baseline.failures)?;
    let candidate_failures = failure_map(&candidate.failures)?;
    let identifiers: BTreeSet<_> = baseline_failures
        .keys()
        .chain(candidate_failures.keys())
        .cloned()
        .collect();
    let mut shared = Vec::new();
    let mut resolved = Vec::new();
    let mut changed = Vec::new();
    let mut candidate_only = Vec::new();

    for identifier in identifiers {
        match (
            baseline_failures.get(&identifier),
            candidate_failures.get(&identifier),
        ) {
            (Some(baseline), Some(candidate))
                if baseline.signature_sha256 == candidate.signature_sha256 =>
            {
                shared.push((*candidate).clone());
            }
            (Some(baseline), Some(candidate)) => changed.push(ChangedFailure {
                identifier,
                baseline_signature_sha256: baseline.signature_sha256.clone(),
                candidate_signature_sha256: candidate.signature_sha256.clone(),
            }),
            (Some(baseline), None) => resolved.push((*baseline).clone()),
            (None, Some(candidate)) => candidate_only.push((*candidate).clone()),
            (None, None) => unreachable!(),
        }
    }

    let candidate_regression = !changed.is_empty() || !candidate_only.is_empty();
    let classification = if candidate_regression {
        "candidate_regression_detected"
    } else if !shared.is_empty() {
        "candidate_has_only_exact_baseline_failures"
    } else if !resolved.is_empty() {
        "candidate_resolved_baseline_failures"
    } else {
        "candidate_clean"
    };
    let candidate_sha = candidate
        .candidate_sha
        .as_deref()
        .expect("validated candidate SHA");
    let readback = Readback {
        schema_version: "ao2.github-issue-repair-result-classification.v1",
        classification,
        repository: &baseline.repository,
        issue_number: baseline.issue_number,
        baseline_source_sha: &baseline.baseline_source_sha,
        candidate_sha,
        command_sha256: &baseline.command_sha256,
        baseline_evidence_sha256: digest(&baseline_bytes),
        candidate_evidence_sha256: digest(&candidate_bytes),
        baseline_exit_code: baseline.exit_code,
        candidate_exit_code: candidate.exit_code,
        baseline_failures_retained: !shared.is_empty(),
        candidate_regression,
        shared_failures: shared,
        resolved_failures: resolved,
        changed_failures: changed,
        candidate_only_failures: candidate_only,
        network_performed: false,
        git_performed: false,
        github_performed: false,
        provider_calls: 0,
        repair_performed: false,
        mutation_performed: false,
        approval_granted: false,
        release_performed: false,
        deployment_performed: false,
        publication_performed: false,
    };

    if json {
        println!("{}", serde_json::to_string(&readback)?);
    } else {
        println!("classification={}", readback.classification);
        println!("candidate_regression={}", readback.candidate_regression);
        println!(
            "baseline_failures_retained={}",
            readback.baseline_failures_retained
        );
    }
    Ok(())
}

fn validate(verification: &Verification, expected_role: &str) -> Result<()> {
    if verification.schema_version != "ao2.github-issue-repair-verification.v1" {
        bail!("unsupported verification schema_version");
    }
    if verification.role != expected_role {
        bail!("verification role must be {expected_role}");
    }
    validate_repository(&verification.repository)?;
    if verification.issue_number == 0 {
        bail!("issue_number must be positive");
    }
    if !is_sha(&verification.baseline_source_sha) || !is_sha(&verification.source_sha) {
        bail!("source SHA fields must be exactly 40 lowercase hexadecimal characters");
    }
    match expected_role {
        "baseline"
            if verification.candidate_sha.is_some()
                || verification.source_sha != verification.baseline_source_sha =>
        {
            bail!("baseline must omit candidate_sha and use baseline_source_sha as source_sha");
        }
        "candidate" => {
            let candidate_sha = verification
                .candidate_sha
                .as_deref()
                .context("candidate must include candidate_sha")?;
            if !is_sha(candidate_sha)
                || candidate_sha != verification.source_sha
                || candidate_sha == verification.baseline_source_sha
            {
                bail!("candidate_sha must be a distinct exact source_sha");
            }
        }
        _ => {}
    }
    if !is_digest(&verification.command_sha256) || !is_digest(&verification.output_sha256) {
        bail!("command and output digests must use lowercase sha256:<64 hex>");
    }
    validate_text("toolchain.name", &verification.toolchain.name, 128)?;
    validate_text("toolchain.version", &verification.toolchain.version, 128)?;
    validate_fresh_timestamp(&verification.completed_at)?;
    if verification.failures.len() > MAX_FAILURES {
        bail!("failures exceed {MAX_FAILURES}-item limit");
    }
    if verification.failures.is_empty() != (verification.exit_code == 0) {
        bail!("exit_code must be zero exactly when failures is empty");
    }
    failure_map(&verification.failures)?;
    if verification.safety.network != "none"
        || verification.safety.credentials_present
        || verification.safety.git_history_present
        || verification.safety.oracle_present
        || verification.safety.external_effects != 0
    {
        bail!("verification safety boundary is not offline and effect-free");
    }
    Ok(())
}

fn failure_map(failures: &[Failure]) -> Result<BTreeMap<String, &Failure>> {
    let mut result = BTreeMap::new();
    for failure in failures {
        validate_text(
            "failure identifier",
            &failure.identifier,
            MAX_IDENTIFIER_BYTES,
        )?;
        if !is_digest(&failure.signature_sha256) {
            bail!("failure signature must use lowercase sha256:<64 hex>");
        }
        if result.insert(failure.identifier.clone(), failure).is_some() {
            bail!("failure identifiers must be unique");
        }
    }
    Ok(result)
}

pub(super) fn validate_repository(repository: &str) -> Result<()> {
    let mut parts = repository.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        bail!("repository must use canonical owner/name syntax");
    };
    if owner.is_empty()
        || owner.len() > 39
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !owner
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !owner
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        || owner.contains("--")
    {
        bail!("repository owner must use canonical GitHub owner grammar");
    }
    if name.is_empty()
        || name.len() > 100
        || matches!(name, "." | "..")
        || name.ends_with('.')
        || name.to_ascii_lowercase().ends_with(".git")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("repository name must use canonical GitHub repository grammar");
    }
    Ok(())
}

pub(super) fn validate_text(name: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        bail!("{name} must be printable, nonempty, and at most {max_bytes} bytes");
    }
    Ok(())
}

pub(super) fn validate_fresh_timestamp(value: &str) -> Result<()> {
    let completed_at = DateTime::parse_from_rfc3339(value)
        .context("completed_at must use RFC3339 timestamp syntax")?
        .with_timezone(&Utc);
    let now = Utc::now();
    if completed_at < now - Duration::days(MAX_AGE_DAYS) {
        bail!("completed_at is stale");
    }
    if completed_at > now + Duration::minutes(MAX_FUTURE_SKEW_MINUTES) {
        bail!("completed_at is too far in the future");
    }
    Ok(())
}

pub(super) fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn is_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
