//! `codex` CLI adapter (README §10 P3, §8.1–§8.4).
//!
//! Defaults to invoking `codex` from `$PATH`. Tests inject a mock-bin
//! path via [`CodexProvider::with_command`] and per-spawn env vars via
//! [`CodexProvider::with_env`] — that avoids mutating the parent
//! process's environment, so tests can run in parallel.

use std::io::Write;
use std::process::Stdio;

use crate::provider::{provider_command, Provider, ProviderError, ProviderRequest};
use crate::surface::canonical_json;

/// Codex CLI adapter.
#[derive(Debug, Clone)]
pub struct CodexProvider {
    /// CLI name or absolute path. Defaults to `"codex"` (resolved via
    /// `$PATH`).
    command: String,
    /// Env vars applied to the spawned child only — never to the
    /// parent process.
    env: Vec<(String, String)>,
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexProvider {
    /// New adapter that invokes `codex` from `$PATH`.
    pub fn new() -> Self {
        Self {
            command: "codex".to_string(),
            env: Vec::new(),
        }
    }

    /// New adapter that invokes the binary at `command` directly.
    /// Used by tests to point at `tests/mock-bins/codex`.
    pub fn with_command(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            env: Vec::new(),
        }
    }

    /// Append an env var that will be set on the spawned child process.
    /// Builder-style for ergonomic test setup.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Inspect the configured command (mostly for diagnostics / tests).
    pub fn command(&self) -> &str {
        &self.command
    }
}

impl Provider for CodexProvider {
    fn draft(&self, request: &ProviderRequest) -> Result<serde_json::Value, ProviderError> {
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

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        serde_json::from_str::<serde_json::Value>(stdout.trim()).map_err(|e| {
            ProviderError::NonJson {
                cli,
                reason: format!("{e}; stdout was: {stdout}"),
            }
        })
    }
}
