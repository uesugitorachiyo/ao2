//! Tests for `ao2 contract gate --support-signing-key`: the CLI flag that
//! lets non-workbench producers (the factory-v3 nightly script, etc.) emit
//! an AO2-signed `evidence-export-<ms>-obligation-gate.json` wrapper +
//! `.json.sig` + `workbench-evidence-signing-public.pem` next to a raw
//! obligation gate, so downstream verifiers can flag it as
//! `signed-and-verified` without operating a workbench HTTP serve loop.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

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

fn write_spec_and_ledger(target: &Path, spec_text: &str, ledger_path: &Path) {
    let spec = target.join("SPEC.md");
    fs::write(&spec, spec_text).expect("write spec");
    fs::write(
        target.join("README.md"),
        "The implementation note preserves: net = gross - fees\n",
    )
    .expect("write README");
    let extract = run_ao2(&[
        "contract",
        "extract",
        "--spec",
        spec.to_str().unwrap(),
        "--out",
        ledger_path.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&extract);
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

#[test]
fn contract_gate_default_on_rejects_unsigned_production() {
    // Slice 18: producer-side default-on. Without --support-signing-key
    // and without the --allow-unsigned-obligation-gates escape valve, the
    // CLI must fail-closed before writing the gate.
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target dir");
    let ledger = tmp.path().join("obligations.json");
    let gate = tmp.path().join("closure-gate.json");
    write_spec_and_ledger(
        &target,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
        &ledger,
    );

    let out = run_ao2(&[
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !out.status.success(),
        "default-on must reject unsigned production; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--support-signing-key by default")
            && stderr.contains("--allow-unsigned-obligation-gates"),
        "stderr must surface the escape-valve guidance; got: {stderr}"
    );
    assert!(
        !gate.exists(),
        "fail-closed must NOT write the raw gate; found {}",
        gate.display()
    );
}

#[test]
fn contract_gate_allow_unsigned_emits_unsigned_gate_only() {
    // Slice 18: explicit escape valve preserves the legacy unsigned path
    // for callers that have not yet provisioned a signing key. The gate
    // is still produced, just without a wrapper / sidecar.
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target dir");
    let ledger = tmp.path().join("obligations.json");
    let gate = tmp.path().join("closure-gate.json");
    write_spec_and_ledger(
        &target,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
        &ledger,
    );

    let out = run_ao2(&[
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        "--allow-unsigned-obligation-gates",
        "--json",
    ]);
    assert_success(&out);
    let report: Value = serde_json::from_slice(&out.stdout).expect("gate json");
    assert_eq!(report["status"], "passed");
    assert!(
        report.get("support_signing_evidence").is_none(),
        "escape valve must not emit support_signing_evidence block; got: {report}"
    );
    let parent = gate.parent().expect("gate parent");
    let wrapper_glob: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("evidence-export-") && name.ends_with("-obligation-gate.json")
        })
        .collect();
    assert!(
        wrapper_glob.is_empty(),
        "escape valve must not write any wrapper; found: {wrapper_glob:?}"
    );
    assert!(
        !parent
            .join("workbench-evidence-signing-public.pem")
            .exists(),
        "escape valve must not write public key sidecar"
    );
}

#[test]
fn contract_gate_with_support_signing_key_emits_signed_wrapper_next_to_gate() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target dir");
    let ledger = tmp.path().join("obligations.json");
    let gate = tmp.path().join("closure-gate.json");
    let key = tmp.path().join("signing-key.pem");
    write_spec_and_ledger(
        &target,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
        &ledger,
    );
    keygen(&key);

    let out = run_ao2(&[
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        "--support-signing-key",
        key.to_str().unwrap(),
        "--support-signer-id",
        "test-nightly-producer",
        "--support-operator-role",
        "operator",
        "--support-run-id",
        "nightly-test-run",
        "--json",
    ]);
    assert_success(&out);
    let report: Value = serde_json::from_slice(&out.stdout).expect("gate json");
    let signing = report
        .get("support_signing_evidence")
        .expect("support_signing_evidence present when --support-signing-key set");
    assert_eq!(
        signing["schema_version"],
        "ao2.contract-gate-support-signing-evidence.v1"
    );
    assert_eq!(signing["signature_verified"], true);
    assert_eq!(signing["signer_id"], "test-nightly-producer");
    assert_eq!(signing["signer_role"], "operator");
    assert_eq!(signing["signer_run_id"], "nightly-test-run");

    let wrapper_path = Path::new(signing["wrapper_path"].as_str().expect("wrapper_path"));
    let signature_path = Path::new(signing["signature_path"].as_str().expect("signature_path"));
    let public_key_path = Path::new(
        signing["public_key_path"]
            .as_str()
            .expect("public_key_path"),
    );
    assert!(wrapper_path.is_file(), "wrapper file missing");
    assert!(signature_path.is_file(), "signature sidecar missing");
    assert!(public_key_path.is_file(), "public key sidecar missing");
    assert_eq!(
        wrapper_path.parent().expect("wrapper parent"),
        gate.parent().expect("gate parent"),
        "default exports-dir is the same directory as --out"
    );
    assert_eq!(
        public_key_path.file_name().unwrap(),
        "workbench-evidence-signing-public.pem"
    );

    // Sanity: wrapper embeds the raw gate.
    let wrapper: Value = serde_json::from_str(&fs::read_to_string(wrapper_path).unwrap()).unwrap();
    let raw_gate: Value = serde_json::from_str(&fs::read_to_string(&gate).unwrap()).unwrap();
    assert_eq!(wrapper["export_kind"], "obligation-gate");
    assert_eq!(wrapper["export"]["gate"], raw_gate);
    assert_eq!(
        wrapper["export"]["audit_event"]["action"],
        "obligation_gate"
    );
    assert_eq!(
        wrapper["export"]["audit_event"]["operator_role"],
        "operator"
    );
}

#[test]
fn contract_gate_signed_wrapper_passes_verifier_via_gate_parent_dir_fallback() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target dir");
    let ledger = tmp.path().join("obligations.json");
    let gate = tmp.path().join("closure-gate.json");
    let key = tmp.path().join("signing-key.pem");
    write_spec_and_ledger(
        &target,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
        &ledger,
    );
    keygen(&key);

    let emit = run_ao2(&[
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        "--support-signing-key",
        key.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&emit);

    // No `--evidence-exports-dir` supplied; the verifier must walk the
    // gate's parent dir as a fallback to find the wrapper.
    let verify = run_ao2(&[
        "contract",
        "verify-obligation-gate-signing",
        "--gate",
        gate.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&verify);
    let report: Value = serde_json::from_slice(&verify.stdout).expect("verify json");
    assert_eq!(report["signing_status"], "signed-and-verified");
    assert_eq!(report["signature_verified"], true);
    assert_eq!(report["ao2_owned"], true);
    let exports_dir = report["evidence_exports_dir"]
        .as_str()
        .expect("exports_dir");
    assert_eq!(
        Path::new(exports_dir),
        gate.parent().expect("gate parent"),
        "verifier must report the gate's parent dir as the matched exports dir"
    );
}

#[test]
fn contract_gate_rejects_empty_signer_id_when_signing_key_set() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target dir");
    let ledger = tmp.path().join("obligations.json");
    let gate = tmp.path().join("closure-gate.json");
    let key = tmp.path().join("signing-key.pem");
    write_spec_and_ledger(
        &target,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
        &ledger,
    );
    keygen(&key);

    let out = run_ao2(&[
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        "--support-signing-key",
        key.to_str().unwrap(),
        "--support-signer-id",
        "   ",
        "--json",
    ]);
    assert!(
        !out.status.success(),
        "must reject empty --support-signer-id"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--support-signer-id must be non-empty"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn contract_gate_rejects_empty_operator_role_when_signing_key_set() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target dir");
    let ledger = tmp.path().join("obligations.json");
    let gate = tmp.path().join("closure-gate.json");
    let key = tmp.path().join("signing-key.pem");
    write_spec_and_ledger(
        &target,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
        &ledger,
    );
    keygen(&key);

    let out = run_ao2(&[
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        "--support-signing-key",
        key.to_str().unwrap(),
        "--support-operator-role",
        "   ",
        "--json",
    ]);
    assert!(
        !out.status.success(),
        "must reject empty --support-operator-role"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--support-operator-role must be non-empty"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn contract_gate_with_explicit_exports_dir_writes_wrapper_there() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).expect("target dir");
    let ledger = tmp.path().join("obligations.json");
    let gate = tmp.path().join("closure-gate.json");
    let key = tmp.path().join("signing-key.pem");
    let custom_exports = tmp.path().join("custom-exports");
    write_spec_and_ledger(
        &target,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
        &ledger,
    );
    keygen(&key);

    let out = run_ao2(&[
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        "--support-signing-key",
        key.to_str().unwrap(),
        "--exports-dir",
        custom_exports.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&out);
    let report: Value = serde_json::from_slice(&out.stdout).expect("gate json");
    let wrapper_path = Path::new(
        report["support_signing_evidence"]["wrapper_path"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        wrapper_path.parent().unwrap(),
        custom_exports,
        "explicit --exports-dir must override the default (gate-parent) location"
    );
    // Verifier finds it via the --evidence-exports-dir override (the
    // verifier's default fallback chain does NOT include this custom dir).
    let verify = run_ao2(&[
        "contract",
        "verify-obligation-gate-signing",
        "--gate",
        gate.to_str().unwrap(),
        "--evidence-exports-dir",
        custom_exports.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&verify);
    let verify_report: Value = serde_json::from_slice(&verify.stdout).expect("verify json");
    assert_eq!(verify_report["signing_status"], "signed-and-verified");
}
