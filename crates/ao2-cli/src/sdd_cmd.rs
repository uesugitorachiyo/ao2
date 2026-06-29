use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use sdd_planner::dispatch::{
    ao2_run_to_runspec, ao_operator_to_runspec, emit_ao2_run_yaml, emit_ao_operator_canonical,
};
use sdd_planner::provider::claude::ClaudeProvider;
use sdd_planner::provider::codex::CodexProvider;
use sdd_planner::{
    canonical_json, orchestrate, scan, shrink, validate, OrchestrateError, Plan, Provider,
    ProviderError, SurfaceMap, DEFAULT_BUDGET_TOKENS,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Subcommand)]
pub enum SddCommand {
    Plan {
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        provider: SddProvider,
        #[arg(
            long = "context-budget-tokens",
            default_value_t = DEFAULT_BUDGET_TOKENS,
            help = "Token budget for the shrunken SDD planning surface map"
        )]
        context_budget_tokens: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Validate {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long = "surface-map")]
        surface_map: Option<PathBuf>,
    },
    Dispatch {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        runner: SddRunner,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SddProvider {
    Codex,
    Claude,
}

impl SddProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SddRunner {
    Ao2,
    AoOperator,
}

pub fn run(command: SddCommand) -> Result<()> {
    match run_inner(command) {
        Ok(()) => Ok(()),
        Err(error) => {
            eprint!("{error}");
            std::process::exit(error.exit_code());
        }
    }
}

fn run_inner(command: SddCommand) -> std::result::Result<(), SddCliError> {
    match command {
        SddCommand::Plan {
            prompt,
            target,
            provider,
            context_budget_tokens,
            out,
        } => sdd_plan(prompt, target, provider, context_budget_tokens, out),
        SddCommand::Validate { plan, surface_map } => sdd_validate(plan, surface_map),
        SddCommand::Dispatch {
            plan,
            runner,
            out,
            dry_run,
        } => sdd_dispatch(plan, runner, out, dry_run),
    }
}

fn sdd_plan(
    prompt: String,
    target: PathBuf,
    provider_kind: SddProvider,
    context_budget_tokens: usize,
    out: Option<PathBuf>,
) -> std::result::Result<(), SddCliError> {
    let prompt_text = read_prompt(&prompt)?;
    let head_sha = git_head_sha(&target).unwrap_or_else(|| "0".repeat(40));
    let surface_map = scan(&target, head_sha).map_err(SddCliError::Io)?;
    let full_surface_map_sha256 = surface_map_sha256(&surface_map)?;
    let shrunken_surface_map = shrink(&surface_map, &prompt_text, context_budget_tokens);
    let shrunken_surface_map_sha256 = surface_map_sha256(&shrunken_surface_map)?;
    let context_metadata = ContextMetadata {
        full_surface_map_sha256,
        shrunken_surface_map_sha256,
        full_file_count: surface_map.files.len(),
        shrunken_file_count: shrunken_surface_map.files.len(),
        context_budget_tokens,
        context_shrink_enabled: true,
    };
    let build_log_root = target.join("target").join("sdd-planner");

    let provider: Box<dyn Provider> = match provider_kind {
        SddProvider::Codex => Box::new(CodexProvider::new()),
        SddProvider::Claude => Box::new(ClaudeProvider::new()),
    };

    let outcome = orchestrate(
        provider.as_ref(),
        &prompt_text,
        &shrunken_surface_map,
        &build_log_root,
        &target,
        provider_kind.as_str(),
    )
    .map_err(SddCliError::Orchestrate)?;

    write_context_metadata(
        &outcome.build_log_dir.join("context.json"),
        &context_metadata,
    )?;

    if let Some(path) = out {
        write_text(&path, &outcome.canonical_json)?;
    } else {
        println!("{}", outcome.canonical_json);
    }
    println!("plan_id={}", outcome.plan_id);
    println!("build_log_dir={}", outcome.build_log_dir.display());
    println!("attempts_used={}", outcome.attempts_used);
    println!(
        "full_surface_map_sha256={}",
        context_metadata.full_surface_map_sha256
    );
    println!(
        "shrunken_surface_map_sha256={}",
        context_metadata.shrunken_surface_map_sha256
    );
    println!("full_file_count={}", context_metadata.full_file_count);
    println!(
        "shrunken_file_count={}",
        context_metadata.shrunken_file_count
    );
    println!(
        "context_budget_tokens={}",
        context_metadata.context_budget_tokens
    );
    println!(
        "context_shrink_enabled={}",
        context_metadata.context_shrink_enabled
    );
    Ok(())
}

fn sdd_validate(
    plan_path: PathBuf,
    surface_map_path: Option<PathBuf>,
) -> std::result::Result<(), SddCliError> {
    let plan_text = fs::read_to_string(&plan_path).map_err(SddCliError::Io)?;
    let surface_map = read_surface_map(surface_map_path.as_deref())?;
    let report = validate(&plan_text, surface_map.as_ref());
    if report.is_pass() {
        println!("PASS");
        return Ok(());
    }
    Err(SddCliError::Validation(report.render()))
}

fn sdd_dispatch(
    plan_path: PathBuf,
    runner: SddRunner,
    out: PathBuf,
    dry_run: bool,
) -> std::result::Result<(), SddCliError> {
    let plan_text = fs::read_to_string(&plan_path).map_err(SddCliError::Io)?;
    let report = validate(&plan_text, None);
    if !report.is_pass() {
        return Err(SddCliError::Validation(report.render()));
    }
    let plan = report
        .plan
        .or_else(|| serde_json::from_str::<Plan>(&plan_text).ok())
        .ok_or_else(|| SddCliError::Other("validated SDD plan was not available".to_string()))?;

    match runner {
        SddRunner::Ao2 => {
            let runspec = ao2_run_to_runspec(&plan);
            let yaml = emit_ao2_run_yaml(&runspec);
            write_text(&out, &yaml)?;
            if dry_run {
                dry_run_ao2(&out)?;
            }
        }
        SddRunner::AoOperator => {
            let runspec = ao_operator_to_runspec(&plan);
            let canonical = emit_ao_operator_canonical(&runspec);
            write_text(&out, &canonical)?;
        }
    }
    println!("out={}", out.display());
    Ok(())
}

fn read_prompt(prompt: &str) -> std::result::Result<String, SddCliError> {
    if let Some(path) = prompt.strip_prefix('@') {
        return fs::read_to_string(path).map_err(SddCliError::Io);
    }
    Ok(prompt.to_string())
}

fn read_surface_map(path: Option<&Path>) -> std::result::Result<Option<SurfaceMap>, SddCliError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = fs::read_to_string(path).map_err(SddCliError::Io)?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(SddCliError::Serde)
}

fn write_text(path: &Path, text: &str) -> std::result::Result<(), SddCliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(SddCliError::Io)?;
    }
    fs::write(path, text).map_err(SddCliError::Io)
}

#[derive(Debug, Serialize)]
struct ContextMetadata {
    full_surface_map_sha256: String,
    shrunken_surface_map_sha256: String,
    full_file_count: usize,
    shrunken_file_count: usize,
    context_budget_tokens: usize,
    context_shrink_enabled: bool,
}

fn write_context_metadata(
    path: &Path,
    metadata: &ContextMetadata,
) -> std::result::Result<(), SddCliError> {
    let value = serde_json::to_value(metadata).map_err(SddCliError::Serde)?;
    write_text(path, &canonical_json(&value))
}

fn surface_map_sha256(surface_map: &SurfaceMap) -> std::result::Result<String, SddCliError> {
    let value = serde_json::to_value(surface_map).map_err(SddCliError::Serde)?;
    let canonical = canonical_json(&value);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    Ok(hex_lower(hasher.finalize()))
}

fn hex_lower(bytes: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02x}").expect("write to string");
    }
    out
}

fn git_head_sha(target: &Path) -> Option<String> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(target)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.len() == 40 {
        Some(sha)
    } else {
        None
    }
}

fn dry_run_ao2(out: &Path) -> std::result::Result<(), SddCliError> {
    let status = ProcessCommand::new(std::env::current_exe().map_err(SddCliError::Io)?)
        .args(["run", "--dry-run", "--spec"])
        .arg(out)
        .status()
        .map_err(SddCliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(SddCliError::DryRunFailed(status.code()))
    }
}

#[derive(Debug)]
enum SddCliError {
    Validation(String),
    Orchestrate(OrchestrateError),
    Io(std::io::Error),
    Serde(serde_json::Error),
    DryRunFailed(Option<i32>),
    Other(String),
}

impl SddCliError {
    fn exit_code(&self) -> i32 {
        match self {
            SddCliError::Validation(_) => 2,
            SddCliError::Orchestrate(OrchestrateError::PlanExhausted { .. }) => 3,
            SddCliError::Orchestrate(OrchestrateError::Provider(_)) => 4,
            SddCliError::Orchestrate(OrchestrateError::Io(_))
            | SddCliError::Orchestrate(OrchestrateError::Serde(_))
            | SddCliError::Io(_)
            | SddCliError::Serde(_)
            | SddCliError::DryRunFailed(_)
            | SddCliError::Other(_) => 5,
        }
    }
}

impl fmt::Display for SddCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SddCliError::Validation(report) => f.write_str(report),
            SddCliError::Orchestrate(error) => match error {
                OrchestrateError::Provider(provider) => write_provider_error(f, provider),
                _ => writeln!(f, "{error}"),
            },
            SddCliError::Io(error) => writeln!(f, "I/O error: {error}"),
            SddCliError::Serde(error) => writeln!(f, "serde error: {error}"),
            SddCliError::DryRunFailed(code) => {
                writeln!(f, "ao2 dry-run failed with exit code {code:?}")
            }
            SddCliError::Other(message) => writeln!(f, "{message}"),
        }
    }
}

fn write_provider_error(f: &mut fmt::Formatter<'_>, error: &ProviderError) -> fmt::Result {
    writeln!(f, "{error}")
}
