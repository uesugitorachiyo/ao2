//! P8 — dispatch bridge to `ao2 run` (README §10 P8).
//!
//! Acceptance coverage:
//!   1. `ao2.sdd-plan.v1` → ao2 native runspec YAML (apiVersion +
//!      kind correct, parses as YAML, contains every step ID, every
//!      depends_on edge is faithfully copied to task.deps).
//!   2. Determinism: two emissions of the same plan are byte-equal.
//!   3. Round-trip via YAML: emit → parse → re-emit yields byte-equal
//!      output (proves the YAML serializer has no hidden state).
//!   4. Content-equal to `expected-ao2-runspec.yaml` golden.
//!
//! The README's `ao2 run --dry-run --spec` smoke is gated by ao2-cli
//! support (P6, owned by Integrator role and landed via user PR); the
//! library-level acceptance covered here is sufficient to unblock P9.

use std::fs;
use std::path::PathBuf;

use sdd_planner::dispatch::ao2_run::{
    emit_yaml, to_runspec, Ao2RunSpec, AO2_RUN_API_VERSION, AO2_RUN_KIND,
};
use sdd_planner::schema::Plan;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_valid_full() -> Plan {
    let path = fixtures_dir().join("valid_full.json");
    let bytes = fs::read(&path).expect("read valid_full.json");
    serde_json::from_slice::<Plan>(&bytes).expect("parse valid_full.json as Plan")
}

#[test]
fn api_version_and_kind_pinned() {
    assert_eq!(AO2_RUN_API_VERSION, "ao2.run/v1");
    assert_eq!(AO2_RUN_KIND, "Run");
    let rs = to_runspec(&load_valid_full());
    assert_eq!(rs.api_version, AO2_RUN_API_VERSION);
    assert_eq!(rs.kind, AO2_RUN_KIND);
}

#[test]
fn every_step_id_lands_in_tasks() {
    let plan = load_valid_full();
    let rs = to_runspec(&plan);

    let plan_ids: Vec<&str> = plan.plan.steps.iter().map(|s| s.id.as_str()).collect();
    let task_ids: Vec<&str> = rs.spec.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(plan_ids, task_ids, "task order/IDs must match plan.steps");
}

#[test]
fn depends_on_copied_into_task_deps() {
    let plan = load_valid_full();
    let rs = to_runspec(&plan);

    for (step, task) in plan.plan.steps.iter().zip(rs.spec.tasks.iter()) {
        assert_eq!(step.id, task.id);
        assert_eq!(
            step.depends_on, task.deps,
            "task.deps must equal step.depends_on (id={})",
            step.id
        );
    }
}

#[test]
fn yaml_emission_is_deterministic() {
    let plan = load_valid_full();
    let a = emit_yaml(&to_runspec(&plan));
    let b = emit_yaml(&to_runspec(&plan));
    assert_eq!(a, b, "two emissions of the same plan must be byte-equal");
}

#[test]
fn yaml_round_trip_preserves_bytes() {
    let plan = load_valid_full();
    let yaml_a = emit_yaml(&to_runspec(&plan));
    let parsed: Ao2RunSpec = serde_yaml::from_str(&yaml_a).expect("re-parse yaml");
    let yaml_b = emit_yaml(&parsed);
    assert_eq!(
        yaml_a, yaml_b,
        "emit → parse → emit must round-trip byte-equal"
    );
}

#[test]
fn yaml_matches_golden_fixture() {
    let plan = load_valid_full();
    let actual = emit_yaml(&to_runspec(&plan));

    let golden_path = fixtures_dir().join("expected-ao2-runspec.yaml");

    if std::env::var("SDD_WRITE_GOLDEN").as_deref() == Ok("1") {
        fs::write(&golden_path, &actual).expect("write golden fixture");
    }

    let expected = fs::read_to_string(&golden_path).expect("read expected-ao2-runspec.yaml");
    assert_eq!(
        normalize_newlines(&actual),
        normalize_newlines(&expected),
        "YAML diverged from golden fixture"
    );
}

fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n")
}
