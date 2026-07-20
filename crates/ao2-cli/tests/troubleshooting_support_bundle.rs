use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const MAX_BYTES: usize = 65_536;

fn ao2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .output()
        .expect("run ao2")
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/troubleshooting-support-bundle/valid-input.json")
}

fn run_bundle(input: &Path) -> (tempfile::TempDir, PathBuf, Output) {
    let temp = tempfile::tempdir().expect("tempdir");
    let output_path = temp.path().join("support-bundle.json");
    let output = ao2(&[
        "support",
        "bundle",
        "--input",
        input.to_str().expect("utf8 input"),
        "--out",
        output_path.to_str().expect("utf8 output"),
        "--json",
    ]);
    (temp, output_path, output)
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_json(value: &Value) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("input.json");
    fs::write(&path, serde_json::to_vec_pretty(value).expect("serialize")).expect("write input");
    (temp, path)
}

fn valid_input() -> Value {
    serde_json::from_slice(&fs::read(fixture_path()).expect("read fixture")).expect("parse fixture")
}

fn rejected(input: &Path, needle: &str) {
    let (_temp, _output_path, output) = run_bundle(input);
    assert!(!output.status.success(), "unexpected success");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(needle),
        "stderr did not contain {needle:?}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn builds_deterministic_observer_only_bundle_with_stable_fingerprint() {
    let (first_temp, first_path, first) = run_bundle(&fixture_path());
    assert_success(&first);
    let first_stdout: Value = serde_json::from_slice(&first.stdout).expect("stdout json");
    let first_file = fs::read(&first_path).expect("first bundle");
    assert_eq!(
        first_stdout,
        serde_json::from_slice::<Value>(&first_file).expect("bundle json")
    );
    assert!(first_file.len() <= MAX_BYTES);
    assert_eq!(
        first_stdout["schema_version"],
        "ao2.troubleshooting-support-bundle.v0.1"
    );
    assert_eq!(first_stdout["ao2_version"], "0.5.2");
    assert_eq!(first_stdout["control_plane_version"], "0.1.16");
    assert_eq!(first_stdout["platform"]["os"], "windows");
    assert_eq!(first_stdout["platform"]["architecture"], "x86_64");
    assert_eq!(first_stdout["approval"]["status"], "waiting");
    assert_eq!(
        first_stdout["bundle_sha256"]
            .as_str()
            .expect("bundle digest")
            .len(),
        64
    );
    assert!(first_stdout["approval"].get("secret").is_none());
    assert_eq!(first_stdout["logs"].as_array().expect("logs").len(), 3);
    assert!(first_stdout["logs"]
        .as_array()
        .expect("logs")
        .iter()
        .all(|entry| entry["text"] == "[REDACTED_LOG]"));
    assert_eq!(first_stdout["redaction"]["input_log_entries"], 3);
    assert_eq!(first_stdout["redaction"]["fully_redacted_log_entries"], 3);
    assert_eq!(first_stdout["observer_only"], true);
    assert_eq!(first_stdout["safe_to_execute"], false);
    assert_eq!(first_stdout["executes_work"], false);
    assert_eq!(first_stdout["calls_providers"], false);
    assert_eq!(first_stdout["issue_write_performed"], false);
    assert_eq!(first_stdout["public_write_performed"], false);
    assert_eq!(first_stdout["release_or_deployment_performed"], false);
    assert_eq!(
        first_stdout["governed_issue_route"]["input_trust"],
        "sanitized_untrusted"
    );

    let (_second_temp, second_path, second) = run_bundle(&fixture_path());
    assert_success(&second);
    let second_file = fs::read(&second_path).expect("second bundle");
    assert_eq!(first_file, second_file);

    let mut changed_logs = valid_input();
    changed_logs["logs"] = json!(["different diagnostic ordering", "another harmless line"]);
    let (_input_temp, input_path) = write_json(&changed_logs);
    let (_third_temp, third_path, third) = run_bundle(&input_path);
    assert_success(&third);
    let third_value: Value =
        serde_json::from_slice(&fs::read(third_path).expect("third bundle")).expect("third json");
    assert_eq!(
        first_stdout["problem_fingerprint"],
        third_value["problem_fingerprint"]
    );

    let mut equivalent = valid_input();
    equivalent["ao2_version"] = json!("  0.5.2  ");
    equivalent["platform"]["os"] = json!("WINDOWS");
    equivalent["workflow"]["identity"] = json!("risky-pr-v1  ");
    equivalent["failure"]["category"] = json!("VERIFICATION_FAILED");
    equivalent["smallest_safe_next_action"] =
        json!("Re-run  ao2 report verify against the retained evidence pack.");
    let (_input_temp, input_path) = write_json(&equivalent);
    let (_normalized_temp, normalized_path, normalized) = run_bundle(&input_path);
    assert_success(&normalized);
    let normalized_value: Value =
        serde_json::from_slice(&fs::read(normalized_path).expect("normalized bundle"))
            .expect("normalized json");
    assert_eq!(
        first_stdout["problem_fingerprint"],
        normalized_value["problem_fingerprint"]
    );
    drop(first_temp);
}

#[test]
fn redacts_credentials_environment_values_and_filesystem_paths() {
    let mut input = valid_input();
    let mac_path = ["/", "Users", "/alice/private-repository"].concat();
    let linux_home_path = ["/", "home", "/alice/private/src/main.rs:41"].concat();
    let unix_system_path = ["/", "var", "/lib/ao2/private.log"].concat();
    let windows_path = ["C:", r"\Users\Alice\private\src\main.rs:41"].concat();
    let windows_build_path = ["D:", r"\build\ao2\private.log"].concat();
    let unc_path = [r"\\", r"server\share\private.log"].concat();
    let colon_private_path = ["/", "Users", "/alice/private/project"].concat();
    let colon_path = format!("path:{colon_private_path}");
    input["logs"] = json!([
        "Authorization: Bearer top-secret-token",
        "Authorization: Basic dXNlcjpzdXBlcnNlY3JldA==",
        "request Authorization: Basic another-private-value",
        "API_TOKEN=top-secret-env-value",
        "PRIVATE_CONTEXT=\"alpha-secret with spaces\" suffix",
        format!("HOME={mac_path}"),
        format!("failed at {linux_home_path}"),
        format!("failed at {unix_system_path}"),
        format!("failed at {windows_path}"),
        format!("failed at {windows_build_path}"),
        format!("failed at {unc_path}"),
        colon_path,
        "failed at ~/private/project/main.rs:41",
        "failed at ./private/project/main.rs:41",
        "failed path:src/private/project/main.rs:41",
        "url=https://example.invalid/check?access_token=private-value"
    ]);
    let (_input_temp, input_path) = write_json(&input);
    let (_output_temp, output_path, output) = run_bundle(&input_path);
    assert_success(&output);
    let rendered = fs::read_to_string(output_path).expect("bundle");
    for forbidden in [
        "top-secret-token",
        "dXNlcjpzdXBlcnNlY3JldA==",
        "another-private-value",
        "top-secret-env-value",
        "alpha-secret",
        &mac_path,
        &linux_home_path,
        &unix_system_path,
        &windows_path,
        &windows_build_path,
        &unc_path,
        &colon_private_path,
        "~/private/project",
        "./private/project",
        "src/private/project",
        "private-repository",
        "private-value",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "bundle leaked {forbidden:?}:\n{rendered}"
        );
    }
    assert!(rendered.contains("[REDACTED_LOG]"));
}

#[test]
fn redacts_ambiguous_environment_and_path_forms_and_rejects_source_fragments() {
    let mut input = valid_input();
    input["logs"] = json!([
        "private_context=\"alpha secret\"",
        r#"PRIVATE_CONTEXT="alpha\\\" tail-secret""#,
        "failed at file://server/private/share.log",
        "failed at `/home/alice/private.log`",
        "failed at </var/lib/ao2/private.log>",
        "failed path:src/private/project/main.rs:41",
        "customer_secret := computePrivateValue()",
        "env:PRIVATE_CONTEXT=alpha-secret",
        "%2FUsers%2FAlice%2Fprivate",
        "repository=private-customer-repair"
    ]);
    let (_input_temp, input_path) = write_json(&input);
    let (_output_temp, output_path, output) = run_bundle(&input_path);
    assert_success(&output);
    let rendered = fs::read_to_string(output_path).expect("bundle");
    for forbidden in [
        "alpha secret",
        "tail-secret",
        "file://server",
        "/home/alice",
        "/var/lib/ao2",
        "src/private/project",
        "computePrivateValue",
        "alpha-secret",
        "%2FUsers",
        "private-customer-repair",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "bundle leaked {forbidden:?}:\n{rendered}"
        );
    }

    for source in [
        "SELECT customer_secret FROM private_table;",
        "if customer_secret != expected { return deny; }",
    ] {
        let mut source_input = valid_input();
        source_input["logs"] = json!([source]);
        let (_temp, path) = write_json(&source_input);
        rejected(&path, "private source content");
    }
}

#[test]
fn rejects_unknown_malformed_oversized_and_unsafe_inputs() {
    let mut unknown = valid_input();
    unknown["credential"] = json!("must-not-be-accepted");
    let (_temp, path) = write_json(&unknown);
    rejected(&path, "unknown field");

    let malformed_temp = tempfile::tempdir().expect("tempdir");
    let malformed = malformed_temp.path().join("malformed.json");
    fs::write(&malformed, b"{").expect("write malformed");
    rejected(&malformed, "parse strict JSON");

    let duplicate = malformed_temp.path().join("duplicate.json");
    let fixture = fs::read_to_string(fixture_path()).expect("read fixture");
    fs::write(
        &duplicate,
        fixture.replacen(
            "\"schema_version\":",
            "\"schema_version\":\"duplicate\",\"schema_version\":",
            1,
        ),
    )
    .expect("write duplicate");
    rejected(&duplicate, "duplicate field");

    let oversized = malformed_temp.path().join("oversized.json");
    fs::write(&oversized, vec![b'x'; MAX_BYTES + 1]).expect("write oversized");
    rejected(&oversized, "65536-byte limit");

    for (field, value) in [
        ("executes_work", true),
        ("calls_providers", true),
        ("issue_write_performed", true),
        ("public_write_performed", true),
        ("release_or_deployment_performed", true),
    ] {
        let mut unsafe_input = valid_input();
        unsafe_input["safety"][field] = json!(value);
        let (_temp, path) = write_json(&unsafe_input);
        rejected(&path, "observer-only safety boundary");
    }
}

#[test]
fn rejects_invalid_digests_fields_and_excess_logs() {
    let mut invalid_digest = valid_input();
    invalid_digest["manifest_sha256"] = json!("not-a-digest");
    let (_temp, path) = write_json(&invalid_digest);
    rejected(&path, "manifest_sha256");

    let mut too_many_logs = valid_input();
    too_many_logs["logs"] = json!((0..17)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>());
    let (_temp, path) = write_json(&too_many_logs);
    rejected(&path, "at most 16");

    let mut oversized_field = valid_input();
    oversized_field["failure"]["category"] = json!("x".repeat(129));
    let (_temp, path) = write_json(&oversized_field);
    rejected(&path, "failure.category");

    let mut source_fragment = valid_input();
    source_fragment["logs"] = json!(["fn private_algorithm() { return customer_secret; }"]);
    let (_temp, path) = write_json(&source_fragment);
    rejected(&path, "private source content");

    let mut rust_source_fragment = valid_input();
    rust_source_fragment["logs"] = json!(["let customer_secret = compute_private_value();"]);
    let (_temp, path) = write_json(&rust_source_fragment);
    rejected(&path, "private source content");

    let mut contradictory_approval = valid_input();
    contradictory_approval["approval"]["status"] = json!("not_attempted");
    let (_temp, path) = write_json(&contradictory_approval);
    rejected(&path, "must be absent");

    let mut contradictory_evidence = valid_input();
    contradictory_evidence["evidence"]["status"] = json!("not_available");
    let (_temp, path) = write_json(&contradictory_evidence);
    rejected(&path, "must be absent");
}

#[test]
fn rejects_credentials_and_machine_paths_outside_sanitized_logs() {
    let mut private_action = valid_input();
    let private_path = ["/", "Users", "/alice/private/repository"].concat();
    private_action["smallest_safe_next_action"] =
        json!(format!("Inspect {private_path} and retry."));
    let (_temp, path) = write_json(&private_action);
    rejected(&path, "smallest_safe_next_action");

    let mut credential_identity = valid_input();
    credential_identity["workflow"]["identity"] = json!("ghp_012345678901234567890123456789012345");
    let (_temp, path) = write_json(&credential_identity);
    rejected(&path, "workflow.identity");
}

#[cfg(unix)]
#[test]
fn rejects_symlink_input_without_following_it() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let link = temp.path().join("input-link.json");
    symlink(fixture_path(), &link).expect("symlink");
    rejected(&link, "without following links");
}

#[test]
fn sanitized_bundle_fingerprint_is_compatible_with_governed_issue_preview() {
    let (_bundle_temp, bundle_path, bundle_output) = run_bundle(&fixture_path());
    assert_success(&bundle_output);
    let bundle: Value =
        serde_json::from_slice(&fs::read(&bundle_path).expect("bundle")).expect("bundle json");
    let fingerprint = bundle["problem_fingerprint"].as_str().expect("fingerprint");
    let bundle_sha256 = bundle["bundle_sha256"].as_str().expect("bundle sha256");
    let mut evidence: Value = serde_json::from_slice(
        &fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/github-issue-draft/valid-evidence.json"),
        )
        .expect("draft fixture"),
    )
    .expect("draft fixture json");
    evidence["draft"]["title"] = json!("Troubleshooting bundle readback");
    evidence["draft"]["body"] = json!(format!(
        "Sanitized untrusted troubleshooting input.\n\nProblem fingerprint: {fingerprint}"
    ));
    evidence["repair"]["evidence_pack_sha256"] = json!(bundle_sha256);
    let (evidence_temp, evidence_path) = write_json(&evidence);
    let action_path = evidence_temp.path().join("action.json");
    let preview = ao2(&[
        "issue",
        "draft-pr",
        "preview",
        "--evidence",
        evidence_path.to_str().expect("evidence path"),
        "--support-bundle",
        bundle_path.to_str().expect("bundle path"),
        "--out",
        action_path.to_str().expect("action path"),
        "--json",
    ]);
    assert_success(&preview);
    let preview_json: Value = serde_json::from_slice(&preview.stdout).expect("preview json");
    assert_eq!(preview_json["subject"]["safety"]["issue_write"], false);
    assert_eq!(preview_json["subject"]["safety"]["merge"], false);
    assert_eq!(preview_json["subject"]["safety"]["release"], false);
    let digest = preview_json["approval"]["action_digest"]
        .as_str()
        .expect("action digest");
    let verify = ao2(&[
        "issue",
        "draft-pr",
        "verify",
        "--action",
        action_path.to_str().expect("action path"),
        "--expected-action-digest",
        digest,
        "--json",
    ]);
    assert_success(&verify);
    let verify_json: Value = serde_json::from_slice(&verify.stdout).expect("verify json");
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["fixture_write_observed"], false);
    assert_eq!(verify_json["client_issue_write_performed"], false);
    assert_eq!(verify_json["client_merge_performed"], false);

    let mut altered_bundle = bundle.clone();
    altered_bundle["logs"]
        .as_array_mut()
        .expect("logs")
        .push(json!({"sequence": 4, "text": "[REDACTED_LOG]"}));
    altered_bundle["redaction"]["input_log_entries"] = json!(4);
    altered_bundle["redaction"]["fully_redacted_log_entries"] = json!(4);
    let (_altered_temp, altered_path) = write_json(&altered_bundle);
    let rejected_action = evidence_temp.path().join("rejected-action.json");
    let altered = ao2(&[
        "issue",
        "draft-pr",
        "preview",
        "--evidence",
        evidence_path.to_str().expect("evidence path"),
        "--support-bundle",
        altered_path.to_str().expect("altered bundle path"),
        "--out",
        rejected_action.to_str().expect("action path"),
        "--json",
    ]);
    assert!(!altered.status.success(), "altered bundle was accepted");
    assert!(String::from_utf8_lossy(&altered.stderr).contains("bundle digest"));

    let mut false_summary = bundle.clone();
    false_summary["redaction"]["fully_redacted_log_entries"] = json!(999);
    let (_summary_temp, summary_path) = write_json(&false_summary);
    let false_summary_action = evidence_temp.path().join("false-summary-action.json");
    let false_summary_output = ao2(&[
        "issue",
        "draft-pr",
        "preview",
        "--evidence",
        evidence_path.to_str().expect("evidence path"),
        "--support-bundle",
        summary_path.to_str().expect("bundle path"),
        "--out",
        false_summary_action.to_str().expect("action path"),
        "--json",
    ]);
    assert!(
        !false_summary_output.status.success(),
        "fabricated redaction summary was accepted"
    );
    assert!(String::from_utf8_lossy(&false_summary_output.stderr).contains("redaction summary"));

    let mut mismatched_evidence = evidence;
    mismatched_evidence["repair"]["evidence_pack_sha256"] = json!("9".repeat(64));
    let (_mismatch_temp, mismatch_path) = write_json(&mismatched_evidence);
    let mismatched_action = evidence_temp.path().join("mismatched-action.json");
    let mismatch = ao2(&[
        "issue",
        "draft-pr",
        "preview",
        "--evidence",
        mismatch_path.to_str().expect("evidence path"),
        "--support-bundle",
        bundle_path.to_str().expect("bundle path"),
        "--out",
        mismatched_action.to_str().expect("action path"),
        "--json",
    ]);
    assert!(
        !mismatch.status.success(),
        "mismatched binding was accepted"
    );
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("not bound"));
}
