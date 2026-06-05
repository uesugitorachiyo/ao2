use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::RsaPrivateKey;
use serde_json::{json, Value};
use sha2::Sha256;
use signature::{SignatureEncoding, Signer};

fn run_ao2(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .output()
        .expect("invoke ao2")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn raw_gate_value(stage: &str, target: &Path) -> Value {
    json!({
        "schema_version": "ao2.obligation-gate.v1",
        "stage": stage,
        "status": "passed",
        "verdict": "accepted",
        "summary": {"pass": 1, "fail": 0, "unverified": 0, "waived": 0},
        "ledger_path": target.join("obligations.json").display().to_string(),
        "target": target.display().to_string(),
        "gate_path": target
            .join("evidence-pack")
            .join(format!("obligation-gate-{stage}.json"))
            .display()
            .to_string(),
        "checked_at": "2026-05-25T05:30:00Z",
        "failed_obligations": [],
        "unverified_obligations": [],
        "checked_ledger": {
            "schema_version": "ao2.obligation-ledger-check.v1",
            "verdict": "accepted",
            "summary": {"pass": 1, "fail": 0, "unverified": 0, "waived": 0},
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

fn build_wrapper(target: &Path, gate: &Value, generated_at_ms: u64) -> Value {
    json!({
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
                "operator_id": "test-operator",
                "operator_role": "reviewer",
                "run_id": "run-test",
                "stage": gate["stage"].clone(),
                "status": gate["status"].clone(),
                "verdict": gate["verdict"].clone()
            }
        }
    })
}

fn workbench_exports_dir(target: &Path) -> PathBuf {
    target
        .join(".ao2")
        .join("workbench")
        .join("evidence-exports")
}

fn keygen(out: &Path) {
    let keygen = run_ao2(&[
        "workbench",
        "support-keygen",
        "--out",
        out.to_str().unwrap(),
        "--bits",
        "2048",
        "--json",
    ]);
    assert_success(&keygen);
}

fn write_public_key(private_key: &Path, public_key: &Path) {
    // Mirror the CLI's `derive_public_key_from_private_key` flow:
    // PKCS#8 -> RSA private key -> SPKI PEM. Done via the binary's keygen
    // step + a real-life signing helper would normally produce the public
    // key as a sidecar; for tests we shell back through `ao2 workbench
    // workbench-evidence-package`-style helpers OR just compute it via the
    // rsa crate to keep the test self-contained.
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

fn write_signed_wrapper(
    exports_dir: &Path,
    generated_at_ms: u64,
    wrapper: &Value,
    private_key: &Path,
) -> (PathBuf, PathBuf, PathBuf) {
    fs::create_dir_all(exports_dir).expect("exports dir");
    let wrapper_path = exports_dir.join(format!(
        "evidence-export-{generated_at_ms}-obligation-gate.json"
    ));
    let signature_path = wrapper_path.with_extension("json.sig");
    let public_key_path = exports_dir.join("workbench-evidence-signing-public.pem");
    fs::write(
        &wrapper_path,
        serde_json::to_string_pretty(wrapper).unwrap() + "\n",
    )
    .expect("write wrapper");
    write_public_key(private_key, &public_key_path);
    let wrapper_bytes = fs::read(&wrapper_path).expect("read wrapper bytes");
    let signature_bytes = sign_bytes(private_key, &wrapper_bytes);
    fs::write(&signature_path, signature_bytes).expect("write signature");
    (wrapper_path, signature_path, public_key_path)
}

fn run_verify(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["contract", "verify-obligation-gate-signing"])
        .args(args)
        .output()
        .expect("invoke ao2 contract verify-obligation-gate-signing")
}

#[test]
fn sign_obligation_gate_wraps_existing_raw_gate_for_release_smoke_producers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, _gate) = write_raw_gate(&target, "closure");
    let key_path = tmp.path().join("signing-key.pem");
    keygen(&key_path);

    let output = run_ao2(&[
        "contract",
        "sign-obligation-gate",
        "--gate",
        gate_path.to_str().unwrap(),
        "--support-signing-key",
        key_path.to_str().unwrap(),
        "--support-signer-id",
        "three-os-release-smoke",
        "--support-operator-role",
        "release",
        "--support-run-id",
        "three-os-smoke-test",
        "--json",
    ]);
    assert_success(&output);
    let signed: Value = serde_json::from_slice(&output.stdout).expect("signed evidence json");
    assert_eq!(
        signed["schema_version"],
        json!("ao2.contract-gate-support-signing-evidence.v1")
    );
    assert_eq!(signed["signature_verified"], json!(true));
    assert_eq!(signed["signer_id"], json!("three-os-release-smoke"));
    assert_eq!(signed["signer_role"], json!("release"));
    assert_eq!(signed["signer_run_id"], json!("three-os-smoke-test"));
    assert!(Path::new(signed["wrapper_path"].as_str().unwrap()).is_file());
    assert!(Path::new(signed["signature_path"].as_str().unwrap()).is_file());
    assert!(Path::new(signed["public_key_path"].as_str().unwrap()).is_file());

    let verify = run_verify(&["--gate", gate_path.to_str().unwrap(), "--json"]);
    assert_success(&verify);
    let report: Value = serde_json::from_slice(&verify.stdout).expect("verify json");
    assert_eq!(report["signing_status"], json!("signed-and-verified"));
    assert_eq!(report["signature_verified"], json!(true));
    assert_eq!(report["ao2_owned"], json!(true));
}

#[test]
fn verify_obligation_gate_signing_accepts_workbench_signed_wrapper() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let exports_dir = workbench_exports_dir(&target);
    let key_path = tmp.path().join("signing-key.pem");
    keygen(&key_path);
    let wrapper = build_wrapper(&target, &gate, 1_700_000_000_000);
    write_signed_wrapper(&exports_dir, 1_700_000_000_000, &wrapper, &key_path);

    let output = run_verify(&["--gate", gate_path.to_str().unwrap(), "--json"]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(
        report["schema_version"],
        json!("ao2.obligation-gate-signing-audit.v1")
    );
    assert_eq!(report["signing_status"], json!("signed-and-verified"));
    assert_eq!(report["signature_verified"], json!(true));
    assert_eq!(report["signature_present"], json!(true));
    assert_eq!(report["public_key_present"], json!(true));
    assert_eq!(report["ao2_owned"], json!(true));
    assert_eq!(report["stage"], json!("closure"));
    assert_eq!(report["factory_v3_role"], json!("no_role"));
}

#[test]
fn verify_obligation_gate_signing_reports_wrapper_not_found_when_only_raw_gate_exists() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, _gate) = write_raw_gate(&target, "midpoint");
    // No wrapper produced; evidence-exports dir is empty (we don't create it).

    let output = run_verify(&["--gate", gate_path.to_str().unwrap(), "--json"]);
    assert!(
        !output.status.success(),
        "verify must exit non-zero when wrapper missing; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["signing_status"], json!("wrapper-not-found"));
    assert_eq!(report["signature_verified"], json!(false));
    assert_eq!(report["matched_wrapper_path"], Value::Null);
    assert_eq!(report["ao2_owned"], json!(false));
}

#[test]
fn verify_obligation_gate_signing_reports_signature_missing_when_sidecar_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let exports_dir = workbench_exports_dir(&target);
    let key_path = tmp.path().join("signing-key.pem");
    keygen(&key_path);
    let wrapper = build_wrapper(&target, &gate, 1_700_000_000_000);
    let (_wrapper_path, signature_path, _public_key_path) =
        write_signed_wrapper(&exports_dir, 1_700_000_000_000, &wrapper, &key_path);
    fs::remove_file(&signature_path).expect("remove signature sidecar");

    let output = run_verify(&["--gate", gate_path.to_str().unwrap(), "--json"]);
    assert!(!output.status.success(), "must fail when signature missing");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["signing_status"], json!("signature-missing"));
    assert_eq!(report["signature_present"], json!(false));
    assert_eq!(report["signature_verified"], json!(false));
}

#[test]
fn verify_obligation_gate_signing_reports_signature_invalid_when_wrapper_tampered() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let (gate_path, gate) = write_raw_gate(&target, "closure");
    let exports_dir = workbench_exports_dir(&target);
    let key_path = tmp.path().join("signing-key.pem");
    keygen(&key_path);
    let wrapper = build_wrapper(&target, &gate, 1_700_000_000_000);
    let (wrapper_path, _signature_path, _public_key_path) =
        write_signed_wrapper(&exports_dir, 1_700_000_000_000, &wrapper, &key_path);
    // Tamper the wrapper bytes AFTER signing — signature must no longer
    // verify against the on-disk wrapper, even though the embedded gate
    // still equals the raw gate (we tamper a field outside `export.gate`).
    let mut wrapper_value: Value =
        serde_json::from_str(&fs::read_to_string(&wrapper_path).unwrap()).unwrap();
    wrapper_value["export"]["audit_event"]["operator_id"] =
        json!("attacker-controlled-operator-id");
    fs::write(
        &wrapper_path,
        serde_json::to_string_pretty(&wrapper_value).unwrap() + "\n",
    )
    .expect("write tampered wrapper");

    let output = run_verify(&["--gate", gate_path.to_str().unwrap(), "--json"]);
    assert!(!output.status.success(), "must fail when signature invalid");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    // Note: the wrapper's embedded gate must still equal the raw gate for
    // the match to succeed. Tampering audit_event preserves export.gate
    // equality but breaks the byte-level signature. If pairing fails
    // because we accidentally tampered export.gate, the status would
    // become wrapper-not-found.
    assert_eq!(report["signing_status"], json!("signature-invalid"));
    assert_eq!(report["signature_verified"], json!(false));
    assert!(!report["matched_wrapper_path"].is_null());
}
