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
fn cli_workbench_provider_contract_api_reports_verification_status() {
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
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/provider-contracts?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");

    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-contract-verification.v1");
    assert_eq!(json["status"], "verified");
    assert_eq!(
        json["required_providers"],
        serde_json::json!(["codex", "claude", "antigravity"])
    );
    assert!(json["reasons"].as_array().unwrap().is_empty());
}

#[test]
fn cli_workbench_provider_pilot_acceptance_export_attaches_to_signed_support_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_bundle = temp.path().join("provider-pilot-acceptance.json");
    fs::write(
        &acceptance_bundle,
        serde_json::json!({
            "schema_version": "ao2.codex-provider-pilot-acceptance.v1",
            "status": "passed",
            "provider": "codex",
            "run_id": "live-codex-provider-pilot",
            "evidence_pack": temp.path().join("evidence-pack.json"),
            "cockpit": temp.path().join("cockpit").join("index.html"),
            "replay": {
                "status": "accepted",
                "event_count": 33,
                "artifact_count": 13,
                "digest_failures": []
            },
            "score": {
                "schema": "ao2.provider-evidence-scorecard.v1",
                "score": 100,
                "max_score": 100,
                "verdict": "ready",
                "run_id": "live-codex-provider-pilot",
                "replay": {
                    "status": "accepted",
                    "digest_failures": 0
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let signing_key = temp.path().join("provider-pilot-support-key.pem");
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
            "provider-pilot-export-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let export_body = format!(
        "kind=provider-pilot-acceptance&acceptance_bundle={}",
        acceptance_bundle.display()
    );
    let evidence_export_response = http_request(
        port,
        &format!(
            "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            export_body.len(),
            export_body
        ),
    );
    assert!(
        evidence_export_response.starts_with("HTTP/1.1 200 OK"),
        "{evidence_export_response}"
    );
    let evidence_export: serde_json::Value =
        serde_json::from_str(http_body(&evidence_export_response)).unwrap();
    assert_eq!(evidence_export["export_kind"], "provider-pilot-acceptance");
    assert_eq!(
        evidence_export["export"]["provider_pilot_acceptance"]["status"],
        "passed"
    );
    assert_eq!(
        evidence_export["export"]["provider_pilot_acceptance"]["provider"],
        "codex"
    );
    assert_eq!(
        evidence_export["export"]["provider_pilot_acceptance"]["score"]["score"],
        100
    );
    let export_path = PathBuf::from(evidence_export["export_path"].as_str().unwrap());
    assert!(export_path.is_file(), "{}", export_path.display());

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
    assert_eq!(
        support_export["support_metadata"]["metadata"]["evidence_export_count"],
        1
    );
    let support_bundle_path = PathBuf::from(support_export["bundle_path"].as_str().unwrap());
    let support_bundle_dir = support_bundle_path.parent().unwrap().to_path_buf();
    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&support_bundle_path).unwrap()).unwrap();
    assert_eq!(
        bundle["evidence_exports"][0]["kind"],
        "provider-pilot-acceptance"
    );
    assert_eq!(
        bundle["evidence_exports"][0]["content"]["export"]["provider_pilot_acceptance"]["run_id"],
        "live-codex-provider-pilot"
    );
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
    assert_eq!(
        inspect_json["evidence_exports"][0]["kind"],
        "provider-pilot-acceptance"
    );
    assert_eq!(
        inspect_json["evidence_exports"][0]["provider_pilot_provider"],
        "codex"
    );
    assert_eq!(
        inspect_json["evidence_exports"][0]["provider_pilot_run_id"],
        "live-codex-provider-pilot"
    );
    assert_eq!(
        inspect_json["evidence_exports"][0]["provider_pilot_score"],
        100
    );
    assert_eq!(
        inspect_json["evidence_exports"][0]["provider_pilot_replay_status"],
        "accepted"
    );
    assert_eq!(
        inspect_json["evidence_exports"][0]["provider_pilot_digest_failure_count"],
        0
    );

    let inspect_text = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        support_bundle_dir.to_str().unwrap(),
    ]);
    assert!(inspect_text.status.success(), "{}", stderr(&inspect_text));
    assert!(stdout(&inspect_text)
        .contains("evidence_export_1=provider-pilot-acceptance codex live-codex-provider-pilot score=100 replay=accepted digest_failures=0"));
}

#[test]
fn cli_workbench_provider_smoke_api_runs_when_execution_enabled() {
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
            "--enable-execution",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = "minimum_score=90";
    let request = format!(
        "POST /api/provider-smoke?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-smoke-all.v1");
    assert_eq!(json["minimum_score"], 90);
    assert_eq!(
        PathBuf::from(json["history_path"].as_str().unwrap()),
        repo.join(".ao2")
            .join("provider-smoke")
            .join("history.json")
    );
    assert_eq!(json["history_entry_count"], 1);
    let scripted = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "scripted")
        .expect("scripted provider should be present");
    assert_eq!(scripted["verdict"], "ready");
    assert!(repo
        .join(".ao2")
        .join("provider-smoke")
        .join("history.json")
        .is_file());
}

#[test]
fn cli_workbench_provider_smoke_api_runs_live_codex_when_explicitly_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);

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
            "--enable-execution",
        ])
        .env("PATH", path)
        .env("AO2_LIVE_CODEX_SMOKE", "1")
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = "minimum_score=90&live_provider=codex";
    let request = format!(
        "POST /api/provider-smoke?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    let codex = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "codex")
        .expect("codex provider should be present");
    assert_eq!(codex["verdict"], "ready");
    assert!(codex["run_id"]
        .as_str()
        .unwrap()
        .starts_with("provider-smoke-codex-"));
    assert!(repo.join(".ao2/provider-smoke/history.json").is_file());
}

#[test]
fn cli_workbench_provider_smoke_api_runs_live_claude_when_explicitly_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_claude(&bin);
    let path = prepend_path(&bin);

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
            "--enable-execution",
        ])
        .env("PATH", path)
        .env("AO2_LIVE_CLAUDE_SMOKE", "1")
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = "minimum_score=90&live_provider=claude";
    let request = format!(
        "POST /api/provider-smoke?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    let claude = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "claude")
        .expect("claude provider should be present");
    assert_eq!(claude["verdict"], "ready");
    assert!(claude["run_id"]
        .as_str()
        .unwrap()
        .starts_with("provider-smoke-claude-"));
    assert!(repo.join(".ao2/provider-smoke/history.json").is_file());
}

#[test]
fn cli_workbench_provider_pilot_api_blocks_when_gate_not_ready() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

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
    let body = format!(
        "template=bug-fix&provider=codex&provider_prompt_file={}&minimum_score=90",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["status"], "blocked");
    assert_eq!(json["provider"], "codex");
    assert_eq!(json["gate"]["verdict"], "not_ready");
    assert_eq!(json["shell_command"], "");
}

#[test]
fn cli_workbench_provider_pilot_api_builds_command_after_gate_passes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CODEX_SMOKE", "1")],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));
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
    let body = format!(
        "template=bug-fix&provider=codex&run_id=workbench-provider-pilot&provider_prompt_file={}&max_repair_attempts=1&minimum_score=90",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["status"], "ready");
    assert_eq!(json["mode"], "command_preview");
    assert_eq!(json["provider"], "codex");
    assert_eq!(json["template"], "bug-fix");
    assert_eq!(json["run_id"], "workbench-provider-pilot");
    assert_eq!(json["gate"]["verdict"], "ready");
    assert!(repo.join(".ao2/generated-workflows/bug-fix.yaml").is_file());
    let shell_command = json["shell_command"].as_str().unwrap();
    assert!(shell_command.contains("ao2 run --template bug-fix"));
    assert!(shell_command.contains("--provider codex"));
    assert!(shell_command.contains("--provider-prompt-file"));
}

#[test]
fn cli_workbench_provider_pilot_api_requires_operator_token() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

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
    let body = format!(
        "template=bug-fix&provider=codex&provider_prompt_file={}",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["error"], "insufficient_operator_role");
}

#[test]
fn cli_workbench_provider_pilot_preflight_reports_invalid_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let missing_prompt = temp.path().join("missing-pilot-prompt.txt");

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
    let body = format!(
        "template=bug-fix&provider=codex&provider_prompt_file={}&minimum_score=90",
        missing_prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot/preflight?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.workbench-provider-pilot-preflight.v1"
    );
    assert_eq!(json["status"], "invalid");
    assert_eq!(json["can_start"], false);
    assert_eq!(json["pilot"], serde_json::Value::Null);
    assert!(json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["name"] == "prompt_file" && check["status"] == "failed" }));
    assert!(json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["name"] == "provider_gate" && check["status"] == "not_applicable" }));
}

#[test]
fn cli_workbench_provider_pilot_preflight_blocks_when_gate_not_ready() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

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
    let body = format!(
        "template=bug-fix&provider=codex&provider_prompt_file={}&minimum_score=90",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot/preflight?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["status"], "blocked");
    assert_eq!(json["can_start"], false);
    assert_eq!(json["pilot"]["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["pilot"]["status"], "blocked");
    assert_eq!(json["pilot"]["gate"]["verdict"], "not_ready");
    assert!(json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["name"] == "provider_gate" && check["status"] == "blocked" }));
}

#[test]
fn cli_workbench_provider_pilot_preflight_passes_after_gate_ready() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CODEX_SMOKE", "1")],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));

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
        .env("PATH", path)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=codex&run_id=workbench-provider-pilot-preflight&provider_prompt_file={}&minimum_score=90",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot/preflight?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["status"], "ready");
    assert_eq!(json["can_start"], true);
    assert_eq!(json["pilot"]["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["pilot"]["status"], "ready");
    assert_eq!(json["pilot"]["gate"]["verdict"], "ready");
    assert_eq!(
        json["pilot"]["run_id"],
        "workbench-provider-pilot-preflight"
    );
    assert!(json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["name"] == "provider_gate" && check["status"] == "passed" }));
}

#[test]
fn cli_workbench_provider_pilot_preflight_requires_operator_token() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

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
    let body = format!(
        "template=bug-fix&provider=codex&provider_prompt_file={}",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot/preflight?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["error"], "insufficient_operator_role");
}

#[test]
fn cli_workbench_provider_pilot_renders_preflight_control() {
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
    let port = read_server_port(&mut child);

    let html = http_request(
        port,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(html.contains("Preflight Provider Pilot"));
    assert_eq!(
        html.matches("<option value=\"antigravity\">Antigravity</option>")
            .count(),
        2
    );
    assert!(html.contains("provider-pilot-preflight-button"));
    assert!(html.contains("provider-pilot-max-budget-usd"));
    assert!(html.contains("provider-pilot-acceptance-bundle"));
    assert!(html.contains("provider-pilot-acceptance-provider"));
    assert!(html.contains("provider-pilot-acceptance-replay-status"));
    assert!(html.contains("provider-pilot-acceptance-min-score"));
    assert!(html.contains("provider-pilot-acceptance-sort"));
    assert!(html.contains("provider-pilot-acceptance-limit"));
    assert!(html.contains("provider-pilot-acceptance-latest-button"));
    assert!(html.contains("provider-pilot-acceptance-export-button"));
    assert!(html.contains("provider-pilot-acceptance-export-latest-button"));
    assert!(html.contains("provider-pilot-cost-ledger-button"));
    assert!(html.contains("provider-pilot-cost-trend-button"));
    assert!(html.contains("provider-pilot-cost-trend-chart"));
    assert!(html.contains("renderProviderPilotCostTrendChart"));
    assert!(html.contains("Provider pilot cost trend chart"));
    assert!(html.contains("/api/provider-pilot/acceptance/latest"));
    assert!(html.contains("/api/provider-pilot/acceptance/export-latest"));
    assert!(html.contains("/api/provider-pilot/cost-ledger"));
    assert!(html.contains("/api/provider-pilot/cost-trend"));
    assert!(html.contains("approval_action_digest"));
    assert!(html.contains("approved_exact_action_digest"));
    assert!(html.contains("kind: 'provider-pilot-acceptance'"));
    assert!(html.contains("acceptance_bundle"));
}

#[test]
fn cli_workbench_provider_pilot_latest_acceptance_api_returns_newest_valid_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_root = temp.path().join("acceptance-root");
    let stale_dir = acceptance_root.join("v0.4.54");
    let latest_dir = acceptance_root.join("v0.4.55");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::create_dir_all(&latest_dir).unwrap();
    fs::write(
        stale_dir.join("provider-pilot-acceptance.json"),
        serde_json::json!({
            "schema_version": "ao2.codex-provider-pilot-acceptance.v1",
            "status": "failed",
            "provider": "codex",
            "run_id": "stale-failed-codex-provider-pilot",
            "evidence_pack": temp.path().join("stale-evidence-pack.json"),
            "cockpit": temp.path().join("stale-cockpit").join("index.html"),
            "replay": {
                "status": "rejected",
                "digest_failures": ["digest mismatch"]
            },
            "score": {
                "score": 10,
                "max_score": 100,
                "verdict": "blocked"
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        latest_dir.join("provider-pilot-acceptance.json"),
        serde_json::json!({
            "schema_version": "ao2.claude-provider-pilot-acceptance.v1",
            "status": "passed",
            "provider": "claude",
            "run_id": "live-claude-provider-pilot",
            "evidence_pack": temp.path().join("evidence-pack.json"),
            "cockpit": temp.path().join("cockpit").join("index.html"),
            "replay": {
                "status": "accepted",
                "event_count": 31,
                "artifact_count": 12,
                "digest_failures": []
            },
            "score": {
                "schema": "ao2.provider-evidence-scorecard.v1",
                "score": 98,
                "max_score": 100,
                "verdict": "ready",
                "run_id": "live-claude-provider-pilot",
                "replay": {
                    "status": "accepted",
                    "digest_failures": 0
                }
            }
        })
        .to_string(),
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
    let port = read_server_port(&mut child);

    let response = http_request(
        port,
        &format!(
            "GET /api/provider-pilot/acceptance/latest?token=test-token&acceptance_root={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            acceptance_root.display()
        ),
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.workbench-latest-provider-pilot-acceptance.v1"
    );
    assert_eq!(
        json["acceptance_bundle"],
        latest_dir
            .join("provider-pilot-acceptance.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(json["acceptance"]["provider"], "claude");
    assert_eq!(json["acceptance"]["run_id"], "live-claude-provider-pilot");
    assert_eq!(json["acceptance"]["score"]["score"], 98);
    assert_eq!(json["candidates_checked"], 2);
    assert_eq!(json["failed_candidates"].as_array().unwrap().len(), 1);
    assert_eq!(json["acceptance_history"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["acceptance_history"][0]["acceptance_bundle"],
        latest_dir
            .join("provider-pilot-acceptance.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(json["acceptance_history"][0]["provider"], "claude");
    assert_eq!(json["acceptance_history"][0]["score"], 98);
    assert_eq!(json["acceptance_history"][0]["replay_status"], "accepted");
}

#[test]
fn cli_workbench_provider_pilot_latest_acceptance_api_reads_nested_release_provider_bundles() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_root = temp.path().join("acceptance-root");
    let antigravity_dir = acceptance_root.join("v0.4.80").join("antigravity");
    write_provider_pilot_acceptance_fixture(ProviderPilotAcceptanceFixture {
        dir: antigravity_dir.clone(),
        provider: "antigravity",
        run_id: "live-antigravity-provider-pilot",
        status: "passed",
        replay_status: "accepted",
        score: 100,
        verdict: "ready",
        evidence_root: temp.path(),
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let response = http_request(
        port,
        &format!(
            "GET /api/provider-pilot/acceptance/latest?token=test-token&provider=antigravity&acceptance_root={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            acceptance_root.display()
        ),
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["provider"], "antigravity");
    assert_eq!(json["run_id"], "live-antigravity-provider-pilot");
    assert_eq!(
        json["acceptance_bundle"],
        antigravity_dir
            .join("provider-pilot-acceptance.json")
            .to_str()
            .unwrap()
    );
}

#[test]
fn cli_workbench_provider_pilot_latest_acceptance_api_filters_sorts_and_limits_history() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_root = temp.path().join("acceptance-root");
    write_provider_pilot_acceptance_fixture(ProviderPilotAcceptanceFixture {
        dir: acceptance_root.join("v0.4.57"),
        provider: "codex",
        run_id: "low-score-codex-provider-pilot",
        status: "passed",
        replay_status: "accepted",
        score: 88,
        verdict: "ready",
        evidence_root: temp.path(),
    });
    write_provider_pilot_acceptance_fixture(ProviderPilotAcceptanceFixture {
        dir: acceptance_root.join("v0.4.58"),
        provider: "codex",
        run_id: "accepted-codex-provider-pilot",
        status: "passed",
        replay_status: "accepted",
        score: 96,
        verdict: "ready",
        evidence_root: temp.path(),
    });
    write_provider_pilot_acceptance_fixture(ProviderPilotAcceptanceFixture {
        dir: acceptance_root.join("v0.4.59"),
        provider: "claude",
        run_id: "accepted-claude-provider-pilot",
        status: "passed",
        replay_status: "accepted",
        score: 99,
        verdict: "ready",
        evidence_root: temp.path(),
    });
    write_provider_pilot_acceptance_fixture(ProviderPilotAcceptanceFixture {
        dir: acceptance_root.join("v0.4.60"),
        provider: "codex",
        run_id: "rejected-codex-provider-pilot",
        status: "failed",
        replay_status: "rejected",
        score: 97,
        verdict: "blocked",
        evidence_root: temp.path(),
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let response = http_request(
        port,
        &format!(
            "GET /api/provider-pilot/acceptance/latest?token=test-token&acceptance_root={}&provider=codex&history_replay_status=accepted&history_min_score=90&history_sort=score_desc&history_limit=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            acceptance_root.display()
        ),
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["provider"], "codex");
    assert_eq!(json["run_id"], "accepted-codex-provider-pilot");
    assert_eq!(json["score"], 96);
    assert_eq!(json["candidates_checked"], 4);
    assert_eq!(json["history_total_count"], 1);
    assert_eq!(json["acceptance_history"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["acceptance_history"][0]["run_id"],
        "accepted-codex-provider-pilot"
    );
    assert_eq!(json["acceptance_history"][0]["score"], 96);
    assert_eq!(json["acceptance_filter"]["provider"], "codex");
    assert_eq!(json["acceptance_filter"]["replay_status"], "accepted");
    assert_eq!(json["acceptance_filter"]["min_score"], 90);
    assert_eq!(json["acceptance_filter"]["sort"], "score_desc");
    assert_eq!(json["acceptance_filter"]["limit"], 1);
    assert_eq!(json["failed_candidates"].as_array().unwrap().len(), 3);
}

#[test]
fn cli_workbench_provider_pilot_latest_acceptance_api_reports_trend_regression() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_root = temp.path().join("acceptance-root");
    write_provider_pilot_acceptance_fixture(ProviderPilotAcceptanceFixture {
        dir: acceptance_root.join("v0.4.59"),
        provider: "codex",
        run_id: "strong-codex-provider-pilot",
        status: "passed",
        replay_status: "accepted",
        score: 99,
        verdict: "ready",
        evidence_root: temp.path(),
    });
    write_provider_pilot_acceptance_fixture(ProviderPilotAcceptanceFixture {
        dir: acceptance_root.join("v0.4.60"),
        provider: "codex",
        run_id: "regressed-codex-provider-pilot",
        status: "passed",
        replay_status: "accepted",
        score: 91,
        verdict: "ready",
        evidence_root: temp.path(),
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let response = http_request(
        port,
        &format!(
            "GET /api/provider-pilot/acceptance/latest?token=test-token&acceptance_root={}&provider=codex&history_replay_status=accepted HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            acceptance_root.display()
        ),
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["run_id"], "regressed-codex-provider-pilot");
    assert_eq!(
        json["acceptance_trend"]["schema_version"],
        "ao2.workbench-provider-pilot-acceptance-trend.v1"
    );
    assert_eq!(json["acceptance_trend"]["current_score"], 91);
    assert_eq!(json["acceptance_trend"]["previous_score"], 99);
    assert_eq!(json["acceptance_trend"]["score_delta"], -8);
    assert_eq!(json["acceptance_trend"]["regression"], true);
    assert_eq!(
        json["acceptance_trend"]["current_run_id"],
        "regressed-codex-provider-pilot"
    );
    assert_eq!(
        json["acceptance_trend"]["previous_run_id"],
        "strong-codex-provider-pilot"
    );
    assert_eq!(json["acceptance_trend"]["accepted_count"], 2);
    assert_eq!(json["acceptance_trend"]["best_score"], 99);
    assert_eq!(json["acceptance_trend"]["worst_score"], 91);
}

#[test]
fn cli_workbench_provider_pilot_cost_ledger_api_reports_budget_usage_totals() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_root = temp.path().join("acceptance-root");
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67"),
        "codex",
        "live-codex-provider-pilot",
        1.00,
        false,
        Some(1000),
        Some(500),
        Some(1500),
        Some(0.12),
    );
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67").join("claude"),
        "claude",
        "live-claude-provider-pilot",
        1.00,
        true,
        Some(2000),
        Some(750),
        Some(2750),
        Some(0.34),
    );

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

    let response = http_request(
        port,
        &format!(
            "GET /api/provider-pilot/cost-ledger?token=test-token&acceptance_root={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            acceptance_root.display()
        ),
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.provider-cost-ledger.v1");
    assert_eq!(json["entry_count"], 2);
    assert_eq!(json["totals"]["max_budget_usd"], 2.0);
    assert_eq!(json["totals"]["observed_cost_usd"], 0.46);
    assert_eq!(json["providers"]["claude"]["total_tokens"], 2750);
    assert_eq!(
        json["providers"]["codex"]["provider_enforced_budget"],
        false
    );
}

#[test]
fn cli_workbench_provider_pilot_cost_trend_api_reports_release_deltas() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_root = temp.path().join("acceptance-root");
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.66"),
        "codex",
        "old-codex-provider-pilot",
        1.00,
        false,
        Some(700),
        Some(300),
        Some(1000),
        Some(0.10),
    );
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67"),
        "codex",
        "live-codex-provider-pilot",
        1.00,
        false,
        Some(1000),
        Some(500),
        Some(1500),
        Some(0.12),
    );
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67").join("claude"),
        "claude",
        "live-claude-provider-pilot",
        1.00,
        true,
        Some(2000),
        Some(750),
        Some(2750),
        Some(0.34),
    );

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

    let response = http_request(
        port,
        &format!(
            "GET /api/provider-pilot/cost-trend?token=test-token&acceptance_root={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            acceptance_root.display()
        ),
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.provider-cost-trend.v1");
    assert_eq!(json["release_count"], 2);
    assert_eq!(json["latest_release_tag"], "v0.4.67");
    assert_eq!(json["delta"]["observed_cost_usd"], 0.36);
    assert_eq!(json["delta"]["total_tokens"], 3250);
    assert_eq!(json["providers"]["codex"]["release_count"], 2);
}

struct ProviderPilotAcceptanceFixture<'a> {
    dir: PathBuf,
    provider: &'a str,
    run_id: &'a str,
    status: &'a str,
    replay_status: &'a str,
    score: u64,
    verdict: &'a str,
    evidence_root: &'a Path,
}

fn write_provider_pilot_acceptance_fixture(fixture: ProviderPilotAcceptanceFixture<'_>) {
    fs::create_dir_all(&fixture.dir).unwrap();
    fs::write(
        fixture.dir.join("provider-pilot-acceptance.json"),
        serde_json::json!({
            "schema_version": format!("ao2.{}-provider-pilot-acceptance.v1", fixture.provider),
            "status": fixture.status,
            "provider": fixture.provider,
            "run_id": fixture.run_id,
            "evidence_pack": fixture.evidence_root.join(format!("{}-evidence-pack.json", fixture.run_id)),
            "cockpit": fixture.evidence_root.join(fixture.run_id).join("cockpit").join("index.html"),
            "replay": {
                "status": fixture.replay_status,
                "event_count": 31,
                "artifact_count": 12,
                "digest_failures": if fixture.replay_status == "accepted" {
                    serde_json::json!([])
                } else {
                    serde_json::json!(["digest mismatch"])
                }
            },
            "score": {
                "schema": "ao2.provider-evidence-scorecard.v1",
                "score": fixture.score,
                "max_score": 100,
                "verdict": fixture.verdict,
                "run_id": fixture.run_id,
                "replay": {
                    "status": fixture.replay_status,
                    "digest_failures": if fixture.replay_status == "accepted" { 0 } else { 1 }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn write_provider_cost_ledger_fixture(
    dir: &Path,
    provider: &str,
    run_id: &str,
    max_budget_usd: f64,
    provider_enforced: bool,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    cost_usd: Option<f64>,
) {
    fs::create_dir_all(dir).unwrap();
    let evidence_pack = dir.join(format!("{run_id}-evidence-pack.json"));
    fs::write(
        &evidence_pack,
        serde_json::json!({
            "schema_version": "ao2.evidence-pack.v1",
            "run_id": run_id,
            "provider_summaries": [{
                "provider": provider,
                "changed_files": ["discount_service/discounts.py"],
                "concerns": [],
                "blockers": [],
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "total_tokens": total_tokens
                },
                "cost_usd": cost_usd,
                "raw_summary": "provider fixed discount validation"
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        dir.join("provider-pilot-acceptance.json"),
        serde_json::json!({
            "schema_version": format!("ao2.{}-provider-pilot-acceptance.v1", provider),
            "status": "passed",
            "provider": provider,
            "run_id": run_id,
            "evidence_pack": evidence_pack,
            "cockpit": dir.join("cockpit").join("index.html"),
            "budget": {
                "max_budget_usd": max_budget_usd,
                "provider_enforced": provider_enforced,
                "timeout_seconds": 900,
                "max_repair_attempts": 1
            },
            "replay": {
                "status": "accepted",
                "event_count": 31,
                "artifact_count": 12,
                "digest_failures": []
            },
            "score": {
                "schema": "ao2.provider-evidence-scorecard.v1",
                "score": 100,
                "max_score": 100,
                "verdict": "ready",
                "run_id": run_id,
                "replay": {
                    "status": "accepted",
                    "digest_failures": 0
                }
            }
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn cli_workbench_provider_pilot_export_latest_acceptance_api_exports_newest_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let acceptance_root = temp.path().join("acceptance-root");
    let latest_dir = acceptance_root.join("v0.4.59");
    fs::create_dir_all(&latest_dir).unwrap();
    fs::write(
        latest_dir.join("provider-pilot-acceptance.json"),
        serde_json::json!({
            "schema_version": "ao2.codex-provider-pilot-acceptance.v1",
            "status": "passed",
            "provider": "codex",
            "run_id": "latest-codex-provider-pilot",
            "evidence_pack": temp.path().join("evidence-pack.json"),
            "cockpit": temp.path().join("cockpit").join("index.html"),
            "replay": {
                "status": "accepted",
                "event_count": 31,
                "artifact_count": 12,
                "digest_failures": []
            },
            "score": {
                "schema": "ao2.provider-evidence-scorecard.v1",
                "score": 99,
                "max_score": 100,
                "verdict": "ready",
                "run_id": "latest-codex-provider-pilot",
                "replay": {
                    "status": "accepted",
                    "digest_failures": 0
                }
            }
        })
        .to_string(),
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
    let body = format!(
        "provider=codex&acceptance_root={}",
        acceptance_root.display()
    );
    let response = http_request(
        port,
        &format!(
            "POST /api/provider-pilot/acceptance/export-latest?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.workbench-provider-pilot-acceptance-export-latest.v1"
    );
    assert_eq!(json["latest"]["provider"], "codex");
    assert_eq!(json["latest"]["run_id"], "latest-codex-provider-pilot");
    assert_eq!(json["export"]["export_kind"], "provider-pilot-acceptance");
    assert_eq!(
        json["export"]["export"]["provider_pilot_acceptance"]["score"]["score"],
        99
    );
    assert!(Path::new(json["export"]["export_path"].as_str().unwrap()).is_file());

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_provider_pilot_start_requires_execution_flag() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

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
    let body = format!(
        "template=bug-fix&provider=codex&provider_prompt_file={}&minimum_score=90",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-error.v1");
    assert_eq!(json["error"], "execution_disabled");
}

#[test]
fn cli_workbench_provider_pilot_start_blocks_when_gate_not_ready() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

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
            "--enable-execution",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=codex&provider_prompt_file={}&minimum_score=90",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["status"], "blocked");
    assert_eq!(json["gate"]["verdict"], "not_ready");
    let queue_path = repo.join(".ao2/workbench/queue.json");
    if queue_path.exists() {
        let queue: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(queue_path).unwrap()).unwrap();
        assert_eq!(queue["jobs"].as_array().unwrap().len(), 0);
    }
}

#[test]
fn cli_workbench_provider_pilot_start_requires_exact_action_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CODEX_SMOKE", "1")],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));

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
            "--enable-execution",
        ])
        .env("PATH", path)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=codex&run_id=workbench-provider-pilot-approval&provider_prompt_file={}&max_repair_attempts=1&max_budget_usd=0.20&minimum_score=90",
        prompt.display()
    );
    let request = format!(
        "POST /api/provider-pilot/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.provider-pilot-approval.v1");
    assert_eq!(json["status"], "approval_required");
    assert_eq!(json["approval_mode"], "exact_action_digest");
    assert_eq!(json["required_form_field"], "approval_action_digest");
    assert_eq!(json["provider"], "codex");
    assert_eq!(json["explicit_live_env"], "AO2_LIVE_CODEX_PILOT");
    assert_eq!(json["action_digest"].as_str().unwrap().len(), 64);
    let queue_path = repo.join(".ao2/workbench/queue.json");
    if queue_path.exists() {
        let queue: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(queue_path).unwrap()).unwrap();
        assert_eq!(queue["jobs"].as_array().unwrap().len(), 0);
    }
}

#[test]
fn cli_workbench_provider_pilot_start_queues_ready_codex_pilot() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CODEX_SMOKE", "1")],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));

    let pilot_preview = ao2([
        "provider",
        "pilot",
        "--target",
        repo.to_str().unwrap(),
        "--provider",
        "codex",
        "--run-id",
        "workbench-provider-pilot-start",
        "--provider-prompt-file",
        prompt.to_str().unwrap(),
        "--provider-max-budget-usd",
        "0.20",
        "--json",
    ]);
    assert!(pilot_preview.status.success(), "{}", stderr(&pilot_preview));
    let pilot_preview_json: serde_json::Value =
        serde_json::from_str(&stdout(&pilot_preview)).unwrap();
    let approval_digest = pilot_preview_json["approval_packet"]["action_digest"]
        .as_str()
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
        ])
        .env("PATH", path)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=codex&run_id=workbench-provider-pilot-start&provider_prompt_file={}&max_repair_attempts=1&max_budget_usd=0.20&minimum_score=90&approval_action_digest={}",
        prompt.display(),
        approval_digest
    );
    let request = format!(
        "POST /api/provider-pilot/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.workbench-provider-pilot-start.v1"
    );
    assert_eq!(json["status"], "queued");
    assert_eq!(json["run_id"], "workbench-provider-pilot-start");
    assert_eq!(json["max_budget_usd"], 0.20);
    assert_eq!(json["pilot"]["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["pilot"]["gate"]["verdict"], "ready");
    assert_eq!(json["approval"]["status"], "approved_exact_action_digest");

    let job = wait_for_queue_job_status(port, "workbench-provider-pilot-start", "accepted");
    assert_eq!(job["provider"], "codex");
    assert_eq!(job["max_budget_usd"], 0.20);
    assert!(job["evidence_pack"]
        .as_str()
        .unwrap()
        .ends_with("evidence-pack.json"));
    assert!(Path::new(job["evidence_pack"].as_str().unwrap()).is_file());
    assert!(Path::new(job["cockpit"].as_str().unwrap()).is_file());

    child.kill().ok();
    child.wait().ok();
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

fn sha256_hex_for_test(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn canonical_sha256_for_test(value: &serde_json::Value) -> String {
    fn write_value(out: &mut String, value: &serde_json::Value) {
        match value {
            serde_json::Value::Null => out.push_str("null"),
            serde_json::Value::Bool(value) => {
                out.push_str(if *value { "true" } else { "false" });
            }
            serde_json::Value::Number(value) => out.push_str(&value.to_string()),
            serde_json::Value::String(value) => write_string(out, value),
            serde_json::Value::Array(values) => {
                out.push('[');
                for (index, item) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_value(out, item);
                }
                out.push(']');
            }
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(out, key);
                    out.push(':');
                    write_value(out, &map[*key]);
                }
                out.push('}');
            }
        }
    }
    fn write_string(out: &mut String, value: &str) {
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{08}' => out.push_str("\\b"),
                '\u{0c}' => out.push_str("\\f"),
                ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => out.push(ch),
            }
        }
        out.push('"');
    }
    let mut canonical = String::new();
    write_value(&mut canonical, value);
    sha256_hex_for_test(canonical.as_bytes())
}

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
}

fn archive_entries(path: &Path) -> Vec<String> {
    let archive = fs::File::open(path).expect("open archive");
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    let mut entries = archive
        .entries()
        .expect("archive entries")
        .map(|entry| {
            entry
                .expect("archive entry")
                .path()
                .expect("entry path")
                .to_string_lossy()
                .trim_start_matches("./")
                .to_string()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn archive_text_entry(path: &Path, wanted: &str) -> String {
    let archive = fs::File::open(path).expect("open archive");
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    for entry in archive.entries().expect("archive entries") {
        let mut entry = entry.expect("archive entry");
        let path = entry
            .path()
            .expect("entry path")
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        if path == wanted {
            let mut body = String::new();
            entry.read_to_string(&mut body).expect("read archive text");
            return body;
        }
    }
    panic!("missing archive entry {wanted}");
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
