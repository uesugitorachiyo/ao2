use std::fs;
use std::path::Path;
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
fn cli_factory_run_executes_legacy_roles_runspec_and_persists_runtime_task_graph_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: Legacy factory role runtime evidence
objective: Fix the discount bug through legacy factory-v3 role contracts with AO2 as execution owner.
acceptance:
  - AO2 persists translated legacy role task graph evidence through run, replay, and closure.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("legacy-runspec.yaml");
    fs::write(
        &runspec,
        r#"schema: factory-v3/runspec/v1
slug: bug-fix
profile: bug-fix
verifier: python -m pytest -q
roles:
- id: intake
  provider_key: FACTORY_V3_PLANNER_PROVIDER
  host_tag: []
  deps: []
  reads:
  - task brief
  writes:
  - docs/status/<slug>/roles/intake.md
- id: planner
  provider_key: FACTORY_V3_PLANNER_PROVIDER
  deps:
  - intake
  reads:
  - docs/status/<slug>/roles/intake.md
  writes:
  - docs/plans/<slug>-plan.md
- id: evaluator-closer
  provider_key: FACTORY_V3_EVALUATOR_CLOSER_PROVIDER
  deps:
  - planner
  reads:
  - docs/plans/<slug>-plan.md
  writes:
  - docs/evaluations/<slug>-evaluation.md
"#,
    )
    .unwrap();
    let plan_out = temp.path().join("legacy-plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let factory_decision = temp.path().join("factory-evaluator-decision.json");
    fs::write(&factory_decision, r#"{"decision":{"verdict":"accepted"}}"#).unwrap();

    let run = ao2([
        "factory",
        "run",
        "--plan",
        plan_out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "legacy-factory-runtime-evidence",
        "--factory-decision",
        factory_decision.to_str().unwrap(),
        "--json",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let result: serde_json::Value = serde_json::from_str(&stdout(&run)).unwrap();
    assert_eq!(result["status"], "Accepted");
    assert_eq!(result["factory_v3_evaluator_parity"]["status"], "matched");
    assert_eq!(
        result["parity_checklist_progress"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        result["replay"]["digest_failures"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let run_dir = Path::new(result["run_dir"].as_str().unwrap());
    let run_record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("run-record.json")).unwrap())
            .unwrap();
    assert_eq!(run_record["workflow_tasks"][0]["id"], "intake");
    assert_eq!(run_record["workflow_tasks"][1]["id"], "planner");
    assert_eq!(run_record["workflow_tasks"][2]["id"], "evaluator-closer");
    assert_eq!(run_record["workflow_dependencies"][0]["from"], "intake");
    assert_eq!(run_record["workflow_dependencies"][0]["to"], "planner");
    assert_eq!(run_record["workflow_dependencies"][1]["from"], "planner");
    assert_eq!(
        run_record["workflow_dependencies"][1]["to"],
        "evaluator-closer"
    );
    assert_eq!(
        run_record["factory_v3_compatibility"]["legacy_roles_runspec"],
        true
    );

    let evidence_pack: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(result["evidence_pack"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(evidence_pack["workflow_tasks"][0]["id"], "intake");
    assert_eq!(
        evidence_pack["workflow_dependencies"][1]["to"],
        "evaluator-closer"
    );
    assert_eq!(
        evidence_pack["factory_v3_compatibility"]["legacy_roles_runspec"],
        true
    );
    assert_eq!(evidence_pack["runtime_contract"]["execution_owner"], "ao2");
    assert_eq!(
        evidence_pack["runtime_contract"]["factory_v3_drives_workflow"],
        false
    );

    let events = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
    let compiled = events
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|event| event["event_type"] == "run.compiled")
        .expect("run.compiled event should be recorded");
    assert_eq!(compiled["payload"]["workflow_tasks"][0]["id"], "intake");
    assert_eq!(
        compiled["payload"]["workflow_dependencies"][1]["to"],
        "evaluator-closer"
    );
    assert_eq!(compiled["payload"]["factory_v3_drives_workflow"], false);
}

#[test]
fn cli_factory_run_executes_materialized_plan_and_replays_without_factory_driver() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 replacement parity run
objective: Fix the discount bug through an AO2-native generated factory compatibility workflow.
acceptance:
  - AO2 runs the generated workflow and replays evidence without factory-v3 driving execution.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: parity-runspec\nverifier:\n  command: python -m pytest -q\n",
    )
    .unwrap();
    let out = temp.path().join("plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let factory_decision = temp.path().join("factory-evaluator-decision.json");
    fs::write(
        &factory_decision,
        r#"{
  "schema_version": "factory-v3.evaluator-decision.v1",
  "decision": { "verdict": "accepted" }
}"#,
    )
    .unwrap();

    let run_result_out = temp.path().join("factory-compat-run-result.json");
    let signing_key = temp.path().join("factory-compat-run-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let run = ao2([
        "factory",
        "run",
        "--plan",
        out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "factory-compat-exec",
        "--factory-decision",
        factory_decision.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "factory-compat-runner",
        "--out",
        run_result_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let json: serde_json::Value = serde_json::from_str(&stdout(&run)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-compat-run-result.v1"
    );
    assert_eq!(json["status"], "Accepted");
    assert_eq!(
        json["trust_boundary"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        json["parity_checklist_progress"]["ao2_executes_generated_factory_compat_plan"],
        true
    );
    assert_eq!(
        json["parity_checklist_progress"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        json["replay"]["digest_failures"].as_array().unwrap().len(),
        0
    );
    assert_eq!(json["native_evaluator_decision"]["verdict"], "accepted");
    assert_eq!(
        json["native_evaluator_decision"]["factory_v3_required_to_decide"],
        false
    );
    assert_eq!(json["factory_v3_evaluator_parity"]["status"], "matched");
    assert_eq!(
        json["factory_v3_evaluator_parity"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        json["native_midpoint_gate_decision"]["schema_version"],
        "ao2.factory-v3-compat-native-midpoint-gate.v1"
    );
    assert_eq!(
        json["native_midpoint_gate_decision"]["owner"],
        "ao2-native-midpoint-gate"
    );
    assert_eq!(json["native_midpoint_gate_decision"]["verdict"], "accepted");
    assert_eq!(
        json["native_midpoint_gate_decision"]["factory_v3_required_to_decide"],
        false
    );
    assert_eq!(
        json["native_midpoint_gate_decision"]["checks"]["digest_replay_clean"],
        true
    );
    assert_eq!(
        json["native_midpoint_gate_decision"]["checks"]["evidence_pack_owner_ok"],
        true
    );
    assert_eq!(
        json["parity_checklist_progress"]["ao2_owns_midpoint_gate_decision"],
        true
    );
    assert_eq!(
        json["parity_checklist_progress"]["ao2_owns_evaluator_closer_decision"],
        true
    );
    assert_eq!(
        json["parity_checklist_progress"]["factory_v3_evaluator_compared_when_supplied"],
        true
    );

    let evaluator_out = temp.path().join("native-evaluator-decision.json");
    let evaluator = ao2([
        "factory",
        "evaluate",
        "--evidence-pack",
        json["evidence_pack"].as_str().unwrap(),
        "--factory-decision",
        factory_decision.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "native-evaluator-test",
        "--out",
        evaluator_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(evaluator.status.success(), "{}", stderr(&evaluator));
    let evaluator_json: serde_json::Value = serde_json::from_str(&stdout(&evaluator)).unwrap();
    assert_eq!(
        evaluator_json["schema_version"],
        "ao2.factory-v3-compat-native-evaluator-result.v1"
    );
    assert_eq!(evaluator_json["owner"], "ao2-native-evaluator-closer");
    assert_eq!(evaluator_json["verdict"], "accepted");
    assert_eq!(
        evaluator_json["factory_v3_evaluator_parity"]["status"],
        "matched"
    );
    assert_eq!(
        evaluator_json["parity_checklist_progress"]
            ["ao2_can_evaluate_existing_evidence_without_factory_driver"],
        true
    );
    assert_eq!(
        evaluator_json["parity_checklist_progress"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(evaluator_json["signature"]["signature_verified"], true);
    assert!(Path::new(evaluator_json["decision_path"].as_str().unwrap()).is_file());
    assert!(Path::new(
        evaluator_json["signature"]["signed_payload_path"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(
        evaluator_json["signature"]["signature_path"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(
        evaluator_json["signature"]["public_key_path"]
            .as_str()
            .unwrap()
    )
    .is_file());

    let verify_evaluator = ao2([
        "factory",
        "verify-evaluator-decision",
        "--decision",
        evaluator_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_evaluator.status.success(),
        "{}",
        stderr(&verify_evaluator)
    );
    let evaluator_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_evaluator)).unwrap();
    assert_eq!(
        evaluator_verification["schema_version"],
        "ao2.factory-v3-compat-native-evaluator-verification.v1"
    );
    assert_eq!(evaluator_verification["status"], "accepted");
    assert_eq!(evaluator_verification["signature_verified"], true);
    assert_eq!(evaluator_verification["signature_digest_match"], true);
    assert_eq!(evaluator_verification["public_key_digest_match"], true);
    assert_eq!(evaluator_verification["signed_payload_digest_match"], true);
    assert_eq!(
        evaluator_verification["decision_payload_matches_signed_payload"],
        true
    );
    assert_eq!(evaluator_verification["trust_boundary_ok"], true);
    assert_eq!(
        evaluator_verification["ao2_decision_owner"],
        "ao2-native-evaluator-decision-verifier"
    );

    let mut tampered_evaluator: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&evaluator_out).unwrap()).unwrap();
    tampered_evaluator["trust_boundary"]["factory_v3_role"] = serde_json::json!("decision_owner");
    let tampered_evaluator_path = temp.path().join("tampered-evaluator-decision.json");
    fs::write(
        &tampered_evaluator_path,
        serde_json::to_string_pretty(&tampered_evaluator).unwrap(),
    )
    .unwrap();
    let verify_tampered_evaluator = ao2([
        "factory",
        "verify-evaluator-decision",
        "--decision",
        tampered_evaluator_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_tampered_evaluator.status.success(),
        "{}",
        stderr(&verify_tampered_evaluator)
    );
    let tampered_evaluator_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_tampered_evaluator)).unwrap();
    assert_eq!(tampered_evaluator_verification["status"], "rejected");
    assert_eq!(
        tampered_evaluator_verification["decision_payload_matches_signed_payload"],
        false
    );
    assert_eq!(tampered_evaluator_verification["trust_boundary_ok"], false);

    let mut unsigned_evaluator: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&evaluator_out).unwrap()).unwrap();
    unsigned_evaluator["signature"] = serde_json::json!({
        "schema_version": "ao2.factory-compat-native-evaluator-signature.v1",
        "signed_payload": "native_evaluator_decision_without_signature_field",
        "signature_verified": false,
        "signature_status": "unsigned"
    });
    let unsigned_evaluator_path = temp.path().join("unsigned-evaluator-decision.json");
    fs::write(
        &unsigned_evaluator_path,
        serde_json::to_string_pretty(&unsigned_evaluator).unwrap(),
    )
    .unwrap();
    let verify_unsigned_evaluator = ao2([
        "factory",
        "verify-evaluator-decision",
        "--decision",
        unsigned_evaluator_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_unsigned_evaluator.status.success(),
        "{}",
        stderr(&verify_unsigned_evaluator)
    );
    let unsigned_evaluator_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_unsigned_evaluator)).unwrap();
    assert_eq!(unsigned_evaluator_verification["status"], "rejected");
    assert_eq!(
        unsigned_evaluator_verification["signature_requirement_satisfied"],
        false
    );

    assert!(Path::new(json["evidence_pack"].as_str().unwrap()).is_file());
    assert_eq!(json["run_result_path"], run_result_out.to_str().unwrap());
    assert!(Path::new(json["run_result_path"].as_str().unwrap()).is_file());
    assert!(Path::new(json["memory_summary_path"].as_str().unwrap()).is_file());
    assert_eq!(
        json["parity_checklist_progress"]["ao2_produces_factory_compat_handoff_evidence"],
        true
    );
    assert_eq!(
        json["parity_checklist_progress"]["ao2_can_sign_factory_compat_handoff_evidence"],
        true
    );
    assert!(Path::new(json["handoff_evidence_path"].as_str().unwrap()).is_file());
    assert_eq!(
        json["factory_compat_handoff_evidence"]["schema_version"],
        "ao2.factory-v3-compat-run-handoff-evidence.v1"
    );
    assert_eq!(
        json["factory_compat_handoff_evidence"]["release_handoff_contract"]
            ["primary_evidence_owner"],
        "ao2"
    );
    assert_eq!(
        json["factory_compat_handoff_evidence"]["native_midpoint_gate_decision"]["owner"],
        "ao2-native-midpoint-gate"
    );
    assert_eq!(
        json["factory_compat_handoff_evidence"]["signature"]["signature_verified"],
        true
    );
    assert_eq!(
        json["factory_compat_handoff_evidence"]["signature"]["signed_payload"],
        "run_result"
    );
    assert!(Path::new(
        json["factory_compat_handoff_evidence"]["signature"]["signature_path"]
            .as_str()
            .unwrap()
    )
    .is_file());
    let verify_handoff = ao2([
        "factory",
        "verify-handoff",
        "--handoff",
        json["handoff_evidence_path"].as_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_handoff.status.success(),
        "{}",
        stderr(&verify_handoff)
    );
    let verification: serde_json::Value = serde_json::from_str(&stdout(&verify_handoff)).unwrap();
    assert_eq!(
        verification["schema_version"],
        "ao2.factory-v3-compat-run-handoff-verification.v1"
    );
    assert_eq!(verification["status"], "accepted");
    assert_eq!(verification["run_result_digest_match"], true);
    assert_eq!(verification["signature_verified"], true);
    assert_eq!(verification["public_key_digest_match"], true);
    assert_eq!(verification["trust_boundary_ok"], true);
    assert_eq!(
        verification["ao2_decision_owner"],
        "ao2-native-factory-handoff-verifier"
    );

    let verify_run_result = ao2([
        "factory",
        "verify-run-result",
        "--run-result",
        json["run_result_path"].as_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_run_result.status.success(),
        "{}",
        stderr(&verify_run_result)
    );
    let run_result_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_run_result)).unwrap();
    assert_eq!(
        run_result_verification["schema_version"],
        "ao2.factory-v3-compat-run-result-verification.v1"
    );
    assert_eq!(run_result_verification["status"], "accepted");
    assert_eq!(run_result_verification["ao2_primary_run_result_ok"], true);
    assert_eq!(run_result_verification["evidence_pack_owner_ok"], true);
    assert_eq!(run_result_verification["replay_digest_clean"], true);
    assert_eq!(run_result_verification["midpoint_gate_accepted"], true);
    assert_eq!(run_result_verification["native_evaluator_accepted"], true);
    assert_eq!(run_result_verification["trust_boundary_ok"], true);
    assert_eq!(
        run_result_verification["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        run_result_verification["ao2_decision_owner"],
        "ao2-native-run-result-verifier"
    );

    let mut tampered_run_result: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["run_result_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    tampered_run_result["trust_boundary"]["factory_v3_role"] = serde_json::json!("decision_owner");
    let tampered_run_result_path = temp.path().join("tampered-run-result.json");
    fs::write(
        &tampered_run_result_path,
        serde_json::to_string_pretty(&tampered_run_result).unwrap(),
    )
    .unwrap();
    let verify_tampered_run_result = ao2([
        "factory",
        "verify-run-result",
        "--run-result",
        tampered_run_result_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_tampered_run_result.status.success(),
        "{}",
        stderr(&verify_tampered_run_result)
    );
    let tampered_run_result_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_tampered_run_result)).unwrap();
    assert_eq!(tampered_run_result_verification["status"], "rejected");
    assert_eq!(tampered_run_result_verification["trust_boundary_ok"], false);
    assert_eq!(
        tampered_run_result_verification["ao2_primary_run_result_ok"],
        false
    );

    let portable_dir = temp.path().join("portable-handoff");
    fs::create_dir_all(&portable_dir).unwrap();
    let portable_run_result = portable_dir.join("run-result.json");
    let portable_signature = portable_dir.join("run-result.sig");
    let portable_public_key = portable_dir.join("run-result.pub");
    fs::copy(
        json["run_result_path"].as_str().unwrap(),
        &portable_run_result,
    )
    .unwrap();
    fs::copy(
        json["factory_compat_handoff_evidence"]["signature"]["signature_path"]
            .as_str()
            .unwrap(),
        &portable_signature,
    )
    .unwrap();
    fs::copy(
        json["factory_compat_handoff_evidence"]["signature"]["public_key_path"]
            .as_str()
            .unwrap(),
        &portable_public_key,
    )
    .unwrap();
    let mut portable_handoff = json["factory_compat_handoff_evidence"].clone();
    portable_handoff["run_result_path"] = serde_json::json!("run-result.json");
    portable_handoff["signature"]["signature_path"] = serde_json::json!("run-result.sig");
    portable_handoff["signature"]["public_key_path"] = serde_json::json!("run-result.pub");
    let portable_handoff_path = portable_dir.join("handoff.json");
    fs::write(
        &portable_handoff_path,
        serde_json::to_string_pretty(&portable_handoff).unwrap(),
    )
    .unwrap();
    let other_cwd = temp.path().join("verify-from-other-cwd");
    fs::create_dir_all(&other_cwd).unwrap();
    let verify_relative_handoff = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "factory",
            "verify-handoff",
            "--handoff",
            portable_handoff_path.to_str().unwrap(),
            "--json",
        ])
        .current_dir(&other_cwd)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .unwrap();
    assert!(
        verify_relative_handoff.status.success(),
        "{}",
        stderr(&verify_relative_handoff)
    );
    let relative_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_relative_handoff)).unwrap();
    assert_eq!(relative_verification["status"], "accepted");
    assert_eq!(relative_verification["run_result_digest_match"], true);
    assert_eq!(relative_verification["signature_verified"], true);
    assert_eq!(relative_verification["public_key_digest_match"], true);

    let mut unsigned_handoff: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["handoff_evidence_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    unsigned_handoff["signature"] = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-run-result-signature.v1",
        "signed_payload": "run_result",
        "signature_verified": false,
        "signature_status": "unsigned"
    });
    let unsigned_handoff_path = temp.path().join("unsigned-handoff.json");
    fs::write(
        &unsigned_handoff_path,
        serde_json::to_string_pretty(&unsigned_handoff).unwrap(),
    )
    .unwrap();
    let verify_unsigned = ao2([
        "factory",
        "verify-handoff",
        "--handoff",
        unsigned_handoff_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_unsigned.status.success(),
        "{}",
        stderr(&verify_unsigned)
    );
    let unsigned_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_unsigned)).unwrap();
    assert_eq!(unsigned_verification["status"], "rejected");
    assert_eq!(
        unsigned_verification["signature_requirement_satisfied"],
        false
    );

    let mut fake_signed_handoff: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["handoff_evidence_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    fake_signed_handoff["signature"] = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-run-result-signature.v1",
        "signed_payload": "run_result",
        "signature_path": temp.path().join("missing.sig").display().to_string(),
        "public_key_path": temp.path().join("missing.pub").display().to_string(),
        "public_key_sha256": "0".repeat(64),
        "signature_verified": false,
        "signature_status": "unsigned"
    });
    let fake_signed_handoff_path = temp.path().join("fake-signed-handoff.json");
    fs::write(
        &fake_signed_handoff_path,
        serde_json::to_string_pretty(&fake_signed_handoff).unwrap(),
    )
    .unwrap();
    let verify_fake_signed = ao2([
        "factory",
        "verify-handoff",
        "--handoff",
        fake_signed_handoff_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_fake_signed.status.success(),
        "{}",
        stderr(&verify_fake_signed)
    );
    let fake_signed_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_fake_signed)).unwrap();
    assert_eq!(fake_signed_verification["status"], "rejected");
    assert_eq!(fake_signed_verification["signature_status"], "signed");
    assert_eq!(fake_signed_verification["signature_verified"], false);
    assert_eq!(
        fake_signed_verification["signature_requirement_satisfied"],
        false
    );

    let mut wrong_boundary_handoff: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["handoff_evidence_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    wrong_boundary_handoff["trust_boundary"]["factory_v3_role"] =
        serde_json::json!("decision_owner");
    let wrong_boundary_handoff_path = temp.path().join("wrong-boundary-handoff.json");
    fs::write(
        &wrong_boundary_handoff_path,
        serde_json::to_string_pretty(&wrong_boundary_handoff).unwrap(),
    )
    .unwrap();
    let verify_wrong_boundary = ao2([
        "factory",
        "verify-handoff",
        "--handoff",
        wrong_boundary_handoff_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_wrong_boundary.status.success(),
        "{}",
        stderr(&verify_wrong_boundary)
    );
    let wrong_boundary_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_wrong_boundary)).unwrap();
    assert_eq!(wrong_boundary_verification["status"], "rejected");
    assert_eq!(wrong_boundary_verification["trust_boundary_ok"], false);

    let mut wrong_release_contract_handoff: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["handoff_evidence_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    wrong_release_contract_handoff["release_handoff_contract"]["primary_evidence_owner"] =
        serde_json::json!("factory-v3");
    let wrong_release_contract_handoff_path =
        temp.path().join("wrong-release-contract-handoff.json");
    fs::write(
        &wrong_release_contract_handoff_path,
        serde_json::to_string_pretty(&wrong_release_contract_handoff).unwrap(),
    )
    .unwrap();
    let verify_wrong_release_contract = ao2([
        "factory",
        "verify-handoff",
        "--handoff",
        wrong_release_contract_handoff_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_wrong_release_contract.status.success(),
        "{}",
        stderr(&verify_wrong_release_contract)
    );
    let wrong_release_contract_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_wrong_release_contract)).unwrap();
    assert_eq!(wrong_release_contract_verification["status"], "rejected");
    assert_eq!(
        wrong_release_contract_verification["release_handoff_contract_ok"],
        false
    );

    let mut digest_mismatch_handoff: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["handoff_evidence_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    digest_mismatch_handoff["run_result_sha256"] = serde_json::json!("0".repeat(64));
    let digest_mismatch_handoff_path = temp.path().join("digest-mismatch-handoff.json");
    fs::write(
        &digest_mismatch_handoff_path,
        serde_json::to_string_pretty(&digest_mismatch_handoff).unwrap(),
    )
    .unwrap();
    let verify_digest_mismatch = ao2([
        "factory",
        "verify-handoff",
        "--handoff",
        digest_mismatch_handoff_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        verify_digest_mismatch.status.success(),
        "{}",
        stderr(&verify_digest_mismatch)
    );
    let digest_mismatch_verification: serde_json::Value =
        serde_json::from_str(&stdout(&verify_digest_mismatch)).unwrap();
    assert_eq!(digest_mismatch_verification["status"], "rejected");
    assert_eq!(
        digest_mismatch_verification["run_result_digest_match"],
        false
    );
    assert_eq!(
        json["parity_checklist_progress"]["ao2_exports_hermes_memory_summary"],
        true
    );
    assert_eq!(
        json["parity_checklist_progress"]["ao2_persists_factory_compat_run_result"],
        true
    );
    let persisted_result: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&run_result_out).unwrap()).unwrap();
    assert_eq!(persisted_result["run_id"], "factory-compat-exec");
    let memory_summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["memory_summary_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        memory_summary["schema_version"],
        "ao2.factory-v3-compat-hermes-memory-summary.v1"
    );
    assert_eq!(memory_summary["owner"], "ao2");
    assert_eq!(
        memory_summary["replacement_parity_progress"]["ao2_replay_completed"],
        true
    );
    assert!(Path::new(json["history_path"].as_str().unwrap()).is_file());
    assert_eq!(
        json["parity_checklist_progress"]["ao2_persists_restart_safe_factory_compat_history"],
        true
    );
    let history: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["history_path"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        history["schema_version"],
        "ao2.factory-v3-compat-run-history.v1"
    );
    assert_eq!(history["entries"].as_array().unwrap().len(), 1);
    assert_eq!(history["entries"][0]["run_id"], "factory-compat-exec");
    assert_eq!(
        history["entries"][0]["continuity"]["survives_server_restart"],
        true
    );
    assert_eq!(
        history["entries"][0]["continuity"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        history["entries"][0]["continuity"]["cancel_retry_state_owner"],
        "ao2-workbench-queue"
    );
}

#[test]
fn cli_factory_replacement_smoke_chains_ao2_primary_run_and_evidence_pack() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 one-command replacement smoke
objective: Prove AO2 can own the factory-v3 replacement execution path with factory-v3 only as parity oracle.
acceptance:
  - AO2 plans, queues, runs, verifies the run result, and packs evidence with one command.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: replacement-smoke\nverifier:\n  command: python -m pytest -q\n",
    )
    .unwrap();
    let factory_decision = temp.path().join("factory-evaluator-decision.json");
    fs::write(
        &factory_decision,
        r#"{
  "schema_version": "factory-v3.evaluator-decision.v1",
  "decision": { "verdict": "accepted" }
}"#,
    )
    .unwrap();
    let signing_key = temp.path().join("replacement-smoke-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("replacement-smoke-out");

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
        "replacement-smoke-run",
        "--factory-decision",
        factory_decision.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "replacement-smoke-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(smoke.status.success(), "{}", stderr(&smoke));
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-compat-replacement-smoke.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], "replacement-smoke-run");
    assert_eq!(json["factory_v3_role"], "parity_oracle_only");
    assert_eq!(json["ao2_decision_owner"], "ao2-native-replacement-smoke");
    assert_eq!(
        json["replacement_checklist"]["ao2_planned_factory_compat_workflow"],
        true
    );
    assert_eq!(
        json["replacement_checklist"]["ao2_queue_executed_factory_compat_workflow"],
        true
    );
    assert_eq!(
        json["replacement_checklist"]["ao2_verified_primary_run_result"],
        true
    );
    assert_eq!(
        json["replacement_checklist"]["ao2_packed_primary_evidence"],
        true
    );
    assert_eq!(
        json["replacement_checklist"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(json["run_result_verification"]["status"], "accepted");
    assert_eq!(
        json["run_result_verification"]["ao2_primary_run_result_ok"],
        true
    );
    assert_eq!(json["pack_evidence"]["status"], "produced");
    assert_eq!(
        json["pack_evidence"]["signature"]["signature_verified"],
        true
    );
    assert_eq!(json["queue_run_next"]["status"], "accepted");
    assert_eq!(
        json["three_os_contract"]["path_separator_safe_artifacts"],
        true
    );
    assert!(Path::new(json["artifacts"]["plan"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["run_result"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["packed_evidence"].as_str().unwrap()).is_file());
    assert!(Path::new(
        json["artifacts"]["run_result_verification"]
            .as_str()
            .unwrap()
    )
    .is_file());
}

#[test]
fn cli_factory_governed_run_chains_planning_execution_evidence_and_signed_closure() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 production governed run
objective: Execute factory-v3-compatible governed work through AO2 without a smoke-only wrapper.
acceptance:
  - AO2 plans, queues, executes, verifies, packs evidence, signs evaluator closure, and records operator handoff in one command.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: governed-production-run
verifier:
  command: python -m pytest -q
",
    )
    .unwrap();
    let signing_key = temp.path().join("governed-run-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("governed-run-out");

    let governed = ao2([
        "factory",
        "governed-run",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "governed-production-run",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "governed-run-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(governed.status.success(), "{}", stderr(&governed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&governed)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-compat-governed-run.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], "governed-production-run");
    assert_eq!(json["ao2_decision_owner"], "ao2-native-governed-run");
    assert_eq!(json["factory_v3_role"], "parity_oracle_only");
    assert_eq!(json["governed_run_checklist"]["smoke_only_wrapper"], false);
    assert_eq!(
        json["governed_run_checklist"]["ao2_planned_factory_compat_workflow"],
        true
    );
    assert_eq!(
        json["governed_run_checklist"]["ao2_queue_executed_factory_compat_workflow"],
        true
    );
    assert_eq!(
        json["governed_run_checklist"]["ao2_verified_primary_run_result"],
        true
    );
    assert_eq!(
        json["governed_run_checklist"]["ao2_packed_primary_evidence"],
        true
    );
    assert_eq!(
        json["governed_run_checklist"]["ao2_signed_evaluator_closure"],
        true
    );
    assert_eq!(
        json["governed_run_checklist"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(json["run_result_verification"]["status"], "accepted");
    assert_eq!(json["pack_evidence"]["status"], "produced");
    assert_eq!(
        json["pack_evidence"]["signature"]["signature_verified"],
        true
    );
    assert_eq!(json["evaluator_decision"]["verdict"], "accepted");
    assert_eq!(
        json["evaluator_decision"]["signature"]["signature_verified"],
        true
    );
    assert_eq!(
        json["evaluator_decision_verification"]["status"],
        "accepted"
    );
    assert_eq!(
        json["evaluator_decision_verification"]["signature_verified"],
        true
    );
    assert!(Path::new(json["artifacts"]["governed_run"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["plan"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["run_result"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["packed_evidence"].as_str().unwrap()).is_file());
    assert!(Path::new(json["artifacts"]["evaluator_decision"].as_str().unwrap()).is_file());
    assert!(Path::new(
        json["artifacts"]["evaluator_decision_verification"]
            .as_str()
            .unwrap()
    )
    .is_file());
}

#[test]
fn cli_greenfield_ingest_materializes_spec_request_runspec_plan_and_obligation_ledger() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("greenfield-target");
    fs::create_dir_all(&repo).unwrap();
    let spec = temp.path().join("missed-call-recovery.md");
    fs::write(
        &spec,
        r#"# Missed Call Recovery

Build a production-ready missed-call recovery application for small service businesses.

Acceptance:
- The app captures missed calls and creates follow-up tasks.
- The app preserves the formula `lost_revenue = missed_calls * average_ticket`.
- The verifier can run with `python -m pytest -q`.
"#,
    )
    .unwrap();
    let out_dir = temp.path().join("greenfield-out");

    let ingest = ao2([
        "greenfield",
        "ingest",
        "--spec",
        spec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "missed-call-recovery",
        "--verifier-command",
        "python -m pytest -q",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(ingest.status.success(), "{}", stderr(&ingest));
    let json: serde_json::Value = serde_json::from_str(&stdout(&ingest)).unwrap();
    assert_eq!(json["schema_version"], "ao2.greenfield-ingest.v1");
    assert_eq!(json["status"], "planned");
    assert_eq!(json["classification"]["shape"], "greenfield");
    assert_eq!(json["work_request"]["shape"], "greenfield");
    assert_eq!(
        json["greenfield_checklist"]["ao2_generated_work_request"],
        true
    );
    assert_eq!(json["greenfield_checklist"]["ao2_generated_runspec"], true);
    assert_eq!(
        json["greenfield_checklist"]["ao2_materialized_governed_plan"],
        true
    );
    assert_eq!(
        json["greenfield_checklist"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert!(json["obligation_ledger"]["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|obligation| obligation["text"]
            .as_str()
            .unwrap_or("")
            .contains("lost_revenue")
            || obligation["statement"]
                .as_str()
                .unwrap_or("")
                .contains("lost_revenue")));
    for key in [
        "spec_intake",
        "work_request",
        "runspec",
        "obligation_ledger",
        "plan",
        "planning_evidence",
        "greenfield_ingest",
    ] {
        assert!(
            Path::new(json["artifacts"][key].as_str().unwrap()).is_file(),
            "artifact {key} should exist"
        );
    }
    let request: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["artifacts"]["work_request"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(request["source_spec_sha256"], sha256_path(&spec));
    assert_eq!(request["shape"], "greenfield");
    assert_eq!(request["size"], "medium");
}

#[test]
fn cli_greenfield_governed_run_chains_spec_ingest_execution_and_signed_closure() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("greenfield-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let spec = temp.path().join("greenfield-discount.md");
    fs::write(
        &spec,
        r#"# Greenfield Discount Service

Build a small governed discount service.

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
printf 'Summary: greenfield governed run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 13\n'
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("greenfield-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("greenfield-governed-out");

    let governed = ao2([
        "greenfield",
        "governed-run",
        "--spec",
        spec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "greenfield-governed-run",
        "--verifier-command",
        "python -m pytest -q",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "greenfield-governed-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(governed.status.success(), "{}", stderr(&governed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&governed)).unwrap();
    assert_eq!(json["schema_version"], "ao2.greenfield-governed-run.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], "greenfield-governed-run");
    assert_eq!(json["ingest"]["status"], "planned");
    assert_eq!(json["governed_run"]["status"], "accepted");
    assert_eq!(
        json["greenfield_governed_run_checklist"]["ao2_ingested_plain_spec"],
        true
    );
    assert_eq!(
        json["greenfield_governed_run_checklist"]["ao2_executed_generated_governed_plan"],
        true
    );
    assert_eq!(
        json["greenfield_governed_run_checklist"]["ao2_signed_evaluator_closure"],
        true
    );
    assert_eq!(
        json["greenfield_governed_run_checklist"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert!(Path::new(
        json["artifacts"]["greenfield_governed_run"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(json["artifacts"]["governed_run"].as_str().unwrap()).is_file());
    assert_eq!(
        json["governed_run"]["evaluator_decision_verification"]["status"],
        "accepted"
    );
}

#[test]
fn cli_factory_greenfield_run_chains_spec_to_governed_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("greenfield-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let spec = temp.path().join("factory-greenfield-discount.md");
    fs::write(
        &spec,
        r#"# Factory Greenfield Discount Service

Build a governed discount service from a plain greenfield spec.

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
printf 'Summary: factory greenfield run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 17\n'
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("factory-greenfield-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("factory-greenfield-out");

    let governed = ao2([
        "factory",
        "greenfield-run",
        "--spec",
        spec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "factory-greenfield-run",
        "--verifier-command",
        "python -m pytest -q",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "factory-greenfield-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(governed.status.success(), "{}", stderr(&governed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&governed)).unwrap();
    assert_eq!(json["schema_version"], "ao2.factory-greenfield-run.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], "factory-greenfield-run");
    assert_eq!(
        json["factory_replacement_boundary"]["factory_v3_role"],
        "parity_oracle_only"
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
        json["greenfield"]["greenfield_governed_run_checklist"]
            ["ao2_executed_generated_governed_plan"],
        true
    );
    assert_eq!(
        json["greenfield"]["governed_run"]["evaluator_decision_verification"]["status"],
        "accepted"
    );
    assert!(Path::new(
        json["artifacts"]["factory_greenfield_run"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(
        json["artifacts"]["greenfield_governed_run"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(json["artifacts"]["evidence_pack"].as_str().unwrap()).is_file());
}

#[test]
fn cli_factory_app_run_chains_spec_to_release_review_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("app-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let spec = temp.path().join("factory-app-discount.md");
    fs::write(
        &spec,
        r#"# Factory App Discount Service

Build a production app workflow from a plain greenfield spec.

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
printf 'Summary: factory app run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 19\n'
"#,
    )
    .unwrap();
    let signing_key = temp.path().join("factory-app-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let out_dir = temp.path().join("factory-app-out");

    let governed = ao2([
        "factory",
        "app-run",
        "--spec",
        spec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "factory-app-run",
        "--verifier-command",
        "python -m pytest -q",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "factory-app-test",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(governed.status.success(), "{}", stderr(&governed));
    let json: serde_json::Value = serde_json::from_str(&stdout(&governed)).unwrap();
    assert_eq!(json["schema_version"], "ao2.factory-app-run.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_id"], "factory-app-run");
    assert_eq!(
        json["rubric_sha256"],
        sha256_path(Path::new(
            json["artifacts"]["evaluator_rubric"].as_str().unwrap()
        ))
    );
    assert_eq!(
        json["evaluator_rubric"]["rubric"]["schema_version"],
        "ao2.factory-evaluator-rubric.v1"
    );
    assert_eq!(
        json["release_review"]["rubric_sha256"],
        json["rubric_sha256"]
    );
    assert_eq!(
        json["factory_replacement_boundary"]["ao2_execution_owner"],
        true
    );
    assert_eq!(
        json["factory_replacement_boundary"]["factory_v3_drives_workflow"],
        false
    );
    assert_eq!(
        json["factory_replacement_boundary"]["factory_v3_role"],
        "parity_oracle_only"
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
        json["app_run_checklist"]["ao2_derived_signed_evaluator_rubric"],
        true
    );
    assert_eq!(json["app_run_checklist"]["ao2_ingested_plain_spec"], true);
    assert_eq!(
        json["app_run_checklist"]["ao2_executed_generated_governed_plan"],
        true
    );
    assert_eq!(
        json["app_run_checklist"]["ao2_signed_evaluator_closure"],
        true
    );
    assert_eq!(
        json["app_run_checklist"]["release_review_artifacts_ready"],
        true
    );
    assert_eq!(
        json["app_run_checklist"]["verifier_outputs_reference_rubric_sha256"],
        true
    );
    assert_eq!(
        json["app_run_checklist"]["closer_outputs_reference_rubric_sha256"],
        true
    );
    assert_eq!(
        json["release_review"]["downstream_contract"]["verifier_outputs_must_reference"],
        "rubric_sha256"
    );
    assert_eq!(
        json["release_review"]["downstream_contract"]["closer_outputs_must_reference"],
        "rubric_sha256"
    );
    assert_eq!(
        json["app"]["governed_run"]["evaluator_decision_verification"]["status"],
        "accepted"
    );
    for key in [
        "factory_app_run",
        "evaluator_rubric",
        "greenfield_governed_run",
        "greenfield_ingest",
        "plan",
        "governed_run",
        "evidence_pack",
        "evaluator_decision",
    ] {
        assert!(
            Path::new(json["artifacts"][key].as_str().unwrap()).is_file(),
            "artifact {key} should exist"
        );
    }
}
