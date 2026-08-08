use super::github_issue_repair_pack::{read_guarded_file, RootGuard};
use super::github_issue_repair_result::{
    digest, is_digest, is_sha, validate_fresh_timestamp, validate_repository, validate_text,
};
use crate::cli::RepairQualificationCommand;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[path = "github_issue_repair_qualification/evidence_files.rs"]
mod evidence_files;
#[path = "github_issue_repair_qualification/process_lifecycle.rs"]
mod process_lifecycle;

use evidence_files::{
    validate_digests as validate_artifacts, validate_files as validate_artifact_files,
};
use process_lifecycle::{validate as validate_process_lifecycle, ProcessLifecycle};

const BUNDLE_SCHEMA_V1: &str = "ao2.github-issue-repair-qualification-bundle.v1";
const BUNDLE_SCHEMA_V2: &str = "ao2.github-issue-repair-qualification-bundle.v2";
const READBACK_SCHEMA_V1: &str = "ao2.github-issue-repair-qualification.v1";
const READBACK_SCHEMA_V2: &str = "ao2.github-issue-repair-qualification.v2";
const MAX_INPUT_BYTES: u64 = 65_536;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    schema_version: String,
    repository: String,
    upstream_repository_id: String,
    operator_owner: String,
    issue_number: u64,
    baseline_source_sha: String,
    candidate_sha: String,
    #[serde(default)]
    qualification_profile: Option<String>,
    source: Source,
    reproduction: Reproduction,
    regression: Regression,
    full_suite: FullSuite,
    candidate_seal: CandidateSeal,
    review: Review,
    draft_pr: DraftPr,
    #[serde(default)]
    process_lifecycle: Option<ProcessLifecycle>,
    artifact_sha256: BTreeMap<String, String>,
    safety: Safety,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Source {
    fetched_at: String,
    source_archive_sha256: String,
    issue_snapshot_sha256: String,
    dependency_cache_manifest_sha256: String,
    extracted_tree_sha256: String,
    toolchain: Toolchain,
    platforms: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Toolchain {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Reproduction {
    completed_at: String,
    command_sha256: String,
    output_sha256: String,
    failure_signature_sha256: String,
    exit_code: i32,
    network: String,
    credentials_present: bool,
    git_history_present: bool,
    oracle_present: bool,
    external_effects: u64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Regression {
    completed_at: String,
    command_sha256: String,
    identifier: String,
    baseline_exit_code: i32,
    baseline_output_sha256: String,
    candidate_exit_code: i32,
    candidate_output_sha256: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct FullSuite {
    completed_at: String,
    baseline_evidence_sha256: String,
    candidate_evidence_sha256: String,
    classification_evidence_sha256: String,
    classification: String,
    candidate_regression: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct CandidateSeal {
    sealed_at: String,
    patch_sha256: String,
    tree_sha256: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Review {
    completed_at: String,
    evidence_sha256: String,
    status: String,
    unresolved_p1: u64,
    unresolved_p2: u64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct DraftPr {
    captured_at: String,
    evidence_sha256: String,
    repository: String,
    repository_id: String,
    owner: String,
    is_fork: bool,
    parent_repository: String,
    parent_repository_id: String,
    number: u64,
    state: String,
    is_draft: bool,
    merged: bool,
    head_sha: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundArtifact<T> {
    repository: String,
    upstream_repository_id: String,
    issue_number: u64,
    baseline_source_sha: String,
    candidate_sha: String,
    evidence: T,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Safety {
    network: String,
    credentials_present: bool,
    git_history_present: bool,
    oracle_present: bool,
    provider_calls: u64,
    external_effects: u64,
    upstream_branch_mutations: u64,
    upstream_pull_request_mutations: u64,
    upstream_issue_comment_mutations: u64,
    release_mutations: u64,
    deployment_mutations: u64,
    publication_mutations: u64,
}

#[derive(Debug, Serialize)]
struct Readback<'a> {
    schema_version: &'static str,
    result: &'static str,
    repository: &'a str,
    upstream_repository_id: &'a str,
    operator_owner: &'a str,
    issue_number: u64,
    baseline_source_sha: &'a str,
    candidate_sha: &'a str,
    toolchain_name: &'a str,
    toolchain_version: &'a str,
    platforms: &'a [String],
    source_fetched_at: &'a str,
    source_archive_sha256: &'a str,
    issue_snapshot_sha256: &'a str,
    dependency_cache_manifest_sha256: &'a str,
    extracted_tree_sha256: &'a str,
    reproduction_completed_at: &'a str,
    reproduction_command_sha256: &'a str,
    reproduction_output_sha256: &'a str,
    reproduction_failure_signature_sha256: &'a str,
    reproduction_exit_code: i32,
    regression_completed_at: &'a str,
    regression_command_sha256: &'a str,
    regression_identifier: &'a str,
    regression_baseline_exit_code: i32,
    regression_baseline_output_sha256: &'a str,
    regression_candidate_exit_code: i32,
    regression_candidate_output_sha256: &'a str,
    full_suite_completed_at: &'a str,
    baseline_evidence_sha256: &'a str,
    candidate_evidence_sha256: &'a str,
    classification_evidence_sha256: &'a str,
    classification: &'a str,
    candidate_sealed_at: &'a str,
    patch_sha256: &'a str,
    tree_sha256: &'a str,
    review_completed_at: &'a str,
    review_evidence_sha256: &'a str,
    review_status: &'a str,
    draft_pr_captured_at: &'a str,
    draft_pr_evidence_sha256: &'a str,
    draft_pr_repository: &'a str,
    draft_pr_repository_id: &'a str,
    draft_pr_parent_repository: &'a str,
    draft_pr_parent_repository_id: &'a str,
    draft_pr_number: u64,
    draft_pr_head_sha: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    qualification_profile: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_lifecycle_completed_at: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_lifecycle_evidence_sha256: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_lifecycle_passed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    orphan_processes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u64>,
    artifact_sha256: &'a BTreeMap<String, String>,
    bundle_sha256: &'a str,
    qualification_digest: String,
    candidate_regression: bool,
    network_performed: bool,
    git_performed: bool,
    github_performed: bool,
    provider_calls: u64,
    repair_performed: bool,
    mutation_performed: bool,
    approval_granted: bool,
    release_performed: bool,
    deployment_performed: bool,
    publication_performed: bool,
}

#[derive(Debug, Serialize)]
struct RejectedReadback {
    schema_version: &'static str,
    result: &'static str,
    rejection_reason: &'static str,
    mutation_performed: bool,
    approval_granted: bool,
    release_performed: bool,
    deployment_performed: bool,
    publication_performed: bool,
}

pub(crate) fn run(command: RepairQualificationCommand) -> Result<()> {
    match command {
        RepairQualificationCommand::Verify { bundle, json } => match verify(&bundle, json) {
            Ok(()) => Ok(()),
            Err(error) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string(&RejectedReadback {
                            schema_version: READBACK_SCHEMA_V1,
                            result: "repair_rejected",
                            rejection_reason: "invalid_bundle",
                            mutation_performed: false,
                            approval_granted: false,
                            release_performed: false,
                            deployment_performed: false,
                            publication_performed: false,
                        })?
                    );
                }
                Err(error)
            }
        },
    }
}

fn verify(path: &Path, json: bool) -> Result<()> {
    let parent = path
        .parent()
        .context("qualification bundle must have a parent directory")?;
    let root_path = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let bundle_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("qualification bundle name must be UTF-8")?;
    let root = RootGuard::open(root_path)?;
    let bytes = read_guarded_file(&root, bundle_name, MAX_INPUT_BYTES, "qualification bundle")?;
    let bundle: Bundle =
        serde_json::from_slice(&bytes).context("parse strict qualification JSON")?;
    validate(&bundle)?;
    validate_artifact_files(&root, bundle_name, &bundle)?;
    root.validate_root_identity()?;
    let bundle_sha256 = digest(&bytes);
    let readback_schema = if bundle.schema_version == BUNDLE_SCHEMA_V2 {
        READBACK_SCHEMA_V2
    } else {
        READBACK_SCHEMA_V1
    };
    let qualification_digest = digest(format!("{readback_schema}\n{bundle_sha256}").as_bytes());
    let process_lifecycle = bundle.process_lifecycle.as_ref();
    let readback = Readback {
        schema_version: readback_schema,
        result: "repair_qualified",
        repository: &bundle.repository,
        upstream_repository_id: &bundle.upstream_repository_id,
        operator_owner: &bundle.operator_owner,
        issue_number: bundle.issue_number,
        baseline_source_sha: &bundle.baseline_source_sha,
        candidate_sha: &bundle.candidate_sha,
        toolchain_name: &bundle.source.toolchain.name,
        toolchain_version: &bundle.source.toolchain.version,
        platforms: &bundle.source.platforms,
        source_fetched_at: &bundle.source.fetched_at,
        source_archive_sha256: &bundle.source.source_archive_sha256,
        issue_snapshot_sha256: &bundle.source.issue_snapshot_sha256,
        dependency_cache_manifest_sha256: &bundle.source.dependency_cache_manifest_sha256,
        extracted_tree_sha256: &bundle.source.extracted_tree_sha256,
        reproduction_completed_at: &bundle.reproduction.completed_at,
        reproduction_command_sha256: &bundle.reproduction.command_sha256,
        reproduction_output_sha256: &bundle.reproduction.output_sha256,
        reproduction_failure_signature_sha256: &bundle.reproduction.failure_signature_sha256,
        reproduction_exit_code: bundle.reproduction.exit_code,
        regression_completed_at: &bundle.regression.completed_at,
        regression_command_sha256: &bundle.regression.command_sha256,
        regression_identifier: &bundle.regression.identifier,
        regression_baseline_exit_code: bundle.regression.baseline_exit_code,
        regression_baseline_output_sha256: &bundle.regression.baseline_output_sha256,
        regression_candidate_exit_code: bundle.regression.candidate_exit_code,
        regression_candidate_output_sha256: &bundle.regression.candidate_output_sha256,
        full_suite_completed_at: &bundle.full_suite.completed_at,
        baseline_evidence_sha256: &bundle.full_suite.baseline_evidence_sha256,
        candidate_evidence_sha256: &bundle.full_suite.candidate_evidence_sha256,
        classification_evidence_sha256: &bundle.full_suite.classification_evidence_sha256,
        classification: &bundle.full_suite.classification,
        candidate_sealed_at: &bundle.candidate_seal.sealed_at,
        patch_sha256: &bundle.candidate_seal.patch_sha256,
        tree_sha256: &bundle.candidate_seal.tree_sha256,
        review_completed_at: &bundle.review.completed_at,
        review_evidence_sha256: &bundle.review.evidence_sha256,
        review_status: &bundle.review.status,
        draft_pr_captured_at: &bundle.draft_pr.captured_at,
        draft_pr_evidence_sha256: &bundle.draft_pr.evidence_sha256,
        draft_pr_repository: &bundle.draft_pr.repository,
        draft_pr_repository_id: &bundle.draft_pr.repository_id,
        draft_pr_parent_repository: &bundle.draft_pr.parent_repository,
        draft_pr_parent_repository_id: &bundle.draft_pr.parent_repository_id,
        draft_pr_number: bundle.draft_pr.number,
        draft_pr_head_sha: &bundle.draft_pr.head_sha,
        qualification_profile: bundle.qualification_profile.as_deref(),
        process_lifecycle_completed_at: process_lifecycle.map(|value| value.completed_at.as_str()),
        process_lifecycle_evidence_sha256: process_lifecycle
            .map(|value| value.evidence_sha256.as_str()),
        process_lifecycle_passed: process_lifecycle.map(|_| true),
        orphan_processes: process_lifecycle.map(|value| value.orphan_processes),
        timeout_seconds: process_lifecycle.map(|value| value.timeout_seconds),
        artifact_sha256: &bundle.artifact_sha256,
        bundle_sha256: &bundle_sha256,
        qualification_digest,
        candidate_regression: false,
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
        println!("result={}", readback.result);
        println!("qualification_digest={}", readback.qualification_digest);
    }
    Ok(())
}

fn validate(bundle: &Bundle) -> Result<()> {
    match bundle.schema_version.as_str() {
        BUNDLE_SCHEMA_V1 => {
            if bundle.qualification_profile.is_some() || bundle.process_lifecycle.is_some() {
                bail!("v1 qualification must not contain a process lifecycle profile");
            }
        }
        BUNDLE_SCHEMA_V2 => {
            if bundle.qualification_profile.as_deref() != Some("process_lifecycle") {
                bail!("v2 qualification_profile must be process_lifecycle");
            }
            let lifecycle = bundle
                .process_lifecycle
                .as_ref()
                .context("process_lifecycle is required for the v2 profile")?;
            validate_process_lifecycle(lifecycle)?;
        }
        _ => bail!("unsupported qualification schema_version"),
    }
    validate_repository(&bundle.repository)?;
    validate_text("operator_owner", &bundle.operator_owner, 128)?;
    validate_text(
        "upstream_repository_id",
        &bundle.upstream_repository_id,
        256,
    )?;
    if bundle.operator_owner.contains('/') {
        bail!("operator owner and upstream repository identity must be explicit");
    }
    if bundle.issue_number == 0 {
        bail!("issue_number must be positive");
    }
    if !is_sha(&bundle.baseline_source_sha)
        || !is_sha(&bundle.candidate_sha)
        || bundle.baseline_source_sha == bundle.candidate_sha
    {
        bail!("baseline and candidate SHAs must be distinct exact source identities");
    }
    validate_source(&bundle.source)?;
    validate_reproduction(&bundle.reproduction)?;
    validate_regression(&bundle.regression)?;
    validate_full_suite(&bundle.full_suite)?;
    validate_candidate_seal(&bundle.candidate_seal)?;
    validate_review(&bundle.review)?;
    validate_draft(
        &bundle.repository,
        &bundle.upstream_repository_id,
        &bundle.operator_owner,
        &bundle.candidate_sha,
        &bundle.draft_pr,
    )?;
    validate_lifecycle_order(bundle)?;
    validate_artifacts(&bundle.artifact_sha256, bundle.process_lifecycle.is_some())?;
    validate_safety(&bundle.safety)?;
    Ok(())
}

fn validate_source(source: &Source) -> Result<()> {
    validate_fresh_timestamp(&source.fetched_at)?;
    for value in [
        &source.source_archive_sha256,
        &source.issue_snapshot_sha256,
        &source.dependency_cache_manifest_sha256,
        &source.extracted_tree_sha256,
    ] {
        if !is_digest(value) {
            bail!("source evidence digests must use lowercase sha256:<64 hex>");
        }
    }
    validate_text("toolchain.name", &source.toolchain.name, 128)?;
    validate_text("toolchain.version", &source.toolchain.version, 128)?;
    if source.platforms.is_empty() || source.platforms.len() > 8 {
        bail!("platforms must contain between one and eight entries");
    }
    for platform in &source.platforms {
        validate_text("platform", platform, 128)?;
    }
    Ok(())
}

fn validate_reproduction(value: &Reproduction) -> Result<()> {
    validate_fresh_timestamp(&value.completed_at)?;
    for digest_value in [
        &value.command_sha256,
        &value.output_sha256,
        &value.failure_signature_sha256,
    ] {
        if !is_digest(digest_value) {
            bail!("reproduction digests must use lowercase sha256:<64 hex>");
        }
    }
    if value.exit_code == 0 {
        bail!("reproduction must retain a nonzero observed exit code");
    }
    if value.network != "none"
        || value.credentials_present
        || value.git_history_present
        || value.oracle_present
        || value.external_effects != 0
    {
        bail!("reproduction safety boundary is not offline and effect-free");
    }
    Ok(())
}

fn validate_regression(value: &Regression) -> Result<()> {
    validate_fresh_timestamp(&value.completed_at)?;
    validate_text("regression.identifier", &value.identifier, 256)?;
    for digest_value in [
        &value.command_sha256,
        &value.baseline_output_sha256,
        &value.candidate_output_sha256,
    ] {
        if !is_digest(digest_value) {
            bail!("regression digests must use lowercase sha256:<64 hex>");
        }
    }
    if value.baseline_exit_code == 0 || value.candidate_exit_code != 0 {
        bail!("regression must bind observed baseline RED and candidate GREEN exits");
    }
    Ok(())
}

fn validate_full_suite(value: &FullSuite) -> Result<()> {
    validate_fresh_timestamp(&value.completed_at)?;
    for digest_value in [
        &value.baseline_evidence_sha256,
        &value.candidate_evidence_sha256,
        &value.classification_evidence_sha256,
    ] {
        if !is_digest(digest_value) {
            bail!("full-suite digests must use lowercase sha256:<64 hex>");
        }
    }
    if value.candidate_regression
        || !matches!(
            value.classification.as_str(),
            "candidate_clean"
                | "candidate_resolved_baseline_failures"
                | "candidate_has_only_exact_baseline_failures"
        )
    {
        bail!("full-suite classification contains a candidate regression");
    }
    Ok(())
}

fn validate_candidate_seal(value: &CandidateSeal) -> Result<()> {
    validate_fresh_timestamp(&value.sealed_at)?;
    if !is_digest(&value.patch_sha256) || !is_digest(&value.tree_sha256) {
        bail!("candidate seal digests must use lowercase sha256:<64 hex>");
    }
    Ok(())
}

fn validate_review(value: &Review) -> Result<()> {
    validate_fresh_timestamp(&value.completed_at)?;
    if !is_digest(&value.evidence_sha256) {
        bail!("review evidence digest must use lowercase sha256:<64 hex>");
    }
    if !matches!(
        value.status.as_str(),
        "no_findings" | "findings_resolved" | "no_findings_after_correction"
    ) || value.unresolved_p1 != 0
        || value.unresolved_p2 != 0
    {
        bail!("independent review has unresolved P1 or P2 findings");
    }
    Ok(())
}

fn validate_draft(
    upstream: &str,
    upstream_repository_id: &str,
    operator_owner: &str,
    candidate_sha: &str,
    value: &DraftPr,
) -> Result<()> {
    validate_fresh_timestamp(&value.captured_at)?;
    validate_repository(&value.repository)?;
    if !is_digest(&value.evidence_sha256) {
        bail!("draft PR evidence digest must use lowercase sha256:<64 hex>");
    }
    let Some((draft_owner, _)) = value.repository.split_once('/') else {
        bail!("operator-fork PR repository must be canonical");
    };
    if draft_owner != operator_owner || value.owner != operator_owner {
        bail!("operator-fork PR must match the authorized operator owner");
    }
    if value.repository == upstream
        || !value.is_fork
        || value.parent_repository != upstream
        || value.parent_repository_id != upstream_repository_id
        || value.repository_id.is_empty()
        || value.repository_id == upstream_repository_id
        || value.number == 0
        || value.state != "OPEN"
        || !value.is_draft
        || value.merged
        || value.head_sha != candidate_sha
    {
        bail!(
            "operator-fork PR fork provenance must be open, draft, unmerged, and exact-head bound"
        );
    }
    Ok(())
}

fn validate_lifecycle_order(bundle: &Bundle) -> Result<()> {
    let mut timestamps = vec![
        ("source", bundle.source.fetched_at.as_str()),
        ("reproduction", bundle.reproduction.completed_at.as_str()),
        ("regression", bundle.regression.completed_at.as_str()),
    ];
    if let Some(lifecycle) = &bundle.process_lifecycle {
        timestamps.push(("process_lifecycle", lifecycle.completed_at.as_str()));
    }
    timestamps.extend([
        ("full_suite", bundle.full_suite.completed_at.as_str()),
        ("candidate_seal", bundle.candidate_seal.sealed_at.as_str()),
        ("review", bundle.review.completed_at.as_str()),
        ("draft_pr", bundle.draft_pr.captured_at.as_str()),
    ]);
    let mut previous: Option<(&str, DateTime<FixedOffset>)> = None;
    for (label, value) in timestamps {
        let parsed = DateTime::parse_from_rfc3339(value)
            .with_context(|| format!("parse {label} timestamp for lifecycle order"))?;
        if let Some((previous_label, previous_time)) = &previous {
            if parsed < *previous_time {
                bail!("evidence lifecycle order is invalid: {label} predates {previous_label}");
            }
        }
        previous = Some((label, parsed));
    }
    Ok(())
}

fn validate_safety(value: &Safety) -> Result<()> {
    if value.network != "none"
        || value.credentials_present
        || value.git_history_present
        || value.oracle_present
        || value.provider_calls != 0
        || value.external_effects != 0
        || value.upstream_branch_mutations != 0
        || value.upstream_pull_request_mutations != 0
        || value.upstream_issue_comment_mutations != 0
        || value.release_mutations != 0
        || value.deployment_mutations != 0
        || value.publication_mutations != 0
    {
        bail!("qualification safety boundary is not offline and mutation-free");
    }
    Ok(())
}
