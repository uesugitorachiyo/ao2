use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

#[test]
fn cli_factory_replacement_smoke_can_run_provider_backed_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 provider-backed replacement smoke
objective: Prove AO2 can own provider-backed factory-v3 replacement execution with factory-v3 only as parity oracle.
acceptance:
  - AO2 plans, queues, executes with the requested provider, verifies, and packs evidence with one command.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: replacement-smoke-provider
verifier:
  command: python -m pytest -q
",
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
printf 'Summary: provider-backed replacement smoke fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 12\n'
"#,
    )
    .unwrap();
    let signing_key = temp
        .path()
        .join("replacement-smoke-provider-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("replacement-smoke-provider-out");

    let smoke = ao2([
        "factory",
        "replacement-smoke",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "replacement-smoke-provider-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "replacement-smoke-provider-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(smoke.status.success(), "{}", stderr(&smoke));
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["provider_execution"]["provider"], "scripted");
    assert_eq!(json["provider_execution"]["mode"], "provider-backed");
    assert_eq!(
        json["replacement_checklist"]["ao2_provider_backed_replacement_workflow"],
        true
    );
    assert_eq!(
        json["queue_run_next"]["entry"]["provider_execution"]["provider"],
        "scripted"
    );
    let evidence =
        fs::read_to_string(json["pack_evidence"]["evidence_pack_out"].as_str().unwrap()).unwrap();
    assert!(evidence.contains("provider_prompt_transcript"));
    assert!(evidence.contains("provider-backed replacement smoke fixed discount validation"));
}

#[test]
fn cli_factory_replacement_smoke_accepts_relative_target_paths() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    fs::write(
        temp.path().join("request.yaml"),
        r#"title: AO2 relative target replacement smoke
objective: Prove replacement smoke remains verifiable when operators pass a relative target path.
acceptance:
  - AO2 stores canonical evidence paths and verifies the run result.
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("runspec.yaml"),
        "id: relative-target-replacement-smoke
verifier:
  command: python -m pytest -q
",
    )
    .unwrap();
    let signing_key = temp.path().join("relative-target-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "factory",
            "replacement-smoke",
            "--request",
            "request.yaml",
            "--runspec",
            "runspec.yaml",
            "--target",
            "discount-service",
            "--run-id",
            "relative-target-replacement-smoke",
            "--signing-key",
        ])
        .arg(&signing_key)
        .args([
            "--signer-id",
            "relative-target-test",
            "--out-dir",
            "out",
            "--json",
        ])
        .current_dir(temp.path())
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("replacement smoke json");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_result_verification"]["status"], "accepted");
    assert_eq!(
        json["run_result_verification"]["required_files_present"],
        true
    );
    assert_eq!(
        json["replacement_checklist"]["ao2_verified_primary_run_result"],
        true
    );
}

#[test]
fn cli_factory_replacement_smoke_gate_accepts_three_signed_ao2_os_smokes() {
    let temp = tempfile::tempdir().unwrap();
    let macos = temp.path().join("macos-smoke.json");
    let ubuntu = temp.path().join("ubuntu-smoke.json");
    let windows = temp.path().join("windows-smoke.json");
    fs::write(
        &macos,
        serde_json::to_string_pretty(&replacement_smoke_fixture("macos", true)).unwrap(),
    )
    .unwrap();
    fs::write(
        &ubuntu,
        serde_json::to_string_pretty(&replacement_smoke_fixture("ubuntu", true)).unwrap(),
    )
    .unwrap();
    fs::write(
        &windows,
        serde_json::to_string_pretty(&replacement_smoke_fixture("windows", true)).unwrap(),
    )
    .unwrap();
    let out = temp.path().join("three-os-gate.json");

    let gate = ao2([
        "factory",
        "replacement-smoke-gate",
        "--smoke",
        &format!("macos={}", macos.display()),
        "--smoke",
        &format!("ubuntu={}", ubuntu.display()),
        "--smoke",
        &format!("windows={}", windows.display()),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(gate.status.success(), "{}", stderr(&gate));
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["accepted_os"].as_array().unwrap().len(), 3);
    assert_eq!(json["missing_os"].as_array().unwrap().len(), 0);
    assert_eq!(json["factory_v3_role"], "parity_oracle_only");
    assert_eq!(
        json["ao2_decision_owner"],
        "ao2-native-three-os-replacement-smoke-gate"
    );
    assert_eq!(
        json["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        json["three_os_contract"]["path_separator_safe_artifacts"],
        true
    );
    assert!(out.is_file());
}

#[test]
fn cli_factory_replacement_parity_status_summarizes_governed_and_three_os_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let governed_run = temp.path().join("governed-run.json");
    let three_os_gate = temp.path().join("three-os-gate.json");
    fs::write(
        &governed_run,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-governed-run.v1",
            "status": "accepted",
            "run_id": "parity-status-governed-run",
            "plan": {
                "schema_version": "ao2.factory-v3-compat-governed-plan.v1",
                "classification": {
                    "size": "large",
                    "shape": "greenfield",
                    "factory_v3_required_before_classification": false
                },
                "ao2_native_plan": {
                    "role_contract_discovery": {
                        "mode": "auto_discovered_from_ao_runspec_layout",
                        "loaded_count": 2
                    },
                    "runnable_workflow": {
                        "factory_v3_drives_workflow": false
                    }
                },
                "parity_checklist_progress": {
                    "ao2_accepts_request_and_classifies": true,
                    "ao2_loads_factory_v3_runspec_profiles_roles": true,
                    "factory_v3_drives_workflow": false
                }
            },
            "provider_adapter_contract": {
                "fulfilled": true
            },
            "governed_run_checklist": {
                "ao2_planned_factory_compat_workflow": true,
                "ao2_auto_loaded_role_contracts": true,
                "ao2_provider_backed_governed_workflow": true,
                "ao2_queue_executed_factory_compat_workflow": true,
                "ao2_verified_primary_run_result": true,
                "ao2_packed_primary_evidence": true,
                "ao2_signed_evaluator_closure": true,
                "factory_v3_drives_workflow": false,
                "factory_v3_role": "parity_oracle_only",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "hermes_role": "front_end_scheduler_queue_and_memory_bookkeeping"
            },
            "parity_checklist_progress": {
                "ao2_executes_generated_factory_compat_plan": true,
                "factory_v3_drives_workflow": false,
                "ao2_owns_midpoint_gate_decision": true,
                "ao2_owns_evaluator_closer_decision": true,
                "factory_v3_evaluator_compared_when_supplied": true,
                "ao2_replay_completed": true,
                "ao2_exports_hermes_memory_summary": true,
                "ao2_persists_factory_compat_run_result": true,
                "ao2_persists_restart_safe_factory_compat_history": true,
                "ao2_produces_factory_compat_handoff_evidence": true,
                "ao2_can_sign_factory_compat_handoff_evidence": true,
                "ao2_provider_adapter_contract_hardened": true
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "decision_owner": "ao2",
                "factory_v3_role": "parity_oracle_only",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
            },
            "factory_v3_role": "parity_oracle_only",
            "ao2_decision_owner": "ao2-native-governed-run",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &three_os_gate,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1",
            "status": "accepted",
            "accepted_os": ["macos", "ubuntu", "windows"],
            "missing_os": [],
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
        }))
        .unwrap(),
    )
    .unwrap();
    let governed_sha = sha256_path(&governed_run);
    let gate_sha = sha256_path(&three_os_gate);

    let status = ao2([
        "factory",
        "replacement-parity-status",
        "--target",
        repo.to_str().unwrap(),
        "--governed-run",
        governed_run.to_str().unwrap(),
        "--governed-run-sha256",
        &governed_sha,
        "--three-os-gate",
        three_os_gate.to_str().unwrap(),
        "--three-os-gate-sha256",
        &gate_sha,
        "--json",
    ]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-replacement-parity-status.v1"
    );
    assert_eq!(json["status"], "ready_for_parity_oracle");
    assert_eq!(json["governed_run_sha256"], governed_sha);
    assert_eq!(json["three_os_gate_sha256"], gate_sha);
    assert_eq!(
        json["checklist"]["accepts_and_classifies_work_request"],
        true
    );
    assert_eq!(json["checklist"]["loads_runspec_profiles_roles"], true);
    assert_eq!(
        json["checklist"]["provider_adapter_contract_hardened"],
        true
    );
    assert_eq!(json["checklist"]["midpoint_and_closure_gates_native"], true);
    assert_eq!(json["checklist"]["evaluator_closer_decision_native"], true);
    assert_eq!(json["checklist"]["queue_history_restart_safe"], true);
    assert_eq!(json["checklist"]["signed_evidence_replay_memory"], true);
    assert_eq!(json["checklist"]["three_os_validated"], true);
    assert_eq!(
        json["checklist"]["release_handoff_support_bundle_native"],
        true
    );
    assert_eq!(json["checklist"]["factory_v3_parity_oracle_only"], true);
    assert_eq!(json["remaining_gaps"].as_array().unwrap().len(), 0);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(
        json["next_recommended_lengthy_task"],
        "Run factory-v3 parity-oracle comparison against this AO2 replacement-parity status, then let ao2-control-plane K37 observe the signed AO2 evidence chain read-only."
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
        &format!(
            "GET /api/factory/replacement-parity-status?token=viewer-token&governed_run={}&governed_run_sha256={}&three_os_gate={}&three_os_gate_sha256={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(&governed_run.display().to_string()),
            governed_sha,
            percent_encode_for_test(&three_os_gate.display().to_string()),
            gate_sha,
        ),
    );
    let child_status = child.wait().unwrap();
    assert!(child_status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let api_json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(api_json["status"], "ready_for_parity_oracle");
    assert_eq!(api_json["governed_run_sha256"], governed_sha);
    assert_eq!(api_json["three_os_gate_sha256"], gate_sha);
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
fn cli_factory_replacement_smoke_gate_rejects_tampered_windows_signature() {
    let temp = tempfile::tempdir().unwrap();
    let macos = temp.path().join("macos-smoke.json");
    let ubuntu = temp.path().join("ubuntu-smoke.json");
    let windows = temp.path().join("windows-smoke.json");
    fs::write(
        &macos,
        serde_json::to_string_pretty(&replacement_smoke_fixture("macos", true)).unwrap(),
    )
    .unwrap();
    fs::write(
        &ubuntu,
        serde_json::to_string_pretty(&replacement_smoke_fixture("ubuntu", true)).unwrap(),
    )
    .unwrap();
    fs::write(
        &windows,
        serde_json::to_string_pretty(&replacement_smoke_fixture("windows", false)).unwrap(),
    )
    .unwrap();

    let gate = ao2([
        "factory",
        "replacement-smoke-gate",
        "--smoke",
        &format!("macos={}", macos.display()),
        "--smoke",
        &format!("ubuntu={}", ubuntu.display()),
        "--smoke",
        &format!("windows={}", windows.display()),
        "--json",
    ]);
    assert!(!gate.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(json["status"], "rejected");
    assert_eq!(json["accepted_os"].as_array().unwrap().len(), 2);
    assert!(json["per_os"].as_array().unwrap().iter().any(|entry| {
        entry["os"] == "windows"
            && entry["status"] == "rejected"
            && entry["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "pack_evidence.signature.signature_verified must be true")
    }));
}

fn replacement_smoke_fixture(os: &str, signature_verified: bool) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-replacement-smoke.v1",
        "status": "accepted",
        "run_id": format!("replacement-smoke-{os}"),
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-replacement-smoke",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "replacement_checklist": {
            "ao2_planned_factory_compat_workflow": true,
            "ao2_queue_executed_factory_compat_workflow": true,
            "ao2_verified_primary_run_result": true,
            "ao2_packed_primary_evidence": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "run_result_verification": {
            "status": "accepted",
            "ao2_primary_run_result_ok": true,
            "trust_boundary_ok": true
        },
        "pack_evidence": {
            "status": "produced",
            "evidence_pack_execution_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "signature": {
                "signature_verified": signature_verified
            }
        },
        "three_os_contract": {
            "path_separator_safe_artifacts": true,
            "requires_native_windows_smoke": true,
            "requires_ubuntu_smoke": true,
            "requires_macos_smoke": true,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    })
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
