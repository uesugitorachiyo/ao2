use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn cli_can_pause_approve_resume_and_replay() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "cli-run",
        "--pause-for-approval",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let run_stdout = stdout(&run);
    assert!(run_stdout.contains("status=WaitingForApproval"));
    assert!(run_stdout.contains("approval_required=true"));
    assert!(run_stdout.contains("required_digest_field=action_digest"));
    assert!(run_stdout.contains("action_digest="));
    assert!(run_stdout.contains("replay_state=waiting_for_approval"));
    assert!(run_stdout.contains("evidence_dir="));
    assert!(run_stdout.contains("next_step=ao2 approve "));
    let ticket_id = value_for(&run_stdout, "approval_ticket_id=");

    let approve = ao2([
        "approve",
        ticket_id,
        "--target",
        repo.to_str().unwrap(),
        "--approver",
        "human:cli-test",
    ]);
    assert!(approve.status.success(), "{}", stderr(&approve));
    assert!(stdout(&approve).contains("status=approved"));

    let resume = ao2([
        "run",
        "--resume",
        "cli-run",
        "--target",
        repo.to_str().unwrap(),
    ]);
    assert!(resume.status.success(), "{}", stderr(&resume));
    let resume_stdout = stdout(&resume);
    assert!(resume_stdout.contains("status=Accepted"));
    assert!(resume_stdout.contains("run_record="));
    assert!(resume_stdout.contains("replay_state=accepted"));
    assert!(resume_stdout.contains("evidence_dir="));

    let replay = ao2(["replay", "cli-run", "--target", repo.to_str().unwrap()]);
    assert!(replay.status.success(), "{}", stderr(&replay));
    let replay_json: serde_json::Value = serde_json::from_str(&stdout(&replay)).unwrap();
    assert_eq!(replay_json["status"], "accepted");
    assert_eq!(replay_json["digest_failures"].as_array().unwrap().len(), 0);
}

#[test]
fn cli_reports_recovery_context_for_unapproved_resume_and_tampered_approval() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "recovery-fixture-run",
        "--pause-for-approval",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let run_stdout = stdout(&run);
    let ticket_id = value_for(&run_stdout, "approval_ticket_id=").to_string();
    let action_digest = value_for(&run_stdout, "action_digest=").to_string();

    let blocked_resume = ao2([
        "run",
        "--resume",
        "recovery-fixture-run",
        "--target",
        repo.to_str().unwrap(),
    ]);
    assert!(!blocked_resume.status.success());
    let blocked_stderr = stderr(&blocked_resume);
    assert!(blocked_stderr.contains("approval_status=pending"));
    assert!(blocked_stderr.contains("required_digest_field=action_digest"));
    assert!(blocked_stderr.contains(&format!("action_digest={action_digest}")));
    assert!(blocked_stderr.contains("replay_state=waiting_for_approval"));
    assert!(blocked_stderr.contains("evidence_dir="));
    assert!(blocked_stderr.contains("next_step=ao2 approve "));
    assert!(!blocked_stderr.contains("provider"));

    let approval_path = repo.join(format!(
        ".ao2/runs/recovery-fixture-run/approvals/{ticket_id}.json"
    ));
    let mut stored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&approval_path).unwrap()).unwrap();
    stored["request"]["args"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("--force"));
    fs::write(
        &approval_path,
        serde_json::to_string_pretty(&stored).unwrap(),
    )
    .unwrap();

    let bad_approval = ao2([
        "approve",
        &ticket_id,
        "--target",
        repo.to_str().unwrap(),
        "--approver",
        "human:cli-test",
    ]);
    assert!(!bad_approval.status.success());
    let bad_approval_stderr = stderr(&bad_approval);
    assert!(bad_approval_stderr.contains("approval_status=rejected"));
    assert!(bad_approval_stderr.contains("required_digest_field=action_digest"));
    assert!(bad_approval_stderr.contains(&format!("action_digest={action_digest}")));
    assert!(bad_approval_stderr.contains("digest_failure=approval digest mismatch"));
    assert!(bad_approval_stderr.contains("evidence_dir="));
    assert!(bad_approval_stderr.contains("recovery=preserve the failing state"));
    assert!(!bad_approval_stderr.contains("provider"));
}

#[test]
fn cli_reports_repeated_resume_as_accepted_replay_state() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "repeated-resume-run",
        "--pause-for-approval",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let run_stdout = stdout(&run);
    let ticket_id = value_for(&run_stdout, "approval_ticket_id=");

    let approve = ao2([
        "approve",
        ticket_id,
        "--target",
        repo.to_str().unwrap(),
        "--approver",
        "human:cli-test",
    ]);
    assert!(approve.status.success(), "{}", stderr(&approve));

    let first_resume = ao2([
        "run",
        "--resume",
        "repeated-resume-run",
        "--target",
        repo.to_str().unwrap(),
    ]);
    assert!(first_resume.status.success(), "{}", stderr(&first_resume));

    let repeated_resume = ao2([
        "run",
        "--resume",
        "repeated-resume-run",
        "--target",
        repo.to_str().unwrap(),
    ]);
    assert!(
        repeated_resume.status.success(),
        "{}",
        stderr(&repeated_resume)
    );
    let repeated_stdout = stdout(&repeated_resume);
    assert!(repeated_stdout.contains("status=Accepted"));
    assert!(repeated_stdout.contains("replay_state=accepted"));
    assert!(repeated_stdout.contains("evidence_dir="));
    assert!(!repeated_stdout.contains("bypass"));
    assert!(!repeated_stdout.contains("provider"));
}

#[test]
fn cli_template_list_and_show_exposes_real_project_templates() {
    let list = ao2(["template", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("bug-fix"));
    assert!(output.contains("small-refactor"));
    assert!(output.contains("dependency-upgrade"));
    assert!(output.contains("test-generation"));
    assert!(output.contains("rust-cargo-bug-fix"));

    let show = ao2(["template", "show", "bug-fix"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let yaml = stdout(&show);
    assert!(yaml.contains("id: bug-fix"));
    assert!(yaml.contains("approval_mode: exact_action_digest"));
    assert!(yaml.contains("evidence_cockpit: required"));

    let rust_show = ao2(["template", "show", "rust-cargo-bug-fix"]);
    assert!(rust_show.status.success(), "{}", stderr(&rust_show));
    let rust_yaml = stdout(&rust_show);
    assert!(rust_yaml.contains("id: rust-cargo-bug-fix"));
    assert!(rust_yaml.contains("command: cargo test"));
    assert!(rust_yaml.contains("Replay has zero digest failures."));

    let missing = ao2(["template", "show", "missing-template"]);
    assert!(!missing.status.success());
    assert!(stderr(&missing).contains("unknown template"));
}

#[test]
fn cli_version_json_reports_build_and_release_identity() {
    let version = ao2(["version", "--json"]);
    assert!(version.status.success(), "{}", stderr(&version));
    let json: serde_json::Value = serde_json::from_str(&stdout(&version)).unwrap();
    assert_eq!(json["package"], "ao2");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        json["target"],
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    );
    assert_eq!(json["release_manifest_schema"], "ao2.release-manifest.v1");
    let git_commit = json["git_commit"].as_str().unwrap();
    assert_eq!(git_commit.len(), 40);
    assert!(git_commit
        .chars()
        .all(|character| character.is_ascii_hexdigit()));
    assert_ne!(json["build_profile"], "unknown");

    let outside_git = tempfile::tempdir().unwrap();
    let relocated = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["version", "--json"])
        .current_dir(outside_git.path())
        .output()
        .unwrap();
    assert!(relocated.status.success(), "{}", stderr(&relocated));
    let relocated_json: serde_json::Value = serde_json::from_slice(&relocated.stdout).unwrap();
    assert_eq!(relocated_json["git_commit"], json["git_commit"]);
    assert_eq!(relocated_json["build_profile"], json["build_profile"]);
}

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

fn ao2<const N: usize>(args: [&str; N]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .unwrap()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn value_for<'a>(output: &'a str, prefix: &str) -> &'a str {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| {
            panic!(
                "missing prefix {prefix} in output:
{output}"
            )
        })
}
