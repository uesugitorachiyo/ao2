use std::fs;
use std::path::Path;
use std::process::Command;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

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
fn cli_git_status_and_diff_emit_read_only_evidence() {
    if !git_available() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_repo(&repo);
    fs::write(repo.join("README.md"), "before\nafter\n").unwrap();

    let status = ao2([
        "git",
        "status",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(status_json["schema_version"], "ao2.git-status.v1");
    assert_eq!(status_json["operation"], "status");
    assert_eq!(status_json["success"], true);
    assert_eq!(status_json["is_dirty"], true);
    assert!(status_json["stdout"]
        .as_str()
        .unwrap()
        .contains("README.md"));
    assert_eq!(
        status_json["argv"],
        serde_json::json!(["git", "status", "--short"])
    );

    let diff = ao2([
        "git",
        "diff",
        "--target",
        repo.to_str().unwrap(),
        "--stat",
        "--json",
    ]);
    assert!(diff.status.success(), "{}", stderr(&diff));
    let diff_json: serde_json::Value = serde_json::from_str(&stdout(&diff)).unwrap();
    assert_eq!(diff_json["schema_version"], "ao2.git-diff.v1");
    assert_eq!(diff_json["operation"], "diff");
    assert_eq!(diff_json["mode"], "stat");
    assert_eq!(diff_json["success"], true);
    assert!(diff_json["stdout"].as_str().unwrap().contains("README.md"));
    assert_eq!(
        diff_json["argv"],
        serde_json::json!(["git", "diff", "--stat"])
    );
}

#[test]
fn cli_git_commit_requires_exact_digest_then_commits_explicit_paths() {
    if !git_available() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_repo(&repo);
    fs::write(repo.join("README.md"), "before\nafter\n").unwrap();

    let preview = ao2([
        "git",
        "commit",
        "--target",
        repo.to_str().unwrap(),
        "--message",
        "update readme",
        "--path",
        "README.md",
        "--json",
    ]);
    assert!(!preview.status.success());
    let preview_json: serde_json::Value = serde_json::from_str(&stdout(&preview)).unwrap();
    assert_eq!(preview_json["schema_version"], "ao2.git-commit-approval.v1");
    assert_eq!(preview_json["status"], "approval_required");
    assert_eq!(preview_json["operation"], "commit");
    let digest = preview_json["action_digest"].as_str().unwrap();
    assert!(digest.len() >= 64);

    let commit = ao2([
        "git",
        "commit",
        "--target",
        repo.to_str().unwrap(),
        "--message",
        "update readme",
        "--path",
        "README.md",
        "--approve-action-digest",
        digest,
        "--approver",
        "human:cli-test",
        "--json",
    ]);
    assert!(commit.status.success(), "{}", stderr(&commit));
    let commit_json: serde_json::Value = serde_json::from_str(&stdout(&commit)).unwrap();
    assert_eq!(commit_json["schema_version"], "ao2.git-commit.v1");
    assert_eq!(commit_json["success"], true);
    assert_eq!(commit_json["approval"]["approver"], "human:cli-test");
    assert_eq!(commit_json["approval"]["action_digest"], digest);
    assert!(commit_json["commit_sha"].as_str().unwrap().len() >= 7);

    let log = Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("update readme"));
}

#[test]
fn cli_git_tag_requires_exact_digest_then_tags_head() {
    if !git_available() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_repo(&repo);

    let preview = ao2([
        "git",
        "tag",
        "--target",
        repo.to_str().unwrap(),
        "--tag",
        "v-test",
        "--message",
        "test tag",
        "--json",
    ]);
    assert!(!preview.status.success());
    let preview_json: serde_json::Value = serde_json::from_str(&stdout(&preview)).unwrap();
    assert_eq!(preview_json["schema_version"], "ao2.git-tag-approval.v1");
    assert_eq!(preview_json["status"], "approval_required");
    let digest = preview_json["action_digest"].as_str().unwrap();

    let tag = ao2([
        "git",
        "tag",
        "--target",
        repo.to_str().unwrap(),
        "--tag",
        "v-test",
        "--message",
        "test tag",
        "--approve-action-digest",
        digest,
        "--approver",
        "human:cli-test",
        "--json",
    ]);
    assert!(tag.status.success(), "{}", stderr(&tag));
    let tag_json: serde_json::Value = serde_json::from_str(&stdout(&tag)).unwrap();
    assert_eq!(tag_json["schema_version"], "ao2.git-tag.v1");
    assert_eq!(tag_json["success"], true);
    assert_eq!(tag_json["tag"], "v-test");

    let list = Command::new("git")
        .args(["tag", "--list", "v-test"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&list.stdout).trim(), "v-test");
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
