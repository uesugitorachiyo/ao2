use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

#[test]
fn cli_factory_project_run_dispatches_app_steps_from_project_plan() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.

Acceptance:
- Dispatch intake app step.
- Dispatch messaging app step.
- Package one project-level release review.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-run-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let intake = write_project_app_step_fixture(temp.path(), "intake");
    let messaging = write_project_app_step_fixture(temp.path(), "messaging");
    let project_plan = temp.path().join("project-plan.json");
    write_signed_project_plan_for_step_fixtures(
        temp.path(),
        &project_spec,
        &signing_key,
        &project_plan,
        &[("intake", &intake), ("messaging", &messaging)],
    );

    let out_dir = temp.path().join("project-run");
    let project_run = ao2([
        "factory",
        "project-run",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-plan",
        project_plan.to_str().unwrap(),
        "--run-id",
        "missed-call-direct-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-run-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(project_run.status.success(), "{}", stderr(&project_run));
    let json: serde_json::Value = serde_json::from_str(&stdout(&project_run)).unwrap();
    assert_eq!(json["schema_version"], "ao2.factory-project-run.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(
        json["project_plan"]["schema_version"],
        "ao2.factory-project-plan.v1"
    );
    assert_eq!(json["app_run_count"], 2);
    assert_eq!(
        json["project_run_checklist"]["ao2_dispatched_project_plan"],
        true
    );
    assert_eq!(
        json["project_run_checklist"]["ao2_collected_app_run_bundles"],
        true
    );
    assert_eq!(
        json["project_run_checklist"]["release_review_package_ready"],
        true
    );
    assert_eq!(
        json["factory_replacement_boundary"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        json["factory_replacement_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        json["factory_replacement_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["factory_replacement_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        json["factory_replacement_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert!(Path::new(json["artifacts"]["factory_project_run"].as_str().unwrap()).is_file());
    assert!(Path::new(
        json["artifacts"]["release_review_package"]
            .as_str()
            .unwrap()
    )
    .is_file());
    let app_runs = json["app_runs"].as_array().unwrap();
    assert!(app_runs
        .iter()
        .all(|item| Path::new(item["app_run"].as_str().unwrap()).is_file()));
    assert!(app_runs
        .iter()
        .all(|item| Path::new(item["bundle"].as_str().unwrap()).is_file()));
}

#[test]
fn cli_factory_project_acceptance_review_verifies_signed_rubric_and_thresholds() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.

Acceptance:
- Dispatch intake app step.
- Dispatch messaging app step.
- Package one project-level release review.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-run-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let review_signing_key = temp.path().join("project-review-signing-key.pem");
    generate_native_signing_key(&review_signing_key, 2048);

    let intake = write_project_app_step_fixture(temp.path(), "intake");
    let messaging = write_project_app_step_fixture(temp.path(), "messaging");
    let project_plan = temp.path().join("project-plan.json");
    write_signed_project_plan_for_step_fixtures(
        temp.path(),
        &project_spec,
        &signing_key,
        &project_plan,
        &[("intake", &intake), ("messaging", &messaging)],
    );

    let out_dir = temp.path().join("project-run");
    let project_run = ao2([
        "factory",
        "project-run",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-plan",
        project_plan.to_str().unwrap(),
        "--run-id",
        "missed-call-review-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-run-review-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(project_run.status.success(), "{}", stderr(&project_run));
    let project_json: serde_json::Value = serde_json::from_str(&stdout(&project_run)).unwrap();
    let project_run_path = Path::new(
        project_json["artifacts"]["factory_project_run"]
            .as_str()
            .unwrap(),
    );
    let review_out = temp.path().join("project-acceptance-review.json");

    let review = ao2([
        "factory",
        "project-acceptance-review",
        "--project-run",
        project_run_path.to_str().unwrap(),
        "--signing-key",
        review_signing_key.to_str().unwrap(),
        "--signer-id",
        "project-acceptance-review-test",
        "--out",
        review_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(review.status.success(), "{}", stderr(&review));
    let review_json: serde_json::Value = serde_json::from_str(&stdout(&review)).unwrap();
    assert_eq!(
        review_json["schema_version"],
        "ao2.factory-project-acceptance-review.v1"
    );
    assert_eq!(review_json["status"], "accepted");
    assert_eq!(review_json["recommended_decision"], "accept");
    assert_eq!(review_json["must_have_artifacts_present"], true);
    assert_eq!(review_json["thresholds_satisfied"], true);
    assert_eq!(review_json["rubric"]["accepted"], true);
    assert_eq!(
        review_json["rubric_sha256"],
        project_json["artifacts"]["acceptance_rubric_sha256"]
    );
    assert_eq!(
        review_json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        review_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(review_json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(review_json["signature"]["signature_status"], "signed");
    assert_eq!(review_json["signature"]["signature_verified"], true);
    assert!(review_out.is_file());
    assert!(!stdout(&review).contains("Bearer "));
}

#[test]
fn cli_factory_project_acceptance_review_rejects_tampered_rubric_digest() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-run-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let review_signing_key = temp.path().join("project-review-signing-key.pem");
    generate_native_signing_key(&review_signing_key, 2048);

    let intake = write_project_app_step_fixture(temp.path(), "intake");
    let messaging = write_project_app_step_fixture(temp.path(), "messaging");
    let project_plan = temp.path().join("project-plan.json");
    write_signed_project_plan_for_step_fixtures(
        temp.path(),
        &project_spec,
        &signing_key,
        &project_plan,
        &[("intake", &intake), ("messaging", &messaging)],
    );
    let out_dir = temp.path().join("project-run");
    let project_run = ao2([
        "factory",
        "project-run",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-plan",
        project_plan.to_str().unwrap(),
        "--run-id",
        "missed-call-review-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-run-review-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(project_run.status.success(), "{}", stderr(&project_run));
    let project_json: serde_json::Value = serde_json::from_str(&stdout(&project_run)).unwrap();
    let project_run_path = Path::new(
        project_json["artifacts"]["factory_project_run"]
            .as_str()
            .unwrap(),
    );
    let mut tampered: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project_run_path).unwrap()).unwrap();
    tampered["artifacts"]["acceptance_rubric_sha256"] = serde_json::Value::String("0".repeat(64));
    let tampered_path = temp.path().join("tampered-project-run.json");
    fs::write(
        &tampered_path,
        serde_json::to_string_pretty(&tampered).unwrap(),
    )
    .unwrap();
    let review_out = temp.path().join("tampered-project-acceptance-review.json");

    let review = ao2([
        "factory",
        "project-acceptance-review",
        "--project-run",
        tampered_path.to_str().unwrap(),
        "--signing-key",
        review_signing_key.to_str().unwrap(),
        "--out",
        review_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(!review.status.success(), "{}", stdout(&review));
    let review_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&review_out).unwrap()).unwrap();
    assert_eq!(review_json["status"], "rejected");
    assert_eq!(review_json["recommended_decision"], "reject");
    assert_eq!(review_json["rubric"]["accepted"], false);
    assert!(review_json["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker
            .as_str()
            .unwrap()
            .contains("acceptance_rubric_sha256 does not match")));
}

#[test]
fn cli_factory_project_run_writes_resumable_state_for_failed_app_step() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.

Acceptance:
- Preserve accepted app-step evidence when a later step fails.
- Resume only rejected app steps after the fix.
- Package one project-level release review after all steps pass.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-run-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let intake = write_project_app_step_fixture(temp.path(), "intake");
    let messaging = write_project_app_step_fixture(temp.path(), "messaging");
    fs::write(
        &messaging.prompt,
        "printf 'Summary: intentionally leaving implementation broken for resume test\\n'\n",
    )
    .unwrap();
    let project_plan = temp.path().join("project-plan.json");
    write_signed_project_plan_for_step_fixtures(
        temp.path(),
        &project_spec,
        &signing_key,
        &project_plan,
        &[("intake", &intake), ("messaging", &messaging)],
    );

    let out_dir = temp.path().join("project-run-failed");
    let failed_run = ao2([
        "factory",
        "project-run",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-plan",
        project_plan.to_str().unwrap(),
        "--run-id",
        "missed-call-resume-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-run-resume-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(failed_run.status.success(), "{}", stderr(&failed_run));
    let failed_json: serde_json::Value = serde_json::from_str(&stdout(&failed_run)).unwrap();
    assert_eq!(failed_json["schema_version"], "ao2.factory-project-run.v1");
    assert_eq!(failed_json["status"], "rejected");
    assert_eq!(failed_json["app_run_count"], 1);
    assert_eq!(failed_json["step_count"], 2);
    assert_eq!(failed_json["failed_step_count"], 1);
    assert_eq!(failed_json["release_review"]["ready"], false);
    assert_eq!(
        failed_json["project_run_checklist"]["release_review_package_ready"],
        false
    );
    assert_eq!(
        failed_json["project_run_checklist"]["ao2_preserved_partial_evidence"],
        true
    );
    assert!(failed_json["artifacts"]["release_review_package"].is_null());
    let state_path = Path::new(
        failed_json["artifacts"]["factory_project_run_state"]
            .as_str()
            .unwrap(),
    );
    assert!(state_path.is_file());
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(state_path).unwrap()).unwrap();
    assert_eq!(state["schema_version"], "ao2.factory-project-run-state.v1");
    assert_eq!(state["status"], "rejected");
    assert_eq!(state["steps"][0]["id"], "intake");
    assert_eq!(state["steps"][0]["status"], "accepted");
    assert!(state["steps"][0]["app_run"]
        .as_str()
        .unwrap()
        .ends_with(".json"));
    assert_eq!(state["steps"][1]["id"], "messaging");
    assert_eq!(state["steps"][1]["status"], "rejected");
    assert!(!fs::read_to_string(state_path).unwrap().contains("Bearer "));

    fs::remove_dir_all(&messaging.target).unwrap();
    write_project_app_step_fixture(temp.path(), "messaging");
    let resume_out_dir = temp.path().join("project-run-resumed");
    let resumed_run = ao2([
        "factory",
        "project-run",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-plan",
        project_plan.to_str().unwrap(),
        "--resume-from",
        state_path.to_str().unwrap(),
        "--run-id",
        "missed-call-resume-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-run-resume-test",
        "--out-dir",
        resume_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(resumed_run.status.success(), "{}", stderr(&resumed_run));
    let resumed_json: serde_json::Value = serde_json::from_str(&stdout(&resumed_run)).unwrap();
    assert_eq!(
        resumed_json["status"],
        "accepted",
        "{}",
        stdout(&resumed_run)
    );
    assert_eq!(resumed_json["app_run_count"], 2);
    assert_eq!(
        resumed_json["project_run_checklist"]["ao2_reused_resume_state"],
        true
    );
    assert_eq!(resumed_json["project_steps"][0]["reused_from_resume"], true);
    assert_eq!(resumed_json["project_steps"][1]["status"], "accepted");
    assert!(Path::new(
        resumed_json["artifacts"]["release_review_package"]
            .as_str()
            .unwrap()
    )
    .is_file());
}

#[test]
fn cli_factory_project_plan_generates_deterministic_project_plan() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

Build a governed missed-call revenue recovery application.

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.

Acceptance:
- AO2 emits a deterministic project plan.
- Factory-v3 remains evaluator-closer owner.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-plan-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let plan_out = project_root.join("project-plan.json");

    let first = ao2([
        "factory",
        "project-plan",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-plan-test",
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(first.status.success(), "{}", stderr(&first));
    let first_json: serde_json::Value = serde_json::from_str(&stdout(&first)).unwrap();
    assert_eq!(first_json["schema_version"], "ao2.factory-project-plan.v1");
    assert_eq!(first_json["status"], "accepted");
    assert_eq!(first_json["run_id"], "missed-call-recovery-project");
    assert_eq!(first_json["app_steps"].as_array().unwrap().len(), 2);
    assert_eq!(first_json["app_steps"][0]["id"], "intake");
    assert_eq!(first_json["app_steps"][1]["id"], "messaging");
    assert_eq!(
        first_json["app_steps"][0]["verifier_command"],
        "npm run verify"
    );
    assert_eq!(
        first_json["factory_replacement_boundary"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        first_json["factory_replacement_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        first_json["factory_replacement_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert_eq!(
        first_json["acceptance_rubric"]["schema_version"],
        "ao2.factory-acceptance-rubric.v1"
    );
    assert_eq!(first_json["acceptance_rubric"]["status"], "accepted");
    assert_eq!(
        first_json["acceptance_rubric"]["signature"]["signature_status"],
        "signed"
    );
    assert_eq!(
        first_json["acceptance_rubric"]["signature"]["signature_verified"],
        true
    );
    assert_eq!(
        first_json["acceptance_rubric"]["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        first_json["acceptance_rubric"]["trust_boundary"]["control_plane_approves_release"],
        false
    );
    let rubric_path = Path::new(
        first_json["artifacts"]["acceptance_rubric"]
            .as_str()
            .unwrap(),
    );
    assert!(rubric_path.is_file());
    assert_eq!(
        first_json["artifacts"]["acceptance_rubric_sha256"],
        sha256_path(rubric_path)
    );
    assert_eq!(
        first_json["acceptance_rubric_sha256"],
        first_json["artifacts"]["acceptance_rubric_sha256"]
    );
    for step in first_json["app_steps"].as_array().unwrap() {
        assert_eq!(
            step["acceptance_rubric_sha256"],
            first_json["acceptance_rubric_sha256"]
        );
    }
    for step in first_json["app_steps"].as_array().unwrap() {
        assert!(Path::new(step["spec"].as_str().unwrap()).is_file());
        assert!(Path::new(step["target"].as_str().unwrap()).is_dir());
        let step_spec = fs::read_to_string(step["spec"].as_str().unwrap()).unwrap();
        assert!(step_spec.contains("Missed Call Recovery Project"));
        assert!(!step_spec.contains("Bearer "));
    }
    assert!(Path::new(first_json["artifacts"]["project_plan"].as_str().unwrap()).is_file());

    let first_plan_text = fs::read_to_string(&plan_out).unwrap();
    let second = ao2([
        "factory",
        "project-plan",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-plan-test",
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(fs::read_to_string(&plan_out).unwrap(), first_plan_text);
}

#[test]
fn cli_factory_project_plan_validate_accepts_generated_plan_and_rejects_unsafe_plan() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let project_root = temp.path().join("generated-project");
    let plan_out = project_root.join("project-plan.json");
    let signing_key = temp.path().join("project-plan-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let generated = ao2([
        "factory",
        "project-plan",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(generated.status.success(), "{}", stderr(&generated));

    let validation_out = project_root.join("project-plan-validation.json");
    let accepted = ao2([
        "factory",
        "project-plan-validate",
        "--project-plan",
        plan_out.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--out",
        validation_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(accepted.status.success(), "{}", stderr(&accepted));
    let accepted_json: serde_json::Value = serde_json::from_str(&stdout(&accepted)).unwrap();
    assert_eq!(
        accepted_json["schema_version"],
        "ao2.factory-project-plan-validation.v1"
    );
    assert_eq!(accepted_json["status"], "accepted");
    assert_eq!(accepted_json["app_step_count"], 2);
    assert_eq!(
        accepted_json["checks"]["all_paths_within_project_root"],
        true
    );
    assert_eq!(
        accepted_json["checks"]["control_plane_remains_observer"],
        true
    );
    assert_eq!(accepted_json["checks"]["signed_acceptance_rubric"], true);
    assert_eq!(
        accepted_json["rubric"]["rubric_schema"],
        "ao2.factory-acceptance-rubric.v1"
    );
    assert_eq!(
        accepted_json["rubric"]["sha256"],
        sha256_path(Path::new(accepted_json["rubric"]["path"].as_str().unwrap()))
    );
    assert_eq!(
        accepted_json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert!(Path::new(accepted_json["artifacts"]["validation"].as_str().unwrap()).is_file());

    let mut unsafe_plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&plan_out).unwrap()).unwrap();
    unsafe_plan["app_steps"][0]["spec"] =
        serde_json::Value::String(temp.path().join("outside-spec.md").display().to_string());
    unsafe_plan["factory_replacement_boundary"]["control_plane_approves_release"] =
        serde_json::Value::Bool(true);
    unsafe_plan["acceptance_rubric_sha256"] = serde_json::Value::String("0".repeat(64));
    let unsafe_plan_out = project_root.join("unsafe-project-plan.json");
    fs::write(
        &unsafe_plan_out,
        serde_json::to_string_pretty(&unsafe_plan).unwrap(),
    )
    .unwrap();
    let unsafe_validation_out = project_root.join("unsafe-project-plan-validation.json");
    let rejected = ao2([
        "factory",
        "project-plan-validate",
        "--project-plan",
        unsafe_plan_out.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--out",
        unsafe_validation_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(!rejected.status.success(), "{}", stdout(&rejected));
    let rejected_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&unsafe_validation_out).unwrap()).unwrap();
    assert_eq!(rejected_json["status"], "rejected");
    assert_eq!(
        rejected_json["checks"]["all_paths_within_project_root"],
        false
    );
    assert_eq!(
        rejected_json["checks"]["control_plane_remains_observer"],
        false
    );
    assert!(rejected_json["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| blocker.as_str().unwrap().contains("escapes project root")));
}

#[test]
fn cli_factory_project_plan_materializes_provider_prompt_scaffolds() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let project_root = temp.path().join("generated-project");
    let prompt_dir = project_root.join("provider-prompts");
    let plan_out = project_root.join("project-plan.json");
    let signing_key = temp.path().join("project-plan-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let generated = ao2([
        "factory",
        "project-plan",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--provider",
        "codex",
        "--provider-prompt-dir",
        prompt_dir.to_str().unwrap(),
        "--verifier-command",
        "python -m pytest -q",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(generated.status.success(), "{}", stderr(&generated));
    let generated_json: serde_json::Value = serde_json::from_str(&stdout(&generated)).unwrap();
    let canonical_project_root = fs::canonicalize(&project_root).unwrap();
    for step in generated_json["app_steps"].as_array().unwrap() {
        let prompt_path = Path::new(step["provider_prompt_file"].as_str().unwrap());
        assert!(prompt_path.is_file());
        assert!(prompt_path.starts_with(&canonical_project_root));
        let prompt = fs::read_to_string(prompt_path).unwrap();
        assert!(prompt.contains("Missed Call Recovery Project"));
        assert!(prompt.contains("python -m pytest -q"));
        assert!(prompt.contains("local OAuth CLI only"));
        assert!(prompt.contains("factory-v3 evaluator-closer"));
        assert!(prompt.contains(step["spec"].as_str().unwrap()));
        assert!(!prompt.contains("Bearer "));
    }

    let validation_out = project_root.join("project-plan-validation.json");
    let validated = ao2([
        "factory",
        "project-plan-validate",
        "--project-plan",
        plan_out.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--out",
        validation_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(validated.status.success(), "{}", stderr(&validated));
    let validated_json: serde_json::Value = serde_json::from_str(&stdout(&validated)).unwrap();
    assert_eq!(validated_json["status"], "accepted");
    assert_eq!(validated_json["checks"]["all_required_files_exist"], true);
}

#[test]
fn cli_factory_project_start_chains_plan_validate_and_project_run() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-start-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("project-start");
    let handoff_bundle = temp.path().join("handoff/project-start-handoff.tgz");
    let handoff_bundle_report = temp
        .path()
        .join("handoff/factory-project-start-bundle.json");

    let started = ao2([
        "factory",
        "project-start",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-start-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--handoff-bundle-out",
        handoff_bundle.to_str().unwrap(),
        "--handoff-bundle-report",
        handoff_bundle_report.to_str().unwrap(),
        "--json",
    ]);
    assert!(started.status.success(), "{}", stderr(&started));
    let json: serde_json::Value = serde_json::from_str(&stdout(&started)).unwrap();
    assert_eq!(json["schema_version"], "ao2.factory-project-start.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], "missed-call-recovery-project");
    assert_eq!(json["app_run_count"], 2);
    assert_eq!(json["step_count"], 2);
    assert_eq!(json["failed_step_count"], 0);
    assert_eq!(json["checks"]["project_plan_status"], "accepted");
    assert_eq!(json["checks"]["project_plan_validation_status"], "accepted");
    assert_eq!(json["checks"]["project_run_status"], "accepted");
    assert_eq!(json["checks"]["release_review_package_ready"], true);
    assert_eq!(
        json["checks"]["project_acceptance_review_status"],
        "accepted"
    );
    assert_eq!(
        json["checks"]["project_acceptance_review_recommended_decision"],
        "accept"
    );
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    for key in [
        "project_plan",
        "project_plan_validation",
        "factory_project_run",
        "factory_project_run_state",
        "project_acceptance_review",
        "release_review_package",
    ] {
        assert!(
            Path::new(json["artifacts"][key].as_str().unwrap()).is_file(),
            "missing artifact {key}"
        );
        assert!(!json["artifacts"][format!("{key}_sha256")]
            .as_str()
            .unwrap()
            .is_empty());
    }
    assert_eq!(
        json["project_acceptance_review"]["schema_version"],
        "ao2.factory-project-acceptance-review.v1"
    );
    assert_eq!(json["project_acceptance_review"]["status"], "accepted");
    assert_eq!(
        json["project_acceptance_review"]["recommended_decision"],
        "accept"
    );
    assert_eq!(
        json["project_acceptance_review"]["rubric_sha256"],
        json["artifacts"]["acceptance_rubric_sha256"]
    );
    assert_eq!(
        json["project_acceptance_review"]["signature"]["signature_status"],
        "signed"
    );
    assert_eq!(
        json["project_acceptance_review"]["signature"]["signature_verified"],
        true
    );
    assert!(Path::new(json["artifacts"]["factory_project_start"].as_str().unwrap()).is_file());
    assert!(
        !fs::read_to_string(json["artifacts"]["factory_project_start"].as_str().unwrap())
            .unwrap()
            .contains("Bearer ")
    );
    assert_eq!(
        json["project_start_bundle"]["schema_version"],
        "ao2.factory-project-start-bundle.v1"
    );
    assert_eq!(json["project_start_bundle"]["status"], "bundled");
    assert_eq!(
        json["hermes_queue_handoff"]["schema_version"],
        "ao2.hermes-project-start-handoff.v1"
    );
    assert_eq!(json["hermes_queue_handoff"]["status"], "ready");
    assert_eq!(
        json["hermes_queue_handoff"]["project_start_bundle"],
        json["project_start_bundle"]["archive"]
    );
    assert_eq!(
        json["hermes_queue_handoff"]["project_start_bundle_sha256"],
        json["project_start_bundle"]["sha256"]
    );
    assert_eq!(
        json["hermes_queue_handoff"]["handoff_entry"],
        "handoff.json"
    );
    assert_eq!(
        json["hermes_queue_handoff"]["manifest_entry"],
        "manifest.json"
    );
    assert_eq!(json["hermes_queue_handoff"]["checksum_entry"], "SHA256SUMS");
    assert_eq!(
        json["hermes_queue_handoff"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        json["hermes_queue_handoff"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        json["hermes_queue_handoff"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["hermes_queue_handoff"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["hermes_queue_handoff"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["artifacts"]["project_start_bundle"],
        json["project_start_bundle"]["archive"]
    );
    assert_eq!(
        json["artifacts"]["project_start_bundle_sha256"],
        json["project_start_bundle"]["sha256"]
    );
    assert!(Path::new(json["project_start_bundle"]["archive"].as_str().unwrap()).is_file());
    assert!(handoff_bundle_report.is_file());
    let persisted_bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&handoff_bundle_report).unwrap()).unwrap();
    assert_eq!(persisted_bundle, json["project_start_bundle"]);
    assert!(!stdout(&started).contains("Bearer "));
}

#[test]
fn cli_factory_project_start_bundle_packages_handoff_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-start-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("project-start");

    let started = ao2([
        "factory",
        "project-start",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-start-bundle-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(started.status.success(), "{}", stderr(&started));
    let started_json: serde_json::Value = serde_json::from_str(&stdout(&started)).unwrap();
    let bundle_out = temp.path().join("project-start-handoff.tgz");
    let bundled = ao2([
        "factory",
        "project-start-bundle",
        "--project-start",
        started_json["artifacts"]["factory_project_start"]
            .as_str()
            .unwrap(),
        "--out",
        bundle_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundled.status.success(), "{}", stderr(&bundled));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundled)).unwrap();
    assert_eq!(
        bundle_json["schema_version"],
        "ao2.factory-project-start-bundle.v1"
    );
    assert_eq!(bundle_json["status"], "bundled");
    assert!(Path::new(bundle_json["archive"].as_str().unwrap()).is_file());
    assert_eq!(bundle_json["manifest_entry"], "manifest.json");
    assert_eq!(bundle_json["checksum_entry"], "SHA256SUMS");
    assert_eq!(bundle_json["handoff_entry"], "handoff.json");
    assert_eq!(
        bundle_json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        bundle_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(bundle_json["trust_boundary"]["mutates_ao_artifacts"], false);
    let labels: Vec<_> = bundle_json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["label"].as_str().unwrap())
        .collect();
    for label in [
        "factory-project-start",
        "project-plan",
        "acceptance-rubric",
        "project-plan-validation",
        "factory-project-run",
        "factory-project-run-state",
        "project-acceptance-review",
        "release-review-package",
        "app-run-bundle",
    ] {
        assert!(labels.contains(&label), "missing label {label}");
    }
}

#[test]
fn cli_factory_project_start_bundle_verify_accepts_detached_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-start-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("project-start");
    let bundle_out = temp.path().join("project-start-handoff.tgz");
    let bundle_report = temp.path().join("factory-project-start-bundle.json");

    let started = ao2([
        "factory",
        "project-start",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-start-bundle-verify-test",
        "--handoff-bundle-out",
        bundle_out.to_str().unwrap(),
        "--handoff-bundle-report",
        bundle_report.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(started.status.success(), "{}", stderr(&started));
    assert!(bundle_out.is_file());

    let verified = ao2([
        "factory",
        "project-start-bundle-verify",
        "--bundle",
        bundle_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(verified.status.success(), "{}", stderr(&verified));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verified)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-bundle-verification.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["checks"]["manifest_verified"], true);
    assert_eq!(json["checks"]["sha256sums_verified"], true);
    assert_eq!(json["checks"]["project_start_verified"], true);
    assert_eq!(json["checks"]["project_run_verified"], true);
    assert_eq!(json["checks"]["acceptance_rubric_verified"], true);
    assert_eq!(json["checks"]["project_acceptance_review_verified"], true);
    assert_eq!(json["checks"]["acceptance_rubric_signature_verified"], true);
    assert_eq!(
        json["checks"]["project_acceptance_review_signature_verified"],
        true
    );
    assert_eq!(json["checks"]["review_rubric_digest_matches"], true);
    assert_eq!(json["checks"]["review_project_run_digest_matches"], true);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(json["failure_count"], 0);
    assert!(!stdout(&verified).contains("Bearer "));
}

#[test]
fn cli_factory_project_start_summary_links_operator_handoff_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-start-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("project-start");
    let bundle_out = temp.path().join("project-start-handoff.tgz");
    let bundle_report = temp.path().join("factory-project-start-bundle.json");
    let bundle_verification = temp
        .path()
        .join("factory-project-start-bundle-verification.json");
    let summary_out = temp.path().join("project-start-summary.json");
    let markdown_out = temp.path().join("project-start-summary.md");

    let started = ao2([
        "factory",
        "project-start",
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery-project",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-start-summary-test",
        "--handoff-bundle-out",
        bundle_out.to_str().unwrap(),
        "--handoff-bundle-report",
        bundle_report.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(started.status.success(), "{}", stderr(&started));
    let started_json: serde_json::Value = serde_json::from_str(&stdout(&started)).unwrap();

    let verified = ao2([
        "factory",
        "project-start-bundle-verify",
        "--bundle",
        bundle_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(verified.status.success(), "{}", stderr(&verified));
    fs::write(&bundle_verification, stdout(&verified)).unwrap();

    let summarized = ao2([
        "factory",
        "project-start-summary",
        "--project-start",
        started_json["artifacts"]["factory_project_start"]
            .as_str()
            .unwrap(),
        "--bundle-verification",
        bundle_verification.to_str().unwrap(),
        "--out",
        summary_out.to_str().unwrap(),
        "--markdown",
        markdown_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(summarized.status.success(), "{}", stderr(&summarized));
    let json: serde_json::Value = serde_json::from_str(&stdout(&summarized)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-operator-summary.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], "missed-call-recovery-project");
    assert_eq!(json["bundle_verification_status"], "accepted");
    for key in [
        "project_plan",
        "acceptance_rubric",
        "project_run",
        "release_review_package",
        "project_acceptance_review",
        "project_start_bundle",
        "project_start_bundle_verification",
    ] {
        assert_eq!(json["artifacts"][key]["exists"], true, "missing {key}");
        assert_eq!(
            json["artifacts"][key]["sha256"], json["artifacts"][key]["expected_sha256"],
            "digest mismatch for {key}"
        );
    }
    assert_eq!(
        json["artifacts"]["project_start_bundle_verification"]["status"],
        "accepted"
    );
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(json["failure_count"], 0);
    assert!(summary_out.is_file());
    assert!(markdown_out.is_file());
    let markdown = fs::read_to_string(&markdown_out).unwrap();
    assert!(markdown.contains("Project-Start Operator Summary"));
    assert!(markdown.contains("project_start_bundle_verification"));
    assert!(!stdout(&summarized).contains("Bearer "));
    assert!(!markdown.contains("Bearer "));
}

#[test]
fn cli_factory_project_start_hermes_flow_contract_emits_deterministic_frontend_contract() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out = temp.path().join("hermes-project-start-flow-contract.json");

    let first = ao2([
        "factory",
        "project-start-hermes-flow-contract",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(first.status.success(), "{}", stderr(&first));
    let first_json: serde_json::Value = serde_json::from_str(&stdout(&first)).unwrap();
    assert_eq!(
        first_json["schema_version"],
        "ao2.hermes-project-start-flow-contract.v1"
    );
    assert_eq!(first_json["status"], "ready");
    assert_eq!(first_json["contract_path"], out.display().to_string());
    assert_sha256_string(&first_json["contract_sha256"], "contract_sha256");
    assert_eq!(first_json["workflow"]["preview"]["method"], "GET");
    assert_eq!(
        first_json["workflow"]["preview"]["endpoint"],
        "/api/factory/project-start/next-action"
    );
    assert_eq!(first_json["workflow"]["preview"]["minimum_role"], "viewer");
    assert_eq!(first_json["workflow"]["publish"]["method"], "POST");
    assert_eq!(
        first_json["workflow"]["publish"]["endpoint"],
        "/api/factory/project-start/operator-record"
    );
    assert_eq!(
        first_json["workflow"]["publish"]["minimum_role"],
        "operator"
    );
    assert_eq!(
        first_json["workflow"]["publish"]["only_when_next_action"],
        "publish_operator_record"
    );
    assert_eq!(
        first_json["hermes_contract"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(first_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        first_json["side_effects"]["would_submit_queue_entry"],
        false
    );
    assert_eq!(first_json["side_effects"]["would_rebuild_wrappers"], false);
    assert_eq!(
        first_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(
        first_json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        first_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(first_json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        first_json["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );

    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(written, first_json["contract"]);
    let first_sha = sha256_path(&out);

    let second = ao2([
        "factory",
        "project-start-hermes-flow-contract",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(second.status.success(), "{}", stderr(&second));
    let second_json: serde_json::Value = serde_json::from_str(&stdout(&second)).unwrap();
    assert_eq!(
        second_json["contract_sha256"],
        first_json["contract_sha256"]
    );
    assert_eq!(sha256_path(&out), first_sha);
}

#[test]
fn cli_factory_project_start_hermes_context_returns_read_only_memory_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let (repo, _bundle_dir) = create_signed_workbench_support_bundle_with_evidence(
        temp.path(),
        "hermes-context-checkpoint",
        "hermes-context-lead",
    );
    let support_dir = repo.join(".ao2").join("workbench").join("support-bundles");
    let before_bundle_count = fs::read_dir(&support_dir).unwrap().count();

    let context = ao2([
        "factory",
        "project-start-hermes-context",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(context.status.success(), "{}", stderr(&context));
    let json: serde_json::Value = serde_json::from_str(&stdout(&context)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-hermes-context.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(
        json["flow_contract"]["schema_version"],
        "ao2.hermes-project-start-flow-contract.v1"
    );
    assert_sha256_string(&json["flow_contract"]["contract_sha256"], "contract_sha256");
    assert_eq!(
        json["flow_contract"]["workflow"]["preview"]["minimum_role"],
        "viewer"
    );
    assert_eq!(
        json["flow_contract"]["workflow"]["publish"]["minimum_role"],
        "operator"
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["schema_version"],
        "ao2.hermes-greenfield-spec-ingest-entrypoint.v1"
    );
    assert_eq!(json["greenfield_spec_ingest"]["status"], "ready");
    assert_eq!(json["greenfield_spec_ingest"]["preview"]["method"], "GET");
    assert_eq!(
        json["greenfield_spec_ingest"]["preview"]["path"],
        "/api/factory/greenfield-spec-ingest"
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["preview"]["minimum_role"],
        "viewer"
    );
    assert_eq!(json["greenfield_spec_ingest"]["submit"]["method"], "POST");
    assert_eq!(
        json["greenfield_spec_ingest"]["submit"]["path"],
        "/api/factory/greenfield-spec-ingest/submit"
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["submit"]["minimum_role"],
        "operator"
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["submit"]["approval_mode"],
        "exact_action_digest"
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["side_effects"]["would_write_files"],
        false
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["side_effects"]["would_execute_provider"],
        false
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["side_effects"]["would_execute_queue"],
        false
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["side_effects"]["would_submit_queue_entry_after_approval"],
        true
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        json["greenfield_spec_ingest"]["trust_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert_eq!(json["latest_support_packet"]["present"], true);
    assert_eq!(
        json["latest_support_packet"]["hermes_project_start_flow_contract"]["present"],
        true
    );
    assert_eq!(
        json["latest_support_packet"]["hermes_project_start_flow_contract"]["preview_role"],
        "viewer"
    );
    assert_eq!(
        json["latest_support_packet"]["hermes_project_start_flow_contract"]["publish_role"],
        "operator"
    );
    assert_eq!(
        json["latest_support_packet"]["hermes_project_start_flow_contract"]
            ["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(json["side_effects"]["would_write_files"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_rebuild_wrappers"], false);
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
        fs::read_dir(&support_dir).unwrap().count(),
        before_bundle_count
    );
}

#[test]
fn cli_factory_project_start_closure_packages_queue_status_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("missed-call-project.md");
    fs::write(
        &project_spec,
        r#"# Missed Call Recovery Project

## App Steps

- Intake workflow captures missed-call lead data.
- Messaging workflow produces reviewable recovery copy.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("project-start-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("queued-generated-project");
    let out_dir = temp.path().join("queued-project-start");

    let submit = ao2([
        "factory",
        "queue-submit-project-start",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "closure-project-start",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "closure-project-start-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));

    let queue_status_path = temp.path().join("factory-queue-project-start-status.json");
    let queue_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "closure-project-start",
        "--json",
    ]);
    assert!(queue_status.status.success(), "{}", stderr(&queue_status));
    fs::write(&queue_status_path, stdout(&queue_status)).unwrap();

    let latest_status_path = temp
        .path()
        .join("factory-queue-project-start-latest-status.json");
    let latest_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(latest_status.status.success(), "{}", stderr(&latest_status));
    fs::write(&latest_status_path, stdout(&latest_status)).unwrap();

    let closure_archive = temp.path().join("project-start-closure.tgz");
    let closed = ao2([
        "factory",
        "project-start-closure",
        "--queue-status",
        queue_status_path.to_str().unwrap(),
        "--latest-queue-status",
        latest_status_path.to_str().unwrap(),
        "--out",
        closure_archive.to_str().unwrap(),
        "--json",
    ]);
    assert!(closed.status.success(), "{}", stderr(&closed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&closed)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-closure.v1"
    );
    assert_eq!(json["status"], "packaged");
    assert_eq!(json["run_id"], "closure-project-start");
    assert_eq!(json["queue_status"], "accepted");
    assert_eq!(json["latest_queue_status"], "accepted");
    assert_eq!(json["latest_selector_matches_run_id_selector"], true);
    assert_eq!(json["manifest_entry"], "manifest.json");
    assert_eq!(json["checksum_entry"], "SHA256SUMS");
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(Path::new(json["archive"].as_str().unwrap()).is_file());

    let entries = archive_entries(Path::new(json["archive"].as_str().unwrap()));
    for expected in [
        "manifest.json",
        "SHA256SUMS",
        "closure.json",
        "queue-status/factory-queue-project-start-status.json",
        "queue-status/factory-queue-project-start-latest-status.json",
        "artifacts/project-start-operator-summary.json",
        "artifacts/project-start-bundle-verification.json",
        "artifacts/project-acceptance-review.json",
        "artifacts/acceptance-rubric.json",
        "artifacts/project-start-handoff.tgz",
    ] {
        assert!(entries.iter().any(|entry| entry == expected), "{expected}");
    }
    let manifest_text = archive_text_entry(
        Path::new(json["archive"].as_str().unwrap()),
        "manifest.json",
    );
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(
        manifest["schema_version"],
        "ao2.factory-project-start-closure.v1"
    );
    assert_eq!(manifest["latest_selector_matches_run_id_selector"], true);
    assert_eq!(
        manifest["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    let checksums = archive_text_entry(Path::new(json["archive"].as_str().unwrap()), "SHA256SUMS");
    assert!(checksums.contains("closure.json"));
    assert!(checksums.contains("queue-status/factory-queue-project-start-latest-status.json"));
    assert!(!stdout(&closed).contains("Bearer "));
    assert!(!manifest_text.contains("BEGIN PRIVATE KEY"));

    let relocated_dir = temp.path().join("relocated-closure-review");
    fs::create_dir_all(&relocated_dir).unwrap();
    let relocated_archive = relocated_dir.join("project-start-closure.tgz");
    fs::copy(&closure_archive, &relocated_archive).unwrap();
    let verified = ao2([
        "factory",
        "project-start-closure-verify",
        "--bundle",
        relocated_archive.to_str().unwrap(),
        "--json",
    ]);
    assert!(verified.status.success(), "{}", stderr(&verified));
    let closure_verification_path =
        relocated_dir.join("factory-project-start-closure-verification.json");
    fs::write(&closure_verification_path, stdout(&verified)).unwrap();
    let verified_json: serde_json::Value = serde_json::from_str(&stdout(&verified)).unwrap();
    assert_eq!(
        verified_json["schema_version"],
        "ao2.factory-project-start-closure-verification.v1"
    );
    assert_eq!(verified_json["status"], "accepted");
    assert_eq!(verified_json["run_id"], "closure-project-start");
    assert_eq!(
        verified_json["bundle"],
        relocated_archive.display().to_string()
    );
    assert_eq!(verified_json["checks"]["manifest_verified"], true);
    assert_eq!(verified_json["checks"]["checksums_verified"], true);
    assert_eq!(verified_json["checks"]["closure_verified"], true);
    assert_eq!(
        verified_json["checks"]["latest_selector_matches_run_id_selector"],
        true
    );
    assert_eq!(
        verified_json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        verified_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        verified_json["trust_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert!(!stdout(&verified).contains("Bearer "));

    let replacement_packet_archive = temp.path().join("factory-replacement-packet.tgz");
    let replacement_packet = ao2([
        "factory",
        "replacement-packet",
        "--queue-status",
        queue_status_path.to_str().unwrap(),
        "--latest-queue-status",
        latest_status_path.to_str().unwrap(),
        "--closure",
        relocated_archive.to_str().unwrap(),
        "--closure-verification",
        closure_verification_path.to_str().unwrap(),
        "--out",
        replacement_packet_archive.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        replacement_packet.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&replacement_packet),
        stderr(&replacement_packet)
    );
    let packet_json: serde_json::Value =
        serde_json::from_str(&stdout(&replacement_packet)).unwrap();
    assert_eq!(
        packet_json["schema_version"],
        "ao2.factory-replacement-packet.v1"
    );
    assert_eq!(packet_json["status"], "packaged");
    assert_eq!(packet_json["run_id"], "closure-project-start");
    assert_eq!(packet_json["checks"]["queue_status_accepted"], true);
    assert_eq!(
        packet_json["checks"]["latest_selector_matches_run_id_selector"],
        true
    );
    assert_eq!(packet_json["checks"]["closure_verification_accepted"], true);
    assert_eq!(
        packet_json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        packet_json["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        packet_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(packet_json["trust_boundary"]["mutates_ao_artifacts"], false);

    let packet_archive = Path::new(packet_json["archive"].as_str().unwrap());
    assert!(packet_archive.is_file());
    let packet_entries = archive_entries(packet_archive);
    for expected in [
        "manifest.json",
        "SHA256SUMS",
        "replacement-packet.json",
        "queue-status/factory-queue-project-start-status.json",
        "queue-status/factory-queue-project-start-latest-status.json",
        "artifacts/project-start-closure.tgz",
        "artifacts/project-start-closure-verification.json",
        "artifacts/project-start-operator-summary.json",
        "artifacts/project-start-bundle-verification.json",
        "artifacts/project-acceptance-review.json",
        "artifacts/acceptance-rubric.json",
    ] {
        assert!(
            packet_entries.iter().any(|entry| entry == expected),
            "{expected}"
        );
    }
    let packet_manifest_text = archive_text_entry(packet_archive, "manifest.json");
    let packet_manifest: serde_json::Value = serde_json::from_str(&packet_manifest_text).unwrap();
    assert_eq!(
        packet_manifest["schema_version"],
        "ao2.factory-replacement-packet.v1"
    );
    assert_eq!(
        packet_manifest["replacement_summary"]["ao2_replaces_factory_v3_workflow_driver"],
        true
    );
    assert_eq!(
        packet_manifest["replacement_summary"]["factory_v3_role"],
        "evaluator_closer_and_sampling_auditor"
    );
    let packet_checksums = archive_text_entry(packet_archive, "SHA256SUMS");
    assert!(packet_checksums.contains("replacement-packet.json"));
    assert!(packet_checksums.contains("artifacts/project-start-closure-verification.json"));
    assert!(!stdout(&replacement_packet).contains("Bearer "));
    assert!(!packet_manifest_text.contains("BEGIN PRIVATE KEY"));

    let verified_packet = ao2([
        "factory",
        "replacement-packet-verify",
        "--bundle",
        packet_archive.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verified_packet.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&verified_packet),
        stderr(&verified_packet)
    );
    let verified_packet_json: serde_json::Value =
        serde_json::from_str(&stdout(&verified_packet)).unwrap();
    assert_eq!(
        verified_packet_json["schema_version"],
        "ao2.factory-replacement-packet-verification.v1"
    );
    assert_eq!(verified_packet_json["status"], "accepted");
    assert_eq!(verified_packet_json["run_id"], "closure-project-start");
    assert_eq!(verified_packet_json["failure_count"], 0);
    assert_eq!(verified_packet_json["checks"]["checksums_verified"], true);
    assert_eq!(verified_packet_json["checks"]["manifest_verified"], true);
    assert_eq!(verified_packet_json["checks"]["packet_verified"], true);
    assert_eq!(
        verified_packet_json["checks"]["trust_boundary_verified"],
        true
    );
    assert_eq!(verified_packet_json["checks"]["secret_scan_passed"], true);
    assert_eq!(
        verified_packet_json["checks"]["ao2_replacement_driver_verified"],
        true
    );
    assert_eq!(
        verified_packet_json["checks"]["factory_v3_evaluator_closer_verified"],
        true
    );
    assert!(!stdout(&verified_packet).contains("Bearer "));
}
