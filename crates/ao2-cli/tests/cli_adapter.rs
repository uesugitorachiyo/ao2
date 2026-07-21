use std::fs;
use std::path::Path;
use std::process::Command;

fn init_git_repo(repo: &Path) {
    fs::create_dir_all(repo).unwrap();
    fs::write(repo.join("README.md"), "before\n").unwrap();
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
    commit_all(repo, "initial");
}

fn commit_all(repo: &Path, message: &str) {
    assert!(Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(status.status.success());
    if status.stdout.is_empty() {
        return;
    }
    assert!(Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn cli_adapter_doctor_reports_scripted_provider() {
    let doctor = ao2(["adapter", "doctor", "--provider", "scripted"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(json["provider"], "scripted");
    assert_eq!(json["available"], true);
    assert!(json["version"].as_str().unwrap().contains("built-in"));
}

#[test]
fn cli_adapter_sandbox_run_reports_diff_without_mutating_target() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("value.txt"), "before\n").unwrap();

    let shell = if cfg!(windows) { "powershell" } else { "sh" };
    let args = if cfg!(windows) {
        "-NoProfile\t-Command\tSet-Content -Path value.txt -Value after"
    } else {
        "-c\tprintf 'after\\n' > value.txt"
    };

    let run = ao2([
        "adapter",
        "run",
        "--provider",
        "scripted",
        "--target",
        repo.to_str().unwrap(),
        "--command",
        shell,
        "--args",
        args,
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    let json: serde_json::Value = serde_json::from_str(&stdout(&run)).unwrap();
    assert_eq!(json["adapter"]["provider"], "scripted");
    assert_eq!(json["changed_files"][0], "value.txt");
    assert_eq!(json["transcript_summary"]["changed_files"][0], "value.txt");
    assert!(json["diff_summary"]
        .as_str()
        .unwrap()
        .contains("modified: value.txt"));
    assert_eq!(
        fs::read_to_string(repo.join("value.txt")).unwrap(),
        "before\n"
    );
}

#[test]
fn cli_adapter_patch_preview_and_apply_promotes_exact_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_repo(&repo);
    fs::write(repo.join("value.txt"), "before\n").unwrap();
    commit_all(&repo, "add value fixture");

    let run = ao2([
        "adapter",
        "run",
        "--provider",
        "scripted",
        "--target",
        repo.to_str().unwrap(),
        "--command",
        "sh",
        "--args",
        "-c\tprintf 'after\\n' > value.txt",
        "--keep-sandbox",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let run_json: serde_json::Value = serde_json::from_str(&stdout(&run)).unwrap();
    let sandbox_path = run_json["sandbox_path"].as_str().unwrap();

    let preview = ao2([
        "adapter",
        "patch",
        "preview",
        "--target",
        repo.to_str().unwrap(),
        "--sandbox",
        sandbox_path,
    ]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let preview_json: serde_json::Value = serde_json::from_str(&stdout(&preview)).unwrap();
    let digest = preview_json["action_digest"].as_str().unwrap();
    assert_eq!(
        preview_json["approval_subject"]["schema_version"],
        "ao2.sandbox-patch-approval-subject.v1"
    );
    assert_eq!(
        preview_json["approval_subject"]["operation_type"],
        "sandbox_patch_apply"
    );
    assert_eq!(
        preview_json["approval_subject"]["operations"][0]["order"],
        0
    );
    assert_eq!(
        preview_json["approval_subject"]["operations"][0]["path"],
        "value.txt"
    );

    let apply = ao2([
        "adapter",
        "patch",
        "apply",
        "--target",
        repo.to_str().unwrap(),
        "--sandbox",
        sandbox_path,
        "--digest",
        digest,
        "--approver",
        "human:cli-test",
    ]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    let apply_json: serde_json::Value = serde_json::from_str(&stdout(&apply)).unwrap();
    assert_eq!(apply_json["action_digest"], digest);
    assert_eq!(
        apply_json["approval_subject"],
        preview_json["approval_subject"]
    );
    assert_eq!(apply_json["applied_files"][0], "value.txt");
    assert_eq!(
        fs::read_to_string(repo.join("value.txt")).unwrap(),
        "after\n"
    );
}

#[test]
fn cli_adapter_patch_apply_rejects_digest_after_target_commit_drift() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_repo(&repo);
    fs::write(repo.join("value.txt"), "before\n").unwrap();
    commit_all(&repo, "add value fixture");

    let run = ao2([
        "adapter",
        "run",
        "--provider",
        "scripted",
        "--target",
        repo.to_str().unwrap(),
        "--command",
        "sh",
        "--args",
        "-c\tprintf 'after\\n' > value.txt",
        "--keep-sandbox",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let run_json: serde_json::Value = serde_json::from_str(&stdout(&run)).unwrap();
    let sandbox_path = run_json["sandbox_path"].as_str().unwrap();

    let preview = ao2([
        "adapter",
        "patch",
        "preview",
        "--target",
        repo.to_str().unwrap(),
        "--sandbox",
        sandbox_path,
    ]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let preview_json: serde_json::Value = serde_json::from_str(&stdout(&preview)).unwrap();
    let digest = preview_json["action_digest"].as_str().unwrap();

    fs::write(repo.join("base-drift.txt"), "advanced base\n").unwrap();
    commit_all(&repo, "advance target after preview");

    let apply = ao2([
        "adapter",
        "patch",
        "apply",
        "--target",
        repo.to_str().unwrap(),
        "--sandbox",
        sandbox_path,
        "--digest",
        digest,
        "--approver",
        "human:cli-test",
    ]);
    assert!(!apply.status.success());
    assert!(
        stderr(&apply).contains("digest mismatch"),
        "{}",
        stderr(&apply)
    );
    assert_eq!(
        fs::read_to_string(repo.join("value.txt")).unwrap(),
        "before\n"
    );
}

#[test]
fn cli_adapter_prompt_runs_scripted_provider_inside_sandbox() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("value.txt"), "before\n").unwrap();

    let run = ao2([
        "adapter",
        "prompt",
        "--provider",
        "scripted",
        "--target",
        repo.to_str().unwrap(),
        "--prompt",
        "printf 'after\\n' > value.txt\nprintf 'Summary: updated value fixture\\n'\nprintf 'Changed files: value.txt\\n'\nprintf 'Input tokens: 7\\n'",
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    let json: serde_json::Value = serde_json::from_str(&stdout(&run)).unwrap();
    assert_eq!(json["adapter"]["provider"], "scripted");
    assert_eq!(json["changed_files"][0], "value.txt");
    assert_eq!(
        json["transcript_summary"]["raw_summary"],
        "updated value fixture"
    );
    assert_eq!(json["transcript_summary"]["usage"]["input_tokens"], 7);
    assert_eq!(
        fs::read_to_string(repo.join("value.txt")).unwrap(),
        "before\n"
    );
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
