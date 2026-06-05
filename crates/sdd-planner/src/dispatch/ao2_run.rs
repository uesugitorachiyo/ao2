//! Dispatch bridge → `ao2 run` (README §10 P8).
//!
//! Translates `ao2.sdd-plan.v1` into the ao2-native runspec YAML
//! (`apiVersion: ao2.run/v1`, `kind: Run`).
//!
//! Reference shape: factory-v3's `apiVersion: ao.dev/v1` runspecs.
//! We rev the apiVersion to `ao2.run/v1` so ao2's loader can route on
//! it explicitly. Field order is fixed in struct declaration order,
//! which `serde_yaml::to_string` honors — so emission is byte-stable
//! across runs and platforms.
//!
//! ## Invariants
//!
//! - Step IDs preserved 1:1 (Plan.steps[i].id == spec.tasks[i].id).
//! - `step.depends_on` → `task.deps` 1:1 (same order).
//! - `step.kind` preserved (`StepKind` lowercase string).
//! - Trust boundary, exit criteria, prompt, target carried through.
//! - Translation is pure: no I/O, no env access, no clock reads.

use serde::{Deserialize, Serialize};

use crate::schema::{
    ExitCriteria, Plan, PlanKind, Prompt, Provenance, StepKind, Target, TrustBoundary,
};

pub const AO2_RUN_API_VERSION: &str = "ao2.run/v1";
pub const AO2_RUN_KIND: &str = "Run";
pub const SOURCE_SCHEMA_VERSION: &str = "ao2.sdd-plan.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ao2RunSpec {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: Spec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Spec {
    pub source: SourceRef,
    pub plan_kind: PlanKind,
    pub goal: String,
    pub non_goals: Vec<String>,
    pub prompt: Prompt,
    pub target: Target,
    pub provenance: Provenance,
    pub trust_boundary: TrustBoundary,
    pub tasks: Vec<Task>,
    pub exit_criteria: ExitCriteria,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRef {
    pub schema_version: String,
    pub plan_id: String,
    pub generated_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub kind: StepKind,
    pub deps: Vec<String>,
    pub paths: Vec<String>,
    pub rationale: String,
    pub acceptance: Vec<String>,
}

/// Translate a validated `Plan` into the ao2-run YAML model.
pub fn to_runspec(plan: &Plan) -> Ao2RunSpec {
    let tasks: Vec<Task> = plan
        .plan
        .steps
        .iter()
        .map(|s| Task {
            id: s.id.clone(),
            kind: s.kind,
            deps: s.depends_on.clone(),
            paths: s.paths.clone(),
            rationale: s.rationale.clone(),
            acceptance: s.acceptance.clone(),
        })
        .collect();

    Ao2RunSpec {
        api_version: AO2_RUN_API_VERSION.to_string(),
        kind: AO2_RUN_KIND.to_string(),
        metadata: Metadata {
            name: plan.plan_id.clone(),
            description: plan.plan.title.clone(),
        },
        spec: Spec {
            source: SourceRef {
                schema_version: SOURCE_SCHEMA_VERSION.to_string(),
                plan_id: plan.plan_id.clone(),
                generated_at_utc: plan.generated_at_utc.clone(),
            },
            plan_kind: plan.plan.kind,
            goal: plan.plan.goal.clone(),
            non_goals: plan.plan.non_goals.clone(),
            prompt: plan.prompt.clone(),
            target: plan.target.clone(),
            provenance: plan.provenance.clone(),
            trust_boundary: plan.trust_boundary.clone(),
            tasks,
            exit_criteria: plan.plan.exit_criteria.clone(),
        },
    }
}

/// Emit the runspec as YAML. Field order is fixed by struct
/// declaration order; `serde_yaml::to_string` is deterministic and
/// preserves that order, so two emissions of the same plan are
/// byte-equal.
pub fn emit_yaml(rs: &Ao2RunSpec) -> String {
    serde_yaml::to_string(rs).expect("ao2 runspec yaml serialization")
}
