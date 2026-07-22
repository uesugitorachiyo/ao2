use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

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

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
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

#[test]
fn cli_plugin_wrapper_harness_runs_digest_pinned_app_run_with_redacted_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let readiness = temp.path().join("plugin-readiness.json");
    let readiness_result = ao2([
        "plugin",
        "readiness",
        "--out",
        readiness.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        readiness_result.status.success(),
        "{}",
        stderr(&readiness_result)
    );
    let readiness_sha256 = sha256_path(&readiness);

    let repo = temp.path().join("app-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let spec = temp.path().join("factory-app-discount.md");
    fs::write(
        &spec,
        r#"# Factory App Discount Service

Acceptance:
- The implementation rejects negative prices.
- The implementation rejects discount rates outside 0..1.
- The verifier can run with `python -m pytest -q`.
"#,
    )
    .unwrap();
    let prompt_path = temp.path().join("provider-prompt.sh");
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
printf 'Summary: plugin wrapper harness fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Authorization: Bearer should-redact\n'
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("factory-app-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let app_out_dir = temp.path().join("factory-app-out");
    let args_file = temp.path().join("wrapper-args.json");
    fs::write(
        &args_file,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.plugin-wrapper-args.v1",
            "run_kind": "app-run",
            "args": [
                "factory",
                "app-run",
                "--spec",
                spec.display().to_string(),
                "--target",
                repo.display().to_string(),
                "--run-id",
                "plugin-wrapper-app-run",
                "--verifier-command",
                "python -m pytest -q",
                "--provider",
                "scripted",
                "--provider-prompt-file",
                prompt_path.display().to_string(),
                "--signing-key",
                signing_key.display().to_string(),
                "--signer-id",
                "plugin-wrapper-harness-test",
                "--out-dir",
                app_out_dir.display().to_string(),
                "--json"
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let args_sha256 = sha256_path(&args_file);
    let harness_out = temp.path().join("plugin-wrapper-harness");

    let harness = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        readiness.to_str().unwrap(),
        "--readiness-sha256",
        &readiness_sha256,
        "--args-file",
        args_file.to_str().unwrap(),
        "--args-sha256",
        &args_sha256,
        "--run-kind",
        "app-run",
        "--out-dir",
        harness_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(harness.status.success(), "{}", stderr(&harness));

    let json: serde_json::Value = serde_json::from_str(&stdout(&harness)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-wrapper-harness.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["readiness_sha256"], readiness_sha256);
    assert_eq!(json["args_sha256"], args_sha256);
    assert_eq!(json["run_kind"], "app-run");
    assert_eq!(json["child_exit_code"], 0);
    assert_eq!(json["exit_code_contract"]["success"], 0);
    assert_eq!(json["exit_code_contract"]["runtime_error"], 1);
    assert_eq!(json["exit_code_contract"]["cli_usage"], 2);
    assert_eq!(json["digest_gates"]["readiness_sha256_verified"], true);
    assert_eq!(json["digest_gates"]["args_sha256_verified"], true);
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["factory_v3_role"], "parity_auditor");
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
    );
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert!(Path::new(json["evidence"]["summary"].as_str().unwrap()).is_file());
    assert!(Path::new(json["evidence"]["stdout_redacted"].as_str().unwrap()).is_file());
    assert!(Path::new(json["evidence"]["stderr_redacted"].as_str().unwrap()).is_file());
    assert!(Path::new(json["ao2_artifacts"]["factory_app_run"].as_str().unwrap()).is_file());
    assert!(Path::new(json["ao2_artifacts"]["evidence_pack"].as_str().unwrap()).is_file());

    let persisted_summary =
        fs::read_to_string(json["evidence"]["summary"].as_str().unwrap()).unwrap();
    let persisted_stdout =
        fs::read_to_string(json["evidence"]["stdout_redacted"].as_str().unwrap()).unwrap();
    for forbidden in [
        "Bearer should-redact",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !persisted_summary.contains(forbidden),
            "summary exposed forbidden marker {forbidden}"
        );
        assert!(
            !persisted_stdout.contains(forbidden),
            "stdout exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_wrapper_harness_verify_replays_project_run_evidence_by_digest() {
    let temp = tempfile::tempdir().unwrap();
    let readiness = temp.path().join("plugin-readiness.json");
    let readiness_result = ao2([
        "plugin",
        "readiness",
        "--out",
        readiness.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        readiness_result.status.success(),
        "{}",
        stderr(&readiness_result)
    );
    let readiness_sha256 = sha256_path(&readiness);

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

    let project_out_dir = temp.path().join("project-run");
    let args_file = temp.path().join("wrapper-project-args.json");
    fs::write(
        &args_file,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.plugin-wrapper-args.v1",
            "run_kind": "project-run",
            "args": [
                "factory",
                "project-run",
                "--project-spec",
                project_spec.display().to_string(),
                "--project-plan",
                project_plan.display().to_string(),
                "--run-id",
                "plugin-wrapper-project-run",
                "--signing-key",
                signing_key.display().to_string(),
                "--signer-id",
                "plugin-wrapper-project-test",
                "--out-dir",
                project_out_dir.display().to_string(),
                "--json"
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let args_sha256 = sha256_path(&args_file);
    let harness_out = temp.path().join("plugin-wrapper-harness-project");
    let harness = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        readiness.to_str().unwrap(),
        "--readiness-sha256",
        &readiness_sha256,
        "--args-file",
        args_file.to_str().unwrap(),
        "--args-sha256",
        &args_sha256,
        "--run-kind",
        "project-run",
        "--out-dir",
        harness_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(harness.status.success(), "{}", stderr(&harness));
    let harness_json: serde_json::Value = serde_json::from_str(&stdout(&harness)).unwrap();
    assert_eq!(
        harness_json["schema_version"],
        "ao2.plugin-wrapper-harness.v1"
    );
    assert_eq!(harness_json["status"], "accepted");
    assert_eq!(harness_json["run_kind"], "project-run");
    assert!(Path::new(
        harness_json["ao2_artifacts"]["factory_project_run"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(
        harness_json["ao2_artifacts"]["release_review_package"]
            .as_str()
            .unwrap()
    )
    .is_file());

    let summary_path = harness_out.join("plugin-wrapper-harness.json");
    let summary_sha256 = sha256_path(&summary_path);
    let verify = ao2([
        "plugin",
        "wrapper-harness-verify",
        "--evidence-dir",
        harness_out.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.plugin-wrapper-harness-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["run_kind"], "project-run");
    assert_eq!(verify_json["digest_gates_verified"], true);
    assert_eq!(verify_json["trust_boundary_verified"], true);
    assert_eq!(verify_json["token_safe_output_verified"], true);
    assert_eq!(
        verify_json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        verify_json["control_plane_observation"]["may_mutate_evidence"],
        false
    );
}

#[test]
fn cli_plugin_wrapper_harness_verify_allows_nullable_project_run_optional_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let evidence_dir = temp.path().join("plugin-wrapper-harness-project-apponly");
    fs::create_dir_all(&evidence_dir).unwrap();

    let stdout_path = evidence_dir.join("stdout.redacted.txt");
    let stderr_path = evidence_dir.join("stderr.redacted.txt");
    let project_spec = temp.path().join("project-spec.md");
    let factory_project_run = temp.path().join("factory-project-run.json");
    let factory_project_run_state = temp.path().join("factory-project-run-state.json");
    let release_review_package = temp.path().join("release-review-package.tgz");
    fs::write(&stdout_path, "{}\n").unwrap();
    fs::write(&stderr_path, "").unwrap();
    fs::write(&project_spec, "# App-only project\n").unwrap();
    fs::write(&factory_project_run, "{}\n").unwrap();
    fs::write(&factory_project_run_state, "{}\n").unwrap();
    fs::write(&release_review_package, "release review package\n").unwrap();

    let summary_path = evidence_dir.join("plugin-wrapper-harness.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.plugin-wrapper-harness.v1",
            "status": "accepted",
            "run_kind": "project-run",
            "readiness_path": "plugin-readiness.json",
            "readiness_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
            "args_file": "wrapper-project-args.json",
            "args_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
            "child_exit_code": 0,
            "exit_code_contract": {
                "success": 0,
                "runtime_error": 1,
                "cli_usage": 2,
                "enforced": true
            },
            "digest_gates": {
                "readiness_sha256_verified": true,
                "args_sha256_verified": true,
                "factory_command_digest_pinned_before_execution": true
            },
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "forbidden_provider_api_key_env_absent": true
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            },
            "token_safe_output": {
                "stdout_redacted": true,
                "stderr_redacted": true,
                "redaction_class_counts": {}
            },
            "evidence": {
                "bundle_path": evidence_dir.display().to_string(),
                "summary": summary_path.display().to_string(),
                "stdout_redacted": stdout_path.display().to_string(),
                "stderr_redacted": stderr_path.display().to_string()
            },
            "ao2_artifacts": {
                "acceptance_rubric": null,
                "acceptance_rubric_sha256": null,
                "factory_project_run": factory_project_run.display().to_string(),
                "factory_project_run_state": factory_project_run_state.display().to_string(),
                "project_plan": null,
                "project_spec": project_spec.display().to_string(),
                "release_review_package": release_review_package.display().to_string()
            },
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let summary_sha256 = sha256_path(&summary_path);

    let verify = ao2([
        "plugin",
        "wrapper-harness-verify",
        "--evidence-dir",
        evidence_dir.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
}

#[test]
fn cli_plugin_wrapper_harness_fails_closed_for_bad_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let readiness = temp.path().join("plugin-readiness.json");
    let readiness_result = ao2([
        "plugin",
        "readiness",
        "--out",
        readiness.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        readiness_result.status.success(),
        "{}",
        stderr(&readiness_result)
    );
    let readiness_sha256 = sha256_path(&readiness);
    let valid_args = temp.path().join("valid-wrapper-args.json");
    fs::write(
        &valid_args,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.plugin-wrapper-args.v1",
            "run_kind": "app-run",
            "args": ["factory", "app-run", "--json"]
        }))
        .unwrap(),
    )
    .unwrap();
    let valid_args_sha256 = sha256_path(&valid_args);
    let out_dir = temp.path().join("harness-negative");

    let readiness_mismatch = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        readiness.to_str().unwrap(),
        "--readiness-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--args-file",
        valid_args.to_str().unwrap(),
        "--args-sha256",
        &valid_args_sha256,
        "--run-kind",
        "app-run",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!readiness_mismatch.status.success());
    assert!(stderr(&readiness_mismatch).contains("readiness_sha256 mismatch"));

    let args_mismatch = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        readiness.to_str().unwrap(),
        "--readiness-sha256",
        &readiness_sha256,
        "--args-file",
        valid_args.to_str().unwrap(),
        "--args-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--run-kind",
        "app-run",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!args_mismatch.status.success());
    assert!(stderr(&args_mismatch).contains("args_sha256 mismatch"));

    let bad_schema = temp.path().join("bad-readiness-schema.json");
    let mut bad_schema_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&readiness).unwrap()).unwrap();
    bad_schema_json["schema_version"] = serde_json::json!("ao2.plugin-readiness.v0");
    fs::write(
        &bad_schema,
        serde_json::to_string_pretty(&bad_schema_json).unwrap(),
    )
    .unwrap();
    let bad_schema_result = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        bad_schema.to_str().unwrap(),
        "--readiness-sha256",
        &sha256_path(&bad_schema),
        "--args-file",
        valid_args.to_str().unwrap(),
        "--args-sha256",
        &valid_args_sha256,
        "--run-kind",
        "app-run",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_schema_result.status.success());
    assert!(stderr(&bad_schema_result).contains("ao2.plugin-readiness.v1"));

    let non_observer = temp.path().join("non-observer-readiness.json");
    let mut non_observer_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&readiness).unwrap()).unwrap();
    non_observer_json["trust_boundary"]["control_plane_role"] = serde_json::json!("approver");
    fs::write(
        &non_observer,
        serde_json::to_string_pretty(&non_observer_json).unwrap(),
    )
    .unwrap();
    let non_observer_result = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        non_observer.to_str().unwrap(),
        "--readiness-sha256",
        &sha256_path(&non_observer),
        "--args-file",
        valid_args.to_str().unwrap(),
        "--args-sha256",
        &valid_args_sha256,
        "--run-kind",
        "app-run",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!non_observer_result.status.success());
    assert!(stderr(&non_observer_result).contains("observer-only"));

    let unsupported_args = temp.path().join("unsupported-wrapper-args.json");
    fs::write(
        &unsupported_args,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.plugin-wrapper-args.v1",
            "run_kind": "app-run",
            "args": ["factory", "queue-submit", "--json"]
        }))
        .unwrap(),
    )
    .unwrap();
    let unsupported = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        readiness.to_str().unwrap(),
        "--readiness-sha256",
        &readiness_sha256,
        "--args-file",
        unsupported_args.to_str().unwrap(),
        "--args-sha256",
        &sha256_path(&unsupported_args),
        "--run-kind",
        "app-run",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!unsupported.status.success());
    assert!(stderr(&unsupported).contains("factory app-run"));

    let run_kind_mismatch_args = temp.path().join("run-kind-mismatch-args.json");
    fs::write(
        &run_kind_mismatch_args,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.plugin-wrapper-args.v1",
            "run_kind": "project-run",
            "args": ["factory", "project-run", "--json"]
        }))
        .unwrap(),
    )
    .unwrap();
    let run_kind_mismatch = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        readiness.to_str().unwrap(),
        "--readiness-sha256",
        &readiness_sha256,
        "--args-file",
        run_kind_mismatch_args.to_str().unwrap(),
        "--args-sha256",
        &sha256_path(&run_kind_mismatch_args),
        "--run-kind",
        "app-run",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!run_kind_mismatch.status.success());
    assert!(stderr(&run_kind_mismatch).contains("run_kind mismatch"));

    let api_key_env = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "plugin",
            "wrapper-harness",
            "--readiness",
            readiness.to_str().unwrap(),
            "--readiness-sha256",
            &readiness_sha256,
            "--args-file",
            valid_args.to_str().unwrap(),
            "--args-sha256",
            &valid_args_sha256,
            "--run-kind",
            "app-run",
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .env("OPENAI_API_KEY", "test-only-forbidden")
        .output()
        .unwrap();
    assert!(!api_key_env.status.success());
    assert!(stderr(&api_key_env).contains("forbidden provider API key"));
}
