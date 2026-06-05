//! P5 end-to-end tests for the orchestrator (README §10 P5, §6).
//!
//! Exercises the retry / fail-closed protocol against two scripted
//! mock providers:
//!   - `orchestrator-bad-then-good`: attempt 1 invalid, attempt 2 clean.
//!   - `orchestrator-always-bad`: every attempt invalid → exhaust at 3.

use std::path::PathBuf;

use sdd_planner::orchestrator::{orchestrate, OrchestrateError, ATTEMPT_BUDGET};
use sdd_planner::provider::codex::CodexProvider;
use sdd_planner::schema::{SurfaceFile, SurfaceMap};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bad_then_good_cli() -> String {
    crate_root()
        .join("tests/fixtures/orchestrator-bad-then-good/cli.sh")
        .to_string_lossy()
        .into_owned()
}

fn always_bad_cli() -> String {
    crate_root()
        .join("tests/fixtures/orchestrator-always-bad/cli.sh")
        .to_string_lossy()
        .into_owned()
}

fn surface_map_for_fixture() -> SurfaceMap {
    SurfaceMap {
        head_sha: "0".repeat(40),
        files: vec![SurfaceFile {
            kind: "rust".to_string(),
            path: "src/main.rs".to_string(),
            public_symbols: Vec::new(),
            sha256: "0".repeat(64),
        }],
    }
}

#[test]
fn attempt_budget_constant_is_three() {
    // README §6: ATTEMPT_BUDGET = 3, hard cap, no override.
    assert_eq!(ATTEMPT_BUDGET, 3);
}

#[test]
fn bad_then_good_succeeds_on_attempt_2() {
    let work = tempfile::tempdir().expect("workdir");
    let state = tempfile::tempdir().expect("state");
    let provider = CodexProvider::with_command(bad_then_good_cli())
        .with_env("SDD_MOCK_STATE_DIR", state.path().to_string_lossy());

    let outcome = orchestrate(
        &provider,
        "Build a tiny CLI.",
        &surface_map_for_fixture(),
        work.path(),
        work.path(),
        "codex",
    )
    .expect("orchestrate should succeed by attempt 2");

    assert_eq!(outcome.attempts_used, 2, "should land on attempt 2");

    // Both attempts logged.
    let attempt_1 = outcome.build_log_dir.join("attempt-1.json");
    let attempt_2 = outcome.build_log_dir.join("attempt-2.json");
    assert!(attempt_1.exists(), "attempt-1.json missing");
    assert!(attempt_2.exists(), "attempt-2.json missing");

    // Success path: NO failure artifacts.
    assert!(
        !outcome.build_log_dir.join("candidate.fail.json").exists(),
        "candidate.fail.json should NOT exist on success"
    );
    assert!(
        !outcome.build_log_dir.join("validation-errors.txt").exists(),
        "validation-errors.txt should NOT exist on success"
    );

    // Canonical emission carries the promoted schema version.
    assert!(
        outcome.canonical_json.contains("\"ao2.sdd-plan.v1\""),
        "canonical JSON must carry ao2.sdd-plan.v1; got: {}",
        outcome.canonical_json
    );
    assert_eq!(outcome.plan.schema_version, "ao2.sdd-plan.v1");
    // Promoted attempts counter equals attempt index where success occurred.
    assert_eq!(outcome.plan.provenance.attempts, 2);
}

#[test]
fn always_bad_exhausts_at_attempt_three() {
    let work = tempfile::tempdir().expect("workdir");
    let provider = CodexProvider::with_command(always_bad_cli());

    let err = orchestrate(
        &provider,
        "Build a tiny CLI.",
        &surface_map_for_fixture(),
        work.path(),
        work.path(),
        "codex",
    )
    .expect_err("always-bad should exhaust the budget");

    let dir = match err {
        OrchestrateError::PlanExhausted {
            budget,
            build_log_dir,
        } => {
            assert_eq!(budget, ATTEMPT_BUDGET);
            build_log_dir
        }
        other => panic!("expected PlanExhausted, got {other:?}"),
    };

    // All 3 attempts logged.
    for n in 1..=ATTEMPT_BUDGET {
        let path = dir.join(format!("attempt-{n}.json"));
        assert!(path.exists(), "attempt-{n}.json missing");
    }
    // Failure artifacts written.
    let fail_path = dir.join("candidate.fail.json");
    let errs_path = dir.join("validation-errors.txt");
    assert!(fail_path.exists(), "candidate.fail.json must exist");
    assert!(errs_path.exists(), "validation-errors.txt must exist");

    // The validation errors text should call out V5 (MutatingPlanner)
    // since the fixture flips trust_boundary.mutates_ao_artifacts=true.
    let errs = std::fs::read_to_string(&errs_path).expect("read errors");
    assert!(
        errs.contains("V5") || errs.to_lowercase().contains("mutate"),
        "validation-errors.txt should reference V5 / mutating: {errs}"
    );
}

#[test]
fn success_outcome_plan_id_matches_directory() {
    let work = tempfile::tempdir().expect("workdir");
    let state = tempfile::tempdir().expect("state");
    let provider = CodexProvider::with_command(bad_then_good_cli())
        .with_env("SDD_MOCK_STATE_DIR", state.path().to_string_lossy());

    let outcome = orchestrate(
        &provider,
        "Build a tiny CLI.",
        &surface_map_for_fixture(),
        work.path(),
        work.path(),
        "codex",
    )
    .expect("orchestrate");

    // build_log_dir is named after the candidate's plan_id (per §6
    // "target/sdd-planner/<plan_id>").
    let last_segment = outcome
        .build_log_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    assert_eq!(last_segment, outcome.plan_id);
}

#[test]
fn provider_error_short_circuits_without_retry() {
    // Provider that exits non-zero — orchestrator must surface
    // ProviderError, NOT retry past it.
    let work = tempfile::tempdir().expect("workdir");
    let provider = CodexProvider::with_command(
        crate_root()
            .join("tests/mock-bins/codex")
            .to_string_lossy()
            .into_owned(),
    )
    .with_env("SDD_MOCK_EXIT", "1");

    let err = orchestrate(
        &provider,
        "anything",
        &surface_map_for_fixture(),
        work.path(),
        work.path(),
        "codex",
    )
    .expect_err("expected provider error to propagate");

    matches!(err, OrchestrateError::Provider(_))
        .then_some(())
        .unwrap_or_else(|| panic!("expected Provider error, got {err:?}"));
}
