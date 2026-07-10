use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SANDBOX_PATCH_APPROVAL_SUBJECT_SCHEMA: &str = "ao2.sandbox-patch-approval-subject.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPatchApprovalSubject {
    pub schema_version: String,
    pub repository_identity: String,
    pub base_commit: String,
    pub operation_type: String,
    pub operations: Vec<SandboxPatchOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPatchOperation {
    pub order: u32,
    pub path: String,
    pub kind: SandboxPatchOperationKind,
    pub before: Option<SandboxFileState>,
    pub after: Option<SandboxFileState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPatchOperationKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxFileState {
    pub kind: SandboxFileKind,
    pub content_sha256: Option<String>,
    pub symlink_target_sha256: Option<String>,
    pub unix_mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxFileKind {
    RegularFile,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPatchPreview {
    pub target_repo: PathBuf,
    pub sandbox_path: PathBuf,
    pub changed_files: Vec<String>,
    pub diff_summary: String,
    pub approval_subject: SandboxPatchApprovalSubject,
    pub action_digest: String,
}

impl SandboxPatchApprovalSubject {
    pub fn action_digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self)?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

pub fn preview_sandbox_patch(
    target_repo: &Path,
    sandbox_path: &Path,
) -> Result<SandboxPatchPreview> {
    ensure_directory(target_repo, "target repository")?;
    ensure_directory(sandbox_path, "sandbox")?;

    let approval_subject = build_approval_subject(target_repo, sandbox_path)?;
    let action_digest = approval_subject.action_digest()?;
    let changed_files = approval_subject
        .operations
        .iter()
        .map(|operation| operation.path.clone())
        .collect::<Vec<_>>();
    let diff_summary = approval_subject
        .operations
        .iter()
        .map(|operation| {
            let kind = match operation.kind {
                SandboxPatchOperationKind::Added => "added",
                SandboxPatchOperationKind::Modified => "modified",
                SandboxPatchOperationKind::Deleted => "deleted",
            };
            format!("{kind}: {}", operation.path)
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(SandboxPatchPreview {
        target_repo: target_repo.to_path_buf(),
        sandbox_path: sandbox_path.to_path_buf(),
        changed_files,
        diff_summary,
        approval_subject,
        action_digest,
    })
}

fn build_approval_subject(
    target_repo: &Path,
    sandbox_path: &Path,
) -> Result<SandboxPatchApprovalSubject> {
    let (repository_identity, base_commit) = repository_state(target_repo)?;
    let before = snapshot_tree(target_repo)?;
    let after = snapshot_tree(sandbox_path)?;
    let paths = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let operations = paths
        .into_iter()
        .filter_map(|path| {
            let before_state = before.get(&path).cloned();
            let after_state = after.get(&path).cloned();
            let kind = match (&before_state, &after_state) {
                (None, Some(_)) => SandboxPatchOperationKind::Added,
                (Some(_), None) => SandboxPatchOperationKind::Deleted,
                (Some(left), Some(right)) if left != right => SandboxPatchOperationKind::Modified,
                _ => return None,
            };
            Some((path, kind, before_state, after_state))
        })
        .enumerate()
        .map(
            |(order, (path, kind, before, after))| SandboxPatchOperation {
                order: order as u32,
                path,
                kind,
                before,
                after,
            },
        )
        .collect();

    Ok(SandboxPatchApprovalSubject {
        schema_version: SANDBOX_PATCH_APPROVAL_SUBJECT_SCHEMA.to_string(),
        repository_identity,
        base_commit,
        operation_type: "sandbox_patch_apply".to_string(),
        operations,
    })
}

fn repository_state(target_repo: &Path) -> Result<(String, String)> {
    let common_dir_text = git_stdout(target_repo, &["rev-parse", "--git-common-dir"])
        .context("resolve Git common directory")?;
    let common_dir = PathBuf::from(&common_dir_text);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        target_repo.join(common_dir)
    };
    let canonical_common_dir = fs::canonicalize(&common_dir)
        .with_context(|| format!("canonicalize Git common directory {}", common_dir.display()))?;
    let identity_text = canonical_common_dir
        .to_str()
        .context("Git common directory path must be valid UTF-8")?
        .replace('\\', "/");
    let repository_identity = format!("sha256:{}", sha256_hex(identity_text.as_bytes()));

    let base_commit =
        git_stdout(target_repo, &["rev-parse", "HEAD"]).context("resolve Git HEAD base commit")?;
    if !matches!(base_commit.len(), 40 | 64)
        || !base_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(anyhow!("Git HEAD is not a canonical object id"));
    }

    Ok((repository_identity, base_commit))
}

fn git_stdout(target_repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(target_repo)
        .output()
        .with_context(|| format!("run Git command: git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "Git command failed: git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout).context("Git output must be valid UTF-8")?;
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return Err(anyhow!(
            "Git command returned empty output: git {}",
            args.join(" ")
        ));
    }
    Ok(stdout.to_string())
}

fn snapshot_tree(root: &Path) -> Result<BTreeMap<String, SandboxFileState>> {
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let rel_path = entry.path().strip_prefix(root)?;
        if rel_path.as_os_str().is_empty()
            || rel_path.components().any(is_ignored_repo_component)
            || entry.file_type().is_dir()
        {
            continue;
        }
        let path = canonical_relative_path(rel_path)?;
        let metadata = fs::symlink_metadata(entry.path())
            .with_context(|| format!("read snapshot metadata {}", entry.path().display()))?;
        let state = if metadata.file_type().is_file() {
            let bytes = fs::read(entry.path())
                .with_context(|| format!("read snapshot file {}", entry.path().display()))?;
            SandboxFileState {
                kind: SandboxFileKind::RegularFile,
                content_sha256: Some(format!("sha256:{}", sha256_hex(&bytes))),
                symlink_target_sha256: None,
                unix_mode: unix_mode(&metadata),
            }
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(entry.path())
                .with_context(|| format!("read symlink target {}", entry.path().display()))?;
            SandboxFileState {
                kind: SandboxFileKind::Symlink,
                content_sha256: None,
                symlink_target_sha256: Some(format!(
                    "sha256:{}",
                    sha256_hex(&os_str_bytes(target.as_os_str())?)
                )),
                unix_mode: None,
            }
        } else {
            return Err(anyhow!(
                "unsupported sandbox patch entry: {}",
                entry.path().display()
            ));
        };
        files.insert(path, state);
    }
    Ok(files)
}

fn canonical_relative_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .context("sandbox patch path must be valid UTF-8")?;
                if part.is_empty() {
                    return Err(anyhow!("sandbox patch path contains an empty component"));
                }
                parts.push(part);
            }
            _ => {
                return Err(anyhow!(
                    "sandbox patch path must be canonical and relative: {}",
                    path.display()
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(anyhow!("sandbox patch path must not be empty"));
    }
    Ok(parts.join("/"))
}

fn is_ignored_repo_component(component: Component<'_>) -> bool {
    matches!(
        component,
        Component::Normal(name)
            if matches!(
                name.to_str(),
                Some(
                    ".ao2"
                        | ".git"
                        | ".hg"
                        | ".svn"
                        | "target"
                        | "node_modules"
                        | ".venv"
                        | "venv"
                        | "__pycache__"
                        | ".pytest_cache"
                        | ".mypy_cache"
                        | ".ruff_cache"
                        | ".next"
                        | ".expo"
                        | "dist"
                        | "build"
                        | "coverage"
                )
            )
    )
}

fn ensure_directory(path: &Path, label: &str) -> Result<()> {
    if !path.is_dir() {
        return Err(anyhow!("{label} is not a directory: {}", path.display()));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(unix)]
fn os_str_bytes(value: &OsStr) -> Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;

    Ok(value.as_bytes().to_vec())
}

#[cfg(windows)]
fn os_str_bytes(value: &OsStr) -> Result<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;

    Ok(value
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>())
}

#[cfg(not(any(unix, windows)))]
fn os_str_bytes(value: &OsStr) -> Result<Vec<u8>> {
    Ok(value
        .to_str()
        .context("symlink target must be valid UTF-8 on this platform")?
        .as_bytes()
        .to_vec())
}

#[cfg(unix)]
fn unix_mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    Some(metadata.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn unix_mode(_metadata: &fs::Metadata) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_path_rejects_aliases_and_traversal() {
        for alias in ["../escape", "./value.txt", "/absolute", ""] {
            assert!(
                canonical_relative_path(Path::new(alias)).is_err(),
                "{alias}"
            );
        }
        assert_eq!(
            canonical_relative_path(Path::new("src/value.txt")).unwrap(),
            "src/value.txt"
        );
    }
}
