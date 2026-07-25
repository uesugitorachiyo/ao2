use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use ao2_adapters::{
    doctor_provider, parse_provider, provider_metadata, DEFAULT_PROVIDER_TIMEOUT_SECONDS,
};
use ao2_core::sha256_hex;
use ao2_runtime::{
    replay_run, run_risky_pr_with_provider_prompt, ProviderRunOptions, ReplayOptions,
};

use crate::cli_util::{
    base64_standard, hex_lower, json_array, json_f64, json_string, json_u64, run_dir,
    sha256_bytes_hex,
};
use crate::control_plane_http::{control_plane_endpoint, post_json_http};
use crate::provider_contract::provider_contract;
use crate::release_crypto::{public_key_pem_from_private_key, sign_bytes_with_private_key};
use crate::release_history::release_tag_sort_key;
use crate::run_resume::approve_and_resume_persisted_sandbox_patches;
use crate::workbench_provider_pilot_acceptance::{
    collect_provider_pilot_acceptance_bundles, provider_cost_ledger_release_tag,
    provider_pilot_acceptance_verification_json,
};
use crate::{
    atomic_write_text, format_budget_usd, generate_api_token, now_unix_ms, resolve_api_token,
    shell_quote, trimmed_required, ProviderCommand, TASK_TEMPLATES,
};

pub(crate) fn provider(command: ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::List => {
            for profile in provider_profiles() {
                println!(
                    "{}\t{}\t{}",
                    profile.name, profile.provider, profile.description
                );
            }
            Ok(())
        }
        ProviderCommand::Registry {
            control_plane_url,
            api_token,
            api_token_env,
            signing_key,
            signer_id,
            json,
        } => provider_registry(
            control_plane_url,
            api_token,
            api_token_env,
            signing_key,
            signer_id,
            json,
        ),
        ProviderCommand::Doctor { provider } => {
            let provider = parse_provider(&provider)?;
            let report = doctor_provider(provider)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        ProviderCommand::Matrix { json } => provider_matrix(json),
        ProviderCommand::Contract {
            provider,
            verify,
            require,
            json,
        } => provider_contract(provider, verify, require, json),
        ProviderCommand::SmokeAll {
            target,
            json,
            minimum_score,
            live_provider,
        } => provider_smoke_all(target, json, minimum_score, live_provider),
        ProviderCommand::Gate {
            target,
            require,
            minimum_score,
            json,
        } => provider_gate(target, require, minimum_score, json),
        ProviderCommand::Pilot {
            target,
            provider,
            template,
            run_id,
            provider_prompt_file,
            max_repair_attempts,
            provider_max_budget_usd,
            minimum_score,
            json,
        } => provider_pilot(ProviderPilotOptions {
            target,
            provider,
            template,
            run_id,
            provider_prompt_file,
            max_repair_attempts,
            max_budget_usd: provider_max_budget_usd,
            minimum_score,
            json,
        }),
        ProviderCommand::CostLedger {
            acceptance_root,
            json,
        } => provider_cost_ledger(acceptance_root, json),
        ProviderCommand::CostTrend {
            acceptance_root,
            json,
        } => provider_cost_trend(acceptance_root, json),
        ProviderCommand::Score {
            target,
            run_id,
            json,
        } => provider_score(target, run_id, json),
    }
}

fn provider_smoke_all(
    target: PathBuf,
    json_output: bool,
    minimum_score: u64,
    live_provider: Vec<String>,
) -> Result<()> {
    let report = provider_smoke_all_json(&target, minimum_score, &live_provider)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("schema={}", json_string(&report, "schema"));
        println!("minimum_score={}", json_u64(&report, "minimum_score"));
        for provider in json_array(&report, "providers") {
            println!(
                "{}\tavailable={}\tverdict={}\tscore={}",
                json_string(provider, "provider"),
                provider
                    .get("available")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                json_string(provider, "verdict"),
                json_u64(provider, "score")
            );
        }
    }
    Ok(())
}

pub(crate) fn provider_smoke_all_json(
    target: &Path,
    minimum_score: u64,
    live_providers: &[String],
) -> Result<serde_json::Value> {
    let live_provider_set = provider_smoke_live_provider_set(live_providers)?;
    let mut providers = Vec::new();
    for name in ["scripted", "codex", "claude", "antigravity"] {
        let provider = parse_provider(name)?;
        let doctor = doctor_provider(provider)?;
        let available = doctor.available;
        let should_run = name == "scripted" || live_provider_set.contains(name);
        if should_run && available {
            if let Some(guard_env) = provider_smoke_guard_env(name) {
                if std::env::var(guard_env).unwrap_or_default() != "1" {
                    providers.push(serde_json::json!({
                        "provider": name,
                        "available": available,
                        "doctor": doctor,
                        "run_id": "",
                        "score": 0,
                        "minimum_score": minimum_score,
                        "verdict": "guarded",
                        "guard_env": guard_env,
                        "scorecard": serde_json::Value::Null
                    }));
                    continue;
                }
            }
            providers.push(provider_smoke_run_json(
                target,
                name,
                provider,
                doctor,
                minimum_score,
            )?);
        } else {
            providers.push(serde_json::json!({
                "provider": name,
                "available": available,
                "doctor": doctor,
                "run_id": "",
                "score": 0,
                "minimum_score": minimum_score,
                "verdict": if available { "not_run" } else { "unavailable" },
                "scorecard": serde_json::Value::Null
            }));
        }
    }
    let mut report = serde_json::json!({
        "schema": "ao2.provider-smoke-all.v1",
        "target": target,
        "minimum_score": minimum_score,
        "live_providers": live_provider_set.iter().cloned().collect::<Vec<_>>(),
        "providers": providers
    });
    let history = record_provider_smoke_history(target, &report)?;
    report["history_path"] = serde_json::json!(provider_smoke_history_path(target));
    report["history_entry_count"] = serde_json::json!(json_u64(&history, "entry_count"));
    Ok(report)
}

fn provider_gate(
    target: PathBuf,
    require: Vec<String>,
    minimum_score: u64,
    json_output: bool,
) -> Result<()> {
    let report = provider_gate_json(&target, &require, minimum_score)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("schema={}", json_string(&report, "schema"));
        println!("verdict={}", json_string(&report, "verdict"));
        println!("minimum_score={}", json_u64(&report, "minimum_score"));
        println!("history_path={}", json_string(&report, "history_path"));
        for provider in json_array(&report, "providers") {
            println!(
                "{}\tverdict={}\tscore={}",
                json_string(provider, "provider"),
                json_string(provider, "verdict"),
                json_u64(provider, "score")
            );
        }
        for reason in json_array(&report, "reasons") {
            println!(
                "reason={}\tprovider={}\tmessage={}",
                json_string(reason, "code"),
                json_string(reason, "provider"),
                json_string(reason, "message")
            );
        }
    }
    if json_string(&report, "verdict") != "ready" {
        anyhow::bail!("provider readiness gate not ready");
    }
    Ok(())
}

fn provider_gate_json(
    target: &Path,
    require: &[String],
    minimum_score: u64,
) -> Result<serde_json::Value> {
    let required_providers = provider_gate_required_providers(require)?;
    let history = read_provider_smoke_history(target)?;
    let latest = &history["latest"];
    let mut provider_entries = Vec::new();
    let mut reasons = Vec::new();

    if latest.is_null() || json_array(&history, "entries").is_empty() {
        reasons.push(provider_gate_reason(
            "missing_history",
            "",
            &format!(
                "No provider smoke history found at {}",
                provider_smoke_history_path(target).display()
            ),
        ));
    }

    for required in &required_providers {
        let provider = json_array(latest, "providers")
            .iter()
            .find(|provider| json_string(provider, "provider") == *required)
            .cloned();
        let Some(provider) = provider else {
            provider_entries.push(serde_json::json!({
                "provider": required,
                "verdict": "missing",
                "score": 0_u64,
                "minimum_score": minimum_score
            }));
            reasons.push(provider_gate_reason(
                "provider_missing",
                required,
                &format!("Required provider {required} is missing from latest smoke history"),
            ));
            continue;
        };

        let score = json_u64(&provider, "score");
        let verdict = json_string(&provider, "verdict");
        provider_entries.push(serde_json::json!({
            "provider": required,
            "verdict": verdict,
            "score": score,
            "minimum_score": minimum_score,
            "run_id": json_string(&provider, "run_id"),
            "guard_env": json_string(&provider, "guard_env")
        }));
        if verdict != "ready" {
            reasons.push(provider_gate_reason(
                "provider_not_ready",
                required,
                &format!("Required provider {required} has verdict {verdict}"),
            ));
        }
        if score < minimum_score {
            reasons.push(provider_gate_reason(
                "score_below_minimum",
                required,
                &format!(
                    "Required provider {required} score {score} is below minimum {minimum_score}"
                ),
            ));
        }
    }

    let verdict = if reasons.is_empty() {
        "ready"
    } else {
        "not_ready"
    };
    Ok(serde_json::json!({
        "schema": "ao2.provider-readiness-gate.v1",
        "target": target,
        "history_path": provider_smoke_history_path(target),
        "history_entry_count": json_u64(&history, "entry_count"),
        "minimum_score": minimum_score,
        "required_providers": required_providers,
        "verdict": verdict,
        "providers": provider_entries,
        "reasons": reasons
    }))
}

pub(crate) struct ProviderPilotOptions {
    pub(crate) target: PathBuf,
    pub(crate) provider: String,
    pub(crate) template: String,
    pub(crate) run_id: Option<String>,
    pub(crate) provider_prompt_file: PathBuf,
    pub(crate) max_repair_attempts: usize,
    pub(crate) max_budget_usd: Option<f64>,
    pub(crate) minimum_score: u64,
    pub(crate) json: bool,
}

fn provider_pilot_readiness_recovery(
    target: &Path,
    provider: &str,
    minimum_score: u64,
) -> serde_json::Value {
    let smoke_command = vec![
        "ao2".to_string(),
        "provider".to_string(),
        "smoke-all".to_string(),
        "--target".to_string(),
        target.display().to_string(),
        "--live-provider".to_string(),
        provider.to_string(),
        "--minimum-score".to_string(),
        minimum_score.to_string(),
        "--json".to_string(),
    ];
    let guard_env = provider_smoke_guard_env(provider).unwrap_or("");
    let posix_shell_command = if guard_env.is_empty() {
        smoke_command
            .iter()
            .map(|part| shell_quote(part))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        let mut shell_parts = vec![format!("{guard_env}=1")];
        shell_parts.extend(smoke_command.iter().map(|part| shell_quote(part)));
        shell_parts.join(" ")
    };
    let powershell_command = if guard_env.is_empty() {
        smoke_command
            .iter()
            .map(|part| powershell_quote(part))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        let mut shell_parts = vec![format!("$env:{guard_env}='1';")];
        shell_parts.extend(smoke_command.iter().map(|part| powershell_quote(part)));
        shell_parts.join(" ")
    };

    serde_json::json!({
        "schema": "ao2.provider-readiness-recovery.v1",
        "status": "blocked_until_live_smoke_passes",
        "provider": provider,
        "guard_env": guard_env,
        "minimum_score": minimum_score,
        "history_path": provider_smoke_history_path(target),
        "smoke_command": smoke_command,
        "smoke_shell_command": posix_shell_command,
        "smoke_posix_shell_command": posix_shell_command,
        "smoke_powershell_command": powershell_command,
        "next_action": "run the smoke_command with the guard_env set to 1, then re-run provider pilot"
    })
}

fn powershell_quote(input: &str) -> String {
    if input
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '\\'))
    {
        input.to_string()
    } else {
        format!("'{}'", input.replace('\'', "''"))
    }
}

fn provider_pilot(options: ProviderPilotOptions) -> Result<()> {
    let report = provider_pilot_json(&options)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("schema={}", json_string(&report, "schema"));
        println!("status={}", json_string(&report, "status"));
        println!("provider={}", json_string(&report, "provider"));
        println!("template={}", json_string(&report, "template"));
        println!("run_id={}", json_string(&report, "run_id"));
        println!("gate_verdict={}", json_string(&report["gate"], "verdict"));
        println!("shell_command={}", json_string(&report, "shell_command"));
    }
    if json_string(&report, "status") != "ready" {
        anyhow::bail!("provider pilot gate not ready");
    }
    Ok(())
}

pub(crate) fn provider_pilot_json(options: &ProviderPilotOptions) -> Result<serde_json::Value> {
    let target = &options.target;
    let provider = options.provider.trim();
    let template = options.template.as_str();
    let provider_prompt_file = &options.provider_prompt_file;
    let max_repair_attempts = options.max_repair_attempts;
    let max_budget_usd = options.max_budget_usd;
    let minimum_score = options.minimum_score;
    let run_id = options.run_id.clone();

    parse_provider(provider)?;
    if provider == "scripted" {
        anyhow::bail!("provider pilot requires codex or claude");
    }
    if !provider_prompt_file.is_file() {
        anyhow::bail!(
            "provider prompt file does not exist: {}",
            provider_prompt_file.display()
        );
    }
    if !TASK_TEMPLATES
        .iter()
        .any(|candidate| candidate.name == template)
    {
        anyhow::bail!("unknown template: {template}");
    }

    let required = vec![provider.to_string()];
    let gate = provider_gate_json(target, &required, minimum_score)?;
    if json_string(&gate, "verdict") != "ready" {
        let readiness_recovery = provider_pilot_readiness_recovery(target, provider, minimum_score);
        return Ok(serde_json::json!({
            "schema": "ao2.provider-pilot-plan.v1",
            "status": "blocked",
            "mode": "command_preview",
            "provider": provider,
            "template": template,
            "run_id": run_id.unwrap_or_default(),
            "target": target,
            "workflow": "",
            "provider_prompt_file": provider_prompt_file,
            "max_repair_attempts": max_repair_attempts,
            "max_budget_usd": max_budget_usd,
            "minimum_score": minimum_score,
            "gate": gate,
            "readiness_recovery": readiness_recovery,
            "command": [],
            "shell_command": ""
        }));
    }

    let workflow = materialize_template_workflow(target, template)?;
    let run_id =
        run_id.unwrap_or_else(|| format!("provider-pilot-{provider}-{}", generate_api_token()));
    let mut command = vec![
        "ao2".to_string(),
        "run".to_string(),
        "--template".to_string(),
        template.to_string(),
        "--target".to_string(),
        target.display().to_string(),
        "--run-id".to_string(),
        run_id.clone(),
        "--provider".to_string(),
        provider.to_string(),
        "--provider-prompt-file".to_string(),
        provider_prompt_file.display().to_string(),
        "--max-repair-attempts".to_string(),
        max_repair_attempts.to_string(),
    ];
    if let Some(max_budget_usd) = max_budget_usd {
        command.push("--provider-max-budget-usd".to_string());
        command.push(format_budget_usd(max_budget_usd)?);
    }
    let shell_command = command
        .iter()
        .map(|part| shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    let approval_packet = provider_pilot_approval_packet(ProviderPilotApprovalInput {
        target,
        provider,
        template,
        run_id: &run_id,
        provider_prompt_file,
        max_repair_attempts,
        max_budget_usd,
        minimum_score,
        command: &command,
        shell_command: &shell_command,
    })?;

    Ok(serde_json::json!({
        "schema": "ao2.provider-pilot-plan.v1",
        "status": "ready",
        "mode": "command_preview",
        "provider": provider,
        "template": template,
        "run_id": run_id,
        "target": target,
        "workflow": workflow,
        "provider_prompt_file": provider_prompt_file,
        "max_repair_attempts": max_repair_attempts,
        "max_budget_usd": max_budget_usd,
        "minimum_score": minimum_score,
        "gate": gate,
        "command": command,
        "shell_command": shell_command,
        "approval_packet": approval_packet
    }))
}

struct ProviderPilotApprovalInput<'a> {
    target: &'a Path,
    provider: &'a str,
    template: &'a str,
    run_id: &'a str,
    provider_prompt_file: &'a Path,
    max_repair_attempts: usize,
    max_budget_usd: Option<f64>,
    minimum_score: u64,
    command: &'a [String],
    shell_command: &'a str,
}

fn provider_pilot_approval_packet(
    input: ProviderPilotApprovalInput<'_>,
) -> Result<serde_json::Value> {
    let explicit_live_env = provider_pilot_guard_env(input.provider).unwrap_or("");
    let digest_input = serde_json::json!({
        "schema_version": "ao2.provider-pilot-approval-digest.v1",
        "provider": input.provider,
        "template": input.template,
        "run_id": input.run_id,
        "target": input.target.display().to_string(),
        "provider_prompt_file": input.provider_prompt_file.display().to_string(),
        "max_repair_attempts": input.max_repair_attempts,
        "max_budget_usd": input.max_budget_usd,
        "minimum_score": input.minimum_score,
        "explicit_live_env": explicit_live_env,
        "command": input.command,
        "shell_command": input.shell_command
    });
    let action_digest = sha256_hex(serde_json::to_string(&digest_input)?.as_bytes());
    Ok(serde_json::json!({
        "schema_version": "ao2.provider-pilot-approval.v1",
        "status": "approval_required",
        "approval_mode": "exact_action_digest",
        "required_form_field": "approval_action_digest",
        "action_digest": action_digest,
        "provider": input.provider,
        "run_id": input.run_id,
        "template": input.template,
        "target": input.target.display().to_string(),
        "provider_prompt_file": input.provider_prompt_file.display().to_string(),
        "max_repair_attempts": input.max_repair_attempts,
        "max_budget_usd": input.max_budget_usd,
        "minimum_score": input.minimum_score,
        "explicit_live_env": explicit_live_env,
        "next_action": "submit approval_action_digest with the exact action_digest to start this provider pilot"
    }))
}

fn provider_gate_required_providers(require: &[String]) -> Result<Vec<String>> {
    let providers = if require.is_empty() {
        vec!["scripted".to_string()]
    } else {
        require
            .iter()
            .map(|provider| provider.trim().to_string())
            .filter(|provider| !provider.is_empty())
            .collect::<Vec<_>>()
    };
    let mut seen = BTreeSet::new();
    let mut required = Vec::new();
    for provider in providers {
        parse_provider(&provider)?;
        if seen.insert(provider.clone()) {
            required.push(provider);
        }
    }
    Ok(required)
}

fn provider_gate_reason(code: &str, provider: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "code": code,
        "provider": provider,
        "message": message
    })
}

fn provider_smoke_live_provider_set(live_providers: &[String]) -> Result<BTreeSet<String>> {
    let mut set = BTreeSet::new();
    for provider in live_providers {
        let name = provider.trim();
        if name.is_empty() {
            continue;
        }
        parse_provider(name)?;
        if name == "scripted" {
            anyhow::bail!("scripted provider always runs; --live-provider is for codex, claude, or antigravity");
        }
        set.insert(name.to_string());
    }
    Ok(set)
}

pub(crate) fn provider_smoke_guard_env(provider: &str) -> Option<&'static str> {
    provider_guard_env(provider, ProviderGuardKind::Smoke)
}

fn provider_pilot_guard_env(provider: &str) -> Option<&'static str> {
    provider_guard_env(provider, ProviderGuardKind::Pilot)
}

enum ProviderGuardKind {
    Smoke,
    Pilot,
}

fn provider_guard_env(provider: &str, kind: ProviderGuardKind) -> Option<&'static str> {
    let provider = parse_provider(provider).ok()?;
    let metadata = provider_metadata(provider);
    let guard = match kind {
        ProviderGuardKind::Smoke => metadata.smoke_guard_env?,
        ProviderGuardKind::Pilot => metadata.pilot_guard_env?,
    };
    match guard.as_str() {
        "AO2_LIVE_CODEX_SMOKE" => Some("AO2_LIVE_CODEX_SMOKE"),
        "AO2_LIVE_CLAUDE_SMOKE" => Some("AO2_LIVE_CLAUDE_SMOKE"),
        "AO2_LIVE_ANTIGRAVITY_SMOKE" => Some("AO2_LIVE_ANTIGRAVITY_SMOKE"),
        "AO2_LIVE_CODEX_PILOT" => Some("AO2_LIVE_CODEX_PILOT"),
        "AO2_LIVE_CLAUDE_PILOT" => Some("AO2_LIVE_CLAUDE_PILOT"),
        "AO2_LIVE_ANTIGRAVITY_PILOT" => Some("AO2_LIVE_ANTIGRAVITY_PILOT"),
        _ => None,
    }
}

fn provider_smoke_run_json(
    target: &Path,
    name: &str,
    provider: ao2_adapters::ProviderKind,
    doctor: ao2_adapters::ProviderDoctorReport,
    minimum_score: u64,
) -> Result<serde_json::Value> {
    let run_id = format!("provider-smoke-{name}-{}", generate_api_token());
    let smoke_root = target.join(".ao2").join("provider-smoke").join(&run_id);
    let smoke_repo = smoke_root.join("repo");
    write_provider_smoke_fixture(&smoke_repo)?;
    let workflow = materialize_template_workflow(&smoke_repo, "bug-fix")?;
    let mut summary = run_risky_pr_with_provider_prompt(ProviderRunOptions {
        target_repo: smoke_repo.clone(),
        workflow_path: workflow,
        run_id: Some(run_id),
        provider,
        prompt: provider_smoke_script().to_string(),
        max_repair_attempts: 1,
        max_budget_usd: None,
        repair_source: None,
    })?;
    if summary.status == ao2_runtime::RunStatus::WaitingForApproval {
        if let Some(resumed) = approve_and_resume_persisted_sandbox_patches(
            &smoke_repo,
            &summary.run_id,
            "human:provider-smoke-operator",
        )? {
            summary = resumed;
        }
    }
    let scorecard = provider_score_json(&smoke_repo, &summary.run_id)?;
    let score = json_u64(&scorecard, "score");
    let verdict = if score >= minimum_score {
        "ready"
    } else {
        "fail"
    };
    Ok(serde_json::json!({
        "provider": name,
        "available": true,
        "doctor": doctor,
        "run_id": summary.run_id,
        "status": format!("{:?}", summary.status),
        "score": score,
        "minimum_score": minimum_score,
        "verdict": verdict,
        "scorecard": scorecard
    }))
}

fn provider_smoke_history_path(target: &Path) -> PathBuf {
    target
        .join(".ao2")
        .join("provider-smoke")
        .join("history.json")
}

pub(crate) fn read_provider_smoke_history(target: &Path) -> Result<serde_json::Value> {
    let path = provider_smoke_history_path(target);
    match fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(provider_smoke_empty_history(target))
        }
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn provider_smoke_empty_history(target: &Path) -> serde_json::Value {
    serde_json::json!({
        "schema": "ao2.provider-smoke-history.v1",
        "target": target,
        "history_path": provider_smoke_history_path(target),
        "entry_count": 0_u64,
        "latest": serde_json::Value::Null,
        "entries": []
    })
}

fn record_provider_smoke_history(
    target: &Path,
    report: &serde_json::Value,
) -> Result<serde_json::Value> {
    let path = provider_smoke_history_path(target);
    let mut history = read_provider_smoke_history(target)?;
    let mut entries = json_array(&history, "entries").to_vec();
    entries.push(serde_json::json!({
        "generated_at_ms": now_unix_ms(),
        "report": report
    }));
    history = serde_json::json!({
        "schema": "ao2.provider-smoke-history.v1",
        "target": target,
        "history_path": path,
        "entry_count": entries.len(),
        "latest": report,
        "entries": entries
    });
    atomic_write_text(&path, &serde_json::to_string_pretty(&history)?)?;
    Ok(history)
}

fn write_provider_smoke_fixture(repo: &Path) -> Result<()> {
    fs::create_dir_all(repo.join("discount_service"))
        .with_context(|| format!("create {}", repo.join("discount_service").display()))?;
    fs::create_dir_all(repo.join("tests"))
        .with_context(|| format!("create {}", repo.join("tests").display()))?;
    fs::write(
        repo.join("discount_service").join("__init__.py"),
        "from .discounts import calculate_discount\n\n__all__ = [\"calculate_discount\"]\n",
    )?;
    fs::write(
        repo.join("discount_service").join("discounts.py"),
        "def calculate_discount(price: float, discount_rate: float) -> float:\n    return price * (1 - discount_rate)\n",
    )?;
    fs::write(
        repo.join("tests").join("test_discounts.py"),
        "from discount_service.discounts import calculate_discount\n\n\ndef test_calculates_discount_for_valid_values():\n    assert calculate_discount(100, 0.25) == 75\n",
    )?;
    fs::write(
        repo.join("pytest.py"),
        r#"import importlib.util
import pathlib
import sys
import traceback


def main():
    root = pathlib.Path.cwd()
    failures = []
    for test_file in sorted((root / "tests").glob("test_*.py")):
        spec = importlib.util.spec_from_file_location(test_file.stem, test_file)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        for name in sorted(dir(module)):
            if name.startswith("test_"):
                try:
                    getattr(module, name)()
                    print(f"PASS {test_file.name}::{name}")
                except Exception:
                    failures.append(f"{test_file.name}::{name}")
                    traceback.print_exc()
    if failures:
        print(f"FAILED {len(failures)} tests: {failures}")
        return 1
    print("all tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
"#,
    )?;
    Ok(())
}

fn provider_smoke_script() -> &'static str {
    r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: provider smoke added validation around discount math\n'
printf 'Changed files: discount_service/discounts.py\n'
"#
}

fn provider_score(target: PathBuf, run_id: String, json_output: bool) -> Result<()> {
    let scorecard = provider_score_json(&target, &run_id)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&scorecard)?);
    } else {
        println!("run_id={}", json_string(&scorecard, "run_id"));
        println!("score={}", json_u64(&scorecard, "score"));
        println!("verdict={}", json_string(&scorecard, "verdict"));
        println!(
            "provider_summary_count={}",
            json_u64(&scorecard, "provider_summary_count")
        );
        for dimension in json_array(&scorecard, "dimensions") {
            println!(
                "{}\tstatus={}\tpoints={}/{}",
                json_string(dimension, "name"),
                json_string(dimension, "status"),
                json_u64(dimension, "points"),
                json_u64(dimension, "max_points")
            );
        }
    }
    Ok(())
}

fn provider_cost_ledger(acceptance_root: PathBuf, json_output: bool) -> Result<()> {
    let ledger = provider_cost_ledger_json(&acceptance_root)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&ledger)?);
    } else {
        println!("schema_version={}", json_string(&ledger, "schema_version"));
        println!("status={}", json_string(&ledger, "status"));
        println!("entry_count={}", json_u64(&ledger, "entry_count"));
        println!(
            "max_budget_usd={:.2}",
            json_f64(&ledger["totals"], "max_budget_usd")
        );
        println!(
            "observed_cost_usd={:.2}",
            json_f64(&ledger["totals"], "observed_cost_usd")
        );
        println!(
            "total_tokens={}",
            json_u64(&ledger["totals"], "total_tokens")
        );
    }
    Ok(())
}

fn provider_cost_trend(acceptance_root: PathBuf, json_output: bool) -> Result<()> {
    let trend = provider_cost_trend_json(&acceptance_root)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&trend)?);
    } else {
        println!("schema_version={}", json_string(&trend, "schema_version"));
        println!("status={}", json_string(&trend, "status"));
        println!("release_count={}", json_u64(&trend, "release_count"));
        println!(
            "latest_release_tag={}",
            json_string(&trend, "latest_release_tag")
        );
        println!(
            "observed_cost_delta_usd={:.2}",
            json_f64(&trend["delta"], "observed_cost_usd")
        );
        println!(
            "total_token_delta={}",
            trend["delta"]
                .get("total_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
        );
    }
    Ok(())
}

#[derive(Clone, Default)]
struct ProviderCostAccumulator {
    entry_count: u64,
    max_budget_usd: f64,
    observed_cost_usd: f64,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    provider_enforced_budget_count: u64,
}

#[derive(Default)]
struct ProviderCostTrendRelease {
    totals: ProviderCostAccumulator,
    providers: BTreeMap<String, ProviderCostAccumulator>,
}

#[derive(Default)]
struct ProviderCostTrendAccumulator {
    release_count: u64,
    totals: ProviderCostAccumulator,
}

pub(crate) fn provider_cost_ledger_json(acceptance_root: &Path) -> Result<serde_json::Value> {
    if !acceptance_root.is_dir() {
        anyhow::bail!(
            "provider pilot acceptance root does not exist: {}",
            acceptance_root.display()
        );
    }

    let mut bundles = Vec::new();
    collect_provider_pilot_acceptance_bundles(acceptance_root, &mut bundles)?;
    bundles.sort();

    let mut entries = Vec::new();
    let mut failed_candidates = Vec::new();
    let mut providers = BTreeMap::<String, ProviderCostAccumulator>::new();
    let mut totals = ProviderCostAccumulator::default();
    for bundle in bundles {
        match provider_pilot_acceptance_verification_json(&bundle) {
            Ok(acceptance) => {
                let provider = json_string(&acceptance, "provider");
                let evidence_pack_path = PathBuf::from(json_string(&acceptance, "evidence_pack"));
                let evidence_pack = read_optional_json(&evidence_pack_path)?;
                let usage = provider_usage_totals(&evidence_pack, &provider);
                let max_budget_usd = json_f64(&acceptance["budget"], "max_budget_usd");
                let observed_cost_usd = usage.3;
                let provider_enforced_budget = acceptance["budget"]
                    .get("provider_enforced")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let release_tag = provider_cost_ledger_release_tag(acceptance_root, &bundle);

                let entry = serde_json::json!({
                    "acceptance_bundle": bundle,
                    "release_tag": release_tag,
                    "provider": provider,
                    "run_id": json_string(&acceptance, "run_id"),
                    "status": json_string(&acceptance, "status"),
                    "score": json_u64(&acceptance["score"], "score"),
                    "verdict": json_string(&acceptance["score"], "verdict"),
                    "replay_status": json_string(&acceptance["replay"], "status"),
                    "max_budget_usd": round_usd(max_budget_usd),
                    "provider_enforced_budget": provider_enforced_budget,
                    "observed_cost_usd": round_usd(observed_cost_usd),
                    "input_tokens": usage.0,
                    "output_tokens": usage.1,
                    "total_tokens": usage.2,
                    "evidence_pack": evidence_pack_path
                });
                entries.push(entry);

                accumulate_provider_costs(
                    &mut totals,
                    max_budget_usd,
                    observed_cost_usd,
                    usage,
                    provider_enforced_budget,
                );
                let provider_totals = providers.entry(provider).or_default();
                accumulate_provider_costs(
                    provider_totals,
                    max_budget_usd,
                    observed_cost_usd,
                    usage,
                    provider_enforced_budget,
                );
            }
            Err(error) => failed_candidates.push(serde_json::json!({
                "acceptance_bundle": bundle,
                "status": "error",
                "error": error.to_string()
            })),
        }
    }

    entries.sort_by(|left, right| {
        release_tag_sort_key(&json_string(right, "release_tag"))
            .cmp(&release_tag_sort_key(&json_string(left, "release_tag")))
            .then_with(|| json_string(left, "provider").cmp(&json_string(right, "provider")))
            .then_with(|| json_string(left, "run_id").cmp(&json_string(right, "run_id")))
    });
    let providers_json = providers
        .into_iter()
        .map(|(provider, totals)| (provider, provider_cost_accumulator_json(&totals)))
        .collect::<serde_json::Map<_, _>>();
    let status = if entries.is_empty() { "empty" } else { "ready" };
    Ok(serde_json::json!({
        "schema_version": "ao2.provider-cost-ledger.v1",
        "status": status,
        "acceptance_root": acceptance_root,
        "entry_count": entries.len(),
        "failed_candidate_count": failed_candidates.len(),
        "failed_candidates": failed_candidates,
        "totals": provider_cost_accumulator_json(&totals),
        "providers": providers_json,
        "entries": entries
    }))
}

pub(crate) fn provider_cost_trend_json(acceptance_root: &Path) -> Result<serde_json::Value> {
    let ledger = provider_cost_ledger_json(acceptance_root)?;
    let mut releases = BTreeMap::<String, ProviderCostTrendRelease>::new();
    let mut provider_totals = BTreeMap::<String, ProviderCostTrendAccumulator>::new();
    let mut provider_releases = BTreeMap::<String, BTreeSet<String>>::new();

    for entry in json_array(&ledger, "entries") {
        let release_tag = {
            let tag = json_string(entry, "release_tag");
            if tag.is_empty() {
                "unversioned".to_string()
            } else {
                tag
            }
        };
        let provider = json_string(entry, "provider");
        let max_budget_usd = json_f64(entry, "max_budget_usd");
        let observed_cost_usd = json_f64(entry, "observed_cost_usd");
        let usage = (
            json_u64(entry, "input_tokens"),
            json_u64(entry, "output_tokens"),
            json_u64(entry, "total_tokens"),
            observed_cost_usd,
        );
        let provider_enforced_budget = entry
            .get("provider_enforced_budget")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let release = releases.entry(release_tag.clone()).or_default();
        accumulate_provider_costs(
            &mut release.totals,
            max_budget_usd,
            observed_cost_usd,
            usage,
            provider_enforced_budget,
        );
        let release_provider = release.providers.entry(provider.clone()).or_default();
        accumulate_provider_costs(
            release_provider,
            max_budget_usd,
            observed_cost_usd,
            usage,
            provider_enforced_budget,
        );

        let global_provider = provider_totals.entry(provider.clone()).or_default();
        accumulate_provider_costs(
            &mut global_provider.totals,
            max_budget_usd,
            observed_cost_usd,
            usage,
            provider_enforced_budget,
        );
        provider_releases
            .entry(provider)
            .or_default()
            .insert(release_tag);
    }

    for (provider, release_tags) in provider_releases {
        if let Some(accumulator) = provider_totals.get_mut(&provider) {
            accumulator.release_count = release_tags.len() as u64;
        }
    }

    let mut release_tags = releases.keys().cloned().collect::<Vec<_>>();
    release_tags.sort_by_key(|left| release_tag_sort_key(left));

    let releases_json = release_tags
        .iter()
        .filter_map(|release_tag| {
            let release = releases.get(release_tag)?;
            let providers_json = release
                .providers
                .iter()
                .map(|(provider, totals)| {
                    (provider.clone(), provider_cost_accumulator_json(totals))
                })
                .collect::<serde_json::Map<_, _>>();
            Some(serde_json::json!({
                "release_tag": release_tag,
                "entry_count": release.totals.entry_count,
                "max_budget_usd": round_usd(release.totals.max_budget_usd),
                "observed_cost_usd": round_usd(release.totals.observed_cost_usd),
                "input_tokens": release.totals.input_tokens,
                "output_tokens": release.totals.output_tokens,
                "total_tokens": release.totals.total_tokens,
                "provider_enforced_budget_count": release.totals.provider_enforced_budget_count,
                "providers": providers_json
            }))
        })
        .collect::<Vec<_>>();

    let provider_json = provider_totals
        .into_iter()
        .map(|(provider, accumulator)| {
            (provider, provider_cost_trend_accumulator_json(&accumulator))
        })
        .collect::<serde_json::Map<_, _>>();

    let latest_release_tag = release_tags.last().cloned().unwrap_or_default();
    let previous_release_tag = release_tags
        .len()
        .checked_sub(2)
        .and_then(|index| release_tags.get(index))
        .cloned()
        .unwrap_or_default();
    let latest = if latest_release_tag.is_empty() {
        None
    } else {
        releases
            .get(&latest_release_tag)
            .map(|release| &release.totals)
    };
    let previous = if previous_release_tag.is_empty() {
        None
    } else {
        releases
            .get(&previous_release_tag)
            .map(|release| &release.totals)
    };
    let status = if releases_json.is_empty() {
        "empty"
    } else {
        "ready"
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.provider-cost-trend.v1",
        "status": status,
        "acceptance_root": acceptance_root,
        "release_count": releases_json.len(),
        "latest_release_tag": latest_release_tag,
        "previous_release_tag": previous_release_tag,
        "delta": provider_cost_delta_json(latest, previous),
        "providers": provider_json,
        "releases": releases_json
    }))
}

pub(crate) fn read_optional_json(path: &Path) -> Result<serde_json::Value> {
    if path.as_os_str().is_empty() || !path.is_file() {
        return Ok(serde_json::Value::Null);
    }
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

fn provider_usage_totals(
    evidence_pack: &serde_json::Value,
    provider: &str,
) -> (u64, u64, u64, f64) {
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut total_tokens = 0_u64;
    let mut cost_usd = 0.0_f64;
    for summary in json_array(evidence_pack, "provider_summaries") {
        let summary_provider = json_string(summary, "provider");
        if !summary_provider.is_empty() && summary_provider != provider {
            continue;
        }
        input_tokens += json_u64(&summary["usage"], "input_tokens");
        output_tokens += json_u64(&summary["usage"], "output_tokens");
        total_tokens += json_u64(&summary["usage"], "total_tokens");
        cost_usd += json_f64(summary, "cost_usd");
    }
    (input_tokens, output_tokens, total_tokens, cost_usd)
}

fn accumulate_provider_costs(
    accumulator: &mut ProviderCostAccumulator,
    max_budget_usd: f64,
    observed_cost_usd: f64,
    usage: (u64, u64, u64, f64),
    provider_enforced_budget: bool,
) {
    accumulator.entry_count += 1;
    accumulator.max_budget_usd += max_budget_usd;
    accumulator.observed_cost_usd += observed_cost_usd;
    accumulator.input_tokens += usage.0;
    accumulator.output_tokens += usage.1;
    accumulator.total_tokens += usage.2;
    if provider_enforced_budget {
        accumulator.provider_enforced_budget_count += 1;
    }
}

fn provider_cost_accumulator_json(accumulator: &ProviderCostAccumulator) -> serde_json::Value {
    serde_json::json!({
        "entry_count": accumulator.entry_count,
        "max_budget_usd": round_usd(accumulator.max_budget_usd),
        "observed_cost_usd": round_usd(accumulator.observed_cost_usd),
        "input_tokens": accumulator.input_tokens,
        "output_tokens": accumulator.output_tokens,
        "total_tokens": accumulator.total_tokens,
        "provider_enforced_budget_count": accumulator.provider_enforced_budget_count,
        "provider_enforced_budget": accumulator.entry_count > 0
            && accumulator.provider_enforced_budget_count == accumulator.entry_count
    })
}

fn provider_cost_trend_accumulator_json(
    accumulator: &ProviderCostTrendAccumulator,
) -> serde_json::Value {
    let mut json = provider_cost_accumulator_json(&accumulator.totals);
    if let Some(object) = json.as_object_mut() {
        object.insert(
            "release_count".to_string(),
            serde_json::json!(accumulator.release_count),
        );
    }
    json
}

fn provider_cost_delta_json(
    current: Option<&ProviderCostAccumulator>,
    previous: Option<&ProviderCostAccumulator>,
) -> serde_json::Value {
    let current = current.cloned().unwrap_or_default();
    let previous = previous.cloned().unwrap_or_default();
    serde_json::json!({
        "entry_count": current.entry_count as i64 - previous.entry_count as i64,
        "max_budget_usd": round_usd(current.max_budget_usd - previous.max_budget_usd),
        "observed_cost_usd": round_usd(current.observed_cost_usd - previous.observed_cost_usd),
        "input_tokens": current.input_tokens as i64 - previous.input_tokens as i64,
        "output_tokens": current.output_tokens as i64 - previous.output_tokens as i64,
        "total_tokens": current.total_tokens as i64 - previous.total_tokens as i64,
        "provider_enforced_budget_count": current.provider_enforced_budget_count as i64
            - previous.provider_enforced_budget_count as i64
    })
}

fn round_usd(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(crate) fn provider_score_json(target: &Path, run_id: &str) -> Result<serde_json::Value> {
    let run_dir = run_dir(target, run_id);
    let evidence_pack_path = run_dir.join("evidence-pack").join("evidence-pack.json");
    let evidence_pack = fs::read_to_string(&evidence_pack_path)
        .with_context(|| format!("read {}", evidence_pack_path.display()))?;
    let evidence_pack: serde_json::Value = serde_json::from_str(&evidence_pack)
        .with_context(|| format!("parse {}", evidence_pack_path.display()))?;
    let replay = replay_run(ReplayOptions {
        target_repo: target.to_path_buf(),
        run_id: run_id.to_string(),
    })?;

    let summaries = json_array(&evidence_pack, "provider_summaries");
    let markers = json_array(&evidence_pack, "markers")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<BTreeSet<_>>();
    let summary_count = summaries.len() as u64;
    let changed_files_count = summaries
        .iter()
        .flat_map(|summary| json_array(summary, "changed_files"))
        .filter(|value| value.as_str().is_some_and(|text| !text.trim().is_empty()))
        .count() as u64;
    let applied_files = provider_score_applied_files(&evidence_pack);
    let applied_files_count = applied_files.len() as u64;
    let blocker_count = summaries
        .iter()
        .flat_map(|summary| json_array(summary, "blockers"))
        .count() as u64;

    let replay_status_value = serde_json::to_value(replay.status)?;
    let replay_status = replay_status_value.as_str().unwrap_or("unknown");
    let replay_accepted = replay_status == "accepted";
    let replay_points = if replay_accepted && replay.digest_failures.is_empty() {
        25
    } else {
        0
    };
    let provider_summary_points = if summaries
        .iter()
        .any(|summary| !json_string(summary, "raw_summary").trim().is_empty())
    {
        25
    } else if summary_count > 0 {
        15
    } else {
        0
    };
    let changed_files_points = if applied_files_count > 0 { 20 } else { 0 };
    let blocker_points = if blocker_count == 0 { 15 } else { 0 };
    let artifact_types = json_array(&evidence_pack, "artifacts")
        .iter()
        .map(|artifact| json_string(artifact, "artifact_type"))
        .collect::<BTreeSet<_>>();
    let sandbox_patch_applied = artifact_types.contains("sandbox_patch_apply");
    let boundary_points = if sandbox_patch_applied && markers.contains("policy_denied_git_push") {
        15
    } else if sandbox_patch_applied {
        10
    } else {
        0
    };

    let dimensions = vec![
        score_dimension(
            "replay_integrity",
            replay_points,
            25,
            if replay_accepted && replay.digest_failures.is_empty() {
                "replay has zero digest failures"
            } else if !replay_accepted {
                "replay status is not accepted"
            } else {
                "replay reported digest failures"
            },
        ),
        score_dimension(
            "provider_summary",
            provider_summary_points,
            25,
            if provider_summary_points == 25 {
                "provider transcript summary includes a parsed summary"
            } else if summary_count > 0 {
                "provider transcript summary exists but lacks raw summary text"
            } else {
                "no provider transcript summaries found"
            },
        ),
        score_dimension(
            "changed_files",
            changed_files_points,
            20,
            if applied_files_count > 0 {
                "AO2 sandbox apply evidence includes changed files"
            } else if changed_files_count > 0 {
                "provider transcript claimed changed files but AO2 sandbox apply evidence is empty"
            } else {
                "no changed files parsed from provider transcript"
            },
        ),
        score_dimension(
            "blocker_hygiene",
            blocker_points,
            15,
            if blocker_count == 0 {
                "provider transcript summaries contain no blockers"
            } else {
                "provider transcript summaries contain blockers"
            },
        ),
        score_dimension(
            "policy_boundary",
            boundary_points,
            15,
            if boundary_points == 15 {
                "sandbox patch and policy-denied action markers are present"
            } else if boundary_points == 10 {
                "sandbox patch marker is present"
            } else {
                "sandbox/policy boundary markers are incomplete"
            },
        ),
    ];
    let score = dimensions
        .iter()
        .map(|dimension| json_u64(dimension, "points"))
        .sum::<u64>();
    let verdict = if !replay_accepted || applied_files_count == 0 {
        "fail"
    } else if score >= 90 {
        "ready"
    } else if score >= 70 {
        "warn"
    } else {
        "fail"
    };

    Ok(serde_json::json!({
        "schema": "ao2.provider-evidence-scorecard.v1",
        "run_id": run_id,
        "target": target,
        "score": score,
        "max_score": 100,
        "verdict": verdict,
        "provider_summary_count": summary_count,
        "changed_files_count": changed_files_count,
        "applied_files_count": applied_files_count,
        "applied_files": applied_files.iter().cloned().collect::<Vec<_>>(),
        "blocker_count": blocker_count,
        "replay": {
            "status": replay_status,
            "event_count": replay.event_count,
            "artifact_count": replay.artifact_count,
            "digest_failures": replay.digest_failures.len()
        },
        "dimensions": dimensions,
        "evidence_pack": evidence_pack_path
    }))
}

fn provider_score_applied_files(evidence_pack: &serde_json::Value) -> BTreeSet<String> {
    let mut applied_files = BTreeSet::new();
    for artifact in json_array(evidence_pack, "artifacts") {
        if json_string(artifact, "artifact_type") != "sandbox_patch_apply" {
            continue;
        }
        let uri = json_string(artifact, "uri");
        if uri.trim().is_empty() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&uri) else {
            continue;
        };
        let Ok(apply) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        for applied_file in json_array(&apply, "applied_files") {
            let applied_file = applied_file.as_str().unwrap_or("").trim();
            if !applied_file.is_empty() {
                applied_files.insert(applied_file.to_string());
            }
        }
    }
    applied_files
}

fn score_dimension(name: &str, points: u64, max_points: u64, evidence: &str) -> serde_json::Value {
    let status = if points == max_points {
        "pass"
    } else if points > 0 {
        "warn"
    } else {
        "fail"
    };
    serde_json::json!({
        "name": name,
        "status": status,
        "points": points,
        "max_points": max_points,
        "evidence": evidence
    })
}

fn provider_matrix(json_output: bool) -> Result<()> {
    let matrix = provider_matrix_json()?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&matrix)?);
    } else {
        println!("AO2 provider readiness matrix");
        println!("schema={}", matrix["schema"].as_str().unwrap_or_default());
        println!("default_timeout_seconds={DEFAULT_PROVIDER_TIMEOUT_SECONDS}");
        for provider in matrix["providers"].as_array().into_iter().flatten() {
            println!(
                "{}\tavailable={}\tboundary={}\ttimeout_seconds={}",
                provider["provider"].as_str().unwrap_or_default(),
                provider["doctor"]["available"].as_bool().unwrap_or(false),
                provider["execution_boundary"].as_str().unwrap_or_default(),
                provider["timeout_seconds"].as_u64().unwrap_or_default(),
            );
        }
    }
    Ok(())
}

pub(crate) fn provider_matrix_json() -> Result<serde_json::Value> {
    let providers = ["scripted", "codex", "claude", "antigravity"]
        .iter()
        .map(|name| {
            let provider = parse_provider(name)?;
            let metadata = provider_metadata(provider);
            let doctor = doctor_provider(provider)?;
            Ok(serde_json::json!({
                "provider": name,
                "metadata_source": metadata.metadata_source.clone(),
                "crate": metadata.metadata_source.clone(),
                "adapter_kind": metadata.adapter_kind.clone(),
                "doctor": doctor,
                "execution_boundary": "sandbox_copy_then_digest_patch",
                "timeout_seconds": DEFAULT_PROVIDER_TIMEOUT_SECONDS,
                "side_effect_boundary": "target mutation only through exact digest patch apply",
                "transcript_fields": metadata.transcript_fields.clone(),
                "policy_invariants": [
                    "provider cannot write target repo directly",
                    "provider transcript is persisted as evidence",
                    "sandbox diff preview emits exact action digest",
                    "patch apply requires matching digest approval",
                    "replay and closure remain runtime-owned"
                ]
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    let matrix = serde_json::json!({
        "schema": "ao2.provider-readiness-matrix.v1",
        "default_timeout_seconds": DEFAULT_PROVIDER_TIMEOUT_SECONDS,
        "providers": providers
    });

    Ok(matrix)
}

fn provider_registry(
    control_plane_url: Option<String>,
    api_token: Option<String>,
    api_token_env: Option<String>,
    signing_key: Option<PathBuf>,
    signer_id: String,
    json_output: bool,
) -> Result<()> {
    let registry = provider_registry_json()?;

    let has_api_token = api_token.is_some() || api_token_env.is_some();
    if signing_key.is_some() && (control_plane_url.is_none() || !has_api_token) {
        return Err(anyhow!(
            "signed provider registry publish requires --control-plane-url and --api-token or --api-token-env"
        ));
    }

    if control_plane_url.is_some() || has_api_token {
        let control_plane_url = control_plane_url
            .as_deref()
            .ok_or_else(|| anyhow!("provider registry publish requires --control-plane-url"))?;
        let api_token = resolve_api_token(api_token.as_deref(), api_token_env.as_deref())?;
        let result = provider_registry_publish_to_control_plane_json(
            registry,
            control_plane_url,
            &api_token,
            signing_key.as_deref(),
            &signer_id,
        )?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("endpoint={}", json_string(&result, "endpoint"));
            println!("sha256={}", json_string(&result["receipt"], "sha256"));
            println!("dashboard_url={}", json_string(&result, "dashboard_url"));
        }
        return Ok(());
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&registry)?);
    } else {
        println!("AO2 provider/plugin registry");
        println!("schema={}", registry["schema"].as_str().unwrap_or_default());
        println!("phase={}", registry["phase"].as_str().unwrap_or_default());
        for provider in registry["providers"].as_array().into_iter().flatten() {
            println!(
                "{}\tphase={}\tguard={}\tcontract={}",
                provider["provider"].as_str().unwrap_or_default(),
                provider["phase"].as_str().unwrap_or_default(),
                provider["guards"]["explicit_live_env"]
                    .as_str()
                    .unwrap_or("not_required"),
                provider["contract"]["same_contract_as"]
                    .as_str()
                    .unwrap_or("self"),
            );
        }
    }
    Ok(())
}

fn provider_registry_publish_to_control_plane_json(
    registry: serde_json::Value,
    control_plane_url: &str,
    api_token: &str,
    signing_key: Option<&Path>,
    signer_id: &str,
) -> Result<serde_json::Value> {
    let api_token = trimmed_required("--api-token", api_token)?;
    let schema = json_string(&registry, "schema");
    if schema != "ao2.provider-plugin-registry.v1" {
        return Err(anyhow!(
            "provider registry publish requires ao2.provider-plugin-registry.v1, got {schema}"
        ));
    }
    let (endpoint, post_body, signature, signed) = if let Some(signing_key) = signing_key {
        let signer_id = trimmed_required("--signer-id", signer_id)?;
        let registry_raw = serde_json::to_string_pretty(&registry)?;
        let signature_bytes = sign_bytes_with_private_key(signing_key, registry_raw.as_bytes())?;
        let public_key_pem = public_key_pem_from_private_key(signing_key)?;
        let signature = serde_json::json!({
            "schema_version": "ao2.cp-provider-registry-signature.v1",
            "signature_algorithm": "RSA/SHA-256",
            "signer_id": signer_id,
            "signature_sha256": sha256_bytes_hex(&signature_bytes),
            "signature_hex": hex_lower(&signature_bytes),
            "public_key_sha256": sha256_bytes_hex(public_key_pem.as_bytes()),
            "public_key_pem": public_key_pem
        });
        (
            control_plane_endpoint(control_plane_url, "/api/v1/provider/registry/signed")?,
            serde_json::to_string(&serde_json::json!({
                "schema_version": "ao2.cp-provider-registry-signed-upload.v1",
                "registry": registry,
                "registry_b64": base64_standard(registry_raw.as_bytes()),
                "signature": signature
            }))?,
            signature,
            true,
        )
    } else {
        (
            control_plane_endpoint(control_plane_url, "/api/v1/provider/registry")?,
            serde_json::to_string(&registry)?,
            serde_json::Value::Null,
            false,
        )
    };
    let receipt = post_json_http(&endpoint, &api_token, &post_body)?;
    let receipt_sha = json_string(&receipt, "sha256");
    let detail_url = if receipt_sha.is_empty() {
        String::new()
    } else {
        control_plane_endpoint(
            control_plane_url,
            &format!("/api/v1/provider/registry/{receipt_sha}/detail"),
        )?
    };
    let dashboard_url =
        control_plane_endpoint(control_plane_url, "/api/v1/provider/registry/dashboard")?;
    let latest_url = control_plane_endpoint(control_plane_url, "/api/v1/provider/registry/latest")?;
    Ok(serde_json::json!({
        "schema_version": "ao2.provider-registry-control-plane-publish.v1",
        "endpoint": endpoint,
        "dashboard_url": dashboard_url,
        "latest_url": latest_url,
        "detail_url": detail_url,
        "signed": signed,
        "signature": signature,
        "registry": registry,
        "receipt": receipt
    }))
}

pub(crate) fn provider_registry_json() -> Result<serde_json::Value> {
    let matrix = provider_matrix_json()?;
    let providers = json_array(&matrix, "providers")
        .iter()
        .map(provider_registry_entry)
        .collect::<Result<Vec<_>>>()?;

    Ok(serde_json::json!({
        "schema": "ao2.provider-plugin-registry.v1",
        "phase": "phase_2_registry_groundwork",
        "trust_boundary": {
            "execution_owner": "ao2-local-cli",
            "front_end_role": "hermes_or_workbench_may_request_and_observe",
            "control_plane_role": "read_only_observer_only",
            "target_mutation": "exact_digest_patch_apply_only",
            "provider_api_key_auth": "forbidden"
        },
        "providers": providers,
        "extension_slots": [
            "adapter_crate",
            "smoke_script",
            "pilot_acceptance_script",
            "factory_hermes_bridge",
            "control_plane_observer"
        ],
        "lifecycle_gates": [
            "adapter doctor",
            "provider matrix --json",
            "provider contract --verify --require codex",
            "provider smoke-all",
            "provider pilot",
            "provider score",
            "release gate"
        ],
        "phase2_deferred_features": [
            "additional provider adapters as separate crates",
            "MCP gateway",
            "team mode",
            "RBAC",
            "Postgres persistence",
            "full trace-to-eval",
            "legacy AO import"
        ],
        "policy_invariants": [
            "providers run in disposable sandbox copies",
            "Hermes and Workbench never bypass AO2 gates",
            "control-plane observes signed evidence and does not sit in the trust path",
            "live providers require explicit operator flags",
            "replay, obligation gates, and closure verdict stay provider-independent"
        ]
    }))
}

fn provider_registry_entry(provider: &serde_json::Value) -> Result<serde_json::Value> {
    let name = json_string(provider, "provider");
    let metadata = provider_metadata(parse_provider(&name)?);
    let live_guard = metadata.pilot_guard_env.clone();
    let same_contract_as = if name == "scripted" {
        "self"
    } else {
        "scripted"
    };

    Ok(serde_json::json!({
        "provider": name,
        "phase": metadata.registry_phase.clone(),
        "adapter_kind": metadata.adapter_kind.clone(),
        "crate": metadata.metadata_source.clone(),
        "metadata_source": metadata.metadata_source.clone(),
        "doctor": provider.get("doctor").cloned().unwrap_or(serde_json::Value::Null),
        "contract": {
            "same_contract_as": same_contract_as,
            "execution_boundary": provider["execution_boundary"],
            "side_effect_boundary": provider["side_effect_boundary"],
            "transcript_fields": provider["transcript_fields"],
            "policy_invariants": provider["policy_invariants"]
        },
        "guards": {
            "explicit_live_env": live_guard,
            "provider_api_key_envs_forbidden": [
                "OPENAI_API_KEY",
                "ANTHROPIC_API_KEY"
            ],
            "default_timeout_seconds": provider["timeout_seconds"]
        },
        "extension_slots": [
            "adapter_crate",
            "smoke_script",
            "pilot_acceptance_script",
            "factory_hermes_bridge",
            "control_plane_observer"
        ],
        "evidence_outputs": [
            "provider transcript",
            "provider transcript summary",
            "provider evidence scorecard",
            "replay verdict",
            "signed evidence pack",
            "cockpit/workbench links"
        ],
        "smoke_script": metadata.smoke_script.clone()
    }))
}

pub(crate) fn provider_warning_strings(provider_name: &str) -> Result<Vec<String>> {
    let matrix = provider_matrix_json()?;
    Ok(provider_warning_strings_from_matrix(&matrix, provider_name))
}

fn provider_warning_strings_from_matrix(
    matrix: &serde_json::Value,
    provider_name: &str,
) -> Vec<String> {
    let Some(provider) = json_array(matrix, "providers")
        .iter()
        .find(|provider| json_string(provider, "provider") == provider_name)
    else {
        return vec![format!("provider_unknown={provider_name}")];
    };
    let mut warnings = Vec::new();
    let doctor = provider.get("doctor").unwrap_or(&serde_json::Value::Null);
    let available = doctor
        .get("available")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !available {
        let blocker = json_string(doctor, "blocker");
        warnings.push(format!(
            "provider_unavailable={}",
            if blocker.is_empty() {
                "unknown"
            } else {
                blocker.as_str()
            }
        ));
    }
    warnings.push(format!(
        "timeout_seconds={}",
        provider
            .get("timeout_seconds")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_PROVIDER_TIMEOUT_SECONDS)
    ));
    warnings.push(format!(
        "execution_boundary={}",
        json_string(provider, "execution_boundary")
    ));
    for invariant in json_array(provider, "policy_invariants") {
        let invariant = invariant.as_str().unwrap_or_default();
        if invariant == "provider cannot write target repo directly" {
            warnings.push(invariant.to_string());
        }
    }
    warnings
}

pub(crate) struct ProviderProfile {
    pub(crate) name: &'static str,
    pub(crate) provider: &'static str,
    pub(crate) description: &'static str,
}

pub(crate) fn provider_profiles() -> &'static [ProviderProfile] {
    &[
        ProviderProfile {
            name: "scripted",
            provider: "scripted",
            description: "Deterministic local provider for smoke tests and fixtures.",
        },
        ProviderProfile {
            name: "codex",
            provider: "codex",
            description: "Codex CLI OAuth provider for implementation roles.",
        },
        ProviderProfile {
            name: "claude",
            provider: "claude",
            description: "Claude Code CLI OAuth provider for implementation roles.",
        },
        ProviderProfile {
            name: "antigravity",
            provider: "antigravity",
            description: "Google Antigravity CLI OAuth provider for implementation roles.",
        },
    ]
}

pub(crate) fn provider_profiles_json() -> Result<String> {
    let profiles = provider_profiles()
        .iter()
        .map(|profile| {
            serde_json::json!({
                "name": profile.name,
                "provider": profile.provider,
                "description": profile.description
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": "ao2.provider-profiles.v1",
        "profiles": profiles
    }))?)
}

pub(crate) fn materialize_template_workflow(target: &Path, name: &str) -> Result<PathBuf> {
    let Some(template) = TASK_TEMPLATES.iter().find(|template| template.name == name) else {
        anyhow::bail!("unknown template: {name}");
    };
    let dir = target.join(".ao2").join("generated-workflows");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let workflow = dir.join(format!("{name}.yaml"));
    fs::write(&workflow, template.content)
        .with_context(|| format!("write {}", workflow.display()))?;
    Ok(workflow)
}
