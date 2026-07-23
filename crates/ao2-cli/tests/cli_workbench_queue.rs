#![allow(dead_code, unused_imports)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn as_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child already consumed")
    }

    fn stop(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
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
fn cli_workbench_queue_requires_explicit_execution_flag() {
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
    let body = "template=bug-fix&provider=scripted&run_id=queue-disabled";
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["error"], "execution_disabled");
}

#[test]
fn cli_workbench_queue_rejects_launch_when_minimum_score_not_met() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queue-score-gate",
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
            "--enable-execution",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = "template=bug-fix&provider=scripted&run_id=queue-score-gate&minimum_score=90";
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    assert_eq!(json["error"], "minimum_provider_score_not_met");
    assert_eq!(json["minimum_score"], 90);
    assert_eq!(json["run_id"], "queue-score-gate");
}

#[test]
fn cli_workbench_queue_executes_scripted_run_and_reports_evidence() {
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
printf 'Summary: queue execution fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=scripted&run_id=queue-exec&provider_prompt_file={}&max_repair_attempts=1",
        prompt_path.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    let start_response = http_request(port, &request);
    assert!(start_response.starts_with("HTTP/1.1 200 OK"));
    let start_json: serde_json::Value = serde_json::from_str(http_body(&start_response)).unwrap();
    assert_eq!(start_json["schema_version"], "ao2.workbench-queue-start.v1");
    assert_eq!(start_json["run_id"], "queue-exec");
    let warnings = start_json["provider_warnings"].as_array().unwrap();
    assert!(warnings
        .iter()
        .any(|warning| warning == "timeout_seconds=900"));
    assert!(warnings
        .iter()
        .any(|warning| warning == "execution_boundary=sandbox_copy_then_digest_patch"));

    let final_job = wait_for_queue_job_status(port, "queue-exec", "accepted");
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(final_job["status"], "accepted");
    assert!(Path::new(final_job["evidence_pack"].as_str().unwrap()).is_file());
    assert!(Path::new(final_job["cockpit"].as_str().unwrap()).is_file());
}

#[test]
fn cli_workbench_queue_starts_repair_resume_from_rejected_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let failed_prompt = temp.path().join("repair-source-prompt.sh");
    fs::write(
        &failed_prompt,
        r#"cat >> tests/test_discounts.py <<'PY'

def test_rejects_negative_price():
    import pytest
    with pytest.raises(ValueError):
        calculate_discount(-1, 0.25)
PY
printf 'Summary: source run added failing negative price regression\n'
printf 'Changed files: tests/test_discounts.py\n'
"#,
    )
    .unwrap();
    let failed = ao2([
        "run",
        "--template",
        "bug-fix",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-repair-source",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        failed_prompt.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);
    assert!(failed.status.success(), "{}", stderr(&failed));
    assert!(stdout(&failed).contains("status=Rejected"));
    let source_evidence =
        repo.join(".ao2/runs/workbench-repair-source/evidence-pack/evidence-pack.json");
    assert!(source_evidence.is_file());

    let repair_prompt = temp.path().join("repair-resume-prompt.sh");
    fs::write(
        &repair_prompt,
        r#"if printf '%s' "$AO2_REPAIR_RUN_HEALTH" | grep -q 'budget_exhausted' \
  && test "$AO2_REPAIR_SOURCE_RUN_ID" = "workbench-repair-source"; then
  cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
else
  printf 'missing repair resume source context\n' >&2
  exit 2
fi
printf 'Summary: workbench repair resume fixed discount validation\n'
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
    assert!(
        html_response.starts_with("HTTP/1.1 200 OK"),
        "{html_response}"
    );
    let html = http_body(&html_response);
    assert!(html.contains("Resume From Rejected Evidence"));
    assert!(html.contains("repair-resume-form"));
    assert!(html.contains("/api/repair/resume/start"));

    let body = format!(
        "template=bug-fix&provider=scripted&run_id=workbench-repair-resumed&evidence_pack={}&provider_prompt_file={}&max_repair_attempts=0",
        source_evidence.to_str().unwrap(),
        repair_prompt.to_str().unwrap()
    );
    let request = format!(
        "POST /api/repair/resume/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let start_response = http_request(port, &request);
    assert!(
        start_response.starts_with("HTTP/1.1 200 OK"),
        "{start_response}"
    );
    let start_json: serde_json::Value = serde_json::from_str(http_body(&start_response)).unwrap();
    assert_eq!(
        start_json["schema_version"],
        "ao2.workbench-repair-resume-start.v1"
    );
    assert_eq!(start_json["status"], "queued");
    assert_eq!(start_json["source_run_id"], "workbench-repair-source");
    assert_eq!(start_json["run_id"], "workbench-repair-resumed");

    let final_job = wait_for_queue_job_status(port, "workbench-repair-resumed", "accepted");
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(final_job["job_kind"], "repair_resume");
    assert_eq!(final_job["repair_source_run_id"], "workbench-repair-source");
    assert_eq!(
        final_job["repair_evidence_pack"],
        source_evidence.display().to_string()
    );
    assert!(Path::new(final_job["evidence_pack"].as_str().unwrap()).is_file());
    let repaired_evidence =
        fs::read_to_string(final_job["evidence_pack"].as_str().unwrap()).unwrap();
    assert!(repaired_evidence.contains("repair_source_context"));
    assert!(repaired_evidence.contains("\"source_run_id\": \"workbench-repair-source\""));
}

#[test]
fn cli_workbench_queue_persists_failed_history_across_restart() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let missing_prompt = temp.path().join("missing-prompt.sh");

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
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=scripted&run_id=queue-persist-failed&provider_prompt_file={}",
        missing_prompt.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let start_response = http_request(port, &request);
    assert!(start_response.starts_with("HTTP/1.1 200 OK"));

    let failed_job = wait_for_queue_job_status(port, "queue-persist-failed", "failed");
    assert!(failed_job["error"]
        .as_str()
        .unwrap()
        .contains("missing-prompt.sh"));
    let _ = child.kill();
    let _ = child.wait();

    assert!(repo.join(".ao2/workbench/queue.json").is_file());

    let mut restarted = Command::new(env!("CARGO_BIN_EXE_ao2"))
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
    let restarted_port = read_server_port(&mut restarted);
    let queue = get_queue(restarted_port);
    let status = restarted.wait().unwrap();

    assert!(status.success());
    let restored = queue["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["run_id"] == "queue-persist-failed")
        .unwrap();
    assert_eq!(restored["status"], "failed");
    assert!(restored["error"]
        .as_str()
        .unwrap()
        .contains("missing-prompt.sh"));
}

#[test]
fn cli_workbench_queue_detail_reports_provider_failure_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let failing_prompt = temp.path().join("invalid-local-auth-missing-prompt.sh");

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
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=scripted&run_id=queue-provider-diagnostics&provider_prompt_file={}",
        failing_prompt.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let start_response = http_request(port, &request);
    assert!(start_response.starts_with("HTTP/1.1 200 OK"));
    let start_json: serde_json::Value = serde_json::from_str(http_body(&start_response)).unwrap();
    let job_id = start_json["job_id"].as_str().unwrap().to_string();

    let failed_job = wait_for_queue_job_status(port, "queue-provider-diagnostics", "failed");
    assert_eq!(failed_job["exit_code"], 1);
    assert_eq!(
        failed_job["diagnosis"]["schema_version"],
        "ao2.workbench-job-diagnosis.v1"
    );
    assert_eq!(failed_job["diagnosis"]["failure_kind"], "non_zero_exit");
    assert_eq!(failed_job["diagnosis"]["timed_out"], false);
    assert!(failed_job["diagnosis"]["recovery_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action.as_str().unwrap().contains("Review stderr")));

    let detail_response = http_request(
        port,
        &format!(
            "GET /api/queue/job?token=test-token&job_id={job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(
        detail_response.starts_with("HTTP/1.1 200 OK"),
        "{detail_response}"
    );
    let detail: serde_json::Value = serde_json::from_str(http_body(&detail_response)).unwrap();
    assert_eq!(
        detail["diagnosis"]["schema_version"],
        "ao2.workbench-job-diagnosis.v1"
    );
    assert_eq!(detail["diagnosis"]["failure_kind"], "non_zero_exit");
    assert_eq!(detail["diagnosis"]["exit_code"], 1);
    assert!(detail["diagnosis"]["stderr_excerpt"]
        .as_str()
        .unwrap()
        .contains("invalid-local-auth-missing-prompt.sh"));
    assert!(detail["diagnosis"]["recovery_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action.as_str().unwrap().contains("local provider auth")));

    let detail_page = http_request(
        port,
        &format!(
            "GET /queue/job?token=test-token&job_id={job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(detail_page.starts_with("HTTP/1.1 200 OK"), "{detail_page}");
    assert!(detail_page.contains("Failure Diagnosis"));
    assert!(detail_page.contains("invalid-local-auth-missing-prompt.sh"));
    assert!(detail_page.contains("local provider auth"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_wait_timeout_message_reports_last_observed_job() {
    let job = serde_json::json!({
        "run_id": "queue-exec",
        "status": "failed",
        "error": "queued ao2 run failed: provider prompt missing",
        "exit_code": 1,
        "stderr_log": "/tmp/queue-exec.stderr.log"
    });

    let message = queue_wait_timeout_message("queue-exec", "accepted", Some(&job));

    assert!(message.contains("job queue-exec did not reach status accepted"));
    assert!(message.contains("last_status=failed"));
    assert!(message.contains("exit_code=1"));
    assert!(message.contains("error=queued ao2 run failed: provider prompt missing"));
    assert!(message.contains("stderr_log=/tmp/queue-exec.stderr.log"));
}

#[test]
fn cli_workbench_queue_can_cancel_running_job() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("slow-prompt.sh");
    fs::write(
        &prompt_path,
        r#"sleep 30
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    return price * (1 - discount_rate)
PY
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=scripted&run_id=queue-cancel-running&provider_prompt_file={}",
        prompt_path.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let start_response = http_request(port, &request);
    assert!(start_response.starts_with("HTTP/1.1 200 OK"));
    let start_json: serde_json::Value = serde_json::from_str(http_body(&start_response)).unwrap();
    let job_id = start_json["job_id"].as_str().unwrap();

    let running_job = wait_for_queue_job_status(port, "queue-cancel-running", "running");
    assert_eq!(running_job["job_id"], job_id);

    let cancel_body = format!("job_id={job_id}");
    let cancel_request = format!(
        "POST /api/queue/cancel?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        cancel_body.len(),
        cancel_body
    );
    let cancel_response = http_request(port, &cancel_request);
    assert!(cancel_response.starts_with("HTTP/1.1 200 OK"));
    let cancel_json: serde_json::Value = serde_json::from_str(http_body(&cancel_response)).unwrap();
    assert_eq!(cancel_json["status"], "cancelled");

    let cancelled_job = wait_for_queue_job_status(port, "queue-cancel-running", "cancelled");
    assert_eq!(cancelled_job["job_id"], job_id);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_can_retry_failed_job_and_renders_controls() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let missing_prompt = temp.path().join("retry-missing-prompt.sh");

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
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=scripted&run_id=queue-retry-failed&provider_prompt_file={}",
        missing_prompt.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let start_response = http_request(port, &request);
    assert!(start_response.starts_with("HTTP/1.1 200 OK"));
    let start_json: serde_json::Value = serde_json::from_str(http_body(&start_response)).unwrap();
    let original_job_id = start_json["job_id"].as_str().unwrap();
    let failed_job = wait_for_queue_job_status(port, "queue-retry-failed", "failed");
    assert_eq!(failed_job["job_id"], original_job_id);

    let html = http_request(
        port,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(html.contains("data-action=\"cancel\""));
    assert!(html.contains("data-action=\"retry\""));

    let retry_body = format!("job_id={original_job_id}");
    let retry_request = format!(
        "POST /api/queue/retry?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        retry_body.len(),
        retry_body
    );
    let retry_response = http_request(port, &retry_request);
    assert!(retry_response.starts_with("HTTP/1.1 200 OK"));
    let retry_json: serde_json::Value = serde_json::from_str(http_body(&retry_response)).unwrap();
    assert_eq!(retry_json["schema_version"], "ao2.workbench-queue-start.v1");
    assert_eq!(retry_json["retry_of"], original_job_id);
    assert_ne!(retry_json["job_id"], original_job_id);
    assert!(retry_json["run_id"]
        .as_str()
        .unwrap()
        .starts_with("queue-retry-failed-retry-"));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_job_detail_reports_logs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("detail-prompt.sh");
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
printf 'Summary: queue detail fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "template=bug-fix&provider=scripted&run_id=queue-detail&provider_prompt_file={}&max_repair_attempts=1",
        prompt_path.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let start_response = http_request(port, &request);
    assert!(start_response.starts_with("HTTP/1.1 200 OK"));
    let start_json: serde_json::Value = serde_json::from_str(http_body(&start_response)).unwrap();
    let job_id = start_json["job_id"].as_str().unwrap();
    let job = wait_for_queue_job_status(port, "queue-detail", "accepted");
    assert_eq!(job["job_id"], job_id);

    let detail_response = http_request(
        port,
        &format!(
            "GET /api/queue/job?token=test-token&job_id={job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(detail_response.starts_with("HTTP/1.1 200 OK"));
    let detail: serde_json::Value = serde_json::from_str(http_body(&detail_response)).unwrap();
    assert_eq!(detail["schema_version"], "ao2.workbench-queue-job.v1");
    assert_eq!(detail["job"]["run_id"], "queue-detail");
    assert!(detail["stdout"]
        .as_str()
        .unwrap()
        .contains("run_id=queue-detail"));
    assert!(Path::new(detail["job"]["stdout_log"].as_str().unwrap()).is_file());
    assert!(Path::new(detail["job"]["stderr_log"].as_str().unwrap()).is_file());

    let html = http_request(
        port,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(html.contains("data-action=\"detail\""));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_live_logs_update_while_job_runs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("live-logs-prompt.sh");
    fs::write(
        &prompt_path,
        r#"sleep 2
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: live logs fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let start_json = start_queue_job(port, "queue-live-logs", &prompt_path);
    let job_id = start_json["job_id"].as_str().unwrap();
    let running = wait_for_queue_job_status(port, "queue-live-logs", "running");
    assert_eq!(running["job_id"], job_id);

    let logs_response = http_request(
        port,
        &format!(
            "GET /api/queue/job/logs?token=test-token&job_id={job_id}&tail_bytes=4096 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(
        logs_response.starts_with("HTTP/1.1 200 OK"),
        "{logs_response}"
    );
    let logs: serde_json::Value = serde_json::from_str(http_body(&logs_response)).unwrap();
    assert_eq!(logs["schema_version"], "ao2.workbench-queue-job-logs.v1");
    assert_eq!(logs["job"]["run_id"], "queue-live-logs");
    assert_eq!(logs["job"]["status"], "running");
    assert!(logs["stdout"]["bytes"].as_u64().is_some());
    assert!(logs["stderr"]["bytes"].as_u64().is_some());

    let accepted = wait_for_queue_job_status(port, "queue-live-logs", "accepted");
    assert_eq!(accepted["job_id"], job_id);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_log_tail_bounds_large_logs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("tail-logs-prompt.sh");
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
printf 'Summary: tail logs fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let start_json = start_queue_job(port, "queue-log-tail", &prompt_path);
    let job_id = start_json["job_id"].as_str().unwrap();
    let job = wait_for_queue_job_status(port, "queue-log-tail", "accepted");
    assert_eq!(job["job_id"], job_id);
    let stdout_log = Path::new(job["stdout_log"].as_str().unwrap());
    fs::write(stdout_log, format!("{}TAIL_MARKER", "A".repeat(2048))).unwrap();

    let logs_response = http_request(
        port,
        &format!(
            "GET /api/queue/job/logs?token=test-token&job_id={job_id}&tail_bytes=64 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(
        logs_response.starts_with("HTTP/1.1 200 OK"),
        "{logs_response}"
    );
    let logs: serde_json::Value = serde_json::from_str(http_body(&logs_response)).unwrap();
    assert_eq!(logs["schema_version"], "ao2.workbench-queue-job-logs.v1");
    assert_eq!(logs["stdout"]["truncated"], true);
    assert!(logs["stdout"]["text"]
        .as_str()
        .unwrap()
        .contains("TAIL_MARKER"));
    assert!(logs["stdout"]["text"].as_str().unwrap().len() <= 64);
    assert!(logs["stdout"]["bytes"].as_u64().unwrap() > 64);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_renders_live_log_controls() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("live-log-controls-prompt.sh");
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
printf 'Summary: live log controls fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-live-log-controls", &prompt_path);
    wait_for_queue_job_status(port, "queue-live-log-controls", "accepted");

    let html = http_request(
        port,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(html.contains("data-action=\"logs\""));
    assert!(html.contains("queue-log-output"));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_detail_page_renders_job_logs_and_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("detail-page-prompt.sh");
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
printf 'Summary: queue detail page fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();
    let mut child = ChildGuard::new(
        Command::new(env!("CARGO_BIN_EXE_ao2"))
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
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let port = read_server_port(child.as_mut());
    let start_json = start_queue_job(port, "queue-detail-page", &prompt_path);
    let job_id = start_json["job_id"].as_str().unwrap();
    let job = wait_for_workbench_support_fixture_job(port, "queue-detail-page");
    assert_eq!(job["job_id"], job_id);

    let detail_response = http_request(
        port,
        &format!(
            "GET /queue/job?token=test-token&job_id={job_id} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(detail_response.starts_with("HTTP/1.1 200 OK"));
    assert!(detail_response.contains("queue-detail-page"));
    assert!(detail_response.contains("run_id=queue-detail-page"));
    assert!(detail_response.contains("Open Evidence"));
    assert!(detail_response.contains("Open Cockpit"));
    assert!(detail_response.contains("Queue Wait"));
    assert!(detail_response.contains("Duration"));
    assert!(detail_response.contains("Exit Code"));
    assert!(detail_response.contains("Retry Count"));
    child.stop();
}

#[test]
fn cli_workbench_queue_records_runtime_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("metrics-prompt.sh");
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-metrics", &prompt_path);
    let job = wait_for_queue_job_status(port, "queue-metrics", "accepted");
    let _ = child.kill();
    let _ = child.wait();

    assert!(job["queued_at_ms"].as_u64().unwrap() > 0);
    assert!(job["started_at_ms"].as_u64().unwrap() >= job["queued_at_ms"].as_u64().unwrap());
    assert!(job["finished_at_ms"].as_u64().unwrap() >= job["started_at_ms"].as_u64().unwrap());
    assert!(job["queue_wait_ms"].as_u64().unwrap() < 60_000);
    assert!(job["duration_ms"].as_u64().unwrap() < 60_000);
    assert_eq!(job["exit_code"], 0);
    assert_eq!(job["retry_count"], 0);
}

#[test]
fn cli_workbench_queue_writes_audit_events_for_cancel_and_retry() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let slow_prompt = temp.path().join("audit-slow-prompt.sh");
    fs::write(
        &slow_prompt,
        r#"sleep 30
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    return price * (1 - discount_rate)
PY
"#,
    )
    .unwrap();
    let missing_prompt = temp.path().join("audit-missing-prompt.sh");
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
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let cancel_start = start_queue_job(port, "queue-audit-cancel", &slow_prompt);
    let cancel_job_id = cancel_start["job_id"].as_str().unwrap();
    wait_for_queue_job_status(port, "queue-audit-cancel", "running");
    let cancel_body = format!("job_id={cancel_job_id}");
    let cancel_request = format!(
        "POST /api/queue/cancel?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        cancel_body.len(),
        cancel_body
    );
    let cancel_response = http_request(port, &cancel_request);
    assert!(
        cancel_response.starts_with("HTTP/1.1 200 OK"),
        "{cancel_response}"
    );
    let cancel_json: serde_json::Value = serde_json::from_str(http_body(&cancel_response)).unwrap();
    if cancel_json["cancel_applied"].as_bool().unwrap_or(false) {
        wait_for_queue_job_status(port, "queue-audit-cancel", "cancelled");
    } else {
        assert_ne!(cancel_json["status"], "queued");
        assert_ne!(cancel_json["status"], "running");
    }

    let failed_start = start_queue_job(port, "queue-audit-retry", &missing_prompt);
    let failed_job_id = failed_start["job_id"].as_str().unwrap();
    wait_for_queue_job_status(port, "queue-audit-retry", "failed");
    let terminal_cancel_body = format!("job_id={failed_job_id}");
    let terminal_cancel_request = format!(
        "POST /api/queue/cancel?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        terminal_cancel_body.len(),
        terminal_cancel_body
    );
    let terminal_cancel_response = http_request(port, &terminal_cancel_request);
    assert!(
        terminal_cancel_response.starts_with("HTTP/1.1 200 OK"),
        "{terminal_cancel_response}"
    );
    let terminal_cancel_json: serde_json::Value =
        serde_json::from_str(http_body(&terminal_cancel_response)).unwrap();
    assert_eq!(terminal_cancel_json["status"], "failed");
    assert_eq!(terminal_cancel_json["cancel_applied"], false);
    let retry_body = format!("job_id={failed_job_id}");
    let retry_request = format!(
        "POST /api/queue/retry?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        retry_body.len(),
        retry_body
    );
    let retry_response = http_request(port, &retry_request);
    assert!(
        retry_response.starts_with("HTTP/1.1 200 OK"),
        "{retry_response}"
    );
    let retry_json: serde_json::Value = serde_json::from_str(http_body(&retry_response)).unwrap();
    let retry_job_id = retry_json["job_id"].as_str().unwrap();
    let _ = child.kill();
    let _ = child.wait();

    let audit = fs::read_to_string(repo.join(".ao2/workbench/audit.jsonl")).unwrap();
    assert!(audit.contains("\"action\":\"cancel\""));
    assert!(audit.contains(cancel_job_id));
    assert!(audit.contains("\"action\":\"retry\""));
    assert!(audit.contains(failed_job_id));
    assert!(audit.contains(retry_job_id));
}

#[test]
fn cli_workbench_queue_filters_by_status() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("filter-prompt.sh");
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
"#,
    )
    .unwrap();
    let missing_prompt = temp.path().join("filter-missing-prompt.sh");
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
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-filter-accepted", &prompt_path);
    start_queue_job(port, "queue-filter-failed", &missing_prompt);
    wait_for_queue_job_status(port, "queue-filter-accepted", "accepted");
    wait_for_queue_job_status(port, "queue-filter-failed", "failed");

    let response = http_request(
        port,
        "GET /api/queue?token=test-token&status=failed HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    let filtered: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    let jobs = filtered["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["run_id"], "queue-filter-failed");
    assert_eq!(jobs[0]["status"], "failed");
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_retention_prunes_old_jobs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let first_missing_prompt = temp.path().join("retention-first-missing.sh");
    let second_missing_prompt = temp.path().join("retention-second-missing.sh");
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
            "--queue-retention",
            "1",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-retention-first", &first_missing_prompt);
    wait_for_queue_job_status(port, "queue-retention-first", "failed");
    start_queue_job(port, "queue-retention-second", &second_missing_prompt);
    wait_for_queue_job_status(port, "queue-retention-second", "failed");

    let queue = get_queue(port);
    let jobs = queue["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["run_id"], "queue-retention-second");
    let queue_file = fs::read_to_string(repo.join(".ao2/workbench/queue.json")).unwrap();
    let queue_file: serde_json::Value = serde_json::from_str(&queue_file).unwrap();
    let persisted_jobs = queue_file["jobs"].as_array().unwrap();
    assert_eq!(persisted_jobs.len(), 1);
    assert_eq!(persisted_jobs[0]["run_id"], "queue-retention-second");
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_audit_api_filters_events_and_renders_panel() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let slow_prompt = temp.path().join("audit-panel-slow.sh");
    fs::write(
        &slow_prompt,
        r#"sleep 30
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    return price * (1 - discount_rate)
PY
"#,
    )
    .unwrap();
    let missing_prompt = temp.path().join("audit-panel-missing.sh");
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
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let failed_start = start_queue_job(port, "queue-audit-panel-retry", &missing_prompt);
    let failed_job_id = failed_start["job_id"].as_str().unwrap();
    wait_for_queue_job_status(port, "queue-audit-panel-retry", "failed");
    let retry_body = format!("job_id={failed_job_id}");
    let retry_request = format!(
        "POST /api/queue/retry?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        retry_body.len(),
        retry_body
    );
    let retry_response = http_request(port, &retry_request);
    assert!(retry_response.starts_with("HTTP/1.1 200 OK"));

    start_queue_job(port, "queue-audit-panel-running", &slow_prompt);
    wait_for_queue_job_status(port, "queue-audit-panel-running", "running");
    let cancel_start = start_queue_job(port, "queue-audit-panel-cancel", &missing_prompt);
    let cancel_job_id = cancel_start["job_id"].as_str().unwrap();
    let cancel_body = format!("job_id={cancel_job_id}");
    let cancel_request = format!(
        "POST /api/queue/cancel?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        cancel_body.len(),
        cancel_body
    );
    let cancel_response = http_request(port, &cancel_request);
    assert!(cancel_response.starts_with("HTTP/1.1 200 OK"));
    wait_for_queue_job_status(port, "queue-audit-panel-cancel", "cancelled");

    let audit_response = http_request(
        port,
        "GET /api/queue/audit?token=test-token&action=cancel HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(audit_response.starts_with("HTTP/1.1 200 OK"));
    let audit: serde_json::Value = serde_json::from_str(http_body(&audit_response)).unwrap();
    assert_eq!(audit["schema_version"], "ao2.workbench-audit.v1");
    assert_eq!(audit["filters"]["action"], "cancel");
    let events = audit["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["action"], "cancel");
    assert_eq!(events[0]["job_id"], cancel_job_id);

    let html = http_request(
        port,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(html.contains("Queue Audit"));
    assert!(html.contains("queue-audit-output"));
    assert!(html.contains("queue-audit-refresh"));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_export_writes_support_bundle_with_logs_and_audit() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("support-bundle-prompt.sh");
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
printf 'Summary: support bundle fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-support-bundle", &prompt_path);
    wait_for_queue_job_status(port, "queue-support-bundle", "accepted");

    let export_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(export_response.starts_with("HTTP/1.1 200 OK"));
    let export: serde_json::Value = serde_json::from_str(http_body(&export_response)).unwrap();
    assert_eq!(export["schema_version"], "ao2.workbench-support-bundle.v1");
    let bundle_path = Path::new(export["bundle_path"].as_str().unwrap());
    assert!(bundle_path.is_file());
    assert!(normalize_separators(&bundle_path.to_string_lossy())
        .contains(".ao2/workbench/support-bundles/"));

    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(bundle_path).unwrap()).unwrap();
    assert_eq!(bundle["schema_version"], "ao2.workbench-support-bundle.v1");
    let hermes_contract = &bundle["hermes_project_start_flow_contract"];
    assert_eq!(
        hermes_contract["schema_version"],
        "ao2.hermes-project-start-flow-contract.v1"
    );
    assert_eq!(hermes_contract["status"], "ready");
    assert_eq!(hermes_contract["embedded"], true);
    assert_sha256_string(&hermes_contract["contract_sha256"], "contract_sha256");
    assert_eq!(
        hermes_contract["workflow"]["preview"]["minimum_role"],
        "viewer"
    );
    assert_eq!(
        hermes_contract["workflow"]["publish"]["minimum_role"],
        "operator"
    );
    assert_eq!(
        hermes_contract["hermes_contract"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(
        hermes_contract["side_effects"]["would_execute_queue"],
        false
    );
    assert_eq!(
        hermes_contract["side_effects"]["would_submit_queue_entry"],
        false
    );
    assert_eq!(
        hermes_contract["side_effects"]["would_rebuild_wrappers"],
        false
    );
    assert_eq!(
        hermes_contract["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(
        hermes_contract["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        hermes_contract["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        hermes_contract["trust_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert_eq!(bundle["queue"]["jobs"][0]["run_id"], "queue-support-bundle");
    assert!(!bundle["audit_events"].as_array().unwrap().is_empty());
    assert!(bundle["job_logs"][0]["stdout"]
        .as_str()
        .unwrap()
        .contains("run_id=queue-support-bundle"));
    assert!(bundle["job_logs"][0]["job"]["evidence_pack"]
        .as_str()
        .unwrap()
        .contains("evidence-pack"));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_export_preview_redacts_secrets_without_writing_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp
        .path()
        .join("support-bundle-redaction-preview-prompt.sh");
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
printf 'Summary: support bundle redaction preview fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-support-redaction-preview", &prompt_path);
    let job = wait_for_queue_job_status(port, "queue-support-redaction-preview", "accepted");
    let stdout_log = PathBuf::from(job["stdout_log"].as_str().unwrap());
    let mut stdout_log_file = fs::OpenOptions::new()
        .append(true)
        .open(&stdout_log)
        .unwrap();
    writeln!(stdout_log_file, "OPENAI_API_KEY=sk-preview-secret").unwrap();
    writeln!(
        stdout_log_file,
        "ANTHROPIC_API_KEY=anthropic-preview-secret"
    )
    .unwrap();
    writeln!(stdout_log_file, "TWILIO_AUTH_TOKEN=twilio-preview-secret").unwrap();
    writeln!(
        stdout_log_file,
        "SUPABASE_SERVICE_ROLE_KEY=sb_secret_preview_value"
    )
    .unwrap();
    writeln!(
        stdout_log_file,
        "Authorization: Bearer bearer-preview-secret"
    )
    .unwrap();
    writeln!(
        stdout_log_file,
        "callback=https://example.com/hook?token=url-preview-secret&access_token=access-preview-secret&api_key=key-preview-secret&signature=sig-preview-secret&safe=ok"
    )
    .unwrap();

    let preview_response = http_request(
        port,
        "POST /api/queue/export-preview?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(
        preview_response.starts_with("HTTP/1.1 200 OK"),
        "{preview_response}"
    );
    let preview: serde_json::Value = serde_json::from_str(http_body(&preview_response)).unwrap();
    assert_eq!(
        preview["schema_version"],
        "ao2.workbench-support-bundle-preview.v1"
    );
    assert_eq!(preview["would_write_bundle"], false);
    assert_eq!(preview["queue_job_count"], 1);
    assert_eq!(preview["job_log_count"], 1);
    assert_eq!(
        preview["redaction_preview"]["schema_version"],
        "ao2.workbench-support-redaction-preview.v1"
    );
    assert_eq!(preview["redaction_preview"]["redaction_count"], 9);
    assert_eq!(preview["redaction_audit"]["redaction_count"], 9);
    assert_eq!(
        preview["redaction_audit"]["secret_classes"]["provider_api_key"],
        2
    );
    assert_eq!(
        preview["redaction_audit"]["secret_classes"]["auth_token"],
        1
    );
    assert_eq!(
        preview["redaction_audit"]["secret_classes"]["service_role_key"],
        1
    );
    assert_eq!(
        preview["redaction_audit"]["secret_classes"]["bearer_authorization"],
        1
    );
    assert_eq!(
        preview["redaction_audit"]["secret_classes"]["query_token"],
        2
    );
    assert_eq!(
        preview["redaction_audit"]["secret_classes"]["query_api_key"],
        1
    );
    assert_eq!(
        preview["redaction_audit"]["secret_classes"]["query_signature"],
        1
    );
    let redacted_fields = preview["redaction_preview"]["redacted_fields"]
        .as_array()
        .unwrap();
    assert!(redacted_fields
        .iter()
        .any(|field| field["path"] == "job_logs[0].stdout"));
    let preview_text = serde_json::to_string(&preview).unwrap();
    assert!(preview_text.contains("[REDACTED]"));
    assert!(!preview_text.contains("sk-preview-secret"));
    assert!(!preview_text.contains("anthropic-preview-secret"));
    assert!(!preview_text.contains("twilio-preview-secret"));
    assert!(!preview_text.contains("sb_secret_preview_value"));
    assert!(!preview_text.contains("bearer-preview-secret"));
    assert!(!preview_text.contains("url-preview-secret"));
    assert!(!preview_text.contains("access-preview-secret"));
    assert!(!preview_text.contains("key-preview-secret"));
    assert!(!preview_text.contains("sig-preview-secret"));
    assert!(!repo.join(".ao2/workbench/support-bundles").exists());

    let export_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(export_response.starts_with("HTTP/1.1 200 OK"));
    let export: serde_json::Value = serde_json::from_str(http_body(&export_response)).unwrap();
    let bundle_path = PathBuf::from(export["bundle_path"].as_str().unwrap());
    assert_eq!(export["bundle"]["redaction_audit"]["redaction_count"], 9);
    assert_eq!(
        export["bundle"]["redaction_audit"]["secret_classes"]["query_signature"],
        1
    );
    let bundle_text = fs::read_to_string(&bundle_path).unwrap();
    assert!(bundle_text.contains("[REDACTED]"));
    assert!(!bundle_text.contains("sk-preview-secret"));
    assert!(!bundle_text.contains("anthropic-preview-secret"));
    assert!(!bundle_text.contains("twilio-preview-secret"));
    assert!(!bundle_text.contains("sb_secret_preview_value"));
    assert!(!bundle_text.contains("bearer-preview-secret"));
    assert!(!bundle_text.contains("url-preview-secret"));
    assert!(!bundle_text.contains("access-preview-secret"));
    assert!(!bundle_text.contains("key-preview-secret"));
    assert!(!bundle_text.contains("sig-preview-secret"));
    let bundle_dir = bundle_path.parent().unwrap();
    let inspect = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let inspect_json: serde_json::Value = serde_json::from_str(&stdout(&inspect)).unwrap();
    assert_eq!(inspect_json["redaction_audit"]["redaction_count"], 9);
    assert_eq!(
        inspect_json["redaction_audit"]["secret_classes"]["query_api_key"],
        1
    );

    let import_dir = temp.path().join("redaction-support-cases");
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
    assert_eq!(import_json["redaction_audit"]["redaction_count"], 9);
    let import_html = fs::read_to_string(import_json["index_path"].as_str().unwrap()).unwrap();
    assert!(import_html.contains("Redaction Audit"));
    assert!(import_html.contains("query_signature"));

    let html = http_request(
        port,
        "GET /?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(html.contains("queue-export-preview-button"));
    assert!(html.contains("/api/queue/export-preview"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_export_attaches_evidence_exports() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("support-bundle-evidence-prompt.sh");
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
printf 'Summary: support bundle evidence export fixed discount validation\n'
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
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-support-evidence", &prompt_path);
    wait_for_queue_job_status(port, "queue-support-evidence", "accepted");

    let export_body = "kind=summary&run_id=queue-support-evidence";
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
    assert_eq!(evidence_export["export_kind"], "summary");

    let support_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(support_response.starts_with("HTTP/1.1 200 OK"));
    let support_export: serde_json::Value =
        serde_json::from_str(http_body(&support_response)).unwrap();
    let bundle_path = PathBuf::from(support_export["bundle_path"].as_str().unwrap());
    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(bundle_path).unwrap()).unwrap();
    let evidence_exports = bundle["evidence_exports"].as_array().unwrap();
    assert_eq!(evidence_exports.len(), 1);
    assert_eq!(evidence_exports[0]["kind"], "summary");
    assert_eq!(
        evidence_exports[0]["content"]["schema_version"],
        "ao2.workbench-evidence-export.v1"
    );
    assert_eq!(
        evidence_exports[0]["content"]["export"]["summary"]["run_id"],
        "queue-support-evidence"
    );
    assert_eq!(evidence_exports[0]["sha256"].as_str().unwrap().len(), 64);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_workbench_queue_export_writes_signed_support_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp.path().join("workbench-support-signing-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let prompt_path = temp.path().join("signed-support-bundle-prompt.sh");
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
printf 'Summary: signed support bundle fixed discount validation\n'
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
            "workbench-lead",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    start_queue_job(port, "queue-signed-support-bundle", &prompt_path);
    wait_for_queue_job_status(port, "queue-signed-support-bundle", "accepted");

    let evidence_body = "kind=summary&run_id=queue-signed-support-bundle";
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

    let export_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(export_response.starts_with("HTTP/1.1 200 OK"));
    let export: serde_json::Value = serde_json::from_str(http_body(&export_response)).unwrap();
    assert_eq!(export["schema_version"], "ao2.workbench-support-bundle.v1");
    assert_eq!(export["support_metadata"]["present"], true);
    assert_eq!(export["support_metadata"]["signature_verified"], true);
    assert_eq!(export["support_metadata"]["signer_id"], "workbench-lead");
    assert_eq!(
        export["support_metadata"]["signature_algorithm"],
        "RSA/SHA-256"
    );
    assert!(
        export["support_metadata"]["metadata_sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert!(
        export["support_metadata"]["public_key_sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );

    let bundle_path = Path::new(export["bundle_path"].as_str().unwrap());
    assert!(bundle_path.is_file());
    let bundle_dir = bundle_path.parent().unwrap();
    assert!(bundle_dir.join("support-bundle-metadata.json").is_file());
    assert!(bundle_dir
        .join("support-bundle-metadata.json.sig")
        .is_file());
    assert!(bundle_dir
        .join("support-bundle-signing-public.pem")
        .is_file());

    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(bundle_dir.join("support-bundle-metadata.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        metadata["schema_version"],
        "ao2.workbench-support-metadata.v1"
    );
    assert_eq!(metadata["signer_id"], "workbench-lead");
    assert_eq!(metadata["queue_job_count"], 1);
    assert_eq!(metadata["evidence_export_count"], 1);
    assert_eq!(
        metadata["workbench_support_bundle_sha256"],
        export["support_metadata"]["metadata"]["workbench_support_bundle_sha256"]
    );
    let _ = child.kill();
    let _ = child.wait();
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
