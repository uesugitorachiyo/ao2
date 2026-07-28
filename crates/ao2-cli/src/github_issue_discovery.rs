use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::cli_util::canonical_json_sha256;

const MAX_INPUT_BYTES: usize = 1_048_576;
const MAX_PAGE_COUNT: usize = 10;
const MAX_RAW_ROW_COUNT: usize = 500;
const MAX_SNAPSHOT_LIMIT: usize = 50;
const MAX_CANDIDATE_LIMIT: usize = 10;
const SELECTED_LIMIT: usize = 1;
const DISCOVERY_SCHEMA: &str = "ao.architecture.autonomous-issue-repair.discovery-result.v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageEnvelope {
    repository: String,
    default_branch: String,
    head_sha: String,
    pages: Vec<IssuePage>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IssuePage {
    page: u32,
    issues: Vec<SanitizedIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SanitizedIssue {
    number: u64,
    state: String,
    updated_at: String,
    title: String,
    body: String,
    labels: Vec<String>,
    classification: String,
    reported_head_sha: String,
    fix_present_at_head: bool,
    environment_accessible: bool,
    security_sensitive: bool,
    target_in_repository: bool,
    no_existing_fix: bool,
    public_reproduction_feasible: bool,
    deterministic_local_reproduction: bool,
    expected_behavior_source: String,
    bounded_policy_compatible: bool,
}

#[derive(Debug, Serialize)]
struct DiscoveryResult {
    schema: &'static str,
    run_id: String,
    repository: String,
    default_branch: String,
    head_sha: String,
    source_url: String,
    snapshot_limit: usize,
    candidate_limit: usize,
    selected_limit: usize,
    page_count: usize,
    response_digests: Vec<String>,
    issues: Vec<DiscoveryIssue>,
    candidates: Vec<DiscoveryCandidate>,
    selected_issue_number: Option<u64>,
    exclusion_ledger: Vec<ExclusionLedgerEntry>,
    mutation_performed: bool,
    completed_at: String,
}

#[derive(Debug, Serialize)]
struct DiscoveryIssue {
    number: u64,
    state: &'static str,
    updated_at: String,
    content_digest: String,
}

#[derive(Debug, Serialize)]
struct DiscoveryCandidate {
    issue_number: u64,
    rank: usize,
    decision_digest: String,
}

#[derive(Debug, Serialize)]
struct CandidateDecision {
    schema: &'static str,
    run_id: String,
    repository: String,
    base_sha: String,
    issue_number: u64,
    rank: usize,
    decision: &'static str,
    eligibility: CandidateEligibility,
    reason_codes: Vec<String>,
    evidence_digests: Vec<String>,
    expected_behavior_source: String,
    decided_at: String,
    decision_digest: String,
}

#[derive(Debug, Serialize)]
struct CandidateEligibility {
    open_bug: bool,
    target_in_repository: bool,
    no_existing_fix: bool,
    current_head_unfixed: bool,
    security_sensitive: bool,
    public_reproduction_feasible: bool,
    deterministic_local_reproduction: bool,
    expected_behavior_grounded: bool,
    bounded_policy_compatible: bool,
}

#[derive(Debug, Serialize)]
struct ExclusionLedgerEntry {
    issue_number: u64,
    reason_codes: Vec<String>,
    evidence_digests: Vec<String>,
}

#[derive(Clone)]
struct IssueRecord {
    issue: SanitizedIssue,
    content_digest: String,
    updated_at: DateTime<Utc>,
    evidence_digests: BTreeSet<String>,
}

pub(crate) struct DiscoveryRequest<'a> {
    pub(crate) page_envelope: &'a Path,
    pub(crate) url: &'a str,
    pub(crate) repository: &'a str,
    pub(crate) default_branch: &'a str,
    pub(crate) head_sha: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) completed_at: &'a str,
    pub(crate) snapshot_limit: usize,
    pub(crate) candidate_limit: usize,
    pub(crate) json: bool,
}

pub(super) fn run(request: DiscoveryRequest<'_>) -> Result<()> {
    let result = discover(
        request.page_envelope,
        request.url,
        request.repository,
        request.default_branch,
        request.head_sha,
        request.run_id,
        request.completed_at,
        request.snapshot_limit,
        request.candidate_limit,
    )?;
    if request.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "selected_issue_number={}",
            result
                .selected_issue_number
                .map(|number| number.to_string())
                .unwrap_or_else(|| "null".to_string())
        );
        println!("candidate_count={}", result.candidates.len());
        println!("excluded_count={}", result.exclusion_ledger.len());
        println!("mutation_performed=false");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn discover(
    page_envelope: &Path,
    url: &str,
    repository: &str,
    default_branch: &str,
    head_sha: &str,
    run_id: &str,
    completed_at: &str,
    snapshot_limit: usize,
    candidate_limit: usize,
) -> Result<DiscoveryResult> {
    validate_limits(snapshot_limit, candidate_limit)?;
    let repository = validate_repository(repository)?;
    let default_branch = required_text("--default-branch", default_branch, 255)?;
    let head_sha = validate_sha("--head-sha", head_sha)?;
    validate_run_id(run_id)?;
    let completed_at = validate_timestamp("--completed-at", completed_at)?;
    let completed_at_value = DateTime::parse_from_rfc3339(&completed_at)?.with_timezone(&Utc);
    let source_url = canonicalize_issue_list_url(url, &repository)?;
    let envelope = read_page_envelope(page_envelope)?;
    if envelope.repository != repository
        || envelope.default_branch != default_branch
        || envelope.head_sha != head_sha
    {
        return Err(anyhow!(
            "page envelope identity does not match command identity"
        ));
    }
    if envelope.pages.is_empty() || envelope.pages.len() > MAX_PAGE_COUNT {
        return Err(anyhow!(
            "page envelope must contain 1..={MAX_PAGE_COUNT} pages"
        ));
    }

    let mut pages = envelope.pages;
    pages.sort_by_key(|page| page.page);
    let mut response_digests = Vec::with_capacity(pages.len());
    let mut rows = Vec::new();
    for (index, page) in pages.into_iter().enumerate() {
        if page.page as usize != index + 1 {
            return Err(anyhow!(
                "page numbers must be ordered exactly 1..=page_count"
            ));
        }
        let page_value = serde_json::to_value(&page)?;
        let page_digest = canonical_json_sha256(&page_value);
        response_digests.push(page_digest.clone());
        for issue in page.issues {
            rows.push(record_issue(
                issue,
                &page_digest,
                &head_sha,
                completed_at_value,
            )?);
        }
    }
    if rows.len() > MAX_RAW_ROW_COUNT {
        return Err(anyhow!("sanitized issue rows exceed raw row limit"));
    }

    let mut records = BTreeMap::<u64, IssueRecord>::new();
    let mut duplicates = BTreeSet::new();
    for record in rows {
        if let Some(existing) = records.remove(&record.issue.number) {
            duplicates.insert(record.issue.number);
            let choose_new = record.content_digest < existing.content_digest;
            let mut evidence_digests = existing.evidence_digests.clone();
            evidence_digests.extend(record.evidence_digests.iter().cloned());
            let mut canonical = if choose_new { record } else { existing };
            canonical.evidence_digests = evidence_digests;
            records.insert(canonical.issue.number, canonical);
        } else {
            records.insert(record.issue.number, record);
        }
    }
    if records.len() > snapshot_limit {
        return Err(anyhow!("unique sanitized issue rows exceed snapshot_limit"));
    }

    let mut eligible = Vec::new();
    let mut exclusions = BTreeMap::<u64, Vec<String>>::new();
    for (number, record) in &records {
        if duplicates.contains(number) {
            exclusions.insert(*number, vec!["duplicate_issue_number".to_string()]);
        } else if let Some(reason_codes) = exclusion_reason_codes(record, &head_sha)? {
            exclusions.insert(*number, reason_codes);
        } else {
            eligible.push(record.clone());
        }
    }
    eligible.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.issue.number.cmp(&right.issue.number))
            .then_with(|| left.content_digest.cmp(&right.content_digest))
    });
    let selected_records: Vec<IssueRecord> =
        eligible.iter().take(candidate_limit).cloned().collect();
    for record in eligible.iter().skip(candidate_limit) {
        exclusions.insert(
            record.issue.number,
            vec!["candidate_limit_exceeded".to_string()],
        );
    }
    let candidate_decisions = selected_records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            candidate_decision(
                record,
                &repository,
                &head_sha,
                run_id,
                index + 1,
                &completed_at,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let candidates = candidate_decisions
        .iter()
        .map(|candidate| DiscoveryCandidate {
            issue_number: candidate.issue_number,
            rank: candidate.rank,
            decision_digest: candidate.decision_digest.clone(),
        })
        .collect::<Vec<_>>();
    let selected_issue_number = candidates.first().map(|candidate| candidate.issue_number);
    for candidate in candidates.iter().skip(SELECTED_LIMIT) {
        exclusions.insert(
            candidate.issue_number,
            vec![format!("not_selected_rank_{}", candidate.rank)],
        );
    }

    let issues = records
        .values()
        .map(|record| DiscoveryIssue {
            number: record.issue.number,
            state: "open",
            updated_at: record.issue.updated_at.clone(),
            content_digest: record.content_digest.clone(),
        })
        .collect();
    let exclusion_ledger = records
        .iter()
        .filter_map(|(number, record)| {
            if Some(*number) == selected_issue_number {
                return None;
            }
            let reason_codes = exclusions
                .remove(number)
                .unwrap_or_else(|| vec!["not_selected_candidate".to_string()]);
            Some(ExclusionLedgerEntry {
                issue_number: *number,
                reason_codes,
                evidence_digests: evidence_digests(record),
            })
        })
        .collect();

    Ok(DiscoveryResult {
        schema: DISCOVERY_SCHEMA,
        run_id: run_id.to_string(),
        repository,
        default_branch,
        head_sha,
        source_url,
        snapshot_limit,
        candidate_limit,
        selected_limit: SELECTED_LIMIT,
        page_count: response_digests.len(),
        response_digests,
        issues,
        candidates,
        selected_issue_number,
        exclusion_ledger,
        mutation_performed: false,
        completed_at,
    })
}

fn read_page_envelope(path: &Path) -> Result<PageEnvelope> {
    let mut file = open_bounded_input(path)?;
    let metadata = validate_opened_input(&file, path)?;
    validate_path_matches_opened(path, &metadata)?;
    #[cfg(windows)]
    let opened_identity = crate::windows_input::disk_file_identity(&file, path)?;
    let mut bytes = Vec::with_capacity(metadata.len().min(MAX_INPUT_BYTES as u64) as usize);
    file.by_ref()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {}", path.display()))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(anyhow!("page envelope exceeds {MAX_INPUT_BYTES} bytes"));
    }
    let after = validate_opened_input(&file, path)?;
    validate_path_matches_opened(path, &after)?;
    #[cfg(windows)]
    validate_windows_path_identity(path, opened_identity)?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

fn open_bounded_input(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        options.custom_flags(crate::windows_input::open_flags());
    }
    options.open(path).with_context(|| {
        format!(
            "open regular page envelope without following links {}",
            path.display()
        )
    })
}

fn validate_opened_input(file: &fs::File, path: &Path) -> Result<fs::Metadata> {
    #[cfg(windows)]
    {
        crate::windows_input::validate_disk_handle(file, path)?;
        crate::windows_input::validate_non_reparse_disk_handle(file, path)?;
    }
    let metadata = file
        .metadata()
        .with_context(|| format!("inspect opened page envelope {}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(anyhow!(
            "page envelope must be a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_INPUT_BYTES as u64 {
        return Err(anyhow!("page envelope exceeds {MAX_INPUT_BYTES} bytes"));
    }
    Ok(metadata)
}

fn validate_path_matches_opened(path: &Path, opened: &fs::Metadata) -> Result<()> {
    let path_metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect page envelope path {}", path.display()))?;
    if !path_metadata.file_type().is_file() {
        return Err(anyhow!(
            "page envelope path must remain a regular file: {}",
            path.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        if path_metadata.dev() != opened.dev() || path_metadata.ino() != opened.ino() {
            return Err(anyhow!(
                "page envelope path changed while reading: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn validate_windows_path_identity(
    path: &Path,
    opened_identity: crate::windows_input::DiskFileIdentity,
) -> Result<()> {
    let reopened = open_bounded_input(path)?;
    validate_opened_input(&reopened, path)?;
    let reopened_identity = crate::windows_input::disk_file_identity(&reopened, path)?;
    if reopened_identity != opened_identity {
        return Err(anyhow!(
            "page envelope path changed while reading: {}",
            path.display()
        ));
    }
    Ok(())
}

fn record_issue(
    issue: SanitizedIssue,
    page_digest: &str,
    head_sha: &str,
    completed_at: DateTime<Utc>,
) -> Result<IssueRecord> {
    if issue.number == 0 || issue.state != "open" {
        return Err(anyhow!(
            "sanitized rows must have a positive open issue number"
        ));
    }
    validate_sanitized_text("issue title", &issue.title, 512)?;
    validate_sanitized_text("issue body", &issue.body, 8_192)?;
    if issue.labels.len() > 20 {
        return Err(anyhow!("sanitized issue labels exceed 20"));
    }
    for label in &issue.labels {
        validate_sanitized_text("issue label", label, 64)?;
    }
    validate_sha("reported_head_sha", &issue.reported_head_sha)?;
    let updated_at =
        DateTime::parse_from_rfc3339(&validate_timestamp("issue updated_at", &issue.updated_at)?)?
            .with_timezone(&Utc);
    if updated_at > completed_at {
        return Err(anyhow!("issue updated_at must not be after completed_at"));
    }
    if issue.fix_present_at_head == issue.no_existing_fix {
        return Err(anyhow!(
            "fix_present_at_head and no_existing_fix must be logical opposites"
        ));
    }
    if (issue.classification == "already_fixed") != issue.fix_present_at_head {
        return Err(anyhow!(
            "already_fixed classification must match exact-current-head fix evidence"
        ));
    }
    if issue.fix_present_at_head && issue.reported_head_sha != head_sha {
        return Err(anyhow!(
            "already-fixed evidence must bind to the exact head_sha"
        ));
    }
    if (issue.classification == "inaccessible_environment") == issue.environment_accessible {
        return Err(anyhow!(
            "inaccessible_environment classification must match environment_accessible"
        ));
    }
    if (issue.classification == "security_sensitive") != issue.security_sensitive {
        return Err(anyhow!(
            "security_sensitive classification must match security_sensitive evidence"
        ));
    }
    validate_classification(&issue.classification)?;
    validate_expected_behavior_source(&issue.expected_behavior_source)?;
    let content_digest = canonical_json_sha256(&serde_json::to_value(&issue)?);
    let mut evidence_digests = BTreeSet::new();
    evidence_digests.insert(content_digest.clone());
    evidence_digests.insert(page_digest.to_string());
    Ok(IssueRecord {
        updated_at,
        issue,
        content_digest,
        evidence_digests,
    })
}

fn candidate_decision(
    record: &IssueRecord,
    repository: &str,
    head_sha: &str,
    run_id: &str,
    rank: usize,
    completed_at: &str,
) -> Result<CandidateDecision> {
    let eligibility = candidate_eligibility(record, head_sha);
    let decision = if rank == 1 { "selected" } else { "eligible" };
    let mut candidate = CandidateDecision {
        schema: "ao.architecture.autonomous-issue-repair.candidate-decision.v1",
        run_id: run_id.to_string(),
        repository: repository.to_string(),
        base_sha: head_sha.to_string(),
        issue_number: record.issue.number,
        rank,
        decision,
        eligibility,
        reason_codes: vec![if rank == 1 {
            "selected_rank_1".to_string()
        } else {
            format!("eligible_rank_{rank}")
        }],
        evidence_digests: evidence_digests(record),
        expected_behavior_source: record.issue.expected_behavior_source.clone(),
        decided_at: completed_at.to_string(),
        decision_digest: String::new(),
    };
    let mut value = serde_json::to_value(&candidate)?;
    value
        .as_object_mut()
        .expect("candidate decision serializes to an object")
        .remove("decision_digest");
    candidate.decision_digest = canonical_json_sha256(&value);
    Ok(candidate)
}

fn evidence_digests(record: &IssueRecord) -> Vec<String> {
    record.evidence_digests.iter().cloned().collect()
}

fn candidate_eligibility(record: &IssueRecord, head_sha: &str) -> CandidateEligibility {
    CandidateEligibility {
        open_bug: record.issue.state == "open" && record.issue.classification == "bug",
        target_in_repository: record.issue.target_in_repository,
        no_existing_fix: record.issue.no_existing_fix,
        current_head_unfixed: record.issue.reported_head_sha == head_sha
            && record.issue.no_existing_fix
            && !record.issue.fix_present_at_head,
        security_sensitive: record.issue.security_sensitive,
        public_reproduction_feasible: record.issue.public_reproduction_feasible,
        deterministic_local_reproduction: record.issue.deterministic_local_reproduction,
        expected_behavior_grounded: record.issue.expected_behavior_source != "unavailable",
        bounded_policy_compatible: record.issue.bounded_policy_compatible,
    }
}

fn exclusion_reason_codes(record: &IssueRecord, head_sha: &str) -> Result<Option<Vec<String>>> {
    let eligibility = candidate_eligibility(record, head_sha);
    let mut reasons = Vec::new();
    if !eligibility.open_bug {
        push_reason(
            &mut reasons,
            classification_exclusion(&record.issue.classification)?.to_string(),
        );
    }
    if !eligibility.target_in_repository {
        push_reason(&mut reasons, "target_outside_repository".to_string());
    }
    if !eligibility.no_existing_fix {
        push_reason(&mut reasons, "existing_fix_present".to_string());
    }
    if !eligibility.current_head_unfixed {
        push_reason(
            &mut reasons,
            if record.issue.reported_head_sha != head_sha {
                "reported_head_mismatch".to_string()
            } else {
                "current_head_fixed".to_string()
            },
        );
    }
    if eligibility.security_sensitive {
        push_reason(&mut reasons, "security_sensitive".to_string());
    }
    if !eligibility.public_reproduction_feasible {
        push_reason(&mut reasons, "public_reproduction_unavailable".to_string());
    }
    if !eligibility.deterministic_local_reproduction {
        push_reason(
            &mut reasons,
            "deterministic_local_reproduction_unavailable".to_string(),
        );
    }
    if !eligibility.expected_behavior_grounded {
        push_reason(&mut reasons, "expected_behavior_unavailable".to_string());
    }
    if !eligibility.bounded_policy_compatible {
        push_reason(&mut reasons, "bounded_policy_incompatible".to_string());
    }
    Ok((!reasons.is_empty()).then_some(reasons))
}

fn push_reason(reasons: &mut Vec<String>, reason: String) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

fn validate_limits(snapshot_limit: usize, candidate_limit: usize) -> Result<()> {
    if !(1..=MAX_SNAPSHOT_LIMIT).contains(&snapshot_limit) {
        return Err(anyhow!("snapshot_limit must be 1..={MAX_SNAPSHOT_LIMIT}"));
    }
    if !(1..=MAX_CANDIDATE_LIMIT).contains(&candidate_limit) {
        return Err(anyhow!("candidate_limit must be 1..={MAX_CANDIDATE_LIMIT}"));
    }
    Ok(())
}

fn canonicalize_issue_list_url(input: &str, repository: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed != input || !trimmed.starts_with("https://github.com/") {
        return Err(anyhow!("issue list URL must use https://github.com"));
    }
    let without_fragment = trimmed
        .split_once('#')
        .map_or(trimmed, |(before, _)| before);
    let without_query = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(before, _)| before);
    let base = without_query.strip_suffix('/').unwrap_or(without_query);
    let path = base
        .strip_prefix("https://github.com/")
        .ok_or_else(|| anyhow!("issue list URL must use github.com"))?;
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.len() != 3
        || segments[2] != "issues"
        || segments.iter().any(|part| !is_repo_part(part))
    {
        return Err(anyhow!(
            "issue list URL must be an unambiguous repository issues surface"
        ));
    }
    let url_repository = format!("{}/{}", segments[0], segments[1]);
    if url_repository != repository {
        return Err(anyhow!(
            "issue list URL repository does not match --repository"
        ));
    }
    Ok(format!("https://github.com/{url_repository}/issues"))
}

fn validate_repository(value: &str) -> Result<String> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| !is_repo_part(part)) {
        return Err(anyhow!(
            "--repository must be OWNER/REPO using safe GitHub path characters"
        ));
    }
    Ok(value.to_string())
}

fn is_repo_part(value: &&str) -> bool {
    !value.is_empty()
        && *value != "."
        && *value != ".."
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

fn validate_run_id(value: &str) -> Result<()> {
    let valid = (8..=128).contains(&value.len())
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || (index > 0 && character == '-')
        });
    if !valid {
        return Err(anyhow!("--run-id must match [a-z0-9][a-z0-9-]{{7,127}}"));
    }
    Ok(())
}

fn validate_sha(flag: &str, value: &str) -> Result<String> {
    if value.len() != 40
        || !value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        return Err(anyhow!(
            "{flag} must be a 40-character lowercase hexadecimal SHA"
        ));
    }
    Ok(value.to_string())
}

fn validate_timestamp(flag: &str, value: &str) -> Result<String> {
    DateTime::parse_from_rfc3339(value).with_context(|| format!("{flag} must be RFC3339"))?;
    Ok(value.to_string())
}

fn required_text(flag: &str, value: &str, max_len: usize) -> Result<String> {
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(anyhow!(
            "{flag} must be non-empty, bounded, and control-character free"
        ));
    }
    Ok(value.to_string())
}

fn validate_sanitized_text(field: &str, value: &str, max_len: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.chars().any(char::is_control)
        || value.contains("[UNTRUSTED_INSTRUCTION_MARKER]")
    {
        return Err(anyhow!("{field} is not a bounded sanitized value"));
    }
    Ok(())
}

fn validate_classification(value: &str) -> Result<()> {
    classification_exclusion(value).map(|_| ())
}

fn validate_expected_behavior_source(value: &str) -> Result<()> {
    if !matches!(
        value,
        "tests" | "documentation" | "protocol" | "maintainer_statement" | "unavailable"
    ) {
        return Err(anyhow!("unsupported expected_behavior_source"));
    }
    Ok(())
}

fn classification_exclusion(value: &str) -> Result<&'static str> {
    match value {
        "bug" => Ok("bug"),
        "duplicate" => Ok("duplicate"),
        "already_fixed" => Ok("already_fixed_current_head"),
        "feature_request" => Ok("feature_request"),
        "support_request" => Ok("support_request"),
        "security_sensitive" => Ok("security_sensitive"),
        "untrusted_instruction" => Ok("untrusted_instruction"),
        "inaccessible_environment" => Ok("inaccessible_environment"),
        "stale_approval" => Ok("stale_approval"),
        "budget_exhausted" => Ok("budget_exhausted"),
        _ => Err(anyhow!("unsupported sanitized issue classification")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_only_repository_issue_lists() {
        assert_eq!(
            canonicalize_issue_list_url(
                "https://github.com/owner/repo/issues/?query=yes#fragment",
                "owner/repo"
            )
            .unwrap(),
            "https://github.com/owner/repo/issues"
        );
        assert_eq!(
            canonicalize_issue_list_url(
                "https://github.com/owner/repo/issues#fragment-only",
                "owner/repo"
            )
            .unwrap(),
            "https://github.com/owner/repo/issues"
        );
        assert!(
            canonicalize_issue_list_url("http://github.com/owner/repo/issues", "owner/repo")
                .is_err()
        );
        assert!(
            canonicalize_issue_list_url("https://github.com/owner/repo/pulls", "owner/repo")
                .is_err()
        );
        assert!(canonicalize_issue_list_url(
            "https://github.com/owner/repo/issues//",
            "owner/repo"
        )
        .is_err());
    }
}
