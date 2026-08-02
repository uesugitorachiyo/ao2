mod executor;
mod hooks;
mod manifest;
mod snapshot;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

use crate::cli_util::atomic_write_text;
use executor::execute;
use manifest::load_manifest;
use snapshot::build_snapshot;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum QualityLevel {
    Commit,
    Push,
    Full,
}

impl QualityLevel {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Push => "push",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum QualityCommand {
    Check {
        #[arg(value_enum)]
        level: QualityLevel,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Hooks {
        #[command(subcommand)]
        command: QualityHooksCommand,
    },
    #[command(hide = true)]
    HookRun {
        #[arg(value_enum)]
        hook: QualityHook,
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum QualityHooksCommand {
    Install {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Status {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum QualityHook {
    Commit,
    Push,
}

#[derive(Debug, Args)]
pub(crate) struct QualityArgs {
    #[command(subcommand)]
    pub command: QualityCommand,
}

#[derive(Debug, Serialize)]
pub(super) struct QualityCheckResult {
    schema_version: &'static str,
    status: &'static str,
    repository: String,
    level: &'static str,
    manifest_path: &'static str,
    manifest_sha256: String,
    source_head: String,
    snapshot: snapshot::QualitySnapshot,
    selection_status: &'static str,
    selected_steps: Vec<executor::SelectedStep>,
    steps: Vec<executor::StepResult>,
    duration_ms: u64,
    source_mutation_detected: bool,
    provider_calls: u64,
    credential_environment_scrubbed: bool,
    failure_codes: Vec<String>,
}

pub(crate) fn quality(command: QualityCommand) -> Result<()> {
    match command {
        QualityCommand::Check {
            level,
            target,
            manifest,
            base,
            out,
            json,
        } => quality_check(level, target, manifest, base, out, json),
        QualityCommand::Hooks { command } => hooks::hooks(command),
        QualityCommand::HookRun { hook, target } => hooks::hook_run(hook, target),
    }
}

pub(super) fn quality_check(
    level: QualityLevel,
    target: PathBuf,
    manifest: Option<PathBuf>,
    base: Option<String>,
    out: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let target = target
        .canonicalize()
        .with_context(|| format!("[TARGET_INVALID] cannot resolve {}", target.display()))?;
    let manifest_path = match manifest {
        Some(path) if path.is_absolute() => path,
        Some(path) => target.join(path),
        None => target.join("ao-quality-gates.json"),
    };
    let loaded = load_manifest(&target, &manifest_path)?;
    let artifact_root = loaded.manifest.evidence.local_artifact_root.clone();
    let out = out
        .map(|path| resolve_result_path(&target, &artifact_root, path))
        .transpose()?;
    let snapshot = build_snapshot(&target, level, base.as_deref())?;
    let maximum_result_bytes = loaded.manifest.evidence.maximum_result_bytes;
    let result = execute(&target, level, loaded, snapshot)?;
    let encoded = serde_json::to_vec_pretty(&result).context("encode quality result")?;
    if encoded.len() > maximum_result_bytes {
        bail!("[RESULT_SIZE_LIMIT] quality result exceeds manifest evidence limit");
    }
    if let Some(path) = out {
        if path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!("[RESULT_SYMLINK] refusing to replace symlinked result path");
        }
        atomic_write_text(&path, &String::from_utf8_lossy(&encoded))?;
    }
    if json {
        println!("{}", String::from_utf8_lossy(&encoded));
    } else {
        println!(
            "quality {}: {} ({} selected, {} executed)",
            result.level,
            result.status,
            result.selected_steps.len(),
            result.steps.len()
        );
    }
    if result.status != "passed" {
        bail!("[QUALITY_GATE_FAILED] {} quality gate failed", result.level);
    }
    Ok(())
}

fn resolve_result_path(target: &Path, artifact_root: &str, requested: PathBuf) -> Result<PathBuf> {
    if requested
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("[RESULT_PATH_UNSAFE] result path must not contain parent traversal");
    }
    let path = if requested.is_absolute() {
        requested
    } else {
        target.join(requested)
    };
    if path.starts_with(target) && !path.starts_with(target.join(artifact_root)) {
        bail!("[RESULT_PATH_OUTSIDE_ARTIFACT_ROOT] in-repository results must use the declared artifact root");
    }
    for ancestor in path.ancestors().take_while(|ancestor| *ancestor != target) {
        if ancestor
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!("[RESULT_PATH_SYMLINK] result path must not traverse a symlink");
        }
    }
    Ok(path)
}
