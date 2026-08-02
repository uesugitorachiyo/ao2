use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        run_git(root.path(), &["init", "--quiet"]);
        run_git(
            root.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        run_git(root.path(), &["config", "user.name", "Fixture"]);
        run_git(
            root.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/acme/sample-repo.git",
            ],
        );
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir_all(root.path().join("docs")).unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 1 }\n",
        )
        .unwrap();
        fs::write(root.path().join("docs/readme.md"), "base\n").unwrap();
        run_git(root.path(), &["add", "."]);
        run_git(root.path(), &["commit", "--quiet", "-m", "base"]);
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn manifest_path(&self) -> PathBuf {
        self.path().join("ao-quality-gates.json")
    }

    fn write_manifest(&self, document: &Value) {
        fs::write(
            self.manifest_path(),
            serde_json::to_vec_pretty(document).unwrap(),
        )
        .unwrap();
    }

    fn quality(&self, level: &str, base: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
        command.args([
            "quality",
            "check",
            level,
            "--target",
            self.path().to_str().unwrap(),
            "--manifest",
            self.manifest_path().to_str().unwrap(),
            "--json",
        ]);
        if let Some(base) = base {
            command.args(["--base", base]);
        }
        command.output().unwrap()
    }

    fn quality_out(&self, level: &str, out: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ao2"))
            .args([
                "quality",
                "check",
                level,
                "--target",
                self.path().to_str().unwrap(),
                "--manifest",
                self.manifest_path().to_str().unwrap(),
                "--out",
                out,
                "--json",
            ])
            .output()
            .unwrap()
    }
}

fn run_git(root: &Path, args: &[&str]) -> String {
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

fn valid_manifest() -> Value {
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
            "commit": {
                "snapshot": "staged_tree",
                "maximum_duration_seconds": 10,
                "network_allowed": false,
                "mutates_source": false,
                "steps": [
                    {
                        "id": "rust-check",
                        "argv": ["git", "diff", "--cached", "--check"],
                        "timeout_seconds": 5,
                        "path_triggers": ["src/**"]
                    },
                    {
                        "id": "docs-check",
                        "argv": ["git", "diff", "--cached", "--check"],
                        "timeout_seconds": 5,
                        "path_triggers": ["docs/**"]
                    }
                ]
            },
            "push": {
                "snapshot": "outgoing_commits",
                "maximum_duration_seconds": 120,
                "network_allowed": false,
                "mutates_source": false,
                "steps": [
                    {
                        "id": "outgoing-check",
                        "argv": ["git", "diff", "--check"],
                        "timeout_seconds": 30,
                        "path_triggers": ["src/**"]
                    }
                ]
            },
            "full": {
                "snapshot": "source_head",
                "maximum_duration_seconds": 300,
                "network_allowed": false,
                "mutates_source": false,
                "steps": [
                    {
                        "id": "full-check",
                        "argv": ["git", "status", "--short"],
                        "timeout_seconds": 30,
                        "path_triggers": ["**"]
                    }
                ]
            }
        }
    })
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid stdout JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn commit_check_binds_only_the_staged_tree_and_cached_paths() {
    let fixture = Fixture::new();
    fixture.write_manifest(&valid_manifest());
    fs::write(
        fixture.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 2 }\n",
    )
    .unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);
    fs::write(fixture.path().join("docs/readme.md"), "unstaged one\n").unwrap();

    let first = fixture.quality("commit", None);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first = stdout_json(&first);
    assert_eq!(first["schema_version"], "ao2.quality-check-result.v1");
    assert_eq!(first["status"], "passed");
    assert_eq!(first["snapshot"]["kind"], "staged_tree");
    assert_eq!(first["snapshot"]["changed_paths"], json!(["src/lib.rs"]));
    assert_eq!(first["selected_steps"][0]["id"], "rust-check");
    assert_eq!(first["selected_steps"].as_array().unwrap().len(), 1);

    fs::write(fixture.path().join("docs/readme.md"), "unstaged two\n").unwrap();
    let second = fixture.quality("commit", None);
    assert!(second.status.success());
    let second = stdout_json(&second);
    assert_eq!(first["snapshot"]["sha256"], second["snapshot"]["sha256"]);
}

#[test]
fn push_check_binds_base_head_and_outgoing_commits_not_worktree_bytes() {
    let fixture = Fixture::new();
    fixture.write_manifest(&valid_manifest());
    let base = run_git(fixture.path(), &["rev-parse", "HEAD"]);
    fs::write(
        fixture.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 2 }\n",
    )
    .unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);
    run_git(fixture.path(), &["commit", "--quiet", "-m", "change"]);
    let head = run_git(fixture.path(), &["rev-parse", "HEAD"]);
    fs::write(fixture.path().join("docs/readme.md"), "unstaged one\n").unwrap();

    let first = fixture.quality("push", Some(&base));
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first = stdout_json(&first);
    assert_eq!(first["snapshot"]["base_sha"], base);
    assert_eq!(first["snapshot"]["head_sha"], head);
    assert_eq!(first["snapshot"]["outgoing_commits"], json!([head]));
    assert_eq!(first["snapshot"]["changed_paths"], json!(["src/lib.rs"]));

    fs::write(fixture.path().join("docs/readme.md"), "unstaged two\n").unwrap();
    let second = stdout_json(&fixture.quality("push", Some(&base)));
    assert_eq!(first["snapshot"]["sha256"], second["snapshot"]["sha256"]);
}

#[test]
fn unsafe_manifest_is_rejected_before_any_step_executes() {
    let fixture = Fixture::new();
    let marker = fixture.path().join("executed-marker");
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["network_allowed"] = json!(true);
    manifest["levels"]["commit"]["steps"][0]["argv"] = json!([
        "git",
        "config",
        "--file",
        marker.to_str().unwrap(),
        "fixture.executed",
        "true"
    ]);
    fixture.write_manifest(&manifest);
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    assert!(!marker.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("FAST_GATE_NETWORK_FORBIDDEN"));
}

#[test]
fn shell_evaluation_and_unsafe_paths_fail_closed() {
    let fixture = Fixture::new();
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] = json!(["sh", "-c", "printf unsafe"]);
    manifest["levels"]["commit"]["steps"][0]["path_triggers"] = json!(["../outside/**"]);
    fixture.write_manifest(&manifest);
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("SHELL_EVALUATION_FORBIDDEN"), "{stderr}");
    assert!(stderr.contains("PATH_PATTERN_UNSAFE"), "{stderr}");
}

#[test]
fn environment_wrappers_cannot_hide_provider_or_network_commands() {
    let fixture = Fixture::new();
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] = json!(["env", "SAFE=1", "codex", "--help"]);
    manifest["levels"]["commit"]["steps"][1]["argv"] =
        json!(["env", "git", "-c", "protocol.version=2", "fetch"]);
    fixture.write_manifest(&manifest);
    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("PROVIDER_COMMAND_FORBIDDEN"), "{stderr}");
    assert!(stderr.contains("NETWORK_COMMAND_FORBIDDEN"), "{stderr}");
}

#[test]
fn duplicate_unknown_and_oversized_manifests_are_rejected() {
    let fixture = Fixture::new();
    let encoded = serde_json::to_string(&valid_manifest()).unwrap();
    fs::write(
        fixture.manifest_path(),
        encoded.replacen(
            "\"repository\":\"sample-repo\"",
            "\"repository\":\"sample-repo\",\"repository\":\"sample-repo\"",
            1,
        ),
    )
    .unwrap();
    let duplicate = fixture.quality("commit", None);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("MANIFEST_DUPLICATE_KEY"));

    let mut unknown = valid_manifest();
    unknown["unexpected"] = json!(true);
    fixture.write_manifest(&unknown);
    let unknown = fixture.quality("commit", None);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("MANIFEST_CONTRACT_INVALID"));

    fs::write(fixture.manifest_path(), vec![b' '; 256 * 1024 + 1]).unwrap();
    let oversized = fixture.quality("commit", None);
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("MANIFEST_SIZE_LIMIT"));
}

#[test]
fn secret_like_manifest_and_unusable_result_limit_are_rejected_before_execution() {
    let fixture = Fixture::new();
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] =
        json!(["git", "status", "OPENAI_API_KEY=not-allowed"]);
    fixture.write_manifest(&manifest);
    let secret = fixture.quality("commit", None);
    assert!(!secret.status.success());
    assert!(String::from_utf8_lossy(&secret.stderr).contains("MANIFEST_SECRET_MATERIAL_FORBIDDEN"));

    let mut manifest = valid_manifest();
    manifest["evidence"]["maximum_result_bytes"] = json!(1);
    fixture.write_manifest(&manifest);
    let undersized = fixture.quality("commit", None);
    assert!(!undersized.status.success());
    assert!(String::from_utf8_lossy(&undersized.stderr).contains("EVIDENCE_SIZE_LIMIT_TOO_SMALL"));
}

#[test]
fn result_path_must_use_the_declared_generated_root() {
    let fixture = Fixture::new();
    fixture.write_manifest(&valid_manifest());
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let outside = fixture.quality_out("commit", "quality-result.json");
    assert!(!outside.status.success());
    assert!(String::from_utf8_lossy(&outside.stderr).contains("RESULT_PATH_OUTSIDE_ARTIFACT_ROOT"));
    assert!(!fixture.path().join("quality-result.json").exists());

    let accepted = fixture.quality_out("commit", "target/quality-gates/result.json");
    assert!(accepted.status.success());
    assert!(fixture
        .path()
        .join("target/quality-gates/result.json")
        .is_file());
}

#[test]
fn wrong_repository_and_non_ancestor_push_base_are_rejected() {
    let fixture = Fixture::new();
    let mut manifest = valid_manifest();
    manifest["repository"] = json!("different-repository");
    manifest["compatibility"]["owner"] = json!("different-repository");
    fixture.write_manifest(&manifest);
    let wrong_repository = fixture.quality("full", None);
    assert!(!wrong_repository.status.success());
    assert!(
        String::from_utf8_lossy(&wrong_repository.stderr).contains("MANIFEST_REPOSITORY_MISMATCH")
    );

    fixture.write_manifest(&valid_manifest());
    let unrelated = "1111111111111111111111111111111111111111";
    let wrong_base = fixture.quality("push", Some(unrelated));
    assert!(!wrong_base.status.success());
    assert!(String::from_utf8_lossy(&wrong_base.stderr).contains("PUSH_BASE_INVALID"));
}

#[test]
fn nonzero_step_is_structured_and_fails_the_gate() {
    let fixture = Fixture::new();
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] =
        json!(["git", "rev-parse", "--verify", "refs/heads/absent"]);
    fixture.write_manifest(&manifest);
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    let result = stdout_json(&output);
    assert_eq!(result["status"], "failed");
    assert_eq!(result["steps"][0]["status"], "failed");
    assert!(result["steps"][0]["exit_code"].as_i64().unwrap() != 0);
    assert_eq!(result["provider_calls"], 0);
}

#[test]
fn source_mutation_by_a_step_is_detected() {
    let fixture = Fixture::new();
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] = json!([
        "git",
        "config",
        "--file",
        "unexpected-source-file",
        "fixture.mutated",
        "true"
    ]);
    fixture.write_manifest(&manifest);
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    let result = stdout_json(&output);
    assert_eq!(result["source_mutation_detected"], true);
    assert!(result["failure_codes"]
        .as_array()
        .unwrap()
        .contains(&json!("SOURCE_MUTATION_DETECTED")));
}

#[cfg(unix)]
#[test]
fn step_timeout_is_bounded_and_structured() {
    let fixture = Fixture::new();
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] = json!(["/bin/sleep", "2"]);
    manifest["levels"]["commit"]["steps"][0]["timeout_seconds"] = json!(1);
    fixture.write_manifest(&manifest);
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let started = std::time::Instant::now();
    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    let result = stdout_json(&output);
    assert_eq!(result["steps"][0]["timed_out"], true);
    assert!(result["failure_codes"]
        .as_array()
        .unwrap()
        .contains(&json!("STEP_TIMEOUT")));
}

#[cfg(unix)]
#[test]
fn descendant_held_output_pipe_cannot_defeat_the_gate_deadline() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let script = fixture.path().join("spawn-descendant.sh");
    fs::write(&script, "#!/bin/sh\n(sleep 5) &\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).unwrap();
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] = json!(["./spawn-descendant.sh"]);
    fixture.write_manifest(&manifest);
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let started = std::time::Instant::now();
    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    let result = stdout_json(&output);
    assert_eq!(
        result["steps"][0]["failure_code"],
        "STEP_DESCENDANT_TERMINATED"
    );
    assert_eq!(result["steps"][0]["descendant_processes_terminated"], true);
}

#[cfg(unix)]
#[test]
fn output_is_redacted_before_its_digest_is_recorded() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path().join("secret-output.txt"),
        "OPENAI_API_KEY=super-secret\n",
    )
    .unwrap();
    run_git(fixture.path(), &["add", "secret-output.txt"]);
    run_git(
        fixture.path(),
        &["commit", "--quiet", "-m", "output fixture"],
    );
    let mut manifest = valid_manifest();
    manifest["levels"]["commit"]["steps"][0]["argv"] =
        json!(["git", "show", "HEAD:secret-output.txt"]);
    fixture.write_manifest(&manifest);
    fs::write(fixture.path().join("src/lib.rs"), "changed\n").unwrap();
    run_git(fixture.path(), &["add", "src/lib.rs"]);

    let output = fixture.quality("commit", None);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("super-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("super-secret"));
    let result = stdout_json(&output);
    let expected = format!("sha256:{:x}", Sha256::digest(b"OPENAI_API_KEY=[REDACTED]"));
    assert_eq!(result["steps"][0]["stdout_redacted_sha256"], expected);
}

#[cfg(unix)]
#[test]
fn symlinked_manifest_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let target = fixture.path().join("manifest-target.json");
    fs::write(&target, serde_json::to_vec(&valid_manifest()).unwrap()).unwrap();
    symlink(&target, fixture.manifest_path()).unwrap();
    let output = fixture.quality("commit", None);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("MANIFEST_SYMLINK"));
}
