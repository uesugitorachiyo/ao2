use std::fs;
use std::path::Path;
use std::process::Command;

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
fn cli_factory_plan_classifies_and_materializes_ao2_native_plan() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 replacement parity
objective: Refactor governed execution provider planning for Windows macOS Ubuntu parity.
acceptance:
  - AO2 classifies size and shape before factory-v3 drives the workflow.
"#,
    )
    .unwrap();
    let profile = temp.path().join("profile.yaml");
    fs::write(&profile, "provider: scripted\nroles:\n  - planner\n").unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(&runspec, "id: parity-runspec\nverifier: npm run verify\n").unwrap();
    let role = temp.path().join("evaluator-closer.md");
    fs::write(
        &role,
        r#"name: evaluator-closer
outputs:
  - evidence
  - custom_metric
  - name: structured_output
    schema: custom.v1
"#,
    )
    .unwrap();
    let out = temp.path().join("plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--profile",
        profile.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--role-contract",
        role.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-compat-plan-result.v1"
    );
    assert_eq!(json["classification"]["shape"], "refactor");
    assert_eq!(json["classification"]["size"], "large");
    assert_eq!(
        json["parity_checklist_progress"]["ao2_accepts_request_and_classifies"],
        true
    );
    assert!(Path::new(json["plan_path"].as_str().unwrap()).is_file());
    assert!(Path::new(json["planning_evidence_path"].as_str().unwrap()).is_file());
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(
        materialized["ao2_native_plan"]["closure_gate"]["factory_v3_role"],
        "parity_oracle_only"
    );
    assert_eq!(
        materialized["factory_v3_inputs"]["role_contracts"][0]["kind"],
        "role_contract"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["roles"][0]["status_required"],
        true
    );
    let role_outputs = materialized["ao2_native_plan"]["roles"][0]["outputs"]
        .as_array()
        .unwrap();
    for required in [
        "evidence",
        "concerns",
        "blockers",
        "changed_files",
        "sandbox",
        "secret_redaction",
    ] {
        assert!(
            role_outputs.iter().any(|output| output == required),
            "missing required AO2/factory role output {required}"
        );
    }
    assert!(role_outputs.iter().any(|output| output == "custom_metric"));
    assert!(role_outputs.iter().any(|output| {
        output["name"] == "structured_output" && output["schema"] == "custom.v1"
    }));
    assert_eq!(
        materialized["ao2_native_plan"]["provider_profiles"][0],
        "scripted"
    );
    let workflow_path = Path::new(json["workflow_path"].as_str().unwrap());
    assert!(workflow_path.is_file());
    let workflow: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(workflow_path).unwrap()).unwrap();
    assert_eq!(workflow["id"], "factory-v3-compat-refactor-large");
    assert_eq!(workflow["template_kind"], "real_project");
    assert_eq!(workflow["roles"][0], "evaluator-closer");
    assert_eq!(workflow["verifier"]["command"], "npm run verify");
    assert_eq!(
        materialized["ao2_native_plan"]["runnable_workflow"]["factory_v3_drives_workflow"],
        false
    );
}

#[test]
fn cli_factory_plan_honors_structured_request_classification_over_heuristics() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.json");
    fs::write(
        &request,
        serde_json::to_string_pretty(&serde_json::json!({
            "classification": "SMALL",
            "shape": "greenfield",
            "objective": "Plan a tiny new helper even though the prose mentions bug fixes, providers, release, macOS, Ubuntu, Windows, and replacement parity.",
            "success_criteria": ["AO2 native classifier preserves operator-declared size and shape"]
        }))
        .unwrap(),
    )
    .unwrap();
    let out = temp.path().join("plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    assert_eq!(json["classification"]["size"], "small");
    assert_eq!(json["classification"]["shape"], "greenfield");
    assert_eq!(json["classification"]["source"], "structured_work_request");
    assert_eq!(
        json["classification"]["factory_v3_required_before_classification"],
        false
    );

    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(
        materialized["workflow_id"],
        "factory-v3-compat-greenfield-small"
    );
    assert_eq!(
        materialized["classification"]["source"],
        "structured_work_request"
    );
}

#[test]
fn cli_factory_plan_classifies_role_contract_scope_before_factory_drives_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: Provider role contract intake\nobjective: Create a governed task plan from attached role contracts.\n",
    )
    .unwrap();
    let role = temp.path().join("provider-smoke.md");
    fs::write(
        &role,
        r#"name: provider-smoke
scope:
  - windows
  - ubuntu
  - macos
outputs:
  - evidence
  - provider readiness
"#,
    )
    .unwrap();

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--role-contract",
        role.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();

    assert_eq!(json["classification"]["size"], "large");
    assert_eq!(
        json["classification"]["factory_v3_required_before_classification"],
        false
    );
    let signals = json["classification"]["signals"].as_array().unwrap();
    assert!(signals
        .iter()
        .any(|signal| signal == "provider_orchestration"));
    assert!(signals.iter().any(|signal| signal == "three_os_or_windows"));
}

#[test]
fn cli_factory_plan_loads_factory_v3_toml_role_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: TOML role contract compatibility\nobjective: Load AO Operator role contracts directly into an AO2-native governed plan.\n",
    )
    .unwrap();
    let role = temp.path().join("evaluator-closer.toml");
    fs::write(
        &role,
        r#"name = "evaluator-closer"
description = "Validates final artifacts against the approved plan and closes or rejects."
inputs = ["hardened plan", "integrated artifact", "verification evidence"]
outputs = ["acceptance decision", "closure evidence", "remaining concerns"]
status_required = true
"#,
    )
    .unwrap();
    let out = temp.path().join("toml-contract-plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--role-contract",
        role.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap())
            .unwrap();
    let role_plan = &materialized["ao2_native_plan"]["roles"][0];
    assert_eq!(role_plan["role_id"], "evaluator-closer");
    assert_eq!(role_plan["source"], "factory-v3-role-contract");
    assert_eq!(role_plan["status_required"], true);
    assert_eq!(role_plan["inputs"][0], "hardened plan");
    assert!(role_plan["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|output| output == "acceptance decision"));
    assert!(role_plan["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|output| output == "secret_redaction"));
    assert_eq!(
        materialized["factory_v3_inputs"]["role_contracts"][0]["kind"],
        "role_contract"
    );
    assert_eq!(
        materialized["classification"]["factory_v3_required_before_classification"],
        false
    );
}

#[test]
fn cli_factory_plan_auto_discovers_matching_role_contracts_from_ao_runspec_layout() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("factory-target");
    init_git_repo(&target);
    let factory_repo = temp.path().join("factory-v3");
    let runspec = factory_repo.join("ao/runspecs/factory-v3-smoke.yaml");
    fs::create_dir_all(runspec.parent().unwrap()).unwrap();
    fs::write(
        &runspec,
        r#"apiVersion: ao.dev/v1
kind: Run
metadata:
  name: factory-v3-smoke
verifier:
  command: python -m pytest -q
spec:
  tasks:
    - id: planner-intake
      kind: agent
      deps: []
      spec:
        provider: codex
    - id: implementer-slice
      kind: agent
      deps: ["planner-intake"]
      spec:
        provider: codex
    - id: evaluator-closer
      kind: agent
      deps: ["implementer-slice"]
      spec:
        provider: codex
"#,
    )
    .unwrap();
    let agents = factory_repo.join("agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(
        agents.join("intake.toml"),
        r#"name = "intake"
description = "Captures and classifies raw user intent."
inputs = ["user intent"]
outputs = ["intake brief"]
status_required = true
"#,
    )
    .unwrap();
    fs::write(
        agents.join("implementer.toml"),
        r#"name = "implementer"
description = "Executes the scoped implementation slice."
inputs = ["slice contract"]
outputs = ["diff artifact", "test evidence"]
status_required = true
"#,
    )
    .unwrap();
    fs::write(
        agents.join("evaluator-closer.toml"),
        r#"name = "evaluator-closer"
description = "Validates final artifacts against evidence."
inputs = ["hardened plan", "verification evidence"]
outputs = ["acceptance decision", "closure evidence"]
status_required = true
"#,
    )
    .unwrap();
    fs::write(
        agents.join("unused-role.toml"),
        r#"name = "unused-role"
description = "Should not be loaded for this runspec."
inputs = []
outputs = []
status_required = true
"#,
    )
    .unwrap();
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: Auto role contracts\nobjective: Load AO Operator role contracts from the sibling agents directory.\n",
    )
    .unwrap();
    let out = temp.path().join("auto-contract-plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap())
            .unwrap();

    assert_eq!(
        materialized["ao2_native_plan"]["role_contract_discovery"]["mode"],
        "auto_discovered_from_ao_runspec_layout"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["role_contract_discovery"]["loaded_count"],
        3
    );
    assert_eq!(
        materialized["factory_v3_inputs"]["role_contracts"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let roles = materialized["ao2_native_plan"]["roles"].as_array().unwrap();
    let role_ids: Vec<_> = roles
        .iter()
        .map(|role| role["role_id"].as_str().unwrap())
        .collect();
    assert_eq!(role_ids, vec!["intake", "implementer", "evaluator-closer"]);
    assert!(roles
        .iter()
        .all(|role| role["source"] == "factory-v3-role-contract"));
    assert!(!serde_json::to_string(&materialized)
        .unwrap()
        .contains("unused-role.toml"));
}

#[test]
fn cli_factory_plan_translates_factory_profile_roles_without_factory_driver() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: Profile compatibility\nobjective: Materialize factory-v3 profile roles directly as an AO2-native governed plan.\n",
    )
    .unwrap();
    let profile = temp.path().join("profile.json");
    fs::write(
        &profile,
        r#"{
  "profile": "synthetic-finserv",
  "schema": "factory-v3/profile/v1",
  "version": "0.1.0",
  "policy_posture": {
    "shell": { "deny_prefixes": ["git push --force"] },
    "network": { "egress_default": "deny" }
  },
  "roles": [
    {
      "id": "planner-intake",
      "role": "Planner Intake",
      "provider_key": "FACTORY_V3_PLANNER_PROVIDER",
      "deps": [],
      "reads": ["request"],
      "writes": ["plan.md"],
      "instructions": ["classify size and shape"]
    },
    {
      "id": "evaluator-closer",
      "role": "Evaluator Closer",
      "provider_key": "FACTORY_V3_EVALUATOR_PROVIDER",
      "deps": ["planner-intake"],
      "reads": ["plan.md", "evidence"],
      "writes": ["decision.json"],
      "instructions": ["close only with evidence"]
    }
  ]
}
"#,
    )
    .unwrap();
    let out = temp.path().join("profile-plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--profile",
        profile.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap())
            .unwrap();
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(json["workflow_path"].as_str().unwrap()).unwrap())
            .unwrap();

    let roles = materialized["ao2_native_plan"]["roles"].as_array().unwrap();
    assert_eq!(roles.len(), 2);
    assert_eq!(roles[0]["role_id"], "planner-intake");
    assert_eq!(roles[0]["source"], "factory-v3-profile-role");
    assert_eq!(roles[0]["provider_profile"], "FACTORY_V3_PLANNER_PROVIDER");
    assert_eq!(roles[0]["reads"][0], "request");
    assert_eq!(roles[1]["deps"][0], "planner-intake");
    assert_eq!(
        materialized["ao2_native_plan"]["factory_v3_translation"]["source"],
        "factory-v3-profile"
    );
    assert!(
        materialized["ao2_native_plan"]["factory_v3_translation"]["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| provider == "FACTORY_V3_EVALUATOR_PROVIDER")
    );
    assert_eq!(
        materialized["ao2_native_plan"]["profile_policy_posture"]["network"]["egress_default"],
        "deny"
    );
    assert_eq!(
        workflow["tasks"][0]["provider_profile"],
        "FACTORY_V3_PLANNER_PROVIDER"
    );
    assert_eq!(
        workflow["dependencies"][0]["source"],
        "factory-v3-profile-role-deps"
    );
    assert_eq!(
        workflow["policy"]["profile_policy_posture"]["shell"]["deny_prefixes"][0],
        "git push --force"
    );
}

#[test]
fn cli_factory_plan_redacts_secrets_from_materialized_plan_and_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: Token-safe planning
objective: Refactor governed workflow with bearer sk-live-secret-token and api_token=fixture-token.
acceptance:
  - Never expose Authorization: Bearer ghp_should_not_leak in AO2 planning artifacts.
"#,
    )
    .unwrap();
    let out = temp.path().join("redacted-plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let plan_text = fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap();
    let workflow_text = fs::read_to_string(json["workflow_path"].as_str().unwrap()).unwrap();
    let evidence_text =
        fs::read_to_string(json["planning_evidence_path"].as_str().unwrap()).unwrap();
    let combined = format!("{plan_text}\n{workflow_text}\n{evidence_text}");

    for secret_fragment in [
        "sk-live-secret-token",
        "api_token=fixture-token",
        "ghp_should_not_leak",
    ] {
        assert!(
            !combined.contains(secret_fragment),
            "factory planning artifact leaked secret fragment {secret_fragment}: {combined}"
        );
    }
    assert!(
        combined.contains("[REDACTED") || combined.contains("redacted"),
        "factory planning artifacts should include explicit redaction evidence"
    );
}

#[test]
fn cli_factory_plan_rejects_factory_inputs_that_request_provider_api_key_auth() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(&request, "title: Provider auth boundary\nobjective: keep AO2 factory parity on local OAuth CLI auth.\n").unwrap();
    let profile = temp.path().join("profile.yaml");
    fs::write(
        &profile,
        r#"provider: codex
auth:
  kind: api_key
  env: OPENAI_API_KEY
"#,
    )
    .unwrap();

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--profile",
        profile.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        !plan.status.success(),
        "factory plan unexpectedly accepted API-key auth profile"
    );
    let error = stderr(&plan);
    assert!(
        error.contains("provider API-key authentication is forbidden"),
        "unexpected stderr: {error}"
    );
    assert!(
        !error.contains("sk-live") && !error.contains("Bearer "),
        "stderr must not leak provider credentials: {error}"
    );
}

#[test]
fn cli_factory_plan_materializes_role_contract_gate_for_evaluator_closure() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: Contract gate parity\nobjective: Preserve AO Operator role output obligations inside AO2-native evaluator closure.\n",
    )
    .unwrap();
    let runspec = temp.path().join("factory-v3-smoke.yaml");
    fs::write(
        &runspec,
        r#"apiVersion: ao.dev/v1
kind: Run
metadata:
  name: contract-gate-smoke
spec:
  tasks:
    - id: implementer-slice
      kind: agent
      deps: []
      spec:
        provider: codex
    - id: evaluator-closer
      kind: agent
      deps: ["implementer-slice"]
      spec:
        provider: claude
"#,
    )
    .unwrap();
    let out = temp.path().join("contract-gate-plan.json");

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
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap())
            .unwrap();

    let gate = &materialized["ao2_native_plan"]["role_contract_gate"];
    assert_eq!(gate["owner"], "ao2-native-evaluator-closer");
    assert_eq!(gate["factory_v3_role"], "parity_oracle_only");
    assert_eq!(gate["status"], "satisfied_at_plan_time");
    assert_eq!(
        gate["required_outputs"],
        serde_json::json!([
            "evidence",
            "concerns",
            "blockers",
            "changed_files",
            "sandbox",
            "secret_redaction"
        ])
    );
    assert_eq!(gate["role_count"], 2);
    assert_eq!(gate["missing_obligations"].as_array().unwrap().len(), 0);
    assert_eq!(gate["matrix"][0]["role_id"], "implementer-slice");
    assert_eq!(gate["matrix"][0]["satisfied"], true);
    assert!(gate["matrix"][0]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|output| output == "secret_redaction"));

    let evidence: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(json["planning_evidence_path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        evidence["role_contract_gate"]["status"],
        "satisfied_at_plan_time"
    );
    assert_eq!(
        evidence["role_contract_gate"]["owner"],
        "ao2-native-evaluator-closer"
    );
}

#[test]
fn cli_factory_plan_translates_factory_runspec_task_graph_without_role_contracts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: Runspec compatibility\nobjective: Materialize AO Operator task graph as AO2-native governed execution.\n",
    )
    .unwrap();
    let runspec = temp.path().join("factory-v3-smoke.yaml");
    fs::write(
        &runspec,
        r#"apiVersion: ao.dev/v1
kind: Run
metadata:
  name: factory-v3-smoke
spec:
  tasks:
    - id: planner-intake
      kind: agent
      deps: []
      spec:
        provider: codex
        agent: codex-default
        promptFile: ao/prompts/planner-intake.md
        policyProfile: ao/policy/local-dev.yaml
    - id: evaluator-closer
      kind: agent
      deps: ["planner-intake"]
      spec:
        provider: claude
        agent: claude-default
        promptFile: ao/prompts/evaluator-closer.md
        policyProfile: ao/policy/local-dev.yaml
"#,
    )
    .unwrap();
    let out = temp.path().join("runspec-plan.json");

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
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap())
            .unwrap();
    let translation = &materialized["ao2_native_plan"]["factory_v3_translation"];
    assert_eq!(translation["source"], "factory-v3-runspec");
    assert_eq!(translation["task_count"], 2);
    assert_eq!(translation["role_ids"][0], "planner-intake");
    assert_eq!(translation["role_ids"][1], "evaluator-closer");
    assert_eq!(translation["providers"][0], "claude");
    assert_eq!(translation["providers"][1], "codex");
    assert_eq!(
        translation["direct_dependency_edges"][0][0],
        "planner-intake"
    );
    assert_eq!(
        translation["direct_dependency_edges"][0][1],
        "evaluator-closer"
    );
    assert_eq!(
        translation["task_graph"][0]["prompt_file"],
        "ao/prompts/planner-intake.md"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["roles"][0]["source"],
        "factory-v3-runspec-task"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["roles"][0]["status_required"],
        true
    );
    assert!(materialized["ao2_native_plan"]["roles"][0]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|output| output == "changed_files"));
    assert!(materialized["ao2_native_plan"]["roles"][0]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|output| output == "secret_redaction"));
    let workflow_path = Path::new(json["workflow_path"].as_str().unwrap());
    let workflow: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(workflow_path).unwrap()).unwrap();
    assert_eq!(workflow["roles"][0], "planner-intake");
    assert_eq!(workflow["roles"][1], "evaluator-closer");
    assert_eq!(workflow["tasks"][0]["id"], "planner-intake");
    assert_eq!(workflow["tasks"][1]["provider"], "claude");
    assert_eq!(workflow["dependencies"][0]["from"], "planner-intake");
    assert_eq!(workflow["dependencies"][0]["to"], "evaluator-closer");
    assert_eq!(
        workflow["evaluator"]["owner"],
        "ao2-native-evaluator-closer"
    );
}

#[test]
fn cli_factory_plan_translates_legacy_factory_roles_runspec() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: Legacy factory roles\nobjective: Load factory-v3 role contracts from a legacy roles runspec.\n",
    )
    .unwrap();
    let runspec = temp.path().join("legacy-runspec.yaml");
    fs::write(
        &runspec,
        r#"schema: factory-v3/runspec/v1
slug: bug-fix
profile: bug-fix
brief: examples/starters/bug-fix-example.md
roles:
- id: intake
  provider_key: FACTORY_V3_PLANNER_PROVIDER
  host_tag: []
  deps: []
  reads:
  - task brief
  - failing test output
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
gates:
  gate_b: true
  gate_r: true
"#,
    )
    .unwrap();
    let out = temp.path().join("legacy-plan.json");

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
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap())
            .unwrap();
    let translation = &materialized["ao2_native_plan"]["factory_v3_translation"];
    assert_eq!(translation["source"], "factory-v3-legacy-roles-runspec");
    assert_eq!(translation["task_count"], 3);
    assert_eq!(translation["role_ids"][0], "intake");
    assert_eq!(translation["role_ids"][1], "planner");
    assert_eq!(translation["role_ids"][2], "evaluator-closer");
    assert_eq!(translation["direct_dependency_edges"][0][0], "intake");
    assert_eq!(translation["direct_dependency_edges"][0][1], "planner");
    assert_eq!(
        translation["task_graph"][0]["provider_profile"],
        "FACTORY_V3_PLANNER_PROVIDER"
    );
    assert_eq!(translation["task_graph"][0]["reads"][0], "task brief");
    assert_eq!(
        translation["task_graph"][2]["writes"][0],
        "docs/evaluations/<slug>-evaluation.md"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["provider_profiles"][0],
        "FACTORY_V3_EVALUATOR_CLOSER_PROVIDER"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["provider_profiles"][1],
        "FACTORY_V3_PLANNER_PROVIDER"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["roles"][0]["source"],
        "factory-v3-legacy-runspec-role"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["roles"][0]["outputs"][0],
        "evidence"
    );
    let workflow_path = Path::new(json["workflow_path"].as_str().unwrap());
    let workflow: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(workflow_path).unwrap()).unwrap();
    assert_eq!(workflow["roles"][0], "intake");
    assert_eq!(workflow["tasks"][1]["role"], "planner");
    assert_eq!(
        workflow["tasks"][1]["provider_profile"],
        "FACTORY_V3_PLANNER_PROVIDER"
    );
    assert_eq!(workflow["dependencies"][1]["from"], "planner");
    assert_eq!(workflow["dependencies"][1]["to"], "evaluator-closer");
    assert_eq!(
        workflow["factory_v3_compatibility"]["legacy_roles_runspec"],
        true
    );
}

#[test]
fn cli_factory_plan_rejects_runspec_dependency_on_unknown_task_before_materializing_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(&request, "title: Invalid dependency graph\n").unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        r#"apiVersion: ao.dev/v1
kind: Run
spec:
  tasks:
    - id: evaluator-closer
      kind: agent
      deps: ["planner-intake"]
      spec:
        provider: codex
"#,
    )
    .unwrap();

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        !plan.status.success(),
        "factory plan accepted a RunSpec with an unknown dependency"
    );
    let error = stderr(&plan);
    assert!(
        error.contains("RunSpec dependency planner-intake for task evaluator-closer does not reference a known task"),
        "unexpected stderr: {error}"
    );
    assert!(
        !error.contains("Bearer ") && !error.contains("sk-"),
        "dependency validation stderr must not leak secrets: {error}"
    );
}

#[test]
fn cli_factory_plan_rejects_duplicate_runspec_task_ids_before_queue_or_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(&request, "title: Duplicate task ids\n").unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        r#"schema: factory-v3/runspec/v1
roles:
- id: planner
  provider_key: FACTORY_V3_PLANNER_PROVIDER
  deps: []
- id: planner
  provider_key: FACTORY_V3_EVALUATOR_PROVIDER
  deps: []
"#,
    )
    .unwrap();

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        !plan.status.success(),
        "factory plan accepted duplicate legacy RunSpec role ids"
    );
    let error = stderr(&plan);
    assert!(
        error.contains("RunSpec contains duplicate task or role id planner"),
        "unexpected stderr: {error}"
    );
}

#[test]
fn cli_factory_plan_rejects_cyclic_runspec_graph_before_materializing_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(&request, "title: Cyclic RunSpec graph\n").unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        r#"apiVersion: ao.dev/v1
kind: Run
spec:
  tasks:
    - id: planner-intake
      kind: agent
      deps: ["evaluator-closer"]
      spec:
        provider: codex
    - id: evaluator-closer
      kind: agent
      deps: ["planner-intake"]
      spec:
        provider: claude
"#,
    )
    .unwrap();

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        !plan.status.success(),
        "factory plan accepted a cyclic RunSpec graph"
    );
    let error = stderr(&plan);
    assert!(
        error.contains("RunSpec contains a dependency cycle involving task id(s):"),
        "unexpected stderr: {error}"
    );
    assert!(
        error.contains("planner-intake") && error.contains("evaluator-closer"),
        "cycle diagnostic should identify the cyclic task ids: {error}"
    );
}

#[test]
fn cli_factory_plan_rejects_cyclic_factory_profile_roles_before_materializing_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(&request, "title: Cyclic profile graph\n").unwrap();
    let profile = temp.path().join("profile.yaml");
    fs::write(
        &profile,
        r#"schema: factory-v3/profile/v1
roles:
  - id: planner-intake
    role: Planner Intake
    provider_key: FACTORY_V3_PLANNER_PROVIDER
    deps: ["evaluator-closer"]
  - id: evaluator-closer
    role: Evaluator Closer
    provider_key: FACTORY_V3_EVALUATOR_PROVIDER
    deps: ["planner-intake"]
"#,
    )
    .unwrap();

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--profile",
        profile.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        !plan.status.success(),
        "factory plan accepted a cyclic factory profile graph"
    );
    let error = stderr(&plan);
    assert!(
        error.contains("factory profile contains a dependency cycle involving role id(s):"),
        "unexpected stderr: {error}"
    );
    assert!(
        error.contains("planner-intake") && error.contains("evaluator-closer"),
        "profile cycle diagnostic should identify the cyclic role ids: {error}"
    );
}

#[test]
fn cli_factory_plan_defaults_missing_runspec_task_kind_to_agent_role() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let request = temp.path().join("request.yaml");
    fs::write(&request, "title: Missing kind compatibility\n").unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        r#"spec:
  tasks:
    - id: planner-intake
      deps: []
      spec:
        provider: codex
"#,
    )
    .unwrap();
    let out = temp.path().join("missing-kind-plan.json");

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
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    let materialized: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(json["plan_path"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        materialized["ao2_native_plan"]["factory_v3_translation"]["task_graph"][0]["kind"],
        "agent"
    );
    assert_eq!(
        materialized["ao2_native_plan"]["factory_v3_translation"]["role_ids"][0],
        "planner-intake"
    );
    let workflow_path = Path::new(json["workflow_path"].as_str().unwrap());
    let workflow: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(workflow_path).unwrap()).unwrap();
    assert_eq!(workflow["roles"][0], "planner-intake");
    assert_eq!(workflow["tasks"][0]["kind"], "agent");
}

#[test]
fn cli_factory_plan_can_sign_planning_evidence_without_factory_driver() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    init_git_repo(&repo);
    let signing_key = temp.path().join("factory-planning-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        "title: AO2 native governed execution\nobjective: Implement provider parity and closure evidence for Windows macOS Ubuntu.\n",
    )
    .unwrap();
    let out = temp.path().join("signed-plan.json");

    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "factory-compat-planner",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json: serde_json::Value = serde_json::from_str(&stdout(&plan)).unwrap();
    assert_eq!(json["signature"]["signature_verified"], true);
    assert_eq!(json["signature"]["signer_id"], "factory-compat-planner");
    let evidence_path = Path::new(json["planning_evidence_path"].as_str().unwrap());
    let evidence_dir = evidence_path.parent().unwrap();
    assert!(evidence_dir
        .join(json["signature"]["signature_path"].as_str().unwrap())
        .is_file());
    assert!(evidence_dir
        .join(json["signature"]["public_key_path"].as_str().unwrap())
        .is_file());

    let evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(evidence_path).unwrap()).unwrap();
    assert_eq!(
        evidence["signed_evidence_status"],
        "signed-and-verified-planning-evidence"
    );
    assert_eq!(
        evidence["classification"]["factory_v3_required_before_classification"],
        false
    );
}
