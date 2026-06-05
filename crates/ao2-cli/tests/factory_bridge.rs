use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

const BRIDGE_SCHEMA: &str = "ao2.factory-bridge.v1";
const BRIDGE_ACTION: &str = "factory-bridge";
const MAPPING_SCHEMA: &str = "factory-v3/ao-operator-ao2-provider-contract/v1";
const MAPPING_VERSION: &str = "1.0.0";
// Pinned to the value emitted by
// factory-v3/scripts/ao_operator_ao2_provider_contract.py:digest on
// 2026-05-25. Drift fails this test so the Rust mapping cannot diverge from
// the Python source-of-truth digest without an explicit, paired update.
const EXPECTED_MAPPING_DIGEST: &str =
    "cda521f5bd1ae42f06ab2f44689161034fa8790163b020ba888719312635cd99";

fn run_bridge(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["factory", "bridge"])
        .args(args)
        .output()
        .expect("invoke ao2 factory bridge")
}

fn run_bridge_mapping(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["factory", "bridge-mapping"])
        .args(args)
        .output()
        .expect("invoke ao2 factory bridge-mapping")
}

fn run_verify_bridge_evidence(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["factory", "verify-bridge-evidence"])
        .args(args)
        .output()
        .expect("invoke ao2 factory verify-bridge-evidence")
}

fn sign_bridge_with_workbench_key(
    tmp: &Path,
    evidence_name: &str,
    signer_id: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let runspec_path = tmp.join(format!("{evidence_name}-runspec.yaml"));
    write_runspec(&runspec_path, &factory_v3_runspec());
    let signing_key = tmp.join(format!("{evidence_name}-key.pem"));
    let keygen = run_ao2(&[
        "workbench",
        "support-keygen",
        "--out",
        signing_key.to_str().unwrap(),
        "--bits",
        "2048",
        "--json",
    ]);
    assert_success(&keygen);
    let out_path = tmp.join(format!("{evidence_name}.json"));
    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        signer_id,
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);
    (out_path, signing_key)
}

fn run_ao2(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .output()
        .expect("invoke ao2")
}

fn write_runspec(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("runspec parent");
    }
    fs::write(path, serde_yaml::to_string(value).expect("yaml encode")).expect("runspec write");
}

fn write_role_contract(path: &Path, name: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("role contract parent");
    }
    fs::write(
        path,
        format!(
            r#"name = "{name}"
description = "Factory role contract fixture for {name}."
inputs = ["slice contract", "acceptance criteria"]
outputs = ["diff artifact", "test evidence"]
status_required = true
"#
        ),
    )
    .expect("role contract write");
}

fn factory_v3_runspec() -> Value {
    json!({
        "schema": "factory-v3/runspec/v1",
        "slug": "bug-fix",
        "roles": [
            {"id": "intake"},
            {"id": "planner"},
            {"id": "implementer"},
            {"id": "reviewer"},
            {"id": "evaluator-closer"},
        ],
    })
}

fn ao_dev_v1_runspec() -> Value {
    json!({
        "apiVersion": "ao.dev/v1",
        "kind": "Run",
        "metadata": {"name": "factory-v3-smoke"},
        "spec": {
            "tasks": [
                {
                    "id": "planner-intake",
                    "kind": "agent",
                    "deps": [],
                    "spec": {
                        "provider": "codex",
                        "agent": "codex-default",
                        "promptFile": "ao/prompts/planner-intake.md",
                        "workspace": ".",
                        "policyProfile": "ao/policy/local-dev.yaml"
                    }
                },
                {
                    "id": "plan-hardener",
                    "kind": "agent",
                    "deps": ["planner-intake"],
                    "spec": {
                        "provider": "codex",
                        "agent": "codex-default",
                        "promptFile": "ao/prompts/plan-hardener.md",
                        "workspace": ".",
                        "policyProfile": "ao/policy/local-dev.yaml"
                    }
                },
                {"id": "factory-manager", "kind": "agent", "deps": ["plan-hardener"]},
                {"id": "implementer-slice", "kind": "agent", "deps": ["factory-manager"]},
                {"id": "reviewer-slice", "kind": "agent", "deps": ["implementer-slice"]},
                {"id": "integrator", "kind": "agent", "deps": ["reviewer-slice"]},
                {
                    "id": "evaluator-closer",
                    "kind": "agent",
                    "deps": ["integrator"],
                    "spec": {
                        "provider": "codex",
                        "agent": "codex-default",
                        "promptFile": "ao/prompts/evaluator-closer.md",
                        "workspace": ".",
                        "policyProfile": "ao/policy/local-dev.yaml"
                    }
                },
            ]
        }
    })
}

fn bad_runspec() -> Value {
    json!({
        "schema": "factory-v3/runspec/v1",
        "slug": "bug-fix",
        "roles": [
            {"id": "implementer"},
            {"id": "ghost-role"},
        ],
    })
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bridge_emits_mapping_resolved_dry_run_evidence_for_factory_v3_runspec() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let work_request_path = tmp.path().join("work-request.json");
    fs::write(
        &work_request_path,
        serde_json::to_string_pretty(&json!({
            "classification": "MODERATE",
            "shape": "bug-fix",
            "problem": "Need AO2-native factory bridge parity.",
            "success_criteria": ["bridge emits governed run plan"],
            "constraints": ["factory-v3 remains parity oracle"],
            "sensitive_fields": ["environment"],
            "trigger_hints": ["factory-bridge"],
            "acceptance_criteria": [{"id": "AC-001", "oracle": "test", "verification": "cargo test"}],
            "slices": [{"id": "slice-01", "reads": ["runspec"], "writes": ["evidence"], "verification": ["cargo test"]}]
        }))
        .unwrap(),
    )
    .expect("work request write");
    let profile_path = tmp.path().join("profile.json");
    fs::write(
        &profile_path,
        serde_json::to_string_pretty(&json!({
            "schema": "factory-v3/profile/v1",
            "version": 1,
            "profile": "profile",
            "description": "AO2 profile compatibility fixture with provider keys only.",
            "common_instructions": ["Do not include secret values."],
            "roles": [
                {
                    "id": "intake",
                    "role": "Intake",
                    "provider_key": "FACTORY_V3_PLANNER_PROVIDER",
                    "deps": [],
                    "reads": ["task brief"],
                    "writes": ["docs/specs/<slug>-spec.md"],
                    "skills": ["skills/factory-intake/SKILL.md"],
                    "instructions": ["Classify request."],
                    "is_mutator": false
                },
                {
                    "id": "implementer",
                    "role": "Implementer",
                    "provider_key": "FACTORY_V3_IMPLEMENTER_PROVIDER",
                    "deps": ["planner"],
                    "reads": ["plan"],
                    "writes": ["scoped application artifacts"],
                    "skills": ["skills/factory-intake/SKILL.md"],
                    "instructions": ["Implement scoped change."],
                    "is_mutator": true
                }
            ]
        }))
        .unwrap(),
    )
    .expect("profile write");
    let out_path = tmp.path().join("bridge-evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--work-request",
        work_request_path.to_str().unwrap(),
        "--profile",
        profile_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    assert!(out_path.is_file(), "bridge evidence file must be written");
    let evidence_text = fs::read_to_string(&out_path).expect("read evidence");
    let evidence: Value = serde_json::from_str(&evidence_text).expect("evidence is JSON");

    assert_eq!(evidence["schema"], json!(BRIDGE_SCHEMA));
    assert_eq!(evidence["action"], json!(BRIDGE_ACTION));
    assert_eq!(evidence["status"], json!("mapping_resolved_dry_run"));
    assert_eq!(evidence["produced_at_ms"], json!(1_700_000_000_000_i64));
    assert_eq!(evidence["generated_at"], json!("2023-11-14T22:13:20Z"));
    assert_eq!(
        evidence["trust_boundary"]["factory_v3_role"],
        json!("parity_oracle_only")
    );
    assert_eq!(
        evidence["trust_boundary"]["bridge_owner"],
        json!("ao2_factory_bridge_subcommand")
    );

    let input_runspec = evidence["input_runspec"].as_object().unwrap();
    assert_eq!(
        input_runspec["path"],
        json!(runspec_path.to_str().unwrap().to_string())
    );
    assert_eq!(input_runspec["schema"], json!("factory-v3/runspec/v1"));
    assert_eq!(input_runspec["name"], json!("bug-fix"));
    assert!(
        input_runspec["sha256"].as_str().unwrap().len() == 64,
        "sha256 should be a 64-char hex string"
    );

    let mapping = evidence["mapping"].as_object().unwrap();
    assert_eq!(mapping["schema"], json!(MAPPING_SCHEMA));
    assert_eq!(mapping["version"], json!(MAPPING_VERSION));
    assert_eq!(mapping["digest"], json!(EXPECTED_MAPPING_DIGEST));

    let resolved = evidence["resolved_roles"].as_array().unwrap();
    let canonicals: Vec<&str> = resolved
        .iter()
        .map(|r| r["canonical_role"].as_str().unwrap())
        .collect();
    assert_eq!(
        canonicals,
        vec![
            "intake",
            "planner",
            "implementer",
            "reviewer",
            "evaluator_closer"
        ]
    );
    let unknown = evidence["unknown_roles"].as_array().unwrap();
    assert!(unknown.is_empty(), "unknown_roles must be empty");

    assert_eq!(
        evidence["work_request"]["classification"],
        json!("MODERATE")
    );
    assert_eq!(evidence["work_request"]["shape"], json!("bug-fix"));
    assert_eq!(
        evidence["work_request"]["classification_status"],
        json!("classified")
    );
    assert_eq!(
        evidence["profile_reference"]["path"],
        json!(profile_path.to_str().unwrap().to_string())
    );
    assert_eq!(
        evidence["profile_reference"]["schema"],
        json!("factory-v3/profile/v1")
    );
    assert_eq!(evidence["profile_reference"]["profile"], json!("profile"));
    assert_eq!(evidence["profile_reference"]["role_count"], json!(2));
    assert_eq!(
        evidence["profile_reference"]["factory_v3_required_to_load"],
        json!(false)
    );
    assert_eq!(
        evidence["profile_reference"]["provider_key_values_exposed"],
        json!(false)
    );
    assert_eq!(
        evidence["profile_reference"]["description_present"],
        json!(true)
    );
    assert!(
        evidence["profile_reference"]["description_sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64,
        "description text should be represented by digest only"
    );
    assert!(
        evidence["profile_reference"].get("description").is_none(),
        "profile description must not be emitted as raw evidence text"
    );
    assert_eq!(
        evidence["profile_reference"]["profile_role_contracts"]["implementer"]["provider_key"],
        json!("FACTORY_V3_IMPLEMENTER_PROVIDER")
    );
    assert_eq!(
        evidence["profile_reference"]["profile_role_contracts"]["implementer"]
            ["provider_auth_contract"],
        json!("local_oauth_cli_only_no_provider_api_keys")
    );
    assert_eq!(
        evidence["governed_run_plan"]["schema"],
        json!("ao2.governed-run-plan.v1")
    );
    assert_eq!(
        evidence["governed_run_plan"]["status"],
        json!("materialized_dry_run")
    );
    assert_eq!(
        evidence["governed_run_plan"]["decision_owner"],
        json!("ao2_native_evaluator_closer")
    );
    assert_eq!(
        evidence["governed_run_plan"]["factory_v3_decision_owner"],
        json!("parity_oracle_only")
    );
    let plan_tasks = evidence["governed_run_plan"]["tasks"].as_array().unwrap();
    assert_eq!(plan_tasks.len(), resolved.len());
    assert_eq!(plan_tasks[0]["sequence"], json!(1));
    assert_eq!(plan_tasks[0]["canonical_role"], json!("intake"));
    assert_eq!(
        plan_tasks[4]["provider_contract"],
        json!("ao2.provider-contract.evaluator-closer.v1")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["owner"],
        json!("ao2")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["adapter_family"],
        json!("local_scripted")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["auth_contract"],
        json!("local_scripted_no_secret_env_values")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["evidence_contract"],
        json!("implementation_digest_patch_and_test_evidence")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["concern_contract"],
        json!("concerns_recorded_or_empty")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["blocker_contract"],
        json!("blockers_resolved_or_explicitly_blocked")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["changed_files_contract"],
        json!("changed_files_digest_recorded")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["secret_redaction_contract"],
        json!("secret_redaction_summary_recorded")
    );
    assert_eq!(
        plan_tasks[2]["provider_adapter_contract"]["sandbox_contract"],
        json!("scoped_write_with_digest_patch_and_repair_budget")
    );
    assert_eq!(
        plan_tasks[2]["profile_role_ref"]["provider_key"],
        json!("FACTORY_V3_IMPLEMENTER_PROVIDER")
    );
    assert_eq!(
        plan_tasks[2]["profile_role_ref"]["provider_key_value_exposed"],
        json!(false)
    );
}

#[test]
fn bridge_profile_reference_rejects_secret_like_profile_values() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let profile_path = tmp.path().join("profile.json");
    fs::write(
        &profile_path,
        serde_json::to_string_pretty(&json!({
            "schema": "factory-v3/profile/v1",
            "version": 1,
            "profile": "profile",
            "roles": [{
                "id": "implementer",
                "provider_key": "sk-not-a-provider-env-key",
                "replay_command": ["codex --token=super-secret"],
                "deterministic": true
            }]
        }))
        .unwrap(),
    )
    .expect("profile write");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--profile",
        profile_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);

    assert!(
        !output.status.success(),
        "secret-like profile value must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("provider_key must be a non-secret env-var name"),
        "stderr should explain provider key validation without echoing the secret-like value: {stderr}"
    );
    assert!(
        !stderr.contains("sk-not-a-provider-env-key") && !stderr.contains("super-secret"),
        "failure output must not echo secret-like profile values: {stderr}"
    );
}

#[test]
fn bridge_profile_reference_rejects_duplicate_and_malformed_roles() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let duplicate_profile_path = tmp.path().join("duplicate-profile.json");
    fs::write(
        &duplicate_profile_path,
        serde_json::to_string_pretty(&json!({
            "schema": "factory-v3/profile/v1",
            "profile": "profile",
            "roles": [
                {"id": "implementer", "provider_key": "FACTORY_V3_IMPLEMENTER_PROVIDER"},
                {"id": "implementer", "provider_key": "FACTORY_V3_IMPLEMENTER_PROVIDER"}
            ]
        }))
        .unwrap(),
    )
    .expect("profile write");
    let duplicate_output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--profile",
        duplicate_profile_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!duplicate_output.status.success());
    assert!(
        String::from_utf8_lossy(&duplicate_output.stderr).contains("duplicate role id implementer")
    );

    let malformed_profile_path = tmp.path().join("malformed-profile.json");
    fs::write(
        &malformed_profile_path,
        serde_json::to_string_pretty(&json!({
            "schema": "factory-v3/profile/v1",
            "profile": "profile",
            "roles": [{"role": "Implementer", "provider_key": "FACTORY_V3_IMPLEMENTER_PROVIDER"}]
        }))
        .unwrap(),
    )
    .expect("profile write");
    let malformed_output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--profile",
        malformed_profile_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!malformed_output.status.success());
    assert!(String::from_utf8_lossy(&malformed_output.stderr).contains("missing string id"));
}

#[test]
fn bridge_profile_role_ref_matches_canonical_role_aliases() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(
        &runspec_path,
        &json!({
            "schema": "factory-v3/runspec/v1",
            "slug": "alias-profile-ref",
            "roles": [{"id": "implementer-slice", "kind": "agent"}]
        }),
    );
    let profile_path = tmp.path().join("profile.json");
    fs::write(
        &profile_path,
        serde_json::to_string_pretty(&json!({
            "schema": "factory-v3/profile/v1",
            "profile": "profile",
            "roles": [{"id": "implementer", "provider_key": "FACTORY_V3_IMPLEMENTER_PROVIDER"}]
        }))
        .unwrap(),
    )
    .expect("profile write");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--profile",
        profile_path.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&output);
    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    let task = &evidence["governed_run_plan"]["tasks"][0];
    assert_eq!(task["role_id"], json!("implementer-slice"));
    assert_eq!(task["canonical_role"], json!("implementer"));
    assert_eq!(task["profile_role_ref"]["id"], json!("implementer"));
}

#[test]
fn bridge_classifies_plaintext_work_request_without_factory_driver() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let work_request_path = tmp.path().join("work-request.md");
    fs::write(
        &work_request_path,
        "AO2 should replace factory-v3 for governed execution parity across Windows, macOS, and Ubuntu. Materialize RunSpec/profile compatibility evidence without factory-v3 classifying first.\n",
    )
    .expect("work request write");
    let out_path = tmp.path().join("bridge-evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--work-request",
        work_request_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(evidence["work_request"]["classification"], json!("COMPLEX"));
    assert_eq!(evidence["work_request"]["shape"], json!("refactor"));
    assert_eq!(
        evidence["work_request"]["classification_status"],
        json!("classified")
    );
    assert_eq!(
        evidence["work_request"]["classification_owner"],
        json!("ao2-native-classifier")
    );
    assert_eq!(
        evidence["work_request"]["factory_v3_required_before_classification"],
        json!(false)
    );
    let signals = evidence["work_request"]["classification_signals"]
        .as_array()
        .unwrap();
    assert!(signals.iter().any(|signal| signal == "replacement_parity"));
    assert!(signals.iter().any(|signal| signal == "three_os_or_windows"));
}

#[test]
fn bridge_normalizes_small_medium_large_json_size_aliases() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let work_request_path = tmp.path().join("work-request.json");
    fs::write(
        &work_request_path,
        serde_json::to_string_pretty(&json!({
            "size": "medium",
            "shape": "bug_fix",
            "problem": "Factory-style size aliases should not require a Python classifier."
        }))
        .unwrap(),
    )
    .expect("work request write");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--work-request",
        work_request_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(
        evidence["work_request"]["classification"],
        json!("MODERATE")
    );
    assert_eq!(evidence["work_request"]["shape"], json!("bug-fix"));
    assert_eq!(
        evidence["work_request"]["classification_status"],
        json!("classified")
    );
}

#[test]
fn bridge_can_sign_mapping_evidence_without_factory_driver() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let work_request_path = tmp.path().join("work-request.json");
    fs::write(
        &work_request_path,
        serde_json::to_string_pretty(&json!({
            "classification": "SMALL",
            "shape": "greenfield",
            "problem": "Sign AO2-native factory bridge evidence before control-plane observation."
        }))
        .unwrap(),
    )
    .expect("work request write");
    let signing_key = tmp.path().join("bridge-signing-key.pem");
    let keygen = run_ao2(&[
        "workbench",
        "support-keygen",
        "--out",
        signing_key.to_str().unwrap(),
        "--bits",
        "2048",
        "--json",
    ]);
    assert_success(&keygen);
    let out_path = tmp.path().join("signed-bridge-evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--work-request",
        work_request_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "factory-bridge-test-signer",
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let stdout_json: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    let evidence: Value =
        serde_json::from_str(&fs::read_to_string(&out_path).expect("signed evidence file"))
            .expect("evidence json");
    assert_eq!(stdout_json, evidence);
    assert_eq!(
        evidence["signed_evidence_status"],
        json!("signed-and-verified-bridge-evidence")
    );
    assert_eq!(
        evidence["signature"]["schema_version"],
        json!("ao2.factory-bridge-evidence-signature.v1")
    );
    assert_eq!(
        evidence["signature"]["signer_id"],
        json!("factory-bridge-test-signer")
    );
    assert_eq!(evidence["signature"]["signature_verified"], json!(true));
    assert!(Path::new(
        evidence["signature"]["signed_payload_path"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(evidence["signature"]["signature_path"].as_str().unwrap()).is_file());
    assert!(Path::new(evidence["signature"]["public_key_path"].as_str().unwrap()).is_file());
    assert_eq!(
        evidence["trust_boundary"]["factory_v3_role"],
        json!("parity_oracle_only")
    );
    assert_eq!(
        evidence["governed_run_plan"]["status"],
        json!("materialized_dry_run")
    );
}

#[test]
fn bridge_requires_out_when_signing_for_stable_sidecar_paths() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let signing_key = tmp.path().join("bridge-signing-key.pem");
    let keygen = run_ao2(&[
        "workbench",
        "support-keygen",
        "--out",
        signing_key.to_str().unwrap(),
        "--bits",
        "2048",
        "--json",
    ]);
    assert_success(&keygen);

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--json",
    ]);
    assert!(!output.status.success(), "bridge should fail without --out");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--signing-key requires --out"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bridge_signature_does_not_verify_against_tampered_signed_payload() {
    use rsa::pkcs1v15::{Signature as RsaPkcs1v15Signature, VerifyingKey};
    use rsa::pkcs8::DecodePublicKey;
    use rsa::RsaPublicKey;
    use sha2::{Digest, Sha256};
    use signature::Verifier;

    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let signing_key = tmp.path().join("bridge-signing-key.pem");
    let keygen = run_ao2(&[
        "workbench",
        "support-keygen",
        "--out",
        signing_key.to_str().unwrap(),
        "--bits",
        "2048",
        "--json",
    ]);
    assert_success(&keygen);
    let out_path = tmp.path().join("signed-bridge-evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "tamper-test-signer",
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value =
        serde_json::from_str(&fs::read_to_string(&out_path).expect("read out")).unwrap();
    let signed_payload_path = Path::new(
        evidence["signature"]["signed_payload_path"]
            .as_str()
            .unwrap(),
    )
    .to_path_buf();
    let signature_path =
        Path::new(evidence["signature"]["signature_path"].as_str().unwrap()).to_path_buf();
    let public_key_path =
        Path::new(evidence["signature"]["public_key_path"].as_str().unwrap()).to_path_buf();
    let recorded_payload_sha = evidence["signature"]["signed_payload_sha256"]
        .as_str()
        .expect("payload sha")
        .to_string();

    let signature_bytes = fs::read(&signature_path).expect("read sig");
    let public_pem = fs::read_to_string(&public_key_path).expect("read pub");
    let public_key = RsaPublicKey::from_public_key_pem(&public_pem).expect("parse public key");
    let verifying_key = VerifyingKey::<Sha256>::new(public_key);
    let signature_obj =
        RsaPkcs1v15Signature::try_from(&signature_bytes[..]).expect("parse signature");

    let original_bytes = fs::read(&signed_payload_path).expect("read original payload");
    let original_sha = format!("{:x}", sha2::Sha256::digest(&original_bytes));
    assert_eq!(
        original_sha, recorded_payload_sha,
        "recorded sha must match canonical signed payload bytes"
    );
    verifying_key
        .verify(&original_bytes, &signature_obj)
        .expect("untampered signature must verify");

    let mut tampered: Value =
        serde_json::from_slice(&original_bytes).expect("payload parses as json");
    tampered["trust_boundary"]["factory_v3_role"] = json!("compromised-attacker-controlled");
    fs::write(
        &signed_payload_path,
        serde_json::to_string_pretty(&tampered).expect("encode tampered") + "\n",
    )
    .expect("write tampered payload");
    let tampered_bytes = fs::read(&signed_payload_path).expect("read tampered");
    assert_ne!(
        original_bytes, tampered_bytes,
        "tampering must change the bytes on disk"
    );
    let tampered_sha = format!("{:x}", sha2::Sha256::digest(&tampered_bytes));
    assert_ne!(
        recorded_payload_sha, tampered_sha,
        "tampered sha must diverge from the recorded signed payload sha"
    );

    let verify_result = verifying_key.verify(&tampered_bytes, &signature_obj);
    assert!(
        verify_result.is_err(),
        "RSA/SHA-256 verify must fail against tampered signed payload"
    );
}

#[test]
fn bridge_handles_ao_dev_v1_runspec_and_full_role_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("smoke.yaml");
    write_runspec(&runspec_path, &ao_dev_v1_runspec());
    let out_path = tmp.path().join("bridge-evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(evidence["status"], json!("mapping_resolved_dry_run"));
    assert_eq!(evidence["input_runspec"]["schema"], json!("ao.dev/v1"));
    assert_eq!(evidence["input_runspec"]["name"], json!("factory-v3-smoke"));

    let resolved = evidence["resolved_roles"].as_array().unwrap();
    let canonicals: Vec<&str> = resolved
        .iter()
        .map(|r| r["canonical_role"].as_str().unwrap())
        .collect();
    assert_eq!(
        canonicals,
        vec![
            "intake",
            "plan_hardener",
            "factory_manager",
            "implementer",
            "reviewer",
            "integrator",
            "evaluator_closer",
        ]
    );
}

#[test]
fn bridge_plan_preserves_factory_runspec_dependencies_and_profile_fields() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("smoke.yaml");
    write_runspec(&runspec_path, &ao_dev_v1_runspec());

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    let tasks = evidence["governed_run_plan"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 7);

    assert_eq!(tasks[0]["role_id"], json!("planner-intake"));
    assert_eq!(tasks[0]["depends_on"], json!([]));
    assert_eq!(tasks[0]["factory_runspec_task"]["kind"], json!("agent"));
    assert_eq!(tasks[0]["factory_runspec_task"]["provider"], json!("codex"));
    assert_eq!(
        tasks[0]["factory_runspec_task"]["agent"],
        json!("codex-default")
    );
    assert_eq!(
        tasks[0]["factory_runspec_task"]["prompt_file"],
        json!("ao/prompts/planner-intake.md")
    );
    assert_eq!(
        tasks[0]["factory_runspec_task"]["policy_profile"],
        json!("ao/policy/local-dev.yaml")
    );
    assert_eq!(tasks[0]["factory_runspec_task"]["workspace"], json!("."));
    assert_eq!(
        tasks[0]["provider_adapter_contract"]["adapter_family"],
        json!("codex_cli_oauth")
    );
    assert_eq!(
        tasks[0]["provider_adapter_contract"]["auth_contract"],
        json!("local_oauth_cli_only_no_provider_api_keys")
    );
    assert_eq!(
        tasks[0]["provider_adapter_contract"]["provider_api_key_auth_allowed"],
        json!(false)
    );

    assert_eq!(tasks[1]["role_id"], json!("plan-hardener"));
    assert_eq!(tasks[1]["depends_on"], json!(["planner-intake"]));
    assert_eq!(
        tasks[1]["factory_runspec_task"]["prompt_file"],
        json!("ao/prompts/plan-hardener.md")
    );

    assert_eq!(tasks[6]["role_id"], json!("evaluator-closer"));
    assert_eq!(tasks[6]["depends_on"], json!(["integrator"]));
    assert_eq!(
        tasks[6]["factory_runspec_task"]["prompt_file"],
        json!("ao/prompts/evaluator-closer.md")
    );
}

#[test]
fn bridge_plan_materializes_ao2_native_midpoint_and_closure_gates() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("smoke.yaml");
    write_runspec(&runspec_path, &ao_dev_v1_runspec());

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    let plan = &evidence["governed_run_plan"];
    assert_eq!(plan["decision_owner"], json!("ao2_native_evaluator_closer"));
    assert_eq!(
        plan["factory_v3_decision_owner"],
        json!("parity_oracle_only")
    );

    let gates = plan["native_gates"].as_array().expect("native gate plan");
    assert_eq!(gates.len(), 2);
    let midpoint = &gates[0];
    assert_eq!(midpoint["stage"], json!("midpoint"));
    assert_eq!(midpoint["owner"], json!("ao2_native_evaluator_closer"));
    assert_eq!(midpoint["factory_v3_role"], json!("parity_oracle_only"));
    assert_eq!(midpoint["status"], json!("planned"));
    assert_eq!(midpoint["required_before_roles"], json!(["reviewer-slice"]));
    assert_eq!(
        midpoint["required_evidence"],
        json!(["implementation_digest_patch_and_test_evidence"])
    );
    assert_eq!(
        midpoint["decision_logic"],
        json!("continue_when_required_evidence_present_and_no_open_blockers_else_repair_or_block")
    );
    assert_eq!(midpoint["emits"], json!("ao2.obligation-gate.midpoint.v1"));

    let closure = &gates[1];
    assert_eq!(closure["stage"], json!("closure"));
    assert_eq!(closure["owner"], json!("ao2_native_evaluator_closer"));
    assert_eq!(closure["factory_v3_role"], json!("parity_oracle_only"));
    assert_eq!(closure["status"], json!("planned"));
    assert_eq!(
        closure["required_after_roles"],
        json!([
            "planner-intake",
            "plan-hardener",
            "factory-manager",
            "implementer-slice",
            "reviewer-slice",
            "integrator",
            "evaluator-closer"
        ])
    );
    assert_eq!(
        closure["required_evidence"],
        json!([
            "role_evidence_obligations_satisfied",
            "concerns_recorded_or_empty",
            "blockers_resolved_or_explicitly_blocked",
            "changed_files_digest_recorded",
            "secret_redaction_summary_recorded"
        ])
    );
    assert_eq!(
        closure["decision_logic"],
        json!(
            "accept_when_all_role_obligations_satisfied_and_no_open_blockers_else_repair_or_reject"
        )
    );
    assert_eq!(closure["emits"], json!("ao2.evaluator-closer-decision.v1"));
}

#[test]
fn bridge_plan_loads_factory_role_contracts_into_ao2_tasks() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("smoke.yaml");
    write_runspec(&runspec_path, &ao_dev_v1_runspec());
    let contracts_dir = tmp.path().join("agents");
    write_role_contract(&contracts_dir.join("implementer.toml"), "implementer");
    write_role_contract(
        &contracts_dir.join("evaluator-closer.toml"),
        "evaluator-closer",
    );

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--role-contracts-dir",
        contracts_dir.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    let role_contracts = evidence["role_contracts"]
        .as_object()
        .expect("role contracts block");
    assert_eq!(role_contracts["owner"], json!("ao2"));
    assert_eq!(role_contracts["factory_v3_required_to_load"], json!(false));
    assert_eq!(role_contracts["loaded_count"], json!(2));
    assert_eq!(
        role_contracts["missing_roles"],
        json!([
            "planner-intake",
            "plan-hardener",
            "factory-manager",
            "reviewer-slice",
            "integrator"
        ])
    );

    let tasks = evidence["governed_run_plan"]["tasks"].as_array().unwrap();
    let implementer = tasks
        .iter()
        .find(|task| task["canonical_role"] == json!("implementer"))
        .expect("implementer task");
    assert_eq!(
        implementer["role_contract_ref"]["contract_status"],
        json!("loaded")
    );
    assert_eq!(
        implementer["role_contract_ref"]["name"],
        json!("implementer")
    );
    assert_eq!(
        implementer["role_contract_ref"]["inputs"],
        json!(["slice contract", "acceptance criteria"])
    );
    assert_eq!(
        implementer["role_contract_ref"]["outputs"],
        json!(["diff artifact", "test evidence"])
    );
    assert_eq!(
        implementer["role_contract_ref"]["status_required"],
        json!(true)
    );
    assert!(
        implementer["role_contract_ref"]["sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64,
        "role contract sha256 must be recorded"
    );

    let evaluator = tasks
        .iter()
        .find(|task| task["canonical_role"] == json!("evaluator_closer"))
        .expect("evaluator task");
    assert_eq!(
        evaluator["role_contract_ref"]["path"],
        json!(contracts_dir
            .join("evaluator-closer.toml")
            .to_str()
            .unwrap()
            .to_string())
    );
}

#[test]
fn bridge_plan_auto_discovers_factory_v3_agents_dir_from_runspec_layout() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("factory-v3");
    let runspec_path = repo
        .join("ao")
        .join("runspecs")
        .join("factory-v3-smoke.yaml");
    write_runspec(&runspec_path, &ao_dev_v1_runspec());
    let contracts_dir = repo.join("agents");
    write_role_contract(&contracts_dir.join("implementer.toml"), "implementer");
    write_role_contract(
        &contracts_dir.join("evaluator-closer.toml"),
        "evaluator-closer",
    );

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    let role_contracts = evidence["role_contracts"]
        .as_object()
        .expect("role contracts block");
    assert_eq!(role_contracts["owner"], json!("ao2"));
    assert_eq!(
        role_contracts["path"],
        json!(contracts_dir.to_str().unwrap())
    );
    assert_eq!(
        role_contracts["discovery"],
        json!("auto_discovered_from_ao_runspec_layout")
    );
    assert_eq!(role_contracts["loaded_count"], json!(2));

    let tasks = evidence["governed_run_plan"]["tasks"].as_array().unwrap();
    let implementer = tasks
        .iter()
        .find(|task| task["canonical_role"] == json!("implementer"))
        .expect("implementer task");
    assert_eq!(
        implementer["role_contract_ref"]["contract_status"],
        json!("loaded")
    );
    assert_eq!(
        implementer["role_contract_ref"]["path"],
        json!(contracts_dir
            .join("implementer.toml")
            .to_str()
            .unwrap()
            .to_string())
    );
}

#[test]
fn bridge_plan_handles_numbered_slice_fan_out_runspec_end_to_end() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("fan-out.yaml");
    let runspec = json!({
        "apiVersion": "ao.dev/v1",
        "kind": "Run",
        "metadata": {"name": "fan-out-smoke"},
        "spec": {
            "tasks": [
                {"id": "planner-intake", "kind": "agent", "deps": []},
                {"id": "plan-hardener", "kind": "agent", "deps": ["planner-intake"]},
                {"id": "factory-manager", "kind": "agent", "deps": ["plan-hardener"]},
                {"id": "implementer-slice-1", "kind": "agent", "deps": ["factory-manager"]},
                {"id": "implementer-slice-2", "kind": "agent", "deps": ["factory-manager"]},
                {"id": "implementer-slice-3", "kind": "agent", "deps": ["factory-manager"]},
                {"id": "reviewer-slice-1", "kind": "agent", "deps": ["implementer-slice-1"]},
                {"id": "reviewer-slice-2", "kind": "agent", "deps": ["implementer-slice-2"]},
                {"id": "integrator", "kind": "agent", "deps": ["reviewer-slice-1"]},
                {"id": "evaluator-closer", "kind": "agent", "deps": ["integrator"]},
            ],
        },
    });
    write_runspec(&runspec_path, &runspec);

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(evidence["status"], json!("mapping_resolved_dry_run"));
    assert_eq!(evidence["unknown_roles"], json!([]));
    let canonical_roles: Vec<&str> = evidence["resolved_roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["canonical_role"].as_str().unwrap())
        .collect();
    assert_eq!(
        canonical_roles,
        vec![
            "intake",
            "plan_hardener",
            "factory_manager",
            "implementer",
            "implementer",
            "implementer",
            "reviewer",
            "reviewer",
            "integrator",
            "evaluator_closer",
        ]
    );
    // Mapping digest is unaffected by the canonical_role function-body change.
    let observed_digest = evidence["mapping"]["digest"].as_str().unwrap();
    assert_eq!(observed_digest.len(), 64);
}

#[test]
fn bridge_rejects_non_numeric_suffixes_as_unknown_roles() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("bad.yaml");
    let runspec = json!({
        "apiVersion": "ao.dev/v1",
        "kind": "Run",
        "metadata": {"name": "bad-suffixes"},
        "spec": {
            "tasks": [
                // Valid: numeric suffix stripped.
                {"id": "implementer-slice-1", "kind": "agent", "deps": []},
                // Invalid: alphabetic suffix not stripped.
                {"id": "implementer-slice-a", "kind": "agent", "deps": []},
                // Invalid: no stem before the numeric suffix.
                {"id": "-1", "kind": "agent", "deps": []},
                // Invalid: no recognized stem.
                {"id": "foo-bar", "kind": "agent", "deps": []},
            ],
        },
    });
    write_runspec(&runspec_path, &runspec);
    let out_path = tmp.path().join("evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
    ]);
    // bridge exits non-zero when any role is unknown.
    assert!(!output.status.success());
    let evidence: Value = serde_json::from_slice(
        &std::fs::read(&out_path).expect("evidence written even when blocked"),
    )
    .expect("evidence is JSON");
    assert_eq!(evidence["status"], json!("blocked_unknown_roles"));
    let mut unknown: Vec<&str> = evidence["unknown_roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    unknown.sort();
    assert_eq!(
        unknown,
        vec!["-1", "foo-bar", "implementer-slice-a"],
        "non-numeric suffixes and bare numeric ids must not canonicalize"
    );
}

#[test]
fn bridge_midpoint_gate_selects_reviewer_by_canonical_role_not_substring() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("smoke.yaml");
    let mut runspec = ao_dev_v1_runspec();
    runspec["spec"]["tasks"][4]["id"] = json!("Reviewer-Slice");
    write_runspec(&runspec_path, &runspec);

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
        "--json",
    ]);
    assert_success(&output);

    let evidence: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    let gates = evidence["governed_run_plan"]["native_gates"]
        .as_array()
        .expect("native gate plan");
    let midpoint = &gates[0];
    assert_eq!(midpoint["status"], json!("planned"));
    assert_eq!(midpoint["required_before_roles"], json!(["Reviewer-Slice"]));
}

#[test]
fn bridge_blocks_on_unknown_role_with_nonzero_exit_and_writes_evidence() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("bad.yaml");
    write_runspec(&runspec_path, &bad_runspec());
    let out_path = tmp.path().join("bridge-evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
    ]);
    assert!(!output.status.success(), "bridge must fail on unknown role");
    assert!(out_path.is_file(), "evidence must still be persisted");

    let evidence: Value =
        serde_json::from_str(&fs::read_to_string(&out_path).expect("read evidence"))
            .expect("evidence is JSON");
    assert_eq!(evidence["status"], json!("blocked_unknown_roles"));
    let unknown = evidence["unknown_roles"].as_array().unwrap();
    let unknown_strs: Vec<&str> = unknown.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(unknown_strs, vec!["ghost-role"]);
    let resolved = evidence["resolved_roles"].as_array().unwrap();
    let canonicals: Vec<&str> = resolved
        .iter()
        .map(|r| r["canonical_role"].as_str().unwrap())
        .collect();
    assert_eq!(canonicals, vec!["implementer"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ghost-role"),
        "stderr should mention the unknown role: {stderr}"
    );
}

#[test]
fn bridge_default_text_output_summarizes_evidence_without_json_flag() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let out_path = tmp.path().join("bridge-evidence.json");

    let output = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
    ]);
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("schema=ao2.factory-bridge.v1"));
    assert!(stdout.contains("status=mapping_resolved_dry_run"));
    assert!(stdout.contains(&format!("mapping_digest={EXPECTED_MAPPING_DIGEST}")));
    assert!(stdout.contains("resolved_role_count=5"));
    assert!(stdout.contains("unknown_role_count=0"));
}

#[test]
fn bridge_evidence_is_byte_for_byte_deterministic_for_pinned_inputs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let runspec_path = tmp.path().join("runspec.yaml");
    write_runspec(&runspec_path, &factory_v3_runspec());
    let first = tmp.path().join("first.json");
    let second = tmp.path().join("second.json");

    let output_first = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        first.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
    ]);
    assert_success(&output_first);
    let output_second = run_bridge(&[
        "--runspec",
        runspec_path.to_str().unwrap(),
        "--out",
        second.to_str().unwrap(),
        "--now-ms",
        "1700000000000",
    ]);
    assert_success(&output_second);

    let first_text = fs::read_to_string(&first).expect("read first");
    let second_text = fs::read_to_string(&second).expect("read second");
    assert_eq!(
        first_text, second_text,
        "evidence must be byte-for-byte deterministic when runspec and now-ms are pinned"
    );
}

#[test]
fn bridge_mapping_digest_matches_python_module_pinned_value() {
    let output = run_bridge_mapping(&["--digest"]);
    assert_success(&output);
    let digest = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(digest, EXPECTED_MAPPING_DIGEST);
}

#[test]
fn bridge_mapping_table_emits_full_table_with_expected_top_keys() {
    let output = run_bridge_mapping(&[]);
    assert_success(&output);
    let table: Value = serde_json::from_slice(&output.stdout).expect("table is JSON");
    let object = table.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "ao2_provider_contracts",
            "canonical_roles",
            "mapping_version",
            "role_aliases",
            "schema",
            "trust_boundary",
        ]
    );
    assert_eq!(table["schema"], json!(MAPPING_SCHEMA));
    assert_eq!(table["mapping_version"], json!(MAPPING_VERSION));
    let contracts = table["ao2_provider_contracts"].as_object().unwrap();
    assert!(contracts.contains_key("evaluator_closer"));
    assert!(contracts.contains_key("implementer"));
}

#[test]
fn verify_bridge_evidence_accepts_signed_evidence_using_body_sidecar_paths() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (evidence_path, _key) =
        sign_bridge_with_workbench_key(tmp.path(), "verify-default", "verify-default-signer");

    let output =
        run_verify_bridge_evidence(&["--evidence", evidence_path.to_str().unwrap(), "--json"]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(
        report["schema_version"],
        json!("ao2.factory-bridge-evidence-verification.v1")
    );
    assert_eq!(report["status"], json!("accepted"));
    assert_eq!(report["signature_status"], json!("signed"));
    assert_eq!(report["signature_verified"], json!(true));
    assert_eq!(report["evidence_body_matches_signed_payload"], json!(true));
    assert_eq!(report["trust_boundary_ok"], json!(true));
    assert_eq!(report["mapping_digest_ok"], json!(true));
    assert_eq!(report["mapping_digest"], json!(EXPECTED_MAPPING_DIGEST));
    assert_eq!(
        report["signed_payload_marker"],
        json!("bridge_evidence_without_signature_field")
    );
    assert_eq!(
        report["signed_evidence_status"],
        json!("signed-and-verified-bridge-evidence")
    );
    assert_eq!(
        report["ao2_decision_owner"],
        json!("ao2-native-bridge-evidence-verifier")
    );
}

#[test]
fn verify_bridge_evidence_accepts_when_sidecar_paths_overridden_to_relocated_copies() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (evidence_path, _key) =
        sign_bridge_with_workbench_key(tmp.path(), "verify-override", "verify-override-signer");
    let evidence: Value =
        serde_json::from_str(&fs::read_to_string(&evidence_path).expect("read evidence")).unwrap();
    let original_signed_payload = Path::new(
        evidence["signature"]["signed_payload_path"]
            .as_str()
            .unwrap(),
    )
    .to_path_buf();
    let original_signature =
        Path::new(evidence["signature"]["signature_path"].as_str().unwrap()).to_path_buf();
    let original_public_key =
        Path::new(evidence["signature"]["public_key_path"].as_str().unwrap()).to_path_buf();

    let relocated_dir = tmp.path().join("relocated");
    fs::create_dir_all(&relocated_dir).expect("relocated dir");
    let relocated_signed_payload = relocated_dir.join("signed-payload.json");
    let relocated_signature = relocated_dir.join("signature.sig");
    let relocated_public_key = relocated_dir.join("public.pem");
    fs::copy(&original_signed_payload, &relocated_signed_payload).expect("copy signed payload");
    fs::copy(&original_signature, &relocated_signature).expect("copy signature");
    fs::copy(&original_public_key, &relocated_public_key).expect("copy public key");

    let output = run_verify_bridge_evidence(&[
        "--evidence",
        evidence_path.to_str().unwrap(),
        "--signed-payload",
        relocated_signed_payload.to_str().unwrap(),
        "--signature",
        relocated_signature.to_str().unwrap(),
        "--public-key",
        relocated_public_key.to_str().unwrap(),
        "--json",
    ]);
    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("accepted"));
    assert_eq!(report["signature_verified"], json!(true));
    assert_eq!(
        report["signed_payload_path"],
        json!(relocated_signed_payload.to_str().unwrap().to_string())
    );
    assert_eq!(
        report["signature_path"],
        json!(relocated_signature.to_str().unwrap().to_string())
    );
    assert_eq!(
        report["public_key_path"],
        json!(relocated_public_key.to_str().unwrap().to_string())
    );
}

#[test]
fn verify_bridge_evidence_rejects_when_signed_payload_sha_does_not_match() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (evidence_path, _key) =
        sign_bridge_with_workbench_key(tmp.path(), "verify-sha-mismatch", "sha-mismatch-signer");
    let evidence: Value =
        serde_json::from_str(&fs::read_to_string(&evidence_path).expect("read evidence")).unwrap();
    let signed_payload_path = Path::new(
        evidence["signature"]["signed_payload_path"]
            .as_str()
            .unwrap(),
    )
    .to_path_buf();
    let mut original = fs::read_to_string(&signed_payload_path).expect("read signed payload");
    original.push(' ');
    fs::write(&signed_payload_path, original).expect("tamper signed payload");

    let output =
        run_verify_bridge_evidence(&["--evidence", evidence_path.to_str().unwrap(), "--json"]);
    assert!(
        !output.status.success(),
        "verify must fail when sidecar sha drifts; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("rejected"));
    assert_eq!(report["signed_payload_digest_match"], json!(false));
    assert_eq!(report["signature_verified"], json!(false));
    assert_eq!(report["trust_boundary_ok"], json!(true));
}

#[test]
fn verify_bridge_evidence_rejects_when_body_diverges_from_signed_payload() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (evidence_path, _key) =
        sign_bridge_with_workbench_key(tmp.path(), "verify-body-tamper", "body-tamper-signer");
    let mut evidence: Value =
        serde_json::from_str(&fs::read_to_string(&evidence_path).expect("read evidence")).unwrap();
    evidence["trust_boundary"]["factory_v3_role"] = json!("compromised-attacker-controlled");
    fs::write(
        &evidence_path,
        serde_json::to_string_pretty(&evidence).expect("encode") + "\n",
    )
    .expect("rewrite evidence");

    let output =
        run_verify_bridge_evidence(&["--evidence", evidence_path.to_str().unwrap(), "--json"]);
    assert!(
        !output.status.success(),
        "verify must fail when body diverges from signed payload; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("report json");
    assert_eq!(report["status"], json!("rejected"));
    assert_eq!(report["signed_payload_digest_match"], json!(true));
    assert_eq!(report["evidence_body_matches_signed_payload"], json!(false));
    assert_eq!(report["signature_verified"], json!(false));
    assert_eq!(report["trust_boundary_ok"], json!(false));
}
