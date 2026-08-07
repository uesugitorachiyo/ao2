use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

const ARTIFACTS: [(&str, &str); 7] = [
    ("source.json", "source"),
    ("reproduction.json", "reproduction"),
    ("regression.json", "regression"),
    ("full-suite.json", "full_suite"),
    ("candidate-seal.json", "candidate_seal"),
    ("review.json", "review"),
    ("draft-pr.json", "draft_pr"),
];

fn write_artifacts(root: &Path, bundle: &mut Value) {
    let mut artifact_sha256 = serde_json::Map::new();
    for (name, field) in ARTIFACTS {
        let artifact = json!({
            "repository": bundle["repository"],
            "upstream_repository_id": bundle["upstream_repository_id"],
            "issue_number": bundle["issue_number"],
            "baseline_source_sha": bundle["baseline_source_sha"],
            "candidate_sha": bundle["candidate_sha"],
            "evidence": bundle[field]
        });
        let bytes = serde_json::to_vec(&artifact).unwrap();
        fs::write(root.join(name), &bytes).unwrap();
        artifact_sha256.insert(
            name.to_string(),
            json!(format!("sha256:{:x}", Sha256::digest(&bytes))),
        );
    }
    bundle["artifact_sha256"] = Value::Object(artifact_sha256);
}

fn valid_bundle(root: &Path) -> Value {
    let completed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut bundle = json!({
        "schema_version": "ao2.github-issue-repair-qualification-bundle.v1",
        "repository": "example/project",
        "upstream_repository_id": "upstream-repository-node-101",
        "operator_owner": "operator",
        "issue_number": 17,
        "baseline_source_sha": "1111111111111111111111111111111111111111",
        "candidate_sha": "2222222222222222222222222222222222222222",
        "source": {
            "fetched_at": completed_at,
            "source_archive_sha256": digest('a'),
            "issue_snapshot_sha256": digest('b'),
            "dependency_cache_manifest_sha256": digest('c'),
            "extracted_tree_sha256": digest('d'),
            "toolchain": { "name": "rust", "version": "1.90.0" },
            "platforms": ["linux/amd64", "darwin/arm64"]
        },
        "reproduction": {
            "completed_at": completed_at,
            "command_sha256": digest('e'),
            "output_sha256": digest('f'),
            "failure_signature_sha256": digest('1'),
            "exit_code": 1,
            "network": "none",
            "credentials_present": false,
            "git_history_present": false,
            "oracle_present": false,
            "external_effects": 0
        },
        "regression": {
            "completed_at": completed_at,
            "command_sha256": digest('2'),
            "identifier": "invalid-date-regression",
            "baseline_exit_code": 1,
            "baseline_output_sha256": digest('3'),
            "candidate_exit_code": 0,
            "candidate_output_sha256": digest('4')
        },
        "full_suite": {
            "completed_at": completed_at,
            "baseline_evidence_sha256": digest('5'),
            "candidate_evidence_sha256": digest('6'),
            "classification_evidence_sha256": digest('7'),
            "classification": "candidate_clean",
            "candidate_regression": false
        },
        "candidate_seal": {
            "sealed_at": completed_at,
            "patch_sha256": digest('8'),
            "tree_sha256": digest('9')
        },
        "review": {
            "completed_at": completed_at,
            "evidence_sha256": digest('a'),
            "status": "no_findings_after_correction",
            "unresolved_p1": 0,
            "unresolved_p2": 0
        },
        "draft_pr": {
            "captured_at": completed_at,
            "evidence_sha256": digest('b'),
            "repository": "operator/project",
            "repository_id": "fork-repository-node-202",
            "owner": "operator",
            "is_fork": true,
            "parent_repository": "example/project",
            "parent_repository_id": "upstream-repository-node-101",
            "number": 3,
            "state": "OPEN",
            "is_draft": true,
            "merged": false,
            "head_sha": "2222222222222222222222222222222222222222"
        },
        "artifact_sha256": {},
        "safety": {
            "network": "none",
            "credentials_present": false,
            "git_history_present": false,
            "oracle_present": false,
            "provider_calls": 0,
            "external_effects": 0,
            "upstream_branch_mutations": 0,
            "upstream_pull_request_mutations": 0,
            "upstream_issue_comment_mutations": 0,
            "release_mutations": 0,
            "deployment_mutations": 0,
            "publication_mutations": 0
        }
    });
    write_artifacts(root, &mut bundle);
    bundle
}

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn verify(bundle: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "issue",
            "repair-qualification",
            "verify",
            "--bundle",
            bundle.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap()
}

type Mutation = fn(&mut Value);

fn assert_rejected(mutate: Mutation, message: &str) {
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("bundle.json");
    let mut value = valid_bundle(temp.path());
    mutate(&mut value);
    write(&bundle, &value);
    let output = verify(&bundle);
    assert!(!output.status.success(), "case unexpectedly passed");
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(readback["result"], "repair_rejected");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn qualifies_a_strict_offline_repair_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("bundle.json");
    write(&bundle, &valid_bundle(temp.path()));

    let output = verify(&bundle);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        readback["schema_version"],
        "ao2.github-issue-repair-qualification.v1"
    );
    assert_eq!(readback["result"], "repair_qualified");
    assert_eq!(readback["repository"], "example/project");
    assert_eq!(readback["issue_number"], 17);
    assert_eq!(
        readback["candidate_sha"],
        "2222222222222222222222222222222222222222"
    );
    assert_eq!(readback["candidate_regression"], false);
    assert_eq!(readback["classification"], "candidate_clean");
    assert_eq!(readback["regression_identifier"], "invalid-date-regression");
    assert_eq!(readback["draft_pr_repository"], "operator/project");
    assert_eq!(readback["draft_pr_number"], 3);
    assert_eq!(
        readback["draft_pr_head_sha"],
        "2222222222222222222222222222222222222222"
    );
    assert_eq!(readback["review_status"], "no_findings_after_correction");
    assert_eq!(readback["reproduction_exit_code"], 1);
    assert_eq!(readback["regression_baseline_exit_code"], 1);
    assert_eq!(readback["regression_candidate_exit_code"], 0);
    assert_eq!(readback["approval_granted"], false);
    assert_eq!(readback["mutation_performed"], false);
    assert_eq!(readback["release_performed"], false);
    assert_eq!(readback["deployment_performed"], false);
    assert_eq!(readback["publication_performed"], false);
    assert!(readback["bundle_sha256"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(readback["qualification_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
}

#[test]
fn qualifies_a_relative_bundle_path() {
    let temp = tempfile::tempdir().unwrap();
    let mut value = valid_bundle(temp.path());
    write_artifacts(temp.path(), &mut value);
    write(&temp.path().join("bundle.json"), &value);

    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .current_dir(temp.path())
        .args([
            "issue",
            "repair-qualification",
            "verify",
            "--bundle",
            "bundle.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rejects_an_altered_evidence_digest() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("bundle.json");
    let mut value = valid_bundle(temp.path());
    value["artifact_sha256"]["review.json"] = json!(digest('f'));
    write(&bundle, &value);

    let output = verify(&bundle);
    assert!(!output.status.success());
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(readback["result"], "repair_rejected");
    assert!(String::from_utf8_lossy(&output.stderr).contains("artifact digest mismatch"));
}

#[test]
fn rejects_artifact_semantics_that_disagree_with_the_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("bundle.json");
    let mut value = valid_bundle(temp.path());
    let review_path = temp.path().join("review.json");
    let mut artifact: Value = serde_json::from_slice(&fs::read(&review_path).unwrap()).unwrap();
    artifact["evidence"]["status"] = json!("no_findings");
    let bytes = serde_json::to_vec(&artifact).unwrap();
    fs::write(&review_path, &bytes).unwrap();
    value["artifact_sha256"]["review.json"] = json!(format!("sha256:{:x}", Sha256::digest(&bytes)));
    write(&bundle, &value);

    let output = verify(&bundle);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("artifact semantics mismatch"));
}

#[test]
fn rejects_identity_freshness_and_digest_drift() {
    let cases: Vec<(Mutation, &str)> = vec![
        (
            |value| value["repository"] = json!("not canonical"),
            "canonical owner/name",
        ),
        (|value| value["issue_number"] = json!(0), "issue_number"),
        (
            |value| value["candidate_sha"] = value["baseline_source_sha"].clone(),
            "distinct exact source",
        ),
        (
            |value| value["source"]["fetched_at"] = json!("2020-01-01T00:00:00Z"),
            "stale",
        ),
        (
            |value| value["review"]["completed_at"] = json!("2020-01-01T00:00:00Z"),
            "stale",
        ),
        (
            |value| value["source"]["source_archive_sha256"] = json!("sha256:no"),
            "source evidence digests",
        ),
        (
            |value| value["reproduction"]["output_sha256"] = json!("sha256:no"),
            "reproduction digests",
        ),
        (
            |value| value["candidate_seal"]["patch_sha256"] = json!("sha256:no"),
            "candidate seal digests",
        ),
        (
            |value| value["candidate_seal"]["tree_sha256"] = json!("sha256:no"),
            "candidate seal digests",
        ),
        (
            |value| value["review"]["evidence_sha256"] = json!("sha256:no"),
            "review evidence digest",
        ),
        (
            |value| value["full_suite"]["classification_evidence_sha256"] = json!("sha256:no"),
            "full-suite digests",
        ),
    ];
    for (mutate, message) in cases {
        assert_rejected(mutate, message);
    }
}

#[test]
fn rejects_missing_red_green_review_draft_and_safety_evidence() {
    let cases: Vec<(Mutation, &str)> = vec![
        (
            |value| {
                value.as_object_mut().unwrap().remove("reproduction");
            },
            "missing field `reproduction`",
        ),
        (
            |value| value["reproduction"]["exit_code"] = json!(0),
            "nonzero observed exit",
        ),
        (
            |value| value["regression"]["baseline_exit_code"] = json!(0),
            "baseline RED",
        ),
        (
            |value| value["regression"]["candidate_exit_code"] = json!(1),
            "candidate GREEN",
        ),
        (
            |value| value["full_suite"]["candidate_regression"] = json!(true),
            "candidate regression",
        ),
        (
            |value| value["review"]["unresolved_p2"] = json!(1),
            "unresolved P1 or P2",
        ),
        (
            |value| value["draft_pr"]["is_draft"] = json!(false),
            "open, draft, unmerged",
        ),
        (
            |value| value["draft_pr"]["merged"] = json!(true),
            "open, draft, unmerged",
        ),
        (
            |value| {
                value["draft_pr"]["head_sha"] = json!("3333333333333333333333333333333333333333")
            },
            "exact-head bound",
        ),
        (
            |value| value["draft_pr"]["is_fork"] = json!(false),
            "fork provenance",
        ),
        (
            |value| value["draft_pr"]["parent_repository"] = json!("other/project"),
            "fork provenance",
        ),
        (
            |value| value["draft_pr"]["owner"] = json!("someone"),
            "authorized operator owner",
        ),
        (
            |value| value["safety"]["upstream_pull_request_mutations"] = json!(1),
            "mutation-free",
        ),
        (
            |value| value["safety"]["provider_calls"] = json!(1),
            "mutation-free",
        ),
        (
            |value| value["reproduction"]["network"] = json!("enabled"),
            "offline and effect-free",
        ),
    ];
    for (mutate, message) in cases {
        assert_rejected(mutate, message);
    }
}

#[test]
fn rejects_impossible_evidence_lifecycle_order() {
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("bundle.json");
    let mut value = valid_bundle(temp.path());
    value["source"]["fetched_at"] = json!("2026-08-07T10:00:00Z");
    value["reproduction"]["completed_at"] = json!("2026-08-07T09:59:59Z");
    write_artifacts(temp.path(), &mut value);
    write(&bundle, &value);

    let output = verify(&bundle);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("lifecycle order"));
}

#[test]
fn rejects_malformed_oversized_missing_and_linked_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let malformed = temp.path().join("malformed.json");
    fs::write(&malformed, b"{").unwrap();
    let output = verify(&malformed);
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"],
        "repair_rejected"
    );

    let duplicate = temp.path().join("duplicate.json");
    fs::write(
        &duplicate,
        br#"{"schema_version":"ao2.github-issue-repair-qualification-bundle.v1","schema_version":"ao2.github-issue-repair-qualification-bundle.v1"}"#,
    )
    .unwrap();
    assert!(!verify(&duplicate).status.success());

    let oversized = temp.path().join("oversized.json");
    fs::write(&oversized, vec![b' '; 65_537]).unwrap();
    assert!(!verify(&oversized).status.success());

    let missing = temp.path().join("missing.json");
    let mut value = valid_bundle(temp.path());
    value["artifact_sha256"]
        .as_object_mut()
        .unwrap()
        .remove("review.json");
    write(&missing, &value);
    let output = verify(&missing);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exactly seven evidence roles"));

    let unsafe_name = temp.path().join("unsafe-name.json");
    let mut value = valid_bundle(temp.path());
    let digest = value["artifact_sha256"]["review.json"].clone();
    value["artifact_sha256"]
        .as_object_mut()
        .unwrap()
        .remove("review.json");
    value["artifact_sha256"]["../review.json"] = digest;
    write(&unsafe_name, &value);
    let output = verify(&unsafe_name);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("required evidence roles"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let target = temp.path().join("target.json");
        let link = temp.path().join("link.json");
        write(&target, &valid_bundle(temp.path()));
        symlink(&target, &link).unwrap();
        assert!(!verify(&link).status.success());

        let hardlink_bundle = temp.path().join("hardlink-bundle.json");
        let value = valid_bundle(temp.path());
        let source = temp.path().join("source.json");
        let alias = temp.path().join("source-alias.json");
        fs::hard_link(&source, &alias).unwrap();
        write(&hardlink_bundle, &value);
        let output = verify(&hardlink_bundle);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("hardlinked"));
    }
}
