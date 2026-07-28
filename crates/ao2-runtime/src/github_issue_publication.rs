use std::collections::BTreeSet;
use std::fmt;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use ao2_core::sha256_hex;
use chrono::{DateTime, Utc};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

const MAX_TITLE_BYTES: usize = 256;
const MAX_BODY_BYTES: usize = 8_192;
const MAX_COMMAND_OUTPUT_BYTES: usize = 1_048_576;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const ARCHITECTURE_CONTRACT_COMMIT: &str = "8e6f247b800b60c520b4e967f7553974a20ec2f8";
const ACTION_SCHEMA: &str = "ao.architecture.autonomous-issue-repair.github-action-digest.v1";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationPlan {
    schema_version: String,
    architecture_contract_commit: String,
    authority: AuthorityBundle,
    push_action: GitHubActionDigest,
    draft_action: GitHubActionDigest,
    draft: DraftText,
}

struct UniqueJSONValue(serde_json::Value);

impl<'de> Deserialize<'de> for UniqueJSONValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJSONVisitor)
    }
}

struct UniqueJSONVisitor;

impl<'de> Visitor<'de> for UniqueJSONVisitor {
    type Value = UniqueJSONValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object fields")
    }

    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJSONValue(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJSONValue(serde_json::Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJSONValue(serde_json::Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .map(UniqueJSONValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJSONValue(serde_json::Value::String(
            value.to_string(),
        )))
    }

    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJSONValue(serde_json::Value::String(value)))
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJSONValue(serde_json::Value::Null))
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJSONValue(serde_json::Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        UniqueJSONValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueJSONValue>()? {
            values.push(value.0);
        }
        Ok(UniqueJSONValue(serde_json::Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate field {key:?}")));
            }
            values.insert(key, object.next_value::<UniqueJSONValue>()?.0);
        }
        Ok(UniqueJSONValue(serde_json::Value::Object(values)))
    }
}

pub fn decode_publication_plan_strict(bytes: &[u8]) -> Result<PublicationPlan> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = UniqueJSONValue::deserialize(&mut deserializer)
        .context("publication plan is not strict duplicate-free JSON")?;
    deserializer
        .end()
        .context("publication plan contains trailing JSON")?;
    serde_json::from_value(value.0)
        .context("publication plan does not match the strict typed contract")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuthorityBundle {
    run_envelope: serde_json::Value,
    candidate_decision: serde_json::Value,
    governance_decision: serde_json::Value,
    reviewer_independence: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunEnvelopeContract {
    schema: String,
    run_id: String,
    #[serde(rename = "loop")]
    loop_: LoopContract,
    trigger: TriggerContract,
    discovery: DiscoveryContract,
    budgets: BudgetContract,
    governance: EnvelopeGovernanceContract,
    routing: RoutingContract,
    created_at: String,
    expires_at: String,
    canonical_digest: String,
    predecessor_digest: serde_json::Value,
    lineage: LineageContract,
    stop_conditions: Vec<String>,
    terminal_statuses: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoopContract {
    goal: String,
    trigger: String,
    discovery: String,
    action: String,
    verification: String,
    state: String,
    human_gates: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerContract {
    mode: String,
    canonical_url: String,
    repository: String,
    default_branch: String,
    pinned_base_commit: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoveryContract {
    snapshot_limit: u64,
    candidate_limit: u64,
    selected_limit: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetContract {
    wall_clock_seconds: u64,
    clone_count: u64,
    test_runs: u64,
    retry_count: u64,
    repair_count: u64,
    publication_count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeGovernanceContract {
    ownership_class: String,
    allowed_actions: Vec<String>,
    denied_actions: Vec<String>,
    sole_control_auto_merge_opt_in: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutingContract {
    default_branch: String,
    pinned_base_commit: String,
    fork_owner: serde_json::Value,
    repair_branch: String,
    protected_path_classes: Vec<String>,
    required_checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LineageContract {
    kind: String,
    predecessor_run_id: serde_json::Value,
    predecessor_digest: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateContract {
    schema: String,
    run_id: String,
    repository: String,
    base_sha: String,
    issue_number: u64,
    rank: u64,
    decision: String,
    eligibility: EligibilityContract,
    reason_codes: Vec<String>,
    evidence_digests: Vec<String>,
    expected_behavior_source: String,
    decided_at: String,
    decision_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EligibilityContract {
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GovernanceContract {
    schema: String,
    run_id: String,
    repository: String,
    base_sha: String,
    head_sha: String,
    governance_class: String,
    classification_sources: Vec<String>,
    push_target: String,
    pull_request_mode: String,
    merge: MergeContract,
    protected_path_touched: bool,
    required_checks: Vec<RequiredCheck>,
    action_digest_required: bool,
    decided_at: String,
    decision_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeContract {
    authorized: bool,
    mode: String,
    approval_kind: String,
    approval_head_sha: serde_json::Value,
    auto_merge_opt_in: bool,
    branch_protection_bypassed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewerContract {
    schema: String,
    run_id: String,
    subject_digest: String,
    reviewer_id: String,
    status: String,
    deterministic_tests_primary: bool,
    satisfies_team_merge_gate: bool,
    reviewed_at: String,
    review_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct GitHubActionDigest {
    schema: String,
    run_id: String,
    repository: String,
    issue_number: u64,
    base_sha: String,
    head_sha: String,
    fork: Option<String>,
    branch: String,
    pr_title_digest: String,
    pr_body_digest: String,
    diff_digest: String,
    required_checks: Vec<RequiredCheck>,
    action: String,
    approved_at: String,
    expires_at: String,
    run_envelope_digest: String,
    candidate_decision_digest: String,
    governance_decision_digest: String,
    reviewer_independence_digest: String,
    action_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RequiredCheck {
    name: String,
    conclusion: String,
    head_sha: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftText {
    title: String,
    body: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyReadback {
    schema_version: &'static str,
    status: &'static str,
    repository: String,
    fork: String,
    branch: String,
    head_sha: String,
    push_action_digest: String,
    draft_action_digest: String,
    github_contacted: bool,
    git_write_performed: bool,
    draft_pr_write_performed: bool,
    merge_performed: bool,
}

#[derive(Debug, Serialize)]
pub struct ApplyReadback {
    schema_version: &'static str,
    status: &'static str,
    repository: String,
    fork: String,
    branch: String,
    head_sha: String,
    pull_number: u64,
    fork_created: bool,
    branch_pushed: bool,
    draft_pr_created: bool,
    independent_readback: bool,
    issue_write_performed: bool,
    ready_for_review_performed: bool,
    review_approval_performed: bool,
    merge_performed: bool,
    release_performed: bool,
}

pub fn verify_publication_plan(
    plan: &PublicationPlan,
    expected_push: &str,
    expected_draft: &str,
    now: DateTime<Utc>,
) -> Result<VerifyReadback> {
    validate_plan(plan, expected_push, expected_draft, now)?;
    Ok(VerifyReadback {
        schema_version: "ao2.github-repair-publication-verification.v1",
        status: "passed",
        repository: plan.push_action.repository.clone(),
        fork: plan.push_action.fork.clone().unwrap_or_default(),
        branch: plan.push_action.branch.clone(),
        head_sha: plan.push_action.head_sha.clone(),
        push_action_digest: plan.push_action.action_digest.clone(),
        draft_action_digest: plan.draft_action.action_digest.clone(),
        github_contacted: false,
        git_write_performed: false,
        draft_pr_write_performed: false,
        merge_performed: false,
    })
}

pub fn apply_publication_plan(
    plan: &PublicationPlan,
    repository: &Path,
    expected_push: &str,
    expected_draft: &str,
    now: DateTime<Utc>,
) -> Result<ApplyReadback> {
    validate_plan(plan, expected_push, expected_draft, now)?;
    apply_with_transport(plan, repository, &mut SystemTransport)
}

fn validate_plan(
    plan: &PublicationPlan,
    expected_push: &str,
    expected_draft: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    if !is_sha256(expected_push) || !is_sha256(expected_draft) {
        bail!("expected action digests must be 64 lowercase hexadecimal characters");
    }
    if plan.schema_version != "ao2.github-repair-publication-plan.v1" {
        bail!("publication plan schema_version is invalid");
    }
    if plan.architecture_contract_commit != ARCHITECTURE_CONTRACT_COMMIT {
        bail!("publication plan Architecture contract commit is not the exact supported producer");
    }
    validate_authority(plan)?;
    validate_action(&plan.push_action, "push_operator_fork", expected_push, now)?;
    validate_action(
        &plan.draft_action,
        "open_upstream_draft_pr",
        expected_draft,
        now,
    )?;
    if plan.push_action.action_digest == plan.draft_action.action_digest {
        bail!("push and draft actions must have distinct exact digests");
    }
    let mut push_identity = plan.push_action.clone();
    let mut draft_identity = plan.draft_action.clone();
    push_identity.action.clear();
    push_identity.action_digest.clear();
    push_identity.approved_at.clear();
    push_identity.expires_at.clear();
    draft_identity.action.clear();
    draft_identity.action_digest.clear();
    draft_identity.approved_at.clear();
    draft_identity.expires_at.clear();
    if push_identity != draft_identity {
        bail!("push and draft actions do not bind the same publication identity");
    }
    validate_text("draft title", &plan.draft.title, MAX_TITLE_BYTES)?;
    validate_text("draft body", &plan.draft.body, MAX_BODY_BYTES)?;
    if sha256(plan.draft.title.as_bytes()) != plan.push_action.pr_title_digest
        || sha256(plan.draft.body.as_bytes()) != plan.push_action.pr_body_digest
    {
        bail!("draft title or body does not match the exact action digest binding");
    }
    Ok(())
}

fn validate_authority(plan: &PublicationPlan) -> Result<()> {
    let authority = &plan.authority;
    validate_authority_digest(
        &authority.run_envelope,
        "ao.architecture.autonomous-issue-repair.run-envelope.v1",
        "canonical_digest",
    )?;
    validate_authority_digest(
        &authority.candidate_decision,
        "ao.architecture.autonomous-issue-repair.candidate-decision.v1",
        "decision_digest",
    )?;
    validate_authority_digest(
        &authority.governance_decision,
        "ao.architecture.autonomous-issue-repair.governance-decision.v1",
        "decision_digest",
    )?;
    validate_authority_digest(
        &authority.reviewer_independence,
        "ao.architecture.autonomous-issue-repair.reviewer-independence.v1",
        "review_digest",
    )?;
    validate_full_authority_contracts(authority)?;
    let push = &plan.push_action;
    for action in [&plan.push_action, &plan.draft_action] {
        if json_string(&authority.run_envelope, &["canonical_digest"])?
            != action.run_envelope_digest
            || json_string(&authority.candidate_decision, &["decision_digest"])?
                != action.candidate_decision_digest
            || json_string(&authority.governance_decision, &["decision_digest"])?
                != action.governance_decision_digest
            || json_string(&authority.reviewer_independence, &["review_digest"])?
                != action.reviewer_independence_digest
        {
            bail!("GitHub action does not bind the exact authority document digests");
        }
    }
    let run_id = &push.run_id;
    if [
        json_string(&authority.run_envelope, &["run_id"])?,
        json_string(&authority.candidate_decision, &["run_id"])?,
        json_string(&authority.governance_decision, &["run_id"])?,
        json_string(&authority.reviewer_independence, &["run_id"])?,
    ]
    .iter()
    .any(|value| value != run_id)
    {
        bail!("publication run_id does not match every authority document");
    }
    if json_string(&authority.run_envelope, &["trigger", "repository"])? != push.repository
        || json_string(&authority.candidate_decision, &["repository"])? != push.repository
        || json_string(&authority.governance_decision, &["repository"])? != push.repository
        || json_u64(&authority.candidate_decision, &["issue_number"])? != push.issue_number
        || json_string(&authority.candidate_decision, &["decision"])? != "selected"
    {
        bail!("publication repository or selected candidate authority drifted");
    }
    for field in [
        "open_bug",
        "target_in_repository",
        "no_existing_fix",
        "current_head_unfixed",
        "public_reproduction_feasible",
        "deterministic_local_reproduction",
        "expected_behavior_grounded",
        "bounded_policy_compatible",
    ] {
        if !json_bool(&authority.candidate_decision, &["eligibility", field])? {
            bail!("selected candidate has a failing publication eligibility predicate");
        }
    }
    if json_bool(
        &authority.candidate_decision,
        &["eligibility", "security_sensitive"],
    )? {
        bail!("security-sensitive candidate cannot use this publication path");
    }
    for path in [
        &["trigger", "pinned_base_commit"][..],
        &["routing", "pinned_base_commit"][..],
    ] {
        if json_string(&authority.run_envelope, path)? != push.base_sha {
            bail!("publication base SHA does not match the run envelope");
        }
    }
    if json_string(&authority.candidate_decision, &["base_sha"])? != push.base_sha
        || json_string(&authority.governance_decision, &["base_sha"])? != push.base_sha
        || json_string(&authority.governance_decision, &["head_sha"])? != push.head_sha
    {
        bail!("publication commit authority drifted");
    }
    if json_string(&authority.run_envelope, &["trigger", "default_branch"])?
        != json_string(&authority.run_envelope, &["routing", "default_branch"])?
    {
        bail!("run envelope default branch fields disagree");
    }
    let fork_owner = json_string(&authority.run_envelope, &["routing", "fork_owner"])?;
    let (_, repository_name) = split_repository(&push.repository)?;
    let expected_fork = format!("{fork_owner}/{repository_name}");
    if push.fork.as_deref() != Some(expected_fork.as_str())
        || json_string(&authority.run_envelope, &["routing", "repair_branch"])? != push.branch
    {
        bail!("publication fork or branch does not match the run envelope");
    }
    let allowed = json_strings(&authority.run_envelope, &["governance", "allowed_actions"])?;
    let denied = json_strings(&authority.run_envelope, &["governance", "denied_actions"])?;
    let required_denials = [
        "push_upstream",
        "open_ready_pr",
        "mark_ready",
        "approve_review",
        "merge",
        "mutate_issue",
        "publish_release",
    ];
    if !allowed.contains("push_operator_fork")
        || !allowed.contains("open_upstream_draft_pr")
        || required_denials
            .iter()
            .any(|value| !denied.contains(*value))
        || json_bool(
            &authority.run_envelope,
            &["governance", "sole_control_auto_merge_opt_in"],
        )?
    {
        bail!("run envelope does not preserve fork-only draft publication boundaries");
    }
    if json_string(&authority.run_envelope, &["governance", "ownership_class"])?
        != json_string(&authority.governance_decision, &["governance_class"])?
    {
        bail!("governance class does not match the run envelope");
    }
    if json_string(&authority.governance_decision, &["push_target"])? != "operator_owned_fork"
        || json_string(&authority.governance_decision, &["pull_request_mode"])?
            != "upstream_draft_only"
        || json_bool(&authority.governance_decision, &["merge", "authorized"])?
        || json_string(&authority.governance_decision, &["merge", "mode"])? != "never"
        || json_bool(
            &authority.governance_decision,
            &["merge", "branch_protection_bypassed"],
        )?
        || !json_bool(&authority.governance_decision, &["action_digest_required"])?
    {
        bail!("governance authority does not require fork-only draft publication");
    }
    let expected_checks = json_strings(&authority.run_envelope, &["routing", "required_checks"])?;
    let governance_check_count = json_at(&authority.governance_decision, &["required_checks"])?
        .as_array()
        .context("authority checks must be an array")?
        .len();
    let governance_checks = json_checks(&authority.governance_decision, &["required_checks"])?;
    let action_checks: BTreeSet<_> = push
        .required_checks
        .iter()
        .map(|check| check.name.as_str())
        .collect();
    if expected_checks != action_checks
        || governance_checks != action_checks
        || governance_check_count != action_checks.len()
        || plan.draft_action.required_checks != push.required_checks
    {
        bail!("authority documents do not bind the exact successful checks");
    }
    if json_string(&authority.reviewer_independence, &["subject_digest"])? != push.diff_digest
        || json_string(&authority.reviewer_independence, &["status"])? != "independent"
        || !json_bool(
            &authority.reviewer_independence,
            &["deterministic_tests_primary"],
        )?
    {
        bail!("independent review authority does not bind the exact diff");
    }
    let envelope_created = parse_time(
        "run envelope created_at",
        json_string(&authority.run_envelope, &["created_at"])?,
    )?;
    let evidence_times = [
        (&authority.candidate_decision, "decided_at"),
        (&authority.governance_decision, "decided_at"),
        (&authority.reviewer_independence, "reviewed_at"),
    ]
    .map(|(document, field)| parse_time(field, json_string(document, &[field])?))
    .into_iter()
    .collect::<Result<Vec<_>>>()?;
    if evidence_times.iter().any(|time| *time < envelope_created) {
        bail!("authority decision predates run envelope creation");
    }
    for action in [&plan.push_action, &plan.draft_action] {
        let approved = parse_time("approved_at", &action.approved_at)?;
        if approved < envelope_created || evidence_times.iter().any(|time| *time > approved) {
            bail!("publication approval predates its run envelope or an authority decision");
        }
    }
    let envelope_expiry = parse_time(
        "run envelope expires_at",
        json_string(&authority.run_envelope, &["expires_at"])?,
    )?;
    if parse_time("action expires_at", &push.expires_at)? > envelope_expiry
        || parse_time("draft expires_at", &plan.draft_action.expires_at)? > envelope_expiry
    {
        bail!("publication action outlives its run envelope");
    }
    Ok(())
}

fn validate_authority_digest(
    value: &serde_json::Value,
    schema: &str,
    digest_field: &str,
) -> Result<()> {
    if json_string(value, &["schema"])? != schema {
        bail!("authority document schema is invalid");
    }
    let expected = json_string(value, &[digest_field])?;
    let mut subject = value.clone();
    subject
        .as_object_mut()
        .context("authority document must be a JSON object")?
        .remove(digest_field);
    if !is_sha256(expected) || canonical_json_sha256(&subject) != expected {
        bail!("authority document canonical digest is invalid");
    }
    Ok(())
}

fn validate_full_authority_contracts(authority: &AuthorityBundle) -> Result<()> {
    let envelope: RunEnvelopeContract = serde_json::from_value(authority.run_envelope.clone())
        .context("run envelope does not match the strict pinned Architecture schema")?;
    let candidate: CandidateContract = serde_json::from_value(authority.candidate_decision.clone())
        .context("candidate decision does not match the strict pinned Architecture schema")?;
    let governance: GovernanceContract =
        serde_json::from_value(authority.governance_decision.clone())
            .context("governance decision does not match the strict pinned Architecture schema")?;
    let reviewer: ReviewerContract =
        serde_json::from_value(authority.reviewer_independence.clone())
            .context("reviewer decision does not match the strict pinned Architecture schema")?;
    validate_envelope_contract(&envelope)?;
    validate_candidate_contract(&candidate)?;
    validate_governance_contract(&governance)?;
    validate_reviewer_contract(&reviewer)
}

fn validate_envelope_contract(value: &RunEnvelopeContract) -> Result<()> {
    if value.schema != "ao.architecture.autonomous-issue-repair.run-envelope.v1" {
        bail!("run envelope schema is invalid");
    }
    validate_run_id("run_id", &value.run_id)?;
    for (name, text, max) in [
        ("loop.goal", value.loop_.goal.as_str(), 512),
        ("loop.trigger", value.loop_.trigger.as_str(), 256),
        ("loop.discovery", value.loop_.discovery.as_str(), 256),
        ("loop.action", value.loop_.action.as_str(), 256),
        ("loop.verification", value.loop_.verification.as_str(), 256),
        ("loop.state", value.loop_.state.as_str(), 256),
        ("loop.human_gates", value.loop_.human_gates.as_str(), 256),
    ] {
        validate_text(name, text, max)?;
    }
    validate_one_of(
        "trigger mode",
        &value.trigger.mode,
        &["explicit_issue", "issue_list"],
    )?;
    split_repository(&value.trigger.repository)?;
    validate_ref(&value.trigger.default_branch)?;
    if !is_commit(&value.trigger.pinned_base_commit) {
        bail!("run envelope pinned base commit is invalid");
    }
    let issue_root = format!("https://github.com/{}/issues", value.trigger.repository);
    let valid_url = match value.trigger.mode.as_str() {
        "issue_list" => value.trigger.canonical_url == issue_root,
        "explicit_issue" => value
            .trigger
            .canonical_url
            .strip_prefix(&(issue_root + "/"))
            .is_some_and(|number| {
                number
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| matches!(byte, b'1'..=b'9'))
                    && number.as_bytes().iter().skip(1).all(u8::is_ascii_digit)
            }),
        _ => false,
    };
    if !valid_url {
        bail!("run envelope canonical GitHub URL is invalid");
    }
    if !(1..=50).contains(&value.discovery.snapshot_limit)
        || !(1..=10).contains(&value.discovery.candidate_limit)
        || value.discovery.selected_limit != 1
    {
        bail!("run envelope discovery bounds are invalid");
    }
    if value.budgets.wall_clock_seconds == 0
        || value.budgets.test_runs == 0
        || value.budgets.repair_count > 1
        || value.budgets.publication_count > 1
    {
        bail!("run envelope budgets are invalid");
    }
    let _ = (
        value.budgets.clone_count,
        value.budgets.retry_count,
        value.governance.sole_control_auto_merge_opt_in,
    );
    validate_one_of(
        "ownership class",
        &value.governance.ownership_class,
        &["sole_control", "team", "external", "unknown"],
    )?;
    validate_string_array(
        "allowed actions",
        &value.governance.allowed_actions,
        1,
        128,
        &[
            "read_public_metadata",
            "clone_public_repository",
            "push_operator_fork",
            "open_upstream_draft_pr",
            "open_ready_pr",
            "request_merge_queue",
            "auto_merge",
        ],
    )?;
    validate_string_array(
        "denied actions",
        &value.governance.denied_actions,
        1,
        128,
        &[
            "push_upstream",
            "open_ready_pr",
            "mark_ready",
            "approve_review",
            "merge",
            "mutate_issue",
            "publish_release",
        ],
    )?;
    validate_ref(&value.routing.default_branch)?;
    validate_schema_branch(&value.routing.repair_branch)?;
    validate_ref(&value.routing.repair_branch)?;
    if !is_commit(&value.routing.pinned_base_commit) {
        bail!("run envelope routing commit is invalid");
    }
    if !value.routing.fork_owner.is_null() {
        let owner = value
            .routing
            .fork_owner
            .as_str()
            .context("run envelope fork_owner must be string or null")?;
        if !valid_slug(owner) {
            bail!("run envelope fork_owner is invalid");
        }
    }
    validate_string_array(
        "protected path classes",
        &value.routing.protected_path_classes,
        0,
        128,
        &[],
    )?;
    validate_string_array(
        "required checks",
        &value.routing.required_checks,
        0,
        256,
        &[],
    )?;
    let created = parse_time("run envelope created_at", &value.created_at)?;
    let expires = parse_time("run envelope expires_at", &value.expires_at)?;
    let lifetime_seconds = expires.signed_duration_since(created).num_seconds();
    if expires <= created
        || lifetime_seconds < 0
        || u64::try_from(lifetime_seconds)? > value.budgets.wall_clock_seconds
        || !is_sha256(&value.canonical_digest)
    {
        bail!("run envelope timestamps or canonical digest are invalid");
    }
    let external_or_unknown = matches!(
        value.governance.ownership_class.as_str(),
        "external" | "unknown"
    );
    if external_or_unknown
        && value.governance.allowed_actions.iter().any(|action| {
            matches!(
                action.as_str(),
                "open_ready_pr" | "request_merge_queue" | "auto_merge"
            )
        })
    {
        bail!("external or unknown run envelope must remain draft-only");
    }
    if value.governance.sole_control_auto_merge_opt_in
        && value.governance.ownership_class != "sole_control"
    {
        bail!("auto-merge opt-in is only valid for sole_control governance");
    }
    if value
        .governance
        .allowed_actions
        .iter()
        .any(|action| action == "auto_merge")
        && (value.governance.ownership_class != "sole_control"
            || !value.governance.sole_control_auto_merge_opt_in)
    {
        bail!("auto_merge requires sole-control explicit opt-in");
    }
    validate_lineage(value)?;
    validate_string_array(
        "stop conditions",
        &value.stop_conditions,
        1,
        128,
        &[
            "policy_ambiguity",
            "security_sensitive",
            "credential_required",
            "lease_conflict",
            "digest_mismatch",
            "required_check_conflict",
            "budget_exhausted",
            "unauthorized_external_write",
            "concurrent_repository_mutation",
        ],
    )?;
    validate_string_array(
        "terminal statuses",
        &value.terminal_statuses,
        1,
        128,
        &[
            "completed",
            "no_eligible_issue",
            "operator_action_required",
            "blocked",
            "expired",
            "cancelled",
        ],
    )
}

fn validate_lineage(value: &RunEnvelopeContract) -> Result<()> {
    match value.lineage.kind.as_str() {
        "origin"
            if value.predecessor_digest.is_null()
                && value.lineage.predecessor_run_id.is_null()
                && value.lineage.predecessor_digest.is_null() =>
        {
            Ok(())
        }
        "narrower_successor" => {
            let top = value
                .predecessor_digest
                .as_str()
                .context("successor predecessor_digest must be a string")?;
            let run_id = value
                .lineage
                .predecessor_run_id
                .as_str()
                .context("successor predecessor_run_id must be a string")?;
            let lineage = value
                .lineage
                .predecessor_digest
                .as_str()
                .context("successor lineage digest must be a string")?;
            validate_run_id("predecessor_run_id", run_id)?;
            if !is_sha256(top) || top != lineage {
                bail!("successor lineage digest is invalid");
            }
            Ok(())
        }
        _ => bail!("run envelope lineage is invalid"),
    }
}

fn validate_candidate_contract(value: &CandidateContract) -> Result<()> {
    if value.schema != "ao.architecture.autonomous-issue-repair.candidate-decision.v1" {
        bail!("candidate schema is invalid");
    }
    validate_run_id("candidate run_id", &value.run_id)?;
    split_repository(&value.repository)?;
    if !is_commit(&value.base_sha)
        || value.issue_number == 0
        || !(1..=10).contains(&value.rank)
        || !is_sha256(&value.decision_digest)
    {
        bail!("candidate identity or digest is invalid");
    }
    validate_one_of(
        "candidate decision",
        &value.decision,
        &["eligible", "selected", "excluded"],
    )?;
    let eligibility = &value.eligibility;
    validate_string_array("candidate reason codes", &value.reason_codes, 1, 128, &[])?;
    validate_string_array(
        "candidate evidence digests",
        &value.evidence_digests,
        1,
        64,
        &[],
    )?;
    if value
        .evidence_digests
        .iter()
        .any(|digest| !is_sha256(digest))
    {
        bail!("candidate evidence digest is invalid");
    }
    validate_one_of(
        "expected behavior source",
        &value.expected_behavior_source,
        &[
            "tests",
            "documentation",
            "protocol",
            "maintainer_statement",
            "unavailable",
        ],
    )?;
    let positive_predicates = [
        eligibility.open_bug,
        eligibility.target_in_repository,
        eligibility.no_existing_fix,
        eligibility.current_head_unfixed,
        eligibility.public_reproduction_feasible,
        eligibility.deterministic_local_reproduction,
        eligibility.expected_behavior_grounded,
        eligibility.bounded_policy_compatible,
    ];
    let all_eligible =
        positive_predicates.iter().all(|value| *value) && !eligibility.security_sensitive;
    let grounded_source = value.expected_behavior_source != "unavailable";
    if matches!(value.decision.as_str(), "eligible" | "selected")
        && (!all_eligible || !grounded_source)
    {
        bail!("eligible or selected candidate requires every predicate and grounded behavior");
    }
    if value.decision == "excluded" && all_eligible && grounded_source {
        bail!("excluded candidate requires a failing predicate");
    }
    parse_time("candidate decided_at", &value.decided_at)?;
    Ok(())
}

fn validate_governance_contract(value: &GovernanceContract) -> Result<()> {
    if value.schema != "ao.architecture.autonomous-issue-repair.governance-decision.v1" {
        bail!("governance schema is invalid");
    }
    validate_run_id("governance run_id", &value.run_id)?;
    split_repository(&value.repository)?;
    if !is_commit(&value.base_sha)
        || !is_commit(&value.head_sha)
        || !is_sha256(&value.decision_digest)
    {
        bail!("governance commit or digest is invalid");
    }
    validate_one_of(
        "governance class",
        &value.governance_class,
        &["sole_control", "team", "external", "unknown"],
    )?;
    validate_string_array(
        "classification sources",
        &value.classification_sources,
        1,
        128,
        &[
            "repository_policy",
            "branch_rules",
            "codeowners",
            "operator_envelope",
            "unknown_default",
        ],
    )?;
    validate_one_of(
        "push target",
        &value.push_target,
        &[
            "authorized_operator_repository",
            "policy_authorized_branch",
            "operator_owned_fork",
        ],
    )?;
    validate_one_of(
        "pull request mode",
        &value.pull_request_mode,
        &["draft_or_ready_by_policy", "upstream_draft_only"],
    )?;
    validate_one_of(
        "merge mode",
        &value.merge.mode,
        &["never", "manual", "merge_queue", "auto_merge"],
    )?;
    validate_one_of(
        "merge approval kind",
        &value.merge.approval_kind,
        &["none", "independent_human", "codeowner"],
    )?;
    if !value.merge.approval_head_sha.is_null()
        && !value
            .merge
            .approval_head_sha
            .as_str()
            .is_some_and(is_commit)
    {
        bail!("merge approval head SHA must be a commit or null");
    }
    if value.merge.branch_protection_bypassed || !value.action_digest_required {
        bail!("governance safety constants are invalid");
    }
    if !value.merge.authorized && value.merge.mode != "never" {
        bail!("unauthorized governance merge mode must be never");
    }
    if value.merge.authorized {
        let checks_pass = !value.required_checks.is_empty()
            && value
                .required_checks
                .iter()
                .all(|check| check.conclusion == "success" && check.head_sha == value.head_sha);
        if value.protected_path_touched || !checks_pass {
            bail!("authorized merge requires unprotected paths and exact successful checks");
        }
    }
    if value.governance_class == "sole_control"
        && value.merge.mode == "auto_merge"
        && !value.merge.auto_merge_opt_in
    {
        bail!("sole-control auto_merge requires explicit opt-in");
    }
    if matches!(value.governance_class.as_str(), "external" | "unknown")
        && (value.merge.authorized
            || value.merge.mode != "never"
            || value.push_target != "operator_owned_fork"
            || value.pull_request_mode != "upstream_draft_only"
            || value.merge.approval_kind != "none"
            || !value.merge.approval_head_sha.is_null()
            || value.merge.auto_merge_opt_in)
    {
        bail!("external or unknown governance must remain fork-only, draft-only, and unmerged");
    }
    if value.governance_class == "team"
        && value.merge.authorized
        && ((!matches!(
            value.merge.approval_kind.as_str(),
            "independent_human" | "codeowner"
        ) || value.merge.approval_head_sha.as_str() != Some(value.head_sha.as_str()))
            || value.merge.mode != "merge_queue")
    {
        bail!("team merge requires exact-head independent approval through the merge queue");
    }
    let mut check_names = BTreeSet::new();
    for check in &value.required_checks {
        validate_text("governance check name", &check.name, 256)?;
        validate_one_of(
            "governance check conclusion",
            &check.conclusion,
            &["success", "failure", "pending", "missing"],
        )?;
        if !is_commit(&check.head_sha)
            || check.head_sha != value.head_sha
            || !check_names.insert(check.name.as_str())
        {
            bail!("governance checks must be unique and bind the exact head SHA");
        }
    }
    parse_time("governance decided_at", &value.decided_at)?;
    Ok(())
}

fn validate_reviewer_contract(value: &ReviewerContract) -> Result<()> {
    if value.schema != "ao.architecture.autonomous-issue-repair.reviewer-independence.v1" {
        bail!("reviewer schema is invalid");
    }
    validate_run_id("reviewer run_id", &value.run_id)?;
    validate_text("reviewer_id", &value.reviewer_id, 128)?;
    validate_one_of(
        "reviewer status",
        &value.status,
        &["independent", "same_vendor", "unavailable", "unverified"],
    )?;
    if !is_sha256(&value.subject_digest)
        || !is_sha256(&value.review_digest)
        || !value.deterministic_tests_primary
    {
        bail!("reviewer digest or deterministic-test constant is invalid");
    }
    let _ = value.satisfies_team_merge_gate;
    parse_time("reviewed_at", &value.reviewed_at)?;
    Ok(())
}

fn validate_one_of(name: &str, value: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        bail!("{name} is outside the pinned Architecture enum")
    }
}

fn validate_run_id(name: &str, value: &str) -> Result<()> {
    let mut bytes = value.bytes();
    if !(8..=128).contains(&value.len())
        || !bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("{name} does not match the pinned Architecture identifier pattern");
    }
    Ok(())
}

fn validate_string_array(
    name: &str,
    values: &[String],
    min_items: usize,
    max_length: usize,
    allowed: &[&str],
) -> Result<()> {
    if values.len() < min_items {
        bail!("{name} has too few entries");
    }
    let mut unique = BTreeSet::new();
    for value in values {
        validate_text(name, value, max_length)?;
        if !unique.insert(value.as_str())
            || (!allowed.is_empty() && !allowed.contains(&value.as_str()))
        {
            bail!("{name} contains a duplicate or unsupported entry");
        }
    }
    Ok(())
}

fn json_at<'a>(value: &'a serde_json::Value, path: &[&str]) -> Result<&'a serde_json::Value> {
    path.iter().try_fold(value, |current, key| {
        current
            .get(*key)
            .context("authority document field is missing")
    })
}

fn json_string<'a>(value: &'a serde_json::Value, path: &[&str]) -> Result<&'a str> {
    json_at(value, path)?
        .as_str()
        .context("authority document field must be a string")
}

fn json_bool(value: &serde_json::Value, path: &[&str]) -> Result<bool> {
    json_at(value, path)?
        .as_bool()
        .context("authority document field must be a boolean")
}

fn json_u64(value: &serde_json::Value, path: &[&str]) -> Result<u64> {
    json_at(value, path)?
        .as_u64()
        .context("authority document field must be an unsigned integer")
}

fn json_strings<'a>(value: &'a serde_json::Value, path: &[&str]) -> Result<BTreeSet<&'a str>> {
    json_at(value, path)?
        .as_array()
        .context("authority document field must be an array")?
        .iter()
        .map(|item| {
            item.as_str()
                .context("authority array entry must be a string")
        })
        .collect()
}

fn json_checks<'a>(value: &'a serde_json::Value, path: &[&str]) -> Result<BTreeSet<&'a str>> {
    json_at(value, path)?
        .as_array()
        .context("authority checks must be an array")?
        .iter()
        .map(|check| {
            if json_string(check, &["conclusion"])? != "success"
                || json_string(check, &["head_sha"])? != json_string(value, &["head_sha"])?
            {
                bail!("authority check is failed or bound to another head");
            }
            json_string(check, &["name"])
        })
        .collect()
}

fn parse_time(name: &str, value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("{name} is not RFC3339"))
        .map(|time| time.with_timezone(&Utc))
}

fn validate_action(
    action: &GitHubActionDigest,
    expected_action: &str,
    expected_digest: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    if action.schema != ACTION_SCHEMA || action.action != expected_action {
        bail!("GitHub action schema or operation is invalid");
    }
    validate_identifier("run_id", &action.run_id)?;
    split_repository(&action.repository)?;
    let fork = action
        .fork
        .as_deref()
        .context("operator-owned fork is required")?;
    let (fork_owner, fork_repo) = split_repository(fork)?;
    let (_, upstream_repo) = split_repository(&action.repository)?;
    if fork_owner.is_empty() || fork_repo != upstream_repo {
        bail!("operator-owned fork must preserve the upstream repository name");
    }
    if action.issue_number == 0
        || !is_commit(&action.base_sha)
        || !is_commit(&action.head_sha)
        || action.base_sha == action.head_sha
    {
        bail!("GitHub action issue or commit identity is invalid");
    }
    validate_schema_branch(&action.branch)?;
    validate_ref(&action.branch)?;
    for value in [
        &action.pr_title_digest,
        &action.pr_body_digest,
        &action.diff_digest,
        &action.run_envelope_digest,
        &action.candidate_decision_digest,
        &action.governance_decision_digest,
        &action.reviewer_independence_digest,
        &action.action_digest,
    ] {
        if !is_sha256(value) {
            bail!("GitHub action contains an invalid SHA-256 digest");
        }
    }
    if action.required_checks.is_empty() {
        bail!("GitHub action requires nonempty successful exact-head checks");
    }
    let mut names = BTreeSet::new();
    for check in &action.required_checks {
        validate_text("required check name", &check.name, 256)?;
        if check.conclusion != "success"
            || check.head_sha != action.head_sha
            || !names.insert(check.name.as_str())
        {
            bail!("GitHub action checks must be unique, successful, and bound to the exact head");
        }
    }
    let approved = DateTime::parse_from_rfc3339(&action.approved_at)
        .context("GitHub action approved_at is not RFC3339")?
        .with_timezone(&Utc);
    let expires = DateTime::parse_from_rfc3339(&action.expires_at)
        .context("GitHub action expires_at is not RFC3339")?
        .with_timezone(&Utc);
    if approved > now
        || expires <= now
        || expires <= approved
        || expires.signed_duration_since(approved).num_seconds() > 24 * 60 * 60
    {
        bail!("GitHub action approval is future-dated, stale, reversed, or longer than 24 hours");
    }
    let mut value = serde_json::to_value(action)?;
    value
        .as_object_mut()
        .context("GitHub action must be an object")?
        .remove("action_digest");
    let actual = canonical_json_sha256(&value);
    if actual != action.action_digest || actual != expected_digest {
        bail!("GitHub action digest does not match the canonical action or expected approval");
    }
    Ok(())
}

trait PublicationTransport {
    fn now(&mut self) -> DateTime<Utc>;
    fn verify_local_repository(&mut self, root: &Path, plan: &PublicationPlan) -> Result<()>;
    fn authenticated_login(&mut self) -> Result<String>;
    fn read_repository(&mut self, repository: &str) -> Result<RepositoryIdentity>;
    fn read_fork(&mut self, fork: &str) -> Result<Option<ForkIdentity>>;
    fn create_fork(&mut self, upstream: &str) -> Result<()>;
    fn read_branch(&mut self, fork: &str, branch: &str) -> Result<Option<String>>;
    fn push_branch(&mut self, root: &Path, fork: &str, branch: &str, head: &str) -> Result<()>;
    fn find_drafts(
        &mut self,
        action: &GitHubActionDigest,
        base_branch: &str,
    ) -> Result<Vec<PullIdentity>>;
    fn create_draft(
        &mut self,
        plan: &PublicationPlan,
        base_branch: &str,
    ) -> Result<CreateDraftOutcome>;
    fn read_pull(&mut self, repository: &str, number: u64) -> Result<PullIdentity>;
    fn retry_delay(&mut self) {
        thread::sleep(Duration::from_secs(1));
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RepositoryIdentity {
    full_name: String,
    default_branch: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ForkIdentity {
    full_name: String,
    default_branch: String,
    owner: OwnerIdentity,
    parent: Option<ParentIdentity>,
}

#[derive(Debug, Clone, Deserialize)]
struct OwnerIdentity {
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ParentIdentity {
    full_name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PullIdentity {
    number: u64,
    state: String,
    draft: bool,
    merged_at: Option<String>,
    title: String,
    body: String,
    base: PullRef,
    head: PullHead,
}

#[derive(Debug, Clone, Deserialize)]
struct PullRef {
    sha: String,
    #[serde(rename = "ref")]
    reference: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PullHead {
    sha: String,
    #[serde(rename = "ref")]
    reference: String,
    repo: PullRepository,
}

#[derive(Debug, Clone, Deserialize)]
struct PullRepository {
    full_name: String,
}

enum CreateDraftOutcome {
    Created(u64),
    Conflict,
}

fn apply_with_transport<T: PublicationTransport>(
    plan: &PublicationPlan,
    repository: &Path,
    transport: &mut T,
) -> Result<ApplyReadback> {
    let push = &plan.push_action;
    let fork = push
        .fork
        .as_deref()
        .context("operator-owned fork is required")?;
    transport.verify_local_repository(repository, plan)?;
    let login = transport.authenticated_login()?;
    let (fork_owner, _) = split_repository(fork)?;
    if login != fork_owner {
        bail!("approved fork owner does not match the authenticated GitHub account");
    }
    let upstream = transport.read_repository(&push.repository)?;
    if upstream.full_name != push.repository {
        bail!("upstream repository identity drifted");
    }
    validate_ref(&upstream.default_branch)?;
    if transport
        .read_branch(&push.repository, &upstream.default_branch)?
        .as_deref()
        != Some(&push.base_sha)
    {
        bail!("upstream default branch does not match the exact approved base");
    }
    let mut fork_created = false;
    match transport.read_fork(fork)? {
        Some(identity) => {
            validate_fork_identity(&identity, fork, &push.repository, &upstream.default_branch)?
        }
        None => {
            validate_action(
                &plan.push_action,
                "push_operator_fork",
                &plan.push_action.action_digest,
                transport.now(),
            )?;
            transport.create_fork(&push.repository)?;
            fork_created = true;
            let mut identity = None;
            for _ in 0..10 {
                identity = transport.read_fork(fork)?;
                if identity.is_some() {
                    break;
                }
                transport.retry_delay();
            }
            validate_fork_identity(
                &identity.context("created fork was not independently readable")?,
                fork,
                &push.repository,
                &upstream.default_branch,
            )?;
        }
    }
    let mut branch_pushed = false;
    match transport.read_branch(fork, &push.branch)? {
        Some(head) if head == push.head_sha => {}
        Some(_) => bail!("existing repair branch does not match the exact approved head"),
        None => {
            validate_action(
                &plan.push_action,
                "push_operator_fork",
                &plan.push_action.action_digest,
                transport.now(),
            )?;
            transport.push_branch(repository, fork, &push.branch, &push.head_sha)?;
            branch_pushed = true;
            let mut readback = None;
            for _ in 0..5 {
                readback = transport.read_branch(fork, &push.branch)?;
                if readback.as_deref() == Some(&push.head_sha) {
                    break;
                }
                transport.retry_delay();
            }
            if readback.as_deref() != Some(&push.head_sha) {
                bail!("pushed repair branch failed exact independent readback");
            }
        }
    }
    let existing = transport.find_drafts(&plan.draft_action, &upstream.default_branch)?;
    let (pull_number, draft_pr_created) = match existing.as_slice() {
        [] => match {
            validate_action(
                &plan.draft_action,
                "open_upstream_draft_pr",
                &plan.draft_action.action_digest,
                transport.now(),
            )?;
            transport.create_draft(plan, &upstream.default_branch)
        }? {
            CreateDraftOutcome::Created(number) => (number, true),
            CreateDraftOutcome::Conflict => {
                let reread = transport.find_drafts(&plan.draft_action, &upstream.default_branch)?;
                match reread.as_slice() {
                    [pull] if exact_pull(pull, plan, &upstream.default_branch) => {
                        (pull.number, false)
                    }
                    [] => bail!("draft create conflict re-read found no matching draft"),
                    [_] => bail!("draft create conflict re-read found identity drift"),
                    _ => bail!("draft create conflict re-read was ambiguous"),
                }
            }
        },
        [pull] if exact_pull(pull, plan, &upstream.default_branch) => (pull.number, false),
        [_] => bail!("existing pull request does not match the exact approved draft identity"),
        _ => bail!("draft pull request lookup returned ambiguous matches"),
    };
    let pull = transport.read_pull(&push.repository, pull_number)?;
    if !exact_pull(&pull, plan, &upstream.default_branch) {
        bail!("draft pull request failed exact independent readback");
    }
    Ok(ApplyReadback {
        schema_version: "ao2.github-repair-publication-readback.v1",
        status: if fork_created || branch_pushed || draft_pr_created {
            "applied"
        } else {
            "idempotent_readback"
        },
        repository: push.repository.clone(),
        fork: fork.to_string(),
        branch: push.branch.clone(),
        head_sha: push.head_sha.clone(),
        pull_number,
        fork_created,
        branch_pushed,
        draft_pr_created,
        independent_readback: true,
        issue_write_performed: false,
        ready_for_review_performed: false,
        review_approval_performed: false,
        merge_performed: false,
        release_performed: false,
    })
}

fn validate_fork_identity(
    identity: &ForkIdentity,
    fork: &str,
    upstream: &str,
    upstream_default_branch: &str,
) -> Result<()> {
    let (owner, _) = split_repository(fork)?;
    if identity.full_name != fork
        || identity.owner.login != owner
        || identity
            .parent
            .as_ref()
            .map(|value| value.full_name.as_str())
            != Some(upstream)
        || identity.default_branch != upstream_default_branch
    {
        bail!("fork identity, parent, owner, or default branch drifted");
    }
    Ok(())
}

fn exact_pull(pull: &PullIdentity, plan: &PublicationPlan, base_branch: &str) -> bool {
    let action = &plan.draft_action;
    let fork = action.fork.as_deref().unwrap_or_default();
    pull.number > 0
        && pull.state == "open"
        && pull.draft
        && pull.merged_at.is_none()
        && pull.title == plan.draft.title
        && pull.body == plan.draft.body
        && pull.base.reference == base_branch
        && pull.base.sha == action.base_sha
        && pull.head.reference == action.branch
        && pull.head.sha == action.head_sha
        && pull.head.repo.full_name == fork
}

struct SystemTransport;

impl PublicationTransport for SystemTransport {
    fn now(&mut self) -> DateTime<Utc> {
        Utc::now()
    }

    fn verify_local_repository(&mut self, root: &Path, plan: &PublicationPlan) -> Result<()> {
        let action = &plan.push_action;
        if run_output("git", &["rev-parse", "HEAD"], root, None)? != action.head_sha {
            bail!("local repository HEAD does not match the exact approved head");
        }
        if !run_output("git", &["status", "--porcelain"], root, None)?.is_empty() {
            bail!("local repository must be clean before publication");
        }
        run_status(
            "git",
            &[
                "merge-base",
                "--is-ancestor",
                &action.base_sha,
                &action.head_sha,
            ],
            root,
            None,
        )
        .context("approved base is not an ancestor of the approved head")?;
        let diff = read_governed_diff(root, &action.base_sha, &action.head_sha)?;
        if sha256(&diff) != action.diff_digest {
            bail!("local repository diff does not match the approved diff digest");
        }
        let changed_paths = read_governed_changed_paths(root, &action.base_sha, &action.head_sha)?;
        let protected_patterns = json_strings(
            &plan.authority.run_envelope,
            &["routing", "protected_path_classes"],
        )?
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let derived = derive_protected_path_touched(&changed_paths, &protected_patterns)?;
        let claimed = json_bool(
            &plan.authority.governance_decision,
            &["protected_path_touched"],
        )?;
        if derived != claimed {
            bail!("governance protected-path claim does not match the approved diff");
        }
        if derived {
            bail!("approved diff touches a protected path");
        }
        Ok(())
    }

    fn authenticated_login(&mut self) -> Result<String> {
        let value: serde_json::Value = gh_json(&["api", "--hostname", "github.com", "user"])?;
        value["login"]
            .as_str()
            .map(str::to_string)
            .context("GitHub user readback omitted login")
    }

    fn read_repository(&mut self, repository: &str) -> Result<RepositoryIdentity> {
        gh_json(&[
            "api",
            "--hostname",
            "github.com",
            &format!("repos/{repository}"),
        ])
    }

    fn read_fork(&mut self, fork: &str) -> Result<Option<ForkIdentity>> {
        gh_optional(&["api", "--hostname", "github.com", &format!("repos/{fork}")])
    }

    fn create_fork(&mut self, upstream: &str) -> Result<()> {
        let body = br#"{"default_branch_only":true}"#;
        run_status(
            "gh",
            &[
                "api",
                "--hostname",
                "github.com",
                "--method",
                "POST",
                &format!("repos/{upstream}/forks"),
                "--input",
                "-",
            ],
            Path::new("."),
            Some(body),
        )
    }

    fn read_branch(&mut self, fork: &str, branch: &str) -> Result<Option<String>> {
        let encoded = percent_encode(branch);
        let value: Option<serde_json::Value> = gh_optional(&[
            "api",
            "--hostname",
            "github.com",
            &format!("repos/{fork}/git/ref/heads/{encoded}"),
        ])?;
        Ok(value.and_then(|item| item["object"]["sha"].as_str().map(str::to_string)))
    }

    fn push_branch(&mut self, root: &Path, fork: &str, branch: &str, head: &str) -> Result<()> {
        let remote = format!("https://github.com/{fork}.git");
        let refspec = format!("{head}:refs/heads/{branch}");
        ensure_no_git_push_rewrites(root)?;
        if run_output("git", &["ls-remote", "--get-url", &remote], root, None)? != remote {
            bail!("Git configuration rewrites the exact approved GitHub destination");
        }
        run_status(
            "git",
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "push",
                "--no-verify",
                "--porcelain",
                &remote,
                &refspec,
            ],
            root,
            None,
        )
    }

    fn find_drafts(
        &mut self,
        action: &GitHubActionDigest,
        base_branch: &str,
    ) -> Result<Vec<PullIdentity>> {
        let fork = action
            .fork
            .as_deref()
            .context("operator-owned fork is required")?;
        let (owner, _) = split_repository(fork)?;
        gh_json(&[
            "api",
            "--hostname",
            "github.com",
            "--method",
            "GET",
            &format!("repos/{}/pulls", action.repository),
            "-f",
            "state=open",
            "-f",
            &format!("head={owner}:{}", action.branch),
            "-f",
            &format!("base={base_branch}"),
            "-f",
            "per_page=10",
        ])
    }

    fn create_draft(
        &mut self,
        plan: &PublicationPlan,
        base_branch: &str,
    ) -> Result<CreateDraftOutcome> {
        let action = &plan.draft_action;
        let fork = action
            .fork
            .as_deref()
            .context("operator-owned fork is required")?;
        let (owner, _) = split_repository(fork)?;
        let body = serde_json::to_vec(&serde_json::json!({
            "title": plan.draft.title,
            "body": plan.draft.body,
            "head": format!("{owner}:{}", action.branch),
            "base": base_branch,
            "draft": true,
        }))?;
        let input = [
            "api",
            "--hostname",
            "github.com",
            "--method",
            "POST",
            &format!("repos/{}/pulls", action.repository),
            "--input",
            "-",
        ];
        let output = command_output("gh", &input, Path::new("."), Some(&body))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("HTTP 409") || stderr.contains("HTTP 422") {
                return Ok(CreateDraftOutcome::Conflict);
            }
            bail!(
                "GitHub draft creation failed closed with status {}",
                output.status
            );
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("GitHub draft creation returned malformed JSON")?;
        let number = value["number"]
            .as_u64()
            .filter(|number| *number > 0)
            .context("created draft pull request omitted a positive number")?;
        Ok(CreateDraftOutcome::Created(number))
    }

    fn read_pull(&mut self, repository: &str, number: u64) -> Result<PullIdentity> {
        gh_json(&[
            "api",
            "--hostname",
            "github.com",
            &format!("repos/{repository}/pulls/{number}"),
        ])
    }
}

fn gh_json<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Result<T> {
    let bytes = run_bytes("gh", args, Path::new("."), None)?;
    serde_json::from_slice(&bytes).context("GitHub CLI returned malformed JSON")
}

fn gh_optional<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Result<Option<T>> {
    let output = command_output("gh", args, Path::new("."), None)?;
    if output.status.success() {
        return serde_json::from_slice(&output.stdout)
            .map(Some)
            .context("GitHub CLI returned malformed JSON");
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("HTTP 404") {
        return Ok(None);
    }
    bail!(
        "GitHub CLI request failed closed with status {}",
        output.status
    )
}

fn command_output(
    program: &str,
    args: &[&str],
    cwd: &Path,
    input: Option<&[u8]>,
) -> Result<std::process::Output> {
    command_output_with_timeout(program, args, cwd, input, COMMAND_TIMEOUT)
}

fn command_output_with_timeout(
    program: &str,
    args: &[&str],
    cwd: &Path,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<std::process::Output> {
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    let process_job = {
        let mut limits = win32job::ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        win32job::Job::create_with_limit_info(&limits)
            .context("create kill-on-close Windows process job")?
    };
    let mut child = command
        .spawn()
        .with_context(|| format!("start bounded {program} command"))?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        if let Err(error) = process_job.assign_process(child.as_raw_handle() as isize) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).context("assign bounded command to Windows process job");
        }
    }
    let stdout = child.stdout.take().context("open bounded command stdout")?;
    let stderr = child.stderr.take().context("open bounded command stderr")?;
    let (stdout_sender, stdout_reader) = mpsc::sync_channel(1);
    let (stderr_sender, stderr_reader) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = stdout_sender.send(read_bounded_pipe(stdout));
    });
    thread::spawn(move || {
        let _ = stderr_sender.send(read_bounded_pipe(stderr));
    });
    let stdin_reader = input.map(|bytes| {
        let mut stdin = child.stdin.take().context("open bounded command stdin")?;
        let bytes = bytes.to_vec();
        let (sender, reader) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let result = stdin.write_all(&bytes);
            drop(stdin);
            let _ = sender.send(result);
        });
        Ok::<_, anyhow::Error>(reader)
    });
    let stdin_reader = stdin_reader.transpose()?;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("poll bounded {program} command"))?
        {
            break status;
        }
        if started.elapsed() >= timeout {
            #[cfg(unix)]
            terminate_process_tree(child.id());
            let _ = child.kill();
            let _ = child.wait();
            bail!("bounded {program} command exceeded its configured timeout");
        }
        thread::sleep(Duration::from_millis(25));
    };
    if let Some(reader) = stdin_reader {
        reader
            .recv_timeout(remaining_command_time(started, child.id(), timeout)?)
            .map_err(|_| {
                #[cfg(unix)]
                terminate_process_tree(child.id());
                anyhow::anyhow!("bounded {program} stdin did not close before the deadline")
            })?
            .context("write bounded command stdin")?;
    }
    let (stdout, stdout_exceeded) = stdout_reader
        .recv_timeout(remaining_pipe_time(started, child.id(), timeout)?)
        .map_err(|_| {
            #[cfg(unix)]
            terminate_process_tree(child.id());
            anyhow::anyhow!("bounded {program} stdout did not close before the deadline")
        })??;
    let (stderr, stderr_exceeded) = stderr_reader
        .recv_timeout(remaining_pipe_time(started, child.id(), timeout)?)
        .map_err(|_| {
            #[cfg(unix)]
            terminate_process_tree(child.id());
            anyhow::anyhow!("bounded {program} stderr did not close before the deadline")
        })??;
    #[cfg(unix)]
    terminate_process_tree(child.id());
    if stdout_exceeded || stderr_exceeded {
        bail!("bounded {program} command output exceeded the 1048576-byte limit");
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn remaining_command_time(started: Instant, _pid: u32, timeout: Duration) -> Result<Duration> {
    let remaining = timeout
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .context("bounded command exceeded its configured timeout");
    if remaining.is_err() {
        #[cfg(unix)]
        terminate_process_tree(_pid);
    }
    remaining
}

fn remaining_pipe_time(started: Instant, pid: u32, timeout: Duration) -> Result<Duration> {
    remaining_command_time(started, pid, timeout)
        .map(|remaining| remaining.min(Duration::from_secs(1)))
}

#[cfg(unix)]
fn terminate_process_tree(pid: u32) {
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn read_bounded_pipe<R: Read>(mut reader: R) -> std::io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::new();
    let mut exceeded = false;
    let mut buffer = [0_u8; 8_192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            exceeded = true;
        }
    }
    Ok((retained, exceeded))
}

fn run_bytes(program: &str, args: &[&str], cwd: &Path, input: Option<&[u8]>) -> Result<Vec<u8>> {
    let output = command_output(program, args, cwd, input)?;
    if !output.status.success() {
        bail!(
            "bounded {program} command failed closed with status {}",
            output.status
        );
    }
    Ok(output.stdout)
}

fn run_output(program: &str, args: &[&str], cwd: &Path, input: Option<&[u8]>) -> Result<String> {
    let bytes = run_bytes(program, args, cwd, input)?;
    Ok(String::from_utf8(bytes)?.trim().to_string())
}

fn run_status(program: &str, args: &[&str], cwd: &Path, input: Option<&[u8]>) -> Result<()> {
    run_bytes(program, args, cwd, input).map(|_| ())
}

fn ensure_no_git_push_rewrites(root: &Path) -> Result<()> {
    let output = command_output(
        "git",
        &["config", "--show-origin", "--name-only", "--list"],
        root,
        None,
    )?;
    if !output.status.success() {
        bail!("Git configuration could not be inspected before the governed push");
    }
    let configured = String::from_utf8(output.stdout).context("Git configuration was not UTF-8")?;
    if configured.lines().any(|line| {
        line.split_whitespace().last().is_some_and(|key| {
            let key = key.to_ascii_lowercase();
            key.starts_with("url.") && key.ends_with(".pushinsteadof")
        })
    }) {
        bail!("Git pushInsteadOf configuration is forbidden for governed publication");
    }
    Ok(())
}

fn read_governed_diff(root: &Path, base: &str, head: &str) -> Result<Vec<u8>> {
    run_bytes(
        "git",
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            &format!("{base}..{head}"),
        ],
        root,
        None,
    )
}

fn read_governed_changed_paths(root: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let output = run_bytes(
        "git",
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            &format!("{base}..{head}"),
            "--",
        ],
        root,
        None,
    )?;
    if output.is_empty() {
        bail!("approved diff must change at least one path");
    }
    output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|bytes| {
            let path =
                std::str::from_utf8(bytes).context("approved diff changed path is not UTF-8")?;
            if path.is_empty()
                || path.starts_with('/')
                || path.contains('\\')
                || path
                    .split('/')
                    .any(|component| component.is_empty() || component == "..")
            {
                bail!("approved diff contains an unsafe changed path");
            }
            Ok(path.to_string())
        })
        .collect()
}

fn derive_protected_path_touched(changed_paths: &[String], patterns: &[String]) -> Result<bool> {
    for pattern in patterns {
        if pattern.is_empty() || pattern.starts_with('/') || pattern.contains('\\') {
            bail!("protected path pattern is unsafe");
        }
        let prefix = pattern.strip_suffix("/**");
        if pattern.contains('*') && prefix.is_none() {
            bail!("protected path pattern uses an unsupported wildcard");
        }
        for path in changed_paths {
            if path.is_empty()
                || path.starts_with('/')
                || path.contains('\\')
                || path
                    .split('/')
                    .any(|component| component.is_empty() || component == "..")
            {
                bail!("changed path is unsafe");
            }
            if let Some(prefix) = prefix {
                if prefix.is_empty() {
                    bail!("protected path prefix is empty");
                }
                if path == prefix {
                    bail!("changed path collides with a protected directory root");
                }
                if path.starts_with(&format!("{prefix}/")) {
                    return Ok(true);
                }
            } else if path == pattern {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn split_repository(value: &str) -> Result<(&str, &str)> {
    let mut parts = value.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if parts.next().is_some() || !valid_slug(owner) || !valid_slug(repo) {
        bail!("repository identity must be owner/repo using bounded GitHub slug characters");
    }
    Ok((owner, repo))
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}

fn validate_ref(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("@{")
        || value.ends_with(".lock")
        || value.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(byte, b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        })
    {
        bail!("repair branch is not a bounded Git reference");
    }
    Ok(())
}

fn validate_schema_branch(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 255
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
    {
        bail!("branch does not match the pinned Architecture pattern");
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!("{name} must be a bounded identifier");
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > max
        || value.bytes().any(|byte| byte == 0 || byte == 0x7f)
    {
        bail!("{name} must be nonempty and at most {max} bytes without control delimiters");
    }
    Ok(())
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(is_lower_hex)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_hex)
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn sha256(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonicalize).collect())
        }
        serde_json::Value::Object(values) => {
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            serde_json::Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonicalize(&values[key])))
                    .collect(),
            )
        }
        value => value.clone(),
    }
}

fn canonical_json_sha256(value: &serde_json::Value) -> String {
    sha256_hex(serde_json::to_vec(&canonicalize(value)).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    type PlanMutation = (&'static str, Box<dyn Fn(&mut PublicationPlan)>);

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-28T12:30:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn action(operation: &str) -> GitHubActionDigest {
        let title = "Fix bounded fixture";
        let body = "Repairs #101 with exact evidence.";
        let mut action = GitHubActionDigest {
            schema: ACTION_SCHEMA.to_string(),
            run_id: "repair-run-20260728".to_string(),
            repository: "fixture/repair".to_string(),
            issue_number: 101,
            base_sha: "1".repeat(40),
            head_sha: "b".repeat(40),
            fork: Some("operator/repair".to_string()),
            branch: "codex/repair-101".to_string(),
            pr_title_digest: sha256(title.as_bytes()),
            pr_body_digest: sha256(body.as_bytes()),
            diff_digest: "f".repeat(64),
            required_checks: vec![
                RequiredCheck {
                    name: "lint".to_string(),
                    conclusion: "success".to_string(),
                    head_sha: "b".repeat(40),
                },
                RequiredCheck {
                    name: "test".to_string(),
                    conclusion: "success".to_string(),
                    head_sha: "b".repeat(40),
                },
            ],
            action: operation.to_string(),
            approved_at: "2026-07-28T12:00:00Z".to_string(),
            expires_at: "2026-07-28T13:00:00Z".to_string(),
            run_envelope_digest: "2".repeat(64),
            candidate_decision_digest: "3".repeat(64),
            governance_decision_digest: "4".repeat(64),
            reviewer_independence_digest: "5".repeat(64),
            action_digest: String::new(),
        };
        let mut value = serde_json::to_value(&action).unwrap();
        value.as_object_mut().unwrap().remove("action_digest");
        action.action_digest = canonical_json_sha256(&value);
        action
    }

    fn authority() -> AuthorityBundle {
        fn bind(mut value: serde_json::Value, field: &str) -> serde_json::Value {
            let digest = canonical_json_sha256(&value);
            value[field] = serde_json::Value::String(digest);
            value
        }
        let run_envelope = bind(
            serde_json::json!({
                "schema": "ao.architecture.autonomous-issue-repair.run-envelope.v1",
                "run_id": "repair-run-20260728",
                "loop": {
                    "goal": "Repair one bounded issue.",
                    "trigger": "Use the pinned repository.",
                    "discovery": "Select one authentic bug.",
                    "action": "Prepare one repair.",
                    "verification": "Require exact checks.",
                    "state": "Persist digest-bound state.",
                    "human_gates": "Require exact action approval."
                },
                "trigger": {
                    "mode": "issue_list",
                    "canonical_url": "https://github.com/fixture/repair/issues",
                    "repository": "fixture/repair",
                    "default_branch": "main",
                    "pinned_base_commit": "1".repeat(40)
                },
                "discovery": {
                    "snapshot_limit": 50,
                    "candidate_limit": 10,
                    "selected_limit": 1
                },
                "budgets": {
                    "wall_clock_seconds": 7200,
                    "clone_count": 1,
                    "test_runs": 2,
                    "retry_count": 1,
                    "repair_count": 1,
                    "publication_count": 1
                },
                "governance": {
                    "ownership_class": "external",
                    "allowed_actions": ["push_operator_fork", "open_upstream_draft_pr"],
                    "denied_actions": [
                        "push_upstream", "open_ready_pr", "mark_ready", "approve_review",
                        "merge", "mutate_issue", "publish_release"
                    ],
                    "sole_control_auto_merge_opt_in": false
                },
                "routing": {
                    "default_branch": "main",
                    "pinned_base_commit": "1".repeat(40),
                    "fork_owner": "operator",
                    "repair_branch": "codex/repair-101",
                    "protected_path_classes": ["workflow"],
                    "required_checks": ["lint", "test"]
                },
                "created_at": "2026-07-28T11:30:00Z",
                "expires_at": "2026-07-28T13:00:00Z",
                "predecessor_digest": null,
                "lineage": {
                    "kind": "origin",
                    "predecessor_run_id": null,
                    "predecessor_digest": null
                },
                "stop_conditions": ["digest_mismatch"],
                "terminal_statuses": ["completed", "blocked"]
            }),
            "canonical_digest",
        );
        let candidate_decision = bind(
            serde_json::json!({
                "schema": "ao.architecture.autonomous-issue-repair.candidate-decision.v1",
                "run_id": "repair-run-20260728",
                "repository": "fixture/repair",
                "base_sha": "1".repeat(40),
                "issue_number": 101,
                "rank": 1,
                "decision": "selected",
                "eligibility": {
                    "open_bug": true,
                    "target_in_repository": true,
                    "no_existing_fix": true,
                    "current_head_unfixed": true,
                    "security_sensitive": false,
                    "public_reproduction_feasible": true,
                    "deterministic_local_reproduction": true,
                    "expected_behavior_grounded": true,
                    "bounded_policy_compatible": true
                },
                "reason_codes": ["eligible_all_predicates_passed"],
                "evidence_digests": ["6".repeat(64)],
                "expected_behavior_source": "tests",
                "decided_at": "2026-07-28T11:45:00Z"
            }),
            "decision_digest",
        );
        let governance_decision = bind(
            serde_json::json!({
                "schema": "ao.architecture.autonomous-issue-repair.governance-decision.v1",
                "run_id": "repair-run-20260728",
                "repository": "fixture/repair",
                "base_sha": "1".repeat(40),
                "head_sha": "b".repeat(40),
                "governance_class": "external",
                "classification_sources": ["repository_policy", "operator_envelope"],
                "push_target": "operator_owned_fork",
                "pull_request_mode": "upstream_draft_only",
                "merge": {
                    "authorized": false,
                    "mode": "never",
                    "approval_kind": "none",
                    "approval_head_sha": null,
                    "auto_merge_opt_in": false,
                    "branch_protection_bypassed": false
                },
                "protected_path_touched": false,
                "required_checks": [
                    {"name": "lint", "conclusion": "success", "head_sha": "b".repeat(40)},
                    {"name": "test", "conclusion": "success", "head_sha": "b".repeat(40)}
                ],
                "action_digest_required": true,
                "decided_at": "2026-07-28T11:50:00Z"
            }),
            "decision_digest",
        );
        let reviewer_independence = bind(
            serde_json::json!({
                "schema": "ao.architecture.autonomous-issue-repair.reviewer-independence.v1",
                "run_id": "repair-run-20260728",
                "subject_digest": "f".repeat(64),
                "reviewer_id": "independent-reviewer",
                "status": "independent",
                "deterministic_tests_primary": true,
                "satisfies_team_merge_gate": false,
                "reviewed_at": "2026-07-28T11:55:00Z"
            }),
            "review_digest",
        );
        AuthorityBundle {
            run_envelope,
            candidate_decision,
            governance_decision,
            reviewer_independence,
        }
    }

    fn plan() -> PublicationPlan {
        let authority = authority();
        let mut push_action = action("push_operator_fork");
        let mut draft_action = action("open_upstream_draft_pr");
        for action in [&mut push_action, &mut draft_action] {
            action.run_envelope_digest =
                json_string(&authority.run_envelope, &["canonical_digest"])
                    .unwrap()
                    .to_string();
            action.candidate_decision_digest =
                json_string(&authority.candidate_decision, &["decision_digest"])
                    .unwrap()
                    .to_string();
            action.governance_decision_digest =
                json_string(&authority.governance_decision, &["decision_digest"])
                    .unwrap()
                    .to_string();
            action.reviewer_independence_digest =
                json_string(&authority.reviewer_independence, &["review_digest"])
                    .unwrap()
                    .to_string();
            let mut value = serde_json::to_value(&*action).unwrap();
            value.as_object_mut().unwrap().remove("action_digest");
            action.action_digest = canonical_json_sha256(&value);
        }
        PublicationPlan {
            schema_version: "ao2.github-repair-publication-plan.v1".to_string(),
            architecture_contract_commit: ARCHITECTURE_CONTRACT_COMMIT.to_string(),
            authority,
            push_action,
            draft_action,
            draft: DraftText {
                title: "Fix bounded fixture".to_string(),
                body: "Repairs #101 with exact evidence.".to_string(),
            },
        }
    }

    fn validate(candidate: &PublicationPlan) -> Result<()> {
        validate_plan(
            candidate,
            &candidate.push_action.action_digest,
            &candidate.draft_action.action_digest,
            fixed_now(),
        )
    }

    fn rebind_authority(candidate: &mut PublicationPlan) {
        for (document, field) in [
            (&mut candidate.authority.run_envelope, "canonical_digest"),
            (
                &mut candidate.authority.candidate_decision,
                "decision_digest",
            ),
            (
                &mut candidate.authority.governance_decision,
                "decision_digest",
            ),
            (
                &mut candidate.authority.reviewer_independence,
                "review_digest",
            ),
        ] {
            document.as_object_mut().unwrap().remove(field);
            document[field] = serde_json::Value::String(canonical_json_sha256(document));
        }
        for action in [&mut candidate.push_action, &mut candidate.draft_action] {
            action.run_envelope_digest =
                json_string(&candidate.authority.run_envelope, &["canonical_digest"])
                    .unwrap()
                    .to_string();
            action.candidate_decision_digest = json_string(
                &candidate.authority.candidate_decision,
                &["decision_digest"],
            )
            .unwrap()
            .to_string();
            action.governance_decision_digest = json_string(
                &candidate.authority.governance_decision,
                &["decision_digest"],
            )
            .unwrap()
            .to_string();
            action.reviewer_independence_digest = json_string(
                &candidate.authority.reviewer_independence,
                &["review_digest"],
            )
            .unwrap()
            .to_string();
            let mut value = serde_json::to_value(&*action).unwrap();
            value.as_object_mut().unwrap().remove("action_digest");
            action.action_digest = canonical_json_sha256(&value);
        }
    }

    fn fork_identity(parent: &str) -> ForkIdentity {
        ForkIdentity {
            full_name: "operator/repair".to_string(),
            default_branch: "main".to_string(),
            owner: OwnerIdentity {
                login: "operator".to_string(),
            },
            parent: Some(ParentIdentity {
                full_name: parent.to_string(),
            }),
        }
    }

    fn pull_identity(plan: &PublicationPlan) -> PullIdentity {
        PullIdentity {
            number: 7,
            state: "open".to_string(),
            draft: true,
            merged_at: None,
            title: plan.draft.title.clone(),
            body: plan.draft.body.clone(),
            base: PullRef {
                sha: plan.draft_action.base_sha.clone(),
                reference: "main".to_string(),
            },
            head: PullHead {
                sha: plan.draft_action.head_sha.clone(),
                reference: plan.draft_action.branch.clone(),
                repo: PullRepository {
                    full_name: plan.draft_action.fork.clone().unwrap(),
                },
            },
        }
    }

    struct FakeTransport {
        login: String,
        repository: RepositoryIdentity,
        forks: VecDeque<Option<ForkIdentity>>,
        branches: VecDeque<Option<String>>,
        draft_lookups: VecDeque<Vec<PullIdentity>>,
        create_outcome: CreateDraftOutcome,
        pull: PullIdentity,
        calls: Vec<&'static str>,
        clocks: VecDeque<DateTime<Utc>>,
    }

    impl FakeTransport {
        fn absent(candidate: &PublicationPlan) -> Self {
            Self {
                login: "operator".to_string(),
                repository: RepositoryIdentity {
                    full_name: "fixture/repair".to_string(),
                    default_branch: "main".to_string(),
                },
                forks: VecDeque::from([None, Some(fork_identity("fixture/repair"))]),
                branches: VecDeque::from([
                    Some(candidate.push_action.base_sha.clone()),
                    None,
                    Some(candidate.push_action.head_sha.clone()),
                ]),
                draft_lookups: VecDeque::from([vec![]]),
                create_outcome: CreateDraftOutcome::Created(7),
                pull: pull_identity(candidate),
                calls: vec![],
                clocks: VecDeque::from([fixed_now(), fixed_now(), fixed_now()]),
            }
        }

        fn idempotent(candidate: &PublicationPlan) -> Self {
            Self {
                forks: VecDeque::from([Some(fork_identity("fixture/repair"))]),
                branches: VecDeque::from([
                    Some(candidate.push_action.base_sha.clone()),
                    Some(candidate.push_action.head_sha.clone()),
                ]),
                draft_lookups: VecDeque::from([vec![pull_identity(candidate)]]),
                ..Self::absent(candidate)
            }
        }
    }

    impl PublicationTransport for FakeTransport {
        fn now(&mut self) -> DateTime<Utc> {
            self.clocks.pop_front().unwrap_or_else(fixed_now)
        }

        fn verify_local_repository(&mut self, _root: &Path, _plan: &PublicationPlan) -> Result<()> {
            self.calls.push("verify_local");
            Ok(())
        }

        fn authenticated_login(&mut self) -> Result<String> {
            self.calls.push("login");
            Ok(self.login.clone())
        }

        fn read_repository(&mut self, _repository: &str) -> Result<RepositoryIdentity> {
            self.calls.push("read_repository");
            Ok(self.repository.clone())
        }

        fn read_fork(&mut self, _fork: &str) -> Result<Option<ForkIdentity>> {
            self.calls.push("read_fork");
            self.forks.pop_front().context("unexpected fork read")
        }

        fn create_fork(&mut self, _upstream: &str) -> Result<()> {
            self.calls.push("create_fork");
            Ok(())
        }

        fn read_branch(&mut self, _fork: &str, _branch: &str) -> Result<Option<String>> {
            self.calls.push("read_branch");
            self.branches.pop_front().context("unexpected branch read")
        }

        fn push_branch(
            &mut self,
            _root: &Path,
            _fork: &str,
            _branch: &str,
            _head: &str,
        ) -> Result<()> {
            self.calls.push("push_branch");
            Ok(())
        }

        fn find_drafts(
            &mut self,
            _action: &GitHubActionDigest,
            _base_branch: &str,
        ) -> Result<Vec<PullIdentity>> {
            self.calls.push("find_drafts");
            self.draft_lookups
                .pop_front()
                .context("unexpected draft lookup")
        }

        fn create_draft(
            &mut self,
            _plan: &PublicationPlan,
            _base_branch: &str,
        ) -> Result<CreateDraftOutcome> {
            self.calls.push("create_draft");
            Ok(match self.create_outcome {
                CreateDraftOutcome::Created(number) => CreateDraftOutcome::Created(number),
                CreateDraftOutcome::Conflict => CreateDraftOutcome::Conflict,
            })
        }

        fn read_pull(&mut self, _repository: &str, _number: u64) -> Result<PullIdentity> {
            self.calls.push("read_pull");
            Ok(self.pull.clone())
        }

        fn retry_delay(&mut self) {}
    }

    #[test]
    fn valid_plan_binds_distinct_actions_title_body_checks_and_expiry() {
        validate(&plan()).unwrap();
    }

    #[test]
    fn self_consistent_authority_still_requires_all_architecture_semantics() {
        let cases: Vec<PlanMutation> = vec![
            (
                "envelope lifetime",
                Box::new(|p| {
                    p.authority.run_envelope["budgets"]["wall_clock_seconds"] =
                        serde_json::json!(60);
                }),
            ),
            (
                "external ready action",
                Box::new(|p| {
                    p.authority.run_envelope["governance"]["allowed_actions"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!("open_ready_pr"));
                }),
            ),
            (
                "external envelope auto-merge opt-in",
                Box::new(|p| {
                    p.authority.run_envelope["governance"]["sole_control_auto_merge_opt_in"] =
                        serde_json::json!(true);
                }),
            ),
            (
                "selected candidate without grounded source",
                Box::new(|p| {
                    p.authority.candidate_decision["expected_behavior_source"] =
                        serde_json::json!("unavailable");
                }),
            ),
            (
                "external merge approval",
                Box::new(|p| {
                    p.authority.governance_decision["merge"]["approval_kind"] =
                        serde_json::json!("independent_human");
                    p.authority.governance_decision["merge"]["approval_head_sha"] =
                        serde_json::json!("b".repeat(40));
                }),
            ),
            (
                "external auto-merge opt-in",
                Box::new(|p| {
                    p.authority.governance_decision["merge"]["auto_merge_opt_in"] =
                        serde_json::json!(true);
                }),
            ),
            (
                "branch outside pinned schema alphabet",
                Box::new(|p| {
                    p.authority.run_envelope["routing"]["repair_branch"] =
                        serde_json::json!("codex/repair!101");
                    p.push_action.branch = "codex/repair!101".to_string();
                    p.draft_action.branch = "codex/repair!101".to_string();
                }),
            ),
            (
                "explicit issue URL with a leading zero",
                Box::new(|p| {
                    p.authority.run_envelope["trigger"]["mode"] =
                        serde_json::json!("explicit_issue");
                    p.authority.run_envelope["trigger"]["canonical_url"] =
                        serde_json::json!("https://github.com/fixture/repair/issues/0101");
                }),
            ),
        ];
        for (name, mutate) in cases {
            let mut candidate = plan();
            mutate(&mut candidate);
            rebind_authority(&mut candidate);
            assert!(
                validate(&candidate).is_err(),
                "{name} semantic violation was accepted"
            );
        }
    }

    #[test]
    fn command_pipe_drains_but_rejects_output_above_the_retention_limit() {
        let (exact, exact_exceeded) =
            read_bounded_pipe(std::io::Cursor::new(vec![b'x'; MAX_COMMAND_OUTPUT_BYTES])).unwrap();
        assert_eq!(exact.len(), MAX_COMMAND_OUTPUT_BYTES);
        assert!(!exact_exceeded);

        let (bounded, exceeded) = read_bounded_pipe(std::io::Cursor::new(vec![
            b'x';
            MAX_COMMAND_OUTPUT_BYTES
                + 1
        ]))
        .unwrap();
        assert_eq!(bounded.len(), MAX_COMMAND_OUTPUT_BYTES);
        assert!(exceeded);
    }

    #[test]
    #[cfg(unix)]
    fn command_deadline_rejects_a_descendant_that_keeps_output_open() {
        let started = Instant::now();
        let result = command_output("/bin/sh", &["-c", "(sleep 30) &"], Path::new("."), None);
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    #[cfg(unix)]
    fn command_deadline_includes_blocked_stdin_delivery() {
        let input = vec![b'x'; 2 * 1024 * 1024];
        let started = Instant::now();
        let result = command_output_with_timeout(
            "/bin/sh",
            &["-c", "sleep 30"],
            Path::new("."),
            Some(&input),
            Duration::from_millis(250),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    #[cfg(unix)]
    fn successful_command_terminates_silent_descendants() {
        let output = command_output_with_timeout(
            "/bin/sh",
            &[
                "-c",
                "(sleep 30 </dev/null >/dev/null 2>&1) & printf '%s' \"$!\"",
            ],
            Path::new("."),
            None,
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(output.status.success());
        let pid = String::from_utf8(output.stdout).unwrap();
        let mut alive = true;
        for _ in 0..20 {
            alive = Command::new("/bin/kill")
                .args(["-0", pid.trim()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !alive {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        assert!(!alive, "silent command descendant survived success cleanup");
    }

    #[test]
    fn governed_push_rejects_push_instead_of_configuration() {
        let root = tempfile::tempdir().unwrap();
        run_status("git", &["init", "--quiet"], root.path(), None).unwrap();
        run_status(
            "git",
            &[
                "config",
                "--local",
                "url.ext::malicious.pushInsteadOf",
                "https://github.com/",
            ],
            root.path(),
            None,
        )
        .unwrap();
        assert!(ensure_no_git_push_rewrites(root.path()).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn governed_diff_never_executes_external_or_textconv_helpers() {
        let root = tempfile::tempdir().unwrap();
        run_status("git", &["init", "--quiet"], root.path(), None).unwrap();
        run_status(
            "git",
            &["config", "user.email", "fixture@example.invalid"],
            root.path(),
            None,
        )
        .unwrap();
        run_status(
            "git",
            &["config", "user.name", "Fixture"],
            root.path(),
            None,
        )
        .unwrap();
        std::fs::write(root.path().join(".gitattributes"), "*.txt diff=fixture\n").unwrap();
        std::fs::write(root.path().join("subject.txt"), "before\n").unwrap();
        run_status("git", &["add", "."], root.path(), None).unwrap();
        run_status(
            "git",
            &["commit", "--quiet", "-m", "base"],
            root.path(),
            None,
        )
        .unwrap();
        let base = run_output("git", &["rev-parse", "HEAD"], root.path(), None).unwrap();
        std::fs::write(root.path().join("subject.txt"), "after\n").unwrap();
        run_status("git", &["add", "."], root.path(), None).unwrap();
        run_status(
            "git",
            &["commit", "--quiet", "-m", "head"],
            root.path(),
            None,
        )
        .unwrap();
        let head = run_output("git", &["rev-parse", "HEAD"], root.path(), None).unwrap();
        let marker = root.path().join("helper-executed");
        let helper = root.path().join("helper.sh");
        std::fs::write(
            &helper,
            format!("#!/bin/sh\n: > '{}'\nexit 99\n", marker.display()),
        )
        .unwrap();
        run_status(
            "/bin/chmod",
            &["+x", helper.to_str().unwrap()],
            root.path(),
            None,
        )
        .unwrap();

        run_status(
            "git",
            &["config", "diff.external", helper.to_str().unwrap()],
            root.path(),
            None,
        )
        .unwrap();
        read_governed_diff(root.path(), &base, &head).unwrap();
        assert!(!marker.exists());

        run_status(
            "git",
            &["config", "--unset", "diff.external"],
            root.path(),
            None,
        )
        .unwrap();
        run_status(
            "git",
            &["config", "diff.fixture.textconv", helper.to_str().unwrap()],
            root.path(),
            None,
        )
        .unwrap();
        read_governed_diff(root.path(), &base, &head).unwrap();
        assert!(!marker.exists());
    }

    #[test]
    fn protected_path_contact_is_derived_from_changed_paths() {
        let patterns = vec![".ao/**".to_string(), ".github/**".to_string()];
        assert!(derive_protected_path_touched(
            &[".github/workflows/ci.yml".to_string()],
            &patterns,
        )
        .unwrap());
        assert!(derive_protected_path_touched(
            &[".ao/autonomous-repair-governance.json".to_string()],
            &patterns,
        )
        .unwrap());
        assert!(!derive_protected_path_touched(
            &[
                "eligibility.go".to_string(),
                "eligibility_regression_test.go".to_string(),
            ],
            &patterns,
        )
        .unwrap());
        assert!(derive_protected_path_touched(&[".github".to_string()], &patterns).is_err());
    }

    #[test]
    fn strict_decoder_rejects_duplicates_at_every_depth_and_trailing_json() {
        let encoded = serde_json::to_string_pretty(&plan()).unwrap();
        let top_level = encoded.replacen(
            "\"schema_version\": \"ao2.github-repair-publication-plan.v1\"",
            "\"schema_version\": \"ao2.github-repair-publication-plan.v1\",\n  \
             \"schema_version\": \"ao2.github-repair-publication-plan.v1\"",
            1,
        );
        assert!(decode_publication_plan_strict(top_level.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("duplicate"));

        let nested_array = encoded.replacen(
            "\"conclusion\": \"success\"",
            "\"conclusion\": \"success\", \"conclusion\": \"success\"",
            1,
        );
        assert!(decode_publication_plan_strict(nested_array.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("duplicate"));

        let trailing = format!("{encoded}\n{{}}");
        assert!(decode_publication_plan_strict(trailing.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("trailing JSON"));
    }

    #[test]
    fn local_verification_rejects_redigested_false_protected_path_claim() {
        let root = tempfile::tempdir().unwrap();
        run_status("git", &["init", "--quiet"], root.path(), None).unwrap();
        run_status(
            "git",
            &["config", "user.email", "fixture@example.invalid"],
            root.path(),
            None,
        )
        .unwrap();
        run_status(
            "git",
            &["config", "user.name", "Fixture"],
            root.path(),
            None,
        )
        .unwrap();
        std::fs::write(root.path().join("subject.txt"), "base\n").unwrap();
        run_status("git", &["add", "."], root.path(), None).unwrap();
        run_status(
            "git",
            &["commit", "--quiet", "-m", "base"],
            root.path(),
            None,
        )
        .unwrap();
        let base = run_output("git", &["rev-parse", "HEAD"], root.path(), None).unwrap();
        std::fs::create_dir_all(root.path().join(".github/workflows")).unwrap();
        std::fs::write(
            root.path().join(".github/workflows/ci.yml"),
            "name: protected\n",
        )
        .unwrap();
        run_status("git", &["add", "."], root.path(), None).unwrap();
        run_status(
            "git",
            &["commit", "--quiet", "-m", "protected"],
            root.path(),
            None,
        )
        .unwrap();
        let head = run_output("git", &["rev-parse", "HEAD"], root.path(), None).unwrap();
        let diff_digest = sha256(&read_governed_diff(root.path(), &base, &head).unwrap());

        let mut candidate = plan();
        candidate.authority.run_envelope["trigger"]["pinned_base_commit"] =
            serde_json::Value::String(base.clone());
        candidate.authority.run_envelope["routing"]["pinned_base_commit"] =
            serde_json::Value::String(base.clone());
        candidate.authority.run_envelope["routing"]["protected_path_classes"] =
            serde_json::json!([".github/**"]);
        candidate.authority.candidate_decision["base_sha"] =
            serde_json::Value::String(base.clone());
        candidate.authority.governance_decision["base_sha"] =
            serde_json::Value::String(base.clone());
        candidate.authority.governance_decision["head_sha"] =
            serde_json::Value::String(head.clone());
        candidate.authority.governance_decision["required_checks"][0]["head_sha"] =
            serde_json::Value::String(head.clone());
        candidate.authority.governance_decision["protected_path_touched"] =
            serde_json::Value::Bool(false);
        candidate.authority.reviewer_independence["subject_digest"] =
            serde_json::Value::String(diff_digest.clone());
        for action in [&mut candidate.push_action, &mut candidate.draft_action] {
            action.base_sha = base.clone();
            action.head_sha = head.clone();
            action.diff_digest = diff_digest.clone();
            action.required_checks[0].head_sha = head.clone();
        }
        rebind_authority(&mut candidate);

        let result = SystemTransport.verify_local_repository(root.path(), &candidate);
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("protected-path claim"));

        candidate.authority.governance_decision["protected_path_touched"] =
            serde_json::Value::Bool(true);
        rebind_authority(&mut candidate);
        let result = SystemTransport.verify_local_repository(root.path(), &candidate);
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("touches a protected path"));
    }

    #[test]
    #[cfg(windows)]
    fn windows_job_deadline_includes_blocked_stdin_delivery() {
        let input = vec![b'x'; 2 * 1024 * 1024];
        let started = Instant::now();
        let result = command_output_with_timeout(
            "powershell.exe",
            &["-NoProfile", "-Command", "Start-Sleep -Seconds 30"],
            Path::new("."),
            Some(&input),
            Duration::from_millis(500),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn plan_rejects_every_authority_relevant_drift() {
        let cases: Vec<PlanMutation> = vec![
            (
                "producer",
                Box::new(|p| p.architecture_contract_commit = "0".repeat(40)),
            ),
            (
                "push operation",
                Box::new(|p| p.push_action.action = "auto_merge".to_string()),
            ),
            (
                "draft operation",
                Box::new(|p| p.draft_action.action = "open_ready_pr".to_string()),
            ),
            (
                "head check",
                Box::new(|p| p.push_action.required_checks[0].head_sha = "c".repeat(40)),
            ),
            (
                "failed check",
                Box::new(|p| p.push_action.required_checks[0].conclusion = "failure".to_string()),
            ),
            (
                "fork",
                Box::new(|p| p.push_action.fork = Some("attacker/repair".to_string())),
            ),
            (
                "branch",
                Box::new(|p| p.draft_action.branch = "codex/other".to_string()),
            ),
            ("title", Box::new(|p| p.draft.title.push('!'))),
            (
                "stale",
                Box::new(|p| {
                    p.push_action.approved_at = "2026-07-28T10:00:00Z".to_string();
                    p.push_action.expires_at = "2026-07-28T11:00:00Z".to_string();
                }),
            ),
        ];
        for (name, mutate) in cases {
            let mut candidate = plan();
            mutate(&mut candidate);
            assert!(validate(&candidate).is_err(), "{name} drift was accepted");
        }
    }

    #[test]
    fn apply_creates_each_absent_resource_once_and_reads_it_back() {
        let candidate = plan();
        let mut transport = FakeTransport::absent(&candidate);
        let readback = apply_with_transport(&candidate, Path::new("."), &mut transport).unwrap();
        assert_eq!(readback.status, "applied");
        assert!(readback.fork_created);
        assert!(readback.branch_pushed);
        assert!(readback.draft_pr_created);
        assert_eq!(
            transport.calls,
            [
                "verify_local",
                "login",
                "read_repository",
                "read_branch",
                "read_fork",
                "create_fork",
                "read_fork",
                "read_branch",
                "push_branch",
                "read_branch",
                "find_drafts",
                "create_draft",
                "read_pull",
            ]
        );
    }

    #[test]
    fn exact_retry_is_read_only_and_idempotent() {
        let candidate = plan();
        let mut transport = FakeTransport::idempotent(&candidate);
        let readback = apply_with_transport(&candidate, Path::new("."), &mut transport).unwrap();
        assert_eq!(readback.status, "idempotent_readback");
        assert!(!readback.fork_created);
        assert!(!readback.branch_pushed);
        assert!(!readback.draft_pr_created);
        assert!(!transport
            .calls
            .iter()
            .any(|call| call.starts_with("create_")));
        assert!(!transport.calls.contains(&"push_branch"));
    }

    #[test]
    fn apply_rechecks_expiry_immediately_before_each_write() {
        let candidate = plan();
        let expired = DateTime::parse_from_rfc3339("2026-07-28T13:00:01Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut before_fork = FakeTransport::absent(&candidate);
        before_fork.clocks = VecDeque::from([expired]);
        assert!(apply_with_transport(&candidate, Path::new("."), &mut before_fork).is_err());
        assert!(!before_fork.calls.contains(&"create_fork"));

        let mut before_branch = FakeTransport::absent(&candidate);
        before_branch.clocks = VecDeque::from([fixed_now(), expired]);
        assert!(apply_with_transport(&candidate, Path::new("."), &mut before_branch).is_err());
        assert!(before_branch.calls.contains(&"create_fork"));
        assert!(!before_branch.calls.contains(&"push_branch"));

        let mut before_draft = FakeTransport::absent(&candidate);
        before_draft.clocks = VecDeque::from([fixed_now(), fixed_now(), expired]);
        assert!(apply_with_transport(&candidate, Path::new("."), &mut before_draft).is_err());
        assert!(before_draft.calls.contains(&"create_fork"));
        assert!(before_draft.calls.contains(&"push_branch"));
        assert!(!before_draft.calls.contains(&"create_draft"));
    }

    #[test]
    fn create_conflict_recovers_only_an_exact_single_draft() {
        let candidate = plan();
        let mut transport = FakeTransport::idempotent(&candidate);
        transport.draft_lookups = VecDeque::from([vec![], vec![pull_identity(&candidate)]]);
        transport.create_outcome = CreateDraftOutcome::Conflict;
        let readback = apply_with_transport(&candidate, Path::new("."), &mut transport).unwrap();
        assert_eq!(readback.status, "idempotent_readback");
        assert!(!readback.draft_pr_created);
        assert_eq!(
            transport
                .calls
                .iter()
                .filter(|call| **call == "create_draft")
                .count(),
            1
        );
    }

    #[test]
    fn apply_fails_closed_on_fork_branch_or_pull_identity_drift() {
        let candidate = plan();

        let mut fork_drift = FakeTransport::idempotent(&candidate);
        fork_drift.forks = VecDeque::from([Some(fork_identity("attacker/repair"))]);
        assert!(apply_with_transport(&candidate, Path::new("."), &mut fork_drift).is_err());
        assert!(!fork_drift.calls.contains(&"push_branch"));

        let mut branch_drift = FakeTransport::idempotent(&candidate);
        branch_drift.branches = VecDeque::from([
            Some(candidate.push_action.base_sha.clone()),
            Some("c".repeat(40)),
        ]);
        assert!(apply_with_transport(&candidate, Path::new("."), &mut branch_drift).is_err());
        assert!(!branch_drift.calls.contains(&"create_draft"));

        let mut pull_drift = FakeTransport::idempotent(&candidate);
        pull_drift.pull.draft = false;
        assert!(apply_with_transport(&candidate, Path::new("."), &mut pull_drift).is_err());
    }
}
