//! Provider integration — phase P3 (+ P4).
//!
//! The planner shells out to an LLM CLI (codex or claude) via a
//! short-lived child process. Wire protocol:
//!
//! - stdin: canonical-JSON `ProviderRequest` (README §8.1).
//! - stdout: a single JSON object that we parse as the candidate plan
//!   (`ao2.sdd-plan-candidate.v1`, README §8.2).
//! - stderr: diagnostic only, never parsed.
//! - exit 0: success; anything else → [`ProviderError::ExitNonZero`].
//!
//! The trait is intentionally minimal: one fallible call, no streaming,
//! no per-attempt state — retry/feedback lives in the orchestrator
//! (P5, README §6).

use crate::schema::SurfaceMap;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub mod claude;
pub mod codex;

/// Provider names accepted by the orchestrator's `--provider` flag.
pub const PROVIDER_CODEX: &str = "codex";
pub const PROVIDER_CLAUDE: &str = "claude";

/// `schema_version` literal for the stdin envelope (README §8.1 / §15).
pub const REQUEST_SCHEMA_VERSION: &str = "ao2.sdd-provider-request.v1";

/// `expected_output.schema` literal sent to the provider (README §8.1).
pub const CANDIDATE_SCHEMA: &str = "ao2.sdd-plan-candidate.v1";

/// Hard cap on `plan.steps.len()` advertised to the provider (README
/// §5.1 V7).
pub const MAX_STEPS: u32 = 25;

/// Engine-owned constraints for software source created or changed by AO2.
/// Repositories still own language-specific thresholds and verifier commands.
pub const SOFTWARE_SOURCE_POLICY: &str = r#"Software-source growth policy:
- Classify by repository and language context. Handwritten production source, test source, executable scripts, and executable qualification runners are in scope. Documentation, evidence, logs, manifests, checksums, reports, non-executable fixtures, schemas, migrations, binaries, archives, runtime state, and other non-source artifacts are outside this policy.
- Before proposing source changes, inspect the destination repository before editing and stop at the first sufficient option: (1) No source change; (2) Reuse or delete existing code; (3) use the standard library or native platform; (4) use an already-installed dependency; (5) make the smallest cohesive change in the existing structure; (6) only then add the minimum new module, abstraction, dependency usage, configuration, compatibility path, or layer required now.
- Before extending a file, check existing helpers and callers, present responsibility, unhealthy file or function growth, dependency direction, and whether extraction creates cohesion or merely fragmentation. No speculative scaffolding, duplicate behavior, unrelated refactors, or copied tests when a table-driven case suffices.
- generated or vendored source stays classified as source and requires an explicit reviewable exception from handwritten-source limits. Cohesive source may also receive a reasoned exception when splitting would worsen the design. Non-source artifacts need no exception.
- After implementation, evaluate the exact base/head source diff with repository-native policy and tools when supplied. Treat size and complexity as signals and ratchets, not universal quality definitions. Do not grow grandfathered oversized source without a recorded exception; behavior-neutral touches without growth remain allowed.
- Prefer deterministic checks for objective facts and model judgment for cohesion. Do not invent a universal parser or threshold or add a counting dependency. Do not split or compress code merely to evade a threshold.
- Do not simplify away trust-boundary validation, security controls, accessibility, data-loss-preventing error handling, required compatibility, or explicitly requested behavior."#;

pub(crate) fn provider_command(command: &str) -> Command {
    if cfg!(windows) {
        if let Some(script) = resolve_windows_posix_script(command) {
            let mut shell = Command::new(windows_posix_shell_command());
            shell.arg(script);
            return shell;
        }
    }

    Command::new(command)
}

fn resolve_windows_posix_script(command: &str) -> Option<PathBuf> {
    let raw = Path::new(command);
    if raw.components().count() > 1 || raw.is_absolute() {
        return posix_script_file(raw);
    }

    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        if let Some(path) = posix_script_file(&dir.join(command)) {
            return Some(path);
        }
    }

    None
}

fn posix_script_file(path: &Path) -> Option<PathBuf> {
    let is_posix_script = path.extension().is_none_or(|ext| ext == "sh");
    if path.is_file() && is_posix_script {
        Some(path.to_path_buf())
    } else {
        None
    }
}

fn windows_posix_shell_command() -> PathBuf {
    [
        r"C:\Program Files\Git\bin\sh.exe",
        r"C:\Program Files\Git\usr\bin\sh.exe",
        r"C:\Program Files (x86)\Git\bin\sh.exe",
        r"C:\Program Files (x86)\Git\usr\bin\sh.exe",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
    .unwrap_or_else(|| PathBuf::from("sh"))
}

/// stdin envelope sent to the provider CLI (README §8.1).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderRequest {
    pub schema_version: String,
    pub request_id: String,
    pub prompt: String,
    pub context: ProviderContext,
    pub expected_output: ExpectedOutput,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderContext {
    pub surface_map: SurfaceMap,
    pub prior_errors: Vec<String>,
    pub software_source_policy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpectedOutput {
    pub schema: String,
    pub max_steps: u32,
}

impl ProviderRequest {
    /// Build a fresh request with a freshly minted ULID `request_id`.
    pub fn new(
        prompt: impl Into<String>,
        surface_map: SurfaceMap,
        prior_errors: Vec<String>,
    ) -> Self {
        Self {
            schema_version: REQUEST_SCHEMA_VERSION.to_string(),
            request_id: ulid::Ulid::new().to_string(),
            prompt: prompt.into(),
            context: ProviderContext {
                surface_map,
                prior_errors,
                software_source_policy: SOFTWARE_SOURCE_POLICY.to_string(),
            },
            expected_output: ExpectedOutput {
                schema: CANDIDATE_SCHEMA.to_string(),
                max_steps: MAX_STEPS,
            },
        }
    }
}

/// Errors any provider adapter can surface. The orchestrator inspects
/// these to decide retry vs. fail-closed (README §6).
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Provider exited with a non-zero status.
    #[error("provider `{cli}` exited with status {code}")]
    ExitNonZero { cli: String, code: i32 },

    /// Provider was killed by a signal (no exit code).
    #[error("provider `{cli}` was terminated by signal")]
    Signal { cli: String },

    /// Provider's stdout did not parse as JSON.
    #[error("provider `{cli}` produced non-JSON output: {reason}")]
    NonJson { cli: String, reason: String },

    /// I/O failure (spawn, write, read).
    #[error("provider `{cli}` I/O: {source}")]
    Io {
        cli: String,
        #[source]
        source: std::io::Error,
    },

    /// Failed to serialise the request envelope.
    #[error("provider `{cli}` envelope serialise: {source}")]
    Serialize {
        cli: String,
        #[source]
        source: serde_json::Error,
    },
}

/// Response surface returned by [`Provider::draft_response`]. Carries
/// the candidate plan JSON the provider emitted plus optional per-draft
/// cost telemetry. `total_cost_usd` is `None` when the provider does
/// not report cost (e.g. the codex adapter today); claude populates it
/// from the `--output-format json` wrapper (G6, README §10).
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub candidate: serde_json::Value,
    pub total_cost_usd: Option<f64>,
}

/// Common surface for codex / claude adapters.
pub trait Provider {
    /// Send `request` to the underlying CLI and return its raw JSON
    /// response. The orchestrator is responsible for validating the
    /// returned value against `ao2.sdd-plan.v1`.
    fn draft(&self, request: &ProviderRequest) -> Result<serde_json::Value, ProviderError>;

    /// Same call as [`Self::draft`] but returns a [`ProviderResponse`]
    /// carrying optional cost telemetry alongside the candidate. The
    /// default impl forwards to `draft` with `total_cost_usd: None`;
    /// providers that surface cost (e.g. claude) override this.
    fn draft_response(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            candidate: self.draft(request)?,
            total_cost_usd: None,
        })
    }
}
