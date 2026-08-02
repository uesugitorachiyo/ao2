use crate::cli::RepairPackCommand;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
#[path = "unix_input.rs"]
mod unix_input;

const MANIFEST_MAX_BYTES: u64 = 65_536;
const ISSUE_SNAPSHOT_MAX_BYTES: u64 = 262_144;
const DEPENDENCY_CACHE_MANIFEST_MAX_BYTES: u64 = 262_144;
const SOURCE_ARCHIVE_MAX_BYTES: u64 = 1_073_741_824;
const TOTAL_ARTIFACTS_MAX_BYTES: u64 = 2_147_483_648;
const IDENTIFIER_MAX_BYTES: usize = 128;
const TOOLCHAIN_FIELD_MAX_BYTES: usize = 128;
const IO_CHUNK_BYTES: usize = 64 * 1024;
const FETCHED_AT_MAX_AGE_DAYS: i64 = 7;
const FETCHED_AT_MAX_FUTURE_SKEW_MINUTES: i64 = 5;

#[derive(Debug)]
struct VerifiedArtifact {
    size_bytes: u64,
    sha256: String,
}

struct RootGuard {
    canonical_path: PathBuf,
    directory: File,
    identity: FileIdentity,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(windows)]
type FileIdentity = crate::windows_input::DiskFileIdentity;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepairPackManifest {
    schema_version: String,
    request_id: String,
    corpus_id: String,
    candidate_id: String,
    repository: String,
    issue_number: u64,
    source_sha: String,
    license: String,
    language: String,
    fetched_at: String,
    source_archive: Artifact,
    issue_snapshot: Artifact,
    dependency_cache_manifest: Artifact,
    toolchain: Toolchain,
    extracted_tree_sha256: String,
    known_fix_fetched: bool,
    safety: SafetyBoundary,
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
struct Toolchain {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SafetyBoundary {
    authority_level: String,
    network: String,
    git_history_present: bool,
    oracle_present: bool,
    credentials_present: bool,
    campaign_root_mounted: bool,
    repair_pack_read_only: bool,
    scratch_read_write: bool,
    third_party_mutation_authorized: bool,
}

#[derive(Debug, Serialize)]
struct ValidationReadback<'a> {
    schema_version: &'static str,
    status: &'static str,
    request_id: &'a str,
    corpus_id: &'a str,
    candidate_id: &'a str,
    repository: &'a str,
    issue_number: u64,
    source_sha: &'a str,
    license: &'a str,
    language: &'a str,
    fetched_at: &'a str,
    manifest_sha256: String,
    source_archive_sha256: &'a str,
    issue_snapshot_sha256: &'a str,
    dependency_cache_manifest_sha256: &'a str,
    extracted_tree_sha256: &'a str,
    failed_rows: u64,
    authority_level: &'static str,
    network: &'static str,
    git_history_present: bool,
    oracle_present: bool,
    credentials_present: bool,
    campaign_root_mounted: bool,
    repair_pack_read_only: bool,
    scratch_read_write: bool,
    third_party_mutation_authorized: bool,
    network_accessed: bool,
    git_invoked: bool,
    github_read_performed: bool,
    github_write_performed: bool,
    repair_executed: bool,
    mutation_performed: bool,
    executes_work: bool,
    approves_work: bool,
}

pub(crate) fn run(command: RepairPackCommand) -> Result<()> {
    match command {
        RepairPackCommand::Validate {
            manifest,
            root,
            json,
        } => validate(&manifest, &root, json),
    }
}

fn validate(manifest_path: &Path, root_path: &Path, json: bool) -> Result<()> {
    let root = RootGuard::open(root_path)?;
    let manifest_name = direct_manifest_child_name(root_path, &root.canonical_path, manifest_path)?;
    let (manifest_bytes, manifest_identity) =
        read_regular_file(&root, &manifest_name, MANIFEST_MAX_BYTES, "manifest")?;
    let manifest: RepairPackManifest = serde_json::from_slice(&manifest_bytes)
        .context("parse strict repair pack manifest JSON")?;
    validate_manifest(&manifest)?;

    let artifact_paths = [
        manifest.source_archive.path.as_str(),
        manifest.issue_snapshot.path.as_str(),
        manifest.dependency_cache_manifest.path.as_str(),
    ];
    if artifact_paths[0] == artifact_paths[1]
        || artifact_paths[0] == artifact_paths[2]
        || artifact_paths[1] == artifact_paths[2]
    {
        bail!("referenced artifacts must not alias the same path");
    }
    if artifact_paths.contains(&manifest_name.as_str()) {
        bail!("manifest must not alias any referenced artifact");
    }
    let total_size = manifest
        .source_archive
        .size_bytes
        .checked_add(manifest.issue_snapshot.size_bytes)
        .and_then(|size| size.checked_add(manifest.dependency_cache_manifest.size_bytes))
        .context("referenced artifact size overflow")?;
    if total_size > TOTAL_ARTIFACTS_MAX_BYTES {
        bail!("total referenced artifacts exceed 2147483648-byte limit");
    }

    let (source_verified, source_identity) = verify_artifact(
        &root,
        &manifest.source_archive,
        SOURCE_ARCHIVE_MAX_BYTES,
        "source_archive",
    )?;
    let (snapshot_verified, snapshot_identity) = verify_artifact(
        &root,
        &manifest.issue_snapshot,
        ISSUE_SNAPSHOT_MAX_BYTES,
        "issue_snapshot",
    )?;
    let (dependency_cache_verified, dependency_cache_identity) = verify_artifact(
        &root,
        &manifest.dependency_cache_manifest,
        DEPENDENCY_CACHE_MANIFEST_MAX_BYTES,
        "dependency_cache_manifest",
    )?;
    if manifest_identity == source_identity
        || manifest_identity == snapshot_identity
        || manifest_identity == dependency_cache_identity
    {
        bail!("manifest must not alias any referenced artifact");
    }
    if source_identity == snapshot_identity
        || source_identity == dependency_cache_identity
        || snapshot_identity == dependency_cache_identity
    {
        bail!("referenced artifacts must not alias one file");
    }

    let readback = ValidationReadback {
        schema_version: "ao2.github-issue-repair-pack-validation.v1",
        status: "passed",
        request_id: &manifest.request_id,
        corpus_id: &manifest.corpus_id,
        candidate_id: &manifest.candidate_id,
        repository: &manifest.repository,
        issue_number: manifest.issue_number,
        source_sha: &manifest.source_sha,
        license: &manifest.license,
        language: &manifest.language,
        fetched_at: &manifest.fetched_at,
        manifest_sha256: digest(&manifest_bytes),
        source_archive_sha256: &source_verified.sha256,
        issue_snapshot_sha256: &snapshot_verified.sha256,
        dependency_cache_manifest_sha256: &dependency_cache_verified.sha256,
        extracted_tree_sha256: &manifest.extracted_tree_sha256,
        failed_rows: 0,
        authority_level: "L1",
        network: "none",
        git_history_present: false,
        oracle_present: false,
        credentials_present: false,
        campaign_root_mounted: false,
        repair_pack_read_only: true,
        scratch_read_write: true,
        third_party_mutation_authorized: false,
        network_accessed: false,
        git_invoked: false,
        github_read_performed: false,
        github_write_performed: false,
        repair_executed: false,
        mutation_performed: false,
        executes_work: false,
        approves_work: false,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&readback)?);
    } else {
        println!("status=passed");
        println!("manifest_sha256={}", readback.manifest_sha256);
        println!("failed_rows=0");
    }
    Ok(())
}

fn direct_manifest_child_name(
    supplied_root: &Path,
    canonical_root: &Path,
    manifest_path: &Path,
) -> Result<String> {
    let current_dir =
        std::env::current_dir().context("resolve current directory for manifest containment")?;
    let absolute_root = if supplied_root.is_absolute() {
        supplied_root.to_path_buf()
    } else {
        current_dir.join(supplied_root)
    };
    let absolute_manifest = if manifest_path.is_absolute() {
        manifest_path.to_path_buf()
    } else {
        current_dir.join(manifest_path)
    };
    let relative = absolute_manifest
        .strip_prefix(&absolute_root)
        .or_else(|_| absolute_manifest.strip_prefix(canonical_root))
        .context("manifest must be contained under the canonical repair pack root")?;
    let declared = relative
        .to_str()
        .context("manifest path must use a UTF-8 direct-child name")?;
    validate_direct_child_name(declared, "manifest")?;
    Ok(declared.to_owned())
}

fn validate_manifest(manifest: &RepairPackManifest) -> Result<()> {
    if manifest.schema_version != "ao2.github-issue-repair-pack.v1" {
        bail!("unsupported repair pack schema_version");
    }
    for (name, value) in [
        ("request_id", manifest.request_id.as_str()),
        ("corpus_id", manifest.corpus_id.as_str()),
        ("candidate_id", manifest.candidate_id.as_str()),
    ] {
        validate_identifier(name, value, IDENTIFIER_MAX_BYTES)?;
    }
    validate_repository(&manifest.repository)?;
    if manifest.issue_number == 0 {
        bail!("issue_number must be positive");
    }
    if !is_lower_hex(&manifest.source_sha, 40) {
        bail!("source_sha must be exactly 40 lowercase hexadecimal characters");
    }
    if !matches!(
        manifest.license.as_str(),
        "MIT" | "Apache-2.0" | "BSD-2-Clause" | "BSD-3-Clause"
    ) {
        bail!("license is not allowed");
    }
    if !matches!(manifest.language.as_str(), "go" | "rust") {
        bail!("language is not allowed");
    }
    let fetched_at = DateTime::parse_from_rfc3339(&manifest.fetched_at)
        .context("fetched_at must use RFC3339 timestamp syntax")?
        .with_timezone(&Utc);
    let now = Utc::now();
    if fetched_at < now - Duration::days(FETCHED_AT_MAX_AGE_DAYS) {
        bail!("fetched_at must be no more than 7 days old");
    }
    if fetched_at > now + Duration::minutes(FETCHED_AT_MAX_FUTURE_SKEW_MINUTES) {
        bail!("fetched_at must not be more than 5 minutes in the future");
    }
    validate_identifier(
        "toolchain.name",
        &manifest.toolchain.name,
        TOOLCHAIN_FIELD_MAX_BYTES,
    )?;
    validate_identifier(
        "toolchain.version",
        &manifest.toolchain.version,
        TOOLCHAIN_FIELD_MAX_BYTES,
    )?;
    validate_digest("extracted_tree_sha256", &manifest.extracted_tree_sha256)?;
    if manifest.known_fix_fetched {
        bail!("known_fix_fetched must be false");
    }
    validate_safety(&manifest.safety)
}

fn validate_identifier(name: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.is_empty() || value.len() > max_bytes {
        bail!("{name} must be nonempty and at most {max_bytes} bytes");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("{name} contains an invalid character");
    }
    Ok(())
}

fn validate_repository(repository: &str) -> Result<()> {
    let mut parts = repository.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        bail!("repository must use canonical owner/name syntax");
    };
    if owner.is_empty() || owner.len() > 39 {
        bail!("repository owner must contain 1 to 39 characters");
    }
    let owner_bytes = owner.as_bytes();
    if !owner_bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        || !owner_bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        || !owner_bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        || owner.contains("--")
    {
        bail!("repository owner must use canonical GitHub owner grammar");
    }
    if name.is_empty() || name.len() > 100 {
        bail!("repository name must contain 1 to 100 characters");
    }
    if matches!(name, "." | "..")
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("repository name must use canonical GitHub repository grammar");
    }
    if name.to_ascii_lowercase().ends_with(".git") {
        bail!("repository must not include a .git suffix");
    }
    Ok(())
}

fn validate_safety(safety: &SafetyBoundary) -> Result<()> {
    if safety.authority_level != "L1"
        || safety.network != "none"
        || safety.git_history_present
        || safety.oracle_present
        || safety.credentials_present
        || safety.campaign_root_mounted
        || !safety.repair_pack_read_only
        || !safety.scratch_read_write
        || safety.third_party_mutation_authorized
    {
        bail!("repair pack safety boundary is not the exact passing L1 boundary");
    }
    Ok(())
}

fn validate_direct_child_name(declared: &str, label: &str) -> Result<()> {
    if declared.is_empty()
        || declared.contains('\\')
        || declared.contains(':')
        || declared.contains('/')
    {
        bail!("{label} must be a direct child of the repair pack root");
    }
    let mut components = Path::new(declared).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        bail!("{label} must use one normal direct-child component");
    }
    Ok(())
}

impl RootGuard {
    fn open(root_path: &Path) -> Result<Self> {
        let root_metadata = fs::symlink_metadata(root_path)
            .with_context(|| format!("inspect repair pack root {}", root_path.display()))?;
        if metadata_is_link(&root_metadata) || !root_metadata.is_dir() {
            bail!("repair pack root must be a real directory, not a link");
        }
        #[cfg(unix)]
        let canonical_path = fs::canonicalize(root_path)
            .with_context(|| format!("canonicalize repair pack root {}", root_path.display()))?;
        #[cfg(unix)]
        let expected_identity = expected_root_identity(&fs::metadata(&canonical_path)?)?;
        let directory = open_root_directory(root_path)
            .with_context(|| format!("open repair pack root {}", root_path.display()))?;
        validate_root_directory_handle(&directory, root_path)?;
        let identity = root_file_identity(&directory, root_path)?;
        #[cfg(unix)]
        if identity != expected_identity {
            bail!("repair pack root identity changed while opening");
        }
        #[cfg(windows)]
        let canonical_path = fs::canonicalize(root_path).with_context(|| {
            format!(
                "canonicalize retained repair pack root {}",
                root_path.display()
            )
        })?;
        let root_metadata_after = fs::symlink_metadata(root_path).with_context(|| {
            format!(
                "reinspect retained repair pack root {}",
                root_path.display()
            )
        })?;
        if metadata_is_link(&root_metadata_after) || !root_metadata_after.is_dir() {
            bail!("repair pack root changed while retaining its directory handle");
        }
        let canonical_after = fs::canonicalize(root_path)
            .with_context(|| format!("recanonicalize repair pack root {}", root_path.display()))?;
        if canonical_after != canonical_path {
            bail!("repair pack root canonical path changed while opening");
        }
        let guard = Self {
            canonical_path,
            directory,
            identity,
        };
        guard.validate_root_identity()?;
        Ok(guard)
    }

    fn validate_root_identity(&self) -> Result<()> {
        validate_root_directory_handle(&self.directory, &self.canonical_path)?;
        if root_file_identity(&self.directory, &self.canonical_path)? != self.identity {
            bail!("repair pack root handle identity changed");
        }
        Ok(())
    }

    fn open_child(&self, name: &str, label: &str) -> Result<File> {
        validate_direct_child_name(name, label)?;
        self.validate_root_identity()?;
        let file = open_child_from_root(self, name)
            .with_context(|| format!("open {label} direct child {name}"))?;
        self.validate_root_identity()?;
        Ok(file)
    }

    fn revalidate_child_identity(
        &self,
        name: &str,
        expected_identity: FileIdentity,
        label: &str,
    ) -> Result<()> {
        let reopened = self.open_child(name, label)?;
        if opened_file_identity(&reopened, Path::new(name))? != expected_identity {
            bail!("{label} direct-child identity changed while reading");
        }
        Ok(())
    }
}

fn metadata_is_link(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    if crate::windows_input::metadata_is_reparse(metadata) {
        return true;
    }
    false
}

fn verify_artifact(
    root: &RootGuard,
    artifact: &Artifact,
    max_bytes: u64,
    label: &str,
) -> Result<(VerifiedArtifact, FileIdentity)> {
    validate_digest(&format!("{label}.sha256"), &artifact.sha256)?;
    if artifact.size_bytes > max_bytes {
        bail!("{label} declared size exceeds {max_bytes}-byte limit");
    }
    validate_direct_child_name(&artifact.path, label)?;
    let (verified, identity) = hash_regular_file(root, &artifact.path, max_bytes, label)?;
    if verified.size_bytes != artifact.size_bytes {
        bail!("{label} size does not match manifest");
    }
    if verified.sha256 != artifact.sha256 {
        bail!("{label} SHA-256 does not match manifest");
    }
    Ok((verified, identity))
}

fn read_regular_file(
    root: &RootGuard,
    name: &str,
    max_bytes: u64,
    label: &str,
) -> Result<(Vec<u8>, FileIdentity)> {
    let mut file = root.open_child(name, label)?;
    let opened = file
        .metadata()
        .with_context(|| format!("inspect open {label}"))?;
    validate_regular_metadata(&opened, max_bytes, label)?;
    let identity = opened_file_identity(&file, Path::new(name))?;

    let capacity = usize::try_from(opened.len()).context("file size does not fit memory bounds")?;
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {label}"))?;
    if bytes.len() as u64 > max_bytes {
        bail!("{label} exceeds {max_bytes}-byte limit");
    }
    let opened_after = file
        .metadata()
        .with_context(|| format!("reinspect open {label}"))?;
    if opened.len() != opened_after.len()
        || opened.len() != bytes.len() as u64
        || opened_file_identity(&file, Path::new(name))? != identity
    {
        bail!("{label} identity or size changed while reading");
    }
    root.revalidate_child_identity(name, identity, label)?;
    Ok((bytes, identity))
}

fn hash_regular_file(
    root: &RootGuard,
    name: &str,
    max_bytes: u64,
    label: &str,
) -> Result<(VerifiedArtifact, FileIdentity)> {
    let mut file = root.open_child(name, label)?;
    let opened = file
        .metadata()
        .with_context(|| format!("inspect open {label}"))?;
    validate_regular_metadata(&opened, max_bytes, label)?;
    let identity = opened_file_identity(&file, Path::new(name))?;
    let verified = hash_and_count(&mut file, max_bytes, label)?;
    let opened_after = file
        .metadata()
        .with_context(|| format!("reinspect open {label}"))?;
    if opened.len() != opened_after.len()
        || opened.len() != verified.size_bytes
        || opened_file_identity(&file, Path::new(name))? != identity
    {
        bail!("{label} identity or size changed while reading");
    }
    root.revalidate_child_identity(name, identity, label)?;
    Ok((verified, identity))
}

fn hash_and_count<R: Read>(
    reader: &mut R,
    max_bytes: u64,
    label: &str,
) -> Result<VerifiedArtifact> {
    let mut hasher = Sha256::new();
    let mut size_bytes = 0_u64;
    let mut buffer = [0_u8; IO_CHUNK_BYTES];
    loop {
        let remaining = max_bytes.saturating_sub(size_bytes).saturating_add(1);
        let requested = usize::try_from(remaining.min(IO_CHUNK_BYTES as u64))?;
        let read = reader
            .read(&mut buffer[..requested])
            .with_context(|| format!("read {label}"))?;
        if read == 0 {
            break;
        }
        size_bytes = size_bytes
            .checked_add(read as u64)
            .context("artifact size overflow")?;
        if size_bytes > max_bytes {
            bail!("{label} exceeds {max_bytes}-byte limit");
        }
        hasher.update(&buffer[..read]);
    }
    Ok(VerifiedArtifact {
        size_bytes,
        sha256: format!("sha256:{:x}", hasher.finalize()),
    })
}

fn validate_regular_metadata(metadata: &Metadata, max_bytes: u64, label: &str) -> Result<()> {
    if !metadata.is_file() {
        bail!("{label} must be a regular file");
    }
    if metadata.len() > max_bytes {
        bail!("{label} exceeds {max_bytes}-byte limit");
    }
    Ok(())
}

#[cfg(unix)]
fn open_root_directory(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    Ok(options.open(path)?)
}

#[cfg(unix)]
fn open_child_from_root(root: &RootGuard, name: &str) -> Result<File> {
    unix_input::open_direct_child(&root.directory, name)
}

#[cfg(windows)]
fn open_root_directory(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(crate::windows_input::root_share_mode())
        .custom_flags(crate::windows_input::root_open_flags());
    Ok(options.open(path)?)
}

#[cfg(windows)]
fn open_child_from_root(root: &RootGuard, name: &str) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(crate::windows_input::open_flags());
    Ok(options.open(root.canonical_path.join(name))?)
}

fn validate_root_directory_handle(file: &File, _path: &Path) -> Result<()> {
    if !file.metadata()?.is_dir() {
        bail!("repair pack root handle must be a directory");
    }
    #[cfg(windows)]
    crate::windows_input::validate_non_reparse_disk_handle(file, _path)?;
    Ok(())
}

#[cfg(unix)]
fn expected_root_identity(metadata: &Metadata) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(unix)]
fn root_file_identity(file: &File, _path: &Path) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn root_file_identity(file: &File, path: &Path) -> Result<FileIdentity> {
    crate::windows_input::validate_non_reparse_disk_handle(file, path)?;
    crate::windows_input::disk_file_identity(file, path)
}

#[cfg(unix)]
fn opened_file_identity(file: &File, path: &Path) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if metadata.nlink() != 1 {
        bail!("input must not be hardlinked");
    }
    root_file_identity(file, path)
}

#[cfg(windows)]
fn opened_file_identity(file: &File, path: &Path) -> Result<FileIdentity> {
    crate::windows_input::validate_non_reparse_disk_handle(file, path)?;
    let state = crate::windows_input::disk_file_state(file, path)?;
    if state.number_of_links != 1 {
        bail!("input must not be hardlinked");
    }
    Ok(state.identity)
}

fn validate_digest(name: &str, value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("{name} must use lowercase sha256:<64 hex> syntax");
    };
    if !is_lower_hex(hex, 64) {
        bail!("{name} must use lowercase sha256:<64 hex> syntax");
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    struct ObservedReader<R> {
        inner: R,
        largest_buffer: usize,
    }

    impl<R: Read> Read for ObservedReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.largest_buffer = self.largest_buffer.max(buffer.len());
            self.inner.read(buffer)
        }
    }

    #[test]
    fn artifact_hashing_uses_a_fixed_size_buffer() {
        let bytes = vec![b'x'; IO_CHUNK_BYTES * 3 + 17];
        let mut reader = ObservedReader {
            inner: Cursor::new(&bytes),
            largest_buffer: 0,
        };

        let verified = hash_and_count(&mut reader, bytes.len() as u64, "artifact").unwrap();

        assert_eq!(verified.size_bytes, bytes.len() as u64);
        assert_eq!(verified.sha256, digest(&bytes));
        assert!(reader.largest_buffer <= IO_CHUNK_BYTES);
    }

    #[test]
    #[cfg(unix)]
    fn root_handle_open_survives_root_swap_and_restore() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("source.tar.gz"), b"trusted").unwrap();
        let guard = RootGuard::open(&root).unwrap();

        let parked = temp.path().join("parked-root");
        fs::rename(&root, &parked).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("source.tar.gz"), b"replacement").unwrap();

        let mut opened = guard.open_child("source.tar.gz", "source_archive").unwrap();
        let mut bytes = Vec::new();
        opened.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"trusted");

        fs::rename(&root, temp.path().join("replacement-root")).unwrap();
        fs::rename(&parked, &root).unwrap();
        guard.validate_root_identity().unwrap();
    }
}
