use std::fs;
use std::path::Path;
use std::process::Command;

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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_factory_queue_persists_history_cancel_and_retry_state_without_factory_driver() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 workbench queue parity
objective: Persist factory-v3-compatible governed run queue state across restart boundaries.
acceptance:
  - AO2 owns queue history, cancel, and retry state without factory-v3 driving execution.
"#,
    )
    .unwrap();
    let out = temp.path().join("queue-plan.json");
    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));

    let submit_receipt = temp.path().join("queue-submit.json");
    let submit = ao2([
        "factory",
        "queue-submit",
        "--plan",
        out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "factory-queue-restart-safe",
        "--out",
        submit_receipt.to_str().unwrap(),
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    let submitted: serde_json::Value = serde_json::from_str(&stdout(&submit)).unwrap();
    assert_eq!(
        submitted["schema_version"],
        "ao2.factory-v3-compat-workbench-queue-submit.v1"
    );
    assert_eq!(submitted["status"], "queued");
    assert_eq!(
        submitted["entry"]["execution_contract"]["execution_owner"],
        "ao2"
    );
    assert_eq!(
        submitted["entry"]["execution_contract"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        submitted["entry"]["parity_checklist_progress"]
            ["ao2_persists_queue_history_cancel_retry_state"],
        true
    );
    assert!(Path::new(submitted["queue_path"].as_str().unwrap()).is_file());
    assert!(submit_receipt.is_file());

    let cancel = ao2([
        "factory",
        "queue-cancel",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "factory-queue-restart-safe",
        "--reason",
        "operator paused before provider execution",
        "--json",
    ]);
    assert!(cancel.status.success(), "{}", stderr(&cancel));
    let cancelled: serde_json::Value = serde_json::from_str(&stdout(&cancel)).unwrap();
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["entry"]["attempts"], 0);
    assert_eq!(
        cancelled["continuity_contract"]["cancel_retry_state_owner"],
        "ao2-workbench-queue"
    );

    let retry = ao2([
        "factory",
        "queue-retry",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "factory-queue-restart-safe",
        "--reason",
        "resume after restart",
        "--json",
    ]);
    assert!(retry.status.success(), "{}", stderr(&retry));
    let retried: serde_json::Value = serde_json::from_str(&stdout(&retry)).unwrap();
    assert_eq!(retried["status"], "queued");
    assert_eq!(retried["entry"]["attempts"], 1);
    assert_eq!(
        retried["entry"]["transition_history"]
            .as_array()
            .unwrap()
            .len(),
        3
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
    assert_eq!(
        listed["schema_version"],
        "ao2.factory-v3-compat-workbench-queue-list.v1"
    );
    assert_eq!(listed["entry_count"], 1);
    assert_eq!(listed["entries"][0]["run_id"], "factory-queue-restart-safe");
    assert_eq!(listed["entries"][0]["status"], "queued");
    assert_eq!(listed["entries"][0]["attempts"], 1);
    assert_eq!(
        listed["continuity_contract"]["survives_server_restart"],
        true
    );
    assert_eq!(
        listed["continuity_contract"]["factory_v3_drives_workflow"],
        false
    );
}

#[test]
fn cli_factory_queue_run_next_executes_persisted_plan_and_records_evidence_refs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 queued governed execution
objective: Execute a queued factory-v3-compatible governed run directly through AO2.
acceptance:
  - AO2 claims the queued run, executes it, and stores evidence references without factory-v3 driving execution.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: queued-parity
verifier: python -m pytest -q
",
    )
    .unwrap();
    let plan_out = temp.path().join("queue-run-next-plan.json");
    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));

    let submit = ao2([
        "factory",
        "queue-submit",
        "--plan",
        plan_out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-native-execution",
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));

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
        "ao2.factory-v3-compat-workbench-queue-run-next.v1"
    );
    assert_eq!(result["run_id"], "queued-native-execution");
    assert_eq!(result["status"], "accepted");
    assert_eq!(result["entry"]["status"], "accepted");
    assert_eq!(result["entry"]["native_evaluator_verdict"], "accepted");
    assert_eq!(
        result["parity_checklist_progress"]["ao2_queue_can_execute_persisted_factory_compat_run"],
        true
    );
    assert_eq!(
        result["parity_checklist_progress"]["factory_v3_drives_workflow"],
        false
    );
    assert!(Path::new(result["entry"]["evidence_pack"].as_str().unwrap()).is_file());
    assert!(Path::new(result["entry"]["run_result_path"].as_str().unwrap()).is_file());
    assert_eq!(
        result["entry"]["transition_history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|transition| transition["status"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["queued", "running", "accepted"]
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
    assert_eq!(listed["entries"][0]["status"], "accepted");
    assert_eq!(
        listed["entries"][0]["evidence_pack"],
        result["entry"]["evidence_pack"]
    );
}

#[test]
fn cli_factory_queue_run_next_executes_provider_backed_persisted_plan() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 provider-backed queued governed execution
objective: Execute a queued factory-v3-compatible governed run through AO2 with the provider adapter contract.
acceptance:
  - AO2 claims the queued run, executes it through the requested provider, and stores evidence references without factory-v3 driving execution.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: queued-provider-parity
verifier: python -m pytest -q
",
    )
    .unwrap();
    let prompt_path = temp.path().join("provider-prompt.sh");
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
printf 'Summary: provider-backed queue-run-next fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 11\n'
"#,
    )
    .unwrap();
    let plan_out = temp.path().join("queue-run-next-provider-plan.json");
    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));

    let submit = ao2([
        "factory",
        "queue-submit",
        "--plan",
        plan_out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-provider-native-execution",
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));
    let result: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    assert_eq!(result["status"], "accepted");
    assert_eq!(
        result["run_result"]["provider_adapter_contract"]["status"],
        "observed"
    );
    assert_eq!(
        result["run_result"]["provider_adapter_contract"]["fulfilled"],
        true
    );
    assert_eq!(
        result["run_result"]["provider_execution"]["provider"],
        "scripted"
    );
    assert_eq!(
        result["entry"]["provider_execution"]["provider"],
        "scripted"
    );
    assert_eq!(
        result["entry"]["provider_execution"]["mode"],
        "provider-backed"
    );
    assert_eq!(
        result["run_result"]["parity_checklist_progress"]["factory_v3_drives_workflow"],
        false
    );
    let evidence = fs::read_to_string(result["entry"]["evidence_pack"].as_str().unwrap()).unwrap();
    assert!(evidence.contains("provider_prompt_transcript"));
    assert!(evidence.contains("provider-backed queue-run-next fixed discount validation"));
}

#[test]
fn cli_factory_queue_run_next_rejects_tampered_queued_plan_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: Tampered queued plan
",
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: tamper-queue
verifier: python -m pytest -q
",
    )
    .unwrap();
    let plan_out = temp.path().join("tamper-plan.json");
    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));

    let submit = ao2([
        "factory",
        "queue-submit",
        "--plan",
        plan_out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "tamper-digest-run",
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    fs::write(&plan_out, "{\"schema_version\":\"tampered\"}\n").unwrap();

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !run_next.status.success(),
        "tampered queue unexpectedly executed"
    );
    assert!(stderr(&run_next).contains("plan digest mismatch"));

    let list = ao2([
        "factory",
        "queue-list",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(list.status.success(), "{}", stderr(&list));
    let listed: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(listed["entries"][0]["status"], "blocked");
    assert_eq!(
        listed["entries"][0]["transition_history"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["reason"],
        "AO2 queue-run-next refused queued plan because persisted plan digest changed before execution"
    );
}

#[cfg(unix)]
#[test]
fn cli_factory_queue_run_next_blocks_unreadable_queued_plan_before_claiming() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(&request, "title: Unreadable queued plan\n").unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: unreadable-queue\nverifier: python -m pytest -q\n",
    )
    .unwrap();
    let plan_out = temp.path().join("unreadable-plan.json");
    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));

    let submit = ao2([
        "factory",
        "queue-submit",
        "--plan",
        plan_out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "unreadable-plan-run",
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    fs::set_permissions(&plan_out, fs::Permissions::from_mode(0o000)).unwrap();

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    fs::set_permissions(&plan_out, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        !run_next.status.success(),
        "unreadable queue unexpectedly executed"
    );
    assert!(stderr(&run_next).contains("plan path is not readable"));

    let list = ao2([
        "factory",
        "queue-list",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(list.status.success(), "{}", stderr(&list));
    let listed: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(listed["entries"][0]["status"], "blocked");
    assert_eq!(
        listed["entries"][0]["transition_history"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["reason"],
        "AO2 queue-run-next refused unreadable queued plan path before execution"
    );
}
