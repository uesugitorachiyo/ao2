//! Local agent adapter contract and process wrapper.

mod sandbox_patch;

pub use sandbox_patch::{
    preview_sandbox_patch, SandboxFileKind, SandboxFileState, SandboxPatchApprovalSubject,
    SandboxPatchOperation, SandboxPatchOperationKind, SandboxPatchPreview,
    SANDBOX_PATCH_APPROVAL_SUBJECT_SCHEMA,
};

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use std::{env, fs};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    Claude,
    Antigravity,
    Scripted,
}

pub const DEFAULT_PROVIDER_TIMEOUT_SECONDS: u64 = 900;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRunRequest {
    pub role_id: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub stdin: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxRunRequest {
    pub target_repo: PathBuf,
    pub request: AdapterRunRequest,
    pub keep_sandbox: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxRunResult {
    pub adapter: AdapterRunResult,
    pub target_repo: PathBuf,
    pub sandbox_path: PathBuf,
    pub changed_files: Vec<String>,
    pub diff_summary: String,
    pub transcript_summary: ProviderTranscriptSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPatchApplyRequest {
    pub target_repo: PathBuf,
    pub sandbox_path: PathBuf,
    pub expected_digest: String,
    pub approver: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPatchApplyResult {
    pub target_repo: PathBuf,
    pub sandbox_path: PathBuf,
    pub applied_files: Vec<String>,
    pub approval_subject: SandboxPatchApprovalSubject,
    pub action_digest: String,
    pub approver: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPromptRequest {
    pub provider: ProviderKind,
    pub target_repo: PathBuf,
    pub prompt: String,
    pub role_id: String,
    pub keep_sandbox: bool,
    pub timeout_ms: Option<u64>,
    pub max_budget_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRunResult {
    pub provider: ProviderKind,
    pub role_id: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub transcript: String,
    pub blocker: Option<AdapterBlocker>,
}

impl AdapterRunResult {
    pub fn scripted(role_id: impl Into<String>, transcript: impl Into<String>) -> Self {
        Self {
            provider: ProviderKind::Scripted,
            role_id: role_id.into(),
            command: "scripted://deterministic-role".to_string(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            transcript: transcript.into(),
            blocker: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterBlocker {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct LocalCliAdapter {
    provider: ProviderKind,
}

const ADAPTER_WORKING_DIR_PLACEHOLDER: &str = "{ao2_adapter_working_dir}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDoctorReport {
    pub provider: ProviderKind,
    pub available: bool,
    pub command: String,
    pub doctor_args: Vec<String>,
    pub metadata_source: String,
    pub version: String,
    pub blocker: Option<AdapterBlocker>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub provider: ProviderKind,
    pub provider_name: String,
    pub metadata_source: String,
    pub description: String,
    pub adapter_kind: String,
    pub registry_phase: String,
    pub smoke_script: String,
    pub smoke_guard_env: Option<String>,
    pub pilot_guard_env: Option<String>,
    pub doctor_command: String,
    pub doctor_args: Vec<String>,
    pub transcript_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderTranscriptSummary {
    pub provider: ProviderKind,
    pub changed_files: Vec<String>,
    pub concerns: Vec<ProviderConcern>,
    pub blockers: Vec<AdapterBlocker>,
    pub usage: ProviderUsage,
    pub cost_usd: Option<f64>,
    pub transcript_ids: Vec<ProviderTranscriptId>,
    pub raw_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderTranscriptId {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConcern {
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl LocalCliAdapter {
    pub fn new(provider: ProviderKind) -> Self {
        Self { provider }
    }

    pub fn run(&self, request: AdapterRunRequest) -> Result<AdapterRunResult> {
        let command_path = resolve_adapter_command(&request.command);
        let args = expand_adapter_args(&request.args, &request.working_dir);
        let mut command = adapter_process_command(&command_path, &args);
        let mut child = command
            .current_dir(&request.working_dir)
            .stdin(if request.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn adapter command {}", command_path.display()))?;

        let stdout_pipe = child.stdout.take().context("open adapter stdout")?;
        let stderr_pipe = child.stderr.take().context("open adapter stderr")?;
        let stdout_handle = thread::spawn(move || read_pipe(stdout_pipe));
        let stderr_handle = thread::spawn(move || read_pipe(stderr_pipe));

        if let Some(stdin) = request.stdin {
            let mut child_stdin = child.stdin.take().context("open adapter stdin")?;
            child_stdin
                .write_all(stdin.as_bytes())
                .context("write adapter stdin")?;
        }

        let timeout = request.timeout_ms.map(Duration::from_millis);
        let started = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait().context("poll adapter command status")? {
                break status;
            }
            if let Some(timeout) = timeout {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().context("wait for killed adapter command")?;
                }
            }
            thread::sleep(Duration::from_millis(25));
        };
        let stdout = redact_sensitive_text(
            &join_pipe_reader(stdout_handle).context("join adapter stdout reader")?,
        );
        let stderr = redact_sensitive_text(
            &join_pipe_reader(stderr_handle).context("join adapter stderr reader")?,
        );
        let command =
            redact_sensitive_text(format!("{} {}", command_path.display(), args.join(" ")).trim());
        let exit_code = status.code();
        let blocker = if timed_out {
            Some(AdapterBlocker {
                kind: "timeout".to_string(),
                message: format!(
                    "adapter command exceeded timeout of {}ms",
                    request.timeout_ms.unwrap_or_default()
                ),
            })
        } else if status.success() {
            None
        } else {
            Some(AdapterBlocker {
                kind: "non_zero_exit".to_string(),
                message: format!("adapter command exited with {:?}", exit_code),
            })
        };
        let transcript = format!(
            "provider: {:?}\nrole_id: {}\ncommand: {}\nexit_code: {:?}\ntimeout_ms: {:?}\ntimed_out: {}\nstdout:\n{}\nstderr:\n{}",
            self.provider,
            request.role_id,
            command,
            exit_code,
            request.timeout_ms,
            timed_out,
            stdout,
            stderr
        );

        Ok(AdapterRunResult {
            provider: self.provider,
            role_id: request.role_id,
            command,
            exit_code,
            stdout,
            stderr,
            transcript,
            blocker,
        })
    }

    pub fn run_in_sandbox(&self, request: SandboxRunRequest) -> Result<SandboxRunResult> {
        ensure_target_repo(&request.target_repo)?;
        let before = snapshot_files(&request.target_repo)?;
        let sandbox_path = create_sandbox_path(&request.target_repo);
        copy_dir_recursive(&request.target_repo, &sandbox_path)?;

        let mut adapter_request = request.request;
        adapter_request.working_dir =
            resolve_sandbox_working_dir(&sandbox_path, &adapter_request.working_dir)?;

        let adapter = self.run(adapter_request)?;
        let after = snapshot_files(&sandbox_path)?;
        let (changed_files, diff_summary) = summarize_diff(&before, &after);
        let transcript_summary =
            parse_provider_transcript(self.provider, &adapter.transcript, &changed_files);

        if !request.keep_sandbox {
            fs::remove_dir_all(&sandbox_path)
                .with_context(|| format!("remove sandbox {}", sandbox_path.display()))?;
        }

        Ok(SandboxRunResult {
            adapter,
            target_repo: request.target_repo,
            sandbox_path,
            changed_files,
            diff_summary,
            transcript_summary,
        })
    }
}

pub fn doctor_provider(provider: ProviderKind) -> Result<ProviderDoctorReport> {
    let metadata = provider_metadata(provider);
    match provider {
        ProviderKind::Scripted => Ok(ProviderDoctorReport {
            provider,
            available: true,
            command: "built-in".to_string(),
            doctor_args: metadata.doctor_args,
            metadata_source: metadata.metadata_source,
            version: "built-in scripted provider".to_string(),
            blocker: None,
        }),
        ProviderKind::Codex | ProviderKind::Claude | ProviderKind::Antigravity => {
            doctor_external(provider, metadata)
        }
    }
}

pub fn provider_metadata(provider: ProviderKind) -> ProviderMetadata {
    match provider {
        ProviderKind::Scripted => ProviderMetadata {
            provider,
            provider_name: "scripted".to_string(),
            metadata_source: "ao2-adapters".to_string(),
            description: "Deterministic local provider for smoke tests and fixtures.".to_string(),
            adapter_kind: "built_in_deterministic".to_string(),
            registry_phase: "phase_0_complete".to_string(),
            smoke_script: "scripts/smoke-release-archives.sh".to_string(),
            smoke_guard_env: None,
            pilot_guard_env: None,
            doctor_command: "built-in".to_string(),
            doctor_args: Vec::new(),
            transcript_fields: provider_transcript_fields(),
        },
        ProviderKind::Codex => codex_provider_metadata(),
        ProviderKind::Claude => claude_provider_metadata(),
        ProviderKind::Antigravity => ProviderMetadata {
            provider,
            provider_name: "antigravity".to_string(),
            metadata_source: "ao2-adapters".to_string(),
            description: "Google Antigravity CLI OAuth provider for implementation roles."
                .to_string(),
            adapter_kind: "local_oauth_cli".to_string(),
            registry_phase: "phase_1_guarded_live_pilot".to_string(),
            smoke_script: "scripts/smoke-antigravity-provider-pilot.sh".to_string(),
            smoke_guard_env: Some("AO2_LIVE_ANTIGRAVITY_SMOKE".to_string()),
            pilot_guard_env: Some("AO2_LIVE_ANTIGRAVITY_PILOT".to_string()),
            doctor_command: "agy".to_string(),
            doctor_args: vec!["--version".to_string()],
            transcript_fields: provider_transcript_fields(),
        },
    }
}

fn codex_provider_metadata() -> ProviderMetadata {
    let metadata = ao2_adapter_codex::metadata();
    ProviderMetadata {
        provider: ProviderKind::Codex,
        provider_name: metadata.provider_name.to_string(),
        metadata_source: metadata.metadata_source.to_string(),
        description: metadata.description.to_string(),
        adapter_kind: metadata.adapter_kind.to_string(),
        registry_phase: metadata.registry_phase.to_string(),
        smoke_script: metadata.smoke_script.to_string(),
        smoke_guard_env: Some(metadata.smoke_guard_env.to_string()),
        pilot_guard_env: Some(metadata.pilot_guard_env.to_string()),
        doctor_command: metadata.doctor_command.to_string(),
        doctor_args: metadata
            .doctor_args
            .iter()
            .map(|arg| arg.to_string())
            .collect(),
        transcript_fields: metadata
            .transcript_fields
            .iter()
            .map(|field| field.to_string())
            .collect(),
    }
}

fn claude_provider_metadata() -> ProviderMetadata {
    let metadata = ao2_adapter_claude::metadata();
    ProviderMetadata {
        provider: ProviderKind::Claude,
        provider_name: metadata.provider_name.to_string(),
        metadata_source: metadata.metadata_source.to_string(),
        description: metadata.description.to_string(),
        adapter_kind: metadata.adapter_kind.to_string(),
        registry_phase: metadata.registry_phase.to_string(),
        smoke_script: metadata.smoke_script.to_string(),
        smoke_guard_env: Some(metadata.smoke_guard_env.to_string()),
        pilot_guard_env: Some(metadata.pilot_guard_env.to_string()),
        doctor_command: metadata.doctor_command.to_string(),
        doctor_args: metadata
            .doctor_args
            .iter()
            .map(|arg| arg.to_string())
            .collect(),
        transcript_fields: metadata
            .transcript_fields
            .iter()
            .map(|field| field.to_string())
            .collect(),
    }
}

fn provider_transcript_fields() -> Vec<String> {
    [
        "changed_files",
        "concerns",
        "blockers",
        "usage",
        "cost_usd",
        "raw_summary",
    ]
    .iter()
    .map(|field| field.to_string())
    .collect()
}

pub fn parse_provider(input: &str) -> Result<ProviderKind> {
    match input {
        "codex" => Ok(ProviderKind::Codex),
        "claude" => Ok(ProviderKind::Claude),
        "antigravity" => Ok(ProviderKind::Antigravity),
        "scripted" => Ok(ProviderKind::Scripted),
        _ => anyhow::bail!(
            "unknown provider: {input}; expected codex, claude, antigravity, or scripted"
        ),
    }
}

pub fn build_provider_prompt_command(
    provider: ProviderKind,
    prompt: &str,
    role_id: &str,
    timeout_ms: Option<u64>,
    max_budget_usd: Option<f64>,
) -> Result<AdapterRunRequest> {
    let max_budget_usd_arg = provider_budget_arg(max_budget_usd)?;
    Ok(match provider {
        ProviderKind::Scripted => AdapterRunRequest {
            role_id: role_id.to_string(),
            command: scripted_shell_command(prompt),
            args: scripted_shell_args(prompt),
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms,
        },
        ProviderKind::Codex => AdapterRunRequest {
            role_id: role_id.to_string(),
            command: PathBuf::from("codex"),
            args: ao2_adapter_codex::build_args(prompt),
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms,
        },
        ProviderKind::Claude => AdapterRunRequest {
            role_id: role_id.to_string(),
            command: PathBuf::from("claude"),
            args: ao2_adapter_claude::build_args(prompt, max_budget_usd_arg),
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms,
        },
        ProviderKind::Antigravity => AdapterRunRequest {
            role_id: role_id.to_string(),
            command: PathBuf::from("agy"),
            args: vec![
                "--add-dir".to_string(),
                ADAPTER_WORKING_DIR_PLACEHOLDER.to_string(),
                "--print".to_string(),
                antigravity_sandbox_execution_prompt(prompt),
                "--sandbox".to_string(),
                "--print-timeout".to_string(),
                "5m".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms,
        },
    })
}

fn expand_adapter_args(args: &[String], working_dir: &Path) -> Vec<String> {
    let working_dir = working_dir.display().to_string();
    args.iter()
        .map(|arg| {
            if arg == ADAPTER_WORKING_DIR_PLACEHOLDER {
                working_dir.clone()
            } else {
                arg.clone()
            }
        })
        .collect()
}

pub fn antigravity_sandbox_execution_prompt(prompt: &str) -> String {
    format!(
        r#"You are running inside an AO2 disposable sandbox copy of the target repository.
Complete the requested coding task in the current repository using your tools (e.g. by editing/writing files or running commands). If the task contains shell script or commands, execute them exactly using terminal tools. Do not just describe them or answer with text.
Do not ask follow-up questions.
Do not edit files outside the current repository.
After the task finishes, print a concise completion report that includes:
Summary: <short summary of the completed change>
Changed files: <comma-separated files changed>
Concern: <severity - message, only if any>
Blocker: <message, only if any>

Task:
{prompt}
"#
    )
}

fn provider_budget_arg(max_budget_usd: Option<f64>) -> Result<Option<String>> {
    let Some(max_budget_usd) = max_budget_usd else {
        return Ok(None);
    };
    if !max_budget_usd.is_finite() || max_budget_usd <= 0.0 {
        anyhow::bail!("provider max budget USD must be a positive finite number");
    }
    Ok(Some(format!("{max_budget_usd:.2}")))
}

pub fn run_provider_prompt_in_sandbox(request: ProviderPromptRequest) -> Result<SandboxRunResult> {
    let adapter_request = build_provider_prompt_command(
        request.provider,
        &request.prompt,
        &request.role_id,
        request.timeout_ms,
        request.max_budget_usd,
    )?;
    LocalCliAdapter::new(request.provider).run_in_sandbox(SandboxRunRequest {
        target_repo: request.target_repo,
        request: adapter_request,
        keep_sandbox: request.keep_sandbox,
    })
}

fn read_pipe<R: Read>(mut pipe: R) -> String {
    let mut bytes = Vec::new();
    let _ = pipe.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).to_string()
}

fn join_pipe_reader(handle: thread::JoinHandle<String>) -> Result<String> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("adapter pipe reader panicked"))
}

fn redact_sensitive_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line_contains_sensitive_material(line) {
                "[redacted sensitive line]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_contains_sensitive_material(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("openai_api_key")
        || lower.contains("anthropic_api_key")
        || lower.contains("ao2_cp_api_token")
        || lower.contains("authorization: bearer")
        || lower.contains("bearer ")
        || lower.contains("cookie:")
        || lower.contains("set-cookie:")
        || lower.contains("password=")
        || lower.contains("password:")
        || lower.contains("secret=")
        || lower.contains("secret:")
        || lower.contains("api_token=")
        || lower.contains("api_token:")
        || lower.contains("access_token=")
        || lower.contains("access_token:")
        || lower.contains("refresh_token=")
        || lower.contains("refresh_token:")
        || lower.contains("?token=")
        || lower.contains("&token=")
        || lower.contains("token=")
        || lower.contains("token:")
        || lower.contains("--api-token")
        || lower.contains("--operator-token")
        || lower.contains("api-token=")
        || lower.contains("api-token:")
        || contains_sensitive_colon_token(&lower)
}

fn contains_sensitive_colon_token(lower: &str) -> bool {
    lower
        .split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
                )
        })
        .any(|token| {
            let token = token.trim_matches(['.', ':']);
            let mut parts = token.split(':');
            let Some(_first) = parts.next() else {
                return false;
            };
            let Some(_second) = parts.next() else {
                return false;
            };
            parts.any(|part| {
                part.contains("secret")
                    || part.contains("token")
                    || part.contains("password")
                    || part.contains("api-key")
                    || part.contains("api_key")
            })
        })
}

pub fn parse_provider_transcript(
    provider: ProviderKind,
    transcript: &str,
    sandbox_changed_files: &[String],
) -> ProviderTranscriptSummary {
    match provider {
        ProviderKind::Codex => from_codex_transcript_summary(ao2_adapter_codex::parse_transcript(
            transcript,
            sandbox_changed_files,
        )),
        ProviderKind::Claude => from_claude_transcript_summary(
            ao2_adapter_claude::parse_transcript(transcript, sandbox_changed_files),
        ),
        ProviderKind::Antigravity => {
            parse_scripted_provider_transcript(provider, transcript, sandbox_changed_files)
        }
        ProviderKind::Scripted => {
            parse_scripted_provider_transcript(provider, transcript, sandbox_changed_files)
        }
    }
}

fn from_codex_transcript_summary(
    summary: ao2_adapter_codex::TranscriptSummary,
) -> ProviderTranscriptSummary {
    ProviderTranscriptSummary {
        provider: ProviderKind::Codex,
        changed_files: summary.changed_files,
        concerns: summary
            .concerns
            .into_iter()
            .map(|concern| ProviderConcern {
                severity: concern.severity,
                message: concern.message,
            })
            .collect(),
        blockers: summary
            .blockers
            .into_iter()
            .map(|blocker| AdapterBlocker {
                kind: blocker.kind,
                message: blocker.message,
            })
            .collect(),
        usage: ProviderUsage {
            input_tokens: summary.usage.input_tokens,
            output_tokens: summary.usage.output_tokens,
            total_tokens: summary.usage.total_tokens,
        },
        cost_usd: summary.cost_usd,
        transcript_ids: summary
            .transcript_ids
            .into_iter()
            .map(|id| ProviderTranscriptId {
                kind: id.kind,
                value: id.value,
            })
            .collect(),
        raw_summary: summary.raw_summary,
    }
}

fn from_claude_transcript_summary(
    summary: ao2_adapter_claude::TranscriptSummary,
) -> ProviderTranscriptSummary {
    ProviderTranscriptSummary {
        provider: ProviderKind::Claude,
        changed_files: summary.changed_files,
        concerns: summary
            .concerns
            .into_iter()
            .map(|concern| ProviderConcern {
                severity: concern.severity,
                message: concern.message,
            })
            .collect(),
        blockers: summary
            .blockers
            .into_iter()
            .map(|blocker| AdapterBlocker {
                kind: blocker.kind,
                message: blocker.message,
            })
            .collect(),
        usage: ProviderUsage {
            input_tokens: summary.usage.input_tokens,
            output_tokens: summary.usage.output_tokens,
            total_tokens: summary.usage.total_tokens,
        },
        cost_usd: summary.cost_usd,
        transcript_ids: summary
            .transcript_ids
            .into_iter()
            .map(|id| ProviderTranscriptId {
                kind: id.kind,
                value: id.value,
            })
            .collect(),
        raw_summary: summary.raw_summary,
    }
}

fn parse_scripted_provider_transcript(
    provider: ProviderKind,
    transcript: &str,
    sandbox_changed_files: &[String],
) -> ProviderTranscriptSummary {
    let parse_body = transcript_parse_body(transcript);
    let mut changed_files = sandbox_changed_files
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut concerns = Vec::new();
    let mut blockers = Vec::new();
    let mut usage = ProviderUsage::default();
    let mut cost_usd = None;
    let mut transcript_ids = BTreeMap::new();
    let mut raw_summary = None;

    for line in parse_body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            merge_transcript_ids(&value, &mut transcript_ids);
            if merge_usage_metadata(&value, &mut usage, &mut cost_usd) {
                continue;
            }
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(value) = value_after_label(trimmed, &lower, &["summary"]) {
            if raw_summary.is_none() && !value.is_empty() {
                raw_summary = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = value_after_label(
            trimmed,
            &lower,
            &["changed files", "changed_files", "files changed"],
        ) {
            for file in split_file_list(value) {
                changed_files.insert(file);
            }
            continue;
        }
        if lower.starts_with("modified:")
            || lower.starts_with("added:")
            || lower.starts_with("deleted:")
        {
            if let Some((_, value)) = trimmed.split_once(':') {
                if let Some(file) = clean_file_token(value) {
                    changed_files.insert(file);
                }
            }
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["concern"]) {
            let parsed = parse_concern(value);
            if !parsed.message.eq_ignore_ascii_case("none") && !parsed.message.is_empty() {
                concerns.push(parsed);
            }
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["blocker"]) {
            let msg = value.trim();
            if !msg.eq_ignore_ascii_case("none") && !msg.is_empty() {
                blockers.push(AdapterBlocker {
                    kind: "provider_reported_blocker".to_string(),
                    message: msg.to_string(),
                });
            }
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["input tokens", "input_tokens"]) {
            usage.input_tokens = parse_u64(value);
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["output tokens", "output_tokens"])
        {
            usage.output_tokens = parse_u64(value);
            continue;
        }
        if let Some(value) =
            value_after_label(trimmed, &lower, &["total tokens", "total_tokens", "tokens"])
        {
            usage.total_tokens = parse_u64(value);
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["cost", "cost usd"]) {
            cost_usd = parse_cost(value);
            continue;
        }
        if let Some((kind, value)) = transcript_id_after_label(trimmed, &lower) {
            transcript_ids.insert(kind, value);
        }
    }

    ProviderTranscriptSummary {
        provider,
        changed_files: changed_files.into_iter().collect(),
        concerns,
        blockers,
        usage,
        cost_usd,
        transcript_ids: transcript_ids
            .into_iter()
            .map(|(kind, value)| ProviderTranscriptId { kind, value })
            .collect(),
        raw_summary,
    }
}

fn transcript_parse_body(transcript: &str) -> &str {
    if let Some((_, after_stdout)) = transcript.split_once("\nstdout:\n") {
        return after_stdout
            .split_once("\nstderr:\n")
            .map(|(stdout, _)| stdout)
            .unwrap_or(after_stdout);
    }
    transcript
}

fn value_after_label<'a>(line: &'a str, lower: &str, labels: &[&str]) -> Option<&'a str> {
    for label in labels {
        let prefix_colon = format!("{label}:");
        let prefix_equals = format!("{label}=");
        if lower.starts_with(&prefix_colon) {
            return Some(line[prefix_colon.len()..].trim());
        }
        if lower.starts_with(&prefix_equals) {
            return Some(line[prefix_equals.len()..].trim());
        }
    }
    None
}

fn split_file_list(value: &str) -> Vec<String> {
    value
        .split([',', ';'])
        .filter_map(clean_file_token)
        .collect()
}

fn clean_file_token(value: &str) -> Option<String> {
    let cleaned = value
        .trim()
        .trim_start_matches("- ")
        .trim_matches(['`', '"', '\''])
        .replace('\\', "/");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn parse_concern(value: &str) -> ProviderConcern {
    if let Some((severity, message)) = value.split_once(" - ") {
        ProviderConcern {
            severity: severity.trim().to_ascii_lowercase(),
            message: message.trim().to_string(),
        }
    } else {
        ProviderConcern {
            severity: "unspecified".to_string(),
            message: value.trim().to_string(),
        }
    }
}

fn parse_u64(value: &str) -> Option<u64> {
    value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn parse_cost(value: &str) -> Option<f64> {
    value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .ok()
}

fn transcript_id_after_label(line: &str, lower: &str) -> Option<(String, String)> {
    for label in [
        "session_id",
        "session id",
        "conversation_id",
        "conversation id",
        "transcript_id",
        "transcript id",
        "response_id",
        "response id",
        "thread_id",
        "thread id",
    ] {
        if let Some(value) =
            value_after_label(line, lower, &[label]).filter(|value| !value.is_empty())
        {
            return Some((normalize_transcript_id_kind(label), value.to_string()));
        }
    }
    None
}

fn merge_transcript_ids(value: &serde_json::Value, ids: &mut BTreeMap<String, String>) {
    for key in [
        "session_id",
        "conversation_id",
        "transcript_id",
        "response_id",
        "thread_id",
    ] {
        if let Some(text) = value.get(key).and_then(serde_json::Value::as_str) {
            if !text.trim().is_empty() {
                ids.insert(key.to_string(), text.trim().to_string());
            }
        }
    }
}

fn normalize_transcript_id_kind(label: &str) -> String {
    label.replace(' ', "_")
}

fn merge_usage_metadata(
    value: &serde_json::Value,
    usage: &mut ProviderUsage,
    cost_usd: &mut Option<f64>,
) -> bool {
    let usage_value = value.get("usage").unwrap_or(value);
    let mut found = false;

    if let Some(value) =
        json_u64_like(usage_value, "input_tokens").or_else(|| json_u64_like(usage_value, "input"))
    {
        usage.input_tokens = Some(value);
        found = true;
    }
    if let Some(value) =
        json_u64_like(usage_value, "output_tokens").or_else(|| json_u64_like(usage_value, "output"))
    {
        usage.output_tokens = Some(value);
        found = true;
    }
    if let Some(value) =
        json_u64_like(usage_value, "total_tokens").or_else(|| json_u64_like(usage_value, "total"))
    {
        usage.total_tokens = Some(value);
        found = true;
    }
    if let Some(value) =
        json_f64_like(usage_value, "cost_usd").or_else(|| json_f64_like(usage_value, "cost"))
    {
        *cost_usd = Some(value);
        found = true;
    }

    found
}

fn json_u64_like(value: &serde_json::Value, key: &str) -> Option<u64> {
    match value.get(key)? {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => parse_u64(text),
        _ => None,
    }
}

fn json_f64_like(value: &serde_json::Value, key: &str) -> Option<f64> {
    match value.get(key)? {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => parse_cost(text),
        _ => None,
    }
}

fn doctor_external(
    provider: ProviderKind,
    metadata: ProviderMetadata,
) -> Result<ProviderDoctorReport> {
    let command_path = resolve_adapter_command(Path::new(&metadata.doctor_command));
    let doctor_command = metadata.doctor_command.clone();
    let doctor_args = metadata.doctor_args.clone();
    let metadata_source = metadata.metadata_source.clone();
    match adapter_process_command(&command_path, &doctor_args).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let version = if stdout.is_empty() { stderr } else { stdout };
            Ok(ProviderDoctorReport {
                provider,
                available: output.status.success(),
                command: command_path.display().to_string(),
                doctor_args: doctor_args.clone(),
                metadata_source: metadata_source.clone(),
                version,
                blocker: if output.status.success() {
                    None
                } else {
                    Some(AdapterBlocker {
                        kind: "doctor_non_zero_exit".to_string(),
                        message: format!(
                            "{} {} exited with {:?}",
                            doctor_command,
                            doctor_args.join(" "),
                            output.status.code()
                        ),
                    })
                },
            })
        }
        Err(err) => Ok(ProviderDoctorReport {
            provider,
            available: false,
            command: command_path.display().to_string(),
            doctor_args,
            metadata_source,
            version: String::new(),
            blocker: Some(AdapterBlocker {
                kind: "provider_not_found".to_string(),
                message: err.to_string(),
            }),
        }),
    }
}

fn scripted_shell_command(script: &str) -> PathBuf {
    if cfg!(windows) && scripted_prompt_prefers_posix_shell(script) {
        if let Some(command) = posix_shell_command() {
            return command;
        }
    }
    if cfg!(windows) {
        PathBuf::from("powershell")
    } else {
        PathBuf::from("sh")
    }
}

fn scripted_shell_args(script: &str) -> Vec<String> {
    if command_is_posix_shell(&scripted_shell_command(script)) {
        vec!["-c".to_string(), script.to_string()]
    } else if cfg!(windows) {
        vec![
            "-NoProfile".to_string(),
            "-Command".to_string(),
            script.to_string(),
        ]
    } else {
        vec!["-c".to_string(), script.to_string()]
    }
}

pub fn scripted_prompt_prefers_posix_shell(script: &str) -> bool {
    let has_windows_markers = script.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("$env:")
            || trimmed.starts_with("Set-Content ")
            || trimmed.starts_with("New-Item ")
            || trimmed.starts_with("Write-Output ")
            || trimmed.starts_with("Start-Sleep ")
            || trimmed.starts_with("if ($")
    });
    if has_windows_markers {
        return false;
    }

    script.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("printf ")
            || trimmed.starts_with("cat >")
            || trimmed.starts_with("test ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("sleep ")
            || trimmed.starts_with("if [")
            || trimmed.starts_with("if printf")
            || trimmed.contains(" <<")
            || trimmed.contains("grep -q")
            || trimmed.contains("$AO2_REPAIR_")
    })
}

pub fn posix_shell_command() -> Option<PathBuf> {
    if command_available(Path::new("sh")) {
        return Some(PathBuf::from("sh"));
    }
    if cfg!(windows) {
        for candidate in [
            r"C:\Program Files\Git\bin\sh.exe",
            r"C:\Program Files\Git\usr\bin\sh.exe",
            r"C:\Program Files (x86)\Git\bin\sh.exe",
            r"C:\Program Files (x86)\Git\usr\bin\sh.exe",
        ] {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn resolve_adapter_command(command: &Path) -> PathBuf {
    if cfg!(windows) && command_is_posix_shell(command) {
        return posix_shell_command().unwrap_or_else(|| command.to_path_buf());
    }
    if cfg!(windows) {
        if let Some(command) = resolve_windows_path_command(command) {
            return command;
        }
    }
    command.to_path_buf()
}

fn command_is_posix_shell(command: &Path) -> bool {
    command
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("sh") || name.eq_ignore_ascii_case("sh.exe"))
        .unwrap_or(false)
}

fn command_available(command: &Path) -> bool {
    Command::new(command)
        .arg("-c")
        .arg("exit 0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn adapter_process_command(command: &Path, args: &[String]) -> Command {
    if cfg!(windows) && command_is_batch_file(command) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command).args(args);
        cmd
    } else {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd
    }
}

fn command_is_batch_file(command: &Path) -> bool {
    command
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
        .unwrap_or(false)
}

fn resolve_windows_path_command(command: &Path) -> Option<PathBuf> {
    let command_text = command.to_string_lossy();
    if command_text.contains('\\') || command_text.contains('/') {
        return None;
    }

    let path_exts = env::var_os("PATHEXT")
        .map(|value| {
            env::split_paths(&value)
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            vec![
                ".COM".to_string(),
                ".EXE".to_string(),
                ".BAT".to_string(),
                ".CMD".to_string(),
            ]
        });
    let candidates = if command.extension().is_some() {
        vec![command_text.to_string()]
    } else {
        let mut candidates = path_exts
            .into_iter()
            .map(|ext| format!("{command_text}{ext}"))
            .collect::<Vec<_>>();
        candidates.push(command_text.to_string());
        candidates
    };

    for dir in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
        for candidate in &candidates {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

pub fn apply_sandbox_patch(request: SandboxPatchApplyRequest) -> Result<SandboxPatchApplyResult> {
    let preview = preview_sandbox_patch(&request.target_repo, &request.sandbox_path)?;
    if preview.action_digest != request.expected_digest {
        anyhow::bail!(
            "digest mismatch: expected {}, got {}",
            request.expected_digest,
            preview.action_digest
        );
    }

    validate_apply_platform_support(&preview.approval_subject)?;
    for operation in &preview.approval_subject.operations {
        let source = request.sandbox_path.join(&operation.path);
        let target = request.target_repo.join(&operation.path);
        match (&operation.kind, &operation.after) {
            (SandboxPatchOperationKind::Deleted, None) => {
                remove_patch_target(&target)?;
            }
            (
                SandboxPatchOperationKind::Added | SandboxPatchOperationKind::Modified,
                Some(state),
            ) => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                match state.kind {
                    SandboxFileKind::RegularFile => {
                        remove_target_symlink(&target)?;
                        fs::copy(&source, &target)
                            .with_context(|| format!("apply sandbox file {}", target.display()))?;
                        set_unix_mode(&target, state.unix_mode)?;
                    }
                    SandboxFileKind::Symlink => {
                        remove_patch_target(&target)?;
                        apply_symlink(&source, &target)?;
                    }
                }
            }
            _ => anyhow::bail!(
                "invalid sandbox patch operation state for {}",
                operation.path
            ),
        }
    }

    Ok(SandboxPatchApplyResult {
        target_repo: request.target_repo,
        sandbox_path: request.sandbox_path,
        applied_files: preview.changed_files,
        approval_subject: preview.approval_subject,
        action_digest: preview.action_digest,
        approver: request.approver,
    })
}

fn validate_apply_platform_support(subject: &SandboxPatchApprovalSubject) -> Result<()> {
    #[cfg(not(unix))]
    if subject.operations.iter().any(|operation| {
        operation
            .after
            .as_ref()
            .is_some_and(|state| state.kind == SandboxFileKind::Symlink)
    }) {
        anyhow::bail!("sandbox symlink apply is unsupported on this platform");
    }

    let _ = subject;
    Ok(())
}

fn remove_target_symlink(target: &Path) -> Result<()> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(target)
            .with_context(|| format!("remove target symlink {}", target.display())),
        Ok(metadata) if metadata.is_dir() => anyhow::bail!(
            "sandbox patch target unexpectedly resolves to a directory: {}",
            target.display()
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("inspect sandbox patch target {}", target.display()))
        }
    }
}

fn remove_patch_target(target: &Path) -> Result<()> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => anyhow::bail!(
            "sandbox patch refuses to remove directory target: {}",
            target.display()
        ),
        Ok(_) => fs::remove_file(target)
            .with_context(|| format!("remove sandbox patch target {}", target.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("inspect sandbox patch target {}", target.display()))
        }
    }
}

#[cfg(unix)]
fn apply_symlink(source: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    let link_target = fs::read_link(source)
        .with_context(|| format!("read sandbox symlink {}", source.display()))?;
    symlink(&link_target, target)
        .with_context(|| format!("apply sandbox symlink {}", target.display()))
}

#[cfg(not(unix))]
fn apply_symlink(_source: &Path, target: &Path) -> Result<()> {
    anyhow::bail!(
        "sandbox symlink apply is unsupported on this platform: {}",
        target.display()
    )
}

#[cfg(unix)]
fn set_unix_mode(target: &Path, mode: Option<u32>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(mode) = mode {
        fs::set_permissions(target, fs::Permissions::from_mode(mode))
            .with_context(|| format!("set sandbox patch mode on {}", target.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_unix_mode(_target: &Path, _mode: Option<u32>) -> Result<()> {
    Ok(())
}

fn ensure_target_repo(target_repo: &Path) -> Result<()> {
    if !target_repo.is_dir() {
        anyhow::bail!("target repo is not a directory: {}", target_repo.display());
    }
    Ok(())
}

fn resolve_sandbox_working_dir(
    sandbox_path: &Path,
    requested_working_dir: &Path,
) -> Result<PathBuf> {
    let candidate = if requested_working_dir == Path::new(".") {
        sandbox_path.to_path_buf()
    } else if requested_working_dir.is_relative() {
        sandbox_path.join(requested_working_dir)
    } else {
        requested_working_dir.to_path_buf()
    };
    let sandbox = fs::canonicalize(sandbox_path)
        .with_context(|| format!("canonicalize sandbox {}", sandbox_path.display()))?;
    let candidate = fs::canonicalize(&candidate)
        .with_context(|| format!("canonicalize adapter working dir {}", candidate.display()))?;
    if !candidate.starts_with(&sandbox) {
        anyhow::bail!(
            "adapter working dir escapes sandbox: {} is outside {}",
            candidate.display(),
            sandbox.display()
        );
    }
    Ok(candidate)
}

fn create_sandbox_path(target_repo: &Path) -> PathBuf {
    let base = std::env::temp_dir();
    let name = target_repo
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo");
    base.join(format!("ao2-sandbox-{name}-{}", Uuid::new_v4()))
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("create sandbox {}", dst.display()))?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        if rel.components().any(is_ignored_repo_component) {
            continue;
        }
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("create sandbox dir {}", target.display()))?;
        } else if entry.file_type().is_symlink() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            copy_symlink(entry.path(), &target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target).with_context(|| {
                format!("copy {} to {}", entry.path().display(), target.display())
            })?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    let link_target = fs::read_link(source)
        .with_context(|| format!("read sandbox symlink {}", source.display()))?;
    symlink(&link_target, target).with_context(|| {
        format!(
            "copy sandbox symlink {} to {}",
            source.display(),
            target.display()
        )
    })
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, _target: &Path) -> Result<()> {
    anyhow::bail!(
        "sandbox symlink copy is unsupported on this platform: {}",
        source.display()
    )
}

fn snapshot_files(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel_path = entry.path().strip_prefix(root)?;
        if rel_path.components().any(is_ignored_repo_component) {
            continue;
        }
        let rel = rel_path.to_string_lossy().replace('\\', "/");
        let bytes = fs::read(entry.path())
            .with_context(|| format!("read snapshot file {}", entry.path().display()))?;
        files.insert(rel, sha256_hex(&bytes));
    }
    Ok(files)
}

fn is_ignored_repo_component(component: std::path::Component<'_>) -> bool {
    matches!(
        component,
        std::path::Component::Normal(name)
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

fn summarize_diff(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> (Vec<String>, String) {
    let paths = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changed = Vec::new();
    let mut summary = Vec::new();

    for path in paths {
        match (before.get(&path), after.get(&path)) {
            (None, Some(_)) => {
                changed.push(path.clone());
                summary.push(format!("added: {path}"));
            }
            (Some(_), None) => {
                changed.push(path.clone());
                summary.push(format!("deleted: {path}"));
            }
            (Some(left), Some(right)) if left != right => {
                changed.push(path.clone());
                summary.push(format!("modified: {path}"));
            }
            _ => {}
        }
    }

    (changed, summary.join("\n"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_arg_expansion_injects_resolved_working_dir() {
        let args = vec![
            "--add-dir".to_string(),
            ADAPTER_WORKING_DIR_PLACEHOLDER.to_string(),
            "--print".to_string(),
            "task".to_string(),
        ];

        let expanded = expand_adapter_args(&args, Path::new("/tmp/ao2-sandbox-workspace"));

        assert_eq!(
            expanded,
            vec!["--add-dir", "/tmp/ao2-sandbox-workspace", "--print", "task"]
        );
    }
}
