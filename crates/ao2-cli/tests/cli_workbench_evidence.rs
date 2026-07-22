use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[test]
fn cli_workbench_evidence_export_writes_summary_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
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
printf 'Summary: workbench export summary fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-export-summary",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let body = "kind=summary&run_id=workbench-export-summary";
    let request = format!(
        "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-evidence-export.v1");
    assert_eq!(json["export_kind"], "summary");
    assert_eq!(
        json["export"]["summary"]["run_id"],
        "workbench-export-summary"
    );
    let export_path = PathBuf::from(json["export_path"].as_str().unwrap());
    assert!(export_path.is_file(), "{}", export_path.display());
    let exported: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(export_path).unwrap()).unwrap();
    assert_eq!(
        exported["export"]["summary"]["run_id"],
        "workbench-export-summary"
    );
}

#[test]
fn cli_workbench_evidence_export_writes_operator_packet_for_support_readback() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp.path().join("operator-packet-support-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let prompt_path = temp.path().join("operator-packet-prompt.sh");
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
printf 'Summary: operator evidence packet fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
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
            "--enable-execution",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "operator-packet-lead",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "workbench-operator-packet", &prompt_path);
    wait_for_queue_job_status(port, "workbench-operator-packet", "accepted");

    let export_body = "kind=operator-packet&run_id=workbench-operator-packet";
    let evidence_export_request = format!(
        "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        export_body.len(),
        export_body
    );
    let evidence_export_response = http_request(port, &evidence_export_request);
    assert!(
        evidence_export_response.starts_with("HTTP/1.1 200 OK"),
        "{evidence_export_response}"
    );
    let evidence_export: serde_json::Value =
        serde_json::from_str(http_body(&evidence_export_response)).unwrap();
    assert_eq!(evidence_export["export_kind"], "operator-packet");
    let packet = &evidence_export["export"]["operator_packet"];
    assert_eq!(packet["schema_version"], "ao2.operator-evidence-packet.v1");
    assert_eq!(packet["run_id"], "workbench-operator-packet");
    assert_eq!(packet["run_record"]["run_id"], "workbench-operator-packet");
    assert_eq!(
        packet["evidence_pack"]["schema_version"],
        "ao2.evidence-pack.v1"
    );
    assert_eq!(packet["evaluator_closure"]["verdict"], "accepted");
    assert_eq!(packet["replay"]["status"], "accepted");
    assert_eq!(packet["provider_scorecard"]["present"], true);
    assert!(packet["provider_scorecard"]["score"].as_u64().unwrap_or(0) >= 90);
    assert!(
        packet["artifacts"]["run_record"]["sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert!(
        packet["artifacts"]["evidence_pack"]["sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert!(packet["artifacts"]["static_report"]["html"]
        .as_str()
        .unwrap()
        .contains("Evaluator Closure Evidence"));

    let support_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(support_response.starts_with("HTTP/1.1 200 OK"));
    let support_export: serde_json::Value =
        serde_json::from_str(http_body(&support_response)).unwrap();
    let bundle_path = PathBuf::from(support_export["bundle_path"].as_str().unwrap());
    let bundle_dir = bundle_path.parent().unwrap().to_path_buf();
    let _ = child.kill();
    let _ = child.wait();

    let verify = ao2([
        "workbench",
        "support-verify",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(verify_json["support_metadata"]["signature_verified"], true);
    assert_eq!(
        verify_json["evidence_exports"][0]["kind"],
        "operator-packet"
    );
    assert_eq!(
        verify_json["evidence_exports"][0]["operator_packet_run_id"],
        "workbench-operator-packet"
    );
    assert_eq!(
        verify_json["evidence_exports"][0]["operator_packet_schema_version"],
        "ao2.operator-evidence-packet.v1"
    );
    assert_eq!(
        verify_json["evidence_exports"][0]["operator_packet_closure_verdict"],
        "accepted"
    );
    assert_eq!(
        verify_json["evidence_exports"][0]["operator_packet_replay_status"],
        "accepted"
    );
    assert_eq!(
        verify_json["evidence_exports"][0]["operator_packet_provider_score_present"],
        true
    );
    assert_eq!(
        verify_json["evidence_exports"][0]["operator_packet_static_report_present"],
        true
    );

    let inspect_text = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
    ]);
    assert!(inspect_text.status.success(), "{}", stderr(&inspect_text));
    let inspect_output = stdout(&inspect_text);
    assert!(inspect_output.contains(
        "evidence_export_1=operator-packet workbench-operator-packet closure=accepted replay=accepted"
    ));

    let import_dir = temp.path().join("workbench-support-cases");
    let import = ao2([
        "workbench",
        "support-import",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--out-dir",
        import_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(import.status.success(), "{}", stderr(&import));
    let import_json: serde_json::Value = serde_json::from_str(&stdout(&import)).unwrap();
    assert_eq!(
        import_json["evidence_exports"][0]["kind"],
        "operator-packet"
    );
    assert_eq!(
        import_json["evidence_exports"][0]["operator_packet_run_id"],
        "workbench-operator-packet"
    );
    let html = fs::read_to_string(import_json["index_path"].as_str().unwrap()).unwrap();
    assert!(html.contains("operator-packet"));
    assert!(html.contains("workbench-operator-packet"));
}

#[test]
fn cli_workbench_evidence_export_writes_diff_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    for run_id in ["workbench-export-left", "workbench-export-right"] {
        let run = ao2([
            "run",
            "../../examples/risky-pr-run/risky-pr.yaml",
            "--target",
            repo.to_str().unwrap(),
            "--run-id",
            run_id,
        ]);
        assert!(run.status.success(), "{}", stderr(&run));
    }

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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let body = "kind=diff&left_run_id=workbench-export-left&right_run_id=workbench-export-right";
    let request = format!(
        "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-evidence-export.v1");
    assert_eq!(json["export_kind"], "diff");
    assert_eq!(
        json["export"]["diff"]["left"]["run_id"],
        "workbench-export-left"
    );
    assert_eq!(
        json["export"]["diff"]["right"]["run_id"],
        "workbench-export-right"
    );
    let export_path = PathBuf::from(json["export_path"].as_str().unwrap());
    assert!(export_path.is_file(), "{}", export_path.display());
}

#[test]
fn cli_workbench_evidence_export_writes_changes_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let baseline = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-export-changes-baseline",
    ]);
    assert!(baseline.status.success(), "{}", stderr(&baseline));
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let candidate = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-export-changes-candidate",
    ]);
    assert!(candidate.status.success(), "{}", stderr(&candidate));

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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let body = "kind=changes&run_id=workbench-export-changes-candidate";
    let request = format!(
        "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-evidence-export.v1");
    assert_eq!(json["export_kind"], "changes");
    assert_eq!(
        json["export"]["changes"]["selected"]["run_id"],
        "workbench-export-changes-candidate"
    );
    assert_eq!(
        json["export"]["changes"]["baseline"]["run_id"],
        "workbench-export-changes-baseline"
    );
    let export_path = PathBuf::from(json["export_path"].as_str().unwrap());
    assert!(export_path.is_file(), "{}", export_path.display());
    let exported: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(export_path).unwrap()).unwrap();
    assert_eq!(
        exported["export"]["changes"]["selected"]["run_id"],
        "workbench-export-changes-candidate"
    );
}

#[test]
fn cli_workbench_evidence_export_requires_operator_token() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-export-viewer",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

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
            "operator-token",
            "--operator-token",
            "viewer:viewer:viewer-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let body = "kind=summary&run_id=workbench-export-viewer";
    let request = format!(
        "POST /api/runs/evidence/export?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-error.v1");
    assert_eq!(json["error"], "insufficient_operator_role");
}

#[test]
fn cli_workbench_evidence_export_renders_controls() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-export-controls",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let export = ao2(["workbench", "export", "--target", repo.to_str().unwrap()]);
    assert!(export.status.success(), "{}", stderr(&export));
    let output = stdout(&export);
    let workbench_path = value_for(&output, "workbench=");
    let html = fs::read_to_string(workbench_path).unwrap();

    assert!(html.contains("Export Summary"));
    assert!(html.contains("Export Diff"));
    assert!(html.contains("Changed Since Previous"));
    assert!(html.contains("Export Changes"));
    assert!(html.contains("run-evidence-changes-button"));
    assert!(html.contains("run-evidence-export-output"));
    assert!(html.contains("/api/runs/evidence/changes"));
    assert!(html.contains("/api/runs/evidence/export"));
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

fn value_for<'a>(output: &'a str, prefix: &str) -> &'a str {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing prefix {prefix} in output:\n{output}"))
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

fn get_queue(port: u16) -> serde_json::Value {
    let response = http_request(
        port,
        "GET /api/queue?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(http_body(&response)).unwrap()
}

fn start_queue_job(port: u16, run_id: &str, prompt_path: &Path) -> serde_json::Value {
    let body = format!(
        "template=bug-fix&provider=scripted&run_id={run_id}&provider_prompt_file={}&max_repair_attempts=1",
        prompt_path.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(http_body(&response)).unwrap()
}

fn wait_for_queue_job_status(port: u16, run_id: &str, expected_status: &str) -> serde_json::Value {
    let mut last_job = None;
    for _ in 0..300 {
        let queue = get_queue(port);
        let job = queue["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["run_id"] == run_id)
            .cloned();
        if let Some(job) = job {
            if job["status"] == expected_status {
                return job;
            }
            last_job = Some(job);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!(
        "{}",
        queue_wait_timeout_message(run_id, expected_status, last_job.as_ref())
    );
}

fn queue_wait_timeout_message(
    run_id: &str,
    expected_status: &str,
    last_job: Option<&serde_json::Value>,
) -> String {
    let mut message = format!("job {run_id} did not reach status {expected_status}");
    let Some(job) = last_job else {
        message.push_str("; last_observed_job=none");
        return message;
    };

    let last_status = queue_wait_field(job, "status");
    message.push_str(&format!(
        "; last_status={}",
        if last_status.is_empty() {
            "<missing>"
        } else {
            &last_status
        }
    ));
    for field in ["exit_code", "error", "stdout_log", "stderr_log"] {
        let value = queue_wait_field(job, field);
        if !value.is_empty() {
            message.push_str(&format!("; {field}={value}"));
        }
    }
    message
}

fn queue_wait_field(job: &serde_json::Value, field: &str) -> String {
    match job.get(field) {
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(serde_json::Value::Number(value)) => value.to_string(),
        Some(serde_json::Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
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
