use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn commit_fixture(root: &Path) {
    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "AO2 Test"]);
    git(root, &["config", "user.email", "ao2-test@example.invalid"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", "fixture"]);
}

#[allow(dead_code)]
pub fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", message]);
}
