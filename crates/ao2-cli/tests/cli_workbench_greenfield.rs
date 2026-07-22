use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn cli_workbench_greenfield_spec_ingest_api_previews_read_only_packet() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_repo(&repo);
    let spec = temp.path().join("missed-call-recovery.md");
    fs::write(
        &spec,
        "# Missed Call Recovery\n\nAcceptance:\n- Captures missed-call leads.\n- Sends owner notification.\n- Shows recovery status.\n",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let request = format!(
        "GET /api/factory/greenfield-spec-ingest?token=test-token&spec={}&run_id=missed-call-recovery&verifier_command={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        percent_encode_for_test(spec.to_str().unwrap()),
        percent_encode_for_test("npm run verify")
    );
    let response = http_request(port, &request);
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-greenfield-spec-ingest.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["run_id"], "missed-call-recovery");
    assert_eq!(json["preflight"]["read_only"], true);
    assert_eq!(json["preflight"]["queue_submission_ready"], true);
    assert_eq!(json["side_effects"]["would_write_files"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["planned_ao2_producer_commands"][0]["command"],
        "ao2 factory project-plan"
    );
    assert_eq!(
        json["expected_artifact_schemas"][1],
        "ao2.factory-acceptance-rubric.v1"
    );
    assert!(!repo.join(".ao2/factory-project-start").exists());
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_greenfield_spec_ingest_submit_requires_operator_exact_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_repo(&repo);
    let spec = temp.path().join("missed-call-recovery.md");
    fs::write(
        &spec,
        "# Missed Call Recovery\n\nAcceptance:\n- Captures missed-call leads.\n- Sends owner notification.\n- Shows recovery status.\n",
    )
    .unwrap();
    let queue_path = repo.join(".ao2/factory-compat/queue.json");
    let body = format!(
        "spec={}&run_id=missed-call-recovery&verifier_command={}",
        percent_encode_for_test(spec.to_str().unwrap()),
        percent_encode_for_test("npm run verify")
    );

    let mut viewer_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "operator-token",
            "--operator-token",
            "viewer:viewer:viewer-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let viewer_port = read_server_port(&mut viewer_child);
    let viewer_response = http_request(
        viewer_port,
        &format!(
            "POST /api/factory/greenfield-spec-ingest/submit?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let viewer_status = viewer_child.wait().unwrap();
    assert!(viewer_status.success());
    assert!(
        viewer_response.starts_with("HTTP/1.1 403 Forbidden"),
        "{viewer_response}"
    );
    assert!(!queue_path.exists(), "viewer token must not submit queue");

    let mut missing_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "operator-token",
            "--operator-token",
            "viewer:viewer:viewer-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let missing_port = read_server_port(&mut missing_child);
    let missing_response = http_request(
        missing_port,
        &format!(
            "POST /api/factory/greenfield-spec-ingest/submit?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let missing_status = missing_child.wait().unwrap();
    assert!(missing_status.success());
    assert!(
        missing_response.starts_with("HTTP/1.1 400 Bad Request"),
        "{missing_response}"
    );
    let missing: serde_json::Value = serde_json::from_str(http_body(&missing_response)).unwrap();
    assert_eq!(
        missing["schema_version"],
        "ao2.factory-greenfield-spec-ingest-submit-approval.v1"
    );
    assert_eq!(missing["status"], "approval_required");
    assert_eq!(missing["approval_mode"], "exact_action_digest");
    assert_eq!(missing["required_form_field"], "approval_action_digest");
    assert_eq!(missing["action_digest"].as_str().unwrap().len(), 64);
    assert_eq!(missing["preflight"]["preflight"]["read_only"], true);
    assert!(!queue_path.exists(), "missing digest must not submit queue");
    let action_digest = missing["action_digest"].as_str().unwrap().to_string();

    let approved_body = format!("{body}&approval_action_digest={action_digest}");
    let mut ready_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "operator-token",
            "--operator-token",
            "viewer:viewer:viewer-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let ready_port = read_server_port(&mut ready_child);
    let ready_response = http_request(
        ready_port,
        &format!(
            "POST /api/factory/greenfield-spec-ingest/submit?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            approved_body.len(),
            approved_body
        ),
    );
    let ready_status = ready_child.wait().unwrap();
    assert!(ready_status.success());
    assert!(
        ready_response.starts_with("HTTP/1.1 200 OK"),
        "{ready_response}"
    );
    let ready: serde_json::Value = serde_json::from_str(http_body(&ready_response)).unwrap();
    assert_eq!(
        ready["schema_version"],
        "ao2.factory-greenfield-spec-ingest-submit.v1"
    );
    assert_eq!(ready["status"], "queued");
    assert_eq!(ready["run_id"], "missed-call-recovery");
    assert_eq!(ready["approval"]["status"], "approved_exact_action_digest");
    assert_eq!(ready["approval"]["action_digest"], action_digest);
    assert_eq!(
        ready["queue_submit"]["schema_version"],
        "ao2.factory-project-start-workbench-queue-submit.v1"
    );
    assert_eq!(
        ready["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        ready["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(ready["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(ready["side_effects"]["executed_provider"], false);
    assert_eq!(ready["side_effects"]["executed_queue"], false);
    assert_eq!(ready["side_effects"]["mutated_control_plane"], false);
    assert!(queue_path.exists(), "operator digest should submit queue");
    let queue: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(queue_path).unwrap()).unwrap();
    assert_eq!(queue["entries"].as_array().unwrap().len(), 1);
    assert_eq!(queue["entries"][0]["run_id"], "missed-call-recovery");
    assert_eq!(queue["entries"][0]["status"], "queued");
}

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

fn percent_encode_for_test(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}
