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

fn write_probe_queue(repo: &Path, run_id: &str, status: &str, job_kind: &str) -> PathBuf {
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    fs::create_dir_all(queue_path.parent().unwrap()).unwrap();
    let entries = if run_id.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "schema_version": "ao2.factory-project-start-workbench-queue-entry.v1",
            "run_id": run_id,
            "job_kind": job_kind,
            "status": status,
            "attempts": 0
        })]
    };
    fs::write(
        &queue_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-workbench-queue.v1",
            "owner": "ao2-workbench-queue",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "entry_count": entries.len(),
            "entries": entries
        }))
        .unwrap(),
    )
    .unwrap();
    queue_path
}

fn write_probe_compact_artifacts(out_dir: &Path, run_id: &str) {
    fs::create_dir_all(out_dir).unwrap();
    for (file, body) in [
        (
            "factory-queue-project-start-submit.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-workbench-queue-submit.v1",
                "status": "queued",
                "job_kind": "factory_project_start",
                "run_id": run_id
            }),
        ),
        (
            "factory-queue-project-start-run-next.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-workbench-queue-run-next.v1",
                "status": "accepted",
                "job_kind": "factory_project_start",
                "run_id": run_id
            }),
        ),
        (
            "factory-queue-project-start-completion-contract.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-queue-completion-contract.v1",
                "status": "accepted",
                "job_kind": "factory_project_start",
                "run_id": run_id
            }),
        ),
        (
            "factory-queue-project-start-completion-contract-consumer.json",
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-queue-completion-contract-consumption.v1",
                "status": "accepted",
                "ready_for_operator_review": true,
                "run_id": run_id,
                "trust_boundary": {
                    "release_acceptance_owner": "factory-v3 evaluator-closer",
                    "control_plane_approves_release": false,
                    "mutates_ao_artifacts": false
                }
            }),
        ),
    ] {
        fs::write(
            out_dir.join(file),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
    }
}

fn project_start_complete_status_probe(
    repo: &Path,
    out_dir: &Path,
    run_id: &str,
) -> serde_json::Value {
    let output = ao2([
        "factory",
        "queue-project-start-complete-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).unwrap()
}

fn assert_blocker_code(value: &serde_json::Value, code: &str) {
    let codes = value["blocker_codes"].as_array().unwrap();
    assert!(
        codes
            .iter()
            .any(|candidate| candidate.as_str() == Some(code)),
        "missing blocker code {code}: {value:#}"
    );
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

fn project_start_next_action(
    repo: &Path,
    out_dir: &Path,
    run_id: &str,
    contract: &Path,
) -> serde_json::Value {
    let output = ao2([
        "factory",
        "queue-project-start-next-action",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    serde_json::from_str(&stdout(&output)).unwrap()
}

#[test]
fn cli_workbench_project_start_factory_next_action_api_reuses_read_only_preview() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let spaced_root = temp.path().join("AI Agent Teams").join("ao-2");
    let out_dir = spaced_root.join("workbench-next-action-out");
    let run_id = "workbench-project-start-next-action";
    let source_contract = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/contracts/hermes-project-start-poll-act-contract.v1.json");
    let contract = spaced_root
        .join("docs")
        .join("hermes-project-start-poll-act-contract.v1.json");
    fs::create_dir_all(contract.parent().unwrap()).unwrap();
    fs::copy(&source_contract, &contract).unwrap();
    let route = format!(
        "/api/factory/project-start/next-action?token=viewer-token&run_id={run_id}&out_dir={}&contract={}",
        out_dir.display(),
        contract.display()
    );
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");

    let mut denied_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
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
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let denied_port = read_server_port(&mut denied_child);
    let denied_response = http_request(
        denied_port,
        &format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            route.replace("token=viewer-token", "token=bad-token")
        ),
    );
    let denied_status = denied_child.wait().unwrap();
    let mut denied_stderr = String::new();
    denied_child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut denied_stderr)
        .unwrap();
    assert!(denied_status.success());
    assert!(
        denied_response.starts_with("HTTP/1.1 403 Forbidden"),
        "{denied_response}"
    );
    assert!(!denied_stderr.contains("api_token="), "{denied_stderr}");
    assert!(!denied_stderr.contains("operator-token"), "{denied_stderr}");
    assert!(!denied_stderr.contains("viewer-token"), "{denied_stderr}");
    let denied: serde_json::Value = serde_json::from_str(http_body(&denied_response)).unwrap();
    assert_eq!(denied["schema_version"], "ao2.workbench-error.v1");
    assert_eq!(denied["error"], "invalid_api_token");
    assert!(!queue_path.exists(), "denied preview must not create queue");
    assert!(
        !out_dir.exists(),
        "denied preview must not create artifacts"
    );

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
        &format!("GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    );
    let missing_status = missing_child.wait().unwrap();
    assert!(missing_status.success());
    assert!(
        missing_response.starts_with("HTTP/1.1 200 OK"),
        "{missing_response}"
    );
    let missing: serde_json::Value = serde_json::from_str(http_body(&missing_response)).unwrap();
    assert_eq!(
        missing["schema_version"],
        "ao2.factory-project-start-next-action.v1"
    );
    assert_eq!(missing["next_action"], "call_queue_project_start_complete");
    assert_eq!(
        missing["status_probe"]["completion_record_state"],
        "missing_queue_file"
    );
    assert_eq!(missing["read_only"], true);
    assert_eq!(missing["would_execute_queue"], false);
    assert_eq!(missing["would_submit_queue_entry"], false);
    assert_eq!(missing["would_rebuild_wrappers"], false);
    assert_eq!(
        missing["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        missing["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(missing["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(!queue_path.exists(), "preview must not create queue");
    assert!(!out_dir.exists(), "preview must not create artifacts");

    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    write_probe_compact_artifacts(&out_dir, run_id);
    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let run_next_sha = sha256_path(&run_next_path);

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
        &format!("GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    );
    let ready_status = ready_child.wait().unwrap();
    assert!(ready_status.success());
    assert!(
        ready_response.starts_with("HTTP/1.1 200 OK"),
        "{ready_response}"
    );
    let ready: serde_json::Value = serde_json::from_str(http_body(&ready_response)).unwrap();
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["next_action"], "publish_operator_record");
    assert_eq!(ready["read_only"], true);
    assert_eq!(ready["would_execute_queue"], false);
    assert_eq!(ready["would_submit_queue_entry"], false);
    assert_eq!(ready["would_rebuild_wrappers"], false);
    assert_eq!(ready["status_probe"]["completion_record_state"], "complete");
    assert_eq!(
        ready["hermes_contract"]["front_end_must_not_scrape_raw_queue_json"],
        true
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));
    assert_eq!(run_next_sha, sha256_path(&run_next_path));
}

#[test]
fn cli_workbench_project_start_factory_operator_record_api_requires_operator_and_delegates_publish()
{
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out_dir = temp.path().join("workbench-publish-operator-record-out");
    let run_id = "workbench-project-start-publish-operator-record";
    let record_out = temp.path().join("workbench-operator-record.json");
    let contract = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/contracts/hermes-project-start-poll-act-contract.v1.json");
    let body = format!(
        "run_id={run_id}&out_dir={}&contract={}&record_out={}",
        out_dir.display(),
        contract.display(),
        record_out.display()
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
            "POST /api/factory/project-start/operator-record?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    assert!(!record_out.exists(), "viewer token must not publish record");

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
            "POST /api/factory/project-start/operator-record?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    assert!(missing["error"]
        .as_str()
        .unwrap_or("")
        .contains("next action is call_queue_project_start_complete"));
    assert!(
        !record_out.exists(),
        "missing queue must not publish record"
    );

    write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    write_probe_compact_artifacts(&out_dir, run_id);
    let consumer_path =
        out_dir.join("factory-queue-project-start-completion-contract-consumer.json");
    let mut consumer: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&consumer_path).unwrap()).unwrap();
    consumer["trust_boundary"]["control_plane_approves_release"] = serde_json::Value::Bool(true);
    fs::write(
        &consumer_path,
        serde_json::to_string_pretty(&consumer).unwrap(),
    )
    .unwrap();
    let mut corrupt_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
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
    let corrupt_port = read_server_port(&mut corrupt_child);
    let corrupt_response = http_request(
        corrupt_port,
        &format!(
            "POST /api/factory/project-start/operator-record?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let corrupt_status = corrupt_child.wait().unwrap();
    assert!(corrupt_status.success());
    assert!(
        corrupt_response.starts_with("HTTP/1.1 400 Bad Request"),
        "{corrupt_response}"
    );
    let corrupt: serde_json::Value = serde_json::from_str(http_body(&corrupt_response)).unwrap();
    assert!(corrupt["error"]
        .as_str()
        .unwrap_or("")
        .contains("next action is operator_review_required"));
    assert!(
        !record_out.exists(),
        "corrupt artifacts must not publish record"
    );

    write_probe_compact_artifacts(&out_dir, run_id);
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
            "POST /api/factory/project-start/operator-record?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
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
        "ao2.factory-project-start-operator-record.v1"
    );
    assert_eq!(ready["status"], "published");
    assert_eq!(ready["run_id"], run_id);
    assert_eq!(ready["record_path"], record_out.display().to_string());
    assert_eq!(
        ready["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert!(record_out.exists(), "operator token should publish record");
}

#[test]
fn cli_workbench_project_start_factory_operator_record_smoke_covers_next_action_to_publish() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let spaced_root = temp.path().join("AI Agent Teams").join("ao-2");
    let out_dir = spaced_root.join("workbench-project-start-smoke-out");
    let run_id = "workbench-project-start-smoke";
    let record_out = spaced_root.join("workbench-project-start-operator-record.json");
    let source_contract = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/contracts/hermes-project-start-poll-act-contract.v1.json");
    let contract = spaced_root
        .join("docs")
        .join("hermes-project-start-poll-act-contract.v1.json");
    fs::create_dir_all(contract.parent().unwrap()).unwrap();
    fs::copy(&source_contract, &contract).unwrap();
    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    write_probe_compact_artifacts(&out_dir, run_id);
    let queue_sha = sha256_path(&queue_path);
    let source_paths = [
        (
            "queue_submit",
            out_dir.join("factory-queue-project-start-submit.json"),
        ),
        (
            "queue_run_next",
            out_dir.join("factory-queue-project-start-run-next.json"),
        ),
        (
            "completion_contract",
            out_dir.join("factory-queue-project-start-completion-contract.json"),
        ),
        (
            "completion_contract_consumer",
            out_dir.join("factory-queue-project-start-completion-contract-consumer.json"),
        ),
    ];
    let source_shas: BTreeMap<&str, String> = source_paths
        .iter()
        .map(|(label, path)| (*label, sha256_path(path)))
        .collect();
    let next_action_route = format!(
        "/api/factory/project-start/next-action?token=viewer-token&run_id={run_id}&out_dir={}&contract={}",
        out_dir.display(),
        contract.display()
    );
    let publish_body = format!(
        "run_id={run_id}&out_dir={}&contract={}&record_out={}",
        out_dir.display(),
        contract.display(),
        record_out.display()
    );

    let mut preview_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
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
    let preview_port = read_server_port(&mut preview_child);
    let preview_response = http_request(
        preview_port,
        &format!(
            "GET {next_action_route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    let preview_status = preview_child.wait().unwrap();
    assert!(preview_status.success());
    assert!(
        preview_response.starts_with("HTTP/1.1 200 OK"),
        "{preview_response}"
    );
    let preview: serde_json::Value = serde_json::from_str(http_body(&preview_response)).unwrap();
    assert_eq!(preview["status"], "ready");
    assert_eq!(preview["next_action"], "publish_operator_record");
    assert_eq!(preview["read_only"], true);
    assert_eq!(preview["would_execute_queue"], false);
    assert_eq!(preview["would_submit_queue_entry"], false);
    assert_eq!(preview["would_rebuild_wrappers"], false);
    assert_eq!(
        preview["hermes_contract"]["front_end_must_not_scrape_raw_queue_json"],
        true
    );
    assert!(!record_out.exists(), "preview must not publish record");
    assert_eq!(queue_sha, sha256_path(&queue_path));
    for (label, path) in &source_paths {
        assert_eq!(
            source_shas[label],
            sha256_path(path),
            "preview must not rewrite {label}"
        );
    }

    let mut publish_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
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
    let publish_port = read_server_port(&mut publish_child);
    let publish_response = http_request(
        publish_port,
        &format!(
            "POST /api/factory/project-start/operator-record?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            publish_body.len(),
            publish_body
        ),
    );
    let publish_status = publish_child.wait().unwrap();
    assert!(publish_status.success());
    assert!(
        publish_response.starts_with("HTTP/1.1 200 OK"),
        "{publish_response}"
    );
    let published: serde_json::Value = serde_json::from_str(http_body(&publish_response)).unwrap();
    assert_eq!(
        published["schema_version"],
        "ao2.factory-project-start-operator-record.v1"
    );
    assert_eq!(published["status"], "published");
    assert_eq!(published["run_id"], run_id);
    assert_eq!(
        published["read_only_preflight"]["next_action"],
        "publish_operator_record"
    );
    assert_eq!(published["would_execute_queue"], false);
    assert_eq!(published["would_submit_queue_entry"], false);
    assert_eq!(published["would_rebuild_wrappers"], false);
    assert_eq!(published["would_mutate_control_plane"], false);
    assert_sha256_string(&published["record_sha256"], "record_sha256");
    assert_eq!(
        published["record"]["hermes_contract"]["front_end_reads_single_operator_record"],
        true
    );
    assert_eq!(
        published["record"]["hermes_contract"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(published["record"]["queue_sha256"], queue_sha);
    for (label, expected_sha) in &source_shas {
        let artifact = &published["record"]["source_artifacts"][*label];
        assert_eq!(
            artifact["path"],
            source_paths
                .iter()
                .find(|(candidate, _)| candidate == label)
                .unwrap()
                .1
                .display()
                .to_string()
        );
        assert_eq!(artifact["sha256"], expected_sha.as_str());
        assert_sha256_string(&artifact["sha256"], label);
    }
    assert_eq!(
        published["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));
    for (label, path) in &source_paths {
        assert_eq!(
            source_shas[label],
            sha256_path(path),
            "publish must not rewrite source artifact {label}"
        );
    }
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&record_out).unwrap()).unwrap();
    assert_eq!(record, published["record"]);
}

#[test]
fn cli_workbench_project_start_factory_hermes_flow_contract_api_delegates_to_contract_producer() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out = temp.path().join("workbench-hermes-flow-contract.json");
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let route = format!(
        "/api/factory/project-start/hermes-flow-contract?token=viewer-token&out={}",
        out.display()
    );

    let mut denied_child = Command::new(env!("CARGO_BIN_EXE_ao2"))
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
    let denied_port = read_server_port(&mut denied_child);
    let denied_response = http_request(
        denied_port,
        &format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            route.replace("token=viewer-token", "token=bad-token")
        ),
    );
    let denied_status = denied_child.wait().unwrap();
    assert!(denied_status.success());
    assert!(
        denied_response.starts_with("HTTP/1.1 403 Forbidden"),
        "{denied_response}"
    );
    assert!(!out.exists(), "denied request must not write contract");
    assert!(
        !queue_path.exists(),
        "denied request must not create queue state"
    );

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
        &format!("GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
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
        "ao2.hermes-project-start-flow-contract.v1"
    );
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["contract_path"], out.display().to_string());
    assert_sha256_string(&ready["contract_sha256"], "contract_sha256");
    assert_eq!(ready["workflow"]["preview"]["minimum_role"], "viewer");
    assert_eq!(ready["workflow"]["publish"]["minimum_role"], "operator");
    assert_eq!(
        ready["workflow"]["publish"]["only_when_next_action"],
        "publish_operator_record"
    );
    assert_eq!(
        ready["hermes_contract"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(ready["side_effects"]["would_execute_queue"], false);
    assert_eq!(ready["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(ready["side_effects"]["would_rebuild_wrappers"], false);
    assert_eq!(ready["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(
        ready["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        ready["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(ready["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(!queue_path.exists(), "route must not create queue state");
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(written, ready["contract"]);
}

fn verified_release_gate_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema": "ao2.release-gate.v1",
        "status": "verified",
        "release": {
            "provenance_verified": true,
            "archive_count": 4
        },
        "smoke": {
            "status": "verified"
        },
        "obligation_gates": {
            "status": "verified"
        },
        "obligation_gate_signing": {
            "status": "verified"
        },
        "replacement_smoke_gate": {
            "schema": "ao2.release-replacement-smoke-gate-verification.v1",
            "status": "verified",
            "gate_status": "accepted",
            "accepted_os": ["macos", "ubuntu", "windows"],
            "reasons": []
        },
        "reasons": []
    })
}

fn verified_release_gate_with_governed_run_fixture() -> serde_json::Value {
    let mut release_gate = verified_release_gate_fixture();
    release_gate["governed_run_evidence"] = serde_json::json!({
        "schema": "ao2.release-governed-run-evidence-verification.v1",
        "status": "verified",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "reasons": []
    });
    release_gate["factory_project_run_readback"] = serde_json::json!({
        "schema": "ao2.release-factory-project-run-readback-verification.v1",
        "status": "verified",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "reasons": []
    });
    release_gate
}

fn accepted_governed_run_fixture(run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-governed-run.v1",
        "status": "accepted",
        "run_id": run_id,
        "plan": {
            "ao2_native_plan": {
                "role_contract_discovery": {
                    "mode": "auto_discovered_from_ao_runspec_layout",
                    "loaded_count": 7
                }
            }
        },
        "run_result_verification": {
            "status": "accepted"
        },
        "pack_evidence": {
            "status": "produced",
            "signature": {
                "signature_verified": true
            }
        },
        "evaluator_decision": {
            "verdict": "accepted",
            "signature": {
                "signature_verified": true
            }
        },
        "evaluator_decision_verification": {
            "status": "accepted",
            "signature_verified": true
        },
        "governed_run_checklist": {
            "ao2_planned_factory_compat_workflow": true,
            "ao2_queue_executed_factory_compat_workflow": true,
            "ao2_verified_primary_run_result": true,
            "ao2_packed_primary_evidence": true,
            "ao2_signed_evaluator_closure": true,
            "ao2_auto_loaded_role_contracts": true,
            "factory_v3_drives_workflow": false
        },
        "artifacts": {
            "governed_run": format!("target/{run_id}/governed-run.json"),
            "run_result_verification": format!("target/{run_id}/run-result-verification.json"),
            "evidence_pack": format!("target/{run_id}/evidence-pack.json"),
            "evaluator_decision": format!("target/{run_id}/evaluator-decision.json")
        },
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-governed-run",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    })
}

fn write_phase1_governed_run_evidence(root: &Path) -> Vec<PathBuf> {
    ["macos", "ubuntu", "windows"]
        .into_iter()
        .map(|os_label| {
            let dir = root.join("governed-run-evidence").join(os_label);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("governed-run.json");
            fs::write(
                &path,
                serde_json::to_string_pretty(&accepted_governed_run_fixture(&format!(
                    "real-factory-runspec-{os_label}"
                )))
                .unwrap(),
            )
            .unwrap();
            path
        })
        .collect()
}

fn accepted_factory_project_run_readback_fixture(os_label: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-project-run-smoke.v1",
        "status": "passed",
        "host_os": os_label,
        "run_id": format!("factory-project-run-{os_label}"),
        "factory_project_schema": "ao2.factory-project-run.v1",
        "queued_auto_replacement_packet": format!("target/{os_label}/queued/factory-replacement-packet.json"),
        "queued_auto_replacement_packet_archive": format!("target/{os_label}/queued/factory-replacement-packet.tgz"),
        "queued_auto_replacement_packet_status": "packaged",
        "queued_auto_replacement_packet_verification": format!("target/{os_label}/queued/factory-replacement-packet-verification.json"),
        "queued_auto_replacement_packet_verification_status": "accepted",
        "queued_auto_replacement_packet_verification_checksums_verified": true,
        "queued_auto_replacement_packet_verification_trust_boundary_verified": true,
        "queued_replacement_packet": format!("target/{os_label}/factory-replacement-packet.json"),
        "queued_replacement_packet_archive": format!("target/{os_label}/factory-replacement-packet.tgz"),
        "queued_replacement_packet_schema": "ao2.factory-replacement-packet.v1",
        "queued_replacement_packet_status": "packaged",
        "queued_replacement_packet_sha256": "a".repeat(64),
        "queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver": true,
        "queued_replacement_packet_factory_v3_role": "evaluator_closer_and_sampling_auditor",
        "queued_replacement_packet_verification": format!("target/{os_label}/factory-replacement-packet-verification.json"),
        "queued_replacement_packet_verification_schema": "ao2.factory-replacement-packet-verification.v1",
        "queued_replacement_packet_verification_status": "accepted",
        "queued_replacement_packet_verification_checksums_verified": true,
        "queued_replacement_packet_verification_trust_boundary_verified": true,
        "queued_replacement_packet_verification_ao2_replacement_driver_verified": true,
        "queued_replacement_packet_verification_factory_v3_evaluator_closer_verified": true
    })
}

fn write_factory_project_run_readbacks(root: &Path) -> Vec<PathBuf> {
    ["macos", "ubuntu", "windows"]
        .into_iter()
        .map(|os_label| {
            let dir = root.join("factory-project-run-readback").join(os_label);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("factory-project-run-summary.json");
            fs::write(
                &path,
                serde_json::to_string_pretty(&accepted_factory_project_run_readback_fixture(
                    os_label,
                ))
                .unwrap(),
            )
            .unwrap();
            path
        })
        .collect()
}

fn accepted_provider_acceptance_preservation_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema": "ao2.provider-pilot-acceptance-preservation.v1",
        "status": "passed",
        "tag": "v0.4.80",
        "providers": {
            "codex": {
                "schema_version": "ao2.codex-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-codex-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/codex/provider-pilot-acceptance.json"
            },
            "claude": {
                "schema_version": "ao2.claude-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-claude-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/claude/provider-pilot-acceptance.json"
            },
            "antigravity": {
                "schema_version": "ao2.antigravity-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-antigravity-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/antigravity/provider-pilot-acceptance.json"
            }
        }
    })
}

fn accepted_replacement_smoke_gate_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1",
        "status": "accepted",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-three-os-replacement-smoke-gate",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "three_os_contract": {
            "path_separator_safe_artifacts": true,
            "requires_native_windows_smoke": true,
            "requires_ubuntu_smoke": true,
            "requires_macos_smoke": true,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    })
}

#[test]
fn cli_workbench_project_start_run_next_requires_operator_exact_digest() {
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
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("workbench-run-next-project-start");
    let run_id = "workbench-project-start-run-next";
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
        run_id,
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    let queue_path = repo.join(".ao2/factory-compat/queue.json");
    assert!(queue_path.exists(), "queue submit should persist queue");
    let body = format!("run_id={run_id}&signer_id=ao2-workbench");

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
            "POST /api/factory/project-start/run-next?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    let queued_before: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&queue_path).unwrap()).unwrap();
    assert_eq!(queued_before["entries"][0]["status"], "queued");

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
            "POST /api/factory/project-start/run-next?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
        "ao2.factory-project-start-workbench-run-next-approval.v1"
    );
    assert_eq!(missing["status"], "approval_required");
    assert_eq!(missing["approval_mode"], "exact_action_digest");
    assert_eq!(missing["required_form_field"], "approval_action_digest");
    assert_eq!(missing["action_digest"].as_str().unwrap().len(), 64);
    assert_eq!(missing["queued_entry"]["status"], "queued");
    assert_eq!(
        missing["side_effects"]["would_execute_queue_after_approval"],
        true
    );
    assert_eq!(missing["side_effects"]["would_mutate_control_plane"], false);
    let queued_after_missing: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&queue_path).unwrap()).unwrap();
    assert_eq!(queued_after_missing["entries"][0]["status"], "queued");
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
            "POST /api/factory/project-start/run-next?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
        "ao2.factory-project-start-workbench-run-next.v1"
    );
    assert_eq!(ready["status"], "accepted");
    assert_eq!(ready["run_id"], run_id);
    assert_eq!(ready["approval"]["status"], "approved_exact_action_digest");
    assert_eq!(ready["approval"]["action_digest"], action_digest);
    assert_eq!(
        ready["queue_run_next"]["schema_version"],
        "ao2.factory-project-start-workbench-queue-run-next.v1"
    );
    assert_eq!(ready["queue_run_next"]["entry"]["status"], "accepted");
    assert_eq!(ready["side_effects"]["executed_queue"], true);
    assert_eq!(ready["side_effects"]["executed_provider"], false);
    assert_eq!(ready["side_effects"]["mutated_control_plane"], false);
    assert_eq!(
        ready["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        ready["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(ready["trust_boundary"]["mutates_ao_artifacts"], false);
    let queue: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(queue_path).unwrap()).unwrap();
    assert_eq!(queue["entries"][0]["run_id"], run_id);
    assert_eq!(queue["entries"][0]["status"], "accepted");

    let lower = ready_response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

#[test]
fn cli_workbench_project_start_completion_summary_reads_compact_hermes_packet() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("summary-project.md");
    fs::write(
        &project_spec,
        r#"# Completion Summary Project

## App Steps

- Build a minimal governed workflow fixture.
- Publish a compact Hermes-readable completion summary.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("completion-summary-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("completion-summary-project-start");
    let run_id = "workbench-project-start-completion-summary";

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
        run_id,
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "completion-summary-test",
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
    let queue_run_next: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    assert_eq!(queue_run_next["status"], "accepted");
    assert_eq!(
        queue_run_next["entry"]["replacement_packet_status"],
        "packaged"
    );
    assert_eq!(
        queue_run_next["entry"]["replacement_packet_verification_status"],
        "accepted"
    );
    assert_eq!(
        queue_run_next["entry"]["replacement_packet_verification_checks"]["checksums_verified"],
        true
    );
    assert_eq!(
        queue_run_next["entry"]["replacement_packet_verification_checks"]
            ["trust_boundary_verified"],
        true
    );
    assert_eq!(
        queue_run_next["entry"]["replacement_packet_verification_checks"]
            ["ao2_replacement_driver_verified"],
        true
    );
    assert_eq!(
        queue_run_next["entry"]["replacement_packet_verification_checks"]
            ["factory_v3_evaluator_closer_verified"],
        true
    );
    let queue_path = Path::new(queue_run_next["queue_path"].as_str().unwrap());
    let queue_sha_before = sha256_path(queue_path);

    let summary = ao2([
        "factory",
        "queue-project-start-completion-summary",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--json",
    ]);
    assert!(summary.status.success(), "{}", stderr(&summary));
    let json: serde_json::Value = serde_json::from_str(&stdout(&summary)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-completion-summary.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["read_only"], true);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["queue"]["status"], "accepted");
    assert_eq!(
        json["artifacts"]["project_start_operator_summary"]["status"],
        "accepted"
    );
    assert_eq!(
        json["artifacts"]["project_start_closure_verification"]["status"],
        "accepted"
    );
    assert_eq!(
        json["artifacts"]["replacement_packet"]["status"],
        "packaged"
    );
    assert_eq!(
        json["artifacts"]["replacement_packet_verification"]["status"],
        "accepted"
    );
    assert_eq!(
        json["replacement_packet_handoff"]["status"],
        "ready_for_operator_review"
    );
    assert_eq!(
        json["replacement_packet_handoff"]["requires_manual_packet_verify_command"],
        false
    );
    assert_eq!(
        json["replacement_packet_handoff"]["checksums_verified"],
        true
    );
    assert_eq!(
        json["replacement_packet_handoff"]["trust_boundary_verified"],
        true
    );
    assert_eq!(
        json["replacement_packet_handoff"]["ao2_replaces_factory_v3_workflow_driver"],
        true
    );
    assert_eq!(
        json["replacement_packet_handoff"]["factory_v3_role"],
        "evaluator_closer_and_sampling_auditor"
    );
    assert_eq!(json["hermes_memory"]["single_record_for_bookkeeping"], true);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "record_replacement_packet_completion_summary"
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
    assert_eq!(queue_sha_before, sha256_path(queue_path));

    let route =
        format!("/api/factory/project-start/completion-summary?token=viewer-token&run_id={run_id}");
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
    let response = http_request(
        port,
        &format!("GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        api["schema_version"],
        "ao2.factory-project-start-completion-summary.v1"
    );
    assert_eq!(api["status"], "accepted");
    assert_eq!(api["run_id"], run_id);
    assert_eq!(api["queue"]["sha256"], json["queue"]["sha256"]);
    assert_eq!(
        api["artifacts"]["project_start_operator_summary"]["sha256"],
        json["artifacts"]["project_start_operator_summary"]["sha256"]
    );
    assert_eq!(
        api["replacement_packet_handoff"]["verification_sha256"],
        json["replacement_packet_handoff"]["verification_sha256"]
    );
    assert_eq!(queue_sha_before, sha256_path(queue_path));

    let lower = response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
        "operator-token",
        "viewer-token",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

#[test]
fn cli_workbench_project_start_completion_summary_memory_checkpoint_records_ao2_memory() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("memory-checkpoint-project.md");
    fs::write(
        &project_spec,
        r#"# Memory Checkpoint Project

## App Steps

- Build a minimal governed workflow fixture.
- Record the compact project-start completion summary into AO2 memory.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("memory-checkpoint-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("memory-checkpoint-project-start");
    let run_id = "workbench-project-start-completion-summary-memory";

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
        run_id,
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "completion-summary-memory-test",
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
    let queue_run_next: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    assert_eq!(queue_run_next["status"], "accepted");
    let queue_path = Path::new(queue_run_next["queue_path"].as_str().unwrap());
    let queue_sha_before = sha256_path(queue_path);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");
    assert!(!memory_records_path.exists());
    assert!(!memory_links_path.exists());

    let approval = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    assert_eq!(
        approval_json["schema_version"],
        "ao2.factory-project-start-completion-summary-memory-checkpoint-approval.v1"
    );
    assert_eq!(approval_json["status"], "approval_required");
    assert_eq!(approval_json["approval_mode"], "exact_action_digest");
    assert_sha256_string(&approval_json["action_digest"], "action_digest");
    assert_eq!(approval_json["summary"]["run_id"], run_id);
    assert_eq!(
        approval_json["side_effects"]["would_write_memory_after_approval"],
        true
    );
    assert_eq!(
        approval_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert!(!memory_records_path.exists());
    assert!(!memory_links_path.exists());
    assert_eq!(queue_sha_before, sha256_path(queue_path));

    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
    let json: serde_json::Value = serde_json::from_str(&stdout(&checkpoint)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-completion-summary-memory-checkpoint.v1"
    );
    assert_eq!(json["status"], "recorded");
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["approval"]["status"], "approved_exact_action_digest");
    assert_eq!(json["approval"]["action_digest"], digest);
    assert_sha256_string(&json["summary_sha256"], "summary_sha256");
    assert_eq!(json["queue_sha256"], queue_sha_before);
    assert_eq!(
        json["memory_record"]["schema_version"],
        "ao2.memory-record.v1"
    );
    assert_eq!(
        json["memory_record"]["kind"],
        "project-start-completion-summary"
    );
    assert_eq!(json["memory_record"]["source"]["run_id"], run_id);
    assert_eq!(json["memory_link"]["run_id"], run_id);
    assert_eq!(
        json["memory_link"]["relationship"],
        "project-start-completion-summary"
    );
    assert_eq!(json["side_effects"]["wrote_memory_record"], true);
    assert_eq!(json["side_effects"]["wrote_memory_run_link"], true);
    assert_eq!(json["side_effects"]["executed_provider"], false);
    assert_eq!(json["side_effects"]["executed_queue"], false);
    assert_eq!(json["side_effects"]["mutated_control_plane"], false);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    let records = fs::read_to_string(&memory_records_path).unwrap();
    assert_eq!(records.lines().count(), 1);
    assert!(records.contains("project-start-completion-summary"));
    let links = fs::read_to_string(&memory_links_path).unwrap();
    assert_eq!(links.lines().count(), 1);
    assert!(links.contains(run_id));

    let route =
        "/api/factory/project-start/completion-summary/memory-checkpoint?token=operator-token";
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
            "admin-token",
            "--operator-token",
            "operator:operator:operator-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!("run_id={run_id}");
    let response = http_request(
        port,
        &format!(
            "POST {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    let api_approval: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api_approval["status"], "approval_required");
    let api_digest = api_approval["action_digest"].as_str().unwrap();

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
            "admin-token",
            "--operator-token",
            "operator:operator:operator-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!("run_id={run_id}&approval_action_digest={api_digest}");
    let response = http_request(
        port,
        &format!(
            "POST {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        api["schema_version"],
        "ao2.factory-project-start-completion-summary-memory-checkpoint.v1"
    );
    assert_eq!(api["status"], "recorded");
    assert_eq!(api["summary_sha256"], json["summary_sha256"]);
    assert_eq!(api["queue_sha256"], json["queue_sha256"]);
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    assert_eq!(
        fs::read_to_string(&memory_records_path)
            .unwrap()
            .lines()
            .count(),
        2
    );

    let lower = response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
        "api-token",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

#[test]
fn cli_workbench_project_start_completion_summary_memory_status_reads_checkpoint_without_mutation()
{
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("memory-status-project.md");
    fs::write(
        &project_spec,
        r#"# Memory Status Project

## App Steps

- Build a governed workflow fixture.
- Read back the completion-summary memory checkpoint without mutating state.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("memory-status-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("memory-status-project-start");
    let run_id = "workbench-project-start-completion-summary-memory-status";

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
        run_id,
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "completion-summary-memory-status-test",
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
    let queue_run_next: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    assert_eq!(queue_run_next["status"], "accepted");
    let queue_path = Path::new(queue_run_next["queue_path"].as_str().unwrap());
    let queue_sha_before = sha256_path(queue_path);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let approval = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
    let checkpoint_json: serde_json::Value = serde_json::from_str(&stdout(&checkpoint)).unwrap();
    assert_eq!(checkpoint_json["status"], "recorded");
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let status = ao2([
        "factory",
        "queue-project-start-completion-summary-memory-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-completion-summary-memory-status.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["summary_sha256"], checkpoint_json["summary_sha256"]);
    assert_eq!(json["queue_sha256"], queue_sha_before);
    assert_eq!(
        json["memory_record"]["id"],
        checkpoint_json["memory_record"]["id"]
    );
    assert_eq!(
        json["memory_record"]["kind"],
        "project-start-completion-summary"
    );
    assert_eq!(json["memory_record"]["source"]["run_id"], run_id);
    assert_eq!(
        json["memory_link"]["memory_id"],
        checkpoint_json["memory_record"]["id"]
    );
    assert_eq!(
        json["memory_link"]["relationship"],
        "project-start-completion-summary"
    );
    assert_eq!(json["hermes_memory"]["checkpoint_is_durable"], true);
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "read_memory_checkpoint"
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
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
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let route = format!(
        "/api/factory/project-start/completion-summary/memory-checkpoint?token=viewer-token&run_id={run_id}"
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
    let response = http_request(
        port,
        &format!("GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        api["schema_version"],
        "ao2.factory-project-start-completion-summary-memory-status.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(api["memory_record"]["id"], json["memory_record"]["id"]);
    assert_eq!(api["summary_sha256"], json["summary_sha256"]);
    assert_eq!(api["queue_sha256"], json["queue_sha256"]);
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let lower = response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
        "operator-token",
        "viewer-token",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

#[test]
fn cli_workbench_project_start_recovery_combines_summary_and_memory_status_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("recovery-project.md");
    fs::write(
        &project_spec,
        r#"# Recovery Project

## App Steps

- Build a governed workflow fixture.
- Recover the full Hermes project-start state from one AO2 read-only packet.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("recovery-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
    let out_dir = temp.path().join("recovery-project-start");
    let run_id = "workbench-project-start-recovery";

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
        run_id,
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-start-recovery-test",
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
    let queue_run_next: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    assert_eq!(queue_run_next["status"], "accepted");
    let queue_path = Path::new(queue_run_next["queue_path"].as_str().unwrap());
    let queue_sha_before = sha256_path(queue_path);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let approval = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
    let checkpoint_json: serde_json::Value = serde_json::from_str(&stdout(&checkpoint)).unwrap();
    assert_eq!(checkpoint_json["status"], "recorded");
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let recovery = ao2([
        "factory",
        "queue-project-start-recovery",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--json",
    ]);
    assert!(recovery.status.success(), "{}", stderr(&recovery));
    let json: serde_json::Value = serde_json::from_str(&stdout(&recovery)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["queue"]["sha256"], queue_sha_before);
    assert_eq!(json["completion_summary"]["run_id"], run_id);
    assert_eq!(json["completion_summary"]["status"], "accepted");
    assert_eq!(
        json["completion_summary"]["sha256"],
        checkpoint_json["summary_sha256"]
    );
    assert_eq!(
        json["memory_checkpoint_status"]["schema_version"],
        "ao2.factory-project-start-completion-summary-memory-status.v1"
    );
    assert_eq!(json["memory_checkpoint_status"]["status"], "ready");
    assert_eq!(
        json["memory_checkpoint_status"]["memory_record"]["id"],
        checkpoint_json["memory_record"]["id"]
    );
    assert_eq!(
        json["memory_checkpoint_status"]["memory_link"]["memory_id"],
        checkpoint_json["memory_record"]["id"]
    );
    assert_eq!(json["surface_status"]["queue_entry"]["present"], true);
    assert_eq!(
        json["surface_status"]["completion_summary"]["present"],
        true
    );
    assert_eq!(json["surface_status"]["memory_checkpoint"]["present"], true);
    assert_eq!(json["surface_status"]["recovery_packet"]["present"], true);
    assert_eq!(
        json["hermes_memory"]["single_recovery_packet_for_bookkeeping"],
        true
    );
    assert_eq!(
        json["hermes_memory"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "resume_from_recovery_packet"
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
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
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let route = format!("/api/factory/project-start/recovery?token=viewer-token&run_id={run_id}");
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
    let response = http_request(
        port,
        &format!("GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        api["schema_version"],
        "ao2.factory-project-start-recovery.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(
        api["completion_summary"]["sha256"],
        json["completion_summary"]["sha256"]
    );
    assert_eq!(
        api["memory_checkpoint_status"]["memory_record"]["id"],
        json["memory_checkpoint_status"]["memory_record"]["id"]
    );
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let lower = response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
        "operator-token",
        "viewer-token",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

#[test]
fn cli_workbench_project_start_latest_recovery_selects_newest_complete_recovery_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("latest-recovery-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let mut latest_checkpoint: Option<serde_json::Value> = None;
    let mut queue_path_buf: Option<PathBuf> = None;
    for run_id in [
        "workbench-project-start-recovery-old",
        "workbench-project-start-recovery-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Latest Recovery Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let Hermes recover from the newest complete AO2 project-start packet.
"#
            ),
        )
        .unwrap();
        let project_root = temp.path().join(format!("{run_id}-project"));
        let out_dir = temp.path().join(format!("{run_id}-out"));

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
            run_id,
            "--provider",
            "scripted",
            "--provider-prompt-dir",
            project_root.join("provider-prompts").to_str().unwrap(),
            "--verifier-command",
            "true",
            "--signing-key",
            signing_key.to_str().unwrap(),
            "--signer-id",
            "project-start-latest-recovery-test",
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
        let queue_run_next: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
        assert_eq!(queue_run_next["status"], "accepted");
        queue_path_buf = Some(PathBuf::from(
            queue_run_next["queue_path"].as_str().unwrap(),
        ));

        let approval = ao2([
            "factory",
            "queue-project-start-completion-summary-memory",
            "--target",
            repo.to_str().unwrap(),
            "--run-id",
            run_id,
            "--json",
        ]);
        assert!(approval.status.success(), "{}", stderr(&approval));
        let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
        let digest = approval_json["action_digest"].as_str().unwrap();
        let checkpoint = ao2([
            "factory",
            "queue-project-start-completion-summary-memory",
            "--target",
            repo.to_str().unwrap(),
            "--run-id",
            run_id,
            "--approve-action-digest",
            digest,
            "--json",
        ]);
        assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
        let checkpoint_json: serde_json::Value =
            serde_json::from_str(&stdout(&checkpoint)).unwrap();
        assert_eq!(checkpoint_json["status"], "recorded");
        latest_checkpoint = Some(checkpoint_json);
    }

    let queue_path = queue_path_buf.unwrap();
    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);
    let latest_checkpoint = latest_checkpoint.unwrap();

    let latest = ao2([
        "factory",
        "queue-project-start-latest-recovery",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(latest.status.success(), "{}", stderr(&latest));
    let json: serde_json::Value = serde_json::from_str(&stdout(&latest)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-latest-recovery.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(
        json["selected"]["run_id"],
        "workbench-project-start-recovery-latest"
    );
    assert_eq!(
        json["selected"]["selection_reason"],
        "latest_terminal_project_start_with_complete_recovery"
    );
    assert_sha256_string(&json["selected"]["queue_sha256"], "selected.queue_sha256");
    assert_sha256_string(
        &json["selected"]["recovery_packet_sha256"],
        "selected.recovery_packet_sha256",
    );
    assert_eq!(json["selected"]["queue_sha256"], queue_sha_before);
    assert_eq!(
        json["recovery_packet"]["schema_version"],
        "ao2.factory-project-start-recovery.v1"
    );
    assert_eq!(
        json["recovery_packet"]["run_id"],
        json["selected"]["run_id"]
    );
    assert_eq!(json["recovery_packet"]["status"], "ready");
    assert_eq!(
        json["recovery_packet"]["queue"]["sha256"],
        json["selected"]["queue_sha256"]
    );
    assert_eq!(
        json["recovery_packet"]["memory_checkpoint_status"]["memory_record"]["id"],
        latest_checkpoint["memory_record"]["id"]
    );
    assert_eq!(
        json["surface_status"]["latest_recovery_selector"]["present"],
        true
    );
    assert_eq!(json["surface_status"]["recovery_packet"]["present"], true);
    assert_eq!(
        json["hermes_memory"]["single_latest_recovery_packet_for_bookkeeping"],
        true
    );
    assert_eq!(json["hermes_memory"]["run_id_memory_required"], false);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "resume_from_latest_recovery_packet"
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let route = "/api/factory/project-start/recovery/latest?token=viewer-token";
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
    let response = http_request(
        port,
        &format!("GET {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        api["schema_version"],
        "ao2.factory-project-start-latest-recovery.v1"
    );
    assert_eq!(api["selected"]["run_id"], json["selected"]["run_id"]);
    assert_eq!(
        api["selected"]["recovery_packet_sha256"],
        json["selected"]["recovery_packet_sha256"]
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let lower = response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
        "operator-token",
        "viewer-token",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

struct PostContinuationExecutionStatusFixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    queue_path: PathBuf,
    memory_records_path: PathBuf,
    memory_links_path: PathBuf,
    run_id: String,
    queue_sha: String,
    recovery_packet_sha: String,
    plan_sha: String,
    claim_status_sha: String,
    continuation_status_sha: String,
}

fn create_post_continuation_execution_status_fixture(
    label: &str,
) -> PostContinuationExecutionStatusFixture {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join(format!("{label}-post-continuation-status-signing-key.pem"));
    generate_native_signing_key(&signing_key, 2048);
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let run_id = format!("workbench-project-start-recovery-{label}");
    let project_spec = temp.path().join(format!("{run_id}.md"));
    fs::write(
        &project_spec,
        format!(
            r#"# Recovery Post-Continuation Execution Status Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 replay the exact-digest post-continuation recovery execution status.
"#
        ),
    )
    .unwrap();
    let project_root = temp.path().join(format!("{run_id}-project"));
    let out_dir = temp.path().join(format!("{run_id}-out"));

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
        &run_id,
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "project-start-recovery-post-continuation-status-test",
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
    let approval = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        &run_id,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint_memory = ao2([
        "factory",
        "queue-project-start-completion-summary-memory",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        &run_id,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(
        checkpoint_memory.status.success(),
        "{}",
        stderr(&checkpoint_memory)
    );

    let action = ao2([
        "factory",
        "queue-project-start-recovery-action",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(action.status.success(), "{}", stderr(&action));
    let action_json: serde_json::Value = serde_json::from_str(&stdout(&action)).unwrap();
    let queue_sha = action_json["exact_digest_requirements"]["resume_from_latest_recovery_packet"]
        ["queue_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap()
        .to_string();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--json",
    ]);
    assert!(
        checkpoint_approval.status.success(),
        "{}",
        stderr(&checkpoint_approval)
    );
    let checkpoint_approval_json: serde_json::Value =
        serde_json::from_str(&stdout(&checkpoint_approval)).unwrap();
    let checkpoint_digest = checkpoint_approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--approve-action-digest",
        checkpoint_digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));

    let plan = ao2([
        "factory",
        "queue-project-start-recovery-resume-plan",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap().to_string();

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--approve-plan-sha256",
        &plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));

    let claim_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--plan-sha256",
        &plan_sha,
        "--json",
    ]);
    assert!(claim_status.status.success(), "{}", stderr(&claim_status));
    let claim_status_json: serde_json::Value =
        serde_json::from_str(&stdout(&claim_status)).unwrap();
    let claim_status_sha = canonical_sha256_for_test(&claim_status_json);

    let continued = ao2([
        "factory",
        "queue-project-start-recovery-resume-continue",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--plan-sha256",
        &plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--approve-claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(continued.status.success(), "{}", stderr(&continued));

    let continuation_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-continuation-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--plan-sha256",
        &plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(
        continuation_status.status.success(),
        "{}",
        stderr(&continuation_status)
    );
    let continuation_status_json: serde_json::Value =
        serde_json::from_str(&stdout(&continuation_status)).unwrap();
    let continuation_status_sha = canonical_sha256_for_test(&continuation_status_json);

    let executed = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execute",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        &queue_sha,
        "--recovery-packet-sha256",
        &recovery_packet_sha,
        "--plan-sha256",
        &plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--continuation-status-sha256",
        &continuation_status_sha,
        "--approve-continuation-status-sha256",
        &continuation_status_sha,
        "--json",
    ]);
    assert!(executed.status.success(), "{}", stderr(&executed));

    PostContinuationExecutionStatusFixture {
        _temp: temp,
        repo,
        queue_path,
        memory_records_path,
        memory_links_path,
        run_id,
        queue_sha,
        recovery_packet_sha,
        plan_sha,
        claim_status_sha,
        continuation_status_sha,
    }
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
