use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
