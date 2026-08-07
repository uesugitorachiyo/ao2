use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

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

#[test]
fn separates_retained_baseline_failures_from_candidate_regressions() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.json");
    let candidate = temp.path().join("candidate.json");
    write(&baseline, &verification("baseline"));
    write(&candidate, &verification("candidate"));

    let output = classify(&baseline, &candidate);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
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
