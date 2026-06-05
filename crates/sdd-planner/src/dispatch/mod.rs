//! Dispatch bridges — translate `ao2.sdd-plan.v1` into runner-native
//! runspecs. README §10 P7 (ao-operator) and P8 (ao2 run).
//!
//! Each runner has its own translator module. Translators are pure
//! functions over the `Plan` schema; they do not mutate ao artifacts
//! and never shell out. Canonical JSON / YAML emission lives in the
//! per-runner module so byte-stability can be unit-tested in isolation.

pub mod ao2_run;
pub mod ao_operator;

pub use ao2_run::{
    emit_yaml as emit_ao2_run_yaml, to_runspec as ao2_run_to_runspec, Ao2RunSpec,
    AO2_RUN_API_VERSION, AO2_RUN_KIND,
};
pub use ao_operator::{
    emit_canonical as emit_ao_operator_canonical, from_runspec as ao_operator_from_runspec,
    to_runspec as ao_operator_to_runspec, AoOperatorRunspec, RunspecEdge, RunspecNode,
    SourceRef as AoOperatorSourceRef, AO_OPERATOR_SCHEMA_VERSION,
};
