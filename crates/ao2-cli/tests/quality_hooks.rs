use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(
            root.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(root.path(), &["config", "user.name", "Fixture"]);
        git(
            root.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/acme/sample-repo.git",
            ],
        );
        fs::write(
            root.path().join("ao-quality-gates.json"),
            serde_json::to_vec_pretty(&manifest()).unwrap(),
        )
        .unwrap();
        fs::write(root.path().join("source.txt"), "fixture\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "--quiet", "-m", "base"]);
        Self { root }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn hooks_dir(&self) -> PathBuf {
        self.root().join(".git/hooks")
    }

    fn quality(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
        command.args(["quality"]);
        command.args(args);
        command.args(["--target", self.root().to_str().unwrap(), "--json"]);
        command.output().unwrap()
    }

    fn hook_run(&self, hook: &str, stdin: &str) -> Output {
        use std::io::Write;

        let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
            .args([
                "quality",
                "hook-run",
                hook,
                "--target",
                self.root().to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn status(&self) -> Value {
        output_json(&self.quality(&["hooks", "status"]))
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn output_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn manifest() -> Value {
    let level = |snapshot: &str, maximum_duration_seconds: u64| {
        json!({
            "snapshot": snapshot,
            "maximum_duration_seconds": maximum_duration_seconds,
            "network_allowed": false,
            "mutates_source": false,
            "steps": [{
                "id": "diff-check",
                "argv": ["git", "diff", "--check"],
                "timeout_seconds": 5,
                "path_triggers": ["**"]
            }]
        })
    };
    json!({
        "schema_version": "ao.quality-gates.v1",
        "repository": "sample-repo",
        "lifecycle": "active_hosted",
        "supported_platforms": ["linux", "macos", "windows"],
        "required_tools": ["git"],
        "generated_paths": ["target/**"],
        "protected_paths": [".git/**"],
        "compatibility": {
            "minimum_consumer_version": "1.0.0",
            "owner": "sample-repo"
        },
        "evidence": {
            "public_safe": true,
            "local_artifact_root": "target/quality-gates",
            "maximum_result_bytes": 262144
        },
        "levels": {
            "commit": level("staged_tree", 10),
            "push": level("outgoing_commits", 120),
            "full": level("source_head", 300)
        }
    })
}

#[test]
fn status_reports_optional_hooks_absent_without_writing() {
    let fixture = Fixture::new();
    let before = git(fixture.root(), &["status", "--porcelain=v1"]);

    let status = fixture.status();

    assert_eq!(status["schema_version"], "ao2.quality-hooks-status.v1");
    assert_eq!(status["status"], "attention");
    assert_eq!(status["optional"], true);
    assert_eq!(status["hooks"][0]["state"], "absent");
    assert_eq!(status["hooks"][1]["state"], "absent");
    assert_eq!(status["source_mutation"], false);
    assert_eq!(status["network_access"], false);
    assert_eq!(status["provider_calls"], 0);
    assert_eq!(git(fixture.root(), &["status", "--porcelain=v1"]), before);
}

#[test]
fn explicit_install_is_idempotent_and_writes_only_thin_wrappers() {
    let fixture = Fixture::new();

    let first = output_json(&fixture.quality(&["hooks", "install"]));
    let commit = fs::read_to_string(fixture.hooks_dir().join("pre-commit")).unwrap();
    let push = fs::read_to_string(fixture.hooks_dir().join("pre-push")).unwrap();
    let second = output_json(&fixture.quality(&["hooks", "install"]));

    assert_eq!(first["status"], "installed");
    assert_eq!(first["changed_hooks"], json!(["pre-commit", "pre-push"]));
    assert_eq!(second["status"], "current");
    assert_eq!(second["changed_hooks"], json!([]));
    assert_eq!(
        commit,
        "#!/bin/sh\n# ao2-quality-hook:v1\nexec ao2 quality hook-run commit\n"
    );
    assert_eq!(
        push,
        "#!/bin/sh\n# ao2-quality-hook:v1\nexec ao2 quality hook-run push\n"
    );
    for wrapper in [&commit, &push] {
        assert!(!wrapper.contains("cargo"));
        assert!(!wrapper.contains("curl"));
        assert!(!wrapper.contains("git commit"));
        assert!(!wrapper.contains("git push"));
        assert!(!wrapper.contains("codex"));
        assert!(!wrapper.contains("claude"));
    }
    assert_eq!(fixture.status()["status"], "current");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for name in ["pre-commit", "pre-push"] {
            let mode = fs::metadata(fixture.hooks_dir().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "{name} must be executable");
        }
    }
}

#[test]
fn status_detects_stale_wrapper_and_install_upgrades_it() {
    let fixture = Fixture::new();
    fs::write(
        fixture.hooks_dir().join("pre-commit"),
        "#!/bin/sh\n# ao2-quality-hook:v0\nexit 0\n",
    )
    .unwrap();

    let status = fixture.status();
    assert_eq!(status["hooks"][0]["state"], "stale");

    let install = output_json(&fixture.quality(&["hooks", "install"]));
    assert_eq!(install["changed_hooks"], json!(["pre-commit", "pre-push"]));
    assert_eq!(fixture.status()["status"], "current");
}

#[test]
fn install_refuses_unmanaged_hook_before_writing_either_wrapper() {
    let fixture = Fixture::new();
    let custom = "#!/bin/sh\necho custom\n";
    fs::write(fixture.hooks_dir().join("pre-push"), custom).unwrap();

    let output = fixture.quality(&["hooks", "install"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HOOK_UNMANAGED"));
    assert!(!fixture.hooks_dir().join("pre-commit").exists());
    assert_eq!(
        fs::read_to_string(fixture.hooks_dir().join("pre-push")).unwrap(),
        custom
    );
    let status = fixture.status();
    assert_eq!(status["status"], "attention");
    assert_eq!(status["hooks"][1]["state"], "unmanaged");
}

#[test]
fn custom_hooks_path_is_diagnostic_only_and_never_modified() {
    let fixture = Fixture::new();
    git(
        fixture.root(),
        &["config", "core.hooksPath", "custom-hooks"],
    );

    let status = fixture.status();
    assert_eq!(status["status"], "attention");
    assert_eq!(status["configuration"], "custom_hooks_path_unsupported");

    let output = fixture.quality(&["hooks", "install"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HOOKS_PATH_CUSTOM"));
    assert!(!fixture.root().join("custom-hooks").exists());
}

#[test]
fn hook_run_commit_delegates_to_exact_staged_snapshot_without_source_mutation() {
    let fixture = Fixture::new();
    fs::write(fixture.root().join("source.txt"), "staged\n").unwrap();
    git(fixture.root(), &["add", "source.txt"]);
    let before = git(fixture.root(), &["status", "--porcelain=v1"]);

    let output = fixture.hook_run("commit", "");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("quality commit: passed"));
    assert_eq!(git(fixture.root(), &["status", "--porcelain=v1"]), before);
}

#[test]
fn hook_run_push_binds_the_single_remote_base_and_local_head() {
    let fixture = Fixture::new();
    let base = git(fixture.root(), &["rev-parse", "HEAD"]);
    fs::write(fixture.root().join("source.txt"), "outgoing\n").unwrap();
    git(fixture.root(), &["add", "source.txt"]);
    git(fixture.root(), &["commit", "--quiet", "-m", "outgoing"]);
    let head = git(fixture.root(), &["rev-parse", "HEAD"]);
    let input = format!("refs/heads/main {head} refs/heads/main {base}\n");

    let output = fixture.hook_run("push", &input);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("quality push: passed"));
}

#[test]
fn hook_run_push_rejects_malformed_or_mismatched_input() {
    let fixture = Fixture::new();
    let malformed = fixture.hook_run("push", "not four fields\n");
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("HOOK_PUSH_INPUT_INVALID"));

    let head = git(fixture.root(), &["rev-parse", "HEAD"]);
    let wrong = "1111111111111111111111111111111111111111";
    let mismatched = fixture.hook_run(
        "push",
        &format!("refs/heads/main {wrong} refs/heads/main {head}\n"),
    );
    assert!(!mismatched.status.success());
    assert!(String::from_utf8_lossy(&mismatched.stderr).contains("HOOK_PUSH_HEAD_MISMATCH"));
}

#[test]
fn new_branch_push_escalates_to_the_full_exact_head_gate() {
    let fixture = Fixture::new();
    let head = git(fixture.root(), &["rev-parse", "HEAD"]);
    let input =
        format!("refs/heads/new {head} refs/heads/new 0000000000000000000000000000000000000000\n");

    let output = fixture.hook_run("push", &input);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("quality full: passed"));
}

#[cfg(unix)]
#[test]
fn symlinked_hook_is_unsafe_and_install_does_not_follow_it() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let outside = fixture.root().join("outside-hook");
    fs::write(&outside, "preserve\n").unwrap();
    symlink(&outside, fixture.hooks_dir().join("pre-commit")).unwrap();

    let status = fixture.status();
    assert_eq!(status["hooks"][0]["state"], "unsafe");

    let output = fixture.quality(&["hooks", "install"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HOOK_UNSAFE"));
    assert_eq!(fs::read_to_string(outside).unwrap(), "preserve\n");
    assert!(!fixture.hooks_dir().join("pre-push").exists());
}
