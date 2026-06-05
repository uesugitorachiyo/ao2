//! Authoritative schema for `ao2.sdd-plan.v1`.
//!
//! Translated field-for-field from README §5. Literal-valued fields
//! (`schema_version`, `trust_boundary.*`) are stored as `String`/`bool`
//! and enforced by the validator (V1, V5) rather than by serde tagging,
//! because the spec also defines a sibling candidate schema
//! (`ao2.sdd-plan-candidate.v1`) that the orchestrator rewrites in place.
//!
//! `SurfaceMap` and `SurfaceFile` declare fields in alphabetical order so
//! a direct `serde_json::to_string` produces canonical JSON (matches the
//! recursive sort performed by `surface::canonical_json`).

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "ao2.sdd-plan.v1";
pub const CANDIDATE_SCHEMA_VERSION: &str = "ao2.sdd-plan-candidate.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    pub schema_version: String,
    pub plan_id: String,
    pub generated_at_utc: String,
    pub prompt: Prompt,
    pub target: Target,
    pub plan: PlanBody,
    pub provenance: Provenance,
    pub trust_boundary: TrustBoundary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prompt {
    pub text: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub repo_path: String,
    pub head_sha: String,
    pub head_subject: String,
    pub surface_map_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanBody {
    pub kind: PlanKind,
    pub title: String,
    pub goal: String,
    pub non_goals: Vec<String>,
    pub steps: Vec<Step>,
    pub exit_criteria: ExitCriteria,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlanKind {
    Build,
    Investigation,
    Refactor,
    Fix,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub kind: StepKind,
    pub paths: Vec<String>,
    pub rationale: String,
    pub acceptance: Vec<String>,
    pub depends_on: Vec<String>,
}

/// Closed enum of allowed `step.kind` values.
///
/// Per G5 (`factory-v3/dogfood/sdd-planner-claude/findings.md`), the allowed
/// set is the closed set `{create, edit, test, verify, delete}`. Serde will
/// reject unknown variants like `"foo"` at parse time because this enum is
/// not tagged with `#[serde(other)]` — unknown lowercase strings fail to
/// deserialize.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    Create,
    Edit,
    Test,
    Verify,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExitCriteria {
    pub tests: Vec<String>,
    pub gates: Vec<String>,
    pub manual: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Provenance {
    pub attempts: u32,
    pub provider: String,
    pub engine_sha: String,
    pub cli_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustBoundary {
    pub control_plane_role: String,
    pub mutates_ao_artifacts: bool,
    pub ingest_authority: String,
    pub release_acceptance_owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceMap {
    pub files: Vec<SurfaceFile>,
    pub head_sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceFile {
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub public_symbols: Vec<String>,
    pub sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_kind_accepts_all_five_closed_variants() {
        let allowed = [
            (r#""create""#, StepKind::Create),
            (r#""edit""#, StepKind::Edit),
            (r#""test""#, StepKind::Test),
            (r#""verify""#, StepKind::Verify),
            (r#""delete""#, StepKind::Delete),
        ];
        for (json, expected) in allowed {
            let parsed: StepKind = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("expected {json} to parse, got {e}"));
            assert_eq!(parsed, expected, "round-trip variant mismatch for {json}");
        }
    }

    #[test]
    fn step_kind_rejects_unknown_variant() {
        let err = serde_json::from_str::<StepKind>(r#""foo""#)
            .expect_err("step.kind \"foo\" must be rejected by the closed enum");
        let message = err.to_string();
        assert!(
            message.contains("foo") || message.contains("unknown variant"),
            "expected unknown-variant error for \"foo\", got: {message}"
        );
    }

    #[test]
    fn step_kind_rejects_unknown_variant_inside_plan_step() {
        let step_json = r#"{
            "id": "step_bad",
            "kind": "foo",
            "paths": ["src/lib.rs"],
            "rationale": "exercise closed enum",
            "acceptance": ["add coverage"],
            "depends_on": []
        }"#;
        let err = serde_json::from_str::<Step>(step_json)
            .expect_err("a Step with kind=\"foo\" must be rejected at parse time");
        assert!(
            err.to_string().contains("foo") || err.to_string().contains("unknown variant"),
            "expected unknown-variant error parsing Step, got: {err}"
        );
    }
}
