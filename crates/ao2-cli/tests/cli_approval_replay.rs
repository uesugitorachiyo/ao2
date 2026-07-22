use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};

fn copy_fixture(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn copy_git_fixture(src: &Path, dst: &Path) {
    copy_fixture(src, dst);
    init_existing_git_repo(dst);
}

fn init_git_repo(repo: &Path) {
    fs::create_dir_all(repo).unwrap();
    fs::write(repo.join("README.md"), "before\n").unwrap();
    init_existing_git_repo(repo);
}

fn init_existing_git_repo(repo: &Path) {
    assert!(Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["config", "user.email", "ao2-test@example.invalid"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["config", "user.name", "AO2 Test"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["config", "core.longpaths", "true"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
}

fn read_test_http_request(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream.read(buffer).unwrap()
}

#[test]
fn test_http_accept_waits_for_slow_windows_child_startup() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let client = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5500));
        TcpStream::connect(("127.0.0.1", port)).unwrap();
    });

    let stream = accept_test_connection(&listener, "delayed local test HTTP request");
    drop(stream);
    client.join().unwrap();
}

#[test]
fn cli_factory_queue_executes_project_start_handoff_job() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-start-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("queued-project-start");
    let receipt_out = temp.path().join("queue-project-start-submit.json");

    let submit = ao2([
        "factory",
        "queue-submit-project-start",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--out",
        receipt_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    let submitted: serde_json::Value = serde_json::from_str(&stdout(&submit)).unwrap();
    assert_eq!(
        submitted["schema_version"],
        "ao2.factory-project-start-workbench-queue-submit.v1"
    );
    assert_eq!(submitted["status"], "queued");
    assert_eq!(submitted["job_kind"], "factory_project_start");
    assert_eq!(
        submitted["entry"]["execution_contract"]["execution_owner"],
        "ao2"
    );
    assert_eq!(
        submitted["entry"]["execution_contract"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        submitted["entry"]["execution_contract"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        submitted["entry"]["parity_checklist_progress"]
            ["ao2_queue_executes_project_start_handoff_job"],
        true
    );
    assert!(receipt_out.is_file());
    assert!(Path::new(submitted["queue_path"].as_str().unwrap()).is_file());

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));
    let result: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    assert_eq!(
        result["schema_version"],
        "ao2.factory-project-start-workbench-queue-run-next.v1"
    );
    assert_eq!(result["run_id"], "queued-project-start");
    assert_eq!(result["job_kind"], "factory_project_start");
    assert_eq!(result["status"], "accepted");
    assert_eq!(result["entry"]["status"], "accepted");
    assert_eq!(result["entry"]["project_start_status"], "accepted");
    assert_eq!(
        result["entry"]["project_acceptance_review_status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_acceptance_review_recommended_decision"],
        "accept"
    );
    assert!(Path::new(
        result["entry"]["project_acceptance_review"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert_eq!(
        result["entry"]["project_start_result"]["project_acceptance_review"]["schema_version"],
        "ao2.factory-project-acceptance-review.v1"
    );
    assert_eq!(
        result["entry"]["project_start_result"]["project_acceptance_review"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_result"]["project_acceptance_review"]["signature"]
            ["signature_status"],
        "signed"
    );
    assert_eq!(
        result["hermes_queue_handoff_schema"],
        "ao2.hermes-project-start-handoff.v1"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["schema_version"],
        "ao2.hermes-project-start-handoff.v1"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["project_start_bundle"],
        result["entry"]["project_start_bundle"]
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["project_start_bundle_sha256"],
        result["entry"]["project_start_bundle_sha256"]
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert!(Path::new(result["entry"]["project_start"].as_str().unwrap()).is_file());
    assert!(Path::new(result["entry"]["project_start_bundle"].as_str().unwrap()).is_file());
    assert_eq!(
        result["entry"]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert!(Path::new(
        result["entry"]["project_start_bundle_verification"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["schema_version"],
        "ao2.factory-project-start-bundle-verification.v1"
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["checks"]
            ["project_acceptance_review_signature_verified"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["checks"]
            ["review_rubric_digest_matches"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["checks"]
            ["review_project_run_digest_matches"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_status"],
        "accepted"
    );
    let operator_summary_path = Path::new(
        result["entry"]["project_start_operator_summary"]
            .as_str()
            .unwrap(),
    );
    assert!(operator_summary_path.is_file());
    assert!(Path::new(
        result["entry"]["project_start_operator_summary_markdown"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["schema_version"],
        "ao2.factory-project-start-operator-summary.v1"
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["checks"]
            ["project_start_accepted"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["checks"]
            ["bundle_verification_accepted"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["checks"]["bundle_digest_matches"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_checks"],
        result["entry"]["project_start_operator_summary_result"]["checks"]
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_sha256"],
        sha256_path(operator_summary_path)
    );
    assert_eq!(result["entry"]["project_start_closure_status"], "packaged");
    let project_start_closure_path =
        Path::new(result["entry"]["project_start_closure"].as_str().unwrap());
    assert!(project_start_closure_path.is_file());
    assert_eq!(
        result["entry"]["project_start_closure_sha256"],
        sha256_path(project_start_closure_path)
    );
    assert_eq!(
        result["entry"]["project_start_closure_result"]["schema_version"],
        "ao2.factory-project-start-closure.v1"
    );
    assert_eq!(
        result["entry"]["project_start_closure_result"]["latest_selector_matches_run_id_selector"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_status"],
        "accepted"
    );
    let project_start_closure_verification_path = Path::new(
        result["entry"]["project_start_closure_verification"]
            .as_str()
            .unwrap(),
    );
    assert!(project_start_closure_verification_path.is_file());
    assert_eq!(
        result["entry"]["project_start_closure_verification_sha256"],
        sha256_path(project_start_closure_verification_path)
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_result"]["schema_version"],
        "ao2.factory-project-start-closure-verification.v1"
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_result"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_checks"]["checksums_verified"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_checks"]["trust_boundary_verified"],
        true
    );
    assert_eq!(
        result["parity_checklist_progress"]["ao2_queue_executes_project_start_handoff_job"],
        true
    );
    assert_eq!(
        result["parity_checklist_progress"]["ao2_queue_verifies_project_start_handoff_bundle"],
        true
    );
    assert_eq!(
        result["parity_checklist_progress"]["factory_v3_drives_workflow"],
        false
    );

    let list = ao2([
        "factory",
        "queue-list",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(list.status.success(), "{}", stderr(&list));
    let listed: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(listed["entries"][0]["job_kind"], "factory_project_start");
    assert_eq!(listed["entries"][0]["status"], "accepted");
    assert_eq!(
        listed["entries"][0]["project_start_bundle"],
        result["entry"]["project_start_bundle"]
    );
    assert_eq!(
        listed["entries"][0]["project_acceptance_review_status"],
        "accepted"
    );
    assert_eq!(
        listed["entries"][0]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert_eq!(
        listed["entries"][0]["project_start_operator_summary_status"],
        "accepted"
    );
    assert_eq!(
        listed["entries"][0]["project_start_closure_status"],
        "packaged"
    );
    assert_eq!(
        listed["entries"][0]["project_start_closure_verification_status"],
        "accepted"
    );

    let queue_path = Path::new(result["queue_path"].as_str().unwrap());
    let queue_sha_before_status = sha256_path(queue_path);
    let queue_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(queue_status.status.success(), "{}", stderr(&queue_status));
    let detail: serde_json::Value = serde_json::from_str(&stdout(&queue_status)).unwrap();
    assert_eq!(detail["schema_version"], "ao2.factory-queue-status.v1");
    assert_eq!(detail["status"], "accepted");
    assert_eq!(detail["run_id"], "queued-project-start");
    assert_eq!(detail["queue_path"], result["queue_path"]);
    assert_eq!(detail["entry"], listed["entries"][0]);
    assert_eq!(
        detail["entry"]["project_start_operator_summary_status"],
        "accepted"
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_sha256"],
        sha256_path(operator_summary_path)
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_checks"]["bundle_digest_matches"],
        true
    );
    assert_eq!(
        detail["entry"]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert_eq!(detail["entry"]["project_start_closure_status"], "packaged");
    assert_eq!(
        detail["entry"]["project_start_closure_verification_status"],
        "accepted"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(detail["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        detail["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        detail["parity_checklist_progress"]["ao2_queue_status_detail_is_read_only"],
        true
    );
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));
    assert!(!stdout(&run_next).contains("Bearer "));
    assert!(!stdout(&queue_status).contains("Bearer "));

    let completion_contract = ao2([
        "factory",
        "queue-completion-contract",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(
        completion_contract.status.success(),
        "{}",
        stderr(&completion_contract)
    );
    let completion: serde_json::Value =
        serde_json::from_str(&stdout(&completion_contract)).unwrap();
    assert_eq!(
        completion["schema_version"],
        "ao2.factory-project-start-queue-completion-contract.v1"
    );
    assert_eq!(completion["status"], "accepted");
    assert_eq!(completion["run_id"], "queued-project-start");
    assert_eq!(
        completion["source_queue_status"]["schema_version"],
        "ao2.factory-queue-status.v1"
    );
    assert_eq!(
        completion["artifacts"]["project_start_bundle"],
        detail["entry"]["project_start_bundle"]
    );
    assert_eq!(
        completion["artifacts"]["project_start_closure"],
        detail["entry"]["project_start_closure"]
    );
    assert_eq!(
        completion["checks"]["project_start_closure_verification_status"],
        "accepted"
    );
    assert_eq!(
        completion["checks"]["project_start_closure_verification_checksums_verified"],
        true
    );
    assert_eq!(
        completion["hermes_contract"]["front_end_reads_single_completion_record"],
        true
    );
    assert_eq!(
        completion["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        completion["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(completion["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));
    assert!(!stdout(&completion_contract).contains("Bearer "));

    let latest_completion_contract = ao2([
        "factory",
        "queue-completion-contract",
        "--target",
        repo.to_str().unwrap(),
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(
        latest_completion_contract.status.success(),
        "{}",
        stderr(&latest_completion_contract)
    );
    let latest_completion: serde_json::Value =
        serde_json::from_str(&stdout(&latest_completion_contract)).unwrap();
    assert_eq!(latest_completion["run_id"], "queued-project-start");
    assert_eq!(
        latest_completion["artifacts"]["project_start_closure_sha256"],
        detail["entry"]["project_start_closure_sha256"]
    );
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    let completion_contract_path = temp.path().join("queue-completion-contract.json");
    fs::write(
        &completion_contract_path,
        format!("{}\n", stdout(&completion_contract)),
    )
    .unwrap();
    let consumed_contract = ao2([
        "factory",
        "queue-completion-contract-consume",
        "--contract",
        completion_contract_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        consumed_contract.status.success(),
        "{}",
        stderr(&consumed_contract)
    );
    let consumed: serde_json::Value = serde_json::from_str(&stdout(&consumed_contract)).unwrap();
    assert_eq!(
        consumed["schema_version"],
        "ao2.factory-project-start-queue-completion-contract-consumption.v1"
    );
    assert_eq!(consumed["status"], "accepted");
    assert_eq!(consumed["ready_for_operator_review"], true);
    assert_eq!(consumed["run_id"], "queued-project-start");
    assert_eq!(
        consumed["source_contract_schema"],
        "ao2.factory-project-start-queue-completion-contract.v1"
    );
    assert_eq!(consumed["hermes_contract"]["consumed_contract_only"], true);
    assert_eq!(
        consumed["hermes_contract"]["front_end_reads_single_completion_record"],
        true
    );
    assert_eq!(
        consumed["hermes_contract"]["requires_manual_closure_commands"],
        false
    );
    assert_eq!(
        consumed["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        consumed["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(consumed["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(!stdout(&consumed_contract).contains("Bearer "));

    let mut rejected_contract = completion.clone();
    rejected_contract["checks"]["project_start_closure_verification_status"] =
        serde_json::Value::String("rejected".to_string());
    fs::write(
        &completion_contract_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&rejected_contract).unwrap()
        ),
    )
    .unwrap();
    let rejected_consume = ao2([
        "factory",
        "queue-completion-contract-consume",
        "--contract",
        completion_contract_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !rejected_consume.status.success(),
        "consumer must fail closed when the completion contract verifier status is rejected"
    );
    assert!(stderr(&rejected_consume)
        .contains("project_start_closure_verification_status must be accepted"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    fs::write(
        project_start_closure_path,
        b"tampered queued project-start closure",
    )
    .unwrap();
    let tampered_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(
        !tampered_status.status.success(),
        "queue-status must fail closed on tampered project-start closure sidecar"
    );
    assert!(stderr(&tampered_status).contains("project_start_closure digest mismatch"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    let tampered_latest = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(
        !tampered_latest.status.success(),
        "latest project-start queue-status must fail closed on tampered closure sidecar"
    );
    assert!(stderr(&tampered_latest).contains("project_start_closure digest mismatch"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    let tampered_contract = ao2([
        "factory",
        "queue-completion-contract",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(
        !tampered_contract.status.success(),
        "queue-completion-contract must reuse queue-status digest checks"
    );
    assert!(stderr(&tampered_contract).contains("project_start_closure digest mismatch"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));
}

#[test]
fn cli_factory_queue_project_start_complete_returns_hermes_ready_result() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-start-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("one-shot-project-start");

    let completed = ao2([
        "factory",
        "queue-project-start-complete",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-complete-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(completed.status.success(), "{}", stderr(&completed));
    let result: serde_json::Value = serde_json::from_str(&stdout(&completed)).unwrap();
    assert_eq!(
        result["schema_version"],
        "ao2.factory-project-start-queue-complete.v1"
    );
    assert_eq!(result["status"], "accepted");
    assert_eq!(result["ready_for_operator_review"], true);
    assert_eq!(result["run_id"], "queued-project-start-one-shot");
    assert_eq!(result["queue_run_next_status"], "accepted");
    assert_eq!(result["completion_contract_status"], "accepted");
    assert_eq!(result["completion_contract_consumer_status"], "accepted");
    assert_eq!(
        result["completion_contract_consumer"]["schema_version"],
        "ao2.factory-project-start-queue-completion-contract-consumption.v1"
    );
    assert_eq!(
        result["completion_contract_consumer"]["hermes_contract"]["consumed_contract_only"],
        true
    );
    assert_eq!(
        result["completion_contract_consumer"]["hermes_contract"]
            ["requires_manual_closure_commands"],
        false
    );
    assert_eq!(
        result["hermes_contract"]["front_end_reads_single_completion_record"],
        true
    );
    assert_eq!(
        result["hermes_contract"]["backend_used_bounded_ao2_queue"],
        true
    );
    assert_eq!(
        result["hermes_contract"]["requires_manual_command_sequence"],
        false
    );
    assert_eq!(
        result["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        result["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(result["trust_boundary"]["mutates_ao_artifacts"], false);
    for key in [
        "queue_submit",
        "queue_run_next",
        "completion_contract",
        "completion_contract_consumer",
    ] {
        assert!(
            Path::new(result["artifacts"][key].as_str().unwrap()).is_file(),
            "missing {key}"
        );
    }
    let queue_path = Path::new(result["queue_path"].as_str().unwrap());
    let queue_sha_after_complete = sha256_path(queue_path);
    let status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(status_json["status"], "accepted");
    assert_eq!(
        status_json["entry"]["project_start_closure_verification_status"],
        "accepted"
    );
    assert_eq!(queue_sha_after_complete, sha256_path(queue_path));
    assert!(!stdout(&completed).contains("Bearer "));

    let replayed = ao2([
        "factory",
        "queue-project-start-complete",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-complete-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        replayed.status.success(),
        "same run-id replay should reuse accepted queue evidence: {}",
        stderr(&replayed)
    );
    let replayed_json: serde_json::Value = serde_json::from_str(&stdout(&replayed)).unwrap();
    assert_eq!(replayed_json["status"], "accepted");
    assert_eq!(
        replayed_json["resume"]["mode"],
        "reused_existing_queue_entry"
    );
    assert_eq!(queue_sha_after_complete, sha256_path(queue_path));
    let queue_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(queue_path).unwrap()).unwrap();
    let matching_entries = queue_json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["run_id"] == "queued-project-start-one-shot")
        .count();
    assert_eq!(
        matching_entries, 1,
        "replay must not duplicate queue entries"
    );

    let consumer_path = Path::new(
        replayed_json["artifacts"]["completion_contract_consumer"]
            .as_str()
            .unwrap(),
    );
    fs::remove_file(consumer_path).unwrap();
    let resumed = ao2([
        "factory",
        "queue-project-start-complete",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-complete-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        resumed.status.success(),
        "partial wrapper resume should rebuild missing consumer: {}",
        stderr(&resumed)
    );
    let resumed_json: serde_json::Value = serde_json::from_str(&stdout(&resumed)).unwrap();
    assert_eq!(resumed_json["status"], "accepted");
    assert_eq!(
        resumed_json["resume"]["mode"],
        "reused_existing_queue_entry"
    );
    assert!(
        consumer_path.is_file(),
        "missing consumer was not regenerated"
    );
    assert_eq!(queue_sha_after_complete, sha256_path(queue_path));

    let consumer_modified_before = fs::metadata(consumer_path).unwrap().modified().unwrap();
    let queue_sha_before_probe = sha256_path(queue_path);
    let probe = ao2([
        "factory",
        "queue-project-start-complete-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        probe.status.success(),
        "read-only complete-status probe should not execute queue: {}",
        stderr(&probe)
    );
    let probe_json: serde_json::Value = serde_json::from_str(&stdout(&probe)).unwrap();
    assert_eq!(
        probe_json["schema_version"],
        "ao2.factory-project-start-queue-complete-status.v1"
    );
    assert_eq!(probe_json["status"], "accepted");
    assert_eq!(probe_json["completion_record_state"], "complete");
    assert_eq!(probe_json["read_only"], true);
    assert_eq!(probe_json["would_execute_queue"], false);
    assert_eq!(probe_json["would_rebuild_wrappers"], false);
    assert_eq!(probe_json["ready_for_operator_review"], true);
    assert_eq!(queue_sha_before_probe, sha256_path(queue_path));
    assert_eq!(
        consumer_modified_before,
        fs::metadata(consumer_path).unwrap().modified().unwrap()
    );
}

fn write_probe_queue(repo: &Path, run_id: &str, status: &str, job_kind: &str) -> PathBuf {
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    fs::create_dir_all(queue_path.parent().unwrap()).unwrap();
    let entries = if run_id.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "schema_version": "ao2.factory-project-start-workbench-queue-entry.v1",
            "run_id": run_id,
            "job_kind": job_kind,
            "status": status,
            "attempts": 0
        })]
    };
    fs::write(
        &queue_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-workbench-queue.v1",
            "owner": "ao2-workbench-queue",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "entry_count": entries.len(),
            "entries": entries
        }))
        .unwrap(),
    )
    .unwrap();
    queue_path
}

fn write_probe_compact_artifacts(out_dir: &Path, run_id: &str) {
    fs::create_dir_all(out_dir).unwrap();
    for (file, body) in [
        (
            "factory-queue-project-start-submit.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-workbench-queue-submit.v1",
                "status": "queued",
                "job_kind": "factory_project_start",
                "run_id": run_id
            }),
        ),
        (
            "factory-queue-project-start-run-next.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-workbench-queue-run-next.v1",
                "status": "accepted",
                "job_kind": "factory_project_start",
                "run_id": run_id
            }),
        ),
        (
            "factory-queue-project-start-completion-contract.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-queue-completion-contract.v1",
                "status": "accepted",
                "job_kind": "factory_project_start",
                "run_id": run_id
            }),
        ),
        (
            "factory-queue-project-start-completion-contract-consumer.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-queue-completion-contract-consumption.v1",
                "status": "accepted",
                "ready_for_operator_review": true,
                "run_id": run_id,
                "trust_boundary": {
                    "release_acceptance_owner": "factory-v3 evaluator-closer",
                    "control_plane_approves_release": false,
                    "mutates_ao_artifacts": false
                }
            }),
        ),
    ] {
        fs::write(
            out_dir.join(file),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
    }
}

fn project_start_complete_status_probe(
    repo: &Path,
    out_dir: &Path,
    run_id: &str,
) -> serde_json::Value {
    let output = ao2([
        "factory",
        "queue-project-start-complete-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).unwrap()
}

fn assert_blocker_code(value: &serde_json::Value, code: &str) {
    let codes = value["blocker_codes"].as_array().unwrap();
    assert!(
        codes
            .iter()
            .any(|candidate| candidate.as_str() == Some(code)),
        "missing blocker code {code}: {value:#}"
    );
}

fn project_start_next_action(
    repo: &Path,
    out_dir: &Path,
    run_id: &str,
    contract: &Path,
) -> serde_json::Value {
    let output = ao2([
        "factory",
        "queue-project-start-next-action",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).unwrap()
}

#[test]
fn cli_factory_queue_project_start_complete_status_reports_fail_closed_matrix_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out_dir = temp.path().join("complete-status-out");
    let run_id = "queued-project-start-status-matrix";
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");

    let missing_queue = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(missing_queue["status"], "missing");
    assert_eq!(
        missing_queue["completion_record_state"],
        "missing_queue_file"
    );
    assert_eq!(missing_queue["read_only"], true);
    assert_eq!(missing_queue["would_execute_queue"], false);
    assert_eq!(missing_queue["would_rebuild_wrappers"], false);
    assert_blocker_code(&missing_queue, "missing_queue_file");
    assert!(!queue_path.exists(), "probe must not create the queue file");
    assert!(
        !out_dir.exists(),
        "probe must not create compact artifact dir"
    );

    let queue_path = write_probe_queue(&repo, "", "queued", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let missing_entry = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(missing_entry["status"], "missing");
    assert_eq!(
        missing_entry["completion_record_state"],
        "missing_queue_entry"
    );
    assert_blocker_code(&missing_entry, "missing_queue_entry");
    assert_eq!(queue_sha, sha256_path(&queue_path));

    for status in ["queued", "running", "rejected"] {
        let queue_path = write_probe_queue(&repo, run_id, status, "factory_project_start");
        let queue_sha = sha256_path(&queue_path);
        let probed = project_start_complete_status_probe(&repo, &out_dir, run_id);
        assert_eq!(probed["status"], status);
        assert_eq!(probed["completion_record_state"], status);
        assert_blocker_code(&probed, &format!("queue_entry_status_{status}"));
        assert_eq!(probed["would_execute_queue"], false);
        assert_eq!(queue_sha, sha256_path(&queue_path));
    }

    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let missing_artifacts = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(missing_artifacts["status"], "incomplete");
    assert_eq!(
        missing_artifacts["completion_record_state"],
        "missing_compact_artifact"
    );
    assert_blocker_code(&missing_artifacts, "missing_compact_artifact_queue_submit");
    assert_blocker_code(
        &missing_artifacts,
        "missing_compact_artifact_completion_contract_consumer",
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    write_probe_compact_artifacts(&out_dir, run_id);
    let consumer_path =
        out_dir.join("factory-queue-project-start-completion-contract-consumer.json");
    let consumer_modified_before = fs::metadata(&consumer_path).unwrap().modified().unwrap();
    let accepted = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["completion_record_state"], "complete");
    assert_eq!(accepted["ready_for_operator_review"], true);
    assert!(accepted["blocker_codes"].as_array().unwrap().is_empty());
    assert_eq!(queue_sha, sha256_path(&queue_path));
    assert_eq!(
        consumer_modified_before,
        fs::metadata(&consumer_path).unwrap().modified().unwrap()
    );

    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let mut run_next: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&run_next_path).unwrap()).unwrap();
    run_next["run_id"] = serde_json::Value::String("wrong-run-id".to_string());
    fs::write(
        &run_next_path,
        serde_json::to_string_pretty(&run_next).unwrap(),
    )
    .unwrap();
    let run_id_mismatch = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(run_id_mismatch["status"], "blocked");
    assert_eq!(
        run_id_mismatch["completion_record_state"],
        "artifact_mismatch"
    );
    assert_blocker_code(&run_id_mismatch, "artifact_run_id_mismatch_queue_run_next");
    assert_eq!(queue_sha, sha256_path(&queue_path));

    write_probe_compact_artifacts(&out_dir, run_id);
    let mut consumer: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&consumer_path).unwrap()).unwrap();
    consumer["trust_boundary"]["control_plane_approves_release"] = serde_json::Value::Bool(true);
    fs::write(
        &consumer_path,
        serde_json::to_string_pretty(&consumer).unwrap(),
    )
    .unwrap();
    let trust_mismatch = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(trust_mismatch["status"], "blocked");
    assert_eq!(
        trust_mismatch["completion_record_state"],
        "artifact_mismatch"
    );
    assert_blocker_code(
        &trust_mismatch,
        "trust_boundary_mismatch_completion_contract_consumer",
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));
}

#[test]
fn cli_factory_queue_project_start_next_action_maps_status_and_contract_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out_dir = temp.path().join("next-action-out");
    let run_id = "queued-project-start-next-action";
    let contract = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/contracts/hermes-project-start-poll-act-contract.v1.json");
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");

    let missing_queue = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(
        missing_queue["schema_version"],
        "ao2.factory-project-start-next-action.v1"
    );
    assert_eq!(
        missing_queue["next_action"],
        "call_queue_project_start_complete"
    );
    assert_eq!(missing_queue["read_only"], true);
    assert_eq!(missing_queue["would_execute_queue"], false);
    assert_eq!(missing_queue["would_submit_queue_entry"], false);
    assert_eq!(missing_queue["would_rebuild_wrappers"], false);
    assert_eq!(
        missing_queue["status_probe"]["completion_record_state"],
        "missing_queue_file"
    );
    assert_blocker_code(&missing_queue["status_probe"], "missing_queue_file");
    assert!(!queue_path.exists(), "next-action must not create queue");
    assert!(!out_dir.exists(), "next-action must not create wrappers");

    let queue_path = write_probe_queue(&repo, run_id, "running", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let running = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(running["next_action"], "wait_and_poll");
    assert_eq!(
        running["status_probe"]["completion_record_state"],
        "running"
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let queue_path = write_probe_queue(&repo, run_id, "rejected", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let rejected = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(rejected["next_action"], "operator_review_required");
    assert_eq!(
        rejected["status_probe"]["completion_record_state"],
        "rejected"
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    write_probe_compact_artifacts(&out_dir, run_id);
    let complete = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(complete["next_action"], "publish_operator_record");
    assert_eq!(
        complete["status_probe"]["completion_record_state"],
        "complete"
    );
    assert_eq!(
        complete["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        complete["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(complete["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let mut run_next: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&run_next_path).unwrap()).unwrap();
    run_next["run_id"] = serde_json::Value::String("wrong-run-id".to_string());
    fs::write(
        &run_next_path,
        serde_json::to_string_pretty(&run_next).unwrap(),
    )
    .unwrap();
    let corrupt = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(corrupt["next_action"], "operator_review_required");
    assert_blocker_code(
        &corrupt["status_probe"],
        "artifact_run_id_mismatch_queue_run_next",
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let bad_contract = temp.path().join("bad-contract.json");
    fs::write(
        &bad_contract,
        r#"{
          "schema_version": "ao2.hermes-project-start-poll-act-contract.v1",
          "decision_table": [],
          "trust_boundary": {
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
          }
        }"#,
    )
    .unwrap();
    let bad = ao2([
        "factory",
        "queue-project-start-next-action",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        bad_contract.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !bad.status.success(),
        "next-action must fail closed on incomplete contract"
    );
    assert!(stderr(&bad).contains("contract omits blocker_code"));
}

#[test]
fn cli_factory_queue_project_start_publish_operator_record_fails_closed_and_writes_compact_record()
{
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out_dir = temp.path().join("publish-operator-record-out");
    let run_id = "queued-project-start-publish-operator-record";
    let record_out = temp.path().join("operator-record.json");
    let contract = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/contracts/hermes-project-start-poll-act-contract.v1.json");
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");

    let missing_queue = ao2([
        "factory",
        "queue-project-start-publish-operator-record",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--record-out",
        record_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !missing_queue.status.success(),
        "publisher must fail closed before the compact completion artifacts exist"
    );
    assert!(
        stderr(&missing_queue).contains("next action is call_queue_project_start_complete"),
        "{}",
        stderr(&missing_queue)
    );
    assert!(
        !record_out.exists(),
        "blocked publish must not write record"
    );
    assert!(
        !queue_path.exists(),
        "blocked publish must not create queue"
    );
    assert!(
        !out_dir.exists(),
        "blocked publish must not create artifacts"
    );

    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    write_probe_compact_artifacts(&out_dir, run_id);
    let consumer_path =
        out_dir.join("factory-queue-project-start-completion-contract-consumer.json");
    let mut consumer: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&consumer_path).unwrap()).unwrap();
    consumer["trust_boundary"]["control_plane_approves_release"] = serde_json::Value::Bool(true);
    fs::write(
        &consumer_path,
        serde_json::to_string_pretty(&consumer).unwrap(),
    )
    .unwrap();
    let corrupt_consumer = ao2([
        "factory",
        "queue-project-start-publish-operator-record",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--record-out",
        record_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !corrupt_consumer.status.success(),
        "publisher must reject corrupt compact artifact trust boundaries"
    );
    assert!(
        stderr(&corrupt_consumer).contains("next action is operator_review_required"),
        "{}",
        stderr(&corrupt_consumer)
    );
    assert!(
        !record_out.exists(),
        "corrupt publish must not write record"
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    write_probe_compact_artifacts(&out_dir, run_id);
    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let run_next_sha = sha256_path(&run_next_path);
    let publish = ao2([
        "factory",
        "queue-project-start-publish-operator-record",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--record-out",
        record_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(publish.status.success(), "{}", stderr(&publish));
    let published: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(
        published["schema_version"],
        "ao2.factory-project-start-operator-record.v1"
    );
    assert_eq!(published["status"], "published");
    assert_eq!(published["run_id"], run_id);
    assert_eq!(published["record_path"], record_out.display().to_string());
    assert_eq!(
        published["read_only_preflight"]["next_action"],
        "publish_operator_record"
    );
    assert_eq!(
        published["record"]["schema_version"],
        "ao2.factory-project-start-operator-record.v1"
    );
    assert_eq!(published["record"]["run_id"], run_id);
    assert_eq!(published["record"]["status"], "ready_for_operator_review");
    assert_eq!(published["record"]["queue_sha256"], queue_sha);
    assert_eq!(
        published["record"]["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        published["record"]["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        published["record"]["trust_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert_eq!(
        published["record"]["source_artifacts"]["queue_run_next"]["sha256"],
        run_next_sha
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));
    assert_eq!(run_next_sha, sha256_path(&run_next_path));

    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&record_out).unwrap()).unwrap();
    assert_eq!(record, published["record"]);
}

#[test]
fn cli_factory_queue_status_reads_completed_project_start_without_mutating_queue() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("project.md");
    fs::write(
        &project_spec,
        r#"# Queue Status Project

## App Steps

- Build a minimal governed workflow fixture.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("queue-status-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("queued-project-start");

    let submit = ao2([
        "factory",
        "queue-submit-project-start",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queue-status-project",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-status-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));

    let queued_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queue-status-project",
        "--json",
    ]);
    assert!(
        !queued_status.status.success(),
        "unfinished queue entries must fail closed"
    );
    assert!(stderr(&queued_status).contains("not completed yet"));

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));
    let run_next_json: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    let queue_path = Path::new(run_next_json["queue_path"].as_str().unwrap());
    let queue_sha_before = sha256_path(queue_path);

    let status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queue-status-project",
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let detail: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(detail["schema_version"], "ao2.factory-queue-status.v1");
    assert_eq!(detail["status"], "accepted");
    assert_eq!(detail["entry"]["run_id"], "queue-status-project");
    assert_eq!(
        detail["entry"]["project_start_operator_summary_status"],
        "accepted"
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_result"]["schema_version"],
        "ao2.factory-project-start-operator-summary.v1"
    );
    assert_eq!(
        detail["entry"]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(detail["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        detail["parity_checklist_progress"]["ao2_queue_status_detail_is_read_only"],
        true
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_sha256"],
        sha256_path(Path::new(
            detail["entry"]["project_start_operator_summary"]
                .as_str()
                .unwrap()
        ))
    );
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    assert!(!stdout(&status).contains("Bearer "));
}

#[test]
fn cli_factory_queue_status_can_select_latest_completed_project_start_without_mutating_queue() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let queue_dir = repo.join(".ao2/factory-compat");
    fs::create_dir_all(&queue_dir).unwrap();
    let queue_path = queue_dir.join("queue.json");
    let old_summary = temp.path().join("old-project-start-summary.json");
    let latest_summary = temp.path().join("latest-project-start-summary.json");
    let old_closure = temp.path().join("old-project-start-closure.tgz");
    let old_closure_json = temp.path().join("old-project-start-closure.json");
    let old_closure_verification = temp
        .path()
        .join("old-project-start-closure-verification.json");
    let latest_closure = temp.path().join("latest-project-start-closure.tgz");
    let latest_closure_json = temp.path().join("latest-project-start-closure.json");
    let latest_closure_verification = temp
        .path()
        .join("latest-project-start-closure-verification.json");
    fs::write(
        &old_summary,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-operator-summary.v1",
            "status": "accepted",
            "run_id": "old-project-start",
            "checks": {"bundle_digest_matches": true}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &latest_summary,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-operator-summary.v1",
            "status": "accepted",
            "run_id": "latest-project-start",
            "checks": {"bundle_digest_matches": true}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&old_closure, b"old closure archive").unwrap();
    fs::write(
        &old_closure_json,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure.v1",
            "status": "packaged",
            "run_id": "old-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &old_closure_verification,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure-verification.v1",
            "status": "accepted",
            "run_id": "old-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&latest_closure, b"latest closure archive").unwrap();
    fs::write(
        &latest_closure_json,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure.v1",
            "status": "packaged",
            "run_id": "latest-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &latest_closure_verification,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure-verification.v1",
            "status": "accepted",
            "run_id": "latest-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    let old_summary_sha = sha256_path(&old_summary);
    let latest_summary_sha = sha256_path(&latest_summary);
    let old_closure_sha = sha256_path(&old_closure);
    let old_closure_json_sha = sha256_path(&old_closure_json);
    let old_closure_verification_sha = sha256_path(&old_closure_verification);
    let latest_closure_sha = sha256_path(&latest_closure);
    let latest_closure_json_sha = sha256_path(&latest_closure_json);
    let latest_closure_verification_sha = sha256_path(&latest_closure_verification);
    fs::write(
        &queue_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-workbench-queue.v1",
            "owner": "ao2-workbench-queue",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "entry_count": 3,
            "continuity_contract": {
                "survives_server_restart": true,
                "factory_v3_drives_workflow": false,
                "cancel_retry_state_owner": "ao2-workbench-queue",
                "history_owner": "ao2",
                "hermes_role": "front_end_scheduler_queue_and_memory_bookkeeping"
            },
            "entries": [
                {
                    "run_id": "old-project-start",
                    "job_kind": "factory_project_start",
                    "status": "accepted",
                    "project_start_operator_summary": old_summary.display().to_string(),
                    "project_start_operator_summary_sha256": old_summary_sha,
                    "project_start_closure": old_closure.display().to_string(),
                    "project_start_closure_sha256": old_closure_sha,
                    "project_start_closure_json": old_closure_json.display().to_string(),
                    "project_start_closure_json_sha256": old_closure_json_sha,
                    "project_start_closure_status": "packaged",
                    "project_start_closure_verification": old_closure_verification.display().to_string(),
                    "project_start_closure_verification_sha256": old_closure_verification_sha,
                    "project_start_closure_verification_status": "accepted"
                },
                {
                    "run_id": "latest-project-start",
                    "job_kind": "factory_project_start",
                    "status": "accepted",
                    "project_start_operator_summary": latest_summary.display().to_string(),
                    "project_start_operator_summary_sha256": latest_summary_sha,
                    "project_start_closure": latest_closure.display().to_string(),
                    "project_start_closure_sha256": latest_closure_sha,
                    "project_start_closure_json": latest_closure_json.display().to_string(),
                    "project_start_closure_json_sha256": latest_closure_json_sha,
                    "project_start_closure_status": "packaged",
                    "project_start_closure_verification": latest_closure_verification.display().to_string(),
                    "project_start_closure_verification_sha256": latest_closure_verification_sha,
                    "project_start_closure_verification_status": "accepted"
                },
                {
                    "run_id": "newer-but-still-running",
                    "job_kind": "factory_project_start",
                    "status": "running"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let queue_sha_before = sha256_path(&queue_path);

    let latest = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(latest.status.success(), "{}", stderr(&latest));
    let detail: serde_json::Value = serde_json::from_str(&stdout(&latest)).unwrap();
    assert_eq!(detail["schema_version"], "ao2.factory-queue-status.v1");
    assert_eq!(detail["status"], "accepted");
    assert_eq!(detail["run_id"], "latest-project-start");
    assert_eq!(detail["entry"]["run_id"], "latest-project-start");
    assert_eq!(
        detail["entry"]["project_start_operator_summary_sha256"],
        latest_summary_sha
    );
    assert_eq!(
        detail["entry"]["project_start_closure_sha256"],
        latest_closure_sha
    );
    assert_eq!(
        detail["entry"]["project_start_closure_json_sha256"],
        latest_closure_json_sha
    );
    assert_eq!(
        detail["entry"]["project_start_closure_verification_sha256"],
        latest_closure_verification_sha
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(detail["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        detail["parity_checklist_progress"]["ao2_queue_status_detail_is_read_only"],
        true
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert!(!stdout(&latest).contains("Bearer "));

    let both_selectors = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "latest-project-start",
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(
        !both_selectors.status.success(),
        "--run-id and --latest-completed-project-start must be mutually exclusive"
    );
    assert!(stderr(&both_selectors).contains("mutually exclusive"));
}

#[test]
fn cli_init_provider_profiles_and_template_run_support_fast_start() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let init = ao2(["init", "--target", repo.to_str().unwrap()]);
    assert!(init.status.success(), "{}", stderr(&init));
    let profiles = fs::read_to_string(repo.join(".ao2/provider-profiles.json")).unwrap();
    assert!(profiles.contains("\"codex\""));
    assert!(profiles.contains("\"claude\""));
    assert!(profiles.contains("\"scripted\""));

    let list = ao2(["provider", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(stdout(&list).contains("codex"));
    assert!(stdout(&list).contains("claude"));

    let doctor = ao2(["provider", "doctor", "--provider", "scripted"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let doctor_json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(doctor_json["provider"], "scripted");

    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: provider preset template run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "--template",
        "bug-fix",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "preset-template-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Accepted"));
    assert!(repo.join(".ao2/generated-workflows/bug-fix.yaml").is_file());
}

#[test]
fn cli_run_provider_prompt_executes_provider_backed_risky_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: added validation around discount math\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 10\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-cli-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Accepted"));
    let evidence = fs::read_to_string(
        repo.join(".ao2/runs/provider-cli-run/evidence-pack/evidence-pack.json"),
    )
    .unwrap();
    assert!(evidence.contains("sandbox_patch_apply"));
    assert!(evidence.contains("provider_summaries"));
    assert!(evidence.contains("added validation around discount math"));
}

#[test]
fn cli_run_provider_prompt_honors_zero_repair_budget() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: validation without tests\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-cli-budget-zero",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Rejected"));
    let evidence = fs::read_to_string(
        repo.join(".ao2/runs/provider-cli-budget-zero/evidence-pack/evidence-pack.json"),
    )
    .unwrap();
    assert!(evidence.contains("repair_budget_exhausted"));
    assert!(evidence.contains("repair_attempts"));
}

#[test]
fn cli_repair_resume_uses_rejected_evidence_context_for_new_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project-repair-resume");
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(repo.join("README.md"), "real project\n").unwrap();
    init_existing_git_repo(&repo);
    let workflow = temp.path().join("repair-resume.yaml");
    fs::write(
        &workflow,
        r#"id: repair-resume
version: 0.1.0
template_kind: real_project
objective: Repair a failed run from prior signed evidence context.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test -f docs/fixed.txt
acceptance:
  - Fixed artifact exists after repair resume.
  - Prior verifier context is carried into the repair prompt.
"#,
    )
    .unwrap();
    let failed_prompt = temp.path().join("failed-prompt.sh");
    fs::write(
        &failed_prompt,
        r#"printf 'first attempt\n' > docs/first-attempt.txt
printf 'Summary: failed repair source run\n'
printf 'Changed files: docs/first-attempt.txt\n'
"#,
    )
    .unwrap();

    let failed = ao2([
        "run",
        workflow.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "repair-source",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        failed_prompt.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);
    assert!(failed.status.success(), "{}", stderr(&failed));
    assert!(stdout(&failed).contains("status=Rejected"));
    let source_evidence = repo.join(".ao2/runs/repair-source/evidence-pack/evidence-pack.json");
    let source_evidence_text = fs::read_to_string(&source_evidence).unwrap();
    assert!(source_evidence_text.contains("budget_exhausted"));

    let repair_prompt = temp.path().join("repair-prompt.sh");
    fs::write(
        &repair_prompt,
        r#"if printf '%s' "$AO2_REPAIR_RUN_HEALTH" | grep -q 'budget_exhausted' \
  && printf '%s' "$AO2_REPAIR_VERIFIER_OUTPUT" | grep -q 'docs/fixed.txt' \
  && test "$AO2_REPAIR_SOURCE_RUN_ID" = "repair-source"; then
  printf 'fixed\n' > docs/fixed.txt
else
  printf 'missing carried repair context\n' >&2
  exit 2
fi
printf 'Summary: repaired from rejected AO2 evidence context\n'
printf 'Changed files: docs/fixed.txt\n'
"#,
    )
    .unwrap();

    let repaired = ao2([
        "repair",
        "resume",
        "--evidence-pack",
        source_evidence.to_str().unwrap(),
        "--workflow",
        workflow.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "repair-resumed",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        repair_prompt.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
        "--json",
    ]);
    assert!(repaired.status.success(), "{}", stderr(&repaired));
    let repaired_json: serde_json::Value = serde_json::from_str(&stdout(&repaired)).unwrap();
    assert_eq!(repaired_json["schema_version"], "ao2.repair-resume.v1");
    assert_eq!(repaired_json["source_run_id"], "repair-source");
    assert_eq!(repaired_json["status"], "accepted");
    assert_eq!(
        fs::read_to_string(repo.join("docs/fixed.txt")).unwrap(),
        "fixed\n"
    );

    let repaired_evidence =
        fs::read_to_string(repo.join(".ao2/runs/repair-resumed/evidence-pack/evidence-pack.json"))
            .unwrap();
    assert!(repaired_evidence.contains("repair_source_context"));
    assert!(repaired_evidence.contains("\"source_run_id\": \"repair-source\""));
    assert!(repaired_evidence.contains("docs/fixed.txt"));
    assert!(repaired_evidence.contains("repair_source"));
    assert!(repaired_evidence.contains("provider_transcript_summary"));
}

#[test]
fn cli_release_phase1_decision_publish_signs_and_posts_to_control_plane() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": "a".repeat(64),
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": "phase1-promotion-checklist.json"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let expected_decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    let expected_decision_raw = serde_json::to_string_pretty(&expected_decision).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 decision publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request
            .contains("\"schema_version\":\"ao2.cp-phase1-promotion-decision-signed-upload.v1\""));
        assert!(request.contains("\"schema\":\"factory-v3/ao2-phase1-promotion-decision/v1\""));
        assert!(request.contains("\"decision\":\"promote_phase1_candidate\""));
        assert!(request.contains("\"signature_algorithm\":\"RSA/SHA-256\""));
        assert!(request.contains("\"signature_hex\""));
        assert!(request.contains("\"public_key_sha256\""));
        assert!(request.contains("\"public_key_pem\""));
        assert!(request.contains("\"signer_id\":\"release-lead\""));
        assert!(!request.contains("cp-token\""));
        let request_body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("signed phase1 decision request has body");
        let upload: serde_json::Value = serde_json::from_str(request_body).unwrap();
        let decision_b64 = upload["decision_b64"]
            .as_str()
            .expect("signed phase1 decision upload carries exact decision_b64 bytes");
        {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let decoded = BASE64_STANDARD.decode(decision_b64).unwrap();
            assert_eq!(decoded, expected_decision_raw.as_bytes());
        }
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decision123","stored_at":"2026-05-22T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "release",
        "phase1-decision-publish",
        "--decision",
        decision_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-control-plane-publish.v1"
    );
    assert_eq!(json["signed"], true);
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/decision/signed")
    );
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "factory-v3/ao2-phase1-promotion-decision/v1"
    );
    assert_eq!(
        json["signature"]["schema_version"],
        "ao2.cp-phase1-promotion-decision-signature.v1"
    );
}

#[test]
fn cli_release_phase1_decision_publish_posts_referenced_checklist_before_signed_decision() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let checklist_path = temp.path().join("phase1-promotion-checklist.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let checklist = serde_json::json!({
        "schema": "factory-v3/ao2-phase1-promotion-checklist/v1",
        "schema_version": "ao2.phase1-promotion-checklist.v1",
        "status": "passed",
        "phase1_state": "phase1_candidate_ready",
        "next_action": "publish signed Phase 1 promotion decision",
        "checklist": {
            "provider_readiness": {"status": "superseded_by_live_acceptance", "phase1_state": "passed"},
            "live_provider_acceptance": {"status": "passed", "state": "live_acceptance_complete"},
            "release_gate": {"status": "passed", "state": "verified"},
            "three_os_smoke": {"status": "passed", "state": "accepted"}
        }
    });
    fs::write(
        &checklist_path,
        serde_json::to_string_pretty(&checklist).unwrap(),
    )
    .unwrap();
    let checklist_sha = canonical_sha256_for_test(&checklist);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": checklist_sha,
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": checklist_path.file_name().unwrap().to_string_lossy()
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let checklist_sha_for_server = checklist_sha.clone();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let mut attempts = 0;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        attempts += 1;
                        assert!(
                            attempts <= 100,
                            "timed out waiting for Phase 1 publish request"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            };
            let mut buffer = [0_u8; 32768];
            stream.set_nonblocking(false).unwrap();
            let read = read_test_http_request(&mut stream, &mut buffer);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body = if request.starts_with("POST /api/v1/phase1/promotion/checklist HTTP/1.1") {
                format!(
                    r#"{{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"{checklist_sha_for_server}","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-checklist/v1"}}"#
                )
            } else {
                r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decision456","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#.to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            requests.push(request);
        }
        assert!(requests[0].starts_with("POST /api/v1/phase1/promotion/checklist HTTP/1.1"));
        assert!(requests[1].starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(requests[0].contains("Authorization: Bearer cp-token"));
        assert!(requests[1].contains("Authorization: Bearer cp-token"));
        assert!(requests[0].contains("\"schema\":\"factory-v3/ao2-phase1-promotion-checklist/v1\""));
        assert!(requests[1].contains("\"checklist_sha256\""));
        assert!(!requests.join("\n").contains("cp-token\""));
    });

    let publish = ao2([
        "release",
        "phase1-decision-publish",
        "--decision",
        decision_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(json["checklist_publish"]["status"], "posted");
    assert_eq!(
        json["checklist_publish"]["receipt"]["sha256"],
        checklist_sha
    );
    assert_eq!(json["receipt"]["sha256"], "decision456");
}

#[test]
fn cli_release_phase1_decision_build_binds_release_and_replacement_gates() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-gate.v1",
            "status": "verified",
            "release": {
                "provenance_verified": true,
                "archive_count": 4
            },
            "smoke": {
                "status": "verified"
            },
            "obligation_gates": {
                "status": "verified"
            },
            "obligation_gate_signing": {
                "status": "verified"
            },
            "replacement_smoke_gate": {
                "schema": "ao2.release-replacement-smoke-gate-verification.v1",
                "status": "verified",
                "gate_status": "accepted",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "reasons": []
            },
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": []
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the replacement run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-build.v1"
    );
    assert_eq!(json["status"], "written");
    assert_eq!(json["decision"]["decision"], "promote_phase1_candidate");
    assert_eq!(json["checklist"]["status"], "passed");
    assert_eq!(
        json["checklist"]["replacement_smoke_gate"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["checklist"]["trust_boundary"]["ao2_decision_owner"],
        "ao2-native-phase1-promotion-decision-builder"
    );
    assert!(decision_path.is_file());
    let decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    assert_eq!(
        decision["schema"],
        "factory-v3/ao2-phase1-promotion-decision/v1"
    );
    assert_eq!(decision["status"], "passed");
    assert_eq!(decision["phase1_state"], "phase1_candidate_ready");
    assert_eq!(
        decision["artifacts"]["replacement_smoke_gate"],
        replacement_gate_path.display().to_string()
    );
    assert_eq!(
        decision["trust_boundary"]["factory_v3_role"],
        "parity_oracle_only"
    );
}

#[test]
fn cli_release_phase1_decision_build_binds_three_os_governed_run_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the governed run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["three_os_governed_run"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["checklist"]["release_gate"]["governed_run_evidence_verification"]["status"],
        "verified"
    );
    assert!(json["checklist"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |check| check["id"] == "governed-run-evidence-accepted" && check["status"] == "passed"
        ));
    let decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    assert_eq!(
        decision["artifacts"]["governed_run_evidence"],
        serde_json::json!([
            governed_run_paths[0].display().to_string(),
            governed_run_paths[1].display().to_string(),
            governed_run_paths[2].display().to_string()
        ])
    );
    assert_eq!(
        decision["artifacts"]["factory_project_run_readback"],
        serde_json::json!([
            project_run_readbacks[0].display().to_string(),
            project_run_readbacks[1].display().to_string(),
            project_run_readbacks[2].display().to_string()
        ])
    );
}

#[test]
fn cli_release_phase1_decision_build_allows_governed_run_only_promotion() {
    let temp = tempfile::tempdir().unwrap();
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-gate.v1",
            "status": "verified",
            "release": {
                "provenance_verified": true,
                "archive_count": 4
            },
            "smoke": {
                "status": "verified"
            },
            "obligation_gates": {
                "status": "verified"
            },
            "obligation_gate_signing": {
                "status": "verified"
            },
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": []
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 governed-run evidence supersedes the legacy replacement-smoke gate.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["checklist"]["three_os_smoke"]["status"],
        "superseded_by_governed_run"
    );
    assert_eq!(
        json["checklist"]["three_os_governed_run"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["decision"]["artifacts"]["governed_run_evidence"],
        serde_json::json!([
            governed_run_paths[0].display().to_string(),
            governed_run_paths[1].display().to_string(),
            governed_run_paths[2].display().to_string()
        ])
    );
    assert!(json["decision"]["artifacts"]["replacement_smoke_gate"].is_null());
}

#[test]
fn cli_release_phase1_decision_build_rejects_missing_project_run_readback_hard_gate() {
    let temp = tempfile::tempdir().unwrap();
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 must not promote without replacement-packet readback proof.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("project-run readback"));
}

#[test]
fn cli_release_phase1_decision_build_rejects_missing_governed_run_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Missing governed run evidence should not promote.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("governed run evidence"));
}

#[test]
fn cli_release_phase1_decision_build_binds_three_provider_acceptance_preservation() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let provider_acceptance_path = temp.path().join("provider-acceptance-preservation.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &provider_acceptance_path,
        serde_json::to_string_pretty(&accepted_provider_acceptance_preservation_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--provider-acceptance-preservation",
        provider_acceptance_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the replacement run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["provider_acceptance_preservation"]["providers"],
        serde_json::json!(["codex", "claude", "antigravity"])
    );
    assert_eq!(
        json["decision"]["artifacts"]["provider_acceptance_preservation"],
        provider_acceptance_path.display().to_string()
    );
    assert!(json["checklist"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |check| check["id"] == "provider-acceptance-preservation-verified"
                && check["status"] == "passed"
        ));
}

#[test]
fn cli_release_phase1_decision_build_rejects_incomplete_provider_acceptance_preservation() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let provider_acceptance_path = temp.path().join("provider-acceptance-preservation.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();
    let mut provider_acceptance = accepted_provider_acceptance_preservation_fixture();
    provider_acceptance["providers"]
        .as_object_mut()
        .unwrap()
        .remove("antigravity");
    fs::write(
        &provider_acceptance_path,
        serde_json::to_string_pretty(&provider_acceptance).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--provider-acceptance-preservation",
        provider_acceptance_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Provider acceptance must be complete.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("provider acceptance preservation missing antigravity"));
}

#[test]
fn cli_release_phase1_decision_build_rejects_unverified_replacement_gate() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    let mut replacement_gate = accepted_replacement_smoke_gate_fixture();
    replacement_gate["status"] = serde_json::json!("rejected");
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&replacement_gate).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-gate.v1",
            "status": "verified",
            "replacement_smoke_gate": {
                "schema": "ao2.release-replacement-smoke-gate-verification.v1",
                "status": "failed",
                "gate_status": "rejected",
                "accepted_os": ["macos", "ubuntu"],
                "reasons": [{"code": "replacement_smoke_gate_missing_os"}]
            },
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": [{"code": "replacement_smoke_gate_failed"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Bad gate should not promote.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("replacement smoke gate must be accepted"));
}

fn verified_release_gate_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema": "ao2.release-gate.v1",
        "status": "verified",
        "release": {
            "provenance_verified": true,
            "archive_count": 4
        },
        "smoke": {
            "status": "verified"
        },
        "obligation_gates": {
            "status": "verified"
        },
        "obligation_gate_signing": {
            "status": "verified"
        },
        "replacement_smoke_gate": {
            "schema": "ao2.release-replacement-smoke-gate-verification.v1",
            "status": "verified",
            "gate_status": "accepted",
            "accepted_os": ["macos", "ubuntu", "windows"],
            "reasons": []
        },
        "reasons": []
    })
}

fn verified_release_gate_with_governed_run_fixture() -> serde_json::Value {
    let mut release_gate = verified_release_gate_fixture();
    release_gate["governed_run_evidence"] = serde_json::json!({
        "schema": "ao2.release-governed-run-evidence-verification.v1",
        "status": "verified",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "reasons": []
    });
    release_gate["factory_project_run_readback"] = serde_json::json!({
        "schema": "ao2.release-factory-project-run-readback-verification.v1",
        "status": "verified",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "reasons": []
    });
    release_gate
}

fn accepted_governed_run_fixture(run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-governed-run.v1",
        "status": "accepted",
        "run_id": run_id,
        "plan": {
            "ao2_native_plan": {
                "role_contract_discovery": {
                    "mode": "auto_discovered_from_ao_runspec_layout",
                    "loaded_count": 7
                }
            }
        },
        "run_result_verification": {
            "status": "accepted"
        },
        "pack_evidence": {
            "status": "produced",
            "signature": {
                "signature_verified": true
            }
        },
        "evaluator_decision": {
            "verdict": "accepted",
            "signature": {
                "signature_verified": true
            }
        },
        "evaluator_decision_verification": {
            "status": "accepted",
            "signature_verified": true
        },
        "governed_run_checklist": {
            "ao2_planned_factory_compat_workflow": true,
            "ao2_queue_executed_factory_compat_workflow": true,
            "ao2_verified_primary_run_result": true,
            "ao2_packed_primary_evidence": true,
            "ao2_signed_evaluator_closure": true,
            "ao2_auto_loaded_role_contracts": true,
            "factory_v3_drives_workflow": false
        },
        "artifacts": {
            "governed_run": format!("target/{run_id}/governed-run.json"),
            "run_result_verification": format!("target/{run_id}/run-result-verification.json"),
            "evidence_pack": format!("target/{run_id}/evidence-pack.json"),
            "evaluator_decision": format!("target/{run_id}/evaluator-decision.json")
        },
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-governed-run",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    })
}

fn write_phase1_governed_run_evidence(root: &Path) -> Vec<PathBuf> {
    ["macos", "ubuntu", "windows"]
        .into_iter()
        .map(|os_label| {
            let dir = root.join("governed-run-evidence").join(os_label);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("governed-run.json");
            fs::write(
                &path,
                serde_json::to_string_pretty(&accepted_governed_run_fixture(&format!(
                    "real-factory-runspec-{os_label}"
                )))
                .unwrap(),
            )
            .unwrap();
            path
        })
        .collect()
}

fn accepted_factory_project_run_readback_fixture(os_label: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-project-run-smoke.v1",
        "status": "passed",
        "host_os": os_label,
        "run_id": format!("factory-project-run-{os_label}"),
        "factory_project_schema": "ao2.factory-project-run.v1",
        "queued_auto_replacement_packet": format!("target/{os_label}/queued/factory-replacement-packet.json"),
        "queued_auto_replacement_packet_archive": format!("target/{os_label}/queued/factory-replacement-packet.tgz"),
        "queued_auto_replacement_packet_status": "packaged",
        "queued_auto_replacement_packet_verification": format!("target/{os_label}/queued/factory-replacement-packet-verification.json"),
        "queued_auto_replacement_packet_verification_status": "accepted",
        "queued_auto_replacement_packet_verification_checksums_verified": true,
        "queued_auto_replacement_packet_verification_trust_boundary_verified": true,
        "queued_replacement_packet": format!("target/{os_label}/factory-replacement-packet.json"),
        "queued_replacement_packet_archive": format!("target/{os_label}/factory-replacement-packet.tgz"),
        "queued_replacement_packet_schema": "ao2.factory-replacement-packet.v1",
        "queued_replacement_packet_status": "packaged",
        "queued_replacement_packet_sha256": "a".repeat(64),
        "queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver": true,
        "queued_replacement_packet_factory_v3_role": "evaluator_closer_and_sampling_auditor",
        "queued_replacement_packet_verification": format!("target/{os_label}/factory-replacement-packet-verification.json"),
        "queued_replacement_packet_verification_schema": "ao2.factory-replacement-packet-verification.v1",
        "queued_replacement_packet_verification_status": "accepted",
        "queued_replacement_packet_verification_checksums_verified": true,
        "queued_replacement_packet_verification_trust_boundary_verified": true,
        "queued_replacement_packet_verification_ao2_replacement_driver_verified": true,
        "queued_replacement_packet_verification_factory_v3_evaluator_closer_verified": true
    })
}

fn write_factory_project_run_readbacks(root: &Path) -> Vec<PathBuf> {
    ["macos", "ubuntu", "windows"]
        .into_iter()
        .map(|os_label| {
            let dir = root.join("factory-project-run-readback").join(os_label);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("factory-project-run-summary.json");
            fs::write(
                &path,
                serde_json::to_string_pretty(&accepted_factory_project_run_readback_fixture(
                    os_label,
                ))
                .unwrap(),
            )
            .unwrap();
            path
        })
        .collect()
}

fn accepted_provider_acceptance_preservation_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema": "ao2.provider-pilot-acceptance-preservation.v1",
        "status": "passed",
        "tag": "v0.4.80",
        "providers": {
            "codex": {
                "schema_version": "ao2.codex-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-codex-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/codex/provider-pilot-acceptance.json"
            },
            "claude": {
                "schema_version": "ao2.claude-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-claude-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/claude/provider-pilot-acceptance.json"
            },
            "antigravity": {
                "schema_version": "ao2.antigravity-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-antigravity-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/antigravity/provider-pilot-acceptance.json"
            }
        }
    })
}

fn accepted_replacement_smoke_gate_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1",
        "status": "accepted",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-three-os-replacement-smoke-gate",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "three_os_contract": {
            "path_separator_safe_artifacts": true,
            "requires_native_windows_smoke": true,
            "requires_ubuntu_smoke": true,
            "requires_macos_smoke": true,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    })
}

#[test]
fn cli_release_phase1_decision_publish_reads_api_token_from_env_without_printing_secret() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": "b".repeat(64),
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": "phase1-promotion-checklist.json"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 decision publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-phase1-token"));
        assert!(request.contains("\"decision\":\"promote_phase1_candidate\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decisionenv123","stored_at":"2026-05-22T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "release",
            "phase1-decision-publish",
            "--decision",
            decision_path.to_str().unwrap(),
            "--signing-key",
            signing_key.to_str().unwrap(),
            "--signer-id",
            "release-lead",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_PHASE1_CP_TOKEN",
            "--json",
        ],
        [("AO2_TEST_PHASE1_CP_TOKEN", "env-phase1-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    let stderr = stderr(&publish);
    assert!(!stdout.contains("env-phase1-token"));
    assert!(!stderr.contains("env-phase1-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-control-plane-publish.v1"
    );
    assert_eq!(json["receipt"]["sha256"], "decisionenv123");
}

#[test]
fn cli_release_phase1_three_os_smoke_build_materializes_control_plane_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let smoke_root = temp.path().join("three-os-smoke");
    fs::create_dir_all(&smoke_root).unwrap();
    let local_log = smoke_root.join("local-smoke.log");
    let windows_log = smoke_root.join("windows-smoke.log");
    let report = smoke_root.join("report.md");
    fs::write(&local_log, "local smoke passed\n").unwrap();
    fs::write(&windows_log, "windows native smoke passed\n").unwrap();
    fs::write(&report, "# report\n").unwrap();

    let summary_path = smoke_root.join("summary.enriched.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "root": smoke_root.display().to_string(),
            "report": report.display().to_string(),
            "local_smoke": "passed",
            "linux_x86_64_remote_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "passed",
            "windows_log": windows_log.display().to_string()
        }))
        .unwrap(),
    )
    .unwrap();
    let provenance_path = temp.path().join("ao2-release-provenance.json");
    fs::write(
        &provenance_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-provenance.v1",
            "version": "0.4.80",
            "git_commit": "addb602d07e413ca5b565d8ebca986925a97017f",
            "git_dirty": false,
            "release_tag": "v0.4.80"
        }))
        .unwrap(),
    )
    .unwrap();
    let out = temp.path().join("phase1-three-os-release-smoke.json");

    let build = ao2([
        "release",
        "phase1-three-os-smoke-build",
        "--summary",
        summary_path.to_str().unwrap(),
        "--provenance",
        provenance_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(json["schema_version"], "ao2.phase1-three-os-smoke-build.v1");
    assert_eq!(json["status"], "written");
    assert!(out.is_file());

    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(
        bundle["schema"],
        "ao2-control-plane.three-os-release-smoke.v1"
    );
    assert_eq!(bundle["status"], "passed");
    assert_eq!(bundle["version"], "0.4.80");
    assert_eq!(bundle["release_candidate_version"], "0.4.80");
    assert_eq!(
        bundle["source_commit"],
        "addb602d07e413ca5b565d8ebca986925a97017f"
    );
    assert_eq!(bundle["source_dirty"], false);
    assert_eq!(bundle["targets"]["macos"]["status"], "passed");
    assert_eq!(bundle["targets"]["ubuntu"]["status"], "passed");
    assert_eq!(bundle["targets"]["windows"]["status"], "passed");
    assert_eq!(
        bundle["targets"]["windows"]["log"],
        windows_log.display().to_string()
    );
    assert!(bundle["rerun_commands"]["all_required"]
        .as_str()
        .unwrap()
        .contains("<local-token>"));
}

#[test]
fn cli_release_phase1_three_os_smoke_publish_posts_bundle_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let smoke_path = temp.path().join("phase1-three-os-release-smoke.json");
    fs::write(
        &smoke_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2-control-plane.three-os-release-smoke.v1",
            "version": "0.4.80",
            "status": "passed",
            "release_candidate_version": "0.4.80",
            "source_commit": "addb602d07e413ca5b565d8ebca986925a97017f",
            "source_dirty": false,
            "targets": {
                "macos": {"status": "passed", "log": "target/three-os-smoke/run/local-smoke.log"},
                "ubuntu": {"status": "passed", "log": "target/three-os-smoke/run/local-smoke.log"},
                "windows": {"status": "passed", "log": "target/three-os-smoke/run/windows-smoke.log"}
            },
            "rerun_commands": [
                "AO2_PHASE1_CP_TOKEN=<local-token> target/release/ao2 release phase1-three-os-smoke-publish"
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream =
            accept_test_connection(&listener, "Phase 1 three-OS smoke publish request");
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/three-os-smoke HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema\":\"ao2-control-plane.three-os-release-smoke.v1\""));
        assert!(request.contains("\"status\":\"passed\""));
        assert!(request.contains("\"source_dirty\":false"));
        assert!(!request.contains("cp-token\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"threeos123","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"ao2-control-plane.three-os-release-smoke.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "release",
        "phase1-three-os-smoke-publish",
        "--smoke",
        smoke_path.to_str().unwrap(),
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    assert!(!stdout.contains("cp-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-three-os-smoke-control-plane-publish.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/three-os-smoke")
    );
    assert_eq!(json["receipt"]["sha256"], "threeos123");
}

#[test]
fn cli_release_phase1_promotion_inputs_publish_posts_verification_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let verification_path = temp.path().join("promotion-inputs-verification.json");
    fs::write(
        &verification_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.phase1-replacement-promotion-inputs-verification.v1",
            "status": "accepted",
            "mode": "decision_gate",
            "manifest_path": "/work/ao2/target/phase1-replacement-promotion/promotion-inputs.json",
            "missing_required_inputs": [],
            "failure_count": 0,
            "failures": [],
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream =
            accept_test_connection(&listener, "Phase 1 promotion inputs publish request");
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/inputs-verification HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-cp-token"));
        assert!(request.contains(
            "\"schema_version\":\"ao2.phase1-replacement-promotion-inputs-verification.v1\""
        ));
        assert!(request.contains("\"status\":\"accepted\""));
        assert!(request.contains("\"control_plane_approves_release\":false"));
        assert!(!request.contains("env-cp-token\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"inputs123","stored_at":"2026-05-29T00:00:00Z","ingested_schema_version":"ao2.phase1-replacement-promotion-inputs-verification.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "release",
            "phase1-promotion-inputs-publish",
            "--verification",
            verification_path.to_str().unwrap(),
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_PHASE1_CP_TOKEN",
            "--json",
        ],
        [("AO2_PHASE1_CP_TOKEN", "env-cp-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    assert!(!stdout.contains("env-cp-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-inputs-control-plane-publish.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/inputs-verification")
    );
    assert_eq!(json["receipt"]["sha256"], "inputs123");
}

#[test]
fn cli_release_phase1_history_fetch_reads_control_plane_history_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("phase1-history.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 history fetch request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/phase1/promotion/history.json HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        let body = r#"{"schema_version":"ao2.cp-phase1-promotion-history.v1","counts":{"checklists":1,"signed_decisions":1,"three_os_smokes":1},"history":{"checklists":[],"signed_decisions":[],"three_os_smokes":[]},"trust_boundary":{"role":"read_only_observer","mutates_ao_artifacts":false,"release_acceptance_owner":"factory-v3 evaluator-closer"}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let fetch = ao2([
        "release",
        "phase1-history-fetch",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    server.join().unwrap();
    assert!(fetch.status.success(), "{}", stderr(&fetch));
    let json: serde_json::Value = serde_json::from_str(&stdout(&fetch)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-history-control-plane-fetch.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/history.json")
    );
    assert_eq!(json["history"]["counts"]["checklists"], 1);
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(out.is_file());
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap();
    assert_eq!(
        written["schema_version"],
        "ao2.cp-phase1-promotion-history.v1"
    );
    assert!(!stdout(&fetch).contains("cp-token"));
}

#[test]
fn cli_release_phase1_history_fetch_accepts_api_token_env_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("phase1-history-env.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 history env-token fetch request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/phase1/promotion/history.json HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-cp-token"));
        let body = r#"{"schema_version":"ao2.cp-phase1-promotion-history.v1","counts":{"checklists":1,"signed_decisions":1,"three_os_smokes":1},"history":{"checklists":[],"signed_decisions":[],"three_os_smokes":[]},"trust_boundary":{"role":"read_only_observer","mutates_ao_artifacts":false,"release_acceptance_owner":"factory-v3 evaluator-closer"}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let fetch = ao2_with_env(
        [
            "release",
            "phase1-history-fetch",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_CP_TOKEN",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        [("AO2_TEST_CP_TOKEN", "env-cp-token")],
    );
    server.join().unwrap();
    assert!(fetch.status.success(), "{}", stderr(&fetch));
    let json: serde_json::Value = serde_json::from_str(&stdout(&fetch)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-history-control-plane-fetch.v1"
    );
    assert_eq!(json["history"]["counts"]["three_os_smokes"], 1);
    assert!(out.is_file());
    assert!(!stdout(&fetch).contains("env-cp-token"));
}

fn sha256_hex_for_test(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn canonical_sha256_for_test(value: &serde_json::Value) -> String {
    fn write_value(out: &mut String, value: &serde_json::Value) {
        match value {
            serde_json::Value::Null => out.push_str("null"),
            serde_json::Value::Bool(value) => {
                out.push_str(if *value { "true" } else { "false" });
            }
            serde_json::Value::Number(value) => out.push_str(&value.to_string()),
            serde_json::Value::String(value) => write_string(out, value),
            serde_json::Value::Array(values) => {
                out.push('[');
                for (index, item) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_value(out, item);
                }
                out.push(']');
            }
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(out, key);
                    out.push(':');
                    write_value(out, &map[*key]);
                }
                out.push('}');
            }
        }
    }
    fn write_string(out: &mut String, value: &str) {
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{08}' => out.push_str("\\b"),
                '\u{0c}' => out.push_str("\\f"),
                ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => out.push(ch),
            }
        }
        out.push('"');
    }
    let mut canonical = String::new();
    write_value(&mut canonical, value);
    sha256_hex_for_test(canonical.as_bytes())
}

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
}

fn ao2<const N: usize>(args: [&str; N]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env(
        "AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER",
        "human:test-auto-approve",
    );
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn generate_native_signing_key(path: &Path, bits: usize) {
    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["workbench", "support-keygen", "--out"])
        .arg(path)
        .args(["--bits", &bits.to_string()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        path.is_file(),
        "native signing key exists: {}",
        path.display()
    );
}

fn ao2_with_env<const N: usize, const M: usize>(
    args: [&str; N],
    env: [(&str, &str); M],
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.envs(env);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env(
        "AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER",
        "human:test-auto-approve",
    );
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn accept_test_connection(listener: &TcpListener, label: &str) -> TcpStream {
    let mut attempts = 0;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                attempts += 1;
                assert!(attempts <= 300, "timed out waiting for {label}");
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}
