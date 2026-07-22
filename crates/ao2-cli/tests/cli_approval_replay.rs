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
fn test_http_accept_waits_for_slow_windows_child_startup() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let client = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5500));
        TcpStream::connect(("127.0.0.1", port)).unwrap();
    });

    let stream = accept_test_connection(&listener, "delayed local test HTTP request");
    drop(stream);
    client.join().unwrap();
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
fn cli_factory_queue_executes_project_start_handoff_job() {
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
    let out_dir = temp.path().join("queued-project-start");
    let receipt_out = temp.path().join("queue-project-start-submit.json");

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
        "queued-project-start",
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
        "--out",
        receipt_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    let submitted: serde_json::Value = serde_json::from_str(&stdout(&submit)).unwrap();
    assert_eq!(
        submitted["schema_version"],
        "ao2.factory-project-start-workbench-queue-submit.v1"
    );
    assert_eq!(submitted["status"], "queued");
    assert_eq!(submitted["job_kind"], "factory_project_start");
    assert_eq!(
        submitted["entry"]["execution_contract"]["execution_owner"],
        "ao2"
    );
    assert_eq!(
        submitted["entry"]["execution_contract"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        submitted["entry"]["execution_contract"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        submitted["entry"]["parity_checklist_progress"]
            ["ao2_queue_executes_project_start_handoff_job"],
        true
    );
    assert!(receipt_out.is_file());
    assert!(Path::new(submitted["queue_path"].as_str().unwrap()).is_file());

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));
    let result: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    assert_eq!(
        result["schema_version"],
        "ao2.factory-project-start-workbench-queue-run-next.v1"
    );
    assert_eq!(result["run_id"], "queued-project-start");
    assert_eq!(result["job_kind"], "factory_project_start");
    assert_eq!(result["status"], "accepted");
    assert_eq!(result["entry"]["status"], "accepted");
    assert_eq!(result["entry"]["project_start_status"], "accepted");
    assert_eq!(
        result["entry"]["project_acceptance_review_status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_acceptance_review_recommended_decision"],
        "accept"
    );
    assert!(Path::new(
        result["entry"]["project_acceptance_review"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert_eq!(
        result["entry"]["project_start_result"]["project_acceptance_review"]["schema_version"],
        "ao2.factory-project-acceptance-review.v1"
    );
    assert_eq!(
        result["entry"]["project_start_result"]["project_acceptance_review"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_result"]["project_acceptance_review"]["signature"]
            ["signature_status"],
        "signed"
    );
    assert_eq!(
        result["hermes_queue_handoff_schema"],
        "ao2.hermes-project-start-handoff.v1"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["schema_version"],
        "ao2.hermes-project-start-handoff.v1"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["project_start_bundle"],
        result["entry"]["project_start_bundle"]
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["project_start_bundle_sha256"],
        result["entry"]["project_start_bundle_sha256"]
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        result["entry"]["hermes_queue_handoff"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert!(Path::new(result["entry"]["project_start"].as_str().unwrap()).is_file());
    assert!(Path::new(result["entry"]["project_start_bundle"].as_str().unwrap()).is_file());
    assert_eq!(
        result["entry"]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert!(Path::new(
        result["entry"]["project_start_bundle_verification"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["schema_version"],
        "ao2.factory-project-start-bundle-verification.v1"
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["checks"]
            ["project_acceptance_review_signature_verified"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["checks"]
            ["review_rubric_digest_matches"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_bundle_verification_result"]["checks"]
            ["review_project_run_digest_matches"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_status"],
        "accepted"
    );
    let operator_summary_path = Path::new(
        result["entry"]["project_start_operator_summary"]
            .as_str()
            .unwrap(),
    );
    assert!(operator_summary_path.is_file());
    assert!(Path::new(
        result["entry"]["project_start_operator_summary_markdown"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["schema_version"],
        "ao2.factory-project-start-operator-summary.v1"
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["checks"]
            ["project_start_accepted"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["checks"]
            ["bundle_verification_accepted"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_result"]["checks"]["bundle_digest_matches"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_checks"],
        result["entry"]["project_start_operator_summary_result"]["checks"]
    );
    assert_eq!(
        result["entry"]["project_start_operator_summary_sha256"],
        sha256_path(operator_summary_path)
    );
    assert_eq!(result["entry"]["project_start_closure_status"], "packaged");
    let project_start_closure_path =
        Path::new(result["entry"]["project_start_closure"].as_str().unwrap());
    assert!(project_start_closure_path.is_file());
    assert_eq!(
        result["entry"]["project_start_closure_sha256"],
        sha256_path(project_start_closure_path)
    );
    assert_eq!(
        result["entry"]["project_start_closure_result"]["schema_version"],
        "ao2.factory-project-start-closure.v1"
    );
    assert_eq!(
        result["entry"]["project_start_closure_result"]["latest_selector_matches_run_id_selector"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_status"],
        "accepted"
    );
    let project_start_closure_verification_path = Path::new(
        result["entry"]["project_start_closure_verification"]
            .as_str()
            .unwrap(),
    );
    assert!(project_start_closure_verification_path.is_file());
    assert_eq!(
        result["entry"]["project_start_closure_verification_sha256"],
        sha256_path(project_start_closure_verification_path)
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_result"]["schema_version"],
        "ao2.factory-project-start-closure-verification.v1"
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_result"]["status"],
        "accepted"
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_checks"]["checksums_verified"],
        true
    );
    assert_eq!(
        result["entry"]["project_start_closure_verification_checks"]["trust_boundary_verified"],
        true
    );
    assert_eq!(
        result["parity_checklist_progress"]["ao2_queue_executes_project_start_handoff_job"],
        true
    );
    assert_eq!(
        result["parity_checklist_progress"]["ao2_queue_verifies_project_start_handoff_bundle"],
        true
    );
    assert_eq!(
        result["parity_checklist_progress"]["factory_v3_drives_workflow"],
        false
    );

    let list = ao2([
        "factory",
        "queue-list",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(list.status.success(), "{}", stderr(&list));
    let listed: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(listed["entries"][0]["job_kind"], "factory_project_start");
    assert_eq!(listed["entries"][0]["status"], "accepted");
    assert_eq!(
        listed["entries"][0]["project_start_bundle"],
        result["entry"]["project_start_bundle"]
    );
    assert_eq!(
        listed["entries"][0]["project_acceptance_review_status"],
        "accepted"
    );
    assert_eq!(
        listed["entries"][0]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert_eq!(
        listed["entries"][0]["project_start_operator_summary_status"],
        "accepted"
    );
    assert_eq!(
        listed["entries"][0]["project_start_closure_status"],
        "packaged"
    );
    assert_eq!(
        listed["entries"][0]["project_start_closure_verification_status"],
        "accepted"
    );

    let queue_path = Path::new(result["queue_path"].as_str().unwrap());
    let queue_sha_before_status = sha256_path(queue_path);
    let queue_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(queue_status.status.success(), "{}", stderr(&queue_status));
    let detail: serde_json::Value = serde_json::from_str(&stdout(&queue_status)).unwrap();
    assert_eq!(detail["schema_version"], "ao2.factory-queue-status.v1");
    assert_eq!(detail["status"], "accepted");
    assert_eq!(detail["run_id"], "queued-project-start");
    assert_eq!(detail["queue_path"], result["queue_path"]);
    assert_eq!(detail["entry"], listed["entries"][0]);
    assert_eq!(
        detail["entry"]["project_start_operator_summary_status"],
        "accepted"
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_sha256"],
        sha256_path(operator_summary_path)
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_checks"]["bundle_digest_matches"],
        true
    );
    assert_eq!(
        detail["entry"]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert_eq!(detail["entry"]["project_start_closure_status"], "packaged");
    assert_eq!(
        detail["entry"]["project_start_closure_verification_status"],
        "accepted"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(detail["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        detail["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        detail["parity_checklist_progress"]["ao2_queue_status_detail_is_read_only"],
        true
    );
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));
    assert!(!stdout(&run_next).contains("Bearer "));
    assert!(!stdout(&queue_status).contains("Bearer "));

    let completion_contract = ao2([
        "factory",
        "queue-completion-contract",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(
        completion_contract.status.success(),
        "{}",
        stderr(&completion_contract)
    );
    let completion: serde_json::Value =
        serde_json::from_str(&stdout(&completion_contract)).unwrap();
    assert_eq!(
        completion["schema_version"],
        "ao2.factory-project-start-queue-completion-contract.v1"
    );
    assert_eq!(completion["status"], "accepted");
    assert_eq!(completion["run_id"], "queued-project-start");
    assert_eq!(
        completion["source_queue_status"]["schema_version"],
        "ao2.factory-queue-status.v1"
    );
    assert_eq!(
        completion["artifacts"]["project_start_bundle"],
        detail["entry"]["project_start_bundle"]
    );
    assert_eq!(
        completion["artifacts"]["project_start_closure"],
        detail["entry"]["project_start_closure"]
    );
    assert_eq!(
        completion["checks"]["project_start_closure_verification_status"],
        "accepted"
    );
    assert_eq!(
        completion["checks"]["project_start_closure_verification_checksums_verified"],
        true
    );
    assert_eq!(
        completion["hermes_contract"]["front_end_reads_single_completion_record"],
        true
    );
    assert_eq!(
        completion["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        completion["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(completion["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));
    assert!(!stdout(&completion_contract).contains("Bearer "));

    let latest_completion_contract = ao2([
        "factory",
        "queue-completion-contract",
        "--target",
        repo.to_str().unwrap(),
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(
        latest_completion_contract.status.success(),
        "{}",
        stderr(&latest_completion_contract)
    );
    let latest_completion: serde_json::Value =
        serde_json::from_str(&stdout(&latest_completion_contract)).unwrap();
    assert_eq!(latest_completion["run_id"], "queued-project-start");
    assert_eq!(
        latest_completion["artifacts"]["project_start_closure_sha256"],
        detail["entry"]["project_start_closure_sha256"]
    );
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    let completion_contract_path = temp.path().join("queue-completion-contract.json");
    fs::write(
        &completion_contract_path,
        format!("{}\n", stdout(&completion_contract)),
    )
    .unwrap();
    let consumed_contract = ao2([
        "factory",
        "queue-completion-contract-consume",
        "--contract",
        completion_contract_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        consumed_contract.status.success(),
        "{}",
        stderr(&consumed_contract)
    );
    let consumed: serde_json::Value = serde_json::from_str(&stdout(&consumed_contract)).unwrap();
    assert_eq!(
        consumed["schema_version"],
        "ao2.factory-project-start-queue-completion-contract-consumption.v1"
    );
    assert_eq!(consumed["status"], "accepted");
    assert_eq!(consumed["ready_for_operator_review"], true);
    assert_eq!(consumed["run_id"], "queued-project-start");
    assert_eq!(
        consumed["source_contract_schema"],
        "ao2.factory-project-start-queue-completion-contract.v1"
    );
    assert_eq!(consumed["hermes_contract"]["consumed_contract_only"], true);
    assert_eq!(
        consumed["hermes_contract"]["front_end_reads_single_completion_record"],
        true
    );
    assert_eq!(
        consumed["hermes_contract"]["requires_manual_closure_commands"],
        false
    );
    assert_eq!(
        consumed["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        consumed["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(consumed["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(!stdout(&consumed_contract).contains("Bearer "));

    let mut rejected_contract = completion.clone();
    rejected_contract["checks"]["project_start_closure_verification_status"] =
        serde_json::Value::String("rejected".to_string());
    fs::write(
        &completion_contract_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&rejected_contract).unwrap()
        ),
    )
    .unwrap();
    let rejected_consume = ao2([
        "factory",
        "queue-completion-contract-consume",
        "--contract",
        completion_contract_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !rejected_consume.status.success(),
        "consumer must fail closed when the completion contract verifier status is rejected"
    );
    assert!(stderr(&rejected_consume)
        .contains("project_start_closure_verification_status must be accepted"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    fs::write(
        project_start_closure_path,
        b"tampered queued project-start closure",
    )
    .unwrap();
    let tampered_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(
        !tampered_status.status.success(),
        "queue-status must fail closed on tampered project-start closure sidecar"
    );
    assert!(stderr(&tampered_status).contains("project_start_closure digest mismatch"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    let tampered_latest = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(
        !tampered_latest.status.success(),
        "latest project-start queue-status must fail closed on tampered closure sidecar"
    );
    assert!(stderr(&tampered_latest).contains("project_start_closure digest mismatch"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));

    let tampered_contract = ao2([
        "factory",
        "queue-completion-contract",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start",
        "--json",
    ]);
    assert!(
        !tampered_contract.status.success(),
        "queue-completion-contract must reuse queue-status digest checks"
    );
    assert!(stderr(&tampered_contract).contains("project_start_closure digest mismatch"));
    assert_eq!(queue_sha_before_status, sha256_path(queue_path));
}

#[test]
fn cli_factory_queue_project_start_complete_returns_hermes_ready_result() {
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
    let out_dir = temp.path().join("one-shot-project-start");

    let completed = ao2([
        "factory",
        "queue-project-start-complete",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-complete-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(completed.status.success(), "{}", stderr(&completed));
    let result: serde_json::Value = serde_json::from_str(&stdout(&completed)).unwrap();
    assert_eq!(
        result["schema_version"],
        "ao2.factory-project-start-queue-complete.v1"
    );
    assert_eq!(result["status"], "accepted");
    assert_eq!(result["ready_for_operator_review"], true);
    assert_eq!(result["run_id"], "queued-project-start-one-shot");
    assert_eq!(result["queue_run_next_status"], "accepted");
    assert_eq!(result["completion_contract_status"], "accepted");
    assert_eq!(result["completion_contract_consumer_status"], "accepted");
    assert_eq!(
        result["completion_contract_consumer"]["schema_version"],
        "ao2.factory-project-start-queue-completion-contract-consumption.v1"
    );
    assert_eq!(
        result["completion_contract_consumer"]["hermes_contract"]["consumed_contract_only"],
        true
    );
    assert_eq!(
        result["completion_contract_consumer"]["hermes_contract"]
            ["requires_manual_closure_commands"],
        false
    );
    assert_eq!(
        result["hermes_contract"]["front_end_reads_single_completion_record"],
        true
    );
    assert_eq!(
        result["hermes_contract"]["backend_used_bounded_ao2_queue"],
        true
    );
    assert_eq!(
        result["hermes_contract"]["requires_manual_command_sequence"],
        false
    );
    assert_eq!(
        result["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        result["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(result["trust_boundary"]["mutates_ao_artifacts"], false);
    for key in [
        "queue_submit",
        "queue_run_next",
        "completion_contract",
        "completion_contract_consumer",
    ] {
        assert!(
            Path::new(result["artifacts"][key].as_str().unwrap()).is_file(),
            "missing {key}"
        );
    }
    let queue_path = Path::new(result["queue_path"].as_str().unwrap());
    let queue_sha_after_complete = sha256_path(queue_path);
    let status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(status_json["status"], "accepted");
    assert_eq!(
        status_json["entry"]["project_start_closure_verification_status"],
        "accepted"
    );
    assert_eq!(queue_sha_after_complete, sha256_path(queue_path));
    assert!(!stdout(&completed).contains("Bearer "));

    let replayed = ao2([
        "factory",
        "queue-project-start-complete",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-complete-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        replayed.status.success(),
        "same run-id replay should reuse accepted queue evidence: {}",
        stderr(&replayed)
    );
    let replayed_json: serde_json::Value = serde_json::from_str(&stdout(&replayed)).unwrap();
    assert_eq!(replayed_json["status"], "accepted");
    assert_eq!(
        replayed_json["resume"]["mode"],
        "reused_existing_queue_entry"
    );
    assert_eq!(queue_sha_after_complete, sha256_path(queue_path));
    let queue_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(queue_path).unwrap()).unwrap();
    let matching_entries = queue_json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["run_id"] == "queued-project-start-one-shot")
        .count();
    assert_eq!(
        matching_entries, 1,
        "replay must not duplicate queue entries"
    );

    let consumer_path = Path::new(
        replayed_json["artifacts"]["completion_contract_consumer"]
            .as_str()
            .unwrap(),
    );
    fs::remove_file(consumer_path).unwrap();
    let resumed = ao2([
        "factory",
        "queue-project-start-complete",
        "--target",
        repo.to_str().unwrap(),
        "--project-spec",
        project_spec.to_str().unwrap(),
        "--project-root",
        project_root.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-project-start-complete-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        resumed.status.success(),
        "partial wrapper resume should rebuild missing consumer: {}",
        stderr(&resumed)
    );
    let resumed_json: serde_json::Value = serde_json::from_str(&stdout(&resumed)).unwrap();
    assert_eq!(resumed_json["status"], "accepted");
    assert_eq!(
        resumed_json["resume"]["mode"],
        "reused_existing_queue_entry"
    );
    assert!(
        consumer_path.is_file(),
        "missing consumer was not regenerated"
    );
    assert_eq!(queue_sha_after_complete, sha256_path(queue_path));

    let consumer_modified_before = fs::metadata(consumer_path).unwrap().modified().unwrap();
    let queue_sha_before_probe = sha256_path(queue_path);
    let probe = ao2([
        "factory",
        "queue-project-start-complete-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queued-project-start-one-shot",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        probe.status.success(),
        "read-only complete-status probe should not execute queue: {}",
        stderr(&probe)
    );
    let probe_json: serde_json::Value = serde_json::from_str(&stdout(&probe)).unwrap();
    assert_eq!(
        probe_json["schema_version"],
        "ao2.factory-project-start-queue-complete-status.v1"
    );
    assert_eq!(probe_json["status"], "accepted");
    assert_eq!(probe_json["completion_record_state"], "complete");
    assert_eq!(probe_json["read_only"], true);
    assert_eq!(probe_json["would_execute_queue"], false);
    assert_eq!(probe_json["would_rebuild_wrappers"], false);
    assert_eq!(probe_json["ready_for_operator_review"], true);
    assert_eq!(queue_sha_before_probe, sha256_path(queue_path));
    assert_eq!(
        consumer_modified_before,
        fs::metadata(consumer_path).unwrap().modified().unwrap()
    );
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
fn cli_factory_queue_project_start_complete_status_reports_fail_closed_matrix_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out_dir = temp.path().join("complete-status-out");
    let run_id = "queued-project-start-status-matrix";
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");

    let missing_queue = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(missing_queue["status"], "missing");
    assert_eq!(
        missing_queue["completion_record_state"],
        "missing_queue_file"
    );
    assert_eq!(missing_queue["read_only"], true);
    assert_eq!(missing_queue["would_execute_queue"], false);
    assert_eq!(missing_queue["would_rebuild_wrappers"], false);
    assert_blocker_code(&missing_queue, "missing_queue_file");
    assert!(!queue_path.exists(), "probe must not create the queue file");
    assert!(
        !out_dir.exists(),
        "probe must not create compact artifact dir"
    );

    let queue_path = write_probe_queue(&repo, "", "queued", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let missing_entry = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(missing_entry["status"], "missing");
    assert_eq!(
        missing_entry["completion_record_state"],
        "missing_queue_entry"
    );
    assert_blocker_code(&missing_entry, "missing_queue_entry");
    assert_eq!(queue_sha, sha256_path(&queue_path));

    for status in ["queued", "running", "rejected"] {
        let queue_path = write_probe_queue(&repo, run_id, status, "factory_project_start");
        let queue_sha = sha256_path(&queue_path);
        let probed = project_start_complete_status_probe(&repo, &out_dir, run_id);
        assert_eq!(probed["status"], status);
        assert_eq!(probed["completion_record_state"], status);
        assert_blocker_code(&probed, &format!("queue_entry_status_{status}"));
        assert_eq!(probed["would_execute_queue"], false);
        assert_eq!(queue_sha, sha256_path(&queue_path));
    }

    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let missing_artifacts = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(missing_artifacts["status"], "incomplete");
    assert_eq!(
        missing_artifacts["completion_record_state"],
        "missing_compact_artifact"
    );
    assert_blocker_code(&missing_artifacts, "missing_compact_artifact_queue_submit");
    assert_blocker_code(
        &missing_artifacts,
        "missing_compact_artifact_completion_contract_consumer",
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    write_probe_compact_artifacts(&out_dir, run_id);
    let consumer_path =
        out_dir.join("factory-queue-project-start-completion-contract-consumer.json");
    let consumer_modified_before = fs::metadata(&consumer_path).unwrap().modified().unwrap();
    let accepted = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["completion_record_state"], "complete");
    assert_eq!(accepted["ready_for_operator_review"], true);
    assert!(accepted["blocker_codes"].as_array().unwrap().is_empty());
    assert_eq!(queue_sha, sha256_path(&queue_path));
    assert_eq!(
        consumer_modified_before,
        fs::metadata(&consumer_path).unwrap().modified().unwrap()
    );

    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let mut run_next: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&run_next_path).unwrap()).unwrap();
    run_next["run_id"] = serde_json::Value::String("wrong-run-id".to_string());
    fs::write(
        &run_next_path,
        serde_json::to_string_pretty(&run_next).unwrap(),
    )
    .unwrap();
    let run_id_mismatch = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(run_id_mismatch["status"], "blocked");
    assert_eq!(
        run_id_mismatch["completion_record_state"],
        "artifact_mismatch"
    );
    assert_blocker_code(&run_id_mismatch, "artifact_run_id_mismatch_queue_run_next");
    assert_eq!(queue_sha, sha256_path(&queue_path));

    write_probe_compact_artifacts(&out_dir, run_id);
    let mut consumer: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&consumer_path).unwrap()).unwrap();
    consumer["trust_boundary"]["control_plane_approves_release"] = serde_json::Value::Bool(true);
    fs::write(
        &consumer_path,
        serde_json::to_string_pretty(&consumer).unwrap(),
    )
    .unwrap();
    let trust_mismatch = project_start_complete_status_probe(&repo, &out_dir, run_id);
    assert_eq!(trust_mismatch["status"], "blocked");
    assert_eq!(
        trust_mismatch["completion_record_state"],
        "artifact_mismatch"
    );
    assert_blocker_code(
        &trust_mismatch,
        "trust_boundary_mismatch_completion_contract_consumer",
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));
}

#[test]
fn cli_factory_queue_project_start_next_action_maps_status_and_contract_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out_dir = temp.path().join("next-action-out");
    let run_id = "queued-project-start-next-action";
    let contract = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/contracts/hermes-project-start-poll-act-contract.v1.json");
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");

    let missing_queue = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(
        missing_queue["schema_version"],
        "ao2.factory-project-start-next-action.v1"
    );
    assert_eq!(
        missing_queue["next_action"],
        "call_queue_project_start_complete"
    );
    assert_eq!(missing_queue["read_only"], true);
    assert_eq!(missing_queue["would_execute_queue"], false);
    assert_eq!(missing_queue["would_submit_queue_entry"], false);
    assert_eq!(missing_queue["would_rebuild_wrappers"], false);
    assert_eq!(
        missing_queue["status_probe"]["completion_record_state"],
        "missing_queue_file"
    );
    assert_blocker_code(&missing_queue["status_probe"], "missing_queue_file");
    assert!(!queue_path.exists(), "next-action must not create queue");
    assert!(!out_dir.exists(), "next-action must not create wrappers");

    let queue_path = write_probe_queue(&repo, run_id, "running", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let running = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(running["next_action"], "wait_and_poll");
    assert_eq!(
        running["status_probe"]["completion_record_state"],
        "running"
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let queue_path = write_probe_queue(&repo, run_id, "rejected", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    let rejected = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(rejected["next_action"], "operator_review_required");
    assert_eq!(
        rejected["status_probe"]["completion_record_state"],
        "rejected"
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
    write_probe_compact_artifacts(&out_dir, run_id);
    let complete = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(complete["next_action"], "publish_operator_record");
    assert_eq!(
        complete["status_probe"]["completion_record_state"],
        "complete"
    );
    assert_eq!(
        complete["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        complete["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(complete["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let mut run_next: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&run_next_path).unwrap()).unwrap();
    run_next["run_id"] = serde_json::Value::String("wrong-run-id".to_string());
    fs::write(
        &run_next_path,
        serde_json::to_string_pretty(&run_next).unwrap(),
    )
    .unwrap();
    let corrupt = project_start_next_action(&repo, &out_dir, run_id, &contract);
    assert_eq!(corrupt["next_action"], "operator_review_required");
    assert_blocker_code(
        &corrupt["status_probe"],
        "artifact_run_id_mismatch_queue_run_next",
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    let bad_contract = temp.path().join("bad-contract.json");
    fs::write(
        &bad_contract,
        r#"{
          "schema_version": "ao2.hermes-project-start-poll-act-contract.v1",
          "decision_table": [],
          "trust_boundary": {
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
          }
        }"#,
    )
    .unwrap();
    let bad = ao2([
        "factory",
        "queue-project-start-next-action",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        bad_contract.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !bad.status.success(),
        "next-action must fail closed on incomplete contract"
    );
    assert!(stderr(&bad).contains("contract omits blocker_code"));
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
fn cli_factory_queue_project_start_publish_operator_record_fails_closed_and_writes_compact_record()
{
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let out_dir = temp.path().join("publish-operator-record-out");
    let run_id = "queued-project-start-publish-operator-record";
    let record_out = temp.path().join("operator-record.json");
    let contract = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/contracts/hermes-project-start-poll-act-contract.v1.json");
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");

    let missing_queue = ao2([
        "factory",
        "queue-project-start-publish-operator-record",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--record-out",
        record_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !missing_queue.status.success(),
        "publisher must fail closed before the compact completion artifacts exist"
    );
    assert!(
        stderr(&missing_queue).contains("next action is call_queue_project_start_complete"),
        "{}",
        stderr(&missing_queue)
    );
    assert!(
        !record_out.exists(),
        "blocked publish must not write record"
    );
    assert!(
        !queue_path.exists(),
        "blocked publish must not create queue"
    );
    assert!(
        !out_dir.exists(),
        "blocked publish must not create artifacts"
    );

    let queue_path = write_probe_queue(&repo, run_id, "accepted", "factory_project_start");
    let queue_sha = sha256_path(&queue_path);
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
    let corrupt_consumer = ao2([
        "factory",
        "queue-project-start-publish-operator-record",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--record-out",
        record_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !corrupt_consumer.status.success(),
        "publisher must reject corrupt compact artifact trust boundaries"
    );
    assert!(
        stderr(&corrupt_consumer).contains("next action is operator_review_required"),
        "{}",
        stderr(&corrupt_consumer)
    );
    assert!(
        !record_out.exists(),
        "corrupt publish must not write record"
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));

    write_probe_compact_artifacts(&out_dir, run_id);
    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let run_next_sha = sha256_path(&run_next_path);
    let publish = ao2([
        "factory",
        "queue-project-start-publish-operator-record",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--contract",
        contract.to_str().unwrap(),
        "--record-out",
        record_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(publish.status.success(), "{}", stderr(&publish));
    let published: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(
        published["schema_version"],
        "ao2.factory-project-start-operator-record.v1"
    );
    assert_eq!(published["status"], "published");
    assert_eq!(published["run_id"], run_id);
    assert_eq!(published["record_path"], record_out.display().to_string());
    assert_eq!(
        published["read_only_preflight"]["next_action"],
        "publish_operator_record"
    );
    assert_eq!(
        published["record"]["schema_version"],
        "ao2.factory-project-start-operator-record.v1"
    );
    assert_eq!(published["record"]["run_id"], run_id);
    assert_eq!(published["record"]["status"], "ready_for_operator_review");
    assert_eq!(published["record"]["queue_sha256"], queue_sha);
    assert_eq!(
        published["record"]["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        published["record"]["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        published["record"]["trust_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert_eq!(
        published["record"]["source_artifacts"]["queue_run_next"]["sha256"],
        run_next_sha
    );
    assert_eq!(queue_sha, sha256_path(&queue_path));
    assert_eq!(run_next_sha, sha256_path(&run_next_path));

    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&record_out).unwrap()).unwrap();
    assert_eq!(record, published["record"]);
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

#[test]
fn cli_factory_queue_status_reads_completed_project_start_without_mutating_queue() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let project_spec = temp.path().join("project.md");
    fs::write(
        &project_spec,
        r#"# Queue Status Project

## App Steps

- Build a minimal governed workflow fixture.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("queue-status-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let project_root = temp.path().join("generated-project");
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
        "queue-status-project",
        "--provider",
        "scripted",
        "--provider-prompt-dir",
        project_root.join("provider-prompts").to_str().unwrap(),
        "--verifier-command",
        "true",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "queue-status-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));

    let queued_status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queue-status-project",
        "--json",
    ]);
    assert!(
        !queued_status.status.success(),
        "unfinished queue entries must fail closed"
    );
    assert!(stderr(&queued_status).contains("not completed yet"));

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));
    let run_next_json: serde_json::Value = serde_json::from_str(&stdout(&run_next)).unwrap();
    let queue_path = Path::new(run_next_json["queue_path"].as_str().unwrap());
    let queue_sha_before = sha256_path(queue_path);

    let status = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "queue-status-project",
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let detail: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(detail["schema_version"], "ao2.factory-queue-status.v1");
    assert_eq!(detail["status"], "accepted");
    assert_eq!(detail["entry"]["run_id"], "queue-status-project");
    assert_eq!(
        detail["entry"]["project_start_operator_summary_status"],
        "accepted"
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_result"]["schema_version"],
        "ao2.factory-project-start-operator-summary.v1"
    );
    assert_eq!(
        detail["entry"]["project_start_bundle_verification_status"],
        "accepted"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(detail["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        detail["parity_checklist_progress"]["ao2_queue_status_detail_is_read_only"],
        true
    );
    assert_eq!(
        detail["entry"]["project_start_operator_summary_sha256"],
        sha256_path(Path::new(
            detail["entry"]["project_start_operator_summary"]
                .as_str()
                .unwrap()
        ))
    );
    assert_eq!(queue_sha_before, sha256_path(queue_path));
    assert!(!stdout(&status).contains("Bearer "));
}

#[test]
fn cli_factory_queue_status_can_select_latest_completed_project_start_without_mutating_queue() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let queue_dir = repo.join(".ao2/factory-compat");
    fs::create_dir_all(&queue_dir).unwrap();
    let queue_path = queue_dir.join("queue.json");
    let old_summary = temp.path().join("old-project-start-summary.json");
    let latest_summary = temp.path().join("latest-project-start-summary.json");
    let old_closure = temp.path().join("old-project-start-closure.tgz");
    let old_closure_json = temp.path().join("old-project-start-closure.json");
    let old_closure_verification = temp
        .path()
        .join("old-project-start-closure-verification.json");
    let latest_closure = temp.path().join("latest-project-start-closure.tgz");
    let latest_closure_json = temp.path().join("latest-project-start-closure.json");
    let latest_closure_verification = temp
        .path()
        .join("latest-project-start-closure-verification.json");
    fs::write(
        &old_summary,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-operator-summary.v1",
            "status": "accepted",
            "run_id": "old-project-start",
            "checks": {"bundle_digest_matches": true}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &latest_summary,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-operator-summary.v1",
            "status": "accepted",
            "run_id": "latest-project-start",
            "checks": {"bundle_digest_matches": true}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&old_closure, b"old closure archive").unwrap();
    fs::write(
        &old_closure_json,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure.v1",
            "status": "packaged",
            "run_id": "old-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &old_closure_verification,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure-verification.v1",
            "status": "accepted",
            "run_id": "old-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&latest_closure, b"latest closure archive").unwrap();
    fs::write(
        &latest_closure_json,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure.v1",
            "status": "packaged",
            "run_id": "latest-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &latest_closure_verification,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-project-start-closure-verification.v1",
            "status": "accepted",
            "run_id": "latest-project-start"
        }))
        .unwrap(),
    )
    .unwrap();
    let old_summary_sha = sha256_path(&old_summary);
    let latest_summary_sha = sha256_path(&latest_summary);
    let old_closure_sha = sha256_path(&old_closure);
    let old_closure_json_sha = sha256_path(&old_closure_json);
    let old_closure_verification_sha = sha256_path(&old_closure_verification);
    let latest_closure_sha = sha256_path(&latest_closure);
    let latest_closure_json_sha = sha256_path(&latest_closure_json);
    let latest_closure_verification_sha = sha256_path(&latest_closure_verification);
    fs::write(
        &queue_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-workbench-queue.v1",
            "owner": "ao2-workbench-queue",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "entry_count": 3,
            "continuity_contract": {
                "survives_server_restart": true,
                "factory_v3_drives_workflow": false,
                "cancel_retry_state_owner": "ao2-workbench-queue",
                "history_owner": "ao2",
                "hermes_role": "front_end_scheduler_queue_and_memory_bookkeeping"
            },
            "entries": [
                {
                    "run_id": "old-project-start",
                    "job_kind": "factory_project_start",
                    "status": "accepted",
                    "project_start_operator_summary": old_summary.display().to_string(),
                    "project_start_operator_summary_sha256": old_summary_sha,
                    "project_start_closure": old_closure.display().to_string(),
                    "project_start_closure_sha256": old_closure_sha,
                    "project_start_closure_json": old_closure_json.display().to_string(),
                    "project_start_closure_json_sha256": old_closure_json_sha,
                    "project_start_closure_status": "packaged",
                    "project_start_closure_verification": old_closure_verification.display().to_string(),
                    "project_start_closure_verification_sha256": old_closure_verification_sha,
                    "project_start_closure_verification_status": "accepted"
                },
                {
                    "run_id": "latest-project-start",
                    "job_kind": "factory_project_start",
                    "status": "accepted",
                    "project_start_operator_summary": latest_summary.display().to_string(),
                    "project_start_operator_summary_sha256": latest_summary_sha,
                    "project_start_closure": latest_closure.display().to_string(),
                    "project_start_closure_sha256": latest_closure_sha,
                    "project_start_closure_json": latest_closure_json.display().to_string(),
                    "project_start_closure_json_sha256": latest_closure_json_sha,
                    "project_start_closure_status": "packaged",
                    "project_start_closure_verification": latest_closure_verification.display().to_string(),
                    "project_start_closure_verification_sha256": latest_closure_verification_sha,
                    "project_start_closure_verification_status": "accepted"
                },
                {
                    "run_id": "newer-but-still-running",
                    "job_kind": "factory_project_start",
                    "status": "running"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let queue_sha_before = sha256_path(&queue_path);

    let latest = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(latest.status.success(), "{}", stderr(&latest));
    let detail: serde_json::Value = serde_json::from_str(&stdout(&latest)).unwrap();
    assert_eq!(detail["schema_version"], "ao2.factory-queue-status.v1");
    assert_eq!(detail["status"], "accepted");
    assert_eq!(detail["run_id"], "latest-project-start");
    assert_eq!(detail["entry"]["run_id"], "latest-project-start");
    assert_eq!(
        detail["entry"]["project_start_operator_summary_sha256"],
        latest_summary_sha
    );
    assert_eq!(
        detail["entry"]["project_start_closure_sha256"],
        latest_closure_sha
    );
    assert_eq!(
        detail["entry"]["project_start_closure_json_sha256"],
        latest_closure_json_sha
    );
    assert_eq!(
        detail["entry"]["project_start_closure_verification_sha256"],
        latest_closure_verification_sha
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        detail["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(detail["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        detail["parity_checklist_progress"]["ao2_queue_status_detail_is_read_only"],
        true
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert!(!stdout(&latest).contains("Bearer "));

    let both_selectors = ao2([
        "factory",
        "queue-status",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "latest-project-start",
        "--latest-completed-project-start",
        "--json",
    ]);
    assert!(
        !both_selectors.status.success(),
        "--run-id and --latest-completed-project-start must be mutually exclusive"
    );
    assert!(stderr(&both_selectors).contains("mutually exclusive"));
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

#[test]
fn cli_init_provider_profiles_and_template_run_support_fast_start() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let init = ao2(["init", "--target", repo.to_str().unwrap()]);
    assert!(init.status.success(), "{}", stderr(&init));
    let profiles = fs::read_to_string(repo.join(".ao2/provider-profiles.json")).unwrap();
    assert!(profiles.contains("\"codex\""));
    assert!(profiles.contains("\"claude\""));
    assert!(profiles.contains("\"scripted\""));

    let list = ao2(["provider", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(stdout(&list).contains("codex"));
    assert!(stdout(&list).contains("claude"));

    let doctor = ao2(["provider", "doctor", "--provider", "scripted"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let doctor_json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(doctor_json["provider"], "scripted");

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
printf 'Summary: provider preset template run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "--template",
        "bug-fix",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "preset-template-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Accepted"));
    assert!(repo.join(".ao2/generated-workflows/bug-fix.yaml").is_file());
}

#[test]
fn cli_run_provider_prompt_executes_provider_backed_risky_run() {
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
printf 'Summary: added validation around discount math\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 10\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-cli-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Accepted"));
    let evidence = fs::read_to_string(
        repo.join(".ao2/runs/provider-cli-run/evidence-pack/evidence-pack.json"),
    )
    .unwrap();
    assert!(evidence.contains("sandbox_patch_apply"));
    assert!(evidence.contains("provider_summaries"));
    assert!(evidence.contains("added validation around discount math"));
}

#[test]
fn cli_run_provider_prompt_honors_zero_repair_budget() {
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
printf 'Summary: validation without tests\n'
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
        "provider-cli-budget-zero",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Rejected"));
    let evidence = fs::read_to_string(
        repo.join(".ao2/runs/provider-cli-budget-zero/evidence-pack/evidence-pack.json"),
    )
    .unwrap();
    assert!(evidence.contains("repair_budget_exhausted"));
    assert!(evidence.contains("repair_attempts"));
}

#[test]
fn cli_repair_resume_uses_rejected_evidence_context_for_new_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project-repair-resume");
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(repo.join("README.md"), "real project\n").unwrap();
    init_existing_git_repo(&repo);
    let workflow = temp.path().join("repair-resume.yaml");
    fs::write(
        &workflow,
        r#"id: repair-resume
version: 0.1.0
template_kind: real_project
objective: Repair a failed run from prior signed evidence context.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test -f docs/fixed.txt
acceptance:
  - Fixed artifact exists after repair resume.
  - Prior verifier context is carried into the repair prompt.
"#,
    )
    .unwrap();
    let failed_prompt = temp.path().join("failed-prompt.sh");
    fs::write(
        &failed_prompt,
        r#"printf 'first attempt\n' > docs/first-attempt.txt
printf 'Summary: failed repair source run\n'
printf 'Changed files: docs/first-attempt.txt\n'
"#,
    )
    .unwrap();

    let failed = ao2([
        "run",
        workflow.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "repair-source",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        failed_prompt.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);
    assert!(failed.status.success(), "{}", stderr(&failed));
    assert!(stdout(&failed).contains("status=Rejected"));
    let source_evidence = repo.join(".ao2/runs/repair-source/evidence-pack/evidence-pack.json");
    let source_evidence_text = fs::read_to_string(&source_evidence).unwrap();
    assert!(source_evidence_text.contains("budget_exhausted"));

    let repair_prompt = temp.path().join("repair-prompt.sh");
    fs::write(
        &repair_prompt,
        r#"if printf '%s' "$AO2_REPAIR_RUN_HEALTH" | grep -q 'budget_exhausted' \
  && printf '%s' "$AO2_REPAIR_VERIFIER_OUTPUT" | grep -q 'docs/fixed.txt' \
  && test "$AO2_REPAIR_SOURCE_RUN_ID" = "repair-source"; then
  printf 'fixed\n' > docs/fixed.txt
else
  printf 'missing carried repair context\n' >&2
  exit 2
fi
printf 'Summary: repaired from rejected AO2 evidence context\n'
printf 'Changed files: docs/fixed.txt\n'
"#,
    )
    .unwrap();

    let repaired = ao2([
        "repair",
        "resume",
        "--evidence-pack",
        source_evidence.to_str().unwrap(),
        "--workflow",
        workflow.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "repair-resumed",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        repair_prompt.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
        "--json",
    ]);
    assert!(repaired.status.success(), "{}", stderr(&repaired));
    let repaired_json: serde_json::Value = serde_json::from_str(&stdout(&repaired)).unwrap();
    assert_eq!(repaired_json["schema_version"], "ao2.repair-resume.v1");
    assert_eq!(repaired_json["source_run_id"], "repair-source");
    assert_eq!(repaired_json["status"], "accepted");
    assert_eq!(
        fs::read_to_string(repo.join("docs/fixed.txt")).unwrap(),
        "fixed\n"
    );

    let repaired_evidence =
        fs::read_to_string(repo.join(".ao2/runs/repair-resumed/evidence-pack/evidence-pack.json"))
            .unwrap();
    assert!(repaired_evidence.contains("repair_source_context"));
    assert!(repaired_evidence.contains("\"source_run_id\": \"repair-source\""));
    assert!(repaired_evidence.contains("docs/fixed.txt"));
    assert!(repaired_evidence.contains("repair_source"));
    assert!(repaired_evidence.contains("provider_transcript_summary"));
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
fn cli_release_phase1_decision_publish_signs_and_posts_to_control_plane() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": "a".repeat(64),
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": "phase1-promotion-checklist.json"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let expected_decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    let expected_decision_raw = serde_json::to_string_pretty(&expected_decision).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 decision publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request
            .contains("\"schema_version\":\"ao2.cp-phase1-promotion-decision-signed-upload.v1\""));
        assert!(request.contains("\"schema\":\"factory-v3/ao2-phase1-promotion-decision/v1\""));
        assert!(request.contains("\"decision\":\"promote_phase1_candidate\""));
        assert!(request.contains("\"signature_algorithm\":\"RSA/SHA-256\""));
        assert!(request.contains("\"signature_hex\""));
        assert!(request.contains("\"public_key_sha256\""));
        assert!(request.contains("\"public_key_pem\""));
        assert!(request.contains("\"signer_id\":\"release-lead\""));
        assert!(!request.contains("cp-token\""));
        let request_body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("signed phase1 decision request has body");
        let upload: serde_json::Value = serde_json::from_str(request_body).unwrap();
        let decision_b64 = upload["decision_b64"]
            .as_str()
            .expect("signed phase1 decision upload carries exact decision_b64 bytes");
        {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let decoded = BASE64_STANDARD.decode(decision_b64).unwrap();
            assert_eq!(decoded, expected_decision_raw.as_bytes());
        }
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decision123","stored_at":"2026-05-22T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "release",
        "phase1-decision-publish",
        "--decision",
        decision_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-control-plane-publish.v1"
    );
    assert_eq!(json["signed"], true);
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/decision/signed")
    );
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "factory-v3/ao2-phase1-promotion-decision/v1"
    );
    assert_eq!(
        json["signature"]["schema_version"],
        "ao2.cp-phase1-promotion-decision-signature.v1"
    );
}

#[test]
fn cli_release_phase1_decision_publish_posts_referenced_checklist_before_signed_decision() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let checklist_path = temp.path().join("phase1-promotion-checklist.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let checklist = serde_json::json!({
        "schema": "factory-v3/ao2-phase1-promotion-checklist/v1",
        "schema_version": "ao2.phase1-promotion-checklist.v1",
        "status": "passed",
        "phase1_state": "phase1_candidate_ready",
        "next_action": "publish signed Phase 1 promotion decision",
        "checklist": {
            "provider_readiness": {"status": "superseded_by_live_acceptance", "phase1_state": "passed"},
            "live_provider_acceptance": {"status": "passed", "state": "live_acceptance_complete"},
            "release_gate": {"status": "passed", "state": "verified"},
            "three_os_smoke": {"status": "passed", "state": "accepted"}
        }
    });
    fs::write(
        &checklist_path,
        serde_json::to_string_pretty(&checklist).unwrap(),
    )
    .unwrap();
    let checklist_sha = canonical_sha256_for_test(&checklist);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": checklist_sha,
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": checklist_path.file_name().unwrap().to_string_lossy()
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let checklist_sha_for_server = checklist_sha.clone();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let mut attempts = 0;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        attempts += 1;
                        assert!(
                            attempts <= 100,
                            "timed out waiting for Phase 1 publish request"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            };
            let mut buffer = [0_u8; 32768];
            stream.set_nonblocking(false).unwrap();
            let read = read_test_http_request(&mut stream, &mut buffer);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body = if request.starts_with("POST /api/v1/phase1/promotion/checklist HTTP/1.1") {
                format!(
                    r#"{{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"{checklist_sha_for_server}","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-checklist/v1"}}"#
                )
            } else {
                r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decision456","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#.to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            requests.push(request);
        }
        assert!(requests[0].starts_with("POST /api/v1/phase1/promotion/checklist HTTP/1.1"));
        assert!(requests[1].starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(requests[0].contains("Authorization: Bearer cp-token"));
        assert!(requests[1].contains("Authorization: Bearer cp-token"));
        assert!(requests[0].contains("\"schema\":\"factory-v3/ao2-phase1-promotion-checklist/v1\""));
        assert!(requests[1].contains("\"checklist_sha256\""));
        assert!(!requests.join("\n").contains("cp-token\""));
    });

    let publish = ao2([
        "release",
        "phase1-decision-publish",
        "--decision",
        decision_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(json["checklist_publish"]["status"], "posted");
    assert_eq!(
        json["checklist_publish"]["receipt"]["sha256"],
        checklist_sha
    );
    assert_eq!(json["receipt"]["sha256"], "decision456");
}

#[test]
fn cli_release_phase1_decision_build_binds_release_and_replacement_gates() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
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
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": []
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the replacement run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-build.v1"
    );
    assert_eq!(json["status"], "written");
    assert_eq!(json["decision"]["decision"], "promote_phase1_candidate");
    assert_eq!(json["checklist"]["status"], "passed");
    assert_eq!(
        json["checklist"]["replacement_smoke_gate"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["checklist"]["trust_boundary"]["ao2_decision_owner"],
        "ao2-native-phase1-promotion-decision-builder"
    );
    assert!(decision_path.is_file());
    let decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    assert_eq!(
        decision["schema"],
        "factory-v3/ao2-phase1-promotion-decision/v1"
    );
    assert_eq!(decision["status"], "passed");
    assert_eq!(decision["phase1_state"], "phase1_candidate_ready");
    assert_eq!(
        decision["artifacts"]["replacement_smoke_gate"],
        replacement_gate_path.display().to_string()
    );
    assert_eq!(
        decision["trust_boundary"]["factory_v3_role"],
        "parity_oracle_only"
    );
}

#[test]
fn cli_release_phase1_decision_build_binds_three_os_governed_run_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the governed run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["three_os_governed_run"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["checklist"]["release_gate"]["governed_run_evidence_verification"]["status"],
        "verified"
    );
    assert!(json["checklist"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |check| check["id"] == "governed-run-evidence-accepted" && check["status"] == "passed"
        ));
    let decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    assert_eq!(
        decision["artifacts"]["governed_run_evidence"],
        serde_json::json!([
            governed_run_paths[0].display().to_string(),
            governed_run_paths[1].display().to_string(),
            governed_run_paths[2].display().to_string()
        ])
    );
    assert_eq!(
        decision["artifacts"]["factory_project_run_readback"],
        serde_json::json!([
            project_run_readbacks[0].display().to_string(),
            project_run_readbacks[1].display().to_string(),
            project_run_readbacks[2].display().to_string()
        ])
    );
}

#[test]
fn cli_release_phase1_decision_build_allows_governed_run_only_promotion() {
    let temp = tempfile::tempdir().unwrap();
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
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
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": []
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 governed-run evidence supersedes the legacy replacement-smoke gate.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["checklist"]["three_os_smoke"]["status"],
        "superseded_by_governed_run"
    );
    assert_eq!(
        json["checklist"]["three_os_governed_run"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["decision"]["artifacts"]["governed_run_evidence"],
        serde_json::json!([
            governed_run_paths[0].display().to_string(),
            governed_run_paths[1].display().to_string(),
            governed_run_paths[2].display().to_string()
        ])
    );
    assert!(json["decision"]["artifacts"]["replacement_smoke_gate"].is_null());
}

#[test]
fn cli_release_phase1_decision_build_rejects_missing_project_run_readback_hard_gate() {
    let temp = tempfile::tempdir().unwrap();
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 must not promote without replacement-packet readback proof.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("project-run readback"));
}

#[test]
fn cli_release_phase1_decision_build_rejects_missing_governed_run_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Missing governed run evidence should not promote.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("governed run evidence"));
}

#[test]
fn cli_release_phase1_decision_build_binds_three_provider_acceptance_preservation() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let provider_acceptance_path = temp.path().join("provider-acceptance-preservation.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &provider_acceptance_path,
        serde_json::to_string_pretty(&accepted_provider_acceptance_preservation_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--provider-acceptance-preservation",
        provider_acceptance_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the replacement run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["provider_acceptance_preservation"]["providers"],
        serde_json::json!(["codex", "claude", "antigravity"])
    );
    assert_eq!(
        json["decision"]["artifacts"]["provider_acceptance_preservation"],
        provider_acceptance_path.display().to_string()
    );
    assert!(json["checklist"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |check| check["id"] == "provider-acceptance-preservation-verified"
                && check["status"] == "passed"
        ));
}

#[test]
fn cli_release_phase1_decision_build_rejects_incomplete_provider_acceptance_preservation() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let provider_acceptance_path = temp.path().join("provider-acceptance-preservation.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();
    let mut provider_acceptance = accepted_provider_acceptance_preservation_fixture();
    provider_acceptance["providers"]
        .as_object_mut()
        .unwrap()
        .remove("antigravity");
    fs::write(
        &provider_acceptance_path,
        serde_json::to_string_pretty(&provider_acceptance).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--provider-acceptance-preservation",
        provider_acceptance_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Provider acceptance must be complete.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("provider acceptance preservation missing antigravity"));
}

#[test]
fn cli_release_phase1_decision_build_rejects_unverified_replacement_gate() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    let mut replacement_gate = accepted_replacement_smoke_gate_fixture();
    replacement_gate["status"] = serde_json::json!("rejected");
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&replacement_gate).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-gate.v1",
            "status": "verified",
            "replacement_smoke_gate": {
                "schema": "ao2.release-replacement-smoke-gate-verification.v1",
                "status": "failed",
                "gate_status": "rejected",
                "accepted_os": ["macos", "ubuntu"],
                "reasons": [{"code": "replacement_smoke_gate_missing_os"}]
            },
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": [{"code": "replacement_smoke_gate_failed"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Bad gate should not promote.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("replacement smoke gate must be accepted"));
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
fn cli_release_phase1_decision_publish_reads_api_token_from_env_without_printing_secret() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": "b".repeat(64),
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": "phase1-promotion-checklist.json"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 decision publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-phase1-token"));
        assert!(request.contains("\"decision\":\"promote_phase1_candidate\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decisionenv123","stored_at":"2026-05-22T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "release",
            "phase1-decision-publish",
            "--decision",
            decision_path.to_str().unwrap(),
            "--signing-key",
            signing_key.to_str().unwrap(),
            "--signer-id",
            "release-lead",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_PHASE1_CP_TOKEN",
            "--json",
        ],
        [("AO2_TEST_PHASE1_CP_TOKEN", "env-phase1-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    let stderr = stderr(&publish);
    assert!(!stdout.contains("env-phase1-token"));
    assert!(!stderr.contains("env-phase1-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-control-plane-publish.v1"
    );
    assert_eq!(json["receipt"]["sha256"], "decisionenv123");
}

#[test]
fn cli_release_phase1_three_os_smoke_build_materializes_control_plane_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let smoke_root = temp.path().join("three-os-smoke");
    fs::create_dir_all(&smoke_root).unwrap();
    let local_log = smoke_root.join("local-smoke.log");
    let windows_log = smoke_root.join("windows-smoke.log");
    let report = smoke_root.join("report.md");
    fs::write(&local_log, "local smoke passed\n").unwrap();
    fs::write(&windows_log, "windows native smoke passed\n").unwrap();
    fs::write(&report, "# report\n").unwrap();

    let summary_path = smoke_root.join("summary.enriched.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "root": smoke_root.display().to_string(),
            "report": report.display().to_string(),
            "local_smoke": "passed",
            "linux_x86_64_remote_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "passed",
            "windows_log": windows_log.display().to_string()
        }))
        .unwrap(),
    )
    .unwrap();
    let provenance_path = temp.path().join("ao2-release-provenance.json");
    fs::write(
        &provenance_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-provenance.v1",
            "version": "0.4.80",
            "git_commit": "addb602d07e413ca5b565d8ebca986925a97017f",
            "git_dirty": false,
            "release_tag": "v0.4.80"
        }))
        .unwrap(),
    )
    .unwrap();
    let out = temp.path().join("phase1-three-os-release-smoke.json");

    let build = ao2([
        "release",
        "phase1-three-os-smoke-build",
        "--summary",
        summary_path.to_str().unwrap(),
        "--provenance",
        provenance_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(json["schema_version"], "ao2.phase1-three-os-smoke-build.v1");
    assert_eq!(json["status"], "written");
    assert!(out.is_file());

    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(
        bundle["schema"],
        "ao2-control-plane.three-os-release-smoke.v1"
    );
    assert_eq!(bundle["status"], "passed");
    assert_eq!(bundle["version"], "0.4.80");
    assert_eq!(bundle["release_candidate_version"], "0.4.80");
    assert_eq!(
        bundle["source_commit"],
        "addb602d07e413ca5b565d8ebca986925a97017f"
    );
    assert_eq!(bundle["source_dirty"], false);
    assert_eq!(bundle["targets"]["macos"]["status"], "passed");
    assert_eq!(bundle["targets"]["ubuntu"]["status"], "passed");
    assert_eq!(bundle["targets"]["windows"]["status"], "passed");
    assert_eq!(
        bundle["targets"]["windows"]["log"],
        windows_log.display().to_string()
    );
    assert!(bundle["rerun_commands"]["all_required"]
        .as_str()
        .unwrap()
        .contains("<local-token>"));
}

#[test]
fn cli_release_phase1_three_os_smoke_publish_posts_bundle_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let smoke_path = temp.path().join("phase1-three-os-release-smoke.json");
    fs::write(
        &smoke_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2-control-plane.three-os-release-smoke.v1",
            "version": "0.4.80",
            "status": "passed",
            "release_candidate_version": "0.4.80",
            "source_commit": "addb602d07e413ca5b565d8ebca986925a97017f",
            "source_dirty": false,
            "targets": {
                "macos": {"status": "passed", "log": "target/three-os-smoke/run/local-smoke.log"},
                "ubuntu": {"status": "passed", "log": "target/three-os-smoke/run/local-smoke.log"},
                "windows": {"status": "passed", "log": "target/three-os-smoke/run/windows-smoke.log"}
            },
            "rerun_commands": [
                "AO2_PHASE1_CP_TOKEN=<local-token> target/release/ao2 release phase1-three-os-smoke-publish"
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream =
            accept_test_connection(&listener, "Phase 1 three-OS smoke publish request");
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/three-os-smoke HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema\":\"ao2-control-plane.three-os-release-smoke.v1\""));
        assert!(request.contains("\"status\":\"passed\""));
        assert!(request.contains("\"source_dirty\":false"));
        assert!(!request.contains("cp-token\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"threeos123","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"ao2-control-plane.three-os-release-smoke.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "release",
        "phase1-three-os-smoke-publish",
        "--smoke",
        smoke_path.to_str().unwrap(),
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    assert!(!stdout.contains("cp-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-three-os-smoke-control-plane-publish.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/three-os-smoke")
    );
    assert_eq!(json["receipt"]["sha256"], "threeos123");
}

#[test]
fn cli_release_phase1_promotion_inputs_publish_posts_verification_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let verification_path = temp.path().join("promotion-inputs-verification.json");
    fs::write(
        &verification_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.phase1-replacement-promotion-inputs-verification.v1",
            "status": "accepted",
            "mode": "decision_gate",
            "manifest_path": "/work/ao2/target/phase1-replacement-promotion/promotion-inputs.json",
            "missing_required_inputs": [],
            "failure_count": 0,
            "failures": [],
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream =
            accept_test_connection(&listener, "Phase 1 promotion inputs publish request");
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/inputs-verification HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-cp-token"));
        assert!(request.contains(
            "\"schema_version\":\"ao2.phase1-replacement-promotion-inputs-verification.v1\""
        ));
        assert!(request.contains("\"status\":\"accepted\""));
        assert!(request.contains("\"control_plane_approves_release\":false"));
        assert!(!request.contains("env-cp-token\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"inputs123","stored_at":"2026-05-29T00:00:00Z","ingested_schema_version":"ao2.phase1-replacement-promotion-inputs-verification.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "release",
            "phase1-promotion-inputs-publish",
            "--verification",
            verification_path.to_str().unwrap(),
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_PHASE1_CP_TOKEN",
            "--json",
        ],
        [("AO2_PHASE1_CP_TOKEN", "env-cp-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    assert!(!stdout.contains("env-cp-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-inputs-control-plane-publish.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/inputs-verification")
    );
    assert_eq!(json["receipt"]["sha256"], "inputs123");
}

#[test]
fn cli_release_phase1_history_fetch_reads_control_plane_history_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("phase1-history.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 history fetch request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/phase1/promotion/history.json HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        let body = r#"{"schema_version":"ao2.cp-phase1-promotion-history.v1","counts":{"checklists":1,"signed_decisions":1,"three_os_smokes":1},"history":{"checklists":[],"signed_decisions":[],"three_os_smokes":[]},"trust_boundary":{"role":"read_only_observer","mutates_ao_artifacts":false,"release_acceptance_owner":"factory-v3 evaluator-closer"}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let fetch = ao2([
        "release",
        "phase1-history-fetch",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    server.join().unwrap();
    assert!(fetch.status.success(), "{}", stderr(&fetch));
    let json: serde_json::Value = serde_json::from_str(&stdout(&fetch)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-history-control-plane-fetch.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/history.json")
    );
    assert_eq!(json["history"]["counts"]["checklists"], 1);
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(out.is_file());
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap();
    assert_eq!(
        written["schema_version"],
        "ao2.cp-phase1-promotion-history.v1"
    );
    assert!(!stdout(&fetch).contains("cp-token"));
}

#[test]
fn cli_release_phase1_history_fetch_accepts_api_token_env_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("phase1-history-env.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for Phase 1 history env-token fetch request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/phase1/promotion/history.json HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-cp-token"));
        let body = r#"{"schema_version":"ao2.cp-phase1-promotion-history.v1","counts":{"checklists":1,"signed_decisions":1,"three_os_smokes":1},"history":{"checklists":[],"signed_decisions":[],"three_os_smokes":[]},"trust_boundary":{"role":"read_only_observer","mutates_ao_artifacts":false,"release_acceptance_owner":"factory-v3 evaluator-closer"}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let fetch = ao2_with_env(
        [
            "release",
            "phase1-history-fetch",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_CP_TOKEN",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        [("AO2_TEST_CP_TOKEN", "env-cp-token")],
    );
    server.join().unwrap();
    assert!(fetch.status.success(), "{}", stderr(&fetch));
    let json: serde_json::Value = serde_json::from_str(&stdout(&fetch)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-history-control-plane-fetch.v1"
    );
    assert_eq!(json["history"]["counts"]["three_os_smokes"], 1);
    assert!(out.is_file());
    assert!(!stdout(&fetch).contains("env-cp-token"));
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
    let start_json = start_queue_job(port, "queue-detail-page", &prompt_path);
    let job_id = start_json["job_id"].as_str().unwrap();
    let job = wait_for_queue_job_status(port, "queue-detail-page", "accepted");
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
    let _ = child.kill();
    let _ = child.wait();
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

#[test]
fn cli_workbench_project_start_recovery_action_contract_guides_hermes_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("recovery-action-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-action-old",
        "workbench-project-start-recovery-action-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Action Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let Hermes choose the next restart-safe action from an AO2 contract.
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
            "project-start-recovery-action-test",
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
    }

    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let action = ao2([
        "factory",
        "queue-project-start-recovery-action",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(action.status.success(), "{}", stderr(&action));
    let json: serde_json::Value = serde_json::from_str(&stdout(&action)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-action.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(
        json["recommended_action"],
        "resume_from_latest_recovery_packet"
    );
    assert_eq!(
        json["selected"]["run_id"],
        "workbench-project-start-recovery-action-latest"
    );
    assert_sha256_string(&json["selected"]["queue_sha256"], "selected.queue_sha256");
    assert_sha256_string(
        &json["selected"]["recovery_packet_sha256"],
        "selected.recovery_packet_sha256",
    );
    assert_eq!(
        json["latest_recovery_selector"]["schema_version"],
        "ao2.factory-project-start-latest-recovery.v1"
    );
    assert_eq!(
        json["latest_recovery_selector"]["selected"]["run_id"],
        json["selected"]["run_id"]
    );
    let allowed_actions = json["allowed_actions"].as_array().unwrap();
    for expected in [
        "resume_from_latest_recovery_packet",
        "wait_for_queue_terminal",
        "record_completion_summary_memory",
        "operator_attention_required",
    ] {
        assert!(
            allowed_actions
                .iter()
                .any(|action| action["action"].as_str() == Some(expected)),
            "allowed action missing: {expected}"
        );
    }
    assert_eq!(
        json["exact_digest_requirements"]["resume_from_latest_recovery_packet"]["queue_sha256"],
        json["selected"]["queue_sha256"]
    );
    assert_eq!(
        json["exact_digest_requirements"]["resume_from_latest_recovery_packet"]
            ["recovery_packet_sha256"],
        json["selected"]["recovery_packet_sha256"]
    );
    assert_eq!(
        json["hermes_contract"]["front_end_can_poll_without_backend_execution"],
        true
    );
    assert_eq!(
        json["hermes_contract"]["front_end_must_call_ao2_backend_for_mutating_action"],
        true
    );
    assert_eq!(
        json["hermes_contract"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
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

    let route = "/api/factory/project-start/recovery/action?token=viewer-token";
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
        "ao2.factory-project-start-recovery-action.v1"
    );
    assert_eq!(api["selected"]["run_id"], json["selected"]["run_id"]);
    assert_eq!(api["recommended_action"], json["recommended_action"]);
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

#[test]
fn cli_workbench_project_start_recovery_resume_receipt_requires_exact_digests_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("recovery-resume-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-resume-old",
        "workbench-project-start-recovery-resume-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Resume Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let Hermes consume one digest-bound AO2 resume receipt.
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
            "project-start-recovery-resume-test",
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
    }

    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let receipt = ao2([
        "factory",
        "queue-project-start-recovery-resume-receipt",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(receipt.status.success(), "{}", stderr(&receipt));
    let json: serde_json::Value = serde_json::from_str(&stdout(&receipt)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-receipt.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["action"], "resume_from_latest_recovery_packet");
    assert_eq!(
        json["selected"]["run_id"],
        "workbench-project-start-recovery-resume-latest"
    );
    assert_eq!(json["selected"]["queue_sha256"], queue_sha);
    assert_eq!(
        json["selected"]["recovery_packet_sha256"],
        recovery_packet_sha
    );
    assert_eq!(
        json["action_contract"]["schema_version"],
        "ao2.factory-project-start-recovery-action.v1"
    );
    assert_eq!(
        json["action_contract"]["recommended_action"],
        "resume_from_latest_recovery_packet"
    );
    assert_eq!(json["digest_verification"]["queue_sha256_matches"], true);
    assert_eq!(
        json["digest_verification"]["recovery_packet_sha256_matches"],
        true
    );
    assert_eq!(
        json["backend_resume_payload"]["run_id"],
        json["selected"]["run_id"]
    );
    assert_eq!(
        json["backend_resume_payload"]["queue_sha256"],
        json["selected"]["queue_sha256"]
    );
    assert_eq!(
        json["backend_resume_payload"]["recovery_packet_sha256"],
        json["selected"]["recovery_packet_sha256"]
    );
    assert_eq!(
        json["backend_resume_payload"]["completion_summary_sha256"],
        json["recovery_packet"]["completion_summary"]["sha256"]
    );
    assert_eq!(
        json["backend_resume_payload"]["memory_record_id"],
        json["recovery_packet"]["memory_checkpoint_status"]["memory_record"]["id"]
    );
    assert_eq!(
        json["hermes_contract"]["front_end_can_submit_backend_resume_payload"],
        true
    );
    assert_eq!(
        json["hermes_contract"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_contract"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
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

    let drift = ao2([
        "factory",
        "queue-project-start-recovery-resume-receipt",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(!drift.status.success(), "{}", stdout(&drift));
    assert!(
        stderr(&drift).contains("queue_sha256 digest drift"),
        "{}",
        stderr(&drift)
    );

    let route = format!(
        "/api/factory/project-start/recovery/resume-receipt?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}"
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
        "ao2.factory-project-start-recovery-resume-receipt.v1"
    );
    assert_eq!(api["selected"]["run_id"], json["selected"]["run_id"]);
    assert_eq!(
        api["backend_resume_payload"],
        json["backend_resume_payload"]
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

#[test]
fn cli_workbench_project_start_recovery_resume_checkpoint_records_ao2_memory_with_exact_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-checkpoint-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-checkpoint-old",
        "workbench-project-start-recovery-checkpoint-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Checkpoint Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Record a digest-approved AO2 recovery resume checkpoint.
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
            "project-start-recovery-checkpoint-test",
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
    }

    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);
    let records_before = fs::read_to_string(&memory_records_path)
        .unwrap()
        .lines()
        .count();
    let links_before = fs::read_to_string(&memory_links_path)
        .unwrap()
        .lines()
        .count();

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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    assert_eq!(
        approval_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-checkpoint-approval.v1"
    );
    assert_eq!(approval_json["status"], "approval_required");
    assert_eq!(approval_json["approval_mode"], "exact_action_digest");
    assert_eq!(approval_json["required_flag"], "--approve-action-digest");
    assert_sha256_string(&approval_json["action_digest"], "action_digest");
    assert_eq!(
        approval_json["receipt"]["schema_version"],
        "ao2.factory-project-start-recovery-resume-receipt.v1"
    );
    assert_eq!(
        approval_json["receipt"]["selected"]["run_id"],
        "workbench-project-start-recovery-checkpoint-latest"
    );
    assert_eq!(
        approval_json["side_effects"]["would_write_memory_after_approval"],
        true
    );
    assert_eq!(approval_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        approval_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
    let json: serde_json::Value = serde_json::from_str(&stdout(&checkpoint)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-checkpoint.v1"
    );
    assert_eq!(json["status"], "recorded");
    assert_eq!(
        json["run_id"],
        "workbench-project-start-recovery-checkpoint-latest"
    );
    assert_eq!(json["approval"]["status"], "approved_exact_action_digest");
    assert_eq!(json["approval"]["action_digest"], digest);
    assert_eq!(json["queue_sha256"], queue_sha);
    assert_eq!(json["recovery_packet_sha256"], recovery_packet_sha);
    assert_eq!(
        json["receipt_sha256"],
        json["memory_record"]["source"]["path_sha256"]
    );
    assert_eq!(
        json["prior_memory_record_id"],
        json["receipt"]["backend_resume_payload"]["memory_record_id"]
    );
    assert_eq!(
        json["completion_summary_sha256"],
        json["receipt"]["backend_resume_payload"]["completion_summary_sha256"]
    );
    assert_eq!(
        json["memory_record"]["kind"],
        "project-start-recovery-resume-checkpoint"
    );
    assert_eq!(json["memory_record"]["source"]["run_id"], json["run_id"]);
    assert_eq!(json["memory_link"]["run_id"], json["run_id"]);
    assert_eq!(
        json["memory_link"]["relationship"],
        "project-start-recovery-resume-checkpoint"
    );
    assert_eq!(json["side_effects"]["wrote_memory_record"], true);
    assert_eq!(json["side_effects"]["wrote_memory_run_link"], true);
    assert_eq!(json["side_effects"]["executed_provider"], false);
    assert_eq!(json["side_effects"]["executed_queue"], false);
    assert_eq!(json["side_effects"]["submitted_queue_entry"], false);
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
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(
        fs::read_to_string(&memory_records_path)
            .unwrap()
            .lines()
            .count(),
        records_before + 1
    );
    assert_eq!(
        fs::read_to_string(&memory_links_path)
            .unwrap()
            .lines()
            .count(),
        links_before + 1
    );

    let route = "/api/factory/project-start/recovery/resume-checkpoint?token=operator-token";
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
    let body = format!("queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}");
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
    let body = format!(
        "queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&approval_action_digest={api_digest}"
    );
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
        "ao2.factory-project-start-recovery-resume-checkpoint.v1"
    );
    assert_eq!(api["status"], "recorded");
    assert_eq!(api["queue_sha256"], json["queue_sha256"]);
    assert_eq!(
        api["recovery_packet_sha256"],
        json["recovery_packet_sha256"]
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));

    let lower = response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
        "api-token",
        "operator-token",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

#[test]
fn cli_workbench_project_start_recovery_resume_checkpoint_status_reads_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-checkpoint-status-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-checkpoint-status-old",
        "workbench-project-start-recovery-checkpoint-status-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Checkpoint Status Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Read the AO2 recovery resume checkpoint status without mutating state.
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
            "project-start-recovery-checkpoint-status-test",
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
    }

    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
    let checkpoint_json: serde_json::Value = serde_json::from_str(&stdout(&checkpoint)).unwrap();
    assert_eq!(checkpoint_json["status"], "recorded");

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let status = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-checkpoint-status.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(
        json["run_id"],
        "workbench-project-start-recovery-checkpoint-status-latest"
    );
    assert_eq!(json["queue_sha256"], queue_sha);
    assert_eq!(json["recovery_packet_sha256"], recovery_packet_sha);
    assert_eq!(json["receipt_sha256"], checkpoint_json["receipt_sha256"]);
    assert_eq!(
        json["completion_summary_sha256"],
        checkpoint_json["completion_summary_sha256"]
    );
    assert_eq!(
        json["prior_memory_record_id"],
        checkpoint_json["prior_memory_record_id"]
    );
    assert_eq!(
        json["memory_record"]["id"],
        checkpoint_json["memory_record"]["id"]
    );
    assert_eq!(
        json["memory_record"]["kind"],
        "project-start-recovery-resume-checkpoint"
    );
    assert_eq!(
        json["memory_record"]["source"]["path_sha256"],
        checkpoint_json["receipt_sha256"]
    );
    assert_eq!(
        json["memory_link"]["memory_id"],
        checkpoint_json["memory_record"]["id"]
    );
    assert_eq!(
        json["memory_link"]["relationship"],
        "project-start-recovery-resume-checkpoint"
    );
    assert_eq!(
        json["receipt"]["schema_version"],
        "ao2.factory-project-start-recovery-resume-receipt.v1"
    );
    assert_eq!(json["receipt"]["selected"]["run_id"], json["run_id"]);
    assert_eq!(json["hermes_memory"]["checkpoint_is_durable"], true);
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "read_recovery_resume_checkpoint_status"
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
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

    let route = format!(
        "/api/factory/project-start/recovery/resume-checkpoint/status?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}"
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
        "ao2.factory-project-start-recovery-resume-checkpoint-status.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(api["memory_record"]["id"], json["memory_record"]["id"]);
    assert_eq!(
        api["memory_link"]["memory_id"],
        json["memory_link"]["memory_id"]
    );
    assert_eq!(api["receipt_sha256"], json["receipt_sha256"]);
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

#[test]
fn cli_workbench_project_start_recovery_resume_continuity_reads_chain_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-continuity-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-continuity-old",
        "workbench-project-start-recovery-continuity-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Continuity Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let Hermes read one AO2 continuity packet for recovery resume bookkeeping.
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
            "project-start-recovery-continuity-test",
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
    }

    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
    let checkpoint_json: serde_json::Value = serde_json::from_str(&stdout(&checkpoint)).unwrap();
    assert_eq!(checkpoint_json["status"], "recorded");

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let continuity = ao2([
        "factory",
        "queue-project-start-recovery-resume-continuity",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(continuity.status.success(), "{}", stderr(&continuity));
    let json: serde_json::Value = serde_json::from_str(&stdout(&continuity)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-continuity.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(
        json["run_id"],
        "workbench-project-start-recovery-continuity-latest"
    );
    assert_eq!(json["queue_sha256"], queue_sha);
    assert_eq!(json["recovery_packet_sha256"], recovery_packet_sha);
    assert_eq!(
        json["checkpoint_status"]["receipt_sha256"],
        checkpoint_json["receipt_sha256"]
    );
    assert_eq!(json["chain_verification"]["action_contract_ready"], true);
    assert_eq!(json["chain_verification"]["resume_receipt_ready"], true);
    assert_eq!(json["chain_verification"]["checkpoint_status_ready"], true);
    assert_eq!(json["chain_verification"]["checkpoint_is_durable"], true);
    assert_eq!(
        json["chain_verification"]["exact_digest_chain_matches"],
        true
    );
    assert_eq!(
        json["continuity_packet"]["action_contract"]["schema_version"],
        "ao2.factory-project-start-recovery-action.v1"
    );
    assert_eq!(
        json["continuity_packet"]["resume_receipt"]["schema_version"],
        "ao2.factory-project-start-recovery-resume-receipt.v1"
    );
    assert_eq!(
        json["continuity_packet"]["resume_checkpoint_status"]["schema_version"],
        "ao2.factory-project-start-recovery-resume-checkpoint-status.v1"
    );
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "read_recovery_resume_continuity"
    );
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
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

    let route = format!(
        "/api/factory/project-start/recovery/resume-continuity?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}"
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
        "ao2.factory-project-start-recovery-resume-continuity.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(api["run_id"], json["run_id"]);
    assert_eq!(
        api["checkpoint_status"]["memory_record"]["id"],
        json["checkpoint_status"]["memory_record"]["id"]
    );
    assert_eq!(
        api["continuity_packet"]["resume_checkpoint_status"]["receipt_sha256"],
        json["checkpoint_status"]["receipt_sha256"]
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

#[test]
fn cli_workbench_project_start_recovery_resume_plan_materializes_digest_bound_plan() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("recovery-resume-plan-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-plan-old",
        "workbench-project-start-recovery-plan-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Plan Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 materialize the next recovery-resume plan for Hermes.
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
            "project-start-recovery-plan-test",
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
    }

    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(approval.status.success(), "{}", stderr(&approval));
    let approval_json: serde_json::Value = serde_json::from_str(&stdout(&approval)).unwrap();
    let digest = approval_json["action_digest"].as_str().unwrap();
    let checkpoint = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-action-digest",
        digest,
        "--json",
    ]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let plan = ao2([
        "factory",
        "queue-project-start-recovery-resume-plan",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-plan.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(
        json["run_id"],
        "workbench-project-start-recovery-plan-latest"
    );
    assert_eq!(json["queue_sha256"], queue_sha);
    assert_eq!(json["recovery_packet_sha256"], recovery_packet_sha);
    assert_eq!(json["classification"]["size"], "bounded");
    assert_eq!(json["classification"]["shape"], "bug-fix");
    assert_eq!(json["evidence"][0]["kind"], "recovery_continuity_packet");
    assert_eq!(json["concerns"].as_array().unwrap().len(), 0);
    assert_eq!(json["blockers"].as_array().unwrap().len(), 0);
    assert_eq!(
        json["governed_recovery_resume_plan"]["action"],
        "resume_from_latest_recovery_packet"
    );
    assert_eq!(
        json["governed_recovery_resume_plan"]["selected_run_id"],
        json["run_id"]
    );
    assert_eq!(
        json["governed_recovery_resume_plan"]["receipt_sha256"],
        json["receipt_sha256"]
    );
    assert_eq!(
        json["governed_recovery_resume_plan"]["checkpoint_memory_record_id"],
        json["checkpoint_memory_record_id"]
    );
    assert_eq!(
        json["governed_recovery_resume_plan"]["checkpoint_run_link_matches"],
        true
    );
    assert_eq!(json["plan_digest_bound"], true);
    assert!(json["plan_sha256"].as_str().unwrap().len() == 64);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "execute_governed_recovery_resume_plan_after_operator_review"
    );
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
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

    let route = format!(
        "/api/factory/project-start/recovery/resume-plan?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}"
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
        "ao2.factory-project-start-recovery-resume-plan.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(api["plan_sha256"], json["plan_sha256"]);
    assert_eq!(
        api["governed_recovery_resume_plan"]["checkpoint_memory_record_id"],
        json["checkpoint_memory_record_id"]
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

#[test]
fn cli_workbench_project_start_recovery_resume_claim_requires_exact_plan_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("recovery-resume-claim-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-claim-old",
        "workbench-project-start-recovery-claim-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Claim Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 claim the recovery-resume plan only after exact digest approval.
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
            "project-start-recovery-claim-test",
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
    }

    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap();

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let approval_required = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(
        approval_required.status.success(),
        "{}",
        stderr(&approval_required)
    );
    let approval_json: serde_json::Value =
        serde_json::from_str(&stdout(&approval_required)).unwrap();
    assert_eq!(
        approval_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-claim-approval.v1"
    );
    assert_eq!(approval_json["status"], "approval_required");
    assert_eq!(approval_json["approval_mode"], "exact_plan_sha256");
    assert_eq!(approval_json["plan_sha256"], plan_sha);
    assert_eq!(approval_json["required_flag"], "--approve-plan-sha256");
    assert_eq!(approval_json["required_form_field"], "approval_plan_sha256");
    assert_eq!(
        approval_json["blockers"][0]["code"],
        "operator_plan_digest_approval_required"
    );
    assert_eq!(
        approval_json["side_effects"]["would_write_memory_after_approval"],
        true
    );
    assert_eq!(
        approval_json["side_effects"]["would_execute_provider"],
        false
    );
    assert_eq!(approval_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        approval_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(
        approval_json["side_effects"]["would_approve_release"],
        false
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let mismatch = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(mismatch.status.success(), "{}", stderr(&mismatch));
    let mismatch_json: serde_json::Value = serde_json::from_str(&stdout(&mismatch)).unwrap();
    assert_eq!(mismatch_json["status"], "approval_digest_mismatch");
    assert_eq!(mismatch_json["blockers"][0]["code"], "plan_digest_mismatch");
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));
    let json: serde_json::Value = serde_json::from_str(&stdout(&claim)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-claim.v1"
    );
    assert_eq!(json["status"], "claimed");
    assert_eq!(
        json["run_id"],
        "workbench-project-start-recovery-claim-latest"
    );
    assert_eq!(json["approved_plan_sha256"], plan_sha);
    assert_eq!(json["plan"]["plan_sha256"], plan_sha);
    assert_eq!(
        json["memory_record"]["kind"],
        "project-start-recovery-resume-claim"
    );
    assert_eq!(
        json["memory_record"]["source"]["path_sha256"],
        json["approved_plan_sha256"]
    );
    assert_eq!(
        json["memory_link"]["relationship"],
        "project-start-recovery-resume-claim"
    );
    assert_eq!(json["side_effects"]["wrote_memory_record"], true);
    assert_eq!(json["side_effects"]["wrote_memory_run_link"], true);
    assert_eq!(json["side_effects"]["executed_provider"], false);
    assert_eq!(json["side_effects"]["executed_queue"], false);
    assert_eq!(json["side_effects"]["submitted_queue_entry"], false);
    assert_eq!(json["side_effects"]["wrote_queue_file"], false);
    assert_eq!(json["side_effects"]["mutated_control_plane"], false);
    assert_eq!(json["side_effects"]["approved_release"], false);
    let canonical_memory_records_path = fs::canonicalize(&memory_records_path).unwrap();
    assert_eq!(
        json["changed_files"][0]["path"],
        canonical_memory_records_path.display().to_string()
    );
    assert_eq!(json["blockers"].as_array().unwrap().len(), 0);
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_ne!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_ne!(memory_links_sha_before, sha256_path(&memory_links_path));

    let memory_records_sha_after_claim = sha256_path(&memory_records_path);
    let memory_links_sha_after_claim = sha256_path(&memory_links_path);
    let body = format!(
        "queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&approval_plan_sha256={plan_sha}"
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
    let response = http_request(
        port,
        &format!(
            "POST /api/factory/project-start/recovery/resume-claim?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        api["schema_version"],
        "ao2.factory-project-start-recovery-resume-claim.v1"
    );
    assert_eq!(api["status"], "claimed");
    assert_eq!(api["approved_plan_sha256"], plan_sha);
    assert_eq!(api["side_effects"]["executed_provider"], false);
    assert_eq!(api["side_effects"]["executed_queue"], false);
    assert_eq!(api["side_effects"]["mutated_control_plane"], false);
    assert_eq!(api["side_effects"]["approved_release"], false);
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_ne!(
        memory_records_sha_after_claim,
        sha256_path(&memory_records_path)
    );
    assert_ne!(
        memory_links_sha_after_claim,
        sha256_path(&memory_links_path)
    );

    let lower = response.to_ascii_lowercase();
    for marker in [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
        "operator-token",
    ] {
        assert!(
            !lower.contains(marker),
            "response must not contain {marker}"
        );
    }
}

#[test]
fn cli_workbench_project_start_recovery_resume_claim_status_replays_exact_claim() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-claim-status-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    for run_id in [
        "workbench-project-start-recovery-claim-status-old",
        "workbench-project-start-recovery-claim-status-latest",
    ] {
        let project_spec = temp.path().join(format!("{run_id}.md"));
        fs::write(
            &project_spec,
            format!(
                r#"# Recovery Claim Status Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 replay the exact recovery-resume claim after Workbench restart.
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
            "project-start-recovery-claim-status-test",
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
    }

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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap();

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));
    let claim_json: serde_json::Value = serde_json::from_str(&stdout(&claim)).unwrap();
    assert_eq!(claim_json["status"], "claimed");

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-claim-status.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(
        json["run_id"],
        "workbench-project-start-recovery-claim-status-latest"
    );
    assert_eq!(json["plan_sha256"], plan_sha);
    assert_eq!(json["approved_plan_sha256"], plan_sha);
    assert_eq!(json["claim_record_count"], 1);
    assert_eq!(json["claim_link_count"], 1);
    assert_eq!(
        json["claim_memory_record"]["id"],
        claim_json["memory_record"]["id"]
    );
    assert_eq!(
        json["claim_memory_record"]["source"]["path_sha256"],
        json["plan_sha256"]
    );
    assert_eq!(
        json["claim_memory_link"]["relationship"],
        "project-start-recovery-resume-claim"
    );
    assert_eq!(json["replay_verification"]["plan_sha256_matches"], true);
    assert_eq!(json["replay_verification"]["claim_record_is_unique"], true);
    assert_eq!(
        json["replay_verification"]["claim_run_link_is_unique"],
        true
    );
    assert_eq!(
        json["replay_verification"]["claim_source_binds_approved_plan"],
        true
    );
    assert_eq!(json["blockers"].as_array().unwrap().len(), 0);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "observe_recovery_resume_claim_status_then_continue_governed_recovery"
    );
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_write_queue_file"], false);
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

    let route = format!(
        "/api/factory/project-start/recovery/resume-claim/status?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&plan_sha256={plan_sha}"
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
        "ao2.factory-project-start-recovery-resume-claim-status.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(
        api["claim_memory_record"]["id"],
        json["claim_memory_record"]["id"]
    );
    assert_eq!(
        api["claim_memory_link"]["memory_id"],
        json["claim_memory_record"]["id"]
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

    let duplicate_claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(
        duplicate_claim.status.success(),
        "{}",
        stderr(&duplicate_claim)
    );

    let duplicate_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(
        duplicate_status.status.success(),
        "{}",
        stderr(&duplicate_status)
    );
    let duplicate_json: serde_json::Value =
        serde_json::from_str(&stdout(&duplicate_status)).unwrap();
    assert_eq!(duplicate_json["status"], "blocked");
    assert_eq!(duplicate_json["claim_record_count"], 2);
    assert_eq!(
        duplicate_json["blockers"][0]["code"],
        "duplicate_recovery_resume_claim_records"
    );
    assert_eq!(
        duplicate_json["replay_verification"]["claim_record_is_unique"],
        false
    );
    assert_eq!(sha256_path(&queue_path), queue_sha_before);
}

#[test]
fn cli_workbench_project_start_recovery_resume_continuation_contract_binds_claim_status() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-continuation-contract-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let run_id = "workbench-project-start-recovery-continuation-contract";
    let project_spec = temp.path().join(format!("{run_id}.md"));
    fs::write(
        &project_spec,
        format!(
            r#"# Recovery Continuation Contract Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 issue a digest-bound continuation contract after claim replay.
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
        "project-start-recovery-continuation-contract-test",
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
        run_id,
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
        run_id,
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap();

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));
    let claim_json: serde_json::Value = serde_json::from_str(&stdout(&claim)).unwrap();
    assert_eq!(claim_json["status"], "claimed");

    let status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    let claim_status_sha = canonical_sha256_for_test(&status_json);

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let contract = ao2([
        "factory",
        "queue-project-start-recovery-resume-continuation-contract",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(contract.status.success(), "{}", stderr(&contract));
    let json: serde_json::Value = serde_json::from_str(&stdout(&contract)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-continuation-contract.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["queue_sha256"], queue_sha);
    assert_eq!(json["recovery_packet_sha256"], recovery_packet_sha);
    assert_eq!(json["plan_sha256"], plan_sha);
    assert_eq!(json["claim_status_sha256"], claim_status_sha);
    assert_eq!(json["expected_claim_status_sha256"], claim_status_sha);
    assert_eq!(json["classification"]["size"], "bounded");
    assert_eq!(json["classification"]["shape"], "bug-fix");
    assert_eq!(
        json["continuation_contract"]["required_prior_status"],
        "ready"
    );
    assert_eq!(
        json["continuation_contract"]["required_claim_status_sha256"],
        claim_status_sha
    );
    assert_eq!(
        json["continuation_contract"]["next_bounded_action"]["action"],
        "execute_recovery_resume_continuation_after_exact_status_digest_approval"
    );
    assert_eq!(
        json["continuation_contract"]["next_bounded_action"]["mutates_queue_or_memory"],
        true
    );
    assert_eq!(
        json["continuation_contract"]["current_contract_is_read_only"],
        true
    );
    assert_eq!(json["blockers"].as_array().unwrap().len(), 0);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "submit_exact_claim_status_digest_to_ao2_recovery_resume_continuation_executor"
    );
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_write_queue_file"], false);
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

    let route = format!(
        "/api/factory/project-start/recovery/resume-continuation-contract?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&plan_sha256={plan_sha}&claim_status_sha256={claim_status_sha}"
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
        "ao2.factory-project-start-recovery-resume-continuation-contract.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(api["claim_status_sha256"], json["claim_status_sha256"]);
    assert_eq!(
        api["continuation_contract"]["next_bounded_action"]["action"],
        json["continuation_contract"]["next_bounded_action"]["action"]
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

    let blocked = ao2([
        "factory",
        "queue-project-start-recovery-resume-continuation-contract",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(blocked.status.success(), "{}", stderr(&blocked));
    let blocked_json: serde_json::Value = serde_json::from_str(&stdout(&blocked)).unwrap();
    assert_eq!(blocked_json["status"], "blocked");
    assert_eq!(
        blocked_json["blockers"][0]["code"],
        "claim_status_digest_mismatch"
    );
    assert_eq!(
        blocked_json["continuation_contract"]["current_contract_is_read_only"],
        true
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));
}

#[test]
fn cli_workbench_project_start_recovery_resume_continue_requires_exact_claim_status_digest() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("recovery-resume-continue-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let run_id = "workbench-project-start-recovery-continue";
    let project_spec = temp.path().join(format!("{run_id}.md"));
    fs::write(
        &project_spec,
        format!(
            r#"# Recovery Continue Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 execute the digest-bound recovery-resume continuation after claim replay.
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
        "project-start-recovery-continue-test",
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
        run_id,
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
        run_id,
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap();

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));

    let status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    let claim_status_sha = canonical_sha256_for_test(&status_json);

    let approval_required = ao2([
        "factory",
        "queue-project-start-recovery-resume-continue",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(
        approval_required.status.success(),
        "{}",
        stderr(&approval_required)
    );
    let approval_json: serde_json::Value =
        serde_json::from_str(&stdout(&approval_required)).unwrap();
    assert_eq!(
        approval_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-continue-approval.v1"
    );
    assert_eq!(approval_json["status"], "approval_required");
    assert_eq!(
        approval_json["required_flag"],
        "--approve-claim-status-sha256"
    );
    assert_eq!(approval_json["claim_status_sha256"], claim_status_sha);

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));

    let continued = ao2([
        "factory",
        "queue-project-start-recovery-resume-continue",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--approve-claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(continued.status.success(), "{}", stderr(&continued));
    let json: serde_json::Value = serde_json::from_str(&stdout(&continued)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-continue.v1"
    );
    assert_eq!(json["status"], "continued");
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["approved_claim_status_sha256"], claim_status_sha);
    assert_eq!(
        json["continuation_memory_record"]["source"]["path_sha256"],
        claim_status_sha
    );
    assert_eq!(
        json["continuation_memory_link"]["relationship"],
        "project-start-recovery-resume-continuation"
    );
    assert_eq!(json["side_effects"]["wrote_memory_record"], true);
    assert_eq!(json["side_effects"]["wrote_memory_run_link"], true);
    assert_eq!(json["side_effects"]["executed_provider"], false);
    assert_eq!(json["side_effects"]["executed_queue"], false);
    assert_eq!(json["side_effects"]["submitted_queue_entry"], false);
    assert_eq!(json["side_effects"]["mutated_control_plane"], false);
    assert_eq!(json["side_effects"]["approved_release"], false);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "read_recovery_resume_continuation_evidence"
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
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_ne!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_ne!(memory_links_sha_before, sha256_path(&memory_links_path));

    let duplicate = ao2([
        "factory",
        "queue-project-start-recovery-resume-continue",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--approve-claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(duplicate.status.success(), "{}", stderr(&duplicate));
    let duplicate_json: serde_json::Value = serde_json::from_str(&stdout(&duplicate)).unwrap();
    assert_eq!(duplicate_json["status"], "blocked");
    assert_eq!(
        duplicate_json["blockers"][0]["code"],
        "duplicate_recovery_resume_continuation_records"
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
    let body = format!(
        "queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&plan_sha256={plan_sha}&claim_status_sha256={claim_status_sha}&approval_claim_status_sha256={claim_status_sha}"
    );
    let response = http_request(
        port,
        &format!(
            "POST /api/factory/project-start/recovery/resume-continue?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "blocked");
    assert_eq!(
        api["blockers"][0]["code"],
        "duplicate_recovery_resume_continuation_records"
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));

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
fn cli_workbench_project_start_recovery_resume_continuation_status_replays_c63_record() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-continuation-status-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let run_id = "workbench-project-start-recovery-continuation-status";
    let project_spec = temp.path().join(format!("{run_id}.md"));
    fs::write(
        &project_spec,
        format!(
            r#"# Recovery Continuation Status Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 replay the digest-bound continuation record after Workbench restart.
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
        "project-start-recovery-continuation-status-test",
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
        run_id,
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
        run_id,
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap();

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));

    let claim_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--approve-claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(continued.status.success(), "{}", stderr(&continued));

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let status = ao2([
        "factory",
        "queue-project-start-recovery-resume-continuation-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-continuation-status.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["queue_sha256"], queue_sha);
    assert_eq!(json["recovery_packet_sha256"], recovery_packet_sha);
    assert_eq!(json["plan_sha256"], plan_sha);
    assert_eq!(json["claim_status_sha256"], claim_status_sha);
    assert_eq!(json["continuation_record_count"], 1);
    assert_eq!(json["continuation_link_count"], 1);
    assert_eq!(
        json["continuation_memory_record"]["source"]["path_sha256"],
        claim_status_sha
    );
    assert_eq!(
        json["continuation_memory_link"]["relationship"],
        "project-start-recovery-resume-continuation"
    );
    assert_eq!(
        json["replay_verification"]["continuation_record_is_unique"],
        true
    );
    assert_eq!(
        json["replay_verification"]["continuation_run_link_is_unique"],
        true
    );
    assert_eq!(
        json["replay_verification"]["continuation_source_binds_claim_status"],
        true
    );
    assert_eq!(
        json["replay_verification"]["continuation_body_binds_claim_status"],
        true
    );
    assert_eq!(
        json["replay_verification"]["continuation_link_targets_record"],
        true
    );
    assert_eq!(
        json["replay_verification"]["workbench_restart_replayable"],
        true
    );
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "observe_recovery_resume_continuation_status_then_continue_governed_recovery"
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_write_queue_file"], false);
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

    let route = format!(
        "/api/factory/project-start/recovery/resume-continuation/status?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&plan_sha256={plan_sha}&claim_status_sha256={claim_status_sha}"
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
        "ao2.factory-project-start-recovery-resume-continuation-status.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(
        api["continuation_memory_record"]["source"]["path_sha256"],
        json["continuation_memory_record"]["source"]["path_sha256"]
    );
    assert_eq!(
        api["replay_verification"]["workbench_restart_replayable"],
        true
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

#[test]
fn cli_workbench_project_start_recovery_resume_post_continuation_action_binds_status() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-post-continuation-action-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let run_id = "workbench-project-start-recovery-post-continuation-action";
    let project_spec = temp.path().join(format!("{run_id}.md"));
    fs::write(
        &project_spec,
        format!(
            r#"# Recovery Post-Continuation Action Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 classify the next bounded action after continuation status replay.
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
        "project-start-recovery-post-continuation-action-test",
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
        run_id,
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
        run_id,
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap();

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));

    let claim_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
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

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let post_action = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-action",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--continuation-status-sha256",
        &continuation_status_sha,
        "--json",
    ]);
    assert!(post_action.status.success(), "{}", stderr(&post_action));
    let json: serde_json::Value = serde_json::from_str(&stdout(&post_action)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-action.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], run_id);
    assert_eq!(json["queue_sha256"], queue_sha);
    assert_eq!(json["recovery_packet_sha256"], recovery_packet_sha);
    assert_eq!(json["plan_sha256"], plan_sha);
    assert_eq!(json["claim_status_sha256"], claim_status_sha);
    assert_eq!(json["continuation_status_sha256"], continuation_status_sha);
    assert_eq!(
        json["expected_continuation_status_sha256"],
        continuation_status_sha
    );
    assert_eq!(json["classification"]["size"], "bounded");
    assert_eq!(json["classification"]["shape"], "bug-fix");
    assert_eq!(
        json["post_continuation_action"]["required_prior_status"],
        "ready"
    );
    assert_eq!(
        json["post_continuation_action"]["required_continuation_status_sha256"],
        continuation_status_sha
    );
    assert_eq!(
        json["post_continuation_action"]["next_bounded_action"]["action"],
        "resume_governed_project_start_after_continuation_status"
    );
    assert_eq!(
        json["post_continuation_action"]["next_bounded_action"]
            ["requires_exact_continuation_status_sha256"],
        true
    );
    assert_eq!(
        json["post_continuation_action"]["current_contract_is_read_only"],
        true
    );
    assert_eq!(json["blockers"].as_array().unwrap().len(), 0);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "submit_exact_continuation_status_digest_to_ao2_post_continuation_executor"
    );
    assert_eq!(
        json["hermes_memory"]["raw_memory_jsonl_scrape_required"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["raw_queue_json_scrape_required"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_write_queue_file"], false);
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

    let route = format!(
        "/api/factory/project-start/recovery/resume-post-continuation/action?token=viewer-token&queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&plan_sha256={plan_sha}&claim_status_sha256={claim_status_sha}&continuation_status_sha256={continuation_status_sha}"
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
        "ao2.factory-project-start-recovery-resume-post-continuation-action.v1"
    );
    assert_eq!(api["status"], "ready");
    assert_eq!(
        api["continuation_status_sha256"],
        json["continuation_status_sha256"]
    );
    assert_eq!(
        api["post_continuation_action"]["next_bounded_action"]["action"],
        json["post_continuation_action"]["next_bounded_action"]["action"]
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

    let blocked = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-action",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--continuation-status-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(blocked.status.success(), "{}", stderr(&blocked));
    let blocked_json: serde_json::Value = serde_json::from_str(&stdout(&blocked)).unwrap();
    assert_eq!(blocked_json["status"], "blocked");
    assert_eq!(
        blocked_json["blockers"][0]["code"],
        "continuation_status_digest_mismatch"
    );
    assert_eq!(
        blocked_json["post_continuation_action"]["current_contract_is_read_only"],
        true
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_eq!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_eq!(memory_links_sha_before, sha256_path(&memory_links_path));
}

#[test]
fn cli_workbench_project_start_recovery_resume_post_continuation_execute_requires_exact_continuation_status_digest(
) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp
        .path()
        .join("recovery-resume-post-continuation-execute-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_path = repo.join(".ao2").join("factory-compat").join("queue.json");
    let memory_records_path = repo.join(".ao2").join("memory").join("records.jsonl");
    let memory_links_path = repo.join(".ao2").join("memory").join("run-links.jsonl");

    let run_id = "workbench-project-start-recovery-post-continuation-execute";
    let project_spec = temp.path().join(format!("{run_id}.md"));
    fs::write(
        &project_spec,
        format!(
            r#"# Recovery Post-Continuation Execute Project

## App Steps

- Build a governed workflow fixture for {run_id}.
- Let AO2 execute the exact-digest post-continuation recovery action.
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
        "project-start-recovery-post-continuation-execute-test",
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
        run_id,
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
        run_id,
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
        .unwrap();
    let recovery_packet_sha = action_json["exact_digest_requirements"]
        ["resume_from_latest_recovery_packet"]["recovery_packet_sha256"]
        .as_str()
        .unwrap();

    let checkpoint_approval = ao2([
        "factory",
        "queue-project-start-recovery-resume-checkpoint",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_sha = plan_json["plan_sha256"].as_str().unwrap();

    let claim = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--approve-plan-sha256",
        plan_sha,
        "--json",
    ]);
    assert!(claim.status.success(), "{}", stderr(&claim));

    let claim_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-claim-status",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
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
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
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

    let approval_required = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execute",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--continuation-status-sha256",
        &continuation_status_sha,
        "--json",
    ]);
    assert!(
        approval_required.status.success(),
        "{}",
        stderr(&approval_required)
    );
    let approval_json: serde_json::Value =
        serde_json::from_str(&stdout(&approval_required)).unwrap();
    assert_eq!(
        approval_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-execute-approval.v1"
    );
    assert_eq!(approval_json["status"], "approval_required");
    assert_eq!(
        approval_json["required_flag"],
        "--approve-continuation-status-sha256"
    );
    assert_eq!(
        approval_json["continuation_status_sha256"],
        continuation_status_sha
    );

    let queue_sha_before = sha256_path(&queue_path);
    let memory_records_sha_before = sha256_path(&memory_records_path);
    let memory_links_sha_before = sha256_path(&memory_links_path);

    let executed = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execute",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--continuation-status-sha256",
        &continuation_status_sha,
        "--approve-continuation-status-sha256",
        &continuation_status_sha,
        "--json",
    ]);
    assert!(executed.status.success(), "{}", stderr(&executed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&executed)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-execute.v1"
    );
    assert_eq!(json["status"], "executed");
    assert_eq!(json["run_id"], run_id);
    assert_eq!(
        json["approved_continuation_status_sha256"],
        continuation_status_sha
    );
    assert_eq!(
        json["post_continuation_memory_record"]["source"]["path_sha256"],
        continuation_status_sha
    );
    assert_eq!(
        json["post_continuation_memory_link"]["relationship"],
        "project-start-recovery-resume-post-continuation-execute"
    );
    assert_eq!(json["side_effects"]["wrote_memory_record"], true);
    assert_eq!(json["side_effects"]["wrote_memory_run_link"], true);
    assert_eq!(json["side_effects"]["executed_provider"], false);
    assert_eq!(json["side_effects"]["executed_queue"], false);
    assert_eq!(json["side_effects"]["submitted_queue_entry"], false);
    assert_eq!(json["side_effects"]["mutated_control_plane"], false);
    assert_eq!(json["side_effects"]["approved_release"], false);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "read_recovery_resume_post_continuation_execution_evidence"
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
    assert_eq!(queue_sha_before, sha256_path(&queue_path));
    assert_ne!(memory_records_sha_before, sha256_path(&memory_records_path));
    assert_ne!(memory_links_sha_before, sha256_path(&memory_links_path));

    let duplicate = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execute",
        "--target",
        repo.to_str().unwrap(),
        "--queue-sha256",
        queue_sha,
        "--recovery-packet-sha256",
        recovery_packet_sha,
        "--plan-sha256",
        plan_sha,
        "--claim-status-sha256",
        &claim_status_sha,
        "--continuation-status-sha256",
        &continuation_status_sha,
        "--approve-continuation-status-sha256",
        &continuation_status_sha,
        "--json",
    ]);
    assert!(duplicate.status.success(), "{}", stderr(&duplicate));
    let duplicate_json: serde_json::Value = serde_json::from_str(&stdout(&duplicate)).unwrap();
    assert_eq!(duplicate_json["status"], "blocked");
    assert_eq!(
        duplicate_json["blockers"][0]["code"],
        "duplicate_recovery_resume_post_continuation_execution_records"
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
    let body = format!(
        "queue_sha256={queue_sha}&recovery_packet_sha256={recovery_packet_sha}&plan_sha256={plan_sha}&claim_status_sha256={claim_status_sha}&continuation_status_sha256={continuation_status_sha}&approval_continuation_status_sha256={continuation_status_sha}"
    );
    let response = http_request(
        port,
        &format!(
            "POST /api/factory/project-start/recovery/resume-post-continuation/execute?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "{response}"
    );
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "blocked");
    assert_eq!(
        api["blockers"][0]["code"],
        "duplicate_recovery_resume_post_continuation_execution_records"
    );
    assert_eq!(queue_sha_before, sha256_path(&queue_path));

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

#[test]
fn cli_workbench_project_start_recovery_resume_post_continuation_execution_status_replays_c66_record(
) {
    let fixture =
        create_post_continuation_execution_status_fixture("post-continuation-execution-status");
    let queue_sha_before = sha256_path(&fixture.queue_path);
    let memory_records_sha_before = sha256_path(&fixture.memory_records_path);
    let memory_links_sha_before = sha256_path(&fixture.memory_links_path);

    let status = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execution-status",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-execution-status.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], fixture.run_id);
    assert_eq!(
        json["approved_continuation_status_sha256"],
        fixture.continuation_status_sha
    );
    assert_eq!(json["post_continuation_execution_record_count"], 1);
    assert_eq!(json["post_continuation_execution_run_link_count"], 1);
    assert_eq!(
        json["post_continuation_execution"]["record_source_binds_continuation_status_sha256"],
        true
    );
    assert_eq!(
        json["post_continuation_execution"]["body_binds_claim_status_sha256"],
        true
    );
    assert_eq!(
        json["post_continuation_execution"]["body_binds_plan_sha256"],
        true
    );
    assert_eq!(
        json["post_continuation_execution"]["body_binds_queue_sha256"],
        true
    );
    assert_eq!(
        json["post_continuation_execution"]["body_binds_recovery_packet_sha256"],
        true
    );
    assert_eq!(
        json["post_continuation_execution"]["body_binds_continuation_memory_record_id"],
        true
    );
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "observe_recovery_resume_post_continuation_execution_status_then_continue_governed_recovery"
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/execution-status?token=viewer-token&queue_sha256={}&recovery_packet_sha256={}&plan_sha256={}&claim_status_sha256={}&continuation_status_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            fixture.queue_sha,
            fixture.recovery_packet_sha,
            fixture.plan_sha,
            fixture.claim_status_sha,
            fixture.continuation_status_sha
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "ready");
    assert_eq!(
        api["post_continuation_execution"]["memory_record_id"],
        json["post_continuation_execution"]["memory_record_id"]
    );
    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

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
fn cli_workbench_project_start_recovery_resume_post_continuation_next_action_binds_execution_status(
) {
    let fixture =
        create_post_continuation_execution_status_fixture("post-continuation-next-action");
    let queue_sha_before = sha256_path(&fixture.queue_path);
    let memory_records_sha_before = sha256_path(&fixture.memory_records_path);
    let memory_links_sha_before = sha256_path(&fixture.memory_links_path);

    let execution_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execution-status",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--json",
    ]);
    assert!(
        execution_status.status.success(),
        "{}",
        stderr(&execution_status)
    );
    let execution_status_json: serde_json::Value =
        serde_json::from_str(&stdout(&execution_status)).unwrap();
    let execution_status_sha = canonical_sha256_for_test(&execution_status_json);

    let next_action = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-next-action",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--json",
    ]);
    assert!(next_action.status.success(), "{}", stderr(&next_action));
    let json: serde_json::Value = serde_json::from_str(&stdout(&next_action)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-next-action.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], fixture.run_id);
    assert_eq!(
        json["post_continuation_execution_status_sha256"],
        execution_status_sha
    );
    assert_eq!(json["execution_status_digest_matches_current"], true);
    assert_eq!(json["classification"]["shape"], "bug-fix");
    assert_eq!(json["classification"]["size"], "bounded");
    assert_eq!(
        json["next_bounded_action"]["action"],
        "close_recovery_resume_post_continuation_after_operator_review"
    );
    assert_eq!(json["next_bounded_action"]["read_only"], true);
    assert_eq!(
        json["next_bounded_action"]["requires_exact_digest_approval"],
        false
    );
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "close_recovery_resume_post_continuation_or_route_next_governed_step"
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/next-action?token=viewer-token&queue_sha256={}&recovery_packet_sha256={}&plan_sha256={}&claim_status_sha256={}&continuation_status_sha256={}&post_continuation_execution_status_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            fixture.queue_sha,
            fixture.recovery_packet_sha,
            fixture.plan_sha,
            fixture.claim_status_sha,
            fixture.continuation_status_sha,
            execution_status_sha
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "ready");
    assert_eq!(
        api["expected_post_continuation_execution_status_sha256"],
        json["expected_post_continuation_execution_status_sha256"]
    );
    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

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
fn cli_workbench_project_start_recovery_resume_post_continuation_closure_binds_next_action() {
    let fixture = create_post_continuation_execution_status_fixture("post-continuation-closure");
    let queue_sha_before = sha256_path(&fixture.queue_path);
    let memory_records_sha_before = sha256_path(&fixture.memory_records_path);
    let memory_links_sha_before = sha256_path(&fixture.memory_links_path);

    let execution_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execution-status",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--json",
    ]);
    assert!(
        execution_status.status.success(),
        "{}",
        stderr(&execution_status)
    );
    let execution_status_json: serde_json::Value =
        serde_json::from_str(&stdout(&execution_status)).unwrap();
    let execution_status_sha = canonical_sha256_for_test(&execution_status_json);

    let next_action = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-next-action",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--json",
    ]);
    assert!(next_action.status.success(), "{}", stderr(&next_action));
    let next_action_json: serde_json::Value = serde_json::from_str(&stdout(&next_action)).unwrap();
    let next_action_sha = canonical_sha256_for_test(&next_action_json);

    let closure = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-closure",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--post-continuation-next-action-sha256",
        &next_action_sha,
        "--json",
    ]);
    assert!(closure.status.success(), "{}", stderr(&closure));
    let json: serde_json::Value = serde_json::from_str(&stdout(&closure)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-closure.v1"
    );
    assert_eq!(json["status"], "ready");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], fixture.run_id);
    assert_eq!(json["closure_ready"], true);
    assert_eq!(
        json["post_continuation_next_action_sha256"],
        next_action_sha
    );
    assert_eq!(
        json["expected_post_continuation_next_action_sha256"],
        next_action_sha
    );
    assert_eq!(json["next_action_digest_matches_current"], true);
    assert_eq!(
        json["handoff"]["handoff_packet"],
        "recovery_resume_post_continuation_closure"
    );
    assert_eq!(
        json["handoff"]["consumer"],
        "Hermes and factory-v3 evaluator-closer"
    );
    assert_eq!(json["handoff"]["raw_jsonl_scrape_required"], false);
    assert_eq!(
        json["hermes_memory"]["next_recommended_action"],
        "send_recovery_resume_closure_packet_to_evaluator_closer"
    );
    assert_eq!(
        json["evidence_chain"]["post_continuation_next_action"]["sha256"],
        next_action_sha
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_write_queue_file"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/closure?token=viewer-token&queue_sha256={}&recovery_packet_sha256={}&plan_sha256={}&claim_status_sha256={}&continuation_status_sha256={}&post_continuation_execution_status_sha256={}&post_continuation_next_action_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            fixture.queue_sha,
            fixture.recovery_packet_sha,
            fixture.plan_sha,
            fixture.claim_status_sha,
            fixture.continuation_status_sha,
            execution_status_sha,
            next_action_sha
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "ready");
    assert_eq!(
        api["expected_post_continuation_next_action_sha256"],
        json["expected_post_continuation_next_action_sha256"]
    );
    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

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
fn cli_workbench_project_start_recovery_resume_post_continuation_evaluator_decision_is_signed() {
    let fixture =
        create_post_continuation_execution_status_fixture("post-continuation-evaluator-decision");
    let signing_key = fixture
        .repo
        .join("recovery-resume-post-continuation-evaluator-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_sha_before = sha256_path(&fixture.queue_path);
    let memory_records_sha_before = sha256_path(&fixture.memory_records_path);
    let memory_links_sha_before = sha256_path(&fixture.memory_links_path);

    let execution_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execution-status",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--json",
    ]);
    assert!(
        execution_status.status.success(),
        "{}",
        stderr(&execution_status)
    );
    let execution_status_json: serde_json::Value =
        serde_json::from_str(&stdout(&execution_status)).unwrap();
    let execution_status_sha = canonical_sha256_for_test(&execution_status_json);

    let next_action = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-next-action",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--json",
    ]);
    assert!(next_action.status.success(), "{}", stderr(&next_action));
    let next_action_json: serde_json::Value = serde_json::from_str(&stdout(&next_action)).unwrap();
    let next_action_sha = canonical_sha256_for_test(&next_action_json);

    let closure = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-closure",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--post-continuation-next-action-sha256",
        &next_action_sha,
        "--json",
    ]);
    assert!(closure.status.success(), "{}", stderr(&closure));
    let closure_json: serde_json::Value = serde_json::from_str(&stdout(&closure)).unwrap();
    let closure_sha = canonical_sha256_for_test(&closure_json);

    let decision = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-evaluator-decision",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--post-continuation-next-action-sha256",
        &next_action_sha,
        "--closure-sha256",
        &closure_sha,
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "ao2-c70-test",
        "--json",
    ]);
    assert!(decision.status.success(), "{}", stderr(&decision));
    let json: serde_json::Value = serde_json::from_str(&stdout(&decision)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-evaluator-decision.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["read_only"], true);
    assert_eq!(json["run_id"], fixture.run_id);
    assert_eq!(json["closure_sha256"], closure_sha);
    assert_eq!(json["expected_closure_sha256"], closure_sha);
    assert_eq!(json["closure_digest_matches_current"], true);
    assert_eq!(
        json["decision"]["verdict"],
        "accept_recovery_closure_evidence"
    );
    assert_eq!(
        json["decision"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["factory_v3_parity_oracle"]["expectations_satisfied"],
        true
    );
    assert_eq!(
        json["signature"]["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-evaluator-decision-signature.v1"
    );
    assert_eq!(json["signature"]["signature_verified"], true);
    assert_eq!(json["signature"]["signer_id"], "ao2-c70-test");
    assert!(Path::new(json["decision_path"].as_str().unwrap()).is_file());
    assert!(Path::new(json["signature"]["signed_payload_path"].as_str().unwrap()).is_file());
    assert!(Path::new(json["signature"]["signature_path"].as_str().unwrap()).is_file());
    assert!(Path::new(json["signature"]["public_key_path"].as_str().unwrap()).is_file());
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_write_memory_run_link"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_submit_queue_entry"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_write_queue_file"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "operator-token",
            "--operator-token",
            "viewer:viewer:viewer-token",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "ao2-c70-workbench-test",
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
            "GET /api/factory/project-start/recovery/resume-post-continuation/evaluator-decision?token=viewer-token&queue_sha256={}&recovery_packet_sha256={}&plan_sha256={}&claim_status_sha256={}&continuation_status_sha256={}&post_continuation_execution_status_sha256={}&post_continuation_next_action_sha256={}&closure_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            fixture.queue_sha,
            fixture.recovery_packet_sha,
            fixture.plan_sha,
            fixture.claim_status_sha,
            fixture.continuation_status_sha,
            execution_status_sha,
            next_action_sha,
            closure_sha
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "accepted");
    assert_eq!(api["signature"]["signer_id"], "ao2-c70-workbench-test");
    assert_eq!(
        api["factory_v3_parity_oracle"]["expectations_satisfied"],
        true
    );

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
fn cli_workbench_project_start_recovery_resume_post_continuation_release_handoff_bundles_decision()
{
    let fixture =
        create_post_continuation_execution_status_fixture("post-continuation-release-handoff");
    let signing_key = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_sha_before = sha256_path(&fixture.queue_path);
    let memory_records_sha_before = sha256_path(&fixture.memory_records_path);
    let memory_links_sha_before = sha256_path(&fixture.memory_links_path);

    let execution_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execution-status",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--json",
    ]);
    assert!(
        execution_status.status.success(),
        "{}",
        stderr(&execution_status)
    );
    let execution_status_json: serde_json::Value =
        serde_json::from_str(&stdout(&execution_status)).unwrap();
    let execution_status_sha = canonical_sha256_for_test(&execution_status_json);

    let next_action = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-next-action",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--json",
    ]);
    assert!(next_action.status.success(), "{}", stderr(&next_action));
    let next_action_json: serde_json::Value = serde_json::from_str(&stdout(&next_action)).unwrap();
    let next_action_sha = canonical_sha256_for_test(&next_action_json);

    let closure = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-closure",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--post-continuation-next-action-sha256",
        &next_action_sha,
        "--json",
    ]);
    assert!(closure.status.success(), "{}", stderr(&closure));
    let closure_json: serde_json::Value = serde_json::from_str(&stdout(&closure)).unwrap();
    let closure_sha = canonical_sha256_for_test(&closure_json);

    let decision = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-evaluator-decision",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--post-continuation-next-action-sha256",
        &next_action_sha,
        "--closure-sha256",
        &closure_sha,
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "ao2-c71-test",
        "--json",
    ]);
    assert!(decision.status.success(), "{}", stderr(&decision));
    let decision_json: serde_json::Value = serde_json::from_str(&stdout(&decision)).unwrap();
    let decision_path = Path::new(decision_json["decision_path"].as_str().unwrap());
    let signed_payload_path = Path::new(
        decision_json["signature"]["signed_payload_path"]
            .as_str()
            .unwrap(),
    );
    let signature_path = Path::new(
        decision_json["signature"]["signature_path"]
            .as_str()
            .unwrap(),
    );
    let public_key_path = Path::new(
        decision_json["signature"]["public_key_path"]
            .as_str()
            .unwrap(),
    );
    let decision_sha = sha256_path(decision_path);
    let out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff.tgz");

    let handoff = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-handoff",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--decision",
        decision_path.to_str().unwrap(),
        "--signed-payload",
        signed_payload_path.to_str().unwrap(),
        "--signature",
        signature_path.to_str().unwrap(),
        "--public-key",
        public_key_path.to_str().unwrap(),
        "--closure-sha256",
        &closure_sha,
        "--decision-sha256",
        &decision_sha,
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(handoff.status.success(), "{}", stderr(&handoff));
    let json: serde_json::Value = serde_json::from_str(&stdout(&handoff)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff.v1"
    );
    assert_eq!(json["status"], "bundled");
    assert_eq!(json["signature_verified"], true);
    assert_eq!(json["closure_sha256"], closure_sha);
    assert_eq!(json["decision_sha256"], decision_sha);
    assert_eq!(json["decision_digest_matches_current"], true);
    assert_eq!(json["closure_digest_matches_decision"], true);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    let archive = Path::new(json["archive"].as_str().unwrap());
    let entries = archive_entries(archive);
    for expected in [
        "manifest.json",
        "SHA256SUMS",
        "release-handoff.json",
        "artifacts/evaluator-decision/evaluator-decision.json",
        "artifacts/evaluator-decision/signed-payload.json",
        "artifacts/evaluator-decision/signature.sig",
        "artifacts/evaluator-decision/public.pem",
    ] {
        assert!(entries.iter().any(|entry| entry == expected), "{expected}");
    }
    let manifest_text = archive_text_entry(archive, "manifest.json");
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(
        manifest["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff.v1"
    );
    assert_eq!(manifest["signature_verified"], true);
    assert_eq!(
        manifest["factory_v3_parity_oracle"]["ready_for_comparison"],
        true
    );
    let checksums = archive_text_entry(archive, "SHA256SUMS");
    assert!(checksums.contains("release-handoff.json"));
    assert!(checksums.contains("artifacts/evaluator-decision/evaluator-decision.json"));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
    let api_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff-api.tgz");
    let response = http_request(
        port,
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-handoff?token=viewer-token&decision={}&signed_payload={}&signature={}&public_key={}&closure_sha256={}&decision_sha256={}&out={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            decision_path.display(),
            signed_payload_path.display(),
            signature_path.display(),
            public_key_path.display(),
            closure_sha,
            decision_sha,
            api_out.display()
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "bundled");
    assert_eq!(api["signature_verified"], true);
    assert_eq!(api["decision_sha256"], decision_sha);
    assert!(Path::new(api["archive"].as_str().unwrap()).is_file());

    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

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
fn cli_workbench_project_start_recovery_resume_post_continuation_release_handoff_status_verifies_bundle_read_only(
) {
    let fixture =
        create_post_continuation_execution_status_fixture("post-continuation-release-status");
    let signing_key = fixture
        .repo
        .join("recovery-resume-post-continuation-release-status-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let queue_sha_before = sha256_path(&fixture.queue_path);
    let memory_records_sha_before = sha256_path(&fixture.memory_records_path);
    let memory_links_sha_before = sha256_path(&fixture.memory_links_path);

    let execution_status = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-execution-status",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--json",
    ]);
    assert!(
        execution_status.status.success(),
        "{}",
        stderr(&execution_status)
    );
    let execution_status_json: serde_json::Value =
        serde_json::from_str(&stdout(&execution_status)).unwrap();
    let execution_status_sha = canonical_sha256_for_test(&execution_status_json);

    let next_action = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-next-action",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--json",
    ]);
    assert!(next_action.status.success(), "{}", stderr(&next_action));
    let next_action_json: serde_json::Value = serde_json::from_str(&stdout(&next_action)).unwrap();
    let next_action_sha = canonical_sha256_for_test(&next_action_json);

    let closure = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-closure",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--post-continuation-next-action-sha256",
        &next_action_sha,
        "--json",
    ]);
    assert!(closure.status.success(), "{}", stderr(&closure));
    let closure_json: serde_json::Value = serde_json::from_str(&stdout(&closure)).unwrap();
    let closure_sha = canonical_sha256_for_test(&closure_json);

    let decision = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-evaluator-decision",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--queue-sha256",
        &fixture.queue_sha,
        "--recovery-packet-sha256",
        &fixture.recovery_packet_sha,
        "--plan-sha256",
        &fixture.plan_sha,
        "--claim-status-sha256",
        &fixture.claim_status_sha,
        "--continuation-status-sha256",
        &fixture.continuation_status_sha,
        "--post-continuation-execution-status-sha256",
        &execution_status_sha,
        "--post-continuation-next-action-sha256",
        &next_action_sha,
        "--closure-sha256",
        &closure_sha,
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "ao2-c72-test",
        "--json",
    ]);
    assert!(decision.status.success(), "{}", stderr(&decision));
    let decision_json: serde_json::Value = serde_json::from_str(&stdout(&decision)).unwrap();
    let decision_path = Path::new(decision_json["decision_path"].as_str().unwrap());
    let signed_payload_path = Path::new(
        decision_json["signature"]["signed_payload_path"]
            .as_str()
            .unwrap(),
    );
    let signature_path = Path::new(
        decision_json["signature"]["signature_path"]
            .as_str()
            .unwrap(),
    );
    let public_key_path = Path::new(
        decision_json["signature"]["public_key_path"]
            .as_str()
            .unwrap(),
    );
    let decision_sha = sha256_path(decision_path);
    let bundle = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff.tgz");

    let handoff = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-handoff",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--decision",
        decision_path.to_str().unwrap(),
        "--signed-payload",
        signed_payload_path.to_str().unwrap(),
        "--signature",
        signature_path.to_str().unwrap(),
        "--public-key",
        public_key_path.to_str().unwrap(),
        "--closure-sha256",
        &closure_sha,
        "--decision-sha256",
        &decision_sha,
        "--out",
        bundle.to_str().unwrap(),
        "--json",
    ]);
    assert!(handoff.status.success(), "{}", stderr(&handoff));

    let status = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-handoff-status",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--bundle",
        bundle.to_str().unwrap(),
        "--closure-sha256",
        &closure_sha,
        "--decision-sha256",
        &decision_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status.v1"
    );
    assert_eq!(json["status"], "verified", "{}", stdout(&status));
    assert_eq!(json["read_only"], true);
    assert_eq!(json["bundle_sha256"], sha256_path(&bundle));
    assert_eq!(json["closure_sha256"], closure_sha);
    assert_eq!(json["decision_sha256"], decision_sha);
    assert_eq!(json["checks"]["sha256sums_verified"], true);
    assert_eq!(json["checks"]["required_manifest_entries_present"], true);
    assert_eq!(json["checks"]["signature_verified"], true);
    assert_eq!(json["checks"]["closure_digest_chain_verified"], true);
    assert_eq!(json["checks"]["decision_digest_chain_verified"], true);
    assert_eq!(
        json["checks"]["factory_v3_parity_expectations_verified"],
        true
    );
    assert_eq!(
        json["factory_v3_parity_oracle"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["factory_v3_parity_oracle"]["ready_for_comparison"],
        true
    );
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(
        json["side_effects"]["would_write_release_handoff_bundle"],
        false
    );
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        json["hermes_status"]["next_recommended_action"],
        "factory_v3_evaluator_closer_compare_c72_verified_release_handoff_status_and_prepare_control_plane_observer_readback"
    );
    let status_path = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff-status.json");
    fs::write(&status_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    let status_sha = sha256_path(&status_path);
    let summary_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff-status-summary.json");

    let summary = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-handoff-status-summary",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--status",
        status_path.to_str().unwrap(),
        "--status-sha256",
        &status_sha,
        "--out",
        summary_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(summary.status.success(), "{}", stderr(&summary));
    let summary_json: serde_json::Value = serde_json::from_str(&stdout(&summary)).unwrap();
    assert_eq!(
        summary_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status-summary.v1"
    );
    assert_eq!(summary_json["status"], "recorded");
    assert_eq!(
        summary_json["status_packet"]["schema_version"],
        json["schema_version"]
    );
    assert_eq!(summary_json["status_sha256"], status_sha);
    assert_eq!(
        summary_json["summary_path"],
        summary_out.display().to_string()
    );
    assert_eq!(
        summary_json["hermes_bookkeeping"]["next_recommended_action"],
        "factory_v3_evaluator_closer_compare_c72_verified_release_handoff_status_and_prepare_control_plane_observer_readback"
    );
    assert_eq!(
        summary_json["hermes_bookkeeping"]["raw_archive_interpretation_required"],
        false
    );
    assert_eq!(
        summary_json["hermes_bookkeeping"]["raw_status_chain_recompute_required"],
        false
    );
    assert_eq!(
        summary_json["factory_v3_parity_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(summary_json["status_checks"]["signature_verified"], true);
    assert_eq!(
        summary_json["status_checks"]["decision_digest_chain_verified"],
        true
    );
    assert_eq!(
        summary_json["side_effects"]["wrote_bookkeeping_summary_artifact"],
        true
    );
    assert_eq!(summary_json["side_effects"]["would_write_memory"], false);
    assert_eq!(
        summary_json["side_effects"]["would_execute_provider"],
        false
    );
    assert_eq!(summary_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        summary_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(summary_json["side_effects"]["would_approve_release"], false);
    assert_eq!(
        summary_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert!(summary_out.is_file());
    let persisted_summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_out).unwrap()).unwrap();
    assert_eq!(
        persisted_summary["status_sha256"],
        summary_json["status_sha256"]
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-handoff-status?token=viewer-token&bundle={}&closure_sha256={}&decision_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&bundle.display().to_string()),
            closure_sha,
            decision_sha,
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api["status"], "verified");
    assert_eq!(api["checks"]["signature_verified"], true);
    assert_eq!(api["decision_sha256"], decision_sha);

    let api_summary_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff-status-summary-api.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
    let summary_response = http_request(
        port,
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-handoff-status-summary?token=viewer-token&status={}&status_sha256={}&out={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&status_path.display().to_string()),
            status_sha,
            percent_encode_for_test(&api_summary_out.display().to_string()),
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        summary_response.starts_with("HTTP/1.1 200 OK"),
        "{summary_response}"
    );
    let api_summary: serde_json::Value =
        serde_json::from_str(http_body(&summary_response)).unwrap();
    assert_eq!(api_summary["status"], "recorded");
    assert_eq!(api_summary["status_sha256"], status_sha);
    assert_eq!(
        api_summary["hermes_bookkeeping"]["next_recommended_action"],
        summary_json["hermes_bookkeeping"]["next_recommended_action"]
    );
    assert_eq!(
        api_summary["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );

    let summary_sha = sha256_path(&summary_out);
    let export_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff-status-summary-export.json");
    let export = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-handoff-status-summary-export",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--summary",
        summary_out.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha,
        "--out",
        export_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let export_json: serde_json::Value = serde_json::from_str(&stdout(&export)).unwrap();
    assert_eq!(
        export_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status-summary-export.v1"
    );
    assert_eq!(export_json["status"], "exported");
    assert_eq!(export_json["summary_sha256"], summary_sha);
    assert_eq!(export_json["status_sha256"], status_sha);
    assert_eq!(export_json["export_path"], export_out.display().to_string());
    assert_eq!(
        export_json["observer_fixture"]["schema_version"],
        "ao2.control-plane.recovery-release-handoff-status-summary-observer-fixture.v1"
    );
    assert_eq!(export_json["observer_fixture"]["producer"], "ao2");
    assert_eq!(
        export_json["observer_fixture"]["consumer"],
        "ao2-control-plane K37"
    );
    assert_eq!(
        export_json["observer_fixture"]["summary_sha256"],
        summary_sha
    );
    assert_eq!(export_json["observer_fixture"]["status_sha256"], status_sha);
    assert_eq!(export_json["publication_contract"]["digest_bound"], true);
    assert_eq!(
        export_json["publication_contract"]["control_plane_observer_fixture"],
        true
    );
    assert_eq!(
        export_json["publication_contract"]["control_plane_may_approve_release"],
        false
    );
    assert_eq!(
        export_json["publication_contract"]["control_plane_may_mutate_ao_artifacts"],
        false
    );
    assert_eq!(
        export_json["factory_v3_parity_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(export_json["status_checks"]["signature_verified"], true);
    assert_eq!(
        export_json["hermes_publication"]["next_recommended_action"],
        summary_json["hermes_bookkeeping"]["next_recommended_action"]
    );
    assert_eq!(
        export_json["side_effects"]["wrote_summary_export_artifact"],
        true
    );
    assert_eq!(export_json["side_effects"]["would_write_memory"], false);
    assert_eq!(export_json["side_effects"]["would_execute_provider"], false);
    assert_eq!(export_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        export_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(export_json["side_effects"]["would_approve_release"], false);
    assert!(export_out.is_file());
    let persisted_export: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&export_out).unwrap()).unwrap();
    assert_eq!(
        persisted_export["observer_fixture_sha256"],
        export_json["observer_fixture_sha256"]
    );

    let api_export_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-handoff-status-summary-export-api.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
    let export_response = http_request(
        port,
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-handoff-status-summary-export?token=viewer-token&summary={}&summary_sha256={}&out={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&summary_out.display().to_string()),
            summary_sha,
            percent_encode_for_test(&api_export_out.display().to_string()),
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        export_response.starts_with("HTTP/1.1 200 OK"),
        "{export_response}"
    );
    let api_export: serde_json::Value = serde_json::from_str(http_body(&export_response)).unwrap();
    assert_eq!(api_export["status"], "exported");
    assert_eq!(api_export["summary_sha256"], summary_sha);
    assert_eq!(
        api_export["observer_fixture"]["consumer"],
        "ao2-control-plane K37"
    );
    assert_eq!(
        api_export["publication_contract"]["control_plane_may_approve_release"],
        false
    );

    let export_sha = sha256_path(&export_out);
    let readiness = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-publication-readiness",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--export",
        export_out.to_str().unwrap(),
        "--export-sha256",
        &export_sha,
        "--json",
    ]);
    assert!(readiness.status.success(), "{}", stderr(&readiness));
    let readiness_json: serde_json::Value = serde_json::from_str(&stdout(&readiness)).unwrap();
    assert_eq!(
        readiness_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-readiness.v1"
    );
    assert_eq!(readiness_json["status"], "ready");
    assert_eq!(readiness_json["export_sha256"], export_sha);
    assert_eq!(readiness_json["summary_sha256"], summary_sha);
    assert_eq!(readiness_json["status_sha256"], status_sha);
    assert_eq!(
        readiness_json["observer_fixture_sha256"],
        export_json["observer_fixture_sha256"]
    );
    assert_eq!(
        readiness_json["checks"]["exact_export_digest_verified"],
        true
    );
    assert_eq!(
        readiness_json["checks"]["observer_fixture_digest_verified"],
        true
    );
    assert_eq!(
        readiness_json["hermes_publication"]["memory_bookkeeping_ready"],
        true
    );
    assert_eq!(
        readiness_json["hermes_publication"]["control_plane_bookkeeping_ready"],
        true
    );
    assert_eq!(
        readiness_json["publication_contract"]["control_plane_may_approve_release"],
        false
    );
    assert_eq!(
        readiness_json["publication_contract"]["control_plane_may_mutate_ao_artifacts"],
        false
    );
    assert_eq!(
        readiness_json["factory_v3_parity_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(readiness_json["side_effects"]["would_write_memory"], false);
    assert_eq!(
        readiness_json["side_effects"]["would_execute_provider"],
        false
    );
    assert_eq!(readiness_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        readiness_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(
        readiness_json["side_effects"]["would_approve_release"],
        false
    );
    let readiness_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-publication-readiness.json");
    fs::write(&readiness_out, stdout(&readiness)).unwrap();
    let readiness_sha = sha256_path(&readiness_out);

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
    let readiness_response = http_request(
        port,
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-publication-readiness?token=viewer-token&export={}&export_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&export_out.display().to_string()),
            export_sha,
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        readiness_response.starts_with("HTTP/1.1 200 OK"),
        "{readiness_response}"
    );
    let api_readiness: serde_json::Value =
        serde_json::from_str(http_body(&readiness_response)).unwrap();
    assert_eq!(api_readiness["status"], "ready");
    assert_eq!(api_readiness["export_sha256"], export_sha);
    assert_eq!(
        api_readiness["checks"]["observer_fixture_digest_verified"],
        true
    );

    let dispatch = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-publication-dispatch-plan",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--readiness",
        readiness_out.to_str().unwrap(),
        "--readiness-sha256",
        &readiness_sha,
        "--json",
    ]);
    assert!(dispatch.status.success(), "{}", stderr(&dispatch));
    let dispatch_json: serde_json::Value = serde_json::from_str(&stdout(&dispatch)).unwrap();
    assert_eq!(
        dispatch_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-dispatch-plan.v1"
    );
    assert_eq!(dispatch_json["status"], "planned");
    assert_eq!(dispatch_json["readiness_sha256"], readiness_sha);
    assert_eq!(dispatch_json["export_sha256"], export_sha);
    assert_eq!(dispatch_json["summary_sha256"], summary_sha);
    assert_eq!(dispatch_json["status_sha256"], status_sha);
    assert_eq!(
        dispatch_json["dispatch_plan"]["hermes_memory_bookkeeping"]["mode"],
        "planned_only"
    );
    assert_eq!(
        dispatch_json["dispatch_plan"]["control_plane_readback"]["mode"],
        "planned_only"
    );
    assert_eq!(
        dispatch_json["dispatch_plan"]["control_plane_readback"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        dispatch_json["checks"]["exact_readiness_digest_verified"],
        true
    );
    assert_eq!(dispatch_json["checks"]["readiness_packet_ready"], true);
    assert_eq!(dispatch_json["side_effects"]["would_write_memory"], false);
    assert_eq!(
        dispatch_json["side_effects"]["would_execute_provider"],
        false
    );
    assert_eq!(dispatch_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        dispatch_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(
        dispatch_json["side_effects"]["would_approve_release"],
        false
    );
    let dispatch_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-publication-dispatch-plan.json");
    fs::write(&dispatch_out, stdout(&dispatch)).unwrap();
    let dispatch_sha = sha256_path(&dispatch_out);

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
    let dispatch_response = http_request(
        port,
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-publication-dispatch-plan?token=viewer-token&readiness={}&readiness_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&readiness_out.display().to_string()),
            readiness_sha,
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        dispatch_response.starts_with("HTTP/1.1 200 OK"),
        "{dispatch_response}"
    );
    let api_dispatch: serde_json::Value =
        serde_json::from_str(http_body(&dispatch_response)).unwrap();
    assert_eq!(api_dispatch["status"], "planned");
    assert_eq!(api_dispatch["readiness_sha256"], readiness_sha);
    assert_eq!(
        api_dispatch["dispatch_plan"]["hermes_memory_bookkeeping"]["mode"],
        "planned_only"
    );

    let observation_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-publication-observation.json");
    let observation = serde_json::json!({
        "schema_version": "ao2.hermes-recovery-publication-observation.v1",
        "status": "observed",
        "dispatch_plan_sha256": dispatch_sha,
        "readiness_sha256": readiness_sha,
        "export_sha256": export_sha,
        "observer_fixture_sha256": dispatch_json["observer_fixture_sha256"],
        "hermes_memory_bookkeeping": {
            "published": true,
            "mode": "external_observation",
            "source_dispatch_plan_sha256": dispatch_sha,
            "would_write_memory_from_readback": false
        },
        "control_plane_readback": {
            "observed": true,
            "mode": "external_observation",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "source_dispatch_plan_sha256": dispatch_sha,
            "would_mutate_control_plane_from_readback": false,
            "would_approve_release_from_readback": false
        },
        "trust_boundary": {
            "decision_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        },
        "concerns": [],
        "blockers": []
    });
    fs::write(
        &observation_out,
        serde_json::to_string_pretty(&observation).unwrap(),
    )
    .unwrap();
    let observation_sha = sha256_path(&observation_out);
    let readback = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-publication-readback",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--dispatch-plan",
        dispatch_out.to_str().unwrap(),
        "--dispatch-plan-sha256",
        &dispatch_sha,
        "--observation",
        observation_out.to_str().unwrap(),
        "--observation-sha256",
        &observation_sha,
        "--json",
    ]);
    assert!(readback.status.success(), "{}", stderr(&readback));
    let readback_json: serde_json::Value = serde_json::from_str(&stdout(&readback)).unwrap();
    assert_eq!(
        readback_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-readback.v1"
    );
    assert_eq!(readback_json["status"], "verified");
    assert_eq!(readback_json["dispatch_plan_sha256"], dispatch_sha);
    assert_eq!(readback_json["observation_sha256"], observation_sha);
    assert_eq!(readback_json["readiness_sha256"], readiness_sha);
    assert_eq!(readback_json["export_sha256"], export_sha);
    assert_eq!(readback_json["summary_sha256"], summary_sha);
    assert_eq!(readback_json["status_sha256"], status_sha);
    assert_eq!(
        readback_json["checks"]["exact_dispatch_plan_digest_verified"],
        true
    );
    assert_eq!(
        readback_json["checks"]["exact_observation_digest_verified"],
        true
    );
    assert_eq!(
        readback_json["checks"]["hermes_memory_bookkeeping_observed"],
        true
    );
    assert_eq!(
        readback_json["checks"]["control_plane_readback_observed"],
        true
    );
    assert_eq!(readback_json["side_effects"]["would_write_memory"], false);
    assert_eq!(
        readback_json["side_effects"]["would_execute_provider"],
        false
    );
    assert_eq!(readback_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        readback_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(
        readback_json["side_effects"]["would_approve_release"],
        false
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
    let readback_response = http_request(
        port,
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-publication-readback?token=viewer-token&dispatch_plan={}&dispatch_plan_sha256={}&observation={}&observation_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&dispatch_out.display().to_string()),
            dispatch_sha,
            percent_encode_for_test(&observation_out.display().to_string()),
            observation_sha,
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        readback_response.starts_with("HTTP/1.1 200 OK"),
        "{readback_response}"
    );
    let api_readback: serde_json::Value =
        serde_json::from_str(http_body(&readback_response)).unwrap();
    assert_eq!(api_readback["status"], "verified");
    assert_eq!(api_readback["dispatch_plan_sha256"], dispatch_sha);
    assert_eq!(api_readback["observation_sha256"], observation_sha);

    let readback_out = fixture
        .repo
        .join("recovery-resume-post-continuation-release-publication-readback.json");
    fs::write(
        &readback_out,
        serde_json::to_string_pretty(&readback_json).unwrap(),
    )
    .unwrap();
    let readback_sha = sha256_path(&readback_out);
    let closure = ao2([
        "factory",
        "queue-project-start-recovery-resume-post-continuation-release-publication-closure",
        "--target",
        fixture.repo.to_str().unwrap(),
        "--readback",
        readback_out.to_str().unwrap(),
        "--readback-sha256",
        &readback_sha,
        "--json",
    ]);
    assert!(closure.status.success(), "{}", stderr(&closure));
    let closure_json: serde_json::Value = serde_json::from_str(&stdout(&closure)).unwrap();
    assert_eq!(
        closure_json["schema_version"],
        "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-closure.v1"
    );
    assert_eq!(closure_json["status"], "closed");
    assert_eq!(closure_json["readback_sha256"], readback_sha);
    assert_eq!(closure_json["dispatch_plan_sha256"], dispatch_sha);
    assert_eq!(closure_json["observation_sha256"], observation_sha);
    assert_eq!(closure_json["readiness_sha256"], readiness_sha);
    assert_eq!(closure_json["export_sha256"], export_sha);
    assert_eq!(closure_json["summary_sha256"], summary_sha);
    assert_eq!(closure_json["status_sha256"], status_sha);
    assert_eq!(
        closure_json["checks"]["exact_readback_digest_verified"],
        true
    );
    assert_eq!(closure_json["checks"]["readback_verified"], true);
    assert_eq!(closure_json["checks"]["no_blockers"], true);
    assert_eq!(
        closure_json["scheduler_closure"]["operator_summary"],
        "recovery publication observed with no blockers"
    );
    assert_eq!(closure_json["side_effects"]["would_write_memory"], false);
    assert_eq!(
        closure_json["side_effects"]["would_execute_provider"],
        false
    );
    assert_eq!(closure_json["side_effects"]["would_execute_queue"], false);
    assert_eq!(
        closure_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(closure_json["side_effects"]["would_approve_release"], false);

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            fixture.repo.to_str().unwrap(),
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
    let closure_response = http_request(
        port,
        &format!(
            "GET /api/factory/project-start/recovery/resume-post-continuation/release-publication-closure?token=viewer-token&readback={}&readback_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&readback_out.display().to_string()),
            readback_sha,
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(
        closure_response.starts_with("HTTP/1.1 200 OK"),
        "{closure_response}"
    );
    let api_closure: serde_json::Value =
        serde_json::from_str(http_body(&closure_response)).unwrap();
    assert_eq!(api_closure["status"], "closed");
    assert_eq!(api_closure["readback_sha256"], readback_sha);

    assert_eq!(queue_sha_before, sha256_path(&fixture.queue_path));
    assert_eq!(
        memory_records_sha_before,
        sha256_path(&fixture.memory_records_path)
    );
    assert_eq!(
        memory_links_sha_before,
        sha256_path(&fixture.memory_links_path)
    );

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
        assert!(
            !summary_response.to_ascii_lowercase().contains(marker),
            "summary response must not contain {marker}"
        );
        assert!(
            !export_response.to_ascii_lowercase().contains(marker),
            "export response must not contain {marker}"
        );
        assert!(
            !readiness_response.to_ascii_lowercase().contains(marker),
            "readiness response must not contain {marker}"
        );
        assert!(
            !dispatch_response.to_ascii_lowercase().contains(marker),
            "dispatch response must not contain {marker}"
        );
        assert!(
            !readback_response.to_ascii_lowercase().contains(marker),
            "readback response must not contain {marker}"
        );
        assert!(
            !closure_response.to_ascii_lowercase().contains(marker),
            "closure response must not contain {marker}"
        );
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
