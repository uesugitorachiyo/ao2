use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: u64 = 65_536;
const MAX_RESPONSE_BYTES: usize = 262_144;
const MAX_RESPONSE_HEADERS: usize = 16_384;
const HTTP_EXCHANGE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CHANGED_FILES: usize = 100;
const MAX_TITLE_BYTES: usize = 256;
const MAX_BODY_BYTES: usize = 8_192;
const EVIDENCE_FOOTER_PREFIX: &str = "AO2-Evidence:";

type DigestFn = fn(&serde_json::Value) -> String;

#[derive(Debug, Subcommand)]
pub(crate) enum DraftPrCommand {
    /// Build a deterministic, digest-bound draft pull request action.
    Preview {
        #[arg(long)]
        evidence: PathBuf,
        #[arg(long = "support-bundle")]
        support_bundle: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Reproduce and verify a draft pull request action approval digest.
    Verify {
        #[arg(long)]
        action: PathBuf,
        #[arg(long = "expected-action-digest")]
        expected_action_digest: String,
        #[arg(long)]
        json: bool,
    },
    /// Exercise draft creation against a credential-free numeric-loopback fixture API.
    FixturePublish {
        #[arg(long)]
        action: PathBuf,
        #[arg(long = "expected-action-digest")]
        expected_action_digest: String,
        #[arg(long = "fixture-api")]
        fixture_api: String,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: DraftPrCommand, digest: DigestFn) -> Result<()> {
    match command {
        DraftPrCommand::Preview {
            evidence,
            support_bundle,
            out,
            json,
        } => preview(&evidence, support_bundle.as_deref(), &out, json, digest),
        DraftPrCommand::Verify {
            action,
            expected_action_digest,
            json,
        } => verify(&action, &expected_action_digest, json, digest),
        DraftPrCommand::FixturePublish {
            action,
            expected_action_digest,
            fixture_api,
            json,
        } => fixture_publish(&action, &expected_action_digest, &fixture_api, json, digest),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftEvidence {
    schema_version: String,
    issue: IssueIdentity,
    repository: RepositoryIdentity,
    repair: RepairEvidence,
    draft: DraftText,
    safety: SafetyBoundary,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct IssueIdentity {
    canonical_url: String,
    number: u64,
    snapshot_sha256: String,
    classification: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RepositoryIdentity {
    target: String,
    base_branch: String,
    base_commit: String,
    head_repository: String,
    head_branch: String,
    head_commit: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RepairEvidence {
    changed_files: Vec<String>,
    diff_sha256: String,
    evidence_pack_sha256: String,
    verification_sha256: String,
    status: String,
    provenance: RepairProvenance,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RepairProvenance {
    request_id: String,
    result_id: String,
    worker_source_commit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftText {
    title: String,
    body: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SafetyBoundary {
    prompt_injection_detected: bool,
    security_sensitive: bool,
    policy_blocked: bool,
    issue_write: bool,
    ready_for_review: bool,
    review_approval: bool,
    merge: bool,
    release: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DraftAction {
    schema_version: String,
    subject: DraftSubject,
    approval: DraftApproval,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DraftSubject {
    operation: String,
    issue: IssueIdentity,
    repository: RepositoryIdentity,
    repair: RepairEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    support_bundle: Option<SupportBundleSubjectBinding>,
    request: DraftRequest,
    safety: SafetyBoundary,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SupportBundleSubjectBinding {
    schema_version: String,
    bundle_sha256: String,
    problem_fingerprint: String,
    workflow_identity: String,
    failure_category: String,
    failed_phase: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DraftRequest {
    method: String,
    path: String,
    body: DraftRequestBody,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DraftRequestBody {
    title: String,
    body: String,
    head: String,
    base: String,
    draft: bool,
    preconditions: CommitPreconditions,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CommitPreconditions {
    base_commit: String,
    head_commit: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DraftApproval {
    algorithm: String,
    action_digest: String,
}

#[derive(Debug, Serialize)]
struct VerifyReadback {
    schema_version: &'static str,
    status: &'static str,
    action_digest: String,
    action_verified: bool,
    fixture_write_observed: bool,
    client_contact_scope: &'static str,
    fixture_exchange_attestation_status: &'static str,
    fixture_write_attestation_status: &'static str,
    fixture_claims_authenticated: bool,
    external_write_observability: &'static str,
    behavior_outside_client_observable_boundary: &'static str,
    client_issue_write_performed: bool,
    client_merge_performed: bool,
}

#[derive(Debug, Serialize)]
struct PublishReadback {
    schema_version: &'static str,
    status: &'static str,
    action_digest: String,
    pull_number: u64,
    post_performed: bool,
    fixture_write_observed: bool,
    client_contact_scope: &'static str,
    fixture_exchange_attestation_status: &'static str,
    fixture_write_attestation_status: &'static str,
    fixture_claims_authenticated: bool,
    external_write_observability: &'static str,
    behavior_outside_client_observable_boundary: &'static str,
    client_issue_write_performed: bool,
    client_ready_for_review_performed: bool,
    client_review_approval_performed: bool,
    client_merge_performed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExistingPull {
    number: u64,
    state: String,
    draft: bool,
    title: String,
    body: String,
    base: PullRef,
    head: PullHead,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PullRef {
    #[serde(rename = "ref")]
    reference: String,
    sha: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PullHead {
    #[serde(rename = "ref")]
    reference: String,
    sha: String,
    repo: PullRepository,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PullRepository {
    full_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureAttestation {
    schema_version: String,
    fixture_instance_id: String,
    claims_local_only: bool,
    claims_forwarding_capable: bool,
    claims_external_network_enabled: bool,
    fixture_exchange_attestation: FixtureExchangeAttestation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureExchangeAttestation {
    schema_version: String,
    fixture_instance_id: String,
    client_challenge: String,
    action_digest: String,
    request_body_sha256: String,
    repository: String,
    action_request_path: String,
    draft: bool,
    base_commit: String,
    head_commit: String,
    exchange_method: String,
    exchange_path: String,
    outcome: String,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pull_number: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureRef {
    schema_version: String,
    #[serde(rename = "ref")]
    reference: String,
    commit: String,
    fixture_exchange_attestation: FixtureExchangeAttestation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixturePulls {
    pulls: Vec<ExistingPull>,
    fixture_exchange_attestation: FixtureExchangeAttestation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureWriteAttestation {
    schema_version: String,
    fixture_instance_id: String,
    client_challenge: String,
    action_digest: String,
    request_body_sha256: String,
    repository: String,
    action_request_path: String,
    draft: bool,
    preconditions_enforced: bool,
    base_commit: String,
    head_commit: String,
    outcome: String,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pull_number: Option<u64>,
    claims_external_endpoint_contacted: bool,
    claims_forwarded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatedPull {
    pull: ExistingPull,
    fixture_exchange_attestation: FixtureExchangeAttestation,
    fixture_write_attestation: FixtureWriteAttestation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateConflict {
    schema_version: String,
    status: String,
    fixture_exchange_attestation: FixtureExchangeAttestation,
    fixture_write_attestation: FixtureWriteAttestation,
}

struct FixtureSession {
    endpoint: SocketAddr,
    client_challenge: String,
    action_digest: String,
    request_body_sha256: String,
    repository: String,
    action_request_path: String,
    draft: bool,
    base_commit: String,
    head_commit: String,
    fixture_instance_id: Option<String>,
}

fn deserialize_required_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn preview(
    path: &Path,
    support_bundle: Option<&Path>,
    out: &Path,
    json: bool,
    digest: DigestFn,
) -> Result<()> {
    let evidence: DraftEvidence = read_bounded_json(path)?;
    let support_binding = if let Some(bundle_path) = support_bundle {
        let binding = crate::support_bundle::validate_for_governed_issue(bundle_path, digest)?;
        if evidence.repair.evidence_pack_sha256 != binding.bundle_sha256 {
            bail!("draft evidence is not bound to the exact support bundle digest");
        }
        let subject_binding = SupportBundleSubjectBinding {
            schema_version: "ao2.github-draft-pr-support-binding.v0.1".to_string(),
            bundle_sha256: binding.bundle_sha256,
            problem_fingerprint: binding.problem_fingerprint,
            workflow_identity: binding.workflow_identity,
            failure_category: binding.failure_category,
            failed_phase: binding.failed_phase,
        };
        let (expected_title, expected_body) = support_draft_text(&subject_binding);
        if evidence.draft.title != expected_title || evidence.draft.body != expected_body {
            bail!("support bundle draft text must match the canonical privacy-safe template");
        }
        Some(subject_binding)
    } else {
        if contains_reserved_support_claim(&evidence.draft.body) {
            bail!(
                "draft body makes a reserved support-bundle claim without validated bundle input"
            );
        }
        None
    };
    let subject = subject_from_evidence(evidence, support_binding)?;
    let subject_value = serde_json::to_value(&subject)?;
    let action = DraftAction {
        schema_version: "ao2.github-draft-pr-action.v1".to_string(),
        subject,
        approval: DraftApproval {
            algorithm: "sha256-ao2-canonical-v1".to_string(),
            action_digest: digest(&subject_value),
        },
    };
    let bytes = serde_json::to_vec_pretty(&action)?;
    let output_len = bytes
        .len()
        .checked_add(1)
        .context("draft PR action length overflow")?;
    if output_len > MAX_INPUT_BYTES as usize {
        bail!("draft PR action exceeds the 65536-byte limit: {output_len} bytes");
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .with_context(|| format!("create new draft PR action {}", out.display()))?;
    output
        .write_all(&bytes)
        .with_context(|| format!("write draft PR action {}", out.display()))?;
    output
        .write_all(b"\n")
        .with_context(|| format!("write draft PR action {}", out.display()))?;
    emit(&action, json)
}

fn verify(path: &Path, expected: &str, json: bool, digest: DigestFn) -> Result<()> {
    let action = load_and_verify_action(path, expected, digest)?;
    let readback = VerifyReadback {
        schema_version: "ao2.github-draft-pr-verification.v1",
        status: "passed",
        action_digest: action.approval.action_digest,
        action_verified: true,
        fixture_write_observed: false,
        client_contact_scope: "none",
        fixture_exchange_attestation_status: "not_checked",
        fixture_write_attestation_status: "not_checked",
        fixture_claims_authenticated: false,
        external_write_observability: "not_observable_from_client",
        behavior_outside_client_observable_boundary: "not_claimed",
        client_issue_write_performed: false,
        client_merge_performed: false,
    };
    emit(&readback, json)
}

fn fixture_publish(
    path: &Path,
    expected: &str,
    fixture_api: &str,
    json: bool,
    digest: DigestFn,
) -> Result<()> {
    let action = load_and_verify_action(path, expected, digest)?;
    let endpoint = parse_fixture_endpoint(fixture_api)?;
    let mut session = FixtureSession::new(endpoint, &action)?;
    verify_fixture_attestation(&mut session)?;
    let repository = &action.subject.repository;
    let (owner, _) = split_repository(&repository.target)?;
    verify_fixture_ref(
        &session,
        &repository.target,
        &repository.base_branch,
        &repository.base_commit,
        "base",
    )?;
    verify_fixture_ref(
        &session,
        &repository.head_repository,
        &repository.head_branch,
        &repository.head_commit,
        "head",
    )?;
    let query_path = format!(
        "{}?state=all&head={}&base={}",
        action.subject.request.path,
        percent_encode(&format!("{owner}:{}", repository.head_branch)),
        percent_encode(&repository.base_branch)
    );
    let existing = discover_drafts(&session, &query_path)?;
    match existing.as_slice() {
        [] => create_draft(&session, action, &query_path, json),
        [candidate] if exact_pull_match(candidate, &action) => emit_publish_readback(
            "idempotent_readback",
            &action,
            candidate.number,
            false,
            false,
            json,
        ),
        [_] => bail!("fixture draft discovery failed closed: existing pull identity drift"),
        _ => bail!("fixture draft discovery failed closed: ambiguous existing pull matches"),
    }
}

impl FixtureSession {
    fn new(endpoint: SocketAddr, action: &DraftAction) -> Result<Self> {
        let request_body = serde_json::to_vec(&action.subject.request.body)?;
        let mut challenge = [0_u8; 32];
        OsRng.fill_bytes(&mut challenge);
        Ok(Self {
            endpoint,
            client_challenge: hex_bytes(&challenge),
            action_digest: action.approval.action_digest.clone(),
            request_body_sha256: sha256_bytes(&request_body),
            repository: action.subject.repository.target.clone(),
            action_request_path: action.subject.request.path.clone(),
            draft: action.subject.request.body.draft,
            base_commit: action.subject.repository.base_commit.clone(),
            head_commit: action.subject.repository.head_commit.clone(),
            fixture_instance_id: None,
        })
    }
}

fn create_draft(
    session: &FixtureSession,
    action: DraftAction,
    query_path: &str,
    json: bool,
) -> Result<()> {
    let body = serde_json::to_vec(&action.subject.request.body)?;
    let response = http_request(session, "POST", &action.subject.request.path, Some(&body))?;
    if response.status == 201 {
        let created: CreatedPull = serde_json::from_slice(&response.body)
            .context("fixture write attestation is missing, malformed, or has unknown fields")?;
        validate_exchange_attestation(
            &created.fixture_exchange_attestation,
            session,
            "POST",
            &action.subject.request.path,
            "created",
            Some(created.pull.number),
        )?;
        validate_write_attestation(
            &created.fixture_write_attestation,
            session,
            "created",
            Some(created.pull.number),
        )?;
        require_positive_pull_number(created.pull.number)?;
        if !exact_pull_match(&created.pull, &action) {
            bail!("fixture draft creation failed closed: response identity drift");
        }
        return emit_publish_readback(
            "fixture_reported_created",
            &action,
            created.pull.number,
            true,
            true,
            json,
        );
    }
    if matches!(response.status, 409 | 422) {
        let conflict: CreateConflict = serde_json::from_slice(&response.body)
            .context("fixture create conflict returned malformed or unknown-field JSON")?;
        if conflict.schema_version != "ao2.local-draft-pr-fixture-conflict.v1"
            || conflict.status != "conflict"
        {
            bail!("fixture create conflict response schema or status is invalid");
        }
        validate_exchange_attestation(
            &conflict.fixture_exchange_attestation,
            session,
            "POST",
            &action.subject.request.path,
            "conflict",
            None,
        )?;
        validate_write_attestation(
            &conflict.fixture_write_attestation,
            session,
            "conflict",
            None,
        )?;
        let existing = discover_drafts(session, query_path)?;
        return match existing.as_slice() {
            [candidate] if exact_pull_match(candidate, &action) => emit_publish_readback(
                "idempotent_readback_after_create_conflict",
                &action,
                candidate.number,
                true,
                true,
                json,
            ),
            [] => bail!("fixture create conflict re-read found no matching draft"),
            [_] => bail!("fixture create conflict re-read found identity drift"),
            _ => bail!("fixture create conflict re-read was ambiguous"),
        };
    }
    bail!(
        "fixture draft creation failed closed with HTTP {}",
        response.status
    )
}

fn emit_publish_readback(
    status: &'static str,
    action: &DraftAction,
    pull_number: u64,
    post_performed: bool,
    write_attestation_checked: bool,
    json: bool,
) -> Result<()> {
    let readback = PublishReadback {
        schema_version: "ao2.github-draft-pr-fixture-publish.v1",
        status,
        action_digest: action.approval.action_digest.clone(),
        pull_number,
        post_performed,
        fixture_write_observed: false,
        client_contact_scope: "numeric_loopback_only",
        fixture_exchange_attestation_status: "strict_challenge_bound_self_attestation",
        fixture_write_attestation_status: if write_attestation_checked {
            "strict_challenge_bound_self_attestation"
        } else {
            "not_applicable"
        },
        fixture_claims_authenticated: false,
        external_write_observability: "not_observable_from_client",
        behavior_outside_client_observable_boundary: "not_claimed",
        client_issue_write_performed: false,
        client_ready_for_review_performed: false,
        client_review_approval_performed: false,
        client_merge_performed: false,
    };
    emit(&readback, json)
}

fn verify_fixture_attestation(session: &mut FixtureSession) -> Result<()> {
    let path = "/ao2/fixture-attestation";
    let response = http_request(session, "GET", path, None)?;
    if response.status != 200 {
        bail!(
            "fixture attestation failed closed with HTTP {}",
            response.status
        );
    }
    let attestation: FixtureAttestation = serde_json::from_slice(&response.body)
        .context("fixture attestation is missing, malformed, or contains unknown fields")?;
    if attestation.schema_version != "ao2.local-draft-pr-fixture-attestation.v1" {
        bail!("fixture attestation schema_version is invalid");
    }
    validate_identifier("fixture_instance_id", &attestation.fixture_instance_id)?;
    if !attestation.claims_local_only {
        bail!("fixture self-attestation must claim local_only=true");
    }
    if attestation.claims_forwarding_capable {
        bail!("fixture self-attestation rejects claimed forwarding capability");
    }
    if attestation.claims_external_network_enabled {
        bail!("fixture self-attestation must claim external_network_enabled=false");
    }
    session.fixture_instance_id = Some(attestation.fixture_instance_id);
    validate_exchange_attestation(
        &attestation.fixture_exchange_attestation,
        session,
        "GET",
        path,
        "fixture_attestation",
        None,
    )
}

fn verify_fixture_ref(
    session: &FixtureSession,
    repository: &str,
    branch: &str,
    expected_commit: &str,
    role: &str,
) -> Result<()> {
    let (owner, repo) = split_repository(repository)?;
    let path = format!(
        "/repos/{owner}/{repo}/git/ref/heads/{}",
        percent_encode(branch)
    );
    let response = http_request(session, "GET", &path, None)?;
    if response.status != 200 {
        bail!(
            "fixture {role} ref read failed closed with HTTP {}",
            response.status
        );
    }
    let readback: FixtureRef = serde_json::from_slice(&response.body)
        .with_context(|| format!("fixture {role} ref response is malformed"))?;
    validate_exchange_attestation(
        &readback.fixture_exchange_attestation,
        session,
        "GET",
        &path,
        &format!("{role}_ref"),
        None,
    )?;
    if readback.schema_version != "ao2.local-draft-pr-fixture-ref.v1"
        || readback.reference != format!("refs/heads/{branch}")
    {
        bail!("fixture {role} ref identity drift");
    }
    if readback.commit != expected_commit {
        bail!("fixture {role} ref commit drift");
    }
    Ok(())
}

fn discover_drafts(session: &FixtureSession, query_path: &str) -> Result<Vec<ExistingPull>> {
    let response = http_request(session, "GET", query_path, None)?;
    if response.status != 200 {
        bail!(
            "fixture draft discovery failed closed with HTTP {}",
            response.status
        );
    }
    let readback: FixturePulls = serde_json::from_slice(&response.body)
        .context("fixture draft discovery returned malformed or unknown-field JSON")?;
    if readback.pulls.len() > 10 {
        bail!("fixture draft discovery failed closed: more than 10 candidates");
    }
    let pull_number = readback
        .pulls
        .as_slice()
        .first()
        .filter(|_| readback.pulls.len() == 1)
        .map(|pull| pull.number);
    validate_exchange_attestation(
        &readback.fixture_exchange_attestation,
        session,
        "GET",
        query_path,
        "pull_discovery",
        pull_number,
    )?;
    for pull in &readback.pulls {
        require_positive_pull_number(pull.number)?;
    }
    Ok(readback.pulls)
}

fn validate_write_attestation(
    attestation: &FixtureWriteAttestation,
    session: &FixtureSession,
    expected_outcome: &str,
    expected_pull_number: Option<u64>,
) -> Result<()> {
    if attestation.schema_version != "ao2.local-draft-pr-fixture-write-attestation.v1"
        || Some(attestation.fixture_instance_id.as_str()) != session.fixture_instance_id.as_deref()
        || attestation.client_challenge != session.client_challenge
        || attestation.action_digest != session.action_digest
        || attestation.request_body_sha256 != session.request_body_sha256
        || attestation.repository != session.repository
        || attestation.action_request_path != session.action_request_path
        || attestation.draft != session.draft
        || !attestation.preconditions_enforced
        || attestation.base_commit != session.base_commit
        || attestation.head_commit != session.head_commit
        || attestation.outcome != expected_outcome
        || attestation.pull_number != expected_pull_number
        || attestation.claims_external_endpoint_contacted
        || attestation.claims_forwarded
    {
        bail!("fixture write self-attestation binding mismatch");
    }
    Ok(())
}

fn validate_exchange_attestation(
    attestation: &FixtureExchangeAttestation,
    session: &FixtureSession,
    expected_method: &str,
    expected_path: &str,
    expected_outcome: &str,
    expected_pull_number: Option<u64>,
) -> Result<()> {
    if attestation.schema_version != "ao2.local-draft-pr-fixture-exchange-attestation.v1"
        || Some(attestation.fixture_instance_id.as_str()) != session.fixture_instance_id.as_deref()
        || attestation.client_challenge != session.client_challenge
        || attestation.action_digest != session.action_digest
        || attestation.request_body_sha256 != session.request_body_sha256
        || attestation.repository != session.repository
        || attestation.action_request_path != session.action_request_path
        || attestation.draft != session.draft
        || attestation.base_commit != session.base_commit
        || attestation.head_commit != session.head_commit
        || attestation.exchange_method != expected_method
        || attestation.exchange_path != expected_path
        || attestation.outcome != expected_outcome
        || attestation.pull_number != expected_pull_number
    {
        bail!("fixture exchange self-attestation binding mismatch");
    }
    Ok(())
}

fn subject_from_evidence(
    evidence: DraftEvidence,
    support_bundle: Option<SupportBundleSubjectBinding>,
) -> Result<DraftSubject> {
    if evidence.schema_version != "ao2.github-draft-pr-evidence.v1" {
        bail!("schema_version must be ao2.github-draft-pr-evidence.v1");
    }
    validate_issue(&evidence.issue, &evidence.repository)?;
    validate_repository(&evidence.repository)?;
    validate_repair(&evidence.repair)?;
    validate_safety(&evidence.safety)?;
    validate_text("title", &evidence.draft.title, MAX_TITLE_BYTES)?;
    validate_text("body", &evidence.draft.body, MAX_BODY_BYTES)?;
    let bound_body = bind_evidence_footer(&evidence.draft.body, &evidence.issue)?;
    let (owner, repo) = split_repository(&evidence.repository.target)?;
    Ok(DraftSubject {
        operation: "open_feature_generated_draft_pr".to_string(),
        issue: evidence.issue,
        request: DraftRequest {
            method: "POST".to_string(),
            path: format!("/repos/{owner}/{repo}/pulls"),
            body: DraftRequestBody {
                title: evidence.draft.title,
                body: bound_body,
                head: format!("{owner}:{}", evidence.repository.head_branch),
                base: evidence.repository.base_branch.clone(),
                draft: true,
                preconditions: CommitPreconditions {
                    base_commit: evidence.repository.base_commit.clone(),
                    head_commit: evidence.repository.head_commit.clone(),
                },
            },
        },
        repository: evidence.repository,
        repair: evidence.repair,
        support_bundle,
        safety: evidence.safety,
    })
}

fn support_draft_text(binding: &SupportBundleSubjectBinding) -> (String, String) {
    (
        format!(
            "AO2 troubleshooting: {} during {}",
            binding.failure_category, binding.failed_phase
        ),
        format!(
            "Sanitized AO2 troubleshooting bundle for {}.\n\nProblem fingerprint: {}\nBundle SHA-256: {}",
            binding.workflow_identity, binding.problem_fingerprint, binding.bundle_sha256
        ),
    )
}

fn contains_reserved_support_claim(body: &str) -> bool {
    let normalized = body.to_ascii_lowercase();
    normalized.contains("problem fingerprint:") || normalized.contains("bundle sha-256:")
}

fn load_and_verify_action(path: &Path, expected: &str, digest: DigestFn) -> Result<DraftAction> {
    if !is_sha256(expected) {
        bail!("expected action digest must be 64 lowercase hexadecimal characters");
    }
    let action: DraftAction = read_bounded_json(path)?;
    if action.schema_version != "ao2.github-draft-pr-action.v1" {
        bail!("action schema_version must be ao2.github-draft-pr-action.v1");
    }
    if action.approval.algorithm != "sha256-ao2-canonical-v1" {
        bail!("action digest algorithm must be sha256-ao2-canonical-v1");
    }
    validate_subject(&action.subject)?;
    let actual = digest(&serde_json::to_value(&action.subject)?);
    if action.approval.action_digest != actual {
        bail!("action digest does not match the canonical subject");
    }
    if expected != actual {
        bail!("expected action digest does not match the canonical subject");
    }
    Ok(action)
}

fn validate_subject(subject: &DraftSubject) -> Result<()> {
    if subject.operation != "open_feature_generated_draft_pr" {
        bail!("operation must be open_feature_generated_draft_pr");
    }
    validate_issue(&subject.issue, &subject.repository)?;
    validate_repository(&subject.repository)?;
    validate_repair(&subject.repair)?;
    validate_safety(&subject.safety)?;
    match &subject.support_bundle {
        Some(binding) => {
            if binding.schema_version != "ao2.github-draft-pr-support-binding.v0.1"
                || !is_sha256(&binding.bundle_sha256)
                || !binding
                    .problem_fingerprint
                    .strip_prefix("sha256:")
                    .is_some_and(is_sha256)
                || subject.repair.evidence_pack_sha256 != binding.bundle_sha256
            {
                bail!("support bundle subject binding is invalid");
            }
            crate::support_bundle::validate_governed_issue_metadata(
                &binding.workflow_identity,
                &binding.failure_category,
                &binding.failed_phase,
            )?;
            let (expected_title, expected_body) = support_draft_text(binding);
            let expected_bound_body = bind_evidence_footer(&expected_body, &subject.issue)?;
            if subject.request.body.title != expected_title
                || subject.request.body.body != expected_bound_body
            {
                bail!("support bundle draft request does not match its canonical binding");
            }
        }
        None if contains_reserved_support_claim(&subject.request.body.body) => {
            bail!("draft action makes a reserved support-bundle claim without typed binding");
        }
        None => {}
    }
    validate_evidence_footer(&subject.request.body.body, &subject.issue)?;
    let (owner, repo) = split_repository(&subject.repository.target)?;
    let expected_path = format!("/repos/{owner}/{repo}/pulls");
    if subject.request.method != "POST" || subject.request.path != expected_path {
        bail!("request must be the exact repository pull creation POST");
    }
    if !subject.request.body.draft {
        bail!("request body draft must be true");
    }
    if subject.request.body.base != subject.repository.base_branch
        || subject.request.body.head != format!("{owner}:{}", subject.repository.head_branch)
    {
        bail!("request body base/head identity does not match repository identity");
    }
    if subject.request.body.preconditions.base_commit != subject.repository.base_commit
        || subject.request.body.preconditions.head_commit != subject.repository.head_commit
    {
        bail!("request body commit preconditions do not match repository identity");
    }
    validate_text("title", &subject.request.body.title, MAX_TITLE_BYTES)?;
    validate_text("body", &subject.request.body.body, MAX_BODY_BYTES)?;
    Ok(())
}

fn validate_issue(issue: &IssueIdentity, repository: &RepositoryIdentity) -> Result<()> {
    if issue.number == 0 {
        bail!("issue number must be positive");
    }
    if issue.classification != "authentic_bug" {
        bail!("issue classification must be authentic_bug");
    }
    if !is_sha256(&issue.snapshot_sha256) {
        bail!("issue snapshot_sha256 must be 64 lowercase hexadecimal characters");
    }
    let (owner, repo) = split_repository(&repository.target)?;
    let expected = format!("https://github.com/{owner}/{repo}/issues/{}", issue.number);
    if issue.canonical_url != expected {
        bail!("issue canonical_url does not match repository and issue number");
    }
    Ok(())
}

fn validate_repository(repository: &RepositoryIdentity) -> Result<()> {
    split_repository(&repository.target)?;
    split_repository(&repository.head_repository)?;
    if repository.target != repository.head_repository {
        bail!("head_repository must exactly match target for the bounded fixture");
    }
    validate_ref("base_branch", &repository.base_branch)?;
    validate_ref("head_branch", &repository.head_branch)?;
    if !is_commit(&repository.base_commit) {
        bail!("base_commit must be 40 lowercase hexadecimal characters");
    }
    if !is_commit(&repository.head_commit) {
        bail!("head_commit must be 40 lowercase hexadecimal characters");
    }
    if repository.base_commit == repository.head_commit {
        bail!("base_commit and head_commit must differ");
    }
    Ok(())
}

fn validate_repair(repair: &RepairEvidence) -> Result<()> {
    if repair.status != "verified" {
        bail!("repair status must be verified");
    }
    for (name, value) in [
        ("diff_sha256", &repair.diff_sha256),
        ("evidence_pack_sha256", &repair.evidence_pack_sha256),
        ("verification_sha256", &repair.verification_sha256),
    ] {
        if !is_sha256(value) {
            bail!("{name} must be 64 lowercase hexadecimal characters");
        }
    }
    if repair.changed_files.is_empty() || repair.changed_files.len() > MAX_CHANGED_FILES {
        bail!("changed_files must contain 1 to {MAX_CHANGED_FILES} paths");
    }
    let mut previous: Option<&str> = None;
    for path in &repair.changed_files {
        validate_changed_path(path)?;
        if previous.is_some_and(|value| value >= path.as_str()) {
            bail!("changed_files must be sorted and unique");
        }
        previous = Some(path);
    }
    validate_identifier("request_id", &repair.provenance.request_id)?;
    validate_identifier("result_id", &repair.provenance.result_id)?;
    if !is_commit(&repair.provenance.worker_source_commit) {
        bail!("worker_source_commit must be 40 lowercase hexadecimal characters");
    }
    Ok(())
}

fn validate_safety(safety: &SafetyBoundary) -> Result<()> {
    for (name, unsafe_value) in [
        (
            "prompt_injection_detected",
            safety.prompt_injection_detected,
        ),
        ("security_sensitive", safety.security_sensitive),
        ("policy_blocked", safety.policy_blocked),
        ("issue_write", safety.issue_write),
        ("ready_for_review", safety.ready_for_review),
        ("review_approval", safety.review_approval),
        ("merge", safety.merge),
        ("release", safety.release),
    ] {
        if unsafe_value {
            bail!("safety boundary {name} must be false");
        }
    }
    Ok(())
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

fn validate_ref(name: &str, value: &str) -> Result<()> {
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
        bail!("{name} is not a bounded Git reference");
    }
    Ok(())
}

fn validate_changed_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 1024
        || value.contains('\\')
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        bail!("changed_files contains an unsafe path");
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || value.split('/').any(|part| part == ".git")
    {
        bail!("changed_files contains traversal or a control path");
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
        bail!("{name} must be a non-empty bounded identifier");
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > max
        || value.bytes().any(|byte| byte == 0 || byte == 0x7f)
    {
        bail!("{name} must be non-empty and at most {max} bytes without control delimiters");
    }
    Ok(())
}

fn bind_evidence_footer(body: &str, issue: &IssueIdentity) -> Result<String> {
    if body.contains(EVIDENCE_FOOTER_PREFIX) {
        bail!("draft body must not supply its own AO2 evidence footer");
    }
    let bound = format!("{body}\n\n{}", evidence_footer(issue));
    validate_text("body with evidence footer", &bound, MAX_BODY_BYTES)?;
    Ok(bound)
}

fn validate_evidence_footer(body: &str, issue: &IssueIdentity) -> Result<()> {
    let suffix = format!("\n\n{}", evidence_footer(issue));
    let Some(unbound_body) = body.strip_suffix(&suffix) else {
        bail!("request body evidence footer is missing or does not match issue identity");
    };
    if unbound_body.trim().is_empty() || unbound_body.contains(EVIDENCE_FOOTER_PREFIX) {
        bail!("request body evidence footer is ambiguous");
    }
    Ok(())
}

fn evidence_footer(issue: &IssueIdentity) -> String {
    format!(
        "{EVIDENCE_FOOTER_PREFIX} issue_url={} snapshot_sha256={}",
        issue.canonical_url, issue.snapshot_sha256
    )
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub(crate) fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let mut file = open_bounded_input(path)?;
    let metadata = validate_opened_input(&file, path)?;
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(MAX_INPUT_BYTES as usize));
    std::io::Read::by_ref(&mut file)
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {}", path.display()))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        bail!(
            "input exceeds the 65536-byte limit after read: {} bytes",
            bytes.len()
        );
    }
    validate_opened_input(&file, path)?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse strict JSON from {}", path.display()))
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
            "open regular file input without following links {}",
            path.display()
        )
    })
}

fn validate_opened_input(file: &fs::File, path: &Path) -> Result<fs::Metadata> {
    #[cfg(windows)]
    crate::windows_input::validate_disk_handle(file, path)?;

    let metadata = file
        .metadata()
        .with_context(|| format!("inspect opened input {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!("input must be a regular file: {}", path.display());
    }
    if metadata.len() > MAX_INPUT_BYTES {
        bail!(
            "input exceeds the 65536-byte limit: {} bytes",
            metadata.len()
        );
    }
    Ok(metadata)
}

fn parse_fixture_endpoint(value: &str) -> Result<SocketAddr> {
    let rest = value
        .strip_prefix("http://")
        .context("fixture API must use http:// with a numeric loopback address")?;
    if rest.contains(['/', '?', '#', '@']) {
        bail!("fixture API must contain no path, query, fragment, or credentials");
    }
    let endpoint: SocketAddr = rest
        .parse()
        .context("fixture API must use a numeric loopback address and explicit port")?;
    if !endpoint.ip().is_loopback() || endpoint.port() == 0 {
        bail!("fixture API must use a numeric loopback address and nonzero port");
    }
    Ok(endpoint)
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn http_request(
    session: &FixtureSession,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<HttpResponse> {
    if !matches!(method, "GET" | "POST")
        || !(path == "/ao2/fixture-attestation" || path.starts_with("/repos/"))
    {
        bail!("fixture HTTP request escaped the bounded endpoint allowlist");
    }
    let deadline = Instant::now() + HTTP_EXCHANGE_TIMEOUT;
    let mut stream =
        TcpStream::connect_timeout(&session.endpoint, remaining_exchange_time(deadline)?)
            .map_err(|error| exchange_io_error("connect", error))?;
    stream.set_read_timeout(Some(remaining_exchange_time(deadline)?))?;
    stream.set_write_timeout(Some(remaining_exchange_time(deadline)?))?;
    let host = match session.endpoint.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", session.endpoint.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", session.endpoint.port()),
    };
    let payload = body.unwrap_or_default();
    let content_headers = if body.is_some() {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        )
    } else {
        String::new()
    };
    let fixture_instance_header = session
        .fixture_instance_id
        .as_ref()
        .map(|value| format!("X-AO2-Fixture-Instance-Id: {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\n\
         X-AO2-Client-Challenge: {}\r\n\
         X-AO2-Action-Digest: {}\r\n\
         X-AO2-Request-Body-SHA256: {}\r\n\
         X-AO2-Repository: {}\r\n\
         X-AO2-Action-Request-Path: {}\r\n\
         X-AO2-Draft: {}\r\n\
         X-AO2-Base-Commit: {}\r\n\
         X-AO2-Head-Commit: {}\r\n\
         {fixture_instance_header}{content_headers}Connection: close\r\n\r\n",
        session.client_challenge,
        session.action_digest,
        session.request_body_sha256,
        session.repository,
        session.action_request_path,
        session.draft,
        session.base_commit,
        session.head_commit,
    );
    stream.set_write_timeout(Some(remaining_exchange_time(deadline)?))?;
    stream
        .write_all(request.as_bytes())
        .map_err(|error| exchange_io_error("write request headers", error))?;
    if body.is_some() {
        stream.set_write_timeout(Some(remaining_exchange_time(deadline)?))?;
        stream
            .write_all(payload)
            .map_err(|error| exchange_io_error("write request body", error))?;
    }
    stream.set_write_timeout(Some(remaining_exchange_time(deadline)?))?;
    stream
        .flush()
        .map_err(|error| exchange_io_error("flush request", error))?;

    let mut response = Vec::new();
    loop {
        let mut buffer = [0_u8; 8192];
        stream.set_read_timeout(Some(remaining_exchange_time(deadline)?))?;
        let count = stream
            .read(&mut buffer)
            .map_err(|error| exchange_io_error("read response", error))?;
        if count == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..count]);
        if response.len() > MAX_RESPONSE_BYTES + MAX_RESPONSE_HEADERS {
            bail!("fixture API response exceeds the 262144-byte body limit");
        }
    }
    parse_http_response(&response)
}

fn remaining_exchange_time(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .context("fixture API total deadline exceeded")
}

fn exchange_io_error(operation: &str, error: std::io::Error) -> anyhow::Error {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        anyhow::anyhow!("fixture API total deadline exceeded during {operation}")
    } else {
        anyhow::Error::new(error).context(format!("fixture API {operation}"))
    }
}

fn parse_http_response(response: &[u8]) -> Result<HttpResponse> {
    let boundary = response
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .context("fixture API response has no header boundary")?;
    if boundary > MAX_RESPONSE_HEADERS {
        bail!("fixture API response headers exceed the bounded limit");
    }
    let headers = std::str::from_utf8(&response[..boundary])
        .context("fixture API response headers are not UTF-8")?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .context("fixture API response has malformed status")?;
    if (300..400).contains(&status) {
        bail!("fixture API redirect responses are forbidden");
    }
    if headers
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("transfer-encoding:"))
    {
        bail!("fixture API transfer encoding is unsupported");
    }
    let body = response[boundary + 4..].to_vec();
    if body.len() > MAX_RESPONSE_BYTES {
        bail!("fixture API response exceeds the 262144-byte body limit");
    }
    if let Some(length) = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    }) {
        if length != body.len() {
            bail!("fixture API response Content-Length mismatch");
        }
    } else {
        bail!("fixture API response requires Content-Length");
    }
    Ok(HttpResponse { status, body })
}

fn exact_pull_match(candidate: &ExistingPull, action: &DraftAction) -> bool {
    let repository = &action.subject.repository;
    let request = &action.subject.request.body;
    candidate.number > 0
        && candidate.state == "open"
        && candidate.draft
        && candidate.title == request.title
        && candidate.body == request.body
        && candidate.base.reference == repository.base_branch
        && candidate.base.sha == repository.base_commit
        && candidate.head.reference == repository.head_branch
        && candidate.head.sha == repository.head_commit
        && candidate.head.repo.full_name == repository.head_repository
}

fn require_positive_pull_number(pull_number: u64) -> Result<()> {
    if pull_number == 0 {
        bail!("fixture pull number must be positive");
    }
    Ok(())
}

fn emit<T: Serialize>(value: &T, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
}
