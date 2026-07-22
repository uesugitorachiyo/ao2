use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[test]
fn cli_workbench_support_bundle_summarizes_queue_failure_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let missing_prompt = temp.path().join("support-diagnostics-missing-prompt.sh");
    let signing_key = temp.path().join("support-diagnostics-key.pem");
    generate_native_signing_key(&signing_key, 3072);

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
    start_queue_job(port, "support-diagnostics-failed", &missing_prompt);
    let failed_job = wait_for_queue_job_status(port, "support-diagnostics-failed", "failed");
    assert_eq!(failed_job["diagnosis"]["failure_kind"], "non_zero_exit");

    let support_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(
        support_response.starts_with("HTTP/1.1 200 OK"),
        "{support_response}"
    );
    let support_export: serde_json::Value =
        serde_json::from_str(http_body(&support_response)).unwrap();
    let support_bundle_path = PathBuf::from(support_export["bundle_path"].as_str().unwrap());
    let support_bundle_dir = support_bundle_path.parent().unwrap().to_path_buf();
    let _ = child.kill();
    let _ = child.wait();

    let inspect = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        support_bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let inspect_json: serde_json::Value = serde_json::from_str(&stdout(&inspect)).unwrap();
    assert_eq!(inspect_json["support_metadata"]["signature_verified"], true);
    assert_eq!(inspect_json["queue_job_diagnosis_count"], 1);
    assert_eq!(
        inspect_json["queue_job_diagnoses"][0]["run_id"],
        "support-diagnostics-failed"
    );
    assert_eq!(
        inspect_json["queue_job_diagnoses"][0]["failure_kind"],
        "non_zero_exit"
    );
    assert!(inspect_json["queue_job_diagnoses"][0]["stderr_excerpt"]
        .as_str()
        .unwrap()
        .contains("support-diagnostics-missing-prompt.sh"));

    let inspect_text = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        support_bundle_dir.to_str().unwrap(),
    ]);
    assert!(inspect_text.status.success(), "{}", stderr(&inspect_text));
    let inspect_stdout = stdout(&inspect_text);
    assert!(inspect_stdout
        .contains("queue_job_diagnosis_1=support-diagnostics-failed non_zero_exit exit=1"));
    assert!(inspect_stdout.contains("error=queued ao2 run failed"));
    assert!(inspect_stdout.contains("recovery=Review stderr first"));

    let import_dir = temp.path().join("workbench-support-cases");
    let import = ao2([
        "workbench",
        "support-import",
        "--bundle-dir",
        support_bundle_dir.to_str().unwrap(),
        "--out-dir",
        import_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(import.status.success(), "{}", stderr(&import));
    let import_json: serde_json::Value = serde_json::from_str(&stdout(&import)).unwrap();
    assert_eq!(import_json["queue_job_diagnosis_count"], 1);
    let html = fs::read_to_string(import_json["index_path"].as_str().unwrap()).unwrap();
    assert!(html.contains("Queue Failure Diagnostics"));
    assert!(html.contains("Primary Error"));
    assert!(html.contains("Recovery"));
    assert!(html.contains("support-diagnostics-missing-prompt.sh"));
    assert!(html.contains("Review stderr first"));
}

#[test]
fn cli_workbench_support_verify_and_import_signed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let (_repo, bundle_dir) = create_signed_workbench_support_bundle(
        temp.path(),
        "support-verify-import",
        "workbench-lead",
    );

    let verify = ao2([
        "workbench",
        "support-verify",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.workbench-support-bundle-verify.v1"
    );
    assert_eq!(verify_json["verified"], true);
    assert_eq!(verify_json["queue_job_count"], 1);
    assert!(verify_json["audit_event_count"].as_u64().unwrap() >= 1);
    assert_eq!(verify_json["support_metadata"]["present"], true);
    assert_eq!(verify_json["support_metadata"]["signature_verified"], true);
    assert_eq!(
        verify_json["support_metadata"]["signer_id"],
        "workbench-lead"
    );

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
        import_json["schema_version"],
        "ao2.workbench-support-bundle-import.v1"
    );
    assert_eq!(import_json["verified"], true);
    assert_eq!(import_json["support_metadata"]["signature_verified"], true);
    let summary_path = PathBuf::from(import_json["summary_path"].as_str().unwrap());
    let index_path = PathBuf::from(import_json["index_path"].as_str().unwrap());
    let imported_bundle_dir = PathBuf::from(import_json["bundle_dir"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(index_path.is_file());
    assert!(imported_bundle_dir.join("support-bundle.json").is_file());
    let html = fs::read_to_string(index_path).unwrap();
    assert!(html.contains("Workbench Support Bundle"));
    assert!(html.contains("Support Bundle Trust"));
    assert!(html.contains("Signature verified"));
    assert!(html.contains("workbench-lead"));
}

#[test]
fn cli_workbench_support_inspect_and_import_render_evidence_exports() {
    let temp = tempfile::tempdir().unwrap();
    let (_repo, bundle_dir) = create_signed_workbench_support_bundle_with_evidence(
        temp.path(),
        "support-evidence-render",
        "evidence-lead",
    );

    let inspect_json = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect_json.status.success(), "{}", stderr(&inspect_json));
    let inspect: serde_json::Value = serde_json::from_str(&stdout(&inspect_json)).unwrap();
    assert_eq!(
        inspect["schema_version"],
        "ao2.workbench-support-bundle-inspect.v1"
    );
    assert_eq!(inspect["verified"], true);
    assert_eq!(inspect["evidence_export_count"], 1);
    assert_eq!(inspect["evidence_exports"][0]["kind"], "summary");
    assert_eq!(
        inspect["evidence_exports"][0]["run_id"],
        "support-evidence-render-run"
    );
    assert_eq!(
        inspect["evidence_exports"][0]["schema_version"],
        "ao2.workbench-evidence-export.v1"
    );
    assert_eq!(
        inspect["evidence_exports"][0]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    let inspect_text = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
    ]);
    assert!(inspect_text.status.success(), "{}", stderr(&inspect_text));
    let inspect_output = stdout(&inspect_text);
    assert!(inspect_output.contains("evidence_exports=1"));
    assert!(inspect_output.contains("evidence_export_1=summary support-evidence-render-run"));

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
    assert_eq!(import_json["evidence_export_count"], 1);
    assert_eq!(import_json["evidence_exports"][0]["kind"], "summary");
    assert_eq!(
        import_json["evidence_exports"][0]["run_id"],
        "support-evidence-render-run"
    );
    let html = fs::read_to_string(import_json["index_path"].as_str().unwrap()).unwrap();
    assert!(html.contains("Evidence Exports"));
    assert!(html.contains("support-evidence-render-run"));
    assert!(html.contains(
        import_json["evidence_exports"][0]["sha256"]
            .as_str()
            .unwrap()
    ));
}

#[test]
fn cli_workbench_support_latest_api_reports_packet_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let (repo, _bundle_dir) = create_signed_workbench_support_bundle_with_evidence(
        temp.path(),
        "support-packet-api",
        "packet-api-lead",
    );

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
    let response = http_request(
        port,
        "GET /api/support/latest?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-support-latest.v1");
    assert_eq!(json["present"], true);
    assert_eq!(json["queue_job_count"], 1);
    assert_eq!(json["evidence_export_count"], 1);
    assert_eq!(json["support_metadata"]["signature_verified"], true);
    assert_eq!(
        json["evidence_exports"][0]["run_id"],
        "support-packet-api-run"
    );
    let hermes_contract = &json["hermes_project_start_flow_contract"];
    assert_eq!(hermes_contract["present"], true);
    assert_eq!(
        hermes_contract["schema_version"],
        "ao2.hermes-project-start-flow-contract.v1"
    );
    assert_sha256_string(&hermes_contract["contract_sha256"], "contract_sha256");
    assert_eq!(hermes_contract["preview_role"], "viewer");
    assert_eq!(hermes_contract["publish_role"], "operator");
    assert_eq!(hermes_contract["raw_queue_json_scrape_required"], false);
    assert_eq!(hermes_contract["would_execute_queue"], false);
    assert_eq!(hermes_contract["would_submit_queue_entry"], false);
    assert_eq!(hermes_contract["would_rebuild_wrappers"], false);
    assert_eq!(hermes_contract["would_mutate_control_plane"], false);
    assert_eq!(
        hermes_contract["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(hermes_contract["control_plane_approves_release"], false);
    assert_eq!(hermes_contract["mutates_ao_artifacts"], false);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_support_inspect_reports_signed_trust_status() {
    let temp = tempfile::tempdir().unwrap();
    let (_repo, bundle_dir) =
        create_signed_workbench_support_bundle(temp.path(), "support-inspect", "support-lead");

    let inspect_json = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect_json.status.success(), "{}", stderr(&inspect_json));
    let inspect: serde_json::Value = serde_json::from_str(&stdout(&inspect_json)).unwrap();
    assert_eq!(
        inspect["schema_version"],
        "ao2.workbench-support-bundle-inspect.v1"
    );
    assert_eq!(inspect["verified"], true);
    assert_eq!(inspect["queue_job_count"], 1);
    assert_eq!(inspect["support_metadata"]["signature_verified"], true);
    assert_eq!(inspect["support_metadata"]["signer_id"], "support-lead");

    let inspect_text = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
    ]);
    assert!(inspect_text.status.success(), "{}", stderr(&inspect_text));
    let output = stdout(&inspect_text);
    assert!(output.contains("support_metadata=signature_verified"));
    assert!(output.contains("signer_id=support-lead"));
}

#[test]
fn cli_workbench_support_verify_rejects_tampered_signed_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let (_repo, bundle_dir) = create_signed_workbench_support_bundle(
        temp.path(),
        "support-tampered-metadata",
        "ops-lead",
    );

    tamper_json_file(
        &bundle_dir.join("support-bundle-metadata.json"),
        |metadata| {
            metadata["signer_id"] = serde_json::json!("attacker");
        },
    );

    let verify = ao2([
        "workbench",
        "support-verify",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!verify.status.success(), "{}", stdout(&verify));
    assert!(
        stderr(&verify).contains("workbench support bundle metadata signature verification failed"),
        "{}",
        stderr(&verify)
    );

    let import_dir = temp.path().join("tampered-workbench-support-cases");
    let import = ao2([
        "workbench",
        "support-import",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--out-dir",
        import_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!import.status.success(), "{}", stdout(&import));
    assert!(
        stderr(&import).contains("workbench support bundle metadata signature verification failed"),
        "{}",
        stderr(&import)
    );
    assert!(!import_dir.exists() || fs::read_dir(&import_dir).unwrap().next().is_none());
}

#[test]
fn cli_workbench_support_verify_rejects_tampered_bundle_body() {
    let temp = tempfile::tempdir().unwrap();
    let (_repo, bundle_dir) =
        create_signed_workbench_support_bundle(temp.path(), "support-tampered-body", "ops-lead");

    tamper_json_file(&bundle_dir.join("support-bundle.json"), |bundle| {
        bundle["audit_events"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "schema_version": "ao2.workbench-audit-event.v1",
                "timestamp_ms": 1,
                "action": "attacker",
                "job_id": "tampered"
            }));
    });

    let verify = ao2([
        "workbench",
        "support-verify",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!verify.status.success(), "{}", stdout(&verify));
    assert!(
        stderr(&verify).contains("workbench support bundle digest mismatch in signed metadata"),
        "{}",
        stderr(&verify)
    );
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

fn tamper_json_file(path: &Path, update: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    update(&mut value);
    fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
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

fn assert_sha256_string(value: &serde_json::Value, label: &str) {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("{label} must be a string: {value:#}"));
    assert_eq!(text.len(), 64, "{label} must be 64 hex chars: {text}");
    assert!(
        text.chars().all(|candidate| candidate.is_ascii_hexdigit()),
        "{label} must be hex: {text}"
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
