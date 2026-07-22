#![allow(dead_code, unused_imports)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;

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

struct ProjectAppStepFixture {
    spec: PathBuf,
    target: PathBuf,
    prompt: PathBuf,
}

fn write_project_app_step_fixture(root: &Path, label: &str) -> ProjectAppStepFixture {
    let target = root.join(format!("{label}-target"));
    copy_fixture(Path::new("../../fixtures/missed-call-recovery"), &target);
    init_existing_git_repo(&target);
    let spec = root.join(format!("{label}-spec.md"));
    fs::write(
        &spec,
        format!(
            r#"# {label} Missed Call Step

Acceptance:
- The implementation models a missed-call lead as a LeadCapture record.
- The recovery message mentions the customer, business, requested service, and a reply path.
- Recent repeat callers are classified as hot with a score of at least 85.
- Leads without text consent return no recovery text.
- The verifier can run with `python -m pytest -q`.
"#
        ),
    )
    .unwrap();
    let tests_dir = target.join("tests");
    fs::create_dir_all(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("test_project_step.py"),
        r#"from missed_call_recovery.workflow import LeadCapture, build_recovery_message, classify_lead


def lead(**overrides):
    data = {
        "customer_name": "Riley",
        "phone": "530-555-0133",
        "missed_at_minutes_ago": 7,
        "repeat_calls_24h": 2,
        "service_requested": "emergency leak repair",
        "business_name": "Missed Call Recovery",
        "consent_to_text": True,
    }
    data.update(overrides)
    return LeadCapture(**data)


def test_project_step_message_and_score():
    message = build_recovery_message(lead())
    assert "Riley" in message
    assert "Missed Call Recovery" in message
    assert "emergency leak repair" in message
    assert "reply" in message.lower()
    classification = classify_lead(lead())
    assert classification["priority"] == "hot"
    assert classification["score"] >= 85


def test_project_step_opt_out():
    assert build_recovery_message(lead(consent_to_text=False)) == ""
"#,
    )
    .unwrap();
    let prompt = root.join(format!("{label}-provider-prompt.sh"));
    fs::write(
        &prompt,
        r#"cat > missed_call_recovery/workflow.py <<'PY'
from dataclasses import dataclass


@dataclass(frozen=True)
class LeadCapture:
    customer_name: str
    phone: str
    missed_at_minutes_ago: int
    repeat_calls_24h: int
    service_requested: str
    business_name: str
    consent_to_text: bool


def classify_lead(capture: LeadCapture) -> dict:
    score = 40
    reasons = []
    if capture.missed_at_minutes_ago <= 15:
        score += 30
        reasons.append("recent missed call")
    if capture.repeat_calls_24h >= 2:
        score += 30
        reasons.append("repeat caller")
    if any(word in capture.service_requested.lower() for word in ["emergency", "leak", "no heat", "water heater"]):
        score += 15
        reasons.append("urgent service request")
    score = min(score, 100)
    return {
        "priority": "hot" if score >= 85 else "standard",
        "score": score,
        "reason": ", ".join(reasons) or "baseline missed-call follow-up",
    }


def build_recovery_message(capture: LeadCapture) -> str:
    if not capture.consent_to_text:
        return ""
    return (
        f"Hi {capture.customer_name}, this is {capture.business_name}. "
        f"Sorry we missed your call about {capture.service_requested}. "
        "Reply here with a good time and our team will follow up."
    )
PY
printf 'Summary: project-run app step implemented missed-call recovery workflow\n'
printf 'Changed files: missed_call_recovery/workflow.py\n'
printf 'Input tokens: 37\n'
"#,
    )
    .unwrap();
    commit_all(&target, "project app-step fixture");
    ProjectAppStepFixture {
        spec,
        target,
        prompt,
    }
}

fn write_signed_project_plan_for_step_fixtures(
    root: &Path,
    project_spec: &Path,
    signing_key: &Path,
    plan_out: &Path,
    steps: &[(&str, &ProjectAppStepFixture)],
) {
    let project_root = root.join("generated-project-plan");
    let generated = ao2([
        "factory",
        "project-plan",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-direct-project",
        "--verifier-command",
        "python -m pytest -q",
        "--provider",
        "scripted",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-run-plan-test",
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(generated.status.success(), "{}", stderr(&generated));
    let mut plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plan_out).unwrap()).unwrap();
    for (index, (id, fixture)) in steps.iter().enumerate() {
        plan["app_steps"][index]["id"] = serde_json::Value::String((*id).to_string());
        plan["app_steps"][index]["title"] = serde_json::Value::String(format!("{id} app step"));
        plan["app_steps"][index]["spec"] =
            serde_json::Value::String(fixture.spec.display().to_string());
        plan["app_steps"][index]["target"] =
            serde_json::Value::String(fixture.target.display().to_string());
        plan["app_steps"][index]["verifier_command"] =
            serde_json::Value::String("python -m pytest -q".to_string());
        plan["app_steps"][index]["provider"] = serde_json::Value::String("scripted".to_string());
        plan["app_steps"][index]["provider_prompt_file"] =
            serde_json::Value::String(fixture.prompt.display().to_string());
    }
    fs::write(plan_out, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
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

fn commit_all(repo: &Path, message: &str) {
    assert!(Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(status.status.success());
    if status.stdout.is_empty() {
        return;
    }
    assert!(Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
}

fn read_test_http_request(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream.read(buffer).unwrap()
}

#[test]
fn cli_workbench_run_evidence_summary_api_reports_replay_score_and_provider_summary() {
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
printf 'Summary: workbench evidence summary fixed discount validation\n'
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
        "workbench-evidence-summary",
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
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/runs/evidence?token=test-token&run_id=workbench-evidence-summary HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let body = http_body(&response);
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.workbench-run-evidence-summary.v1"
    );
    assert_eq!(json["run_id"], "workbench-evidence-summary");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["verdict"], "accepted");
    assert_eq!(json["replay"]["digest_failures"], 0);
    assert_eq!(json["scorecard"]["present"], true);
    assert_eq!(
        json["scorecard"]["schema"],
        "ao2.provider-evidence-scorecard.v1"
    );
    assert!(json["scorecard"]["score"].as_u64().unwrap_or(0) >= 90);
    assert!(!json["provider_summaries"].as_array().unwrap().is_empty());
    assert!(!json["closures"].as_array().unwrap().is_empty());
    assert!(json["evidence_pack"]
        .as_str()
        .unwrap()
        .contains("evidence-pack.json"));
    assert!(json["run_record"]
        .as_str()
        .unwrap()
        .contains("run-record.json"));
    assert!(
        normalize_separators(json["static_report"].as_str().unwrap()).contains("report/index.html")
    );
    assert!(normalize_separators(json["cockpit"].as_str().unwrap()).contains("cockpit/index.html"));
    let report_sections = json["report_sections"].as_array().unwrap();
    for section in [
        "Local Run Record",
        "Static Export Evidence",
        "Evaluator Closure Evidence",
        "Replay Evidence",
    ] {
        assert!(report_sections
            .iter()
            .any(|item| item.as_str() == Some(section)));
    }
}

#[test]
fn cli_workbench_run_evidence_publish_api_posts_signed_pack() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp.path().join("workbench-evidence-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-evidence-publish",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cp_port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream = accept_test_connection(&listener, "signed evidence publish request");
        let mut buffer = [0_u8; 32768];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/evidence-pack/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema_version\":\"ao2.cp-evidence-pack-signed-upload.v1\""));
        assert!(request.contains("\"schema_version\":\"ao2.evidence-pack.v1\""));
        assert!(request.contains("\"run_id\":\"workbench-evidence-publish\""));
        assert!(request.contains("\"signer_id\":\"ao2-workbench\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"publishedpack","stored_at":"2026-05-20T00:00:00Z","ingested_schema_version":"ao2.evidence-pack.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

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
            "ao2-workbench",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "run_id=workbench-evidence-publish&control_plane_url=http://127.0.0.1:{cp_port}&api_token=cp-token"
    );
    let response = http_request(
        port,
        &format!(
            "POST /api/runs/evidence/publish?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    server.join().unwrap();
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.evidence-pack-control-plane-publish.v1"
    );
    assert_eq!(json["receipt"]["sha256"], "publishedpack");
    assert_eq!(
        json["detail_url"],
        format!("http://127.0.0.1:{cp_port}/api/v1/evidence-pack/publishedpack/detail")
    );
    assert_eq!(
        json["dashboard_url"],
        format!("http://127.0.0.1:{cp_port}/api/v1/evidence-pack/dashboard")
    );
    assert_eq!(json["signature"]["signer_id"], "ao2-workbench");
    assert_eq!(json["signature"]["signature_algorithm"], "RSA/SHA-256");
    assert!(
        json["signature"]["public_key_sha256"]
            .as_str()
            .unwrap()
            .len()
            >= 64
    );
    let detail_html = json["detail_html"].as_str().unwrap_or("");
    assert!(detail_html.contains("AO2 Evidence Publish Receipt"));
    assert!(detail_html.contains("publishedpack"));
    assert!(detail_html.contains("ao2-workbench"));
    assert!(detail_html.contains("Public key SHA256"));

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
            "--control-plane-url",
            &format!("http://127.0.0.1:{cp_port}"),
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let html_response = http_request(
        port,
        "GET /?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(child.wait().unwrap().success());
    assert!(html_response.contains("run-evidence-publish-form"));
    assert!(html_response.contains("/api/runs/evidence/publish"));
    assert!(html_response.contains("name=\"kind\""));
    assert!(html_response.contains("value=\"operator-packet\""));
    assert!(html_response.contains("run-evidence-open-published-detail-button"));
    assert!(html_response.contains("Open Verified Detail"));
    assert!(html_response.contains("openPublishedEvidenceDetail"));
    assert!(html_response.contains("runEvidenceOpenPublishedDetailButton.disabled = false"));
    assert!(html_response.contains(&format!(
        "data-default-control-plane-url=\"http://127.0.0.1:{cp_port}\""
    )));
    assert!(html_response.contains(&format!(
        "name=\"control_plane_url\" value=\"http://127.0.0.1:{cp_port}\""
    )));
    assert!(!html_response.contains("value=\"cp-token\""));
}

#[test]
fn cli_workbench_run_evidence_publish_api_posts_signed_operator_packet() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp
        .path()
        .join("workbench-operator-packet-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-operator-packet-publish",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cp_port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream =
            accept_test_connection(&listener, "signed operator packet publish request");
        let mut buffer = vec![0_u8; 1024 * 1024];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/operator-packet/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema_version\":\"ao2.cp-operator-packet-signed-upload.v1\""));
        assert!(request.contains("\"schema_version\":\"ao2.operator-evidence-packet.v1\""));
        assert!(request.contains("\"run_id\":\"workbench-operator-packet-publish\""));
        assert!(request.contains("\"signer_id\":\"ao2-workbench-operator\""));
        assert!(request.contains("\"operator_packet_b64\""));
        assert!(request.contains("\"signature_algorithm\":\"RSA/SHA-256\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"publishedoperatorpacket","stored_at":"2026-06-07T00:00:00Z","ingested_schema_version":"ao2.operator-evidence-packet.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

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
            "ao2-workbench-operator",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "kind=operator-packet&run_id=workbench-operator-packet-publish&control_plane_url=http://127.0.0.1:{cp_port}&api_token=cp-token"
    );
    let response = http_request(
        port,
        &format!(
            "POST /api/runs/evidence/publish?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    server.join().unwrap();
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.operator-packet-control-plane-publish.v1"
    );
    assert_eq!(json["receipt"]["sha256"], "publishedoperatorpacket");
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "ao2.operator-evidence-packet.v1"
    );
    assert_eq!(
        json["detail_url"],
        format!("http://127.0.0.1:{cp_port}/api/v1/operator-packet/publishedoperatorpacket/detail")
    );
    assert_eq!(
        json["dashboard_url"],
        format!("http://127.0.0.1:{cp_port}/api/v1/operator-packet/dashboard")
    );
    assert_eq!(json["signature"]["signer_id"], "ao2-workbench-operator");
    assert_eq!(json["signature"]["signature_algorithm"], "RSA/SHA-256");
}

#[test]
fn cli_workbench_run_evidence_detail_proxy_fetches_control_plane_html() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cp_port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream = accept_test_connection(&listener, "signed evidence detail request");
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with(&format!("GET /api/v1/evidence-pack/{sha}/detail HTTP/1.1")));
        assert!(request.contains("Authorization: Bearer cp-token"));
        let body = format!(
            "<!doctype html><html><body><h1>AO2 Evidence Pack Detail</h1><code>{sha}</code><p>Verified</p></body></html>"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

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
            "--control-plane-url",
            &format!("http://127.0.0.1:{cp_port}"),
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body =
        format!("sha256={sha}&control_plane_url=http://127.0.0.1:{cp_port}&api_token=cp-token");
    let response = http_request(
        port,
        &format!(
            "POST /api/runs/evidence/detail?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    server.join().unwrap();
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.evidence-control-plane-detail.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{cp_port}/api/v1/evidence-pack/{sha}/detail")
    );
    assert!(json["detail_html"]
        .as_str()
        .unwrap()
        .contains("AO2 Evidence Pack Detail"));
    assert!(json["detail_html"].as_str().unwrap().contains(sha));

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
            "--control-plane-url",
            &format!("http://127.0.0.1:{cp_port}"),
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let html_response = http_request(
        port,
        "GET /?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(child.wait().unwrap().success());
    assert!(html_response.contains("run-evidence-detail-form"));
    assert!(html_response.contains("/api/runs/evidence/detail"));
    assert!(html_response.contains(&format!(
        "name=\"control_plane_url\" value=\"http://127.0.0.1:{cp_port}\""
    )));
    assert!(!html_response.contains("value=\"cp-token\""));
}

#[test]
fn cli_workbench_run_evidence_dashboard_proxy_opens_attention_filter() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cp_port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream = accept_test_connection(&listener, "signed evidence dashboard request");
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/evidence-pack/dashboard?gate=attention HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        let body = "<!doctype html><html><body><h1>AO2 Signed Evidence Packs</h1><p>Gate Attention</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

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
            "--control-plane-url",
            &format!("http://127.0.0.1:{cp_port}"),
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body =
        format!("control_plane_url=http://127.0.0.1:{cp_port}&api_token=cp-token&gate=attention");
    let response = http_request(
        port,
        &format!(
            "POST /api/runs/evidence/dashboard?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    server.join().unwrap();
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.evidence-control-plane-dashboard.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{cp_port}/api/v1/evidence-pack/dashboard?gate=attention")
    );
    assert!(json["dashboard_html"]
        .as_str()
        .unwrap()
        .contains("Gate Attention"));

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
            "--control-plane-url",
            &format!("http://127.0.0.1:{cp_port}"),
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let html_response = http_request(
        port,
        "GET /?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(child.wait().unwrap().success());
    assert!(html_response.contains("run-evidence-dashboard-form"));
    assert!(html_response.contains("/api/runs/evidence/dashboard"));
    assert!(html_response.contains("Open Gate Attention Dashboard"));
    assert!(!html_response.contains("value=\"cp-token\""));
}

#[test]
fn cli_workbench_run_evidence_summary_api_reports_obligation_ledger() {
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
printf 'Summary: obligation ledger surfaced in workbench evidence\n'
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
        "workbench-obligation-ledger",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let ledger_dir = repo
        .join(".ao2")
        .join("runs")
        .join("workbench-obligation-ledger")
        .join("evidence-pack");
    fs::write(
        ledger_dir.join("obligation-ledger.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-ledger.v1",
            "source_contracts": [],
            "obligations": [],
            "summary": {"pass": 2, "fail": 0, "unverified": 0, "waived": 0},
            "verdict": "accepted",
            "created_at": "2026-05-19T00:00:00Z"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        ledger_dir.join("obligation-gate-midpoint.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-gate.v1",
            "stage": "midpoint",
            "status": "passed",
            "verdict": "accepted",
            "summary": {"pass": 2, "fail": 0, "unverified": 0, "waived": 0}
        }))
        .unwrap(),
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
            "--once",
            "--api-token",
            "test-token",
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
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/runs/evidence?token=test-token&run_id=workbench-obligation-ledger HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let body = http_body(&response);
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["obligation_ledger"]["present"], true);
    assert_eq!(json["obligation_ledger"]["verdict"], "accepted");
    assert_eq!(json["obligation_ledger"]["summary"]["pass"], 2);
    assert!(json["obligation_ledger"]["path"]
        .as_str()
        .unwrap()
        .contains("obligation-ledger.json"));
    assert_eq!(json["obligation_gates"]["present"], true);
    assert_eq!(json["obligation_gates"]["count"], 1);
    assert_eq!(json["obligation_gates"]["gates"][0]["stage"], "midpoint");
    assert_eq!(json["obligation_gates"]["gates"][0]["status"], "passed");
}

#[test]
fn cli_workbench_run_evidence_summary_api_rejects_unknown_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

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
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/runs/evidence?token=test-token&run_id=missing-run HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let body = http_body(&response);
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-error.v1");
    assert!(json["error"].as_str().unwrap().contains("missing-run"));
}

#[test]
fn cli_workbench_run_evidence_summary_renders_summary_controls() {
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
printf 'Summary: workbench summary controls fixed discount validation\n'
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
        "workbench-summary-controls",
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

    assert!(html.contains("workbench-summary-controls"));
    assert!(html.contains("Run Evidence Summary"));
    assert!(html.contains("run-evidence-summary-output"));
    assert!(html.contains("data-action=\"evidence-summary\""));
    assert!(html.contains("obligation-annotation-form"));
    assert!(html.contains("/api/obligations/annotate"));
    assert!(html.contains("obligation-gate-form"));
    assert!(html.contains("/api/obligations/gate"));
    assert!(html.contains("data-obligation-gate-stage=\"midpoint\""));
    assert!(html.contains("data-obligation-gate-stage=\"closure\""));
    assert!(html.contains("obligation_gates"));
}

#[test]
fn cli_workbench_run_evidence_diff_api_compares_two_runs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let baseline = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-diff-baseline",
    ]);
    assert!(baseline.status.success(), "{}", stderr(&baseline));

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
printf 'Summary: workbench diff candidate fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let candidate = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-diff-candidate",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
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
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/runs/evidence/diff?token=test-token&left_run_id=workbench-diff-baseline&right_run_id=workbench-diff-candidate HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-run-evidence-diff.v1");
    assert_eq!(json["left"]["run_id"], "workbench-diff-baseline");
    assert_eq!(json["right"]["run_id"], "workbench-diff-candidate");
    assert_eq!(json["comparison"]["digest_failure_delta"], 0);
    assert!(json["right"]["provider_summary_count"].as_u64().unwrap() >= 1);
    assert!(
        json["comparison"]["provider_summary_delta"]
            .as_i64()
            .unwrap()
            >= 1
    );
    assert!(json["right"]["evidence_pack"]
        .as_str()
        .unwrap()
        .contains("evidence-pack.json"));
}

#[test]
fn cli_workbench_run_evidence_diff_api_rejects_unknown_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let baseline = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-diff-known",
    ]);
    assert!(baseline.status.success(), "{}", stderr(&baseline));

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
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/runs/evidence/diff?token=test-token&left_run_id=workbench-diff-known&right_run_id=missing-run HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-error.v1");
    assert!(json["error"].as_str().unwrap().contains("missing-run"));
}

#[test]
fn cli_workbench_run_evidence_changes_api_compares_previous_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let baseline = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-changes-baseline",
    ]);
    assert!(baseline.status.success(), "{}", stderr(&baseline));
    std::thread::sleep(std::time::Duration::from_millis(1100));

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
printf 'Summary: workbench changed evidence candidate fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let candidate = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-changes-candidate",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
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
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/runs/evidence/changes?token=test-token&run_id=workbench-changes-candidate HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.workbench-run-evidence-changes.v1"
    );
    assert_eq!(json["selected"]["run_id"], "workbench-changes-candidate");
    assert_eq!(json["baseline"]["run_id"], "workbench-changes-baseline");
    assert_eq!(
        json["diff"]["schema_version"],
        "ao2.workbench-run-evidence-diff.v1"
    );
    assert_eq!(json["diff"]["left"]["run_id"], "workbench-changes-baseline");
    assert_eq!(
        json["diff"]["right"]["run_id"],
        "workbench-changes-candidate"
    );
}

#[test]
fn cli_workbench_run_evidence_changes_api_rejects_without_previous_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-changes-only",
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
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/runs/evidence/changes?token=test-token&run_id=workbench-changes-only HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-error.v1");
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("no previous run found"));
}

#[test]
fn cli_workbench_run_evidence_diff_renders_controls() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    for run_id in ["workbench-diff-left", "workbench-diff-right"] {
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

    let export = ao2(["workbench", "export", "--target", repo.to_str().unwrap()]);
    assert!(export.status.success(), "{}", stderr(&export));
    let output = stdout(&export);
    let workbench_path = value_for(&output, "workbench=");
    let html = fs::read_to_string(workbench_path).unwrap();

    assert!(html.contains("Run Evidence Diff"));
    assert!(html.contains("run-evidence-diff-left"));
    assert!(html.contains("run-evidence-diff-right"));
    assert!(html.contains("run-evidence-diff-button"));
    assert!(html.contains("run-evidence-diff-output"));
    assert!(html.contains("/api/runs/evidence/diff"));
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

fn normalize_separators(input: &str) -> String {
    input.replace('\\', "/")
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

fn ao2_with_env<const N: usize, const M: usize>(
    args: [&str; N],
    env: [(&str, &str); M],
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.envs(env);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env(
        "AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER",
        "human:test-auto-approve",
    );
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn write_fake_codex(bin: &Path) {
    fs::create_dir_all(bin).unwrap();
    let unix = bin.join("codex");
    fs::write(
        &unix,
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--version" ]; then
  printf "codex fake 0.0.0\n"
  exit 0
fi
mkdir -p discount_service
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf "Summary: fake Codex provider smoke added validation around discount math\n"
printf "Changed files: discount_service/discounts.py\n"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unix, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        bin.join("codex.cmd"),
        r#"@echo off
if "%1"=="--version" (
  echo codex fake 0.0.0
  exit /b 0
)
if not exist discount_service mkdir discount_service
(
echo def calculate_discount(price: float, discount_rate: float^) -^> float:
echo     if price ^< 0:
echo         raise ValueError("price must be non-negative"^)
echo     if discount_rate ^< 0 or discount_rate ^> 1:
echo         raise ValueError("discount_rate must be between 0 and 1"^)
echo     return price * (1 - discount_rate^)
) > discount_service\discounts.py
echo Summary: fake Codex provider smoke added validation around discount math
echo Changed files: discount_service/discounts.py
"#,
    )
    .unwrap();
}

fn write_fake_claude(bin: &Path) {
    fs::create_dir_all(bin).unwrap();
    let unix = bin.join("claude");
    fs::write(
        &unix,
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--version" ]; then
  printf "claude fake 0.0.0\n"
  exit 0
fi
mkdir -p discount_service
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf "Summary: fake Claude provider smoke added validation around discount math\n"
printf "Changed files: discount_service/discounts.py\n"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unix, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        bin.join("claude.cmd"),
        r#"@echo off
if "%1"=="--version" (
  echo claude fake 0.0.0
  exit /b 0
)
if not exist discount_service mkdir discount_service
(
echo def calculate_discount(price: float, discount_rate: float^) -^> float:
echo     if price ^< 0:
echo         raise ValueError("price must be non-negative"^)
echo     if discount_rate ^< 0 or discount_rate ^> 1:
echo         raise ValueError("discount_rate must be between 0 and 1"^)
echo     return price * (1 - discount_rate^)
) > discount_service\discounts.py
echo Summary: fake Claude provider smoke added validation around discount math
echo Changed files: discount_service/discounts.py
"#,
    )
    .unwrap();
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

fn accept_test_connection(listener: &TcpListener, label: &str) -> TcpStream {
    let mut attempts = 0;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                attempts += 1;
                assert!(attempts <= 300, "timed out waiting for {label}");
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
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
