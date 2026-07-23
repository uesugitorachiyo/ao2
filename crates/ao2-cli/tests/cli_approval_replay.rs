use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

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
