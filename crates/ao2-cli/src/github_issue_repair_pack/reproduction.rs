use super::*;
use anyhow::ensure;

const MAX_BYTES: u64 = 65_536;
const FIXTURE_MAX_BYTES: u64 = 262_144;
const OUTPUT_MAX_BYTES: u64 = 1_048_576;
const FAILURE_SIGNATURE_MAX_BYTES: usize = 1_024;
const MAX_ARG_BYTES: usize = 256;
const MAX_ARGV_BYTES: usize = 4_096;
const MAX_ARGV_ITEMS: usize = 64;

#[derive(Debug, Default)]
pub(super) enum ArtifactField {
    #[default]
    Missing,
    Present(Artifact),
}

impl<'de> Deserialize<'de> for ArtifactField {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Artifact::deserialize(deserializer).map(Self::Present)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Version {
    V1,
    V2,
}

impl Version {
    pub(super) fn validation_schema(self) -> &'static str {
        match self {
            Self::V1 => "ao2.github-issue-repair-pack-validation.v1",
            Self::V2 => "ao2.github-issue-repair-pack-validation.v2",
        }
    }
}

pub(super) struct Validated {
    pub(super) sha256: String,
    pub(super) fixture_sha256: String,
    pub(super) output_sha256: String,
}

pub(super) fn print_text(value: Option<&Validated>) {
    if let Some(value) = value {
        println!("eligibility_status=reproduced");
        println!("reproduction_evidence_sha256={}", value.sha256);
        println!("reproduction_fixture_sha256={}", value.fixture_sha256);
        println!("reproduction_output_sha256={}", value.output_sha256);
    }
}

struct BoundArtifact {
    bytes: Vec<u8>,
    identity: FileIdentity,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema_version: String,
    request_id: String,
    candidate_id: String,
    source_sha: String,
    command_argv: Vec<String>,
    working_directory: String,
    fixture_install_path: String,
    test_identifier: String,
    toolchain: Toolchain,
    fixture_sha256: String,
    output_sha256: String,
    failure_signature: String,
    failure_signature_sha256: String,
    result: String,
    expected_exit_code: u16,
    observed_exit_code: u16,
    network: String,
    git_history_present: bool,
    oracle_present: bool,
    credentials_present: bool,
    external_effects: u64,
    completed_at: String,
}

pub(super) fn version(
    schema: &str,
    evidence: &ArtifactField,
    fixture: &ArtifactField,
    output: &ArtifactField,
) -> Result<Version> {
    let present = [
        matches!(evidence, ArtifactField::Present(_)),
        matches!(fixture, ArtifactField::Present(_)),
        matches!(output, ArtifactField::Present(_)),
    ];
    match schema {
        "ao2.github-issue-repair-pack.v1" if !present.iter().any(|item| *item) => Ok(Version::V1),
        "ao2.github-issue-repair-pack.v1" => {
            bail!("v1 repair pack must not declare reproduction artifacts")
        }
        "ao2.github-issue-repair-pack.v2" if present.iter().all(|item| *item) => Ok(Version::V2),
        "ao2.github-issue-repair-pack.v2" => {
            bail!("v2 repair pack requires evidence, fixture, and output artifacts")
        }
        _ => bail!("unsupported repair pack schema_version"),
    }
}

pub(super) fn validate(
    root: &RootGuard,
    manifest: &RepairPackManifest,
    manifest_name: &str,
    manifest_identity: FileIdentity,
    artifact_identities: [FileIdentity; 3],
    prior_size: u64,
) -> Result<Option<Validated>> {
    let ArtifactField::Present(evidence_artifact) = &manifest.reproduction_evidence else {
        return Ok(None);
    };
    let ArtifactField::Present(fixture_artifact) = &manifest.reproduction_fixture else {
        bail!("v2 repair pack requires reproduction_fixture");
    };
    let ArtifactField::Present(output_artifact) = &manifest.reproduction_output else {
        bail!("v2 repair pack requires reproduction_output");
    };

    let reproduction_artifacts = [
        ("reproduction_evidence", evidence_artifact, MAX_BYTES),
        ("reproduction_fixture", fixture_artifact, FIXTURE_MAX_BYTES),
        ("reproduction_output", output_artifact, OUTPUT_MAX_BYTES),
    ];
    let base_paths = [
        manifest_name,
        manifest.source_archive.path.as_str(),
        manifest.issue_snapshot.path.as_str(),
        manifest.dependency_cache_manifest.path.as_str(),
    ];
    for (index, (label, artifact, _)) in reproduction_artifacts.iter().enumerate() {
        ensure!(
            !base_paths.contains(&artifact.path.as_str())
                && !reproduction_artifacts[index + 1..]
                    .iter()
                    .any(|(_, other, _)| other.path == artifact.path),
            "{label} must not alias the manifest or another artifact"
        );
    }

    let total_size =
        reproduction_artifacts
            .iter()
            .try_fold(prior_size, |size, (_, artifact, _)| {
                size.checked_add(artifact.size_bytes)
                    .context("referenced artifact size overflow")
            })?;
    ensure!(
        total_size <= TOTAL_ARTIFACTS_MAX_BYTES,
        "total referenced artifacts exceed 2147483648-byte limit"
    );

    let evidence_bound = read_bound(root, evidence_artifact, MAX_BYTES, "reproduction_evidence")?;
    let fixture_bound = read_bound(
        root,
        fixture_artifact,
        FIXTURE_MAX_BYTES,
        "reproduction_fixture",
    )?;
    let output_bound = read_bound(
        root,
        output_artifact,
        OUTPUT_MAX_BYTES,
        "reproduction_output",
    )?;
    let identities = [
        evidence_bound.identity,
        fixture_bound.identity,
        output_bound.identity,
    ];
    for (index, identity) in identities.iter().enumerate() {
        ensure!(
            *identity != manifest_identity
                && !artifact_identities.contains(identity)
                && !identities[index + 1..].contains(identity),
            "reproduction artifacts must not alias another input file"
        );
    }

    let evidence: Evidence = serde_json::from_slice(&evidence_bound.bytes)
        .context("parse strict reproduction evidence JSON")?;
    validate_evidence(
        &evidence,
        manifest,
        &fixture_bound.sha256,
        &output_bound.sha256,
        &output_bound.bytes,
    )?;

    for (label, artifact, max_bytes, bound) in [
        (
            "reproduction_evidence",
            evidence_artifact,
            MAX_BYTES,
            &evidence_bound,
        ),
        (
            "reproduction_fixture",
            fixture_artifact,
            FIXTURE_MAX_BYTES,
            &fixture_bound,
        ),
        (
            "reproduction_output",
            output_artifact,
            OUTPUT_MAX_BYTES,
            &output_bound,
        ),
    ] {
        let confirmed = read_bound(root, artifact, max_bytes, label)?;
        ensure!(
            confirmed.identity == bound.identity && confirmed.bytes == bound.bytes,
            "{label} changed while validating"
        );
    }

    Ok(Some(Validated {
        sha256: evidence_bound.sha256,
        fixture_sha256: fixture_bound.sha256,
        output_sha256: output_bound.sha256,
    }))
}

fn read_bound(
    root: &RootGuard,
    artifact: &Artifact,
    max_bytes: u64,
    label: &str,
) -> Result<BoundArtifact> {
    validate_digest(&format!("{label}.sha256"), &artifact.sha256)?;
    ensure!(
        artifact.size_bytes <= max_bytes,
        "{label} declared size exceeds {max_bytes}-byte limit"
    );
    validate_direct_child_name(&artifact.path, label)?;
    let (bytes, identity) = read_regular_file(root, &artifact.path, max_bytes, label)?;
    ensure!(
        bytes.len() as u64 == artifact.size_bytes,
        "{label} size does not match manifest"
    );
    let sha256 = digest(&bytes);
    ensure!(
        sha256 == artifact.sha256,
        "{label} SHA-256 does not match manifest"
    );
    Ok(BoundArtifact {
        bytes,
        identity,
        sha256,
    })
}

fn validate_evidence(
    evidence: &Evidence,
    manifest: &RepairPackManifest,
    fixture_sha256: &str,
    output_sha256: &str,
    output: &[u8],
) -> Result<()> {
    ensure!(
        evidence.schema_version == "ao2.github-issue-reproduction-evidence.v1",
        "unsupported reproduction evidence schema_version"
    );
    ensure!(
        evidence.request_id == manifest.request_id
            && evidence.candidate_id == manifest.candidate_id
            && evidence.source_sha == manifest.source_sha,
        "reproduction evidence identity does not match manifest"
    );
    validate_argv(&evidence.command_argv, manifest, &evidence.test_identifier)?;
    ensure!(
        evidence.working_directory == ".",
        "reproduction evidence working_directory must be the extracted source root"
    );
    validate_fixture_binding(evidence, manifest)?;
    ensure!(
        evidence.toolchain.name == manifest.toolchain.name
            && evidence.toolchain.version == manifest.toolchain.version,
        "reproduction evidence toolchain does not match manifest"
    );
    ensure!(
        evidence.fixture_sha256 == fixture_sha256 && evidence.output_sha256 == output_sha256,
        "reproduction evidence fixture or output digest does not match its artifact"
    );
    ensure!(
        evidence.failure_signature.len() >= 8
            && evidence.failure_signature.len() <= FAILURE_SIGNATURE_MAX_BYTES
            && evidence
                .failure_signature
                .bytes()
                .any(|byte| byte.is_ascii_alphanumeric())
            && !evidence
                .failure_signature
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == 0x7f),
        "reproduction evidence failure_signature must be bounded printable text"
    );
    validate_digest(
        "failure_signature_sha256",
        &evidence.failure_signature_sha256,
    )?;
    ensure!(
        digest(evidence.failure_signature.as_bytes()) == evidence.failure_signature_sha256,
        "reproduction evidence failure signature digest does not match"
    );
    ensure!(
        output
            .windows(evidence.failure_signature.len())
            .any(|window| window == evidence.failure_signature.as_bytes()),
        "reproduction output does not contain the bound failure signature"
    );
    ensure!(
        evidence.result == "reproduced_failure",
        "reproduction evidence result must be reproduced_failure"
    );
    ensure!(
        (1..=255).contains(&evidence.expected_exit_code)
            && evidence.expected_exit_code == evidence.observed_exit_code,
        "reproduction evidence exit codes must be equal and between 1 and 255"
    );
    ensure!(
        evidence.network == "none"
            && !evidence.git_history_present
            && !evidence.oracle_present
            && !evidence.credentials_present
            && evidence.external_effects == 0,
        "reproduction evidence safety boundary is not the exact passing boundary"
    );
    let completed = DateTime::parse_from_rfc3339(&evidence.completed_at)
        .context("reproduction evidence completed_at must use RFC3339 timestamp syntax")?
        .with_timezone(&Utc);
    let fetched = DateTime::parse_from_rfc3339(&manifest.fetched_at)?.with_timezone(&Utc);
    let now = Utc::now();
    ensure!(
        completed >= now - Duration::days(FETCHED_AT_MAX_AGE_DAYS),
        "reproduction evidence completed_at must be no more than 7 days old"
    );
    ensure!(
        completed <= now + Duration::minutes(FETCHED_AT_MAX_FUTURE_SKEW_MINUTES),
        "reproduction evidence completed_at must not be more than 5 minutes in the future"
    );
    ensure!(
        completed <= fetched,
        "reproduction evidence completed_at must not be later than manifest fetched_at"
    );
    Ok(())
}

fn validate_argv(
    argv: &[String],
    manifest: &RepairPackManifest,
    test_identifier: &str,
) -> Result<()> {
    ensure!(
        !argv.is_empty() && argv.len() <= MAX_ARGV_ITEMS,
        "reproduction evidence command_argv must contain 1 to 64 arguments"
    );
    let mut total = 0_usize;
    for argument in argv {
        ensure!(
            !argument.is_empty() && argument.len() <= MAX_ARG_BYTES,
            "reproduction evidence command_argv arguments must contain 1 to 256 bytes"
        );
        ensure!(
            !argument
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == 0x7f),
            "reproduction evidence command_argv contains a control character"
        );
        total = total
            .checked_add(argument.len())
            .context("command_argv size overflow")?;
    }
    ensure!(
        total <= MAX_ARGV_BYTES,
        "reproduction evidence command_argv exceeds 4096-byte limit"
    );
    match manifest.language.as_str() {
        "go" => validate_go_args(argv, test_identifier),
        "rust" => validate_rust_args(argv, test_identifier),
        _ => bail!("reproduction evidence language has no supported test runner"),
    }
}

fn validate_fixture_binding(evidence: &Evidence, manifest: &RepairPackManifest) -> Result<()> {
    let ArtifactField::Present(fixture) = &manifest.reproduction_fixture else {
        bail!("v2 repair pack requires reproduction_fixture");
    };
    match manifest.language.as_str() {
        "go" => {
            validate_go_test_identifier(&evidence.test_identifier)?;
            ensure!(
                evidence.fixture_install_path == fixture.path
                    && evidence.fixture_install_path.ends_with("_test.go"),
                "Go reproduction fixture must install as the bound source-root test file"
            );
        }
        "rust" => {
            validate_rust_target(&evidence.test_identifier)?;
            ensure!(
                evidence.fixture_install_path == format!("tests/{}.rs", evidence.test_identifier),
                "Rust reproduction fixture install path must match the focused test target"
            );
        }
        _ => bail!("reproduction evidence language has no supported fixture binding"),
    }
    Ok(())
}

fn validate_go_args(argv: &[String], test_identifier: &str) -> Result<()> {
    validate_go_test_identifier(test_identifier)?;
    ensure!(
        argv.first().is_some_and(|arg| arg == "go") && argv.get(1).is_some_and(|arg| arg == "test"),
        "Go reproduction must invoke go test directly"
    );
    let mut index = 2;
    let mut package_seen = false;
    let mut run_seen = false;
    while index < argv.len() {
        match argv[index].as_str() {
            "." if !package_seen => package_seen = true,
            "-v" | "-count=1" => {}
            "-run" if !run_seen => {
                index += 1;
                let pattern = argv
                    .get(index)
                    .context("Go reproduction -run requires a bounded pattern")?;
                ensure!(
                    pattern == &format!("^{test_identifier}$"),
                    "Go reproduction -run pattern must match test_identifier exactly"
                );
                run_seen = true;
            }
            _ => bail!("Go reproduction contains an unsupported test argument"),
        }
        index += 1;
    }
    ensure!(
        package_seen && run_seen,
        "Go reproduction must bind the source-root package and focused test"
    );
    Ok(())
}

fn validate_rust_args(argv: &[String], test_identifier: &str) -> Result<()> {
    validate_rust_target(test_identifier)?;
    ensure!(
        argv.first().is_some_and(|arg| arg == "cargo")
            && argv.get(1).is_some_and(|arg| arg == "test"),
        "Rust reproduction must invoke cargo test directly"
    );
    ensure!(
        argv.len() == 4 && argv[2] == "--test" && argv[3] == test_identifier,
        "Rust reproduction must bind exactly one focused test target"
    );
    Ok(())
}

fn validate_go_test_identifier(value: &str) -> Result<()> {
    ensure!(
        value.len() > 4
            && value.starts_with("Test")
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "Go reproduction test_identifier must name one portable Test function"
    );
    Ok(())
}

fn validate_rust_target(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "Rust reproduction test_identifier must name one portable Cargo test target"
    );
    Ok(())
}
