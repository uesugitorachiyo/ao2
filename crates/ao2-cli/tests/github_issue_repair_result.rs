use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

type Mutation = fn(&mut Value, &mut Value);

fn verification(role: &str) -> Value {
    let mut value = json!({
        "schema_version": "ao2.github-issue-repair-verification.v1",
        "role": role,
        "repository": "example/project",
        "issue_number": 17,
        "baseline_source_sha": "1111111111111111111111111111111111111111",
        "source_sha": "1111111111111111111111111111111111111111",
        "command_sha256": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
        "toolchain": { "name": "go", "version": "1.26.4" },
        "completed_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "exit_code": 1,
        "output_sha256": "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        "failures": [{
            "identifier": "TestExistingFailure",
            "signature_sha256": "sha256:4444444444444444444444444444444444444444444444444444444444444444"
        }],
        "safety": {
            "network": "none",
            "credentials_present": false,
            "git_history_present": false,
            "oracle_present": false,
            "external_effects": 0
        }
    });
    if role == "candidate" {
        value["source_sha"] = json!("5555555555555555555555555555555555555555");
        value["candidate_sha"] = json!("5555555555555555555555555555555555555555");
    }
    value
}

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn classify(baseline: &Path, candidate: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "issue",
            "repair-result",
            "classify",
            "--baseline",
            baseline.to_str().unwrap(),
            "--candidate",
            candidate.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap()
}

fn pair() -> (Value, Value) {
    (verification("baseline"), verification("candidate"))
}

fn run_pair(baseline_value: &Value, candidate_value: &Value) -> (tempfile::TempDir, Output) {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    let candidate = temp.path().join("candidate.json");
    write(&baseline, baseline_value);
    write(&candidate, candidate_value);
    let output = classify(&baseline, &candidate);
    (temp, output)
}

fn readback(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_rejected(output: Output, message: &str) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn separates_retained_baseline_failures_from_candidate_regressions() {
    let (baseline, candidate) = pair();
    let (_temp, output) = run_pair(&baseline, &candidate);
    let readback = readback(&output);
    assert_eq!(
        readback["schema_version"],
        "ao2.github-issue-repair-result-classification.v1"
    );
    assert_eq!(readback["candidate_regression"], false);
    assert_eq!(readback["baseline_failures_retained"], true);
    assert_eq!(readback["shared_failures"].as_array().unwrap().len(), 1);
    assert_eq!(
        readback["classification"],
        "candidate_has_only_exact_baseline_failures"
    );
}

#[test]
fn classifies_clean_resolved_candidate_only_and_changed_failures() {
    let (mut baseline, mut candidate) = pair();
    baseline["exit_code"] = json!(0);
    baseline["failures"] = json!([]);
    candidate["exit_code"] = json!(0);
    candidate["failures"] = json!([]);
    let (_temp, output) = run_pair(&baseline, &candidate);
    assert_eq!(readback(&output)["classification"], "candidate_clean");

    let (baseline, mut candidate) = pair();
    candidate["exit_code"] = json!(0);
    candidate["failures"] = json!([]);
    let (_temp, output) = run_pair(&baseline, &candidate);
    let result = readback(&output);
    assert_eq!(
        result["classification"],
        "candidate_resolved_baseline_failures"
    );
    assert_eq!(result["resolved_failures"].as_array().unwrap().len(), 1);

    let (mut baseline, candidate) = pair();
    baseline["exit_code"] = json!(0);
    baseline["failures"] = json!([]);
    let (_temp, output) = run_pair(&baseline, &candidate);
    let result = readback(&output);
    assert_eq!(result["classification"], "candidate_regression_detected");
    assert_eq!(
        result["candidate_only_failures"].as_array().unwrap().len(),
        1
    );

    let (baseline, mut candidate) = pair();
    candidate["failures"][0]["signature_sha256"] =
        json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let (_temp, output) = run_pair(&baseline, &candidate);
    let result = readback(&output);
    assert_eq!(result["candidate_regression"], true);
    assert_eq!(result["changed_failures"].as_array().unwrap().len(), 1);
    assert!(result["shared_failures"].as_array().unwrap().is_empty());

    let (mut baseline, mut candidate) = pair();
    let earlier = json!({
        "identifier": "AFirstFailure",
        "signature_sha256": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    });
    baseline["failures"]
        .as_array_mut()
        .unwrap()
        .push(earlier.clone());
    candidate["failures"].as_array_mut().unwrap().push(earlier);
    let (_temp, output) = run_pair(&baseline, &candidate);
    let result = readback(&output);
    assert_eq!(result["shared_failures"][0]["identifier"], "AFirstFailure");
    assert_eq!(
        result["shared_failures"][1]["identifier"],
        "TestExistingFailure"
    );
}

#[test]
fn rejects_identity_role_freshness_digest_and_safety_drift() {
    let cases: Vec<(&str, Mutation, &str)> = vec![
        (
            "wrong role",
            |_, candidate| candidate["role"] = json!("baseline"),
            "verification role must be candidate",
        ),
        (
            "wrong source",
            |_, candidate| {
                candidate["baseline_source_sha"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            },
            "comparison identities do not match",
        ),
        (
            "wrong command",
            |_, candidate| {
                candidate["command_sha256"] =
                    json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            },
            "comparison identities do not match",
        ),
        (
            "stale",
            |baseline, _| baseline["completed_at"] = json!("2020-01-01T00:00:00Z"),
            "completed_at is stale",
        ),
        (
            "future",
            |baseline, _| {
                baseline["completed_at"] =
                    json!((Utc::now() + Duration::minutes(6))
                        .to_rfc3339_opts(SecondsFormat::Secs, true))
            },
            "completed_at is too far in the future",
        ),
        (
            "malformed digest",
            |_, candidate| candidate["output_sha256"] = json!("sha256:no"),
            "command and output digests",
        ),
        (
            "unsafe network",
            |_, candidate| candidate["safety"]["network"] = json!("enabled"),
            "safety boundary is not offline",
        ),
        (
            "credentials present",
            |_, candidate| candidate["safety"]["credentials_present"] = json!(true),
            "safety boundary is not offline",
        ),
        (
            "Git history present",
            |_, candidate| candidate["safety"]["git_history_present"] = json!(true),
            "safety boundary is not offline",
        ),
        (
            "oracle present",
            |_, candidate| candidate["safety"]["oracle_present"] = json!(true),
            "safety boundary is not offline",
        ),
        (
            "external effect",
            |_, candidate| candidate["safety"]["external_effects"] = json!(1),
            "safety boundary is not offline",
        ),
    ];

    for (name, mutate, message) in cases {
        let (mut baseline, mut candidate) = pair();
        mutate(&mut baseline, &mut candidate);
        let (_temp, output) = run_pair(&baseline, &candidate);
        assert_rejected(output, message);
        assert!(!name.is_empty());
    }
}

#[test]
fn rejects_duplicate_failures_and_duplicate_or_malformed_json() {
    let (baseline, mut candidate) = pair();
    candidate["failures"] = json!([
        candidate["failures"][0].clone(),
        candidate["failures"][0].clone()
    ]);
    let (_temp, output) = run_pair(&baseline, &candidate);
    assert_rejected(output, "failure identifiers must be unique");

    let temp = tempfile::tempdir().unwrap();
    let baseline_path = temp.path().join("baseline.json");
    let candidate_path = temp.path().join("candidate.json");
    fs::write(
        &baseline_path,
        br#"{"schema_version":"one","schema_version":"two"}"#,
    )
    .unwrap();
    write(&candidate_path, &verification("candidate"));
    assert_rejected(classify(&baseline_path, &candidate_path), "duplicate field");

    fs::write(&baseline_path, b"{").unwrap();
    assert_rejected(
        classify(&baseline_path, &candidate_path),
        "parse strict baseline JSON",
    );
}

#[test]
fn rejects_oversized_and_symlinked_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    let candidate = temp.path().join("candidate.json");
    fs::write(&baseline, vec![b' '; 65_537]).unwrap();
    write(&candidate, &verification("candidate"));
    assert_rejected(
        classify(&baseline, &candidate),
        "exceeds the 65536-byte limit",
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = temp.path().join("target.json");
        let link = temp.path().join("link.json");
        write(&target, &verification("baseline"));
        symlink(&target, &link).unwrap();
        assert_rejected(classify(&link, &candidate), "without following links");
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_file;
        let target = temp.path().join("target.json");
        let link = temp.path().join("link.json");
        write(&target, &verification("baseline"));
        symlink_file(&target, &link).expect("create file symlink");
        assert_rejected(classify(&link, &candidate), "input must be a regular file");
    }
}
