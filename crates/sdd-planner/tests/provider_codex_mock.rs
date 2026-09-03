//! P3 acceptance tests for the codex provider adapter (README §10 P3,
//! §8.1–§8.4).
//!
//! Tests invoke a portable shell mock at `tests/mock-bins/codex` rather
//! than the real CLI, so they require no network, no OAuth token, and
//! no installed codex. The mock honours env-var knobs to simulate exit
//! codes, non-JSON output, and stdin capture. Env vars are applied
//! per-spawn via `CodexProvider::with_env`, never to the parent
//! process — tests are therefore parallel-safe.

use std::path::PathBuf;

use sdd_planner::provider::codex::CodexProvider;
use sdd_planner::provider::{
    Provider, ProviderError, ProviderRequest, CANDIDATE_SCHEMA, REQUEST_SCHEMA_VERSION,
};
use sdd_planner::schema::{SurfaceFile, SurfaceMap};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mock_bin() -> String {
    crate_root()
        .join("tests/mock-bins/codex")
        .to_string_lossy()
        .into_owned()
}

fn default_candidate_fixture() -> String {
    crate_root()
        .join("tests/fixtures/codex-candidate.json")
        .to_string_lossy()
        .into_owned()
}

fn sample_surface_map() -> SurfaceMap {
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

fn sample_request() -> ProviderRequest {
    ProviderRequest::new("Build a tiny CLI.", sample_surface_map(), Vec::new())
}

#[test]
fn happy_path_returns_parsed_json() {
    let provider = CodexProvider::with_command(mock_bin())
        .with_env("SDD_MOCK_STDOUT", default_candidate_fixture());
    let value = provider
        .draft(&sample_request())
        .expect("draft on happy path");
    assert_eq!(
        value
            .get("schema_version")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        CANDIDATE_SCHEMA,
        "candidate must carry the expected schema_version"
    );
    assert!(value.get("plan").is_some(), "plan field missing");
}

#[test]
fn non_zero_exit_yields_exit_non_zero_error() {
    let provider = CodexProvider::with_command(mock_bin()).with_env("SDD_MOCK_EXIT", "7");
    let err = provider
        .draft(&sample_request())
        .expect_err("expected error on non-zero exit");
    match err {
        ProviderError::ExitNonZero { code, .. } => assert_eq!(code, 7),
        other => panic!("expected ExitNonZero, got {other:?}"),
    }
}

#[test]
fn non_json_output_yields_non_json_error() {
    let provider = CodexProvider::with_command(mock_bin()).with_env("SDD_MOCK_BAD_JSON", "1");
    let err = provider
        .draft(&sample_request())
        .expect_err("expected error on non-JSON output");
    match err {
        ProviderError::NonJson { reason, .. } => {
            assert!(
                reason.contains("this is not JSON"),
                "NonJson reason should include offending stdout; got {reason}"
            );
        }
        other => panic!("expected NonJson, got {other:?}"),
    }
}

#[test]
fn stdin_envelope_matches_spec_8_1() {
    let capture_dir = tempfile::tempdir().expect("tempdir");
    let capture_path = capture_dir.path().join("envelope.json");
    let provider = CodexProvider::with_command(mock_bin())
        .with_env("SDD_MOCK_CAPTURE", capture_path.to_string_lossy())
        .with_env("SDD_MOCK_STDOUT", default_candidate_fixture());

    let request = sample_request();
    let request_id = request.request_id.clone();
    provider.draft(&request).expect("draft");

    let captured = std::fs::read_to_string(&capture_path).expect("read captured envelope");
    let envelope: serde_json::Value =
        serde_json::from_str(&captured).expect("captured stdin must be JSON");

    assert_eq!(
        envelope.get("schema_version").and_then(|v| v.as_str()),
        Some(REQUEST_SCHEMA_VERSION),
        "envelope schema_version must match §8.1"
    );
    assert_eq!(
        envelope.get("request_id").and_then(|v| v.as_str()),
        Some(request_id.as_str()),
        "request_id round-trips"
    );
    assert_eq!(
        envelope.get("prompt").and_then(|v| v.as_str()),
        Some("Build a tiny CLI."),
        "prompt round-trips"
    );
    let ctx = envelope
        .get("context")
        .expect("envelope must have context (§8.1)");
    assert!(
        ctx.get("surface_map").is_some(),
        "context.surface_map missing"
    );
    assert!(
        ctx.get("prior_errors").is_some(),
        "context.prior_errors missing"
    );
    let source_policy = ctx
        .get("software_source_policy")
        .and_then(|value| value.as_str())
        .expect("context.software_source_policy missing");
    for (case, required) in [
        ("existing capability", "No source change"),
        (
            "reuse or standard library",
            "standard library or native platform",
        ),
        ("small suitable change", "smallest cohesive change"),
        (
            "overloaded source growth",
            "unhealthy file or function growth",
        ),
        ("cohesive new module", "minimum new module"),
        (
            "behavior-neutral oversized touch",
            "behavior-neutral touches without growth",
        ),
        (
            "generated or vendored exception",
            "generated or vendored source",
        ),
        (
            "cohesive split exception",
            "splitting would worsen the design",
        ),
        ("unjustified source layer", "No speculative scaffolding"),
        (
            "large non-source artifact",
            "non-source artifacts are outside this policy",
        ),
    ] {
        assert!(
            source_policy.contains(required),
            "software source policy omitted representative case {case:?}: {source_policy}"
        );
    }
    assert!(source_policy.contains("exact base/head source diff"));
    assert!(source_policy.contains("repository-native"));
    assert!(source_policy.contains("security controls"));
    let expected = envelope
        .get("expected_output")
        .expect("envelope must have expected_output (§8.1)");
    assert_eq!(
        expected.get("schema").and_then(|v| v.as_str()),
        Some(CANDIDATE_SCHEMA)
    );
    assert_eq!(expected.get("max_steps").and_then(|v| v.as_u64()), Some(25));
}

#[test]
fn default_command_is_bare_codex_on_path() {
    let p = CodexProvider::new();
    assert_eq!(p.command(), "codex");
}

#[test]
fn missing_binary_yields_io_error_not_panic() {
    let provider =
        CodexProvider::with_command("/definitely/does/not/exist/sdd-codex-xyzzy".to_string());
    let err = provider
        .draft(&sample_request())
        .expect_err("expected I/O error for missing binary");
    match err {
        ProviderError::Io { .. } => (),
        other => panic!("expected Io, got {other:?}"),
    }
}
