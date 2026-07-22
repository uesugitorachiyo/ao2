use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[test]
fn cli_workbench_export_builds_operator_dashboard() {
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
printf 'Summary: workbench demo fixed discount validation\n'
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
        "workbench-demo",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let export = ao2(["workbench", "export", "--target", repo.to_str().unwrap()]);
    assert!(export.status.success(), "{}", stderr(&export));
    let output = stdout(&export);
    let workbench_path = value_for(&output, "workbench=");
    let html = fs::read_to_string(workbench_path).unwrap();

    for needle in [
        "AO2 Workbench",
        "Run Queue",
        "workbench-demo",
        "Provider Health",
        "Provider Readiness",
        "Provider Score",
        "provider-score-ready",
        "ao2.provider-evidence-scorecard.v1",
        "Provider Safety Warnings",
        "role_contract_discovery",
        "ao2_auto_loaded_role_contracts",
        "provider-warning-output",
        "provider-readiness-data",
        "Run Provider Smoke",
        "provider-smoke-output",
        "provider-smoke-live-provider",
        "sandbox_copy_then_digest_patch",
        "900s",
        "provider cannot write target repo directly",
        "Provider Contracts",
        "phase_1",
        "AO2_LIVE_CODEX_SMOKE",
        "Provider Contract Verification",
        "contract-verification-output",
        "ao2.provider-contract-verification.v1",
        "status=verified",
        "Release Health",
        "release-health-release",
        "release-health-asset-dir",
        "release-health-provenance-dir",
        "release-health-output",
        "release-rollback-health",
        "/api/release-health",
        "ao2 doctor --json --release",
        "renderReleaseRollbackHealth",
        "rollback_status=",
        "macos-aarch64",
        "linux-x86_64",
        "windows-x86_64",
        "Release History",
        "release-history-dir",
        "release-history-refresh",
        "release-history-export",
        "release-history-output",
        "/api/release-history",
        "renderReleaseHistory",
        "Release Comparison Bundle",
        "release-comparison-out-dir",
        "release-comparison-generate",
        "release-comparison-verify",
        "release-comparison-latest",
        "release-comparison-export",
        "release-comparison-verification",
        "release-comparison-output",
        "release-retention-keep-releases",
        "release-retention-keep-bundles",
        "release-retention-preview",
        "release-retention-prune",
        "release-retention-output",
        "/api/release-comparison",
        "/api/release-comparison/latest",
        "/api/release-retention/prune",
        "renderReleaseComparisonVerification",
        "Project-Start Next Action",
        "project-start-next-action-run-id",
        "project-start-next-action-out-dir",
        "project-start-next-action-contract",
        "project-start-next-action-refresh",
        "project-start-next-action-output",
        "/api/factory/project-start/next-action",
        "refreshProjectStartNextAction",
        "project-start-operator-record-form",
        "project-start-operator-record-run-id",
        "project-start-operator-record-out-dir",
        "project-start-operator-record-contract",
        "project-start-operator-record-record-out",
        "project-start-operator-record-publish",
        "project-start-operator-record-output",
        "/api/factory/project-start/operator-record",
        "publishProjectStartOperatorRecord",
        "Project-Start Hermes Flow Contract",
        "project-start-hermes-flow-contract-form",
        "project-start-hermes-flow-contract-out",
        "project-start-hermes-flow-contract-refresh",
        "project-start-hermes-flow-contract-output",
        "/api/factory/project-start/hermes-flow-contract",
        "refreshProjectStartHermesFlowContract",
        "Task Templates",
        "ao2 upgrade apply --github-release",
    ] {
        assert!(html.contains(needle), "missing {needle}");
    }
}

#[test]
fn cli_workbench_export_renders_latest_support_bundle_trust() {
    let temp = tempfile::tempdir().unwrap();
    let (repo, bundle_dir) =
        create_signed_workbench_support_bundle(temp.path(), "support-trust-panel", "ops-lead");
    let verify = ao2([
        "workbench",
        "support-verify",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    let public_key_sha = verify_json["support_metadata"]["public_key_sha256"]
        .as_str()
        .unwrap()
        .to_string();

    let out = temp.path().join("workbench.html");
    let export = ao2([
        "workbench",
        "export",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let html = fs::read_to_string(out).unwrap();
    assert!(html.contains("Support Bundle Trust"));
    assert!(html.contains("Signature verified"));
    assert!(html.contains("ops-lead"));
    assert!(html.contains(&public_key_sha));
}

#[test]
fn cli_workbench_export_renders_latest_support_packet_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let (repo, _bundle_dir) = create_signed_workbench_support_bundle_with_evidence(
        temp.path(),
        "support-packet-panel",
        "packet-lead",
    );

    let out = temp.path().join("workbench.html");
    let export = ao2([
        "workbench",
        "export",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let html = fs::read_to_string(out).unwrap();
    assert!(html.contains("Latest Support Packet"));
    assert!(html.contains("Support Bundle Trust"));
    assert!(html.contains("Evidence Exports"));
    assert!(html.contains("support-packet-panel-run"));
    assert!(html.contains("packet-lead"));
    assert!(html.contains("Hermes Project-Start Flow Contract"));
    assert!(html.contains("ao2.hermes-project-start-flow-contract.v1"));
    assert!(html.contains("Preview Role"));
    assert!(html.contains("Publish Role"));
    assert!(html.contains("factory-v3 evaluator-closer"));
}

#[test]
fn cli_workbench_export_renders_latest_support_packet_queue_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let (repo, _bundle_dir) =
        create_signed_workbench_support_bundle_with_failed_job(temp.path(), "support-packet-diag");

    let out = temp.path().join("workbench.html");
    let export = ao2([
        "workbench",
        "export",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let html = fs::read_to_string(out).unwrap();
    assert!(html.contains("Latest Support Packet"));
    assert!(html.contains("Queue Failure Diagnostics"));
    assert!(html.contains("Primary Error"));
    assert!(html.contains("Recovery"));
    assert!(html.contains("support-packet-diag-failed"));
    assert!(html.contains("non_zero_exit"));
    assert!(html.contains("support-packet-diag-missing-prompt.sh"));
    assert!(html.contains("Review stderr first"));
}

fn create_signed_workbench_support_bundle(
    base: &Path,
    name: &str,
    signer_id: &str,
) -> (PathBuf, PathBuf) {
    create_signed_workbench_support_bundle_fixture(base, name, signer_id, false)
}

fn create_signed_workbench_support_bundle_with_evidence(
    base: &Path,
    name: &str,
    signer_id: &str,
) -> (PathBuf, PathBuf) {
    create_signed_workbench_support_bundle_fixture(base, name, signer_id, true)
}

fn create_signed_workbench_support_bundle_with_failed_job(
    base: &Path,
    name: &str,
) -> (PathBuf, PathBuf) {
    let repo = base.join(format!("{name}-repo"));
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = base.join(format!("{name}-support-signing-key.pem"));
    let missing_prompt = base.join(format!("{name}-missing-prompt.sh"));
    generate_native_signing_key(&signing_key, 2048);
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
            "diagnostics-lead",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, &format!("{name}-failed"), &missing_prompt);
    wait_for_queue_job_status(port, &format!("{name}-failed"), "failed");
    let export_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(export_response.starts_with("HTTP/1.1 200 OK"));
    let export: serde_json::Value = serde_json::from_str(http_body(&export_response)).unwrap();
    let bundle_path = PathBuf::from(export["bundle_path"].as_str().unwrap());
    let bundle_dir = bundle_path.parent().unwrap().to_path_buf();
    let _ = child.kill();
    let _ = child.wait();
    (repo, bundle_dir)
}

fn create_signed_workbench_support_bundle_fixture(
    base: &Path,
    name: &str,
    signer_id: &str,
    export_evidence: bool,
) -> (PathBuf, PathBuf) {
    let repo = base.join(format!("{name}-repo"));
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = base.join(format!("{name}-support-signing-key.pem"));
    let prompt_path = base.join(format!("{name}-prompt.sh"));
    generate_native_signing_key(&signing_key, 2048);
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
printf 'Summary: signed workbench support bundle fixture\n'
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
            signer_id,
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, &format!("{name}-run"), &prompt_path);
    wait_for_workbench_support_fixture_job(port, &format!("{name}-run"));
    if export_evidence {
        let evidence_body = format!("kind=summary&run_id={name}-run");
        let evidence_request = format!(
            "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            evidence_body.len(),
            evidence_body
        );
        let evidence_response = http_request(port, &evidence_request);
        assert!(
            evidence_response.starts_with("HTTP/1.1 200 OK"),
            "{evidence_response}"
        );
    }
    let export_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(export_response.starts_with("HTTP/1.1 200 OK"));
    let export: serde_json::Value = serde_json::from_str(http_body(&export_response)).unwrap();
    let bundle_path = PathBuf::from(export["bundle_path"].as_str().unwrap());
    let bundle_dir = bundle_path.parent().unwrap().to_path_buf();
    let _ = child.kill();
    let _ = child.wait();
    (repo, bundle_dir)
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
    wait_for_queue_job_status_with_attempts(port, run_id, expected_status, 300)
}

fn wait_for_workbench_support_fixture_job(port: u16, run_id: &str) -> serde_json::Value {
    let attempts = if cfg!(windows) { 900 } else { 300 };
    wait_for_queue_job_status_with_attempts(port, run_id, "accepted", attempts)
}

fn wait_for_queue_job_status_with_attempts(
    port: u16,
    run_id: &str,
    expected_status: &str,
    attempts: usize,
) -> serde_json::Value {
    let mut last_job = None;
    for _ in 0..attempts {
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
