//! sdd-planner — converts (prompt, target_repo) → ao2.sdd-plan.v1
//!
//! Architecture: see `crates/sdd-planner/src/provider/spec.md` and
//! `docs/roadmap/PHASE-2-FACTORY-V3-RETIREMENT.md`.

pub mod context;
pub mod dispatch;
pub mod orchestrator;
pub mod provider;
pub mod schema;
pub mod surface;
pub mod validator;

pub use context::{shrink, DEFAULT_BUDGET_TOKENS};
pub use orchestrator::{orchestrate, OrchestrateError, PlanOutcome, ATTEMPT_BUDGET};
pub use provider::{Provider, ProviderError, ProviderRequest};
pub use schema::{
    Plan, PlanKind, Provenance, Step, StepKind, SurfaceFile, SurfaceMap, TrustBoundary,
};
pub use surface::{canonical_json, kind_for_extension, scan};
pub use validator::{validate, ValidationError, ValidationReport};
