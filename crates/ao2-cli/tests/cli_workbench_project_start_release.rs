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
