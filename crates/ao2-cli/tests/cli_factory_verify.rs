use std::fs;
use std::path::Path;
use std::process::Command;

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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_factory_verify_planning_evidence_accepts_signed_evidence_without_factory_driver() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("factory-planning-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: AO2 planning verifier\nobjective: Verify signed planning evidence without factory-v3 owning the decision.\n",
    )
    .unwrap();
    let out = temp.path().join("signed-plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "factory-compat-planner",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let planned: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    assert!(
        !Path::new(
            planned["signature"]["signed_payload_path"]
                .as_str()
                .unwrap()
        )
        .is_absolute(),
        "signed planning payload path should be bundle-relative"
    );
    assert!(
        !Path::new(planned["signature"]["signature_path"].as_str().unwrap()).is_absolute(),
        "planning signature path should be bundle-relative"
    );
    assert!(
        !Path::new(planned["signature"]["public_key_path"].as_str().unwrap()).is_absolute(),
        "planning public key path should be bundle-relative"
    );

    let verify = ao2([
        "factory",
        "verify-planning-evidence",
        "--evidence",
        planned["planning_evidence_path"].as_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verified: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(verified["status"], "accepted");
    assert_eq!(verified["signature_verified"], true);
    assert_eq!(verified["evidence_body_matches_signed_payload"], true);
    assert_eq!(verified["trust_boundary_ok"], true);
    assert_eq!(
        verified["planning_decision_contract"]["factory_v3_role"],
        "parity_oracle_only"
    );
}
