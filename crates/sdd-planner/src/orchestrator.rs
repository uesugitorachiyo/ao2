//! Orchestrator — phase P5.
//!
//! Implements the README §6 retry / fail-closed protocol verbatim:
//!
//! ```text
//! ATTEMPT_BUDGET = 3
//! attempt = 1
//! loop {
//!   candidate = provider.draft(prompt, context, prior_errors)
//!   fs::write("attempt-{attempt}.json", candidate)
//!   report = validator.validate(candidate, surface_map)
//!   if report.is_pass() { emit_canonical(report.plan); return Ok }
//!   if attempt >= ATTEMPT_BUDGET {
//!     fs::write("candidate.fail.json", candidate)
//!     fs::write("validation-errors.txt", report.render())
//!     return Err(PlanExhausted)
//!   }
//!   prior_errors = report.errors_for_provider_feedback()
//!   attempt += 1
//! }
//! ```
//!
//! - `ProviderError` (spawn / non-zero / non-JSON) short-circuits the
//!   loop — only validator failures trigger a retry.
//! - All artifacts live under `<build_log_root>/<plan_id>/`.
//!   `<plan_id>` is taken from attempt 1's candidate if parseable; on
//!   shape failure we fall back to the request_id ULID so the dir is
//!   never anonymous.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::provider::{Provider, ProviderError, ProviderRequest};
use crate::schema::{Plan, SurfaceMap, SCHEMA_VERSION};
use crate::surface::canonical_json;
use crate::validator::{validate, ValidationReport};

/// Hard cap on provider drafts per run (README §6).
pub const ATTEMPT_BUDGET: u32 = 3;

/// Successful orchestration result.
#[derive(Debug, Clone)]
pub struct PlanOutcome {
    pub plan: Plan,
    pub plan_id: String,
    pub canonical_json: String,
    pub build_log_dir: PathBuf,
    pub attempts_used: u32,
}

/// Orchestration failures.
#[derive(Debug, thiserror::Error)]
pub enum OrchestrateError {
    #[error(
        "plan exhausted after {budget} attempts; see {build_log_dir}/candidate.fail.json + validation-errors.txt"
    )]
    PlanExhausted { budget: u32, build_log_dir: PathBuf },

    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Run the §6 retry loop against a provider and emit a canonical
/// `ao2.sdd-plan.v1` artifact on success.
///
/// `build_log_root` is the parent directory under which a per-run
/// `<plan_id>/` subdirectory is created. Callers typically pass
/// `<repo>/target/sdd-planner` (gitignored per README §11 D10).
///
/// `repo_path` is the engine-owned ground truth for the target
/// repository; it overrides any value the model writes into
/// `target.repo_path`.
///
/// `provider_name` is the orchestrator's configured provider (the
/// value of the CLI `--provider` flag, e.g. `"codex"` or `"claude"`).
/// It is engine-authoritative for `provenance.provider` and overwrites
/// whatever the model wrote into that field (G3, see
/// `factory-v3/dogfood/sdd-planner-claude/findings.md`).
pub fn orchestrate(
    provider: &dyn Provider,
    prompt: &str,
    surface_map: &SurfaceMap,
    build_log_root: &Path,
    repo_path: &Path,
    provider_name: &str,
) -> Result<PlanOutcome, OrchestrateError> {
    let surface_map_value = serde_json::to_value(surface_map)?;
    let surface_map_canon = canonical_json(&surface_map_value);
    let surface_map_sha256 = sha256_hex(&surface_map_canon);

    let mut prior_errors: Vec<String> = Vec::new();
    let mut build_log_dir: Option<PathBuf> = None;
    let mut last_candidate_text: Option<String> = None;
    let mut last_report: Option<ValidationReport> = None;

    for attempt in 1..=ATTEMPT_BUDGET {
        let request = ProviderRequest::new(prompt, surface_map.clone(), prior_errors.clone());
        let response = provider.draft_response(&request)?;
        let candidate_value = response.candidate;
        let attempt_cost = response.total_cost_usd;

        // Resolve the build-log directory once, off the first attempt.
        if build_log_dir.is_none() {
            let plan_id = candidate_value
                .get("plan_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&request.request_id)
                .to_string();
            let dir = build_log_root.join(&plan_id);
            std::fs::create_dir_all(&dir)?;
            build_log_dir = Some(dir);
        }
        let dir = build_log_dir.as_ref().expect("dir set above").clone();
        let attempt_path = dir.join(format!("attempt-{attempt}.json"));

        // G6: persist per-attempt cost alongside the candidate JSON.
        // `total_cost_usd` is added as a sibling top-level field on the
        // attempt log only (NOT on the promoted plan), so it does not
        // leak into the canonical `ao2.sdd-plan.v1` artifact. Providers
        // that do not report cost (codex today) leave the field absent.
        let mut attempt_log_value = candidate_value.clone();
        if let (Some(cost), Some(obj)) = (attempt_cost, attempt_log_value.as_object_mut()) {
            obj.insert(
                "total_cost_usd".to_string(),
                serde_json::Number::from_f64(cost)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        let attempt_log_text = canonical_json(&attempt_log_value);
        std::fs::write(&attempt_path, &attempt_log_text)?;
        let candidate_text = canonical_json(&candidate_value);

        // Promote candidate → plan in-memory before validating
        // (rewrite schema_version, fill provenance.attempts, overwrite
        // the seven engine-owned fields). The on-disk attempt-N.json
        // preserves the provider's raw output.
        let mut promoted = candidate_value.clone();
        promote_candidate_to_plan(
            &mut promoted,
            attempt,
            repo_path,
            &surface_map_sha256,
            Utc::now(),
            provider_name,
        );
        let promoted_text = canonical_json(&promoted);
        let report = validate(&promoted_text, Some(surface_map));
        if report.is_pass() {
            let plan = report
                .plan
                .clone()
                .expect("validator promises plan present on pass");
            let canonical = canonical_json(&serde_json::to_value(&plan)?);
            return Ok(PlanOutcome {
                plan_id: plan.plan_id.clone(),
                plan,
                canonical_json: canonical,
                build_log_dir: dir,
                attempts_used: attempt,
            });
        }
        prior_errors = report.errors_for_provider_feedback();
        last_candidate_text = Some(candidate_text);
        last_report = Some(report);
    }

    // Exhausted: write diagnostics and fail closed.
    let dir = build_log_dir.expect("set on first attempt");
    if let Some(text) = last_candidate_text {
        std::fs::write(dir.join("candidate.fail.json"), text)?;
    }
    if let Some(rep) = last_report {
        std::fs::write(dir.join("validation-errors.txt"), rep.render())?;
    }
    Err(OrchestrateError::PlanExhausted {
        budget: ATTEMPT_BUDGET,
        build_log_dir: dir,
    })
}

/// Lowercase hex SHA-256 of the input string.
fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Shell `git log -1 --format=%s` in `repo_path` to retrieve the head
/// subject. Returns empty string on any failure so promotion stays
/// infallible.
fn head_subject_for(repo_path: &Path) -> String {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// Engine-owned git SHA: prefers `VERGEN_GIT_SHA` set at build time,
/// falls back to the planner crate version when unset.
fn engine_sha() -> &'static str {
    option_env!("VERGEN_GIT_SHA").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// Rewrite a `ao2.sdd-plan-candidate.v1` JSON object in place so it
/// reads as `ao2.sdd-plan.v1` and overwrites the engine-owned fields
/// with engine-derived truth: `prompt.sha256`, `target.repo_path`,
/// `target.head_subject`, `target.surface_map_sha256`, `provenance.engine_sha`,
/// `provenance.cli_version`, `provenance.provider`, and `generated_at_utc`.
/// `provenance.attempts` is filled from the current attempt counter.
///
/// `provider_name` is engine-authoritative for `provenance.provider`
/// (G3, `factory-v3/dogfood/sdd-planner-claude/findings.md`) and replaces
/// whatever value the model wrote there.
fn promote_candidate_to_plan(
    value: &mut serde_json::Value,
    attempt: u32,
    repo_path: &Path,
    surface_map_sha256: &str,
    now: DateTime<Utc>,
    provider_name: &str,
) {
    let head_subject = head_subject_for(repo_path);
    let repo_path_str = repo_path.to_string_lossy().into_owned();
    let prompt_sha = value
        .get("prompt")
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .map(sha256_hex)
        .unwrap_or_else(|| sha256_hex(""));

    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "schema_version".to_string(),
            serde_json::Value::String(SCHEMA_VERSION.to_string()),
        );
        obj.insert(
            "generated_at_utc".to_string(),
            serde_json::Value::String(now.to_rfc3339()),
        );
        if let Some(prompt_obj) = obj.get_mut("prompt").and_then(|p| p.as_object_mut()) {
            prompt_obj.insert("sha256".to_string(), serde_json::Value::String(prompt_sha));
        }
        if let Some(target_obj) = obj.get_mut("target").and_then(|t| t.as_object_mut()) {
            target_obj.insert(
                "repo_path".to_string(),
                serde_json::Value::String(repo_path_str),
            );
            target_obj.insert(
                "head_subject".to_string(),
                serde_json::Value::String(head_subject),
            );
            target_obj.insert(
                "surface_map_sha256".to_string(),
                serde_json::Value::String(surface_map_sha256.to_string()),
            );
        }
        if let Some(prov) = obj.get_mut("provenance").and_then(|p| p.as_object_mut()) {
            prov.insert(
                "attempts".to_string(),
                serde_json::Value::Number(serde_json::Number::from(attempt)),
            );
            prov.insert(
                "engine_sha".to_string(),
                serde_json::Value::String(engine_sha().to_string()),
            );
            prov.insert(
                "cli_version".to_string(),
                serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
            );
            prov.insert(
                "provider".to_string(),
                serde_json::Value::String(provider_name.to_string()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promote_overwrites_engine_owned_fields() {
        let mut candidate = serde_json::json!({
            "schema_version": "ao2.sdd-plan-candidate.v1",
            "plan_id": "01TESTPLAN0000000000000000",
            "generated_at_utc": "",
            "prompt": {
                "text": "build a tiny CLI",
                "sha256": "0".repeat(64),
            },
            "target": {
                "repo_path": "",
                "head_sha": "0".repeat(40),
                "head_subject": "",
                "surface_map_sha256": "",
            },
            "plan": {
                "kind": "build",
                "title": "Tiny CLI",
                "goal": "Print hello.",
                "non_goals": [],
                "steps": [],
                "exit_criteria": { "tests": [], "gates": [], "manual": [] }
            },
            "provenance": {
                "attempts": 0,
                "provider": "codex",
                "engine_sha": "",
                "cli_version": "",
            },
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "ingest_authority": "ao2-runner",
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        });

        let repo_path = Path::new("/tmp/sdd-planner-unit-test-repo");
        let surface_map_sha256 = "a".repeat(64);
        let now: DateTime<Utc> = "2026-05-27T12:00:00Z".parse().expect("parse fixed now");

        promote_candidate_to_plan(
            &mut candidate,
            1,
            repo_path,
            &surface_map_sha256,
            now,
            "codex",
        );

        assert_eq!(
            candidate["prompt"]["sha256"].as_str().unwrap(),
            sha256_hex("build a tiny CLI"),
        );
        assert_eq!(
            candidate["target"]["repo_path"].as_str().unwrap(),
            "/tmp/sdd-planner-unit-test-repo",
        );
        assert_eq!(
            candidate["target"]["surface_map_sha256"].as_str().unwrap(),
            surface_map_sha256,
        );
        assert_eq!(
            candidate["provenance"]["cli_version"].as_str().unwrap(),
            env!("CARGO_PKG_VERSION"),
        );
        assert_eq!(
            candidate["generated_at_utc"].as_str().unwrap(),
            now.to_rfc3339(),
        );
        assert_eq!(
            candidate["schema_version"].as_str().unwrap(),
            SCHEMA_VERSION,
        );
        assert_eq!(candidate["provenance"]["attempts"].as_u64().unwrap(), 1,);
    }

    /// G3 regression (findings.md):
    /// `provenance.provider` must come from the orchestrator's
    /// `--provider` flag, NOT from the model's candidate output.
    #[test]
    fn promote_overwrites_provider_from_orchestrator_flag() {
        // Model returns provider="codex" in the candidate.
        let mut candidate = serde_json::json!({
            "schema_version": "ao2.sdd-plan-candidate.v1",
            "plan_id": "01TESTPLAN0000000000000000",
            "generated_at_utc": "",
            "prompt": {
                "text": "build a tiny CLI",
                "sha256": "0".repeat(64),
            },
            "target": {
                "repo_path": "",
                "head_sha": "0".repeat(40),
                "head_subject": "",
                "surface_map_sha256": "",
            },
            "plan": {
                "kind": "build",
                "title": "Tiny CLI",
                "goal": "Print hello.",
                "non_goals": [],
                "steps": [],
                "exit_criteria": { "tests": [], "gates": [], "manual": [] }
            },
            "provenance": {
                "attempts": 0,
                "provider": "codex",
                "engine_sha": "",
                "cli_version": "",
            },
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "ingest_authority": "ao2-runner",
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        });

        let repo_path = Path::new("/tmp/sdd-planner-unit-test-repo");
        let surface_map_sha256 = "a".repeat(64);
        let now: DateTime<Utc> = "2026-05-27T12:00:00Z".parse().expect("parse fixed now");

        // Orchestrator dispatched with --provider claude.
        promote_candidate_to_plan(
            &mut candidate,
            1,
            repo_path,
            &surface_map_sha256,
            now,
            "claude",
        );

        // The model said "codex" but the orchestrator owns the field.
        assert_eq!(
            candidate["provenance"]["provider"].as_str().unwrap(),
            "claude",
            "provenance.provider must come from the orchestrator's --provider flag, not the model",
        );

        // The rest of the provenance block is still rewritten by the
        // engine — sanity check we didn't break the surrounding fields.
        assert_eq!(candidate["provenance"]["attempts"].as_u64().unwrap(), 1,);
        assert_eq!(
            candidate["provenance"]["cli_version"].as_str().unwrap(),
            env!("CARGO_PKG_VERSION"),
        );
    }

    /// When `VERGEN_GIT_SHA` is set at compile time, `engine_sha()` must
    /// return a 40-char lowercase hex git SHA. When unset (non-git
    /// checkout), the assertion is skipped because `engine_sha()` falls
    /// back to `CARGO_PKG_VERSION`.
    #[test]
    fn engine_sha_is_40_hex_chars_when_vergen_set() {
        match option_env!("VERGEN_GIT_SHA") {
            Some(_) => {
                let sha = engine_sha();
                assert_eq!(
                    sha.len(),
                    40,
                    "engine_sha must be 40 chars when VERGEN_GIT_SHA is set, got {} chars: {:?}",
                    sha.len(),
                    sha,
                );
                assert!(
                    sha.chars()
                        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                    "engine_sha must match [0-9a-f]{{40}} when VERGEN_GIT_SHA is set, got {:?}",
                    sha,
                );
            }
            None => {
                // Non-git checkout: vergen could not emit a SHA, so
                // engine_sha() falls back to CARGO_PKG_VERSION. Nothing
                // to assert about shape under this condition.
            }
        }
    }
}
