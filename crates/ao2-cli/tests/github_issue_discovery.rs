use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

const HEAD: &str = "1111111111111111111111111111111111111111";
const RUN_ID: &str = "repair-run-20260728";
const COMPLETED_AT: &str = "2026-07-28T00:00:00Z";

fn temp_file(name: &str, value: &Value) -> PathBuf {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("ao2-discovery-{name}-{id}.json"));
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn envelope(pages: Vec<Value>) -> Value {
    json!({
        "repository": "uesugitorachiyo/ao2",
        "default_branch": "main",
        "head_sha": HEAD,
        "pages": pages,
    })
}

fn issue(number: u64, updated_at: &str, classification: &str) -> Value {
    json!({
        "number": number,
        "state": "open",
        "updated_at": updated_at,
        "title": format!("Sanitized issue {number}"),
        "body": "Sanitized bounded discovery fixture.",
        "labels": ["bug"],
        "classification": classification,
        "reported_head_sha": HEAD,
        "fix_present_at_head": false,
        "environment_accessible": true,
        "security_sensitive": false,
        "target_in_repository": true,
        "no_existing_fix": true,
        "public_reproduction_feasible": true,
        "deterministic_local_reproduction": true,
        "expected_behavior_source": "tests",
        "bounded_policy_compatible": true,
    })
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}:{}",
                    serde_json::to_string(key).unwrap(),
                    canonical_json(value)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn candidate_document(
    issue_number: u64,
    rank: u64,
    decision: &str,
    evidence_digests: Value,
) -> Value {
    json!({
        "schema": "ao.architecture.autonomous-issue-repair.candidate-decision.v1",
        "run_id": RUN_ID,
        "repository": "uesugitorachiyo/ao2",
        "base_sha": HEAD,
        "issue_number": issue_number,
        "rank": rank,
        "decision": decision,
        "eligibility": {
            "open_bug": true,
            "target_in_repository": true,
            "no_existing_fix": true,
            "current_head_unfixed": true,
            "security_sensitive": false,
            "public_reproduction_feasible": true,
            "deterministic_local_reproduction": true,
            "expected_behavior_grounded": true,
            "bounded_policy_compatible": true,
        },
        "reason_codes": [if decision == "selected" { "selected_rank_1" } else { "eligible_rank_2" }],
        "evidence_digests": evidence_digests,
        "expected_behavior_source": "tests",
        "decided_at": COMPLETED_AT,
    })
}

fn candidate_digest(mut candidate: Value) -> String {
    candidate["decision_digest"] = Value::Null;
    candidate.as_object_mut().unwrap().remove("decision_digest");
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(canonical_json(&candidate).as_bytes())
    )
}

fn digest_value(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(canonical_json(value).as_bytes()))
}

fn assert_strict_candidate(candidate: &Value, discovery: &Value) {
    let required = [
        "schema",
        "run_id",
        "repository",
        "base_sha",
        "issue_number",
        "rank",
        "decision",
        "eligibility",
        "reason_codes",
        "evidence_digests",
        "expected_behavior_source",
        "decided_at",
        "decision_digest",
    ];
    assert_eq!(candidate.as_object().unwrap().len(), required.len());
    for key in required {
        assert!(candidate.get(key).is_some(), "missing {key}");
    }
    let eligibility = candidate["eligibility"].as_object().unwrap();
    for key in [
        "open_bug",
        "target_in_repository",
        "no_existing_fix",
        "current_head_unfixed",
        "public_reproduction_feasible",
        "deterministic_local_reproduction",
        "expected_behavior_grounded",
        "bounded_policy_compatible",
    ] {
        assert_eq!(eligibility[key], true, "{key}");
    }
    assert_eq!(eligibility["security_sensitive"], false);
    assert_ne!(candidate["expected_behavior_source"], "unavailable");
    assert_eq!(
        candidate["decision_digest"],
        candidate_digest(candidate.clone())
    );
    assert!(discovery["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| {
            entry["issue_number"] == candidate["issue_number"]
                && entry["decision_digest"] == candidate["decision_digest"]
        }));
    if candidate["decision"] == "selected" {
        assert_eq!(
            discovery["selected_issue_number"],
            candidate["issue_number"]
        );
    }
}

fn discover_with(
    path: &Path,
    url: &str,
    repository: &str,
    head_sha: &str,
    snapshot_limit: usize,
    candidate_limit: usize,
) -> std::process::Output {
    let args = vec![
        "issue".to_string(),
        "discover".to_string(),
        "--page-envelope".to_string(),
        path.display().to_string(),
        "--url".to_string(),
        url.to_string(),
        "--repository".to_string(),
        repository.to_string(),
        "--default-branch".to_string(),
        "main".to_string(),
        "--head-sha".to_string(),
        head_sha.to_string(),
        "--run-id".to_string(),
        RUN_ID.to_string(),
        "--completed-at".to_string(),
        COMPLETED_AT.to_string(),
        "--snapshot-limit".to_string(),
        snapshot_limit.to_string(),
        "--candidate-limit".to_string(),
        candidate_limit.to_string(),
        "--json".to_string(),
    ];
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(&args)
        .output()
        .unwrap()
}

fn discover(path: &Path) -> std::process::Output {
    discover_with(
        path,
        "https://github.com/uesugitorachiyo/ao2/issues/?ignored=yes#fragment",
        "uesugitorachiyo/ao2",
        HEAD,
        50,
        10,
    )
}

fn successful_discovery(path: &Path) -> Value {
    let output = discover(path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn emits_exact_schema_and_deterministic_bounded_selection() {
    let path = temp_file(
        "positive",
        &envelope(vec![
            json!({"page": 1, "issues": [
                issue(3, "2026-07-27T22:00:00Z", "bug"),
                issue(7, "2026-07-27T23:00:00Z", "bug"),
            ]}),
            json!({"page": 2, "issues": [issue(9, "2026-07-27T20:00:00Z", "feature_request")]}),
        ]),
    );
    let value = successful_discovery(&path);
    assert_eq!(
        value["schema"],
        "ao.architecture.autonomous-issue-repair.discovery-result.v1"
    );
    assert_eq!(
        value["source_url"],
        "https://github.com/uesugitorachiyo/ao2/issues"
    );
    assert_eq!(value["head_sha"], HEAD);
    assert_eq!(value["page_count"], 2);
    assert_eq!(value["issues"].as_array().unwrap().len(), 3);
    assert_eq!(value["candidates"][0]["issue_number"], 7);
    assert_eq!(value["candidates"][0]["rank"], 1);
    assert_eq!(value["selected_issue_number"], 7);
    assert_eq!(value["mutation_performed"], false);
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "candidate_limit",
            "candidates",
            "completed_at",
            "default_branch",
            "exclusion_ledger",
            "head_sha",
            "issues",
            "mutation_performed",
            "page_count",
            "repository",
            "response_digests",
            "run_id",
            "schema",
            "selected_issue_number",
            "selected_limit",
            "snapshot_limit",
            "source_url",
        ]
    );
    let exclusions = value["exclusion_ledger"].as_array().unwrap();
    assert_eq!(exclusions.len(), 2);
    assert!(exclusions.iter().any(|entry| entry["issue_number"] == 9));
    assert!(exclusions.iter().any(|entry| {
        entry["issue_number"] == 3 && entry["reason_codes"] == json!(["not_selected_rank_2"])
    }));
    let mut evidence = vec![
        value["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["number"] == 7)
            .unwrap()["content_digest"]
            .clone(),
        value["response_digests"][0].clone(),
    ];
    evidence.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    let mut selected = candidate_document(7, 1, "selected", Value::Array(evidence));
    selected["decision_digest"] = json!(value["candidates"][0]["decision_digest"]);
    assert_strict_candidate(&selected, &value);
    assert_eq!(
        value["candidates"][0]["decision_digest"],
        "849065f3794fcd4284edff1942979540c60d3556aea7a0a2044435590eb611f1"
    );
    let mut rank_two_evidence = vec![
        value["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["number"] == 3)
            .unwrap()["content_digest"]
            .clone(),
        value["response_digests"][0].clone(),
    ];
    rank_two_evidence.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    let mut eligible = candidate_document(3, 2, "eligible", Value::Array(rank_two_evidence));
    eligible["decision_digest"] = json!(value["candidates"][1]["decision_digest"]);
    assert_strict_candidate(&eligible, &value);
}

#[test]
fn cross_page_duplicates_bind_every_occurrence_digest_and_unique_snapshot_limit() {
    let first = issue(40, "2026-07-27T22:00:00Z", "bug");
    let mut second = issue(40, "2026-07-27T21:00:00Z", "bug");
    second["body"] = json!("A distinct sanitized duplicate occurrence.");
    let page_one = json!({"page": 1, "issues": [first.clone()]});
    let page_two = json!({"page": 2, "issues": [second.clone()]});
    let path = temp_file(
        "cross-page-duplicate",
        &envelope(vec![page_one.clone(), page_two.clone()]),
    );
    let value = successful_discovery(&path);
    assert_eq!(value["issues"].as_array().unwrap().len(), 1);
    assert_eq!(value["selected_issue_number"], Value::Null);
    let evidence = value["exclusion_ledger"][0]["evidence_digests"]
        .as_array()
        .unwrap();
    let mut expected = vec![
        digest_value(&first),
        digest_value(&second),
        digest_value(&page_one),
        digest_value(&page_two),
    ];
    expected.sort();
    assert_eq!(
        evidence,
        &expected.into_iter().map(Value::String).collect::<Vec<_>>()
    );

    let duplicate_rows = (0..51)
        .map(|_| issue(41, "2026-07-27T22:00:00Z", "bug"))
        .collect::<Vec<_>>();
    let unique_limited = temp_file(
        "unique-snapshot-limit",
        &envelope(vec![json!({"page": 1, "issues": duplicate_rows})]),
    );
    assert!(discover_with(
        &unique_limited,
        "https://github.com/uesugitorachiyo/ao2/issues",
        "uesugitorachiyo/ao2",
        HEAD,
        1,
        10,
    )
    .status
    .success());

    let raw_over_limit = (0..501)
        .map(|index| issue(index + 100, "2026-07-27T22:00:00Z", "bug"))
        .collect::<Vec<_>>();
    let raw_over_limit = temp_file(
        "raw-row-limit",
        &envelope(vec![json!({"page": 1, "issues": raw_over_limit})]),
    );
    assert!(!discover(&raw_over_limit).status.success());
}

#[test]
fn rejects_noncontiguous_pagination_and_temporal_or_classification_contradictions() {
    let gap = temp_file(
        "page-gap",
        &envelope(vec![
            json!({"page": 2, "issues": [issue(30, "2026-07-27T22:00:00Z", "bug")]} ),
        ]),
    );
    assert!(!discover(&gap).status.success());

    let mut future = issue(31, "2026-07-28T00:00:01Z", "bug");
    future["updated_at"] = json!("2026-07-28T00:00:01Z");
    let future = temp_file(
        "future",
        &envelope(vec![json!({"page": 1, "issues": [future]} )]),
    );
    assert!(!discover(&future).status.success());

    let mut inconsistent = issue(32, "2026-07-27T22:00:00Z", "inaccessible_environment");
    inconsistent["environment_accessible"] = json!(true);
    let inconsistent = temp_file(
        "classification",
        &envelope(vec![json!({"page": 1, "issues": [inconsistent]} )]),
    );
    assert!(!discover(&inconsistent).status.success());

    let already_fixed_without_evidence = temp_file(
        "already-fixed-inconsistent",
        &envelope(vec![
            json!({"page": 1, "issues": [issue(33, "2026-07-27T22:00:00Z", "already_fixed")]}),
        ]),
    );
    assert!(!discover(&already_fixed_without_evidence).status.success());

    let security_without_evidence = temp_file(
        "security-inconsistent",
        &envelope(vec![
            json!({"page": 1, "issues": [issue(34, "2026-07-27T22:00:00Z", "security_sensitive")]}),
        ]),
    );
    assert!(!discover(&security_without_evidence).status.success());

    let mut fixed_with_no_existing_fix = issue(35, "2026-07-27T22:00:00Z", "already_fixed");
    fixed_with_no_existing_fix["fix_present_at_head"] = json!(true);
    let fixed_with_no_existing_fix = temp_file(
        "fixed-with-no-existing-fix",
        &envelope(vec![
            json!({"page": 1, "issues": [fixed_with_no_existing_fix]}),
        ]),
    );
    assert!(!discover(&fixed_with_no_existing_fix).status.success());

    let mut unfixed_with_existing_fix = issue(36, "2026-07-27T22:00:00Z", "bug");
    unfixed_with_existing_fix["no_existing_fix"] = json!(false);
    let unfixed_with_existing_fix = temp_file(
        "unfixed-with-existing-fix",
        &envelope(vec![
            json!({"page": 1, "issues": [unfixed_with_existing_fix]}),
        ]),
    );
    assert!(!discover(&unfixed_with_existing_fix).status.success());
}

#[test]
fn excludes_each_failing_explicit_predicate_with_unique_reason_codes() {
    let mut security = issue(33, "2026-07-27T22:00:00Z", "security_sensitive");
    security["security_sensitive"] = json!(true);
    let path = temp_file(
        "security-sensitive",
        &envelope(vec![json!({"page": 1, "issues": [security]})]),
    );
    let value = successful_discovery(&path);
    let reasons = value["exclusion_ledger"][0]["reason_codes"]
        .as_array()
        .unwrap();
    assert_eq!(reasons, &vec![json!("security_sensitive")]);

    let mut unavailable = issue(34, "2026-07-27T22:00:00Z", "bug");
    unavailable["target_in_repository"] = json!(false);
    unavailable["expected_behavior_source"] = json!("unavailable");
    let path = temp_file(
        "predicate-failures",
        &envelope(vec![json!({"page": 1, "issues": [unavailable]})]),
    );
    let value = successful_discovery(&path);
    assert_eq!(value["selected_issue_number"], Value::Null);
    assert_eq!(
        value["exclusion_ledger"][0]["reason_codes"],
        json!(["target_outside_repository", "expected_behavior_unavailable"])
    );
}

#[test]
fn architecture_guard_keeps_main_at_the_recorded_ratchet() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let main = fs::read_to_string(root.join("crates/ao2-cli/src/main.rs")).unwrap();
    assert!(main.lines().count() <= 475);
    let python = if Command::new("python3")
        .arg("--version")
        .status()
        .is_ok_and(|status| status.success())
    {
        "python3"
    } else {
        "python"
    };
    assert!(Command::new(python)
        .arg("scripts/check-rust-architecture.py")
        .current_dir(root)
        .status()
        .unwrap()
        .success());
}

#[test]
fn discovery_structurally_binds_windows_reopened_path_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let discovery =
        fs::read_to_string(root.join("crates/ao2-cli/src/github_issue_discovery.rs")).unwrap();
    assert!(discovery.contains("crate::windows_input::disk_file_identity"));
    assert!(discovery.contains("validate_windows_path_identity"));
    assert!(discovery.contains("open_bounded_input(path)?"));
}

#[test]
fn is_stable_across_object_key_order_with_ordered_pages() {
    let first = temp_file(
        "stable-first",
        &envelope(vec![
            json!({"page": 2, "issues": [issue(4, "2026-07-27T20:00:00Z", "bug")]}),
            json!({"page": 1, "issues": [issue(2, "2026-07-27T21:00:00Z", "bug")]}),
        ]),
    );
    let second = temp_file(
        "stable-second",
        &json!({
            "pages": [
                {"issues": [issue(2, "2026-07-27T21:00:00Z", "bug")], "page": 1},
                {"issues": [issue(4, "2026-07-27T20:00:00Z", "bug")], "page": 2},
            ],
            "head_sha": HEAD,
            "default_branch": "main",
            "repository": "uesugitorachiyo/ao2",
        }),
    );
    assert_eq!(successful_discovery(&first), successful_discovery(&second));
}

#[test]
fn digest_changes_for_sanitized_content_and_duplicate_is_excluded() {
    let mut duplicate = issue(12, "2026-07-27T22:00:00Z", "bug");
    duplicate["body"] = json!("Distinct sanitized content");
    let path = temp_file(
        "duplicate",
        &envelope(vec![json!({"page": 1, "issues": [
            issue(12, "2026-07-27T23:00:00Z", "bug"), duplicate,
        ]})]),
    );
    let value = successful_discovery(&path);
    assert_eq!(value["issues"].as_array().unwrap().len(), 1);
    assert_eq!(value["selected_issue_number"], Value::Null);
    assert_eq!(
        value["exclusion_ledger"][0]["reason_codes"],
        json!(["duplicate_issue_number"])
    );

    let original = temp_file(
        "digest-original",
        &envelope(vec![
            json!({"page": 1, "issues": [issue(13, "2026-07-27T23:00:00Z", "bug")]} ),
        ]),
    );
    let mut altered_issue = issue(13, "2026-07-27T23:00:00Z", "bug");
    altered_issue["body"] = json!("Altered bounded sanitized fixture content.");
    let altered = temp_file(
        "digest-altered",
        &envelope(vec![json!({"page": 1, "issues": [altered_issue]} )]),
    );
    let original_result = successful_discovery(&original);
    let altered_result = successful_discovery(&altered);
    assert_ne!(
        original_result["issues"][0]["content_digest"],
        altered_result["issues"][0]["content_digest"]
    );
    assert_ne!(
        original_result["response_digests"],
        altered_result["response_digests"]
    );
}

#[test]
fn excludes_already_fixed_and_returns_zero_selection_with_full_ledger() {
    let mut fixed = issue(20, "2026-07-27T22:00:00Z", "already_fixed");
    fixed["fix_present_at_head"] = json!(true);
    fixed["no_existing_fix"] = json!(false);
    let path = temp_file(
        "zero",
        &envelope(vec![json!({"page": 1, "issues": [
            fixed,
            issue(21, "2026-07-27T21:00:00Z", "support_request"),
        ]})]),
    );
    let value = successful_discovery(&path);
    assert_eq!(value["selected_issue_number"], Value::Null);
    assert_eq!(value["exclusion_ledger"].as_array().unwrap().len(), 2);
    assert_eq!(
        value["exclusion_ledger"][0]["reason_codes"],
        json!([
            "already_fixed_current_head",
            "existing_fix_present",
            "current_head_fixed"
        ])
    );
}

#[test]
fn rejects_bad_identity_url_head_limits_and_malformed_or_oversized_input() {
    let valid = temp_file(
        "validation",
        &envelope(vec![
            json!({"page": 1, "issues": [issue(1, "2026-07-27T22:00:00Z", "bug")]}),
        ]),
    );
    assert!(!discover_with(
        &valid,
        "https://github.com/uesugitorachiyo/ao2/pulls",
        "uesugitorachiyo/ao2",
        HEAD,
        50,
        10
    )
    .status
    .success());
    for unsafe_url in [
        "https://github.com.evil/uesugitorachiyo/ao2/issues",
        "https://github.com@evil.example/uesugitorachiyo/ao2/issues",
        "https://github.com/uesugitorachiyo%2Fao2/issues",
    ] {
        assert!(
            !discover_with(&valid, unsafe_url, "uesugitorachiyo/ao2", HEAD, 50, 10)
                .status
                .success()
        );
    }
    assert!(!discover_with(
        &valid,
        "https://github.com/uesugitorachiyo/ao2/issues",
        "uesugitorachiyo/not-ao2",
        HEAD,
        50,
        10
    )
    .status
    .success());
    assert!(!discover_with(
        &valid,
        "https://github.com/uesugitorachiyo/ao2/issues",
        "uesugitorachiyo/ao2",
        "2222222222222222222222222222222222222222",
        50,
        10
    )
    .status
    .success());
    assert!(!discover_with(
        &valid,
        "https://github.com/uesugitorachiyo/ao2/issues",
        "uesugitorachiyo/ao2",
        HEAD,
        51,
        10
    )
    .status
    .success());
    assert!(!discover_with(
        &valid,
        "https://github.com/uesugitorachiyo/ao2/issues",
        "uesugitorachiyo/ao2",
        HEAD,
        50,
        11
    )
    .status
    .success());
    let malformed = temp_file("malformed", &json!({"pages": []}));
    assert!(!discover(&malformed).status.success());
    let too_many_pages = temp_file(
        "too-many-pages",
        &envelope(
            (1..=11)
                .map(|page| json!({"page": page, "issues": []}))
                .collect(),
        ),
    );
    assert!(!discover(&too_many_pages).status.success());
    let oversized = std::env::temp_dir().join("ao2-discovery-oversized.json");
    fs::write(&oversized, vec![b'x'; 1_048_577]).unwrap();
    assert!(!discover(&oversized).status.success());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_and_fifo_page_envelopes_without_blocking() {
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    let directory = tempdir().unwrap();
    let target = directory.path().join("target.json");
    fs::write(
        &target,
        serde_json::to_vec(&envelope(vec![json!({"page": 1, "issues": []})])).unwrap(),
    )
    .unwrap();
    let link = directory.path().join("page-envelope-link.json");
    symlink(&target, &link).unwrap();
    assert!(!discover(&link).status.success());

    let fifo = directory.path().join("page-envelope.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    assert!(!discover(&fifo).status.success());
}

#[test]
fn plain_output_is_concise_and_intake_output_remains_byte_compatible() {
    let path = temp_file(
        "human",
        &envelope(vec![
            json!({"page": 1, "issues": [issue(5, "2026-07-27T22:00:00Z", "bug")]}),
        ]),
    );
    let args = vec![
        "issue",
        "discover",
        "--page-envelope",
        path.to_str().unwrap(),
        "--url",
        "https://github.com/uesugitorachiyo/ao2/issues",
        "--repository",
        "uesugitorachiyo/ao2",
        "--default-branch",
        "main",
        "--head-sha",
        HEAD,
        "--run-id",
        RUN_ID,
        "--completed-at",
        COMPLETED_AT,
    ];
    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(&args)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "selected_issue_number=5\ncandidate_count=1\nexcluded_count=0\nmutation_performed=false\n"
    );
    let intake = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "issue",
            "intake",
            "--url",
            "https://github.com/uesugitorachiyo/ao2/issues/123?x=1",
        ])
        .output()
        .unwrap();
    assert!(intake.status.success());
    assert_eq!(
        String::from_utf8(intake.stdout).unwrap(),
        "state=intake_validated\ncanonical_url=https://github.com/uesugitorachiyo/ao2/issues/123\n"
    );
    let acquire = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "issue",
            "acquire",
            "--url",
            "https://github.com/uesugitorachiyo/ao2/issues/123",
            "--upstream-url",
            "https://github.com/uesugitorachiyo/ao2.git",
            "--default-branch",
            "main",
            "--target-commit",
            "80ec5321f42d4bab17d5e64fdae6aa099ba59d4a",
        ])
        .output()
        .unwrap();
    assert!(acquire.status.success());
    assert_eq!(
        String::from_utf8(acquire.stdout).unwrap(),
        "state=acquisition_planned\ntarget_commit=80ec5321f42d4bab17d5e64fdae6aa099ba59d4a\nupstream_matches_issue_repository=true\nmutation_policy=read_only_acquisition_plan_no_clone_or_checkout_performed_by_this_readback\n"
    );
}
