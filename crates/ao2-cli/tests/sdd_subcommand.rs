use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn sdd_validate_passes_and_fails_with_rule_text() {
    let valid = fixture("valid_minimal.json");
    let pass = ao2(["sdd", "validate", "--plan", valid.to_str().unwrap()], []);
    assert!(pass.status.success(), "{}", stderr(&pass));
    assert_eq!(stdout(&pass), "PASS\n");

    let invalid = fixture("invalid_empty_acceptance.json");
    let fail = ao2(["sdd", "validate", "--plan", invalid.to_str().unwrap()], []);
    assert_eq!(fail.status.code(), Some(2), "{}", stderr(&fail));
    let stderr = stderr(&fail);
    assert!(stderr.contains("FAIL:"), "{stderr}");
    assert!(stderr.contains("V3:"), "{stderr}");
}

#[test]
fn sdd_plan_writes_provider_canonical_json_verbatim() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("tiny");
    fs::create_dir_all(target.join("src")).unwrap();
    fs::write(
        target.join("Cargo.toml"),
        "[package]\nname='tiny'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        target.join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .unwrap();

    let prompt = temp.path().join("prompt.md");
    fs::write(&prompt, "Build a tiny CLI that prints hello.").unwrap();
    let out = temp.path().join("plan.json");
    let candidate = fixture("codex-candidate.json");
    let path = prepend_path(&mock_bins());
    let output = ao2(
        [
            "sdd",
            "plan",
            "--prompt",
            &format!("@{}", prompt.display()),
            "--target",
            target.to_str().unwrap(),
            "--provider",
            "codex",
            "--out",
            out.to_str().unwrap(),
        ],
        [
            ("PATH", path.as_str()),
            ("SDD_MOCK_STDOUT", candidate.to_str().unwrap()),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("plan_id=01JCODEXABCDEFGHJKMNPQRSTV"));

    let written = fs::read_to_string(&out).unwrap();
    let value: serde_json::Value = serde_json::from_str(&written).unwrap();
    let expected = sdd_planner::canonical_json(&value);
    assert_eq!(written, expected);
    assert_eq!(value["schema_version"], "ao2.sdd-plan.v1");
    assert_eq!(
        value["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(value["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(target
        .join("target/sdd-planner/01JCODEXABCDEFGHJKMNPQRSTV/attempt-1.json")
        .is_file());
}

#[test]
fn sdd_dispatch_validates_and_emits_runner_specs() {
    let temp = tempfile::tempdir().unwrap();
    let plan = fixture("valid_full.json");

    let ao2_out = temp.path().join("ao2-run.yaml");
    let ao2_dispatch = ao2(
        [
            "sdd",
            "dispatch",
            "--plan",
            plan.to_str().unwrap(),
            "--runner",
            "ao2",
            "--out",
            ao2_out.to_str().unwrap(),
        ],
        [],
    );
    assert!(ao2_dispatch.status.success(), "{}", stderr(&ao2_dispatch));
    assert_eq!(
        normalize_newlines(&fs::read_to_string(&ao2_out).unwrap()),
        normalize_newlines(&fs::read_to_string(fixture("expected-ao2-runspec.yaml")).unwrap())
    );

    let operator_out = temp.path().join("ao-operator-runspec.json");
    let operator = ao2(
        [
            "sdd",
            "dispatch",
            "--plan",
            plan.to_str().unwrap(),
            "--runner",
            "ao-operator",
            "--out",
            operator_out.to_str().unwrap(),
        ],
        [],
    );
    assert!(operator.status.success(), "{}", stderr(&operator));
    assert_eq!(
        fs::read_to_string(&operator_out).unwrap(),
        fs::read_to_string(fixture("expected-ao-operator-runspec.json")).unwrap()
    );
}

#[test]
fn ao2_run_dry_run_accepts_generated_sdd_runspec_without_mutation() {
    let spec = fixture("expected-ao2-runspec.yaml");
    let output = ao2(["run", "--dry-run", "--spec", spec.to_str().unwrap()], []);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("status=dry_run_accepted"), "{stdout}");
    assert!(stdout.contains("schema_version=ao2.run/v1"), "{stdout}");
    assert!(
        stdout.contains("control_plane_role=read_only_observer"),
        "{stdout}"
    );
    assert!(stdout.contains("mutates_ao_artifacts=false"), "{stdout}");
}

#[test]
fn ao2_run_spec_provider_free_real_project_behavior_is_documented() {
    let docs = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/SCHEMAS-AND-INTERFACES.md"),
    )
    .unwrap();
    for required in [
        "ao2 run --spec",
        "provider-free real_project",
        "evidence-only",
        "does not apply fixture-specific patches",
        "provider_free.commands",
        "provider_free_command_log",
        "--provider scripted",
    ] {
        assert!(
            docs.contains(required),
            "SDD interface docs missing provider-free real-project detail {required:?}"
        );
    }
}

#[test]
fn provider_free_command_contract_fixture_is_documented() {
    let contract_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/ao2-provider-free-commands-v0.1.schema.json");
    let contract_text = fs::read_to_string(&contract_path).unwrap();
    let contract: serde_json::Value = serde_json::from_str(&contract_text).unwrap();
    assert_eq!(
        contract["properties"]["schema_version"]["const"],
        "ao2.provider-free-commands.v0.1"
    );
    assert!(contract_text.contains("provider_free_command_log"));
    assert!(contract_text.contains("git push"));

    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sdd-planner/tests/fixtures/provider-free-command-runspec.yaml");
    let fixture_text = fs::read_to_string(&fixture_path).unwrap();
    assert!(fixture_text.contains("provider_free:"));
    assert!(fixture_text.contains("commands:"));
    assert!(fixture_text.contains("generated.txt"));
}

#[test]
fn sdd_dispatch_dry_run_invokes_ao2_spec_preflight() {
    let temp = tempfile::tempdir().unwrap();
    let plan = fixture("valid_full.json");
    let out = temp.path().join("ao2-run.yaml");
    let output = ao2(
        [
            "sdd",
            "dispatch",
            "--plan",
            plan.to_str().unwrap(),
            "--runner",
            "ao2",
            "--out",
            out.to_str().unwrap(),
            "--dry-run",
        ],
        [],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("status=dry_run_accepted"), "{stdout}");
    assert!(stdout.contains("out="), "{stdout}");
    assert!(
        out.is_file(),
        "dry-run should still emit the translated runspec"
    );
}

#[test]
fn ao2_run_executes_generated_sdd_runspec_through_governed_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let spec = temp.path().join("ao2-run.yaml");
    let spec_text = fs::read_to_string(fixture("expected-ao2-runspec.yaml"))
        .unwrap()
        .replace(
            "repo_path: /tmp/repo-full",
            &format!("repo_path: {}", repo.display()),
        )
        .replace("cargo test --release", "python -m pytest")
        .replace(
            "cargo clippy --workspace -- -D warnings",
            "python -m pytest",
        );
    fs::write(&spec, spec_text).unwrap();

    let output = ao2(
        [
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--run-id",
            "sdd-governed-run",
        ],
        [],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("status=Accepted"), "{stdout}");
    let evidence_pack = value_for(&stdout, "evidence_pack=");
    let sdd_task_graph = value_for(&stdout, "sdd_task_graph=");
    assert!(Path::new(evidence_pack).is_file());
    assert!(Path::new(sdd_task_graph).is_file());

    let run_record: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".ao2/runs/sdd-governed-run/run-record.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(run_record["workflow_tasks"][0]["id"], "step_scaffold");
    assert_eq!(
        run_record["workflow_tasks"][0]["policy_profile"],
        "ao2-sdd-run-task"
    );
    assert_eq!(
        run_record["workflow_dependencies"][0]["from"],
        "step_scaffold"
    );
    assert_eq!(run_record["workflow_dependencies"][0]["to"], "step_tests");
    assert_eq!(
        run_record["factory_v3_compatibility"]["source_schema"],
        "ao2.sdd-plan.v1"
    );
    let evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(evidence_pack).unwrap()).unwrap();
    assert_eq!(
        evidence["runtime_contract"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        evidence["factory_v3_compatibility"]["control_plane_role"],
        "read_only_observer"
    );
    let task_graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sdd_task_graph).unwrap()).unwrap();
    assert_eq!(
        task_graph["schema_version"],
        "ao2.sdd-task-graph-execution.v1"
    );
    assert_eq!(task_graph["execution_mode"], "provider_free");
    assert_eq!(task_graph["task_count"], 3);
    assert_eq!(task_graph["tasks"][0]["id"], "step_scaffold");
    assert_eq!(task_graph["tasks"][2]["id"], "step_release");
    assert_eq!(
        task_graph["tasks"][0]["provider_contract"]["secret_redaction"],
        true
    );
    let workflow = value_for(&stdout, "workflow=");
    assert_eq!(
        Path::new(&workflow),
        repo.join(".ao2/generated-workflows/01jfulldefghjkmnpqrstvwxyz1-sdd-run.yaml")
    );
    assert!(Path::new(&workflow).is_file());
}

#[test]
fn ao2_run_spec_provider_free_real_project_does_not_use_discount_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("go.mod"),
        "module example.com/real-project\n\ngo 1.22\n",
    )
    .unwrap();
    fs::write(
        repo.join("main_test.go"),
        "package main\n\nimport \"testing\"\n\nfunc TestSmoke(t *testing.T) {}\n",
    )
    .unwrap();

    let spec = temp.path().join("ao2-run.yaml");
    let spec_text = fs::read_to_string(fixture("expected-ao2-runspec.yaml"))
        .unwrap()
        .replace(
            "repo_path: /tmp/repo-full",
            &format!("repo_path: {}", repo.display()),
        )
        .replace("cargo test --release", "go test ./...")
        .replace("cargo clippy --workspace -- -D warnings", "go test ./...");
    fs::write(&spec, spec_text).unwrap();

    let output = ao2(
        [
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--run-id",
            "sdd-provider-free-real-project",
        ],
        [],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("status=Accepted"), "{stdout}");
    assert!(
        !repo.join("discount_service").exists(),
        "provider-free SDD run must not materialize the discount fixture"
    );
    let sdd_task_graph = value_for(&stdout, "sdd_task_graph=");
    let task_graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sdd_task_graph).unwrap()).unwrap();
    assert_eq!(task_graph["execution_mode"], "provider_free");
    assert_eq!(task_graph["task_count"], 3);
}

#[test]
fn ao2_run_spec_provider_free_real_project_executes_explicit_local_commands() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project-local-command");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("go.mod"),
        "module example.com/real-project-local-command\n\ngo 1.22\n",
    )
    .unwrap();

    let spec = temp.path().join("ao2-run.yaml");
    let spec_text = fs::read_to_string(fixture("expected-ao2-runspec.yaml"))
        .unwrap()
        .replace(
            "repo_path: /tmp/repo-full",
            &format!("repo_path: {}", repo.display()),
        )
        .replace(
            "cargo test --release",
            "python -c 'from pathlib import Path; raise SystemExit(0 if Path(\"generated.txt\").read_text() == \"generated\" + chr(10) else 1)'",
        )
        .replace(
            "cargo clippy --workspace -- -D warnings",
            "python -c 'from pathlib import Path; raise SystemExit(0 if Path(\"generated.txt\").is_file() else 1)'",
        );
    let mut spec_value: serde_yaml::Value = serde_yaml::from_str(&spec_text).unwrap();
    spec_value["spec"]["tasks"][0]["provider_free"] = serde_yaml::Value::Mapping({
        let mut mapping = serde_yaml::Mapping::new();
        mapping.insert(
            serde_yaml::Value::String("commands".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                "python -c 'from pathlib import Path; Path(\"generated.txt\").write_text(\"generated\" + chr(10))'"
                    .to_string(),
            )]),
        );
        mapping
    });
    fs::write(&spec, serde_yaml::to_string(&spec_value).unwrap()).unwrap();

    let output = ao2(
        [
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--run-id",
            "sdd-provider-free-local-command",
        ],
        [],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("status=Accepted"), "{stdout}");
    let generated = fs::read_to_string(repo.join("generated.txt")).unwrap();
    assert_eq!(generated.replace("\r\n", "\n"), "generated\n");

    let evidence_pack = value_for(&stdout, "evidence_pack=");
    let evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(evidence_pack).unwrap()).unwrap();
    assert!(
        evidence["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["artifact_type"] == "provider_free_command_log"),
        "provider-free command execution must be recorded in evidence pack"
    );

    let sdd_task_graph = value_for(&stdout, "sdd_task_graph=");
    let task_graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sdd_task_graph).unwrap()).unwrap();
    assert_eq!(task_graph["execution_mode"], "provider_free");
    assert_eq!(
        task_graph["task_executions"][0]["provider_free_command_count"],
        1
    );
}

#[test]
fn ao2_run_spec_provider_free_rejects_unsafe_local_commands_before_execution() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project-unsafe-command");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("go.mod"),
        "module example.com/real-project-unsafe-command\n\ngo 1.22\n",
    )
    .unwrap();

    let spec = temp.path().join("ao2-run.yaml");
    let spec_text = fs::read_to_string(fixture("expected-ao2-runspec.yaml"))
        .unwrap()
        .replace(
            "repo_path: /tmp/repo-full",
            &format!("repo_path: {}", repo.display()),
        )
        .replace("cargo test --release", "true")
        .replace("cargo clippy --workspace -- -D warnings", "true");
    let mut spec_value: serde_yaml::Value = serde_yaml::from_str(&spec_text).unwrap();
    spec_value["spec"]["tasks"][0]["provider_free"] = serde_yaml::Value::Mapping({
        let mut mapping = serde_yaml::Mapping::new();
        mapping.insert(
            serde_yaml::Value::String("commands".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                "git push origin main".to_string(),
            )]),
        );
        mapping
    });
    fs::write(&spec, serde_yaml::to_string(&spec_value).unwrap()).unwrap();

    let output = ao2(
        [
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--run-id",
            "sdd-provider-free-unsafe-command",
        ],
        [],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let stderr = stderr(&output);
    assert!(
        stderr.contains("provider_free.commands[0] is not allowed"),
        "{stderr}"
    );
    assert!(
        !repo
            .join(".ao2/runs/sdd-provider-free-unsafe-command/evidence-pack/evidence-pack.json")
            .exists(),
        "unsafe provider-free command should fail before accepted evidence pack export"
    );
}

#[test]
fn ao2_run_spec_provider_free_applies_task_denied_patterns() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project-task-policy");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("go.mod"),
        "module example.com/real-project-task-policy\n\ngo 1.22\n",
    )
    .unwrap();

    let spec = temp.path().join("ao2-run.yaml");
    let spec_text = fs::read_to_string(fixture("expected-ao2-runspec.yaml"))
        .unwrap()
        .replace(
            "repo_path: /tmp/repo-full",
            &format!("repo_path: {}", repo.display()),
        )
        .replace("cargo test --release", "true")
        .replace("cargo clippy --workspace -- -D warnings", "true");
    let mut spec_value: serde_yaml::Value = serde_yaml::from_str(&spec_text).unwrap();
    spec_value["spec"]["exit_criteria"]["tests"] =
        serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("true".to_string())]);
    spec_value["spec"]["exit_criteria"]["gates"] =
        serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("true".to_string())]);
    spec_value["spec"]["tasks"][0]["provider_free"] = serde_yaml::Value::Mapping({
        let mut provider_free = serde_yaml::Mapping::new();
        provider_free.insert(
            serde_yaml::Value::String("commands".to_string()),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                "python -c 'print(\"blocked by task policy\")'".to_string(),
            )]),
        );
        provider_free.insert(
            serde_yaml::Value::String("policy".to_string()),
            serde_yaml::Value::Mapping({
                let mut policy = serde_yaml::Mapping::new();
                policy.insert(
                    serde_yaml::Value::String("denied_patterns".to_string()),
                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                        "python -c".to_string(),
                    )]),
                );
                policy
            }),
        );
        provider_free
    });
    fs::write(&spec, serde_yaml::to_string(&spec_value).unwrap()).unwrap();

    let output = ao2(
        [
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--run-id",
            "sdd-provider-free-task-policy",
        ],
        [],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let stderr = stderr(&output);
    assert!(
        stderr.contains("provider_free.policy.denied_patterns[0]"),
        "{stderr}"
    );
    assert!(!repo
        .join(".ao2/runs/sdd-provider-free-task-policy/evidence-pack/evidence-pack.json")
        .exists());
}

#[test]
fn ao2_run_spec_provider_mode_builds_prompt_from_sdd_task_graph() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let spec = temp.path().join("ao2-run.yaml");
    let spec_text = fs::read_to_string(fixture("expected-ao2-runspec.yaml"))
        .unwrap()
        .replace(
            "repo_path: /tmp/repo-full",
            &format!("repo_path: {}", repo.display()),
        )
        .replace("cargo test --release", "python -m pytest")
        .replace(
            "cargo clippy --workspace -- -D warnings",
            "python -m pytest",
        );
    let mut spec_value: serde_yaml::Value = serde_yaml::from_str(&spec_text).unwrap();
    spec_value["spec"]["tasks"]
        .as_sequence_mut()
        .unwrap()
        .reverse();
    fs::write(&spec, serde_yaml::to_string(&spec_value).unwrap()).unwrap();

    let output = ao2(
        [
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--provider",
            "scripted",
            "--run-id",
            "sdd-provider-run",
        ],
        [],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("status=Accepted"), "{stdout}");
    assert!(
        stdout.contains("status=governed_provider_run_started"),
        "{stdout}"
    );
    let evidence_pack = value_for(&stdout, "evidence_pack=");
    let sdd_task_graph = value_for(&stdout, "sdd_task_graph=");
    let evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(evidence_pack).unwrap()).unwrap();
    assert_eq!(
        evidence["runtime_contract"]["provider_adapter_contract"]["status"],
        "observed"
    );
    assert_eq!(
        evidence["runtime_contract"]["provider_adapter_contract"]["provider_summary_count"],
        3
    );
    assert_eq!(evidence["provider_summaries"][0]["provider"], "scripted");
    assert_eq!(
        evidence["provider_summaries"][0]["task_id"],
        "step_scaffold"
    );
    assert_eq!(evidence["provider_summaries"][1]["task_id"], "step_tests");
    assert_eq!(evidence["provider_summaries"][2]["task_id"], "step_release");
    assert_eq!(
        evidence["provider_summaries"][0]["raw_summary"],
        "AO2 scripted provider accepted SDD task graph 01JFULLDEFGHJKMNPQRSTVWXYZ1"
    );
    assert_eq!(
        evidence["factory_v3_compatibility"]["factory_v3_drives_workflow"],
        false
    );
    let task_graph: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sdd_task_graph).unwrap()).unwrap();
    assert_eq!(task_graph["execution_mode"], "aggregate_provider");
    assert_eq!(
        task_graph["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(evidence["task_executions"][0]["task_id"], "step_scaffold");
    assert_eq!(task_graph["task_executions"][0]["task_id"], "step_scaffold");
    assert_eq!(task_graph["task_executions"][1]["task_id"], "step_tests");
    assert_eq!(task_graph["task_executions"][2]["task_id"], "step_release");
    assert_eq!(
        task_graph["task_executions"][1]["dependency_prerequisites"],
        serde_json::json!(["step_scaffold"])
    );
    assert_eq!(
        task_graph["task_executions"][2]["dependency_prerequisites"],
        serde_json::json!(["step_tests"])
    );
    assert_eq!(
        task_graph["task_executions"][0]["closure_status"],
        "accepted"
    );
    assert_eq!(
        task_graph["task_executions"][0]["provider_summary_refs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        task_graph["task_executions"][0]["sandbox_patch_refs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!task_graph["task_executions"][0]["event_refs"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn ao2_run_spec_provider_prompt_file_can_extend_scripted_task_execution() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let spec = temp.path().join("ao2-run.yaml");
    let prompt = temp.path().join("operator-prompt.sh");
    let spec_text = fs::read_to_string(fixture("expected-ao2-runspec.yaml"))
        .unwrap()
        .replace(
            "repo_path: /tmp/repo-full",
            &format!("repo_path: {}", repo.display()),
        )
        .replace("cargo test --release", "python -m pytest")
        .replace(
            "cargo clippy --workspace -- -D warnings",
            "python -m pytest",
        );
    fs::write(&spec, spec_text).unwrap();
    fs::write(
        &prompt,
        "mkdir -p docs\nprintf 'provider extension ran\\n' > docs/ao2-sdd-provider-extension.txt\nprintf 'Changed files: docs/ao2-sdd-provider-extension.txt\\n'\n",
    )
    .unwrap();

    let output = ao2(
        [
            "run",
            "--spec",
            spec.to_str().unwrap(),
            "--provider",
            "scripted",
            "--provider-prompt-file",
            prompt.to_str().unwrap(),
            "--run-id",
            "sdd-provider-prompt-file-run",
        ],
        [],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(repo.join("docs/ao2-sdd-provider-extension.txt").is_file());
    let stdout = stdout(&output);
    let evidence_pack = value_for(&stdout, "evidence_pack=");
    let evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(evidence_pack).unwrap()).unwrap();
    let changed_files = evidence["provider_summaries"][0]["changed_files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        changed_files.contains(&"docs/ao2-sdd-provider-extension.txt"),
        "{changed_files:?}"
    );
    assert_eq!(
        evidence["factory_v3_compatibility"]["control_plane_role"],
        "read_only_observer"
    );
}

fn fixture(name: &str) -> PathBuf {
    planner_crate_dir().join("tests/fixtures").join(name)
}

fn mock_bins() -> PathBuf {
    planner_crate_dir().join("tests/mock-bins")
}

fn planner_crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../sdd-planner")
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

fn ao2<const N: usize, const M: usize>(
    args: [&str; N],
    env: [(&str, &str); M],
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.envs(env);
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn prepend_path(bin: &Path) -> String {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = std::env::split_paths(&current).collect::<Vec<_>>();
    paths.insert(0, bin.to_path_buf());
    std::env::join_paths(paths)
        .unwrap()
        .to_string_lossy()
        .to_string()
}

fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn value_for<'a>(stdout: &'a str, prefix: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing {prefix} in {stdout}"))
}
