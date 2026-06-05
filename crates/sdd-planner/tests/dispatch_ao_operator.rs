//! P7 — dispatch bridge to ao-operator (README §10 P7).
//!
//! Acceptance proofs:
//!   1. Step IDs preserved 1:1 (plan → runspec.nodes).
//!   2. Dependency edges preserved 1:1 (depends_on → runspec.edges).
//!   3. Round-trip stable: `to_runspec → from_runspec → to_runspec`
//!      yields byte-equal canonical JSON.
//!   4. Determinism: `to_runspec` called twice on the same plan emits
//!      byte-equal canonical JSON.
//!   5. Byte-equal to `expected-ao-operator-runspec.json` golden.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use sdd_planner::dispatch::ao_operator::{
    emit_canonical, from_runspec, to_runspec, AO_OPERATOR_SCHEMA_VERSION,
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
fn schema_version_is_pinned() {
    assert_eq!(AO_OPERATOR_SCHEMA_VERSION, "ao.run-operator.v1");
}

#[test]
fn every_step_id_preserved_in_nodes() {
    let plan = load_valid_full();
    let runspec = to_runspec(&plan);

    let plan_ids: Vec<&str> = plan.plan.steps.iter().map(|s| s.id.as_str()).collect();
    let node_ids: Vec<&str> = runspec.nodes.iter().map(|n| n.id.as_str()).collect();

    assert_eq!(
        plan_ids, node_ids,
        "node order and IDs must match plan.steps 1:1"
    );
}

#[test]
fn dependency_edges_preserved_one_to_one() {
    let plan = load_valid_full();
    let runspec = to_runspec(&plan);

    // Count: total depends_on entries == total edges.
    let total_deps: usize = plan.plan.steps.iter().map(|s| s.depends_on.len()).sum();
    assert_eq!(
        total_deps,
        runspec.edges.len(),
        "edge count must equal total depends_on entries"
    );

    // Set equality: each (parent → child) pair from the plan appears
    // exactly once in edges (and vice versa).
    let plan_pairs: HashSet<(String, String)> = plan
        .plan
        .steps
        .iter()
        .flat_map(|s| s.depends_on.iter().map(move |d| (d.clone(), s.id.clone())))
        .collect();
    let edge_pairs: HashSet<(String, String)> = runspec
        .edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();
    assert_eq!(
        plan_pairs, edge_pairs,
        "edge multiset must match depends_on"
    );
}

#[test]
fn round_trip_yields_byte_equal_canonical_json() {
    let plan = load_valid_full();
    let rs1 = to_runspec(&plan);
    let bytes_a = emit_canonical(&rs1);

    // runspec → plan → runspec — must canonicalize to same bytes.
    let plan2 = from_runspec(&rs1);
    let rs2 = to_runspec(&plan2);
    let bytes_b = emit_canonical(&rs2);

    assert_eq!(
        bytes_a, bytes_b,
        "round-trip must be byte-stable; diverged at canonical emission"
    );
}

#[test]
fn to_runspec_is_deterministic() {
    let plan = load_valid_full();
    let a = emit_canonical(&to_runspec(&plan));
    let b = emit_canonical(&to_runspec(&plan));
    assert_eq!(a, b, "two translations of the same plan must be byte-equal");
}

#[test]
fn canonical_emission_matches_golden_fixture() {
    let plan = load_valid_full();
    let actual = emit_canonical(&to_runspec(&plan));

    let golden_path = fixtures_dir().join("expected-ao-operator-runspec.json");

    // One-shot regeneration: `SDD_WRITE_GOLDEN=1 cargo test ...` writes
    // the current translator output to the fixture and then asserts.
    // No newline appended — README §5.3 forbids trailing newlines.
    if std::env::var("SDD_WRITE_GOLDEN").as_deref() == Ok("1") {
        fs::write(&golden_path, &actual).expect("write golden fixture");
    }

    let golden = fs::read_to_string(&golden_path).expect("read expected-ao-operator-runspec.json");
    let expected = golden.trim_end_matches('\n').to_string();

    assert_eq!(
        actual, expected,
        "canonical emission diverged from golden fixture"
    );
}
