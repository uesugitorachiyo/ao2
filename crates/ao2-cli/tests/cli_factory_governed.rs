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
fn cli_factory_governed_run_reports_auto_discovered_role_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let factory_repo = temp.path().join("factory-v3");
    let runspec = factory_repo.join("ao/runspecs/factory-v3-smoke.yaml");
    fs::create_dir_all(runspec.parent().unwrap()).unwrap();
    fs::write(
        &runspec,
        r#"apiVersion: ao.dev/v1
kind: Run
metadata:
  name: factory-v3-smoke
verifier:
  command: python -m pytest -q
spec:
  tasks:
    - id: planner-intake
      kind: agent
      deps: []
      spec:
        provider: codex
    - id: evaluator-closer
      kind: agent
      deps: ["planner-intake"]
      spec:
        provider: codex
"#,
    )
    .unwrap();
    let agents = factory_repo.join("agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(
        agents.join("intake.toml"),
        r#"name = "intake"
description = "Captures and classifies raw user intent."
inputs = ["user intent"]
outputs = ["intake brief"]
status_required = true
"#,
    )
    .unwrap();
    fs::write(
        agents.join("evaluator-closer.toml"),
        r#"name = "evaluator-closer"
description = "Validates final artifacts against evidence."
inputs = ["hardened plan", "verification evidence"]
outputs = ["acceptance decision", "closure evidence"]
status_required = true
"#,
    )
    .unwrap();
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 auto-contract governed run
objective: Execute factory-v3-compatible governed work with role contracts discovered by AO2.
acceptance:
  - AO2 reports role contract discovery in the governed-run checklist.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("governed-auto-contract-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("governed-auto-contract-out");

    let governed = ao2([
        "factory",
        "governed-run",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "governed-auto-contract-run",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "governed-auto-contract-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(governed.status.success(), "{}", stderr(&governed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&governed)).unwrap();
    assert_eq!(json["status"], "accepted");
    assert_eq!(
        json["governed_run_checklist"]["ao2_auto_loaded_role_contracts"],
        true
    );
    assert_eq!(
        json["plan"]["ao2_native_plan"]["role_contract_discovery"]["mode"],
        "auto_discovered_from_ao_runspec_layout"
    );
    assert_eq!(
        json["plan"]["ao2_native_plan"]["role_contract_discovery"]["loaded_count"],
        2
    );
    assert_eq!(
        json["evaluator_decision_verification"]["status"],
        "accepted"
    );
}

#[test]
fn cli_factory_governed_run_supports_provider_backed_execution() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 provider-backed production governed run
objective: Execute factory-v3-compatible governed work through AO2 provider adapters without a smoke-only wrapper.
acceptance:
  - AO2 executes provider-backed work, signs evaluator closure, and keeps factory-v3 as parity oracle only.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: governed-provider-run
verifier:
  command: python -m pytest -q
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
printf 'Summary: provider-backed governed run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 13\n'
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("governed-provider-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("governed-provider-out");

    let governed = ao2([
        "factory",
        "governed-run",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "governed-provider-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "governed-provider-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(governed.status.success(), "{}", stderr(&governed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&governed)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-compat-governed-run.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["provider_execution"]["provider"], "scripted");
    assert_eq!(json["provider_execution"]["mode"], "provider-backed");
    assert_eq!(
        json["governed_run_checklist"]["ao2_provider_backed_governed_workflow"],
        true
    );
    assert_eq!(
        json["queue_run_next"]["entry"]["provider_execution"]["provider"],
        "scripted"
    );
    assert_eq!(
        json["evaluator_decision_verification"]["status"],
        "accepted"
    );
    let evidence =
        fs::read_to_string(json["pack_evidence"]["evidence_pack_out"].as_str().unwrap()).unwrap();
    assert!(evidence.contains("provider_prompt_transcript"));
    assert!(evidence.contains("provider-backed governed run fixed discount validation"));
}

fn generate_native_signing_key(path: &Path, bits: usize) {
    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["workbench", "support-keygen", "--out"])
        .arg(path)
        .args(["--bits", &bits.to_string()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        path.is_file(),
        "native signing key exists: {}",
        path.display()
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
