use anyhow::{bail, Context, Result};
use ao2_policy::redact_secrets;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::{Command, Output};

use super::QualityLevel;

const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CHANGED_PATHS: usize = 100_000;
const MAX_OUTGOING_COMMITS: usize = 10_000;

#[derive(Debug, Clone, Serialize)]
pub(super) struct QualitySnapshot {
    pub kind: &'static str,
    pub sha256: String,
    pub head_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree_sha: Option<String>,
    pub outgoing_commits: Vec<String>,
    pub changed_paths: Vec<String>,
}

#[derive(Serialize)]
struct SnapshotDigest<'a> {
    schema_version: &'static str,
    kind: &'static str,
    head_sha: &'a str,
    base_sha: &'a Option<String>,
    index_sha256: &'a Option<String>,
    tree_sha: &'a Option<String>,
    outgoing_commits: &'a [String],
    changed_paths: &'a [String],
}

pub(super) fn build_snapshot(
    target: &Path,
    level: QualityLevel,
    requested_base: Option<&str>,
) -> Result<QualitySnapshot> {
    let head_sha = git_text(target, &["rev-parse", "--verify", "HEAD^{commit}"])
        .context("[SOURCE_HEAD_INVALID] resolve HEAD")?;
    match level {
        QualityLevel::Commit => staged_snapshot(target, head_sha),
        QualityLevel::Push => outgoing_snapshot(target, head_sha, requested_base),
        QualityLevel::Full => full_snapshot(target, head_sha),
    }
}

fn staged_snapshot(target: &Path, head_sha: String) -> Result<QualitySnapshot> {
    let index = git_bytes(target, &["ls-files", "--stage", "-z"])
        .context("[STAGED_TREE_INVALID] read Git index")?;
    reject_unmerged_index(&index)?;
    let index_sha256 = Some(format!("sha256:{:x}", Sha256::digest(&index)));
    let changed_paths = git_paths(
        target,
        &[
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--diff-filter=ACDMRTUXB",
            "--no-ext-diff",
            "--no-renames",
            "HEAD",
            "--",
        ],
    )
    .context("[STAGED_PATHS_INVALID] read cached changed paths")?;
    finish_snapshot(
        "staged_tree",
        head_sha,
        None,
        index_sha256,
        None,
        Vec::new(),
        changed_paths,
    )
}

fn outgoing_snapshot(
    target: &Path,
    head_sha: String,
    requested_base: Option<&str>,
) -> Result<QualitySnapshot> {
    let base_ref = match requested_base {
        Some(value) if !value.is_empty() && value.len() <= 256 && !value.contains('\0') => value,
        Some(_) => bail!("[PUSH_BASE_INVALID] push base is empty or oversized"),
        None => "@{upstream}",
    };
    let peeled = format!("{base_ref}^{{commit}}");
    let base_sha = git_text(target, &["rev-parse", "--verify", &peeled])
        .context("[PUSH_BASE_INVALID] resolve push base")?;
    let ancestry = git_output(
        target,
        &["merge-base", "--is-ancestor", &base_sha, &head_sha],
    )?;
    if !ancestry.status.success() {
        bail!("[PUSH_BASE_NOT_ANCESTOR] push base is not an ancestor of HEAD");
    }
    let range = format!("{base_sha}..{head_sha}");
    let outgoing_commits = git_lines(target, &["rev-list", "--reverse", &range])?;
    if outgoing_commits.len() > MAX_OUTGOING_COMMITS {
        bail!("[OUTGOING_COMMIT_LIMIT] outgoing commit count exceeds {MAX_OUTGOING_COMMITS}");
    }
    let changed_paths = git_paths(
        target,
        &[
            "diff",
            "--name-only",
            "-z",
            "--diff-filter=ACDMRTUXB",
            "--no-ext-diff",
            "--no-renames",
            &base_sha,
            &head_sha,
            "--",
        ],
    )
    .context("[OUTGOING_PATHS_INVALID] read outgoing changed paths")?;
    finish_snapshot(
        "outgoing_commits",
        head_sha,
        Some(base_sha),
        None,
        None,
        outgoing_commits,
        changed_paths,
    )
}

fn full_snapshot(target: &Path, head_sha: String) -> Result<QualitySnapshot> {
    let tree_sha = git_text(target, &["rev-parse", "--verify", "HEAD^{tree}"])
        .context("[SOURCE_TREE_INVALID] resolve HEAD tree")?;
    finish_snapshot(
        "source_head",
        head_sha,
        None,
        None,
        Some(tree_sha),
        Vec::new(),
        Vec::new(),
    )
}

fn finish_snapshot(
    kind: &'static str,
    head_sha: String,
    base_sha: Option<String>,
    index_sha256: Option<String>,
    tree_sha: Option<String>,
    outgoing_commits: Vec<String>,
    changed_paths: Vec<String>,
) -> Result<QualitySnapshot> {
    let digest_input = SnapshotDigest {
        schema_version: "ao2.quality-snapshot.v1",
        kind,
        head_sha: &head_sha,
        base_sha: &base_sha,
        index_sha256: &index_sha256,
        tree_sha: &tree_sha,
        outgoing_commits: &outgoing_commits,
        changed_paths: &changed_paths,
    };
    let encoded = serde_json::to_vec(&digest_input).context("encode quality snapshot")?;
    Ok(QualitySnapshot {
        kind,
        sha256: format!("sha256:{:x}", Sha256::digest(encoded)),
        head_sha,
        base_sha,
        index_sha256,
        tree_sha,
        outgoing_commits,
        changed_paths,
    })
}

pub(super) fn git_state(target: &Path) -> Result<String> {
    let status = git_bytes(
        target,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    )?;
    let index = git_bytes(target, &["ls-files", "--stage", "-z"])?;
    let mut digest = Sha256::new();
    digest.update(b"ao2.quality-git-state.v1\0");
    digest.update((status.len() as u64).to_be_bytes());
    digest.update(&status);
    digest.update((index.len() as u64).to_be_bytes());
    digest.update(&index);
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn reject_unmerged_index(index: &[u8]) -> Result<()> {
    for entry in index
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let tab = entry
            .iter()
            .position(|byte| *byte == b'\t')
            .context("[STAGED_TREE_INVALID] malformed index entry")?;
        let metadata = std::str::from_utf8(&entry[..tab])
            .context("[STAGED_TREE_INVALID] index metadata is not UTF-8")?;
        let stage = metadata
            .split_ascii_whitespace()
            .nth(2)
            .context("[STAGED_TREE_INVALID] index stage is missing")?;
        if stage != "0" {
            bail!("[STAGED_TREE_UNMERGED] staged tree contains unmerged entries");
        }
    }
    Ok(())
}

fn git_paths(target: &Path, args: &[&str]) -> Result<Vec<String>> {
    let bytes = git_bytes(target, args)?;
    let mut paths = Vec::new();
    for raw in bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = std::str::from_utf8(raw)
            .context("[GIT_PATH_UTF8_REQUIRED] changed Git path is not UTF-8")?;
        if path.starts_with('/') || path.split('/').any(|part| part == "..") || path.contains('\\')
        {
            bail!("[GIT_PATH_UNSAFE] changed Git path is unsafe");
        }
        paths.push(path.to_string());
    }
    paths.sort();
    paths.dedup();
    if paths.len() > MAX_CHANGED_PATHS {
        bail!("[CHANGED_PATH_LIMIT] changed path count exceeds {MAX_CHANGED_PATHS}");
    }
    Ok(paths)
}

fn git_lines(target: &Path, args: &[&str]) -> Result<Vec<String>> {
    let text = git_text(target, args)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    Ok(text.lines().map(str::to_string).collect())
}

fn git_text(target: &Path, args: &[&str]) -> Result<String> {
    let bytes = git_bytes(target, args)?;
    Ok(std::str::from_utf8(&bytes)
        .context("[GIT_OUTPUT_UTF8_REQUIRED] Git output is not UTF-8")?
        .trim()
        .to_string())
}

fn git_bytes(target: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = git_output(target, args)?;
    if !output.status.success() {
        bail!(
            "[GIT_COMMAND_FAILED] git {:?} exited {:?}: {}",
            args,
            output.status.code(),
            redact_secrets(&String::from_utf8_lossy(&output.stderr)).trim()
        );
    }
    if output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        bail!("[GIT_OUTPUT_SIZE_LIMIT] Git output exceeds {MAX_GIT_OUTPUT_BYTES} bytes");
    }
    Ok(output.stdout)
}

fn git_output(target: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "diff.external=",
            "-c",
            "submodule.recurse=false",
        ])
        .args(args)
        .current_dir(target)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("[GIT_COMMAND_UNAVAILABLE] execute Git")
}
