use crate::cli::ContributionPacketCommand;
use crate::github_issue_intake::github_issue_repair_pack::{
    is_lower_hex, read_guarded_file, validate_digest, validate_identifier, validate_repository,
    verify_guarded_artifact, RootGuard,
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const PACKET_MAX_BYTES: u64 = 65_536;
const EVIDENCE_MAX_BYTES: u64 = 524_288;
const PATCH_MAX_BYTES: u64 = 4_194_304;
const FRESH_DAYS: i64 = 7;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    schema_version: String,
    packet_id: String,
    repository: String,
    issue_number: u64,
    source_sha: String,
    issue_snapshot_sha256: String,
    reproduction_evidence: Artifact,
    patch: Artifact,
    tests: Artifact,
    policy: Artifact,
    authorship: Authorship,
    limitations: Vec<String>,
    governance_state: GovernanceState,
    source_current: bool,
    issue_current: bool,
    maintainer_feedback: Option<Artifact>,
    created_at: String,
    safety: Safety,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorship {
    identity: String,
    attestation: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceState {
    ReviewReady,
    Denied,
    Pending,
    RevisionRequested,
    Rejected,
    Cancelled,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeedbackEvidence {
    repository: String,
    issue_number: u64,
    source_sha: String,
    received_at: String,
    technical_state_changed: bool,
    mutation_authority_granted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Safety {
    network_accessed: bool,
    credentials_present: bool,
    provider_called: bool,
    upstream_mutated: bool,
    operator_fork_mutated: bool,
    publication_attempted: bool,
    mutation_authorized: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReproductionEvidence {
    repository: String,
    issue_number: u64,
    source_sha: String,
    result: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestEvidence {
    repository: String,
    issue_number: u64,
    source_sha: String,
    defining: String,
    neighboring: String,
    full: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyEvidence {
    repository: String,
    license: String,
    contribution_policy: String,
}

#[derive(Debug, Serialize)]
struct Readback<'a> {
    schema_version: &'static str,
    result: &'static str,
    packet_id: &'a str,
    repository: &'a str,
    issue_number: u64,
    source_sha: &'a str,
    packet_sha256: String,
    governance_state: GovernanceState,
    contribution_ready: bool,
    technical_state_changed: bool,
    mutation_authorized: bool,
    network_accessed: bool,
    credentials_present: bool,
    provider_called: bool,
    upstream_mutated: bool,
    operator_fork_mutated: bool,
    publication_attempted: bool,
    executes_work: bool,
    approves_work: bool,
    publishes: bool,
}

pub(crate) fn run(command: ContributionPacketCommand) -> Result<()> {
    match command {
        ContributionPacketCommand::Verify { packet, root, json } => verify(&packet, &root, json),
    }
}

fn verify(packet_path: &Path, root_path: &Path, json: bool) -> Result<()> {
    let root_canonical = fs::canonicalize(root_path).with_context(|| {
        format!(
            "canonicalize contribution packet root {}",
            root_path.display()
        )
    })?;
    let parent = packet_path
        .parent()
        .context("contribution packet path has no parent")?;
    if fs::canonicalize(parent)? != root_canonical {
        bail!("contribution packet must be a direct child of root");
    }
    let packet_name = packet_path
        .file_name()
        .and_then(|value| value.to_str())
        .context("contribution packet filename must be UTF-8")?;
    let root = RootGuard::open(root_path)?;
    let packet_bytes = read_guarded_file(&root, packet_name, PACKET_MAX_BYTES, "packet")?;
    let packet: Packet =
        serde_json::from_slice(&packet_bytes).context("parse strict contribution packet JSON")?;
    validate_packet(&packet)?;

    for (label, artifact, limit) in [
        (
            "reproduction_evidence",
            &packet.reproduction_evidence,
            EVIDENCE_MAX_BYTES,
        ),
        ("patch", &packet.patch, PATCH_MAX_BYTES),
        ("tests", &packet.tests, EVIDENCE_MAX_BYTES),
        ("policy", &packet.policy, EVIDENCE_MAX_BYTES),
    ] {
        verify_guarded_artifact(
            &root,
            &artifact.path,
            artifact.size_bytes,
            &artifact.sha256,
            limit,
            label,
        )?;
    }
    if let Some(feedback) = &packet.maintainer_feedback {
        verify_guarded_artifact(
            &root,
            &feedback.path,
            feedback.size_bytes,
            &feedback.sha256,
            EVIDENCE_MAX_BYTES,
            "maintainer_feedback",
        )?;
    }

    let reproduction: ReproductionEvidence = parse_artifact(
        &root,
        &packet.reproduction_evidence,
        "reproduction_evidence",
    )?;
    if reproduction.repository != packet.repository
        || reproduction.issue_number != packet.issue_number
        || reproduction.source_sha != packet.source_sha
        || reproduction.result != "reproduced_failure"
    {
        bail!("reproduction evidence identity or result does not match packet");
    }
    let tests: TestEvidence = parse_artifact(&root, &packet.tests, "tests")?;
    if tests.repository != packet.repository
        || tests.issue_number != packet.issue_number
        || tests.source_sha != packet.source_sha
        || [tests.defining, tests.neighboring, tests.full]
            .iter()
            .any(|value| value != "passed")
    {
        bail!("test evidence identity or passing status does not match packet");
    }
    let policy: PolicyEvidence = parse_artifact(&root, &packet.policy, "policy")?;
    if policy.repository != packet.repository
        || policy.license.trim().is_empty()
        || policy.contribution_policy != "accepted"
    {
        bail!("policy evidence does not permit a contribution packet");
    }

    let feedback = packet
        .maintainer_feedback
        .as_ref()
        .map(|artifact| parse_artifact::<FeedbackEvidence>(&root, artifact, "maintainer_feedback"))
        .transpose()?;
    if let Some(feedback) = &feedback {
        if feedback.repository != packet.repository
            || feedback.issue_number != packet.issue_number
            || feedback.source_sha != packet.source_sha
            || feedback.mutation_authority_granted
        {
            bail!("maintainer feedback identity or authority is invalid");
        }
        let received_at = parse_time("maintainer feedback received_at", &feedback.received_at)?;
        let created_at = parse_time("created_at", &packet.created_at)?;
        if received_at < created_at || received_at > Utc::now() + Duration::minutes(5) {
            bail!("maintainer feedback timestamp is outside the packet lifecycle");
        }
    }
    let technical_state_changed = feedback
        .as_ref()
        .is_some_and(|feedback| feedback.technical_state_changed);
    let readback = Readback {
        schema_version: "ao2.github-issue-contribution-packet-validation.v1",
        result: "packet_valid",
        packet_id: &packet.packet_id,
        repository: &packet.repository,
        issue_number: packet.issue_number,
        source_sha: &packet.source_sha,
        packet_sha256: format!("sha256:{:x}", Sha256::digest(&packet_bytes)),
        governance_state: packet.governance_state,
        contribution_ready: matches!(packet.governance_state, GovernanceState::ReviewReady)
            && !technical_state_changed,
        technical_state_changed,
        mutation_authorized: false,
        network_accessed: false,
        credentials_present: false,
        provider_called: false,
        upstream_mutated: false,
        operator_fork_mutated: false,
        publication_attempted: false,
        executes_work: false,
        approves_work: false,
        publishes: false,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&readback)?);
    } else {
        println!("packet_valid {}", packet.packet_id);
    }
    Ok(())
}

fn validate_packet(packet: &Packet) -> Result<()> {
    if packet.schema_version != "ao2.github-issue-contribution-packet.v1" {
        bail!("contribution packet schema_version is invalid");
    }
    validate_identifier("packet_id", &packet.packet_id, 128)?;
    validate_repository(&packet.repository)?;
    if packet.issue_number == 0 || !is_lower_hex(&packet.source_sha, 40) {
        bail!("contribution packet issue or source identity is invalid");
    }
    validate_digest("issue_snapshot_sha256", &packet.issue_snapshot_sha256)?;
    if !packet.source_current || !packet.issue_current {
        bail!("contribution packet source and issue must remain current");
    }
    if packet.authorship.identity != "human:local-operator"
        || packet.authorship.attestation != "authored_from_sealed_local_repair"
    {
        bail!("contribution packet authorship is invalid");
    }
    if packet.limitations.is_empty()
        || packet.limitations.len() > 16
        || packet
            .limitations
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 512)
    {
        bail!("contribution packet limitations are invalid");
    }
    let created_at = parse_time("created_at", &packet.created_at)?;
    let now = Utc::now();
    if created_at < now - Duration::days(FRESH_DAYS) || created_at > now + Duration::minutes(5) {
        bail!("contribution packet is stale or future-dated");
    }
    if packet.safety.network_accessed
        || packet.safety.credentials_present
        || packet.safety.provider_called
        || packet.safety.upstream_mutated
        || packet.safety.operator_fork_mutated
        || packet.safety.publication_attempted
        || packet.safety.mutation_authorized
    {
        bail!("contribution packet safety boundary is unsafe");
    }
    Ok(())
}

fn parse_artifact<T: for<'de> Deserialize<'de>>(
    root: &RootGuard,
    artifact: &Artifact,
    label: &str,
) -> Result<T> {
    let bytes = read_guarded_file(root, &artifact.path, EVIDENCE_MAX_BYTES, label)?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse strict {label} JSON"))
}

fn parse_time(label: &str, value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("parse {label}"))?
        .with_timezone(&Utc))
}
