//! `claude` CLI adapter (README §10 P4).
//!
//! Same shape as [`super::codex::CodexProvider`] — the planner doesn't
//! know which CLI it's talking to; only the orchestrator's
//! `--provider` flag picks one over the other. Both honour the §8.1
//! stdin envelope and §8.2 stdout contract.
//!
//! Defaults to invoking `claude` from `$PATH`. Tests inject a mock-bin
//! path via [`ClaudeProvider::with_command`] and per-spawn env vars via
//! [`ClaudeProvider::with_env`] to avoid mutating the parent process's
//! environment.

use std::io::Write;
use std::process::Stdio;

use crate::provider::{
    provider_command, Provider, ProviderError, ProviderRequest, ProviderResponse,
};
use crate::surface::canonical_json;

/// Claude CLI adapter.
#[derive(Debug, Clone)]
pub struct ClaudeProvider {
    command: String,
    env: Vec<(String, String)>,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeProvider {
    /// New adapter that invokes `claude` from `$PATH`.
    pub fn new() -> Self {
        Self {
            command: "claude".to_string(),
            env: Vec::new(),
        }
    }

    /// New adapter that invokes the binary at `command` directly.
    pub fn with_command(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            env: Vec::new(),
        }
    }

    /// Append an env var that will be set on the spawned child process.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Inspect the configured command.
    pub fn command(&self) -> &str {
        &self.command
    }
}

impl ClaudeProvider {
    /// Run the spawned claude CLI and return its raw stdout string.
    /// Shared by [`draft`](Self::draft) and
    /// [`draft_response`](Self::draft_response).
    fn run_cli(&self, request: &ProviderRequest) -> Result<String, ProviderError> {
        let cli = self.command.clone();
        let value = serde_json::to_value(request).map_err(|source| ProviderError::Serialize {
            cli: cli.clone(),
            source,
        })?;
        let envelope = canonical_json(&value);

        let mut cmd = provider_command(&self.command);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| ProviderError::Io {
                cli: cli.clone(),
                source,
            })?;

        {
            let stdin = child.stdin.as_mut().ok_or_else(|| ProviderError::Io {
                cli: cli.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "child stdin unavailable",
                ),
            })?;
            stdin
                .write_all(envelope.as_bytes())
                .map_err(|source| ProviderError::Io {
                    cli: cli.clone(),
                    source,
                })?;
        }

        let output = child
            .wait_with_output()
            .map_err(|source| ProviderError::Io {
                cli: cli.clone(),
                source,
            })?;

        if !output.status.success() {
            return match output.status.code() {
                Some(code) => Err(ProviderError::ExitNonZero { cli, code }),
                None => Err(ProviderError::Signal { cli }),
            };
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Parse the claude CLI stdout into a `(candidate, total_cost_usd)`
/// pair. Detects the `claude --print --output-format json` wrapper
/// envelope (G6) — when present, the inner candidate is decoded out of
/// the wrapper's `result` string field and `total_cost_usd` is lifted
/// off the wrapper. When the stdout is not wrapped (e.g. the existing
/// mock-bin fixture path), the parsed value is returned directly with
/// `total_cost_usd = None`.
pub(crate) fn parse_claude_stdout(
    stdout: &str,
    cli: &str,
) -> Result<(serde_json::Value, Option<f64>), ProviderError> {
    let trimmed = stdout.trim();
    let parsed: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| ProviderError::NonJson {
            cli: cli.to_string(),
            reason: format!("{e}; stdout was: {stdout}"),
        })?;

    // Wrapper detection: claude --output-format json emits a top-level
    // object whose `result` field carries the assistant's reply as a
    // JSON string. Any other shape (raw candidate JSON, used by current
    // fixtures + mock bins) flows through untouched.
    if let Some(result_str) = parsed.get("result").and_then(|v| v.as_str()) {
        let candidate: serde_json::Value =
            serde_json::from_str(result_str).map_err(|e| ProviderError::NonJson {
                cli: cli.to_string(),
                reason: format!("wrapper `result` not JSON: {e}; result was: {result_str}"),
            })?;
        let cost = parsed.get("total_cost_usd").and_then(|v| v.as_f64());
        return Ok((candidate, cost));
    }

    Ok((parsed, None))
}

impl Provider for ClaudeProvider {
    fn draft(&self, request: &ProviderRequest) -> Result<serde_json::Value, ProviderError> {
        let stdout = self.run_cli(request)?;
        let (candidate, _cost) = parse_claude_stdout(&stdout, &self.command)?;
        Ok(candidate)
    }

    fn draft_response(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let stdout = self.run_cli(request)?;
        let (candidate, total_cost_usd) = parse_claude_stdout(&stdout, &self.command)?;
        Ok(ProviderResponse {
            candidate,
            total_cost_usd,
        })
    }
}
