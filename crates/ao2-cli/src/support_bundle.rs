use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 65_536;
const MAX_LOGS: usize = 16;
const MAX_LOG_BYTES: usize = 2_048;
const ALLOWED_WORKFLOWS: &[&str] = &[
    "risky-pr-v1",
    "public-release-build-v1",
    "release-gate-v1",
    "pulse-v1",
];
const ALLOWED_VERIFIERS: &[&str] = &[
    "ao2-report-verifier-v1",
    "ao2-release-verifier-v1",
    "ao2-evidence-verifier-v1",
];
const ALLOWED_FAILURE_CATEGORIES: &[&str] = &[
    "approval_blocked",
    "evidence_missing",
    "manifest_mismatch",
    "release_mismatch",
    "replay_failed",
    "verification_failed",
    "worker_stale",
];
const ALLOWED_FAILURE_PHASES: &[&str] = &[
    "approval",
    "evidence",
    "execution",
    "intake",
    "planning",
    "recovery",
    "release",
    "replay",
    "report_contract",
    "verification",
];
const ALLOWED_NEXT_ACTIONS: &[&str] = &[
    "Inspect the exact approval action digest before retrying.",
    "Refresh the worker heartbeat before trusting running state.",
    "Re-run ao2 report verify against the retained evidence pack.",
    "Restore the retained evidence pack before retrying.",
    "Retry replay from the last verified checkpoint.",
    "Stop and request bounded operator authority for the exact action.",
];

type DigestFn = fn(&serde_json::Value) -> String;

#[derive(Debug, Subcommand)]
pub(crate) enum SupportCommand {
    /// Build a bounded, privacy-safe troubleshooting support bundle.
    Bundle {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: SupportCommand, digest: DigestFn) -> Result<()> {
    match command {
        SupportCommand::Bundle { input, out, json } => build(&input, &out, json, digest),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportSource {
    schema_version: String,
    ao2_version: String,
    control_plane_version: String,
    platform: Platform,
    workflow: WorkflowIdentity,
    approval: ApprovalState,
    replay: StatusState,
    evidence: EvidenceState,
    manifest_sha256: String,
    release_sha256: String,
    failure: FailureIdentity,
    logs: Vec<String>,
    smallest_safe_next_action: String,
    safety: SafetyBoundary,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Platform {
    os: String,
    architecture: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkflowIdentity {
    identity: String,
    verifier_identity: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApprovalState {
    status: String,
    action_digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StatusState {
    status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EvidenceState {
    status: String,
    digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FailureIdentity {
    category: String,
    phase: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SafetyBoundary {
    executes_work: bool,
    calls_providers: bool,
    issue_write_performed: bool,
    public_write_performed: bool,
    release_or_deployment_performed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SupportBundle {
    schema_version: String,
    ao2_version: String,
    control_plane_version: String,
    platform: Platform,
    workflow: WorkflowIdentity,
    approval: ApprovalState,
    replay: StatusState,
    evidence: EvidenceState,
    manifest_sha256: String,
    release_sha256: String,
    failure: FailureIdentity,
    logs: Vec<SanitizedLog>,
    redaction: RedactionSummary,
    problem_fingerprint: String,
    bundle_sha256: String,
    smallest_safe_next_action: String,
    governed_issue_route: GovernedIssueRoute,
    observer_only: bool,
    safe_to_execute: bool,
    executes_work: bool,
    calls_providers: bool,
    issue_write_performed: bool,
    public_write_performed: bool,
    release_or_deployment_performed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SanitizedLog {
    sequence: usize,
    text: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RedactionSummary {
    input_log_entries: usize,
    fully_redacted_log_entries: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GovernedIssueRoute {
    input_trust: String,
    evidence_kind: String,
    problem_fingerprint: String,
    issue_write_performed: bool,
    public_write_performed: bool,
}

fn build(input: &Path, out: &Path, print_json: bool, digest: DigestFn) -> Result<()> {
    let mut source: SupportSource = read_bounded_json(input)?;
    normalize_source(&mut source);
    validate_source(&source)?;

    let fingerprint_subject = fingerprint_subject(
        &source.ao2_version,
        &source.control_plane_version,
        &source.platform,
        &source.workflow,
        &source.approval,
        &source.replay,
        &source.evidence,
        &source.manifest_sha256,
        &source.release_sha256,
        &source.failure,
        &source.smallest_safe_next_action,
    );
    let problem_fingerprint = format!("sha256:{}", digest(&fingerprint_subject));
    let (logs, redaction) = sanitize_logs(&source.logs)?;
    let mut bundle = SupportBundle {
        schema_version: "ao2.troubleshooting-support-bundle.v0.1".to_string(),
        ao2_version: source.ao2_version,
        control_plane_version: source.control_plane_version,
        platform: source.platform,
        workflow: source.workflow,
        approval: source.approval,
        replay: source.replay,
        evidence: source.evidence,
        manifest_sha256: source.manifest_sha256,
        release_sha256: source.release_sha256,
        failure: source.failure,
        logs,
        redaction,
        problem_fingerprint: problem_fingerprint.clone(),
        bundle_sha256: String::new(),
        smallest_safe_next_action: source.smallest_safe_next_action,
        governed_issue_route: GovernedIssueRoute {
            input_trust: "sanitized_untrusted".to_string(),
            evidence_kind: "troubleshooting_support_bundle".to_string(),
            problem_fingerprint,
            issue_write_performed: false,
            public_write_performed: false,
        },
        observer_only: true,
        safe_to_execute: false,
        executes_work: false,
        calls_providers: false,
        issue_write_performed: false,
        public_write_performed: false,
        release_or_deployment_performed: false,
    };
    bundle.bundle_sha256 = canonical_bundle_sha256(&bundle, digest)?;
    let rendered = format!("{}\n", serde_json::to_string_pretty(&bundle)?);
    if rendered.len() as u64 > MAX_BYTES {
        bail!(
            "support bundle exceeds the 65536-byte limit: {} bytes",
            rendered.len()
        );
    }
    write_exclusive(out, rendered.as_bytes())?;
    if print_json {
        print!("{rendered}");
    } else {
        println!("support_bundle={}", out.display());
        println!("problem_fingerprint={}", bundle.problem_fingerprint);
    }
    Ok(())
}

pub(crate) struct GovernedIssueBinding {
    pub(crate) problem_fingerprint: String,
    pub(crate) bundle_sha256: String,
    pub(crate) workflow_identity: String,
    pub(crate) failure_category: String,
    pub(crate) failed_phase: String,
}

pub(crate) fn validate_for_governed_issue(
    path: &Path,
    digest: DigestFn,
) -> Result<GovernedIssueBinding> {
    let bundle: SupportBundle = read_bounded_json(path)?;
    validate_bundle(&bundle, digest)?;
    Ok(GovernedIssueBinding {
        problem_fingerprint: bundle.problem_fingerprint,
        bundle_sha256: bundle.bundle_sha256,
        workflow_identity: bundle.workflow.identity,
        failure_category: bundle.failure.category,
        failed_phase: bundle.failure.phase,
    })
}

pub(crate) fn validate_governed_issue_metadata(
    workflow_identity: &str,
    failure_category: &str,
    failed_phase: &str,
) -> Result<()> {
    validate_allowed(
        "support_bundle.workflow_identity",
        workflow_identity,
        ALLOWED_WORKFLOWS,
    )?;
    validate_allowed(
        "support_bundle.failure_category",
        failure_category,
        ALLOWED_FAILURE_CATEGORIES,
    )?;
    validate_allowed(
        "support_bundle.failed_phase",
        failed_phase,
        ALLOWED_FAILURE_PHASES,
    )
}

fn validate_bundle(bundle: &SupportBundle, digest: DigestFn) -> Result<()> {
    if bundle.schema_version != "ao2.troubleshooting-support-bundle.v0.1" {
        bail!("unsupported troubleshooting support bundle schema");
    }
    validate_product_metadata(
        &bundle.ao2_version,
        &bundle.control_plane_version,
        &bundle.platform,
        &bundle.workflow,
        &bundle.failure,
        &bundle.smallest_safe_next_action,
    )?;
    validate_approval(&bundle.approval)?;
    validate_status(
        "replay.status",
        &bundle.replay.status,
        &["not_attempted", "accepted", "failed", "blocked"],
    )?;
    validate_evidence(&bundle.evidence)?;
    validate_digest("manifest_sha256", &bundle.manifest_sha256)?;
    validate_digest("release_sha256", &bundle.release_sha256)?;
    validate_bundle_normalization(bundle)?;
    if bundle.logs.len() > MAX_LOGS {
        bail!("support bundle logs must contain at most 16 entries");
    }
    if bundle.redaction.input_log_entries != bundle.logs.len()
        || bundle.redaction.fully_redacted_log_entries != bundle.logs.len()
    {
        bail!("support bundle redaction summary does not match its log entries");
    }
    for (index, log) in bundle.logs.iter().enumerate() {
        if log.sequence != index + 1 {
            bail!("support bundle log sequence is not contiguous");
        }
        if log.text != "[REDACTED_LOG]" {
            bail!("support bundle contains a non-redacted diagnostic entry");
        }
    }
    if bundle.governed_issue_route.input_trust != "sanitized_untrusted"
        || bundle.governed_issue_route.evidence_kind != "troubleshooting_support_bundle"
        || bundle.governed_issue_route.problem_fingerprint != bundle.problem_fingerprint
        || bundle.governed_issue_route.issue_write_performed
        || bundle.governed_issue_route.public_write_performed
        || !bundle.observer_only
        || bundle.safe_to_execute
        || bundle.executes_work
        || bundle.calls_providers
        || bundle.issue_write_performed
        || bundle.public_write_performed
        || bundle.release_or_deployment_performed
    {
        bail!("support bundle violates the governed observer-only boundary");
    }
    let expected = format!(
        "sha256:{}",
        digest(&fingerprint_subject(
            &bundle.ao2_version,
            &bundle.control_plane_version,
            &bundle.platform,
            &bundle.workflow,
            &bundle.approval,
            &bundle.replay,
            &bundle.evidence,
            &bundle.manifest_sha256,
            &bundle.release_sha256,
            &bundle.failure,
            &bundle.smallest_safe_next_action,
        ))
    );
    if bundle.problem_fingerprint != expected {
        bail!("support bundle problem fingerprint does not match its canonical fields");
    }
    if bundle.bundle_sha256 != canonical_bundle_sha256(bundle, digest)? {
        bail!("support bundle digest does not match its canonical contents");
    }
    Ok(())
}

fn validate_bundle_normalization(bundle: &SupportBundle) -> Result<()> {
    if bundle.ao2_version != normalize_text(&bundle.ao2_version)
        || bundle.control_plane_version != normalize_text(&bundle.control_plane_version)
        || bundle.platform.os != bundle.platform.os.trim().to_ascii_lowercase()
        || bundle.platform.architecture != bundle.platform.architecture.trim().to_ascii_lowercase()
        || bundle.workflow.identity != normalize_text(&bundle.workflow.identity)
        || bundle.workflow.verifier_identity != normalize_text(&bundle.workflow.verifier_identity)
        || bundle.failure.category != normalize_text(&bundle.failure.category).to_ascii_lowercase()
        || bundle.failure.phase != normalize_text(&bundle.failure.phase).to_ascii_lowercase()
        || bundle.smallest_safe_next_action != normalize_text(&bundle.smallest_safe_next_action)
    {
        bail!("support bundle fields are not canonically normalized");
    }
    Ok(())
}

fn canonical_bundle_sha256(bundle: &SupportBundle, digest: DigestFn) -> Result<String> {
    let mut subject = serde_json::to_value(bundle)?;
    subject
        .as_object_mut()
        .context("support bundle must serialize as an object")?
        .remove("bundle_sha256");
    Ok(digest(&subject))
}

fn validate_source(source: &SupportSource) -> Result<()> {
    if source.schema_version != "ao2.troubleshooting-support-source.v0.1" {
        bail!("unsupported troubleshooting support source schema");
    }
    validate_product_metadata(
        &source.ao2_version,
        &source.control_plane_version,
        &source.platform,
        &source.workflow,
        &source.failure,
        &source.smallest_safe_next_action,
    )?;
    validate_approval(&source.approval)?;
    validate_status(
        "replay.status",
        &source.replay.status,
        &["not_attempted", "accepted", "failed", "blocked"],
    )?;
    validate_evidence(&source.evidence)?;
    validate_digest("manifest_sha256", &source.manifest_sha256)?;
    validate_digest("release_sha256", &source.release_sha256)?;
    if source.logs.len() > MAX_LOGS {
        bail!("logs must contain at most 16 entries");
    }
    if source.safety.executes_work
        || source.safety.calls_providers
        || source.safety.issue_write_performed
        || source.safety.public_write_performed
        || source.safety.release_or_deployment_performed
    {
        bail!("troubleshooting source violates the observer-only safety boundary");
    }
    Ok(())
}

fn normalize_source(source: &mut SupportSource) {
    source.ao2_version = normalize_text(&source.ao2_version);
    source.control_plane_version = normalize_text(&source.control_plane_version);
    source.platform.os = source.platform.os.trim().to_ascii_lowercase();
    source.platform.architecture = source.platform.architecture.trim().to_ascii_lowercase();
    source.workflow.identity = normalize_text(&source.workflow.identity);
    source.workflow.verifier_identity = normalize_text(&source.workflow.verifier_identity);
    source.failure.category = normalize_text(&source.failure.category).to_ascii_lowercase();
    source.failure.phase = normalize_text(&source.failure.phase).to_ascii_lowercase();
    source.smallest_safe_next_action = normalize_text(&source.smallest_safe_next_action);
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_product_metadata(
    ao2_version: &str,
    control_plane_version: &str,
    platform: &Platform,
    workflow: &WorkflowIdentity,
    failure: &FailureIdentity,
    smallest_safe_next_action: &str,
) -> Result<()> {
    validate_version("ao2_version", ao2_version)?;
    validate_version("control_plane_version", control_plane_version)?;
    validate_allowed("platform.os", &platform.os, &["linux", "macos", "windows"])?;
    validate_allowed(
        "platform.architecture",
        &platform.architecture,
        &["aarch64", "x86_64"],
    )?;
    validate_allowed("workflow.identity", &workflow.identity, ALLOWED_WORKFLOWS)?;
    validate_allowed(
        "workflow.verifier_identity",
        &workflow.verifier_identity,
        ALLOWED_VERIFIERS,
    )?;
    validate_allowed(
        "failure.category",
        &failure.category,
        ALLOWED_FAILURE_CATEGORIES,
    )?;
    validate_allowed("failure.phase", &failure.phase, ALLOWED_FAILURE_PHASES)?;
    validate_allowed(
        "smallest_safe_next_action",
        smallest_safe_next_action,
        ALLOWED_NEXT_ACTIONS,
    )
}

fn validate_version(name: &str, value: &str) -> Result<()> {
    validate_text(name, value, 64)?;
    let components = value.split('.').collect::<Vec<_>>();
    if !(2..=3).contains(&components.len())
        || components.iter().any(|component| {
            component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        bail!("{name} must be a numeric two- or three-component version");
    }
    Ok(())
}

fn validate_allowed(name: &str, value: &str, allowed: &[&str]) -> Result<()> {
    if !allowed.contains(&value) {
        bail!("{name} is not an allowed public support value");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn fingerprint_subject(
    ao2_version: &str,
    control_plane_version: &str,
    platform: &Platform,
    workflow: &WorkflowIdentity,
    approval: &ApprovalState,
    replay: &StatusState,
    evidence: &EvidenceState,
    manifest_sha256: &str,
    release_sha256: &str,
    failure: &FailureIdentity,
    smallest_safe_next_action: &str,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.troubleshooting-problem-fingerprint.v0.1",
        "ao2_version": ao2_version,
        "control_plane_version": control_plane_version,
        "platform": platform,
        "workflow": workflow,
        "approval": approval,
        "replay": replay,
        "evidence": evidence,
        "manifest_sha256": manifest_sha256,
        "release_sha256": release_sha256,
        "failure": failure,
        "smallest_safe_next_action": smallest_safe_next_action
    })
}

fn validate_approval(approval: &ApprovalState) -> Result<()> {
    if !matches!(
        approval.status.as_str(),
        "not_required" | "not_attempted" | "waiting" | "approved" | "rejected"
    ) {
        bail!("approval.status is unsupported");
    }
    validate_state_digest(
        "approval.action_digest",
        &approval.action_digest,
        matches!(
            approval.status.as_str(),
            "waiting" | "approved" | "rejected"
        ),
    )
}

fn validate_evidence(evidence: &EvidenceState) -> Result<()> {
    validate_status(
        "evidence.status",
        &evidence.status,
        &["not_available", "retained", "verified", "failed"],
    )?;
    validate_state_digest(
        "evidence.digest",
        &evidence.digest,
        evidence.status != "not_available",
    )
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > max
        || value
            .bytes()
            .any(|byte| byte == 0 || byte == 0x7f || (byte < 0x20 && byte != b'\t'))
    {
        bail!("{name} must be non-empty and at most {max} bytes without control characters");
    }
    Ok(())
}

fn validate_status(name: &str, value: &str, allowed: &[&str]) -> Result<()> {
    if !allowed.contains(&value) {
        bail!("{name} is unsupported");
    }
    Ok(())
}

fn validate_state_digest(name: &str, value: &Option<String>, required: bool) -> Result<()> {
    match value {
        Some(value) => validate_digest(name, value),
        None if required => bail!("{name} is required for this status"),
        None => Ok(()),
    }?;
    if !required && value.is_some() {
        bail!("{name} must be absent for this status");
    }
    Ok(())
}

fn validate_digest(name: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("{name} must be a lowercase SHA-256 digest");
    }
    Ok(())
}

fn sanitize_logs(logs: &[String]) -> Result<(Vec<SanitizedLog>, RedactionSummary)> {
    let mut sanitized = Vec::with_capacity(logs.len());
    let summary = RedactionSummary {
        input_log_entries: logs.len(),
        fully_redacted_log_entries: logs.len(),
    };
    for (index, raw) in logs.iter().enumerate() {
        validate_text("log entry", raw, MAX_LOG_BYTES)?;
        if looks_like_private_source(raw) {
            bail!("log entry appears to contain private source content");
        }
        sanitized.push(SanitizedLog {
            sequence: index + 1,
            text: "[REDACTED_LOG]".to_string(),
        });
    }
    Ok((sanitized, summary))
}

fn looks_like_private_source(input: &str) -> bool {
    let prefixes = [
        "fn ",
        "pub fn ",
        "let ",
        "const ",
        "static ",
        "var ",
        "function ",
        "def ",
        "class ",
        "import ",
        "package ",
        "module ",
        "impl ",
        "struct ",
        "enum ",
        "interface ",
        "namespace ",
        "using ",
        "if ",
        "for ",
        "while ",
        "match ",
        "return ",
        "select ",
        "insert ",
        "update ",
        "delete ",
        "#include ",
        "#!/",
        "<script",
        "use crate::",
    ];
    let normalized = input.trim_start().to_ascii_lowercase();
    let trimmed = normalized.as_str();
    let candidates = std::iter::once(trimmed).chain(
        trimmed
            .split_once(": ")
            .map(|(_, remainder)| remainder.trim_start()),
    );
    candidates
        .clone()
        .any(|candidate| prefixes.iter().any(|prefix| candidate.starts_with(prefix)))
        || candidates.into_iter().any(|candidate| {
            candidate.contains('{') && candidate.contains('}') && candidate.contains("return ")
        })
}

fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let mut file = open_bounded_input(path)?;
    let metadata = validate_opened_input(&file, path)?;
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(MAX_BYTES as usize));
    Read::by_ref(&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {}", path.display()))?;
    if bytes.len() as u64 > MAX_BYTES {
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
        options.custom_flags(windows_input_open_flags());
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
    validate_windows_opened_input(file, path)?;
    let metadata = file
        .metadata()
        .with_context(|| format!("inspect opened input {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!("input must be a regular file: {}", path.display());
    }
    if metadata.len() > MAX_BYTES {
        bail!(
            "input exceeds the 65536-byte limit: {} bytes",
            metadata.len()
        );
    }
    Ok(metadata)
}

#[cfg(windows)]
fn windows_input_open_flags() -> u32 {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    };
    FILE_FLAG_OPEN_REPARSE_POINT | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION
}

#[cfg(windows)]
fn validate_windows_opened_input(file: &fs::File, path: &Path) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, GetFileInformationByHandleEx, GetFileType, FILE_ATTRIBUTE_TAG_INFO,
    };

    let file_type = unsafe { GetFileType(file.as_raw_handle()) };
    let mut tag_info = FILE_ATTRIBUTE_TAG_INFO::default();
    let inspected = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            std::ptr::from_mut(&mut tag_info).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if inspected == 0 {
        bail!(
            "inspect Windows input reparse attributes before read: {}",
            path.display()
        );
    }
    validate_windows_file_characteristics(file_type, tag_info.FileAttributes, path)
}

#[cfg(windows)]
fn validate_windows_file_characteristics(
    file_type: u32,
    file_attributes: u32,
    path: &Path,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_TYPE_DISK};

    if file_type != FILE_TYPE_DISK || file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        bail!(
            "Windows input must be a non-reparse FILE_TYPE_DISK before read: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_TYPE_DISK, FILE_TYPE_PIPE,
    };

    #[test]
    fn input_characteristics_reject_non_disk_and_reparse_handles() {
        let path = Path::new("support-bundle.json");
        assert!(
            validate_windows_file_characteristics(FILE_TYPE_DISK, FILE_ATTRIBUTE_NORMAL, path)
                .is_ok()
        );
        assert!(
            validate_windows_file_characteristics(FILE_TYPE_PIPE, FILE_ATTRIBUTE_NORMAL, path)
                .is_err()
        );
        assert!(validate_windows_file_characteristics(
            FILE_TYPE_DISK,
            FILE_ATTRIBUTE_REPARSE_POINT,
            path
        )
        .is_err());
    }
}

fn write_exclusive(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).with_context(|| {
        format!(
            "create support bundle without overwriting {}",
            path.display()
        )
    })?;
    file.write_all(bytes)
        .with_context(|| format!("write support bundle {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync support bundle {}", path.display()))
}
