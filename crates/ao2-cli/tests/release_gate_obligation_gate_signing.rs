use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::RsaPrivateKey;
use serde_json::{json, Value};
use sha2::Sha256;
use signature::{SignatureEncoding, Signer};

fn ao2(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("invoke ao2")
}

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn keygen(out: &Path) {
    let output = ao2(&[
        "workbench",
        "support-keygen",
        "--out",
        out.to_str().unwrap(),
        "--bits",
        "2048",
        "--json",
    ]);
    assert_success(&output, "workbench support-keygen failed");
}

fn write_public_key(private_key: &Path, public_key: &Path) {
    let pem = fs::read_to_string(private_key).expect("read private key");
    let key = RsaPrivateKey::from_pkcs8_pem(&pem).expect("pkcs8 private key");
    use rsa::pkcs8::EncodePublicKey;
    use rsa::pkcs8::LineEnding;
    let public_pem = rsa::RsaPublicKey::from(&key)
        .to_public_key_pem(LineEnding::LF)
        .expect("encode public key pem");
    fs::write(public_key, public_pem).expect("write public key");
}

fn sign_bytes(private_key: &Path, input: &[u8]) -> Vec<u8> {
    let pem = fs::read_to_string(private_key).expect("read private key");
    let key = RsaPrivateKey::from_pkcs8_pem(&pem).expect("pkcs8 private key");
    let signing_key = SigningKey::<Sha256>::new(key);
    let signature: rsa::pkcs1v15::Signature = signing_key.sign(input);
    signature.to_bytes().to_vec()
}

fn raw_gate_value(stage: &str, target: &Path) -> Value {
    json!({
        "schema_version": "ao2.obligation-gate.v1",
        "stage": stage,
        "status": "passed",
        "verdict": "accepted",
        "summary": {"pass": 3, "fail": 0, "unverified": 0, "waived": 0},
        "ledger_path": target.join("obligations.json").display().to_string(),
        "target": target.display().to_string(),
        "gate_path": target
            .join("evidence-pack")
            .join(format!("obligation-gate-{stage}.json"))
            .display()
            .to_string(),
        "checked_at": "2026-05-25T05:55:00Z",
        "failed_obligations": [],
        "unverified_obligations": [],
        "checked_ledger": {
            "schema_version": "ao2.obligation-ledger-check.v1",
            "verdict": "accepted",
            "summary": {"pass": 3, "fail": 0, "unverified": 0, "waived": 0},
            "obligations": []
        }
    })
}

fn write_raw_gate(target: &Path, stage: &str) -> (PathBuf, Value) {
    let evidence_dir = target.join("evidence-pack");
    fs::create_dir_all(&evidence_dir).expect("evidence dir");
    let gate = raw_gate_value(stage, target);
    let gate_path = evidence_dir.join(format!("obligation-gate-{stage}.json"));
    fs::write(
        &gate_path,
        serde_json::to_string_pretty(&gate).unwrap() + "\n",
    )
    .expect("write raw gate");
    (gate_path, gate)
}

fn write_signed_wrapper(target: &Path, gate: &Value, generated_at_ms: u64, key_path: &Path) {
    let exports_dir = target
        .join(".ao2")
        .join("workbench")
        .join("evidence-exports");
    fs::create_dir_all(&exports_dir).expect("exports dir");
    let wrapper = json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": "obligation-gate",
        "target": target.display().to_string(),
        "export": {
            "gate": gate,
            "audit_event": {
                "schema_version": "ao2.workbench-audit-event.v1",
                "timestamp_ms": generated_at_ms,
                "action": "obligation_gate",
                "operator_id": "release-gate-test",
                "operator_role": "reviewer",
                "run_id": "release-gate-source-run",
                "stage": gate["stage"].clone(),
                "status": gate["status"].clone(),
                "verdict": gate["verdict"].clone()
            }
        }
    });
    let wrapper_path = exports_dir.join(format!(
        "evidence-export-{generated_at_ms}-obligation-gate.json"
    ));
    let signature_path = wrapper_path.with_extension("json.sig");
    let public_key_path = exports_dir.join("workbench-evidence-signing-public.pem");
    fs::write(
        &wrapper_path,
        serde_json::to_string_pretty(&wrapper).unwrap() + "\n",
    )
    .expect("write wrapper");
    write_public_key(key_path, &public_key_path);
    let wrapper_bytes = fs::read(&wrapper_path).expect("read wrapper bytes");
    let signature_bytes = sign_bytes(key_path, &wrapper_bytes);
    fs::write(&signature_path, signature_bytes).expect("write signature");
}

struct ReleaseFixture {
    summary_path: PathBuf,
    enriched_path: PathBuf,
    provenance_dir: PathBuf,
    archive: String,
}

fn build_release_fixture(temp: &tempfile::TempDir, version: &str) -> ReleaseFixture {
    let dist = temp.path().join("dist");
    let provenance_dir = temp.path().join("dist-provenance");
    let summary_path = temp.path().join("summary.json");
    let enriched_path = temp.path().join("summary.enriched.json");

    let package = ao2(&[
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        version,
    ]);
    assert_success(&package, "release package failed");
    let package_json: Value = serde_json::from_slice(&package.stdout).expect("package json");
    let archive = package_json["archive"]
        .as_str()
        .expect("archive path in package json")
        .to_string();

    let sign = ao2(&[
        "release",
        "sign-provenance",
        "--version",
        version,
        "--macos-archive",
        &archive,
        "--linux-archive",
        &archive,
        "--linux-x86-64-archive",
        &archive,
        "--windows-archive",
        &archive,
        "--provenance-dir",
        provenance_dir.to_str().unwrap(),
        "--private-key",
        temp.path().join("release-key.pem").to_str().unwrap(),
        "--json",
    ]);
    assert_success(&sign, "release sign-provenance failed");

    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": false,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable"
        }))
        .unwrap(),
    )
    .expect("write summary");

    ReleaseFixture {
        summary_path,
        enriched_path,
        provenance_dir,
        archive,
    }
}

#[test]
fn release_provenance_can_sign_hosted_three_archive_bundle_without_macos() {
    let temp = tempfile::tempdir().unwrap();
    let version = "9.9.9-hosted";
    let package_out = temp.path().join("package");
    let provenance_dir = temp.path().join("dist-provenance");

    let package = ao2(&[
        "release",
        "package",
        "--out-dir",
        package_out.to_str().unwrap(),
        "--version",
        version,
    ]);
    assert_success(&package, "release package failed");
    let package_json: Value = serde_json::from_slice(&package.stdout).expect("package json");
    let source_archive = PathBuf::from(package_json["archive"].as_str().unwrap());
    let linux_archive = temp
        .path()
        .join(format!("ao2-{version}-linux-aarch64.tar.gz"));
    let linux_x86_archive = temp
        .path()
        .join(format!("ao2-{version}-linux-x86_64.tar.gz"));
    let windows_archive = temp
        .path()
        .join(format!("ao2-{version}-windows-x86_64.tar.gz"));
    fs::copy(&source_archive, &linux_archive).expect("copy linux archive");
    fs::copy(&source_archive, &linux_x86_archive).expect("copy linux x86 archive");
    fs::copy(&source_archive, &windows_archive).expect("copy windows archive");

    let sign = ao2(&[
        "release",
        "sign-provenance",
        "--version",
        version,
        "--linux-archive",
        linux_archive.to_str().unwrap(),
        "--linux-x86-64-archive",
        linux_x86_archive.to_str().unwrap(),
        "--windows-archive",
        windows_archive.to_str().unwrap(),
        "--provenance-dir",
        provenance_dir.to_str().unwrap(),
        "--private-key",
        temp.path().join("release-key.pem").to_str().unwrap(),
        "--json",
    ]);
    assert_success(&sign, "release sign-provenance failed");
    let sign_json: Value = serde_json::from_slice(&sign.stdout).expect("sign json");
    assert_eq!(sign_json["archive_count"], 3);

    let verify = ao2(&[
        "release",
        "verify-provenance",
        "--linux-archive",
        linux_archive.to_str().unwrap(),
        "--linux-x86-64-archive",
        linux_x86_archive.to_str().unwrap(),
        "--windows-archive",
        windows_archive.to_str().unwrap(),
        "--provenance-dir",
        provenance_dir.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&verify, "release verify-provenance failed");
    let verify_json: Value = serde_json::from_slice(&verify.stdout).expect("verify json");
    assert_eq!(verify_json["archive_count"], 3);
    assert_eq!(verify_json["provenance_verified"], true);
}

#[test]
fn release_gate_can_verify_hosted_three_archive_bundle_without_macos() {
    let temp = tempfile::tempdir().unwrap();
    let version = "9.9.9-hosted-gate";
    let package_out = temp.path().join("package");
    let provenance_dir = temp.path().join("dist-provenance");
    let summary_path = temp.path().join("summary.json");

    let package = ao2(&[
        "release",
        "package",
        "--out-dir",
        package_out.to_str().unwrap(),
        "--version",
        version,
    ]);
    assert_success(&package, "release package failed");
    let package_json: Value = serde_json::from_slice(&package.stdout).expect("package json");
    let source_archive = PathBuf::from(package_json["archive"].as_str().unwrap());
    let linux_archive = temp
        .path()
        .join(format!("ao2-{version}-linux-aarch64.tar.gz"));
    let linux_x86_archive = temp
        .path()
        .join(format!("ao2-{version}-linux-x86_64.tar.gz"));
    let windows_archive = temp
        .path()
        .join(format!("ao2-{version}-windows-x86_64.tar.gz"));
    fs::copy(&source_archive, &linux_archive).expect("copy linux archive");
    fs::copy(&source_archive, &linux_x86_archive).expect("copy linux x86 archive");
    fs::copy(&source_archive, &windows_archive).expect("copy windows archive");

    let sign = ao2(&[
        "release",
        "sign-provenance",
        "--version",
        version,
        "--linux-archive",
        linux_archive.to_str().unwrap(),
        "--linux-x86-64-archive",
        linux_x86_archive.to_str().unwrap(),
        "--windows-archive",
        windows_archive.to_str().unwrap(),
        "--provenance-dir",
        provenance_dir.to_str().unwrap(),
        "--private-key",
        temp.path().join("release-key.pem").to_str().unwrap(),
    ]);
    assert_success(&sign, "release sign-provenance failed");

    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "linux_x86_64_remote_smoke": "passed",
            "native_windows_required": false,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "hosted_release_gate_archive_only",
            "obligation_gates": {
                "schema_version": "ao2.workbench-obligation-gates.v1",
                "present": true,
                "count": 1,
                "gates": [{
                    "schema_version": "ao2.workbench-obligation-gate-summary.v1",
                    "stage": "closure",
                    "status": "passed",
                    "verdict": "accepted",
                    "summary": {"pass": 3, "fail": 0, "unverified": 0, "waived": 0}
                }]
            }
        }))
        .unwrap(),
    )
    .expect("write summary");

    let gate = ao2(&[
        "release",
        "gate",
        "--summary",
        summary_path.to_str().unwrap(),
        "--provenance-dir",
        provenance_dir.to_str().unwrap(),
        "--linux-archive",
        linux_archive.to_str().unwrap(),
        "--linux-x86-64-archive",
        linux_x86_archive.to_str().unwrap(),
        "--windows-archive",
        windows_archive.to_str().unwrap(),
        "--allow-unsigned-obligation-gates",
    ]);
    assert_success(&gate, "release gate failed");
    let gate_json: Value = serde_json::from_slice(&gate.stdout).expect("gate json");
    assert_eq!(gate_json["status"], "verified");
    assert_eq!(gate_json["release"]["archive_count"], 3);
}

fn enrich_summary(fixture: &ReleaseFixture, target: &Path, gate_paths: &[&Path]) {
    let mut args: Vec<String> = vec![
        "release".to_string(),
        "summary-enrich".to_string(),
        "--summary".to_string(),
        fixture.summary_path.to_str().unwrap().to_string(),
        "--target".to_string(),
        target.to_str().unwrap().to_string(),
        "--out".to_string(),
        fixture.enriched_path.to_str().unwrap().to_string(),
        "--json".to_string(),
    ];
    for path in gate_paths {
        args.push("--obligation-gate".to_string());
        args.push(path.to_str().unwrap().to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let enrich = ao2(&arg_refs);
    assert_success(&enrich, "release summary-enrich failed");
}

/// Signing-mode opt-in selector for the release gate fixture runner.
enum SigningMode {
    /// Use the new default-on behavior (no extra flag).
    DefaultOn,
    /// Explicitly opt out via `--allow-unsigned-obligation-gates`.
    AllowUnsigned,
    /// Explicitly pass the legacy `--require-obligation-gate-signing` flag.
    /// Equivalent to default-on; exercised so back-compat callers stay green.
    LegacyRequire,
}

fn run_release_gate(fixture: &ReleaseFixture, mode: SigningMode) -> std::process::Output {
    run_release_gate_with_extra_args(fixture, mode, &[])
}

fn run_release_gate_with_extra_args(
    fixture: &ReleaseFixture,
    mode: SigningMode,
    extra_args: &[&str],
) -> std::process::Output {
    let mut args: Vec<&str> = vec![
        "release",
        "gate",
        "--summary",
        fixture.enriched_path.to_str().unwrap(),
        "--provenance-dir",
        fixture.provenance_dir.to_str().unwrap(),
        "--macos-archive",
        &fixture.archive,
        "--linux-archive",
        &fixture.archive,
        "--linux-x86-64-archive",
        &fixture.archive,
        "--windows-archive",
        &fixture.archive,
    ];
    match mode {
        SigningMode::DefaultOn => {}
        SigningMode::AllowUnsigned => args.push("--allow-unsigned-obligation-gates"),
        SigningMode::LegacyRequire => args.push("--require-obligation-gate-signing"),
    }
    args.extend_from_slice(extra_args);
    ao2(&args)
}

fn accepted_governed_run_fixture(run_id: &str) -> Value {
    json!({
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

fn write_governed_run_fixture(root: &Path, os_label: &str) -> PathBuf {
    let dir = root.join(os_label);
    fs::create_dir_all(&dir).expect("governed run dir");
    let path = dir.join("governed-run.json");
    fs::write(
        &path,
        serde_json::to_string_pretty(&accepted_governed_run_fixture(&format!(
            "real-factory-runspec-{os_label}"
        )))
        .unwrap(),
    )
    .expect("write governed run fixture");
    path
}

fn write_bom_governed_run_fixture(root: &Path, os_label: &str) -> PathBuf {
    let dir = root.join(os_label);
    fs::create_dir_all(&dir).expect("governed run dir");
    let path = dir.join("governed-run.json");
    let body = serde_json::to_string_pretty(&accepted_governed_run_fixture(&format!(
        "real-factory-runspec-{os_label}"
    )))
    .unwrap();
    fs::write(&path, format!("\u{feff}{body}")).expect("write governed run fixture with bom");
    path
}

fn accepted_project_run_readback_fixture(os_label: &str) -> Value {
    json!({
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
        "queued_replacement_packet_sha256": "b".repeat(64),
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

fn write_project_run_readback_fixture(root: &Path, os_label: &str, accepted: bool) -> PathBuf {
    let dir = root.join(os_label);
    fs::create_dir_all(&dir).expect("project run readback dir");
    let path = dir.join("factory-project-run-summary.json");
    let mut fixture = accepted_project_run_readback_fixture(os_label);
    if !accepted {
        fixture["queued_replacement_packet_verification_status"] = json!("failed");
    }
    fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap())
        .expect("write project run readback fixture");
    path
}

fn write_greenfield_three_os_gate(root: &Path, accepted: bool) -> PathBuf {
    let path = root.join("greenfield-three-os-smoke-gate.json");
    fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "ao2.greenfield-three-os-smoke-gate.v1",
            "status": if accepted { "accepted" } else { "rejected" },
            "accepted_os": if accepted {
                json!(["macos", "ubuntu", "windows"])
            } else {
                json!(["macos", "ubuntu"])
            },
            "missing_os": if accepted { json!([]) } else { json!(["windows"]) },
            "duplicate_os": [],
            "unknown_os": [],
            "input_errors": [],
            "factory_v3_role": "parity_oracle_only",
            "ao2_decision_owner": "ao2-native-greenfield-three-os-smoke-gate",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "three_os_contract": {
                "path_separator_safe_artifacts": accepted,
                "requires_native_windows_smoke": true,
                "requires_ubuntu_smoke": true,
                "requires_macos_smoke": true,
                "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "factory_v3_role": "parity_oracle_only",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "control_plane_approves_release": false,
                "mutates_ao_artifacts": false
            }
        }))
        .unwrap(),
    )
    .expect("write greenfield three-os gate");
    path
}

#[test]
fn release_gate_accepts_signed_obligation_gate_when_signing_required() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-signed");
    enrich_summary(&fixture, &target, &[&gate_path]);

    let gate_output = run_release_gate(&fixture, SigningMode::LegacyRequire);
    assert_success(&gate_output, "release gate must accept signed gate");
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("report json");
    assert_eq!(report["schema"], json!("ao2.release-gate.v1"));
    assert_eq!(report["status"], json!("verified"));
    let signing = &report["obligation_gate_signing"];
    assert_eq!(
        signing["schema"],
        json!("ao2.release-obligation-gate-signing-verification.v1")
    );
    assert_eq!(signing["status"], json!("verified"));
    let per_gate = signing["per_gate"].as_array().expect("per_gate array");
    assert_eq!(per_gate.len(), 1);
    assert_eq!(per_gate[0]["stage"], json!("closure"));
    assert_eq!(per_gate[0]["signing_status"], json!("signed-and-verified"));
    assert_eq!(per_gate[0]["signature_verified"], json!(true));
    assert_eq!(per_gate[0]["ao2_owned"], json!(true));
    assert!(signing["reasons"].as_array().unwrap().is_empty());
    let release_reasons = report["reasons"].as_array().expect("reasons array");
    assert!(
        release_reasons.is_empty(),
        "expected no release reasons, got {release_reasons:?}"
    );
}

#[test]
fn release_gate_accepts_relocated_signed_obligation_gate_sidecars() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-relocated-signed");
    enrich_summary(&fixture, &target, &[&gate_path]);

    let relocated = temp.path().join("relocated-summary-bundle");
    fs::create_dir_all(&relocated).expect("relocated dir");
    let relocated_summary = relocated.join("summary.enriched.json");
    fs::copy(&fixture.enriched_path, &relocated_summary).expect("copy summary");
    fs::copy(
        &gate_path,
        relocated.join(gate_path.file_name().expect("gate basename")),
    )
    .expect("copy gate");
    let exports_dir = target
        .join(".ao2")
        .join("workbench")
        .join("evidence-exports");
    for entry in fs::read_dir(&exports_dir).expect("exports dir") {
        let path = entry.expect("entry").path();
        if path.is_file() {
            fs::copy(
                &path,
                relocated.join(path.file_name().expect("export basename")),
            )
            .expect("copy export sidecar");
        }
    }
    fs::remove_dir_all(&target).expect("remove original target");

    let relocated_fixture = ReleaseFixture {
        summary_path: fixture.summary_path.clone(),
        enriched_path: relocated_summary,
        provenance_dir: fixture.provenance_dir.clone(),
        archive: fixture.archive.clone(),
    };
    let gate_output = run_release_gate(&relocated_fixture, SigningMode::DefaultOn);
    assert_success(
        &gate_output,
        "release gate must accept a moved summary with signed sidecars beside it",
    );
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    assert_eq!(
        report["obligation_gate_signing"]["status"],
        json!("verified")
    );
}

#[test]
fn release_gate_accepts_verified_three_os_governed_run_evidence_when_supplied() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-governed-run");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let evidence_root = temp.path().join("governed-run-evidence");
    let macos = write_governed_run_fixture(&evidence_root, "macos");
    let ubuntu = write_governed_run_fixture(&evidence_root, "ubuntu");
    let windows = write_governed_run_fixture(&evidence_root, "windows");

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--governed-run-evidence",
            macos.to_str().unwrap(),
            "--governed-run-evidence",
            ubuntu.to_str().unwrap(),
            "--governed-run-evidence",
            windows.to_str().unwrap(),
        ],
    );
    assert_success(
        &output,
        "release gate must accept three-OS governed run evidence",
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    assert_eq!(
        report["governed_run_evidence"]["schema"],
        json!("ao2.release-governed-run-evidence-verification.v1")
    );
    assert_eq!(report["governed_run_evidence"]["status"], json!("verified"));
    assert_eq!(
        report["governed_run_evidence"]["accepted_os"],
        json!(["macos", "ubuntu", "windows"])
    );
}

#[test]
fn release_gate_accepts_verified_factory_project_run_readback_when_supplied() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-project-run-readback");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let readback_root = temp.path().join("project-run-readback");
    let macos = write_project_run_readback_fixture(&readback_root, "macos", true);
    let ubuntu = write_project_run_readback_fixture(&readback_root, "ubuntu", true);
    let windows = write_project_run_readback_fixture(&readback_root, "windows", true);

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--factory-project-run-summary",
            macos.to_str().unwrap(),
            "--factory-project-run-summary",
            ubuntu.to_str().unwrap(),
            "--factory-project-run-summary",
            windows.to_str().unwrap(),
        ],
    );
    assert_success(
        &output,
        "release gate must accept three-OS project-run replacement-packet readback",
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    assert_eq!(
        report["factory_project_run_readback"]["schema"],
        json!("ao2.release-factory-project-run-readback-verification.v1")
    );
    assert_eq!(
        report["factory_project_run_readback"]["status"],
        json!("verified")
    );
    assert_eq!(
        report["factory_project_run_readback"]["accepted_os"],
        json!(["macos", "ubuntu", "windows"])
    );
}

#[test]
fn release_gate_fails_closed_on_rejected_factory_project_run_readback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-project-run-readback-rejected");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let readback_root = temp.path().join("project-run-readback");
    let macos = write_project_run_readback_fixture(&readback_root, "macos", true);
    let ubuntu = write_project_run_readback_fixture(&readback_root, "ubuntu", true);
    let windows = write_project_run_readback_fixture(&readback_root, "windows", false);

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--factory-project-run-summary",
            macos.to_str().unwrap(),
            "--factory-project-run-summary",
            ubuntu.to_str().unwrap(),
            "--factory-project-run-summary",
            windows.to_str().unwrap(),
        ],
    );
    assert!(
        !output.status.success(),
        "release gate must fail closed on rejected project-run readback\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("failed"));
    assert_eq!(
        report["factory_project_run_readback"]["status"],
        json!("failed")
    );
    assert!(report["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == json!("factory_project_run_readback_failed")));
}

#[test]
fn release_gate_accepts_windows_governed_run_evidence_with_utf8_bom() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-governed-run-bom");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let evidence_root = temp.path().join("governed-run-evidence");
    let macos = write_governed_run_fixture(&evidence_root, "macos");
    let ubuntu = write_governed_run_fixture(&evidence_root, "ubuntu");
    let windows = write_bom_governed_run_fixture(&evidence_root, "windows");

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--governed-run-evidence",
            macos.to_str().unwrap(),
            "--governed-run-evidence",
            ubuntu.to_str().unwrap(),
            "--governed-run-evidence",
            windows.to_str().unwrap(),
        ],
    );
    assert_success(
        &output,
        "release gate must accept UTF-8 BOM-prefixed Windows governed run evidence",
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["governed_run_evidence"]["status"], json!("verified"));
    assert_eq!(
        report["governed_run_evidence"]["accepted_os"],
        json!(["macos", "ubuntu", "windows"])
    );
}

#[test]
fn release_gate_fails_closed_on_incomplete_governed_run_evidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-governed-run-incomplete");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let evidence_root = temp.path().join("governed-run-evidence");
    let macos = write_governed_run_fixture(&evidence_root, "macos");
    let ubuntu = write_governed_run_fixture(&evidence_root, "ubuntu");

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--governed-run-evidence",
            macos.to_str().unwrap(),
            "--governed-run-evidence",
            ubuntu.to_str().unwrap(),
        ],
    );
    assert!(
        !output.status.success(),
        "release gate must fail closed without Windows governed run evidence\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("failed"));
    assert_eq!(report["governed_run_evidence"]["status"], json!("failed"));
    assert!(report["governed_run_evidence"]["missing_os"]
        .as_array()
        .unwrap()
        .iter()
        .any(|os| os == "windows"));
    assert!(report["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == json!("governed_run_evidence_failed")));
}

#[test]
fn release_gate_accepts_verified_replacement_smoke_gate_when_supplied() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-replacement-gate");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let replacement_gate = temp.path().join("replacement-smoke-gate.json");
    fs::write(
        &replacement_gate,
        serde_json::to_string_pretty(&json!({
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
        }))
        .unwrap(),
    )
    .expect("write replacement gate");

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--replacement-smoke-gate",
            replacement_gate.to_str().unwrap(),
        ],
    );
    assert_success(&output, "release gate must accept replacement smoke gate");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    assert_eq!(
        report["replacement_smoke_gate"]["schema"],
        json!("ao2.release-replacement-smoke-gate-verification.v1")
    );
    assert_eq!(
        report["replacement_smoke_gate"]["status"],
        json!("verified")
    );
    assert_eq!(
        report["replacement_smoke_gate"]["gate_status"],
        json!("accepted")
    );
}

#[test]
fn release_gate_accepts_verified_greenfield_three_os_gate_when_supplied() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-greenfield-three-os-gate");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let greenfield_gate = write_greenfield_three_os_gate(temp.path(), true);

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--greenfield-three-os-smoke-gate",
            greenfield_gate.to_str().unwrap(),
        ],
    );
    assert_success(&output, "release gate must accept greenfield three-os gate");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    assert_eq!(
        report["greenfield_three_os_smoke_gate"]["schema"],
        json!("ao2.release-greenfield-three-os-smoke-gate-verification.v1")
    );
    assert_eq!(
        report["greenfield_three_os_smoke_gate"]["status"],
        json!("verified")
    );
    assert_eq!(
        report["greenfield_three_os_smoke_gate"]["gate_status"],
        json!("accepted")
    );
}

#[test]
fn release_gate_fails_closed_on_rejected_replacement_smoke_gate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_001_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-replacement-gate-rejected");
    enrich_summary(&fixture, &target, &[&gate_path]);
    let replacement_gate = temp.path().join("replacement-smoke-gate.json");
    fs::write(
        &replacement_gate,
        serde_json::to_string_pretty(&json!({
            "schema_version": "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1",
            "status": "rejected",
            "accepted_os": ["macos", "ubuntu"],
            "missing_os": ["windows"],
            "duplicate_os": [],
            "unknown_os": [],
            "input_errors": [],
            "factory_v3_role": "parity_oracle_only",
            "ao2_decision_owner": "ao2-native-three-os-replacement-smoke-gate",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "three_os_contract": {
                "path_separator_safe_artifacts": false,
                "requires_native_windows_smoke": true,
                "requires_ubuntu_smoke": true,
                "requires_macos_smoke": true,
                "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
            }
        }))
        .unwrap(),
    )
    .expect("write replacement gate");

    let output = run_release_gate_with_extra_args(
        &fixture,
        SigningMode::DefaultOn,
        &[
            "--replacement-smoke-gate",
            replacement_gate.to_str().unwrap(),
        ],
    );
    assert!(
        !output.status.success(),
        "release gate must fail closed on rejected replacement smoke gate\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("failed"));
    assert_eq!(report["replacement_smoke_gate"]["status"], json!("failed"));
    assert!(report["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == json!("replacement_smoke_gate_failed")));
}

#[test]
fn release_gate_fails_closed_when_obligation_gate_unsigned_and_signing_required() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, _gate) = write_raw_gate(&target, "closure");
    // Intentionally do NOT write the workbench wrapper or sidecars.

    let fixture = build_release_fixture(&temp, "9.9.9-unsigned");
    enrich_summary(&fixture, &target, &[&gate_path]);

    let gate_output = run_release_gate(&fixture, SigningMode::LegacyRequire);
    assert!(
        !gate_output.status.success(),
        "release gate must fail closed when --require-obligation-gate-signing is set and gate is unsigned\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&gate_output.stdout),
        String::from_utf8_lossy(&gate_output.stderr)
    );
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("report json");
    assert_eq!(report["status"], json!("failed"));
    let signing = &report["obligation_gate_signing"];
    assert_eq!(signing["status"], json!("failed"));
    let per_gate = signing["per_gate"].as_array().expect("per_gate array");
    assert_eq!(per_gate.len(), 1);
    assert_eq!(per_gate[0]["signing_status"], json!("wrapper-not-found"));
    assert_eq!(per_gate[0]["signature_verified"], json!(false));
    let signing_reasons = signing["reasons"].as_array().expect("signing reasons");
    assert!(signing_reasons
        .iter()
        .any(|reason| reason["code"] == json!("obligation_gate_signing_not_verified")));
    let release_reasons = report["reasons"].as_array().expect("release reasons");
    assert!(release_reasons
        .iter()
        .any(|reason| reason["code"] == json!("obligation_gate_signing_unverified")));
}

#[test]
fn release_gate_allow_unsigned_obligation_gates_preserves_legacy_behavior() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, _gate) = write_raw_gate(&target, "closure");
    // Unsigned gate; escape valve set; release gate must still pass and
    // omit the obligation_gate_signing block.

    let fixture = build_release_fixture(&temp, "9.9.9-allow-unsigned");
    enrich_summary(&fixture, &target, &[&gate_path]);

    let gate_output = run_release_gate(&fixture, SigningMode::AllowUnsigned);
    assert_success(
        &gate_output,
        "release gate must accept unsigned gate when --allow-unsigned-obligation-gates is set",
    );
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    assert!(
        report.get("obligation_gate_signing").is_none(),
        "obligation_gate_signing block must be absent when escape valve is set; report:\n{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
}

#[test]
fn release_gate_requires_signing_by_default_and_fails_closed_on_unsigned() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, _gate) = write_raw_gate(&target, "closure");
    // Unsigned gate; NO flag passed; under default-on signing the release
    // gate must fail closed.

    let fixture = build_release_fixture(&temp, "9.9.9-default-on");
    enrich_summary(&fixture, &target, &[&gate_path]);

    let gate_output = run_release_gate(&fixture, SigningMode::DefaultOn);
    assert!(
        !gate_output.status.success(),
        "release gate must fail closed by default when obligation gate is unsigned\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&gate_output.stdout),
        String::from_utf8_lossy(&gate_output.stderr)
    );
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("report json");
    assert_eq!(report["status"], json!("failed"));
    let signing = &report["obligation_gate_signing"];
    assert_eq!(signing["status"], json!("failed"));
    let per_gate = signing["per_gate"].as_array().expect("per_gate array");
    assert_eq!(per_gate.len(), 1);
    assert_eq!(per_gate[0]["signing_status"], json!("wrapper-not-found"));
    let release_reasons = report["reasons"].as_array().expect("release reasons");
    assert!(release_reasons
        .iter()
        .any(|reason| reason["code"] == json!("obligation_gate_signing_unverified")));
}

#[test]
fn release_gate_default_on_with_signed_wrapper_passes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_002_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-default-on-signed");
    enrich_summary(&fixture, &target, &[&gate_path]);

    let gate_output = run_release_gate(&fixture, SigningMode::DefaultOn);
    assert_success(
        &gate_output,
        "release gate must accept signed gate by default with no flags",
    );
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    let signing = &report["obligation_gate_signing"];
    assert_eq!(signing["status"], json!("verified"));
    let per_gate = signing["per_gate"].as_array().expect("per_gate array");
    assert_eq!(per_gate.len(), 1);
    assert_eq!(per_gate[0]["signing_status"], json!("signed-and-verified"));
}

#[test]
fn release_gate_legacy_require_flag_remains_accepted_as_no_op() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);
    write_signed_wrapper(&target, &gate, 1_700_000_003_000, &key_path);

    let fixture = build_release_fixture(&temp, "9.9.9-legacy-require");
    enrich_summary(&fixture, &target, &[&gate_path]);

    // Legacy --require-obligation-gate-signing flag is still accepted by
    // the CLI parser even though signing is required by default. Existing
    // scripts that pass it must continue to work.
    let gate_output = run_release_gate(&fixture, SigningMode::LegacyRequire);
    assert_success(
        &gate_output,
        "release gate must continue to accept the legacy --require-obligation-gate-signing flag",
    );
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("report json");
    assert_eq!(report["status"], json!("verified"));
    assert_eq!(
        report["obligation_gate_signing"]["status"],
        json!("verified")
    );
}
