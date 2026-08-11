use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const REPRODUCTION: &[u8] = br#"{"repository":"example/project","issue_number":17,"source_sha":"0123456789abcdef0123456789abcdef01234567","result":"reproduced_failure"}"#;
const PATCH: &[u8] = b"diff --git a/file b/file\n";
const TESTS: &[u8] = br#"{"repository":"example/project","issue_number":17,"source_sha":"0123456789abcdef0123456789abcdef01234567","defining":"passed","neighboring":"passed","full":"passed"}"#;
const POLICY: &[u8] =
    br#"{"repository":"example/project","license":"MIT","contribution_policy":"accepted"}"#;

fn artifact(path: &str, bytes: &[u8]) -> Value {
    json!({
        "path": path,
        "size_bytes": bytes.len(),
        "sha256": format!("sha256:{:x}", Sha256::digest(bytes)),
    })
}

fn valid_packet(root: &Path) -> Value {
    fs::write(root.join("reproduction.json"), REPRODUCTION).unwrap();
    fs::write(root.join("repair.patch"), PATCH).unwrap();
    fs::write(root.join("tests.json"), TESTS).unwrap();
    fs::write(root.join("policy.json"), POLICY).unwrap();
    json!({
        "schema_version": "ao2.github-issue-contribution-packet.v1",
        "packet_id": "contribution-example-17",
        "repository": "example/project",
        "issue_number": 17,
        "source_sha": "0123456789abcdef0123456789abcdef01234567",
        "issue_snapshot_sha256": format!("sha256:{}", "1".repeat(64)),
        "reproduction_evidence": artifact("reproduction.json", REPRODUCTION),
        "patch": artifact("repair.patch", PATCH),
        "tests": artifact("tests.json", TESTS),
        "policy": artifact("policy.json", POLICY),
        "authorship": {
            "identity": "human:local-operator",
            "attestation": "authored_from_sealed_local_repair"
        },
        "limitations": ["maintainer review remains required"],
        "governance_state": "review_ready",
        "source_current": true,
        "issue_current": true,
        "maintainer_feedback": null,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "safety": {
            "network_accessed": false,
            "credentials_present": false,
            "provider_called": false,
            "upstream_mutated": false,
            "operator_fork_mutated": false,
            "publication_attempted": false,
            "mutation_authorized": false
        }
    })
}

fn run(root: &Path, packet: &Value) -> Output {
    let packet_path = root.join("packet.json");
    fs::write(&packet_path, serde_json::to_vec(packet).unwrap()).unwrap();
    run_path(root, &packet_path)
}

fn run_path(root: &Path, packet_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "issue",
            "contribution-packet",
            "verify",
            "--root",
            root.to_str().unwrap(),
            "--packet",
            packet_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap()
}

#[test]
fn valid_packet_is_read_only_and_review_ready() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), &valid_packet(dir.path()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(readback["result"], "packet_valid");
    assert_eq!(readback["contribution_ready"], true);
    assert_eq!(readback["mutation_authorized"], false);
    assert_eq!(readback["executes_work"], false);
    assert_eq!(readback["publishes"], false);
}

#[test]
fn non_ready_governance_states_validate_without_granting_authority() {
    for state in [
        "denied",
        "pending",
        "revision_requested",
        "rejected",
        "cancelled",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut packet = valid_packet(dir.path());
        packet["governance_state"] = json!(state);
        let output = run(dir.path(), &packet);
        assert!(
            output.status.success(),
            "state={state}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(readback["contribution_ready"], false, "state={state}");
        assert_eq!(readback["mutation_authorized"], false, "state={state}");
    }
}

#[test]
fn stale_changed_or_altered_packets_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut stale = valid_packet(dir.path());
    stale["created_at"] =
        json!((Utc::now() - Duration::days(8)).to_rfc3339_opts(SecondsFormat::Secs, true));
    assert!(!run(dir.path(), &stale).status.success());

    let mut changed = valid_packet(dir.path());
    changed["issue_current"] = json!(false);
    assert!(!run(dir.path(), &changed).status.success());

    let altered = valid_packet(dir.path());
    fs::write(dir.path().join("repair.patch"), b"altered").unwrap();
    assert!(!run(dir.path(), &altered).status.success());
}

#[test]
fn maintainer_feedback_changes_technical_state_but_never_authority() {
    let dir = tempfile::tempdir().unwrap();
    let mut packet = valid_packet(dir.path());
    let feedback = serde_json::to_vec(&json!({
        "repository": "example/project",
        "issue_number": 17,
        "source_sha": "0123456789abcdef0123456789abcdef01234567",
        "received_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "technical_state_changed": true,
        "mutation_authority_granted": false
    }))
    .unwrap();
    fs::write(dir.path().join("feedback.json"), &feedback).unwrap();
    packet["maintainer_feedback"] = artifact("feedback.json", &feedback);
    let output = run(dir.path(), &packet);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(readback["technical_state_changed"], true);
    assert_eq!(readback["mutation_authorized"], false);
    assert_eq!(readback["contribution_ready"], false);

    fs::write(dir.path().join("feedback.json"), b"altered").unwrap();
    assert!(!run(dir.path(), &packet).status.success());

    let unsafe_feedback = serde_json::to_vec(&json!({
        "repository": "example/project",
        "issue_number": 17,
        "source_sha": "0123456789abcdef0123456789abcdef01234567",
        "received_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "technical_state_changed": true,
        "mutation_authority_granted": true
    }))
    .unwrap();
    fs::write(dir.path().join("feedback.json"), &unsafe_feedback).unwrap();
    packet["maintainer_feedback"] = artifact("feedback.json", &unsafe_feedback);
    assert!(!run(dir.path(), &packet).status.success());
}

#[test]
fn malformed_missing_mismatched_oversized_and_unsafe_inputs_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut wrong_source = valid_packet(dir.path());
    wrong_source["source_sha"] = json!("1111111111111111111111111111111111111111");
    assert!(!run(dir.path(), &wrong_source).status.success());

    let mut unsafe_packet = valid_packet(dir.path());
    unsafe_packet["safety"]["mutation_authorized"] = json!(true);
    assert!(!run(dir.path(), &unsafe_packet).status.success());

    let mut oversized = valid_packet(dir.path());
    oversized["patch"]["size_bytes"] = json!(4_194_305_u64);
    assert!(!run(dir.path(), &oversized).status.success());

    let missing = valid_packet(dir.path());
    fs::remove_file(dir.path().join("reproduction.json")).unwrap();
    assert!(!run(dir.path(), &missing).status.success());

    let valid = valid_packet(dir.path());
    let bytes = serde_json::to_string(&valid).unwrap();
    let duplicate = bytes.replacen(
        "\"packet_id\":",
        "\"packet_id\":\"duplicate\",\"packet_id\":",
        1,
    );
    let packet_path = dir.path().join("duplicate.json");
    fs::write(&packet_path, duplicate).unwrap();
    assert!(!run_path(dir.path(), &packet_path).status.success());

    fs::write(&packet_path, b"not JSON").unwrap();
    assert!(!run_path(dir.path(), &packet_path).status.success());
    fs::write(&packet_path, vec![b' '; 65_537]).unwrap();
    assert!(!run_path(dir.path(), &packet_path).status.success());
}

#[cfg(unix)]
#[test]
fn symlinked_artifact_is_rejected() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let packet = valid_packet(dir.path());
    fs::rename(
        dir.path().join("repair.patch"),
        dir.path().join("real.patch"),
    )
    .unwrap();
    symlink("real.patch", dir.path().join("repair.patch")).unwrap();
    assert!(!run(dir.path(), &packet).status.success());
}
