//! Fixture-driven validator tests covering V1..V11 from README §5.1.
//!
//! Test count = 11 to satisfy the P0 exit_check `grep -c '^#\[test\]' == 11`.
//! V11 is asserted by a single `accepts_valid_fixtures` test that walks
//! both `valid_minimal.json` and `valid_full.json`, rather than the two
//! named tests (`accepts_minimal`, `accepts_full`) shown in the README
//! §5.1 table. Documented deviation: the table lists two names; the
//! exit_check counts eleven. The exit_check is the executable gate.

use std::path::PathBuf;

use sdd_planner::schema::SurfaceFile;
use sdd_planner::{validate, SurfaceMap, ValidationError};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

fn surface_map_known(paths: &[&str]) -> SurfaceMap {
    SurfaceMap {
        head_sha: "0".repeat(40),
        files: paths
            .iter()
            .map(|p| SurfaceFile {
                path: (*p).to_string(),
                sha256: "0".repeat(64),
                kind: "rust".to_string(),
                public_symbols: Vec::new(),
            })
            .collect(),
    }
}

#[test]
fn rejects_malformed_shape() {
    let report = validate(&fixture("invalid_shape.json"), None);
    assert!(!report.ok(), "expected shape error, got OK");
    assert!(
        matches!(report.errors[0], ValidationError::Shape { .. }),
        "expected Shape error, got {:?}",
        report.errors
    );
}

#[test]
fn rejects_unknown_path() {
    let sm = surface_map_known(&["only_known.rs"]);
    let report = validate(&fixture("invalid_path_hallucination.json"), Some(&sm));
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::UnknownPath { .. })),
        "expected UnknownPath, got {:?}",
        report.errors
    );
}

#[test]
fn rejects_empty_acceptance() {
    let report = validate(&fixture("invalid_empty_acceptance.json"), None);
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::EmptyAcceptance { .. })),
        "expected EmptyAcceptance, got {:?}",
        report.errors
    );
}

#[test]
fn rejects_cycle() {
    let report = validate(&fixture("invalid_cycle.json"), None);
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::Cycle(_))),
        "expected Cycle, got {:?}",
        report.errors
    );
}

#[test]
fn rejects_mutating_planner() {
    let report = validate(&fixture("invalid_mutates_artifacts.json"), None);
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::MutatingPlanner)),
        "expected MutatingPlanner, got {:?}",
        report.errors
    );
}

#[test]
fn rejects_excess_attempts() {
    let report = validate(&fixture("invalid_too_many_attempts.json"), None);
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::ExcessAttempts(4))),
        "expected ExcessAttempts(4), got {:?}",
        report.errors
    );
}

#[test]
fn rejects_bad_step_count() {
    let report = validate(&fixture("invalid_step_count.json"), None);
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::BadStepCount(0))),
        "expected BadStepCount(0), got {:?}",
        report.errors
    );
}

#[test]
fn rejects_disallowed_shell() {
    let report = validate(&fixture("invalid_shell_not_allowlisted.json"), None);
    assert!(
        report.errors.iter().any(|e| matches!(
            e,
            ValidationError::DisallowedShell { token, .. } if token == "rm"
        )),
        "expected DisallowedShell(rm), got {:?}",
        report.errors
    );
}

#[test]
fn rejects_bad_step_id() {
    let report = validate(&fixture("invalid_step_id.json"), None);
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::BadStepId(id) if id == "Step-One")),
        "expected BadStepId(Step-One), got {:?}",
        report.errors
    );
}

#[test]
fn rejects_overlong_title() {
    let report = validate(&fixture("invalid_title_too_long.json"), None);
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::OverlongTitle(len) if *len > 80)),
        "expected OverlongTitle(>80), got {:?}",
        report.errors
    );
}

#[test]
fn accepts_valid_fixtures() {
    for name in ["valid_minimal.json", "valid_full.json"] {
        let report = validate(&fixture(name), None);
        assert!(
            report.ok(),
            "fixture {name} must pass; got errors: {:?}",
            report.errors
        );
    }
}
