use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::cli::WorkbenchCommand;
use crate::cli_util::{
    form_value_owned, generate_api_token, json_string, open_report_target, percent_decode,
    percent_encode, shell_quote,
};
use crate::control_plane_http::{control_plane_endpoint, get_text_http};
use crate::control_plane_ops::{
    workbench_support_bundle_import, workbench_support_bundle_inspect,
    workbench_support_bundle_verify, workbench_support_keygen,
};
use crate::factory_evidence::{factory_plan_json, FactoryPlanSigning};
use crate::factory_governance::validate_factory_replacement_smoke_run_id;
use crate::provider_ops::{
    provider_pilot_json, provider_profiles, provider_score_json, provider_warning_strings,
    ProviderPilotOptions,
};
use crate::workbench_contract::{
    WorkbenchOperator, WorkbenchOperatorRole, WorkbenchSupportSigning,
};
use crate::workbench_render::{render_workbench, WorkbenchRenderOptions};
use crate::workbench_server::{serve_workbench, ServeWorkbenchOptions};
use crate::TASK_TEMPLATES;

pub(super) fn workbench_export(
    target: PathBuf,
    out: Option<PathBuf>,
    open: bool,
    html: String,
) -> Result<()> {
    let path = out.unwrap_or_else(|| target.join(".ao2").join("workbench").join("index.html"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, html).with_context(|| format!("write {}", path.display()))?;
    println!("workbench={}", path.display());
    if open {
        open_report_target(&path)?;
        println!("open_target={}", path.display());
    }
    Ok(())
}

pub(super) fn build_workbench_operators(
    admin_token: String,
    operator_tokens: Vec<String>,
) -> Result<Vec<WorkbenchOperator>> {
    let mut operators = vec![WorkbenchOperator {
        id: "local-admin".to_string(),
        role: WorkbenchOperatorRole::Admin,
        token: admin_token,
    }];
    for operator_token in operator_tokens {
        let operator = parse_workbench_operator_token(&operator_token)?;
        if operators
            .iter()
            .any(|existing| existing.token == operator.token)
        {
            return Err(anyhow!(
                "duplicate workbench operator token for {}",
                operator.id
            ));
        }
        operators.push(operator);
    }
    Ok(operators)
}

fn parse_workbench_operator_token(value: &str) -> Result<WorkbenchOperator> {
    let fields = value.split(':').collect::<Vec<_>>();
    if fields.len() != 3 || fields.iter().any(|field| field.trim().is_empty()) {
        return Err(anyhow!(
            "invalid workbench operator token format; expected <id>:<role>:<token>"
        ));
    }
    let role = match fields[1].trim() {
        "viewer" => WorkbenchOperatorRole::Viewer,
        "operator" => WorkbenchOperatorRole::Operator,
        "admin" => WorkbenchOperatorRole::Admin,
        other => return Err(anyhow!("invalid workbench operator role {other}")),
    };
    Ok(WorkbenchOperator {
        id: fields[0].trim().to_string(),
        role,
        token: fields[2].trim().to_string(),
    })
}

pub(super) fn workbench_evidence_control_plane_dashboard_json(
    form: &std::collections::BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let control_plane_url = form
        .get("control_plane_url")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("control_plane_url is required")?;
    let api_token = form
        .get("api_token")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("api_token is required")?;
    let gate = form
        .get("gate")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "attention".to_string());
    let path = if gate == "all" {
        "/api/v1/evidence-pack/dashboard".to_string()
    } else {
        format!(
            "/api/v1/evidence-pack/dashboard?gate={}",
            percent_encode(&gate)
        )
    };
    let endpoint = control_plane_endpoint(&control_plane_url, &path)?;
    let dashboard_html = get_text_http(&endpoint, &api_token)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.evidence-control-plane-dashboard.v1",
        "endpoint": endpoint,
        "gate": gate,
        "dashboard_html": dashboard_html
    }))
}

pub(super) fn templates_json() -> serde_json::Value {
    let templates = TASK_TEMPLATES
        .iter()
        .map(|template| {
            serde_json::json!({
                "name": template.name,
                "description": template.description
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": "ao2.templates.v1",
        "templates": templates,
        "providers": provider_profiles()
            .iter()
            .map(|profile| profile.name)
            .collect::<Vec<_>>()
    })
}

pub(super) fn parse_form_body(body: &str) -> std::collections::BTreeMap<String, String> {
    let mut form = std::collections::BTreeMap::new();
    for part in body.split('&') {
        if part.is_empty() {
            continue;
        }
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        form.insert(percent_decode(key), percent_decode(value));
    }
    form
}

pub(super) fn workbench_launch_json(
    target: &Path,
    form: &std::collections::BTreeMap<String, String>,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let template = form
        .get("template")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| "bug-fix".to_string());
    if !TASK_TEMPLATES.iter().any(|entry| entry.name == template) {
        anyhow::bail!("unknown template: {template}");
    }

    let provider = form
        .get("provider")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| "scripted".to_string());
    if !provider_profiles()
        .iter()
        .any(|entry| entry.name == provider)
    {
        anyhow::bail!("unknown provider: {provider}");
    }
    let provider_warnings = provider_warning_strings(&provider)?;

    let run_id = form
        .get("run_id")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| format!("workbench-{}", generate_api_token()));
    validate_factory_replacement_smoke_run_id(&run_id)?;
    let max_repair_attempts = form
        .get("max_repair_attempts")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1);
    let minimum_score = form
        .get("minimum_score")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let scorecard = validate_minimum_provider_score(target, &run_id, minimum_score)?;
    let provider_prompt_file = form
        .get("provider_prompt_file")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let ao_operator_runspec = form
        .get("ao_operator_runspec")
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| !path.as_os_str().is_empty());
    if let Some(path) = ao_operator_runspec.as_ref() {
        if !path.is_file() {
            anyhow::bail!("ao_operator_runspec does not exist: {}", path.display());
        }
    }
    let support_signing = support_signing.context(
        "workbench launch requires --support-signing-key so AO2 can build a signed factory governed-run command",
    )?;
    let launch_dir = target
        .join(".ao2")
        .join("workbench")
        .join("governed-launches")
        .join(&run_id);
    fs::create_dir_all(&launch_dir)
        .with_context(|| format!("create workbench launch dir {}", launch_dir.display()))?;
    let request_path = launch_dir.join("request.yaml");
    let runspec_path = launch_dir.join("runspec.yaml");
    let out_dir = launch_dir.join("out");
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create workbench launch out dir {}", out_dir.display()))?;
    write_workbench_governed_request(&request_path, &run_id, &template, &provider)?;
    let effective_runspec_path = if let Some(path) = ao_operator_runspec.as_ref() {
        path.clone()
    } else {
        write_workbench_governed_runspec(&runspec_path, &run_id)?;
        runspec_path
    };
    let preflight_plan_path = launch_dir.join("preflight-plan.json");
    let preflight_plan = factory_plan_json(
        &request_path,
        None,
        Some(&effective_runspec_path),
        &[],
        FactoryPlanSigning {
            key: Some(&support_signing.key_path),
            signer_id: &support_signing.signer_id,
        },
        target,
        Some(&preflight_plan_path),
    )?;
    let role_contract_discovery =
        preflight_plan["ao2_native_plan"]["role_contract_discovery"].clone();
    let auto_loaded_role_contracts = role_contract_discovery["mode"]
        == "auto_discovered_from_ao_runspec_layout"
        && role_contract_discovery["loaded_count"]
            .as_u64()
            .is_some_and(|count| count > 0);

    let mut command = vec![
        "ao2".to_string(),
        "factory".to_string(),
        "governed-run".to_string(),
        "--request".to_string(),
        request_path.display().to_string(),
        "--runspec".to_string(),
        effective_runspec_path.display().to_string(),
        "--target".to_string(),
        target.display().to_string(),
        "--run-id".to_string(),
        run_id.clone(),
        "--signing-key".to_string(),
        support_signing.key_path.display().to_string(),
        "--signer-id".to_string(),
        support_signing.signer_id.clone(),
        "--max-repair-attempts".to_string(),
        max_repair_attempts.to_string(),
        "--out-dir".to_string(),
        out_dir.display().to_string(),
        "--json".to_string(),
    ];
    if let Some(path) = &provider_prompt_file {
        command.push("--provider".to_string());
        command.push(provider.clone());
        command.push("--provider-prompt-file".to_string());
        command.push(path.clone());
    }
    let shell_command = command
        .iter()
        .map(|part| shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ");

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-launch.v1",
        "status": "ready",
        "mode": "command_preview",
        "launch_surface": "factory-governed-run",
        "safety": "browser builds a signed factory governed-run command preview; execution remains explicit in the local shell",
        "target": target.display().to_string(),
        "run_id": run_id,
        "template": template,
        "provider": provider,
        "provider_warnings": provider_warnings,
        "minimum_score": minimum_score,
        "provider_score": scorecard,
        "max_repair_attempts": max_repair_attempts,
        "provider_prompt_file": provider_prompt_file,
        "provider_execution_mode": if provider_prompt_file.is_some() { "provider_backed" } else { "provider_free_preview" },
        "request_path": request_path.display().to_string(),
        "runspec_path": effective_runspec_path.display().to_string(),
        "out_dir": out_dir.display().to_string(),
        "role_contract_discovery": role_contract_discovery,
        "launch_preflight": {
            "status": "planned",
            "plan_path": preflight_plan_path.display().to_string(),
            "planning_evidence_path": preflight_plan["planning_evidence_path"].clone(),
            "ao2_auto_loaded_role_contracts": auto_loaded_role_contracts,
            "factory_v3_required_to_plan": false,
            "execution_performed": false
        },
        "signing_key_required": true,
        "signing_key_supplied": true,
        "signer_id": support_signing.signer_id,
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-governed-run",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "command": command,
        "shell_command": shell_command
    }))
}

fn write_workbench_governed_request(
    path: &Path,
    run_id: &str,
    template: &str,
    provider: &str,
) -> Result<()> {
    let content = format!(
        "title: AO2 workbench factory governed-run {run_id}\nobjective: |\n  Execute an AO2-native factory governed-run from the workbench launch surface.\n  AO2 owns the trusted decision, signature, memory, and evidence boundary.\ncontext: |\n  The launch was prepared by AO2 workbench for template '{template}' and provider profile '{provider}'.\n  factory-v3/AO Operator remains parity oracle and evaluator discipline reference.\nacceptance:\n  - factory governed-run completes with signed evaluator decision evidence\n  - evidence pack is materialized under the launch output directory\n  - ao2-control-plane remains a read-only observer after signed evidence exists\n"
    );
    fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

fn write_workbench_governed_runspec(path: &Path, run_id: &str) -> Result<()> {
    let content = format!(
        "run_id: {run_id}\nverifier_command: python -m pytest -q\nsuccess_criteria:\n  - governed-run status is accepted\n  - evaluator decision signature verifies\n  - evidence pack digest is recorded\n"
    );
    fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

#[derive(Debug)]
pub(super) struct WorkbenchMinimumScoreError {
    pub(super) run_id: String,
    pub(super) minimum_score: u64,
    pub(super) score: u64,
    pub(super) verdict: String,
}

impl std::fmt::Display for WorkbenchMinimumScoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "minimum_provider_score_not_met")
    }
}

impl std::error::Error for WorkbenchMinimumScoreError {}

pub(super) fn parse_optional_budget_form(
    form: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<Option<f64>> {
    let Some(value) = form_value_owned(form, key) else {
        return Ok(None);
    };
    let budget = value
        .parse::<f64>()
        .with_context(|| format!("{key} must be a number"))?;
    if !budget.is_finite() || budget <= 0.0 {
        anyhow::bail!("{key} must be a positive finite number");
    }
    Ok(Some(budget))
}

pub(super) fn validate_minimum_provider_score(
    target: &Path,
    run_id: &str,
    minimum_score: u64,
) -> Result<serde_json::Value> {
    if minimum_score == 0 {
        return Ok(serde_json::Value::Null);
    }
    let scorecard =
        provider_score_json(target, run_id).map_err(|_| WorkbenchMinimumScoreError {
            run_id: run_id.to_string(),
            minimum_score,
            score: 0,
            verdict: "missing".to_string(),
        })?;
    let score = crate::cli_util::json_u64(&scorecard, "score");
    if score < minimum_score {
        return Err(WorkbenchMinimumScoreError {
            run_id: run_id.to_string(),
            minimum_score,
            score,
            verdict: json_string(&scorecard, "verdict"),
        }
        .into());
    }
    Ok(scorecard)
}

pub(super) fn workbench_provider_pilot_json(
    target: &Path,
    form: &std::collections::BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let provider = form
        .get("provider")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .context("provider is required")?;
    let template = form
        .get("template")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| "bug-fix".to_string());
    let run_id = form
        .get("run_id")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string());
    let provider_prompt_file = form
        .get("provider_prompt_file")
        .filter(|value| !value.trim().is_empty())
        .map(|value| PathBuf::from(value.trim()))
        .context("provider_prompt_file is required")?;
    let max_repair_attempts = form
        .get("max_repair_attempts")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1);
    let minimum_score = form
        .get("minimum_score")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(90);
    let max_budget_usd = parse_optional_budget_form(form, "max_budget_usd")?;

    provider_pilot_json(&ProviderPilotOptions {
        target: target.to_path_buf(),
        provider,
        template,
        run_id,
        provider_prompt_file,
        max_repair_attempts,
        max_budget_usd,
        minimum_score,
        json: true,
    })
}

pub(super) fn workbench_provider_pilot_preflight_json(
    target: &Path,
    form: &std::collections::BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let provider = form_value_owned(form, "provider").unwrap_or_default();
    let template = form_value_owned(form, "template").unwrap_or_else(|| "bug-fix".to_string());
    let prompt_file = form_value_owned(form, "provider_prompt_file").unwrap_or_default();
    let mut checks = Vec::new();
    let mut valid = true;

    let provider_ok = matches!(provider.as_str(), "codex" | "claude" | "antigravity");
    if !provider_ok {
        valid = false;
    }
    checks.push(serde_json::json!({
        "name": "provider",
        "status": if provider_ok { "passed" } else { "failed" },
        "message": if provider_ok {
            format!("provider {provider} is supported for provider pilots")
        } else if provider.is_empty() {
            "provider is required".to_string()
        } else {
            format!("provider pilot requires codex, claude, or antigravity, got {provider}")
        }
    }));

    let template_ok = TASK_TEMPLATES
        .iter()
        .any(|candidate| candidate.name == template);
    if !template_ok {
        valid = false;
    }
    checks.push(serde_json::json!({
        "name": "template",
        "status": if template_ok { "passed" } else { "failed" },
        "message": if template_ok {
            format!("template {template} is available")
        } else {
            format!("unknown template: {template}")
        }
    }));

    let prompt_path = PathBuf::from(&prompt_file);
    let prompt_ok = !prompt_file.is_empty() && prompt_path.is_file();
    if !prompt_ok {
        valid = false;
    }
    checks.push(serde_json::json!({
        "name": "prompt_file",
        "status": if prompt_ok { "passed" } else { "failed" },
        "message": if prompt_file.is_empty() {
            "provider_prompt_file is required".to_string()
        } else if prompt_ok {
            format!("provider prompt file exists: {}", prompt_path.display())
        } else {
            format!("provider prompt file does not exist: {}", prompt_path.display())
        }
    }));

    if !valid {
        checks.push(serde_json::json!({
            "name": "provider_gate",
            "status": "not_applicable",
            "message": "provider readiness gate was not evaluated because local preflight checks failed"
        }));
        return Ok(serde_json::json!({
            "schema_version": "ao2.workbench-provider-pilot-preflight.v1",
            "status": "invalid",
            "can_start": false,
            "checks": checks,
            "pilot": serde_json::Value::Null
        }));
    }

    let pilot = workbench_provider_pilot_json(target, form)?;
    let ready = json_string(&pilot, "status") == "ready";
    let gate_verdict = json_string(&pilot["gate"], "verdict");
    checks.push(serde_json::json!({
        "name": "provider_gate",
        "status": if ready { "passed" } else { "blocked" },
        "message": format!("provider gate verdict: {gate_verdict}"),
        "verdict": gate_verdict
    }));

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-provider-pilot-preflight.v1",
        "status": if ready { "ready" } else { "blocked" },
        "can_start": ready,
        "checks": checks,
        "pilot": pilot
    }))
}

pub(crate) fn workbench(command: WorkbenchCommand) -> Result<()> {
    match command {
        WorkbenchCommand::Export {
            target,
            out,
            open,
            provenance_dir,
        } => {
            let html = render_workbench(
                &target,
                &provenance_dir,
                WorkbenchRenderOptions {
                    operator: None,
                    execution_enabled: false,
                    can_operate: false,
                    release_comparison_signing_enabled: false,
                    control_plane_url: None,
                    release_gate_artifact_path: None,
                },
            )?;
            workbench_export(target, out, open, html)
        }
        WorkbenchCommand::Serve {
            target,
            host,
            port,
            once,
            provenance_dir,
            api_token,
            operator_tokens,
            enable_execution,
            queue_retention,
            control_plane_url,
            support_signing_key,
            support_signer_id,
        } => serve_workbench(ServeWorkbenchOptions {
            target,
            host,
            port,
            once,
            provenance_dir,
            api_token,
            operator_tokens,
            enable_execution,
            queue_retention,
            control_plane_url,
            support_signing_key,
            support_signer_id,
        }),
        WorkbenchCommand::SupportVerify { bundle_dir, json } => {
            workbench_support_bundle_verify(bundle_dir, json)
        }
        WorkbenchCommand::SupportImport {
            bundle_dir,
            out_dir,
            json,
        } => workbench_support_bundle_import(bundle_dir, out_dir, json),
        WorkbenchCommand::SupportInspect { bundle_dir, json } => {
            workbench_support_bundle_inspect(bundle_dir, json)
        }
        WorkbenchCommand::SupportKeygen { out, bits, json } => {
            workbench_support_keygen(out, bits, json)
        }
    }
}
