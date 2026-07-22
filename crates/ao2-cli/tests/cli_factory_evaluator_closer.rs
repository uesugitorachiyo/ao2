use std::fs;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

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

fn sha256_path(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_factory_evaluator_rubric_produces_ao2_native_signed_acceptance_bar() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("governed-app.md");
    fs::write(
        &spec,
        r#"# Governed Notes App

Build a governed notes application with durable evidence.

Acceptance:
- AO2 derives the acceptance bar before provider execution.
- Verifier and closer outputs reference rubric_sha256.
- factory-v3 compares the result as a parity auditor only.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("evaluator-rubric-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out = temp.path().join("evaluator-rubric.json");

    let result = ao2([
        "factory",
        "evaluator-rubric",
        "--spec",
        spec.to_str().unwrap(),
        "--run-id",
        "governed-notes",
        "--verifier-command",
        "npm run verify",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "c82-rubric-test",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(result.status.success(), "{}", stderr(&result));
    let json: serde_json::Value = serde_json::from_str(&stdout(&result)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-evaluator-rubric-result.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["rubric_sha256"], sha256_path(&out));
    assert_eq!(
        json["rubric"]["schema_version"],
        "ao2.factory-evaluator-rubric.v1"
    );
    assert_eq!(
        json["rubric"]["release_acceptance"]["primary_owner"],
        "ao2 evaluator-closer"
    );
    assert_eq!(
        json["rubric"]["release_acceptance"]["factory_v3_role"],
        "parity_auditor"
    );
    assert_eq!(
        json["rubric"]["release_acceptance"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        json["rubric"]["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(
        json["rubric"]["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        json["rubric"]["trust_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert_eq!(
        json["rubric"]["downstream_contract"]["verifier_outputs_must_reference"],
        "rubric_sha256"
    );
    assert_eq!(
        json["rubric"]["signature"]["schema_version"],
        "ao2.factory-evaluator-rubric-signature.v1"
    );
    assert_eq!(json["rubric"]["signature"]["signature_status"], "signed");
    assert_eq!(json["rubric"]["signature"]["signature_verified"], true);
    assert!(out.is_file());
    assert!(Path::new(json["artifacts"]["signed_payload"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["signature"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["public_key"].as_str().unwrap()).is_file());
    assert!(!stdout(&result).contains("Bearer "));
}

#[test]
fn cli_factory_closer_decision_signs_and_verifies_rubric_bound_closure() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("governed-closer-app.md");
    fs::write(
        &spec,
        r#"# Governed Closer App

Acceptance:
- Verifier evidence references rubric_sha256.
- Release candidate evidence is accepted.
- Closure remains AO2-owned with factory-v3 as parity auditor.
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("closer-decision-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let rubric_path = temp.path().join("evaluator-rubric.json");
    let rubric = ao2([
        "factory",
        "evaluator-rubric",
        "--spec",
        spec.to_str().unwrap(),
        "--run-id",
        "governed-closer",
        "--verifier-command",
        "npm run verify",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--out",
        rubric_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(rubric.status.success(), "{}", stderr(&rubric));
    let rubric_sha256 = sha256_path(&rubric_path);

    let evidence_path = temp.path().join("release-candidate-verification.json");
    let evidence = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-verification.v1",
        "status": "passed",
        "summary_path": "target/plugin-release-candidate.json",
        "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
        "source_schema_version": "ao2.plugin-release-candidate.v1",
        "evidence_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
        "rubric_sha256": rubric_sha256,
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "provider_api_key_env_required": false
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
        },
        "side_effects": {
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_write_memory": false,
            "would_mutate_control_plane": false,
            "would_mutate_ao_artifacts": false,
            "would_approve_release": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&evidence).unwrap(),
    )
    .unwrap();
    let evidence_sha256 = sha256_path(&evidence_path);

    let manifest_path = temp.path().join("skill-contract-manifest.json");
    let manifest = serde_json::json!({
        "schema_version": "ao2.skill-contract-manifest.v1",
        "status": "accepted",
        "producer": "ao2",
        "entry_count": 1,
        "entries": [
            {
                "name": "closure_verification",
                "source_repo": "factory-v3",
                "source_path": "scripts/verify_closure.py",
                "source_sha256": "3333333333333333333333333333333333333333333333333333333333333333",
                "category": "runtime_critical",
                "ao2_disposition": "enforced",
                "enforcement": {
                    "ao2_command": "ao2 factory closer-decision",
                    "ao2_test": "cli_factory_closer_decision_signs_and_verifies_rubric_bound_closure",
                    "ao2_artifact": "ao2.factory-closer-decision.v1"
                },
                "blocker": null,
                "trust_boundary_notes": "AO2 signs closer decisions while factory-v3 remains a parity auditor."
            }
        ],
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "provider_api_key_env_required": false
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
        },
        "side_effects": {
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_write_memory": false,
            "would_mutate_control_plane": false,
            "would_mutate_ao_artifacts": false,
            "would_approve_release": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let manifest_sha256 = sha256_path(&manifest_path);

    let decision_path = temp.path().join("closer-decision.json");
    let decision = ao2([
        "factory",
        "closer-decision",
        "--rubric",
        rubric_path.to_str().unwrap(),
        "--rubric-sha256",
        &rubric_sha256,
        "--evidence",
        evidence_path.to_str().unwrap(),
        "--evidence-sha256",
        &evidence_sha256,
        "--skill-contract-manifest",
        manifest_path.to_str().unwrap(),
        "--skill-contract-manifest-sha256",
        &manifest_sha256,
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "closer-decision-test",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(decision.status.success(), "{}", stderr(&decision));
    let json: serde_json::Value = serde_json::from_str(&stdout(&decision)).unwrap();
    assert_eq!(json["schema_version"], "ao2.factory-closer-decision.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["decision"], "accepted");
    assert_eq!(json["rubric_sha256"], rubric_sha256);
    assert_eq!(json["evidence_sha256"], evidence_sha256);
    assert_eq!(json["skill_contract_manifest_sha256"], manifest_sha256);
    assert_eq!(json["closure_verification"]["ao2_disposition"], "enforced");
    assert_eq!(json["signature"]["signature_status"], "signed");
    assert_eq!(json["signature"]["signature_verified"], true);
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["token_safe_output_verified"], true);
    assert_eq!(json["decision_sha256"], sha256_path(&decision_path));

    let verification = ao2([
        "factory",
        "closer-decision-verify",
        "--decision",
        decision_path.to_str().unwrap(),
        "--decision-sha256",
        &sha256_path(&decision_path),
        "--json",
    ]);
    assert!(verification.status.success(), "{}", stderr(&verification));
    let verification_json: serde_json::Value =
        serde_json::from_str(&stdout(&verification)).unwrap();
    assert_eq!(
        verification_json["schema_version"],
        "ao2.factory-closer-decision-verification.v1"
    );
    assert_eq!(verification_json["status"], "accepted");
    assert_eq!(verification_json["signature_verified"], true);
    assert_eq!(verification_json["trust_boundary_ok"], true);
    assert_eq!(verification_json["rubric_linkage_verified"], true);

    let bad_digest = ao2([
        "factory",
        "closer-decision",
        "--rubric",
        rubric_path.to_str().unwrap(),
        "--rubric-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--evidence",
        evidence_path.to_str().unwrap(),
        "--evidence-sha256",
        &evidence_sha256,
        "--skill-contract-manifest",
        manifest_path.to_str().unwrap(),
        "--skill-contract-manifest-sha256",
        &manifest_sha256,
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("closer decision rubric sha256 mismatch"));
}
