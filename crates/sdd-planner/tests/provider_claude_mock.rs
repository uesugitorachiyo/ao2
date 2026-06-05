//! P4 acceptance tests for the claude provider adapter (README §10 P4).
//!
//! Same shape as the codex adapter tests; together they prove that
//! `--provider {codex,claude}` is just a selector — the rest of the
//! pipeline is provider-agnostic.

use std::path::PathBuf;

use sdd_planner::provider::claude::ClaudeProvider;
use sdd_planner::provider::{
    Provider, ProviderError, ProviderRequest, CANDIDATE_SCHEMA, PROVIDER_CLAUDE, PROVIDER_CODEX,
    REQUEST_SCHEMA_VERSION,
};
use sdd_planner::schema::{SurfaceFile, SurfaceMap};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mock_bin() -> String {
    crate_root()
        .join("tests/mock-bins/claude")
        .to_string_lossy()
        .into_owned()
}

fn default_candidate_fixture() -> String {
    crate_root()
        .join("tests/fixtures/claude-candidate.json")
        .to_string_lossy()
        .into_owned()
}

fn sample_request() -> ProviderRequest {
    ProviderRequest::new(
        "Build a tiny CLI.",
        SurfaceMap {
            head_sha: "0".repeat(40),
            files: vec![SurfaceFile {
                kind: "rust".to_string(),
                path: "src/main.rs".to_string(),
                public_symbols: Vec::new(),
                sha256: "0".repeat(64),
            }],
        },
        Vec::new(),
    )
}

#[test]
fn happy_path_returns_parsed_json() {
    let provider = ClaudeProvider::with_command(mock_bin())
        .with_env("SDD_MOCK_STDOUT", default_candidate_fixture());
    let value = provider.draft(&sample_request()).expect("draft");
    assert_eq!(
        value.get("schema_version").and_then(|v| v.as_str()),
        Some(CANDIDATE_SCHEMA)
    );
    assert_eq!(
        value
            .get("provenance")
            .and_then(|p| p.get("provider"))
            .and_then(|v| v.as_str()),
        Some("claude"),
        "claude fixture should self-identify as provider=claude"
    );
}

#[test]
fn non_zero_exit_yields_exit_non_zero_error() {
    let provider = ClaudeProvider::with_command(mock_bin()).with_env("SDD_MOCK_EXIT", "13");
    let err = provider.draft(&sample_request()).expect_err("err");
    match err {
        ProviderError::ExitNonZero { code, .. } => assert_eq!(code, 13),
        other => panic!("expected ExitNonZero, got {other:?}"),
    }
}

#[test]
fn non_json_output_yields_non_json_error() {
    let provider = ClaudeProvider::with_command(mock_bin()).with_env("SDD_MOCK_BAD_JSON", "1");
    let err = provider.draft(&sample_request()).expect_err("err");
    assert!(matches!(err, ProviderError::NonJson { .. }));
}

#[test]
fn stdin_envelope_matches_spec_8_1() {
    let capture_dir = tempfile::tempdir().expect("tempdir");
    let capture_path = capture_dir.path().join("envelope.json");
    let provider = ClaudeProvider::with_command(mock_bin())
        .with_env("SDD_MOCK_CAPTURE", capture_path.to_string_lossy())
        .with_env("SDD_MOCK_STDOUT", default_candidate_fixture());

    let request = sample_request();
    let request_id = request.request_id.clone();
    provider.draft(&request).expect("draft");

    let captured = std::fs::read_to_string(&capture_path).expect("read envelope");
    let envelope: serde_json::Value = serde_json::from_str(&captured).expect("envelope JSON");
    assert_eq!(
        envelope.get("schema_version").and_then(|v| v.as_str()),
        Some(REQUEST_SCHEMA_VERSION)
    );
    assert_eq!(
        envelope.get("request_id").and_then(|v| v.as_str()),
        Some(request_id.as_str())
    );
}

#[test]
fn default_command_is_bare_claude_on_path() {
    assert_eq!(ClaudeProvider::new().command(), "claude");
}

#[test]
fn provider_constants_match_cli_flag_values() {
    // README §10 P4 acceptance: "Selectable via the orchestrator's
    // --provider flag" — these are the literal flag values P5 wires up.
    assert_eq!(PROVIDER_CODEX, "codex");
    assert_eq!(PROVIDER_CLAUDE, "claude");
}
