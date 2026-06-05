//! Dispatch bridge → ao-operator (README §10 P7).
//!
//! Translates `ao2.sdd-plan.v1` into an `ao.run-operator.v1` runspec.
//!
//! ## Invariants
//!
//! - Step IDs preserved 1:1 (Plan.steps[i].id == RunSpec.nodes[i].id).
//! - Dependency edges preserved 1:1: for every `dep ∈ step.depends_on`
//!   we emit exactly one `RunspecEdge { from: dep, to: step.id }`.
//!   Order is plan-step-major, then depends_on-major — fully
//!   deterministic.
//! - Round-trip stable: `from_runspec(to_runspec(plan)) == plan` and
//!   `emit_canonical(to_runspec(plan))` is byte-equal across calls and
//!   across `to_runspec → from_runspec → to_runspec` chains.
//! - Translation is pure: no I/O, no env access, no clock reads.

use serde::{Deserialize, Serialize};

use crate::schema::{
    ExitCriteria, Plan, PlanBody, PlanKind, Prompt, Provenance, Step, StepKind, Target,
    TrustBoundary,
};
use crate::surface::canonical_json;

pub const AO_OPERATOR_SCHEMA_VERSION: &str = "ao.run-operator.v1";
pub const SOURCE_SCHEMA_VERSION: &str = "ao2.sdd-plan.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AoOperatorRunspec {
    pub edges: Vec<RunspecEdge>,
    pub exit_criteria: ExitCriteria,
    pub generated_at_utc: String,
    pub goal: String,
    pub kind: PlanKind,
    pub non_goals: Vec<String>,
    pub nodes: Vec<RunspecNode>,
    pub plan_id: String,
    pub prompt: Prompt,
    pub provenance: Provenance,
    pub runspec_id: String,
    pub schema_version: String,
    pub source: SourceRef,
    pub target: Target,
    pub title: String,
    pub trust_boundary: TrustBoundary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunspecNode {
    pub acceptance: Vec<String>,
    pub id: String,
    pub kind: StepKind,
    pub paths: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunspecEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRef {
    pub schema_version: String,
}

/// Translate a validated `Plan` into an ao-operator runspec.
pub fn to_runspec(plan: &Plan) -> AoOperatorRunspec {
    let nodes: Vec<RunspecNode> = plan
        .plan
        .steps
        .iter()
        .map(|s| RunspecNode {
            acceptance: s.acceptance.clone(),
            id: s.id.clone(),
            kind: s.kind,
            paths: s.paths.clone(),
            rationale: s.rationale.clone(),
        })
        .collect();

    let mut edges: Vec<RunspecEdge> = Vec::new();
    for step in &plan.plan.steps {
        for dep in &step.depends_on {
            edges.push(RunspecEdge {
                from: dep.clone(),
                to: step.id.clone(),
            });
        }
    }

    AoOperatorRunspec {
        edges,
        exit_criteria: plan.plan.exit_criteria.clone(),
        generated_at_utc: plan.generated_at_utc.clone(),
        goal: plan.plan.goal.clone(),
        kind: plan.plan.kind,
        non_goals: plan.plan.non_goals.clone(),
        nodes,
        plan_id: plan.plan_id.clone(),
        prompt: plan.prompt.clone(),
        provenance: plan.provenance.clone(),
        runspec_id: plan.plan_id.clone(),
        schema_version: AO_OPERATOR_SCHEMA_VERSION.to_string(),
        source: SourceRef {
            schema_version: SOURCE_SCHEMA_VERSION.to_string(),
        },
        target: plan.target.clone(),
        title: plan.plan.title.clone(),
        trust_boundary: plan.trust_boundary.clone(),
    }
}

/// Reconstruct the originating `Plan` from a runspec. Used to prove
/// round-trip stability under the §10 P7 acceptance.
pub fn from_runspec(rs: &AoOperatorRunspec) -> Plan {
    let steps: Vec<Step> = rs
        .nodes
        .iter()
        .map(|n| {
            let depends_on: Vec<String> = rs
                .edges
                .iter()
                .filter(|e| e.to == n.id)
                .map(|e| e.from.clone())
                .collect();
            Step {
                id: n.id.clone(),
                kind: n.kind,
                paths: n.paths.clone(),
                rationale: n.rationale.clone(),
                acceptance: n.acceptance.clone(),
                depends_on,
            }
        })
        .collect();

    Plan {
        schema_version: SOURCE_SCHEMA_VERSION.to_string(),
        plan_id: rs.plan_id.clone(),
        generated_at_utc: rs.generated_at_utc.clone(),
        prompt: rs.prompt.clone(),
        target: rs.target.clone(),
        plan: PlanBody {
            kind: rs.kind,
            title: rs.title.clone(),
            goal: rs.goal.clone(),
            non_goals: rs.non_goals.clone(),
            steps,
            exit_criteria: rs.exit_criteria.clone(),
        },
        provenance: rs.provenance.clone(),
        trust_boundary: rs.trust_boundary.clone(),
        quality: None,
    }
}

/// Emit the runspec as canonical JSON (recursive key-sort, no
/// whitespace, no trailing newline) — same convention as Plan
/// canonical emission, so two translators on the same plan produce
/// byte-equal output regardless of struct field declaration order.
pub fn emit_canonical(rs: &AoOperatorRunspec) -> String {
    let value = serde_json::to_value(rs).expect("runspec serialization");
    canonical_json(&value)
}
