use std::fs;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

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

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_factory_greenfield_spec_ingest_emits_read_only_preflight_packet() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let spec = temp.path().join("missed-call-recovery.md");
    fs::write(
        &spec,
        "# Missed Call Recovery\n\nBuild a small missed-call recovery app.\n\nAcceptance:\n- Captures missed-call leads.\n- Sends owner notification.\n- Shows recovery status.\n",
    )
    .unwrap();

    let before_ao2_exists = repo.join(".ao2").exists();
    let result = ao2([
        "factory",
        "greenfield-spec-ingest",
        "--spec",
        spec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery",
        "--verifier-command",
        "npm run verify",
        "--json",
    ]);
    assert!(result.status.success(), "{}", stderr(&result));
    let json: serde_json::Value = serde_json::from_str(&stdout(&result)).unwrap();

    assert_eq!(
        json["schema_version"],
        "ao2.factory-greenfield-spec-ingest.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["run_id"], "missed-call-recovery");
    assert_eq!(json["source_spec"]["path"], spec.display().to_string());
    assert_eq!(
        json["source_spec"]["sha256"],
        sha256_path(&spec),
        "preflight must bind to exact spec bytes"
    );
    assert_eq!(json["classification"]["shape"], "greenfield");
    assert_eq!(json["classification"]["owner"], "ao2-native-classifier");
    assert_eq!(
        json["classification"]["factory_v3_required_before_classification"],
        false
    );
    assert_eq!(json["preflight"]["read_only"], true);
    assert_eq!(json["preflight"]["queue_submission_ready"], true);
    assert_eq!(
        json["preflight"]["missing_required_inputs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        json["planned_ao2_producer_commands"][0]["command"],
        "ao2 factory project-plan"
    );
    assert_eq!(
        json["planned_ao2_producer_commands"][1]["command"],
        "ao2 factory project-start"
    );
    assert_eq!(
        json["expected_artifact_schemas"][0],
        "ao2.factory-project-plan.v1"
    );
    assert_eq!(
        json["expected_artifact_schemas"][1],
        "ao2.factory-acceptance-rubric.v1"
    );
    assert_eq!(json["side_effects"]["would_write_files"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(json["trust_boundary"]["factory_v3_drives_workflow"], false);
    assert_eq!(repo.join(".ao2").exists(), before_ao2_exists);
}

#[test]
fn cli_factory_greenfield_spec_ingest_submit_requires_exact_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let spec = temp.path().join("missed-call-recovery.md");
    fs::write(
        &spec,
        "# Missed Call Recovery\n\nAcceptance:\n- Captures missed-call leads.\n- Sends owner notification.\n- Shows recovery status.\n",
    )
    .unwrap();
    let queue_path = repo.join(".ao2/factory-compat/queue.json");

    let approval = ao2([
        "factory",
        "greenfield-spec-ingest-submit",
        "--spec",
        spec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery",
        "--verifier-command",
        "npm run verify",
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    assert_eq!(
        approval_json["schema_version"],
        "ao2.factory-greenfield-spec-ingest-submit-approval.v1"
    );
    assert_eq!(approval_json["status"], "approval_required");
    assert_eq!(approval_json["approval_mode"], "exact_action_digest");
    assert_eq!(approval_json["required_flag"], "--approve-action-digest");
    assert_eq!(approval_json["action_digest"].as_str().unwrap().len(), 64);
    assert_eq!(approval_json["preflight"]["preflight"]["read_only"], true);
    assert_eq!(
        approval_json["side_effects"]["would_write_queue_file_after_approval"],
        true
    );
    assert_eq!(
        approval_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert!(!queue_path.exists(), "missing digest must not submit queue");

    let digest = approval_json["action_digest"].as_str().unwrap();
    let submit = ao2([
        "factory",
        "greenfield-spec-ingest-submit",
        "--spec",
        spec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery",
        "--verifier-command",
        "npm run verify",
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    let submit_json: serde_json::Value = serde_json::from_str(&stdout(&submit)).unwrap();
    assert_eq!(
        submit_json["schema_version"],
        "ao2.factory-greenfield-spec-ingest-submit.v1"
    );
    assert_eq!(submit_json["status"], "queued");
    assert_eq!(submit_json["run_id"], "missed-call-recovery");
    assert_eq!(
        submit_json["approval"]["status"],
        "approved_exact_action_digest"
    );
    assert_eq!(submit_json["approval"]["action_digest"], digest);
    assert_eq!(
        submit_json["queue_submit"]["schema_version"],
        "ao2.factory-project-start-workbench-queue-submit.v1"
    );
    assert_eq!(
        submit_json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        submit_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(submit_json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(submit_json["side_effects"]["executed_provider"], false);
    assert_eq!(submit_json["side_effects"]["executed_queue"], false);
    assert_eq!(submit_json["side_effects"]["mutated_control_plane"], false);
    assert!(queue_path.exists(), "approved digest should submit queue");
    let queue: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(queue_path).unwrap()).unwrap();
    assert_eq!(queue["entries"].as_array().unwrap().len(), 1);
    assert_eq!(queue["entries"][0]["run_id"], "missed-call-recovery");
    assert_eq!(queue["entries"][0]["status"], "queued");
}
