use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[test]
fn cli_workbench_launch_api_builds_governed_run_command() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp.path().join("support-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "test-token",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "workbench-governed-run-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let body = "template=bug-fix&provider=scripted&run_id=launch-demo&max_repair_attempts=2";
    let request = format!(
        "POST /api/launch?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-launch.v1");
    assert_eq!(json["status"], "ready");
    assert_eq!(json["mode"], "command_preview");
    assert_eq!(json["run_id"], "launch-demo");
    assert_eq!(json["launch_surface"], "factory-governed-run");
    assert_eq!(json["signing_key_required"], true);
    assert_eq!(json["signing_key_supplied"], true);
    assert_eq!(json["factory_v3_role"], "parity_oracle_only");
    assert_eq!(json["ao2_decision_owner"], "ao2-native-governed-run");
    assert_eq!(
        json["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    let warnings = json["provider_warnings"].as_array().unwrap();
    assert!(warnings
        .iter()
        .any(|warning| warning == "timeout_seconds=900"));
    assert!(warnings
        .iter()
        .any(|warning| warning == "execution_boundary=sandbox_copy_then_digest_patch"));
    let request_path = PathBuf::from(json["request_path"].as_str().unwrap());
    let runspec_path = PathBuf::from(json["runspec_path"].as_str().unwrap());
    assert!(request_path.is_file(), "{}", request_path.display());
    assert!(runspec_path.is_file(), "{}", runspec_path.display());
    assert!(fs::read_to_string(&request_path)
        .unwrap()
        .contains("factory governed-run"));
    assert!(fs::read_to_string(&runspec_path)
        .unwrap()
        .contains("verifier_command: python -m pytest -q"));
    let command = json["command"].as_array().unwrap();
    assert_eq!(command[0], "ao2");
    assert_eq!(command[1], "factory");
    assert_eq!(command[2], "governed-run");
    assert!(command.iter().any(|part| part == "--signing-key"));
    assert!(command
        .iter()
        .any(|part| part == signing_key.to_str().unwrap()));
    assert!(json["shell_command"]
        .as_str()
        .unwrap()
        .contains("ao2 factory governed-run"));
}

#[test]
fn cli_workbench_launch_api_preflights_real_ao_operator_runspec_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
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
  command: python3 -m pytest -q
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
inputs = ["verification evidence"]
outputs = ["acceptance decision"]
status_required = true
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("support-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "test-token",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "workbench-governed-run-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=scripted&run_id=launch-real-runspec&ao_operator_runspec={}",
        runspec.display()
    );
    let request = format!(
        "POST /api/launch?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["status"], "ready");
    assert_eq!(json["runspec_path"], runspec.to_str().unwrap());
    assert_eq!(
        json["role_contract_discovery"]["mode"],
        "auto_discovered_from_ao_runspec_layout"
    );
    assert_eq!(json["role_contract_discovery"]["loaded_count"], 2);
    assert_eq!(
        json["role_contract_discovery"]["missing_roles"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(json["launch_preflight"]["status"], "planned");
    assert_eq!(
        json["launch_preflight"]["ao2_auto_loaded_role_contracts"],
        true
    );
    assert!(Path::new(json["launch_preflight"]["plan_path"].as_str().unwrap()).is_file());
    let command = json["command"].as_array().unwrap();
    let runspec_arg_index = command
        .iter()
        .position(|part| part == "--runspec")
        .expect("--runspec arg");
    assert_eq!(command[runspec_arg_index + 1], runspec.to_str().unwrap());
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

fn read_server_port(child: &mut std::process::Child) -> u16 {
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line.trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap()
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn http_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}
