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
        "summary": {"pass": 1, "fail": 0, "unverified": 0, "waived": 0},
        "ledger_path": target.join("obligations.json").display().to_string(),
        "target": target.display().to_string(),
        "gate_path": target
            .join("evidence-pack")
            .join(format!("obligation-gate-{stage}.json"))
            .display()
            .to_string(),
        "checked_at": "2026-05-25T06:00:00Z",
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

fn write_raw_gate(target: &Path, run_id: &str, stage: &str) -> (PathBuf, Value) {
    let evidence_dir = target
        .join(".ao2")
        .join("runs")
        .join(run_id)
        .join("evidence-pack");
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
                "operator_id": "survey-test",
                "operator_role": "reviewer",
                "run_id": "survey-test-run",
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

fn run_survey(target: &Path) -> std::process::Output {
    ao2(&[
        "contract",
        "obligation-gate-signing-survey",
        "--target",
        target.to_str().unwrap(),
        "--json",
    ])
}

fn run_survey_with_summary(summary: &Path) -> std::process::Output {
    ao2(&[
        "contract",
        "obligation-gate-signing-survey",
        "--summary",
        summary.to_str().unwrap(),
        "--json",
    ])
}

fn run_survey_with_both(target: &Path, summary: &Path) -> std::process::Output {
    ao2(&[
        "contract",
        "obligation-gate-signing-survey",
        "--target",
        target.to_str().unwrap(),
        "--summary",
        summary.to_str().unwrap(),
        "--json",
    ])
}

fn run_survey_no_args() -> std::process::Output {
    ao2(&["contract", "obligation-gate-signing-survey", "--json"])
}

fn write_release_summary(summary_path: &Path, gates: &[(String, String)]) {
    let gate_values: Vec<Value> = gates
        .iter()
        .map(|(stage, path)| {
            json!({
                "stage": stage,
                "status": "passed",
                "verdict": "accepted",
                "path": path,
                "summary": {"pass": 1, "fail": 0, "unverified": 0, "waived": 0}
            })
        })
        .collect();
    let summary = json!({
        "obligation_gates": {
            "count": gate_values.len(),
            "gates": gate_values,
            "present": !gate_values.is_empty(),
            "status": "verified"
        }
    });
    fs::write(
        summary_path,
        serde_json::to_string_pretty(&summary).unwrap() + "\n",
    )
    .expect("write summary");
}

#[test]
fn survey_reports_empty_when_no_runs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let output = run_survey(&target);
    assert_success(&output, "survey must succeed on empty target");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(
        report["schema_version"],
        json!("ao2.obligation-gate-signing-survey.v1")
    );
    assert_eq!(report["status"], json!("empty"));
    assert_eq!(report["total_gates"], json!(0));
    assert_eq!(report["signed_and_verified"], json!(0));
    assert_eq!(report["unsigned"], json!(0));
    assert!(report["per_gate"].as_array().unwrap().is_empty());
}

#[test]
fn survey_inventories_mixed_signed_and_unsigned_gates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);

    // Run A — signed gate.
    let (_gate_a_path, gate_a) = write_raw_gate(&target, "run-a", "closure");
    write_signed_wrapper(&target, &gate_a, 1_700_000_010_000, &key_path);

    // Run B — unsigned gate (raw only, no wrapper).
    let (_gate_b_path, _gate_b) = write_raw_gate(&target, "run-b", "midpoint");

    let output = run_survey(&target);
    assert_success(&output, "survey must succeed even when some gates unsigned");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("remediation-required"));
    assert_eq!(report["total_gates"], json!(2));
    assert_eq!(report["signed_and_verified"], json!(1));
    assert_eq!(report["unsigned"], json!(1));
    assert_eq!(report["errors"], json!(0));

    let per_gate = report["per_gate"].as_array().expect("per_gate array");
    assert_eq!(per_gate.len(), 2);
    let run_a_entry = per_gate
        .iter()
        .find(|entry| entry["run_id"] == json!("run-a"))
        .expect("run-a entry");
    assert_eq!(run_a_entry["stage"], json!("closure"));
    assert_eq!(run_a_entry["signing_status"], json!("signed-and-verified"));
    assert_eq!(run_a_entry["signature_verified"], json!(true));
    assert_eq!(run_a_entry["ao2_owned"], json!(true));
    assert_eq!(run_a_entry["suggested_remediation"], Value::Null);
    let run_b_entry = per_gate
        .iter()
        .find(|entry| entry["run_id"] == json!("run-b"))
        .expect("run-b entry");
    assert_eq!(run_b_entry["stage"], json!("midpoint"));
    assert_eq!(run_b_entry["signing_status"], json!("wrapper-not-found"));
    assert_eq!(run_b_entry["signature_verified"], json!(false));
    assert!(
        run_b_entry["suggested_remediation"]
            .as_str()
            .expect("remediation command")
            .contains("--run-id run-b"),
        "remediation command must reference run-b: {}",
        run_b_entry["suggested_remediation"]
    );
}

#[test]
fn survey_reports_all_signed_when_every_gate_has_wrapper() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);

    let (_gate_a_path, gate_a) = write_raw_gate(&target, "run-a", "closure");
    write_signed_wrapper(&target, &gate_a, 1_700_000_011_000, &key_path);
    let (_gate_b_path, gate_b) = write_raw_gate(&target, "run-b", "midpoint");
    write_signed_wrapper(&target, &gate_b, 1_700_000_012_000, &key_path);

    let output = run_survey(&target);
    assert_success(&output, "survey must succeed when all gates signed");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("all-signed-and-verified"));
    assert_eq!(report["total_gates"], json!(2));
    assert_eq!(report["signed_and_verified"], json!(2));
    assert_eq!(report["unsigned"], json!(0));
    let per_gate = report["per_gate"].as_array().expect("per_gate array");
    assert!(per_gate
        .iter()
        .all(|entry| entry["signing_status"] == json!("signed-and-verified")));
    assert!(per_gate
        .iter()
        .all(|entry| entry["suggested_remediation"].is_null()));
}

#[test]
fn survey_with_summary_inventories_release_summary_gates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);

    let (gate_a_path, gate_a) = write_raw_gate(&target, "run-a", "closure");
    write_signed_wrapper(&target, &gate_a, 1_700_000_020_000, &key_path);
    let (gate_b_path, _gate_b) = write_raw_gate(&target, "run-b", "midpoint");

    let summary_path = temp.path().join("release-summary.json");
    write_release_summary(
        &summary_path,
        &[
            ("closure".to_string(), gate_a_path.display().to_string()),
            ("midpoint".to_string(), gate_b_path.display().to_string()),
        ],
    );

    let output = run_survey_with_summary(&summary_path);
    assert_success(&output, "survey --summary must succeed");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("remediation-required"));
    assert_eq!(report["total_gates"], json!(2));
    assert_eq!(report["signed_and_verified"], json!(1));
    assert_eq!(report["unsigned"], json!(1));
    assert_eq!(report["missing"], json!(0));
    assert_eq!(report["errors"], json!(0));
    assert_eq!(report["sources"], json!(["release-summary"]));
    assert_eq!(report["target"], json!(""));
    assert_eq!(report["summary"], json!(summary_path.display().to_string()));

    let per_gate = report["per_gate"].as_array().expect("per_gate");
    assert_eq!(per_gate.len(), 2);
    for entry in per_gate {
        assert_eq!(entry["sources"], json!(["release-summary"]));
        assert_eq!(entry["run_id"], json!(""));
    }
}

#[test]
fn survey_with_summary_handles_missing_gate_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ghost_path = temp.path().join("nonexistent-obligation-gate.json");
    let summary_path = temp.path().join("release-summary.json");
    write_release_summary(
        &summary_path,
        &[("closure".to_string(), ghost_path.display().to_string())],
    );

    let output = run_survey_with_summary(&summary_path);
    assert_success(
        &output,
        "survey must succeed even when summary references missing files",
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("remediation-required"));
    assert_eq!(report["total_gates"], json!(1));
    assert_eq!(report["missing"], json!(1));
    assert_eq!(report["signed_and_verified"], json!(0));
    let per_gate = report["per_gate"].as_array().expect("per_gate");
    assert_eq!(per_gate.len(), 1);
    let entry = &per_gate[0];
    assert_eq!(entry["signing_status"], json!("gate-file-missing"));
    assert_eq!(entry["signature_verified"], json!(false));
    assert_eq!(entry["sources"], json!(["release-summary"]));
    let remediation = entry["suggested_remediation"]
        .as_str()
        .expect("remediation string");
    assert!(
        remediation.contains("missing on disk"),
        "remediation should explain missing file: {remediation}"
    );
}

#[test]
fn survey_with_both_modes_dedupes_overlapping_gates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).expect("target");
    let key_path = temp.path().join("signing-key.pem");
    keygen(&key_path);

    let (gate_a_path, gate_a) = write_raw_gate(&target, "run-a", "closure");
    write_signed_wrapper(&target, &gate_a, 1_700_000_030_000, &key_path);
    let (gate_b_path, _gate_b) = write_raw_gate(&target, "run-b", "midpoint");

    let summary_path = temp.path().join("release-summary.json");
    write_release_summary(
        &summary_path,
        &[
            ("closure".to_string(), gate_a_path.display().to_string()),
            ("midpoint".to_string(), gate_b_path.display().to_string()),
        ],
    );

    let output = run_survey_with_both(&target, &summary_path);
    assert_success(&output, "survey with both flags must succeed");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["total_gates"], json!(2));
    assert_eq!(report["signed_and_verified"], json!(1));
    assert_eq!(report["unsigned"], json!(1));
    assert_eq!(
        report["sources"],
        json!(["runs-dir-scan", "release-summary"])
    );

    let per_gate = report["per_gate"].as_array().expect("per_gate");
    assert_eq!(per_gate.len(), 2, "duplicate paths must be deduplicated");
    for entry in per_gate {
        let sources = entry["sources"].as_array().expect("sources array");
        let labels: Vec<&str> = sources.iter().filter_map(|value| value.as_str()).collect();
        assert!(labels.contains(&"runs-dir-scan"));
        assert!(labels.contains(&"release-summary"));
    }
}

#[test]
fn survey_requires_target_or_summary() {
    let output = run_survey_no_args();
    assert!(
        !output.status.success(),
        "survey with neither flag must fail; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--target") || stderr.contains("--summary"),
        "error must mention required flags: {stderr}"
    );
}
