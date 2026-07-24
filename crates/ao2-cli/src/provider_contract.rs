use anyhow::{Context, Result};

use super::{
    build_provider_prompt_command, doctor_provider, json_array, json_string, json_u64,
    parse_provider, DEFAULT_PROVIDER_TIMEOUT_SECONDS,
};
use crate::provider_ops::{provider_matrix_json, provider_smoke_guard_env};

pub(crate) fn provider_contract(
    provider: String,
    verify: bool,
    require: Vec<String>,
    json_output: bool,
) -> Result<()> {
    if verify {
        let required = if require.is_empty() {
            vec![provider]
        } else {
            require
        };
        let report = provider_contract_verify_json(&required);
        if json_output {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("schema={}", json_string(&report, "schema"));
            println!("status={}", json_string(&report, "status"));
            for reason in json_array(&report, "reasons") {
                println!(
                    "reason={}\tprovider={}\tmessage={}",
                    json_string(reason, "code"),
                    json_string(reason, "provider"),
                    json_string(reason, "message")
                );
            }
        }
        if json_string(&report, "status") != "verified" {
            anyhow::bail!("provider contract verification failed");
        }
        return Ok(());
    }

    let report = provider_contract_json(&provider)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("schema={}", json_string(&report, "schema"));
        println!("provider={}", json_string(&report, "provider"));
        println!("phase={}", json_string(&report, "phase"));
        println!(
            "execution_boundary={}",
            json_string(&report, "execution_boundary")
        );
        println!(
            "live_execution_guard_env={}",
            json_string(&report, "live_execution_guard_env")
        );
    }
    Ok(())
}

pub(crate) fn provider_contract_verify_json(required: &[String]) -> serde_json::Value {
    let required_providers = required
        .iter()
        .map(|provider| provider.trim().to_string())
        .filter(|provider| !provider.is_empty())
        .collect::<Vec<_>>();
    let required_providers = if required_providers.is_empty() {
        vec!["scripted".to_string()]
    } else {
        required_providers
    };
    let mut contracts = Vec::new();
    let mut reasons = Vec::new();

    for provider in &required_providers {
        if parse_provider(provider).is_err() {
            reasons.push(provider_contract_reason(
                "unknown_provider",
                provider,
                &format!("unknown provider: {provider}"),
            ));
            continue;
        }
        match provider_contract_json(provider) {
            Ok(contract) => {
                reasons.extend(provider_contract_verification_reasons(&contract));
                contracts.push(contract);
            }
            Err(error) => {
                reasons.push(provider_contract_reason(
                    "contract_unavailable",
                    provider,
                    &error.to_string(),
                ));
            }
        }
    }

    serde_json::json!({
        "schema": "ao2.provider-contract-verification.v1",
        "status": if reasons.is_empty() { "verified" } else { "failed" },
        "required_providers": required_providers,
        "contracts": contracts,
        "reasons": reasons
    })
}

fn provider_contract_verification_reasons(contract: &serde_json::Value) -> Vec<serde_json::Value> {
    let provider = json_string(contract, "provider");
    let mut reasons = Vec::new();
    if json_string(contract, "execution_boundary") != "sandbox_copy_then_digest_patch" {
        reasons.push(provider_contract_reason(
            "invalid_execution_boundary",
            &provider,
            "provider must execute in a sandbox copy before digest patch promotion",
        ));
    }
    if json_string(contract, "side_effect_boundary")
        != "target mutation only through exact digest patch apply"
    {
        reasons.push(provider_contract_reason(
            "invalid_side_effect_boundary",
            &provider,
            "provider target mutation must remain behind exact digest patch apply",
        ));
    }
    if matches!(provider.as_str(), "codex" | "claude" | "antigravity") {
        if json_string(contract, "phase") != "phase_1" {
            reasons.push(provider_contract_reason(
                "invalid_phase",
                &provider,
                "live provider contract must be phase_1",
            ));
        }
        if json_string(contract, "same_contract_as") != "scripted" {
            reasons.push(provider_contract_reason(
                "missing_scripted_contract_equivalence",
                &provider,
                "live provider contract must declare same_contract_as scripted",
            ));
        }
        if json_string(contract, "live_execution_guard_env").is_empty() {
            reasons.push(provider_contract_reason(
                "missing_live_guard",
                &provider,
                "live provider contract must declare a live execution guard env",
            ));
        }
    }
    if json_string(&contract["prompt_command"], "command").is_empty() {
        reasons.push(provider_contract_reason(
            "missing_prompt_command",
            &provider,
            "provider contract must include prompt command shape",
        ));
    }
    for invariant in [
        "provider cannot write target repo directly",
        "provider transcript is persisted as evidence",
        "sandbox diff preview emits exact action digest",
        "patch apply requires matching digest approval",
        "replay and closure remain runtime-owned",
    ] {
        if !json_array(contract, "policy_invariants")
            .iter()
            .any(|value| value.as_str() == Some(invariant))
            && !json_array(contract, "evidence_contract")
                .iter()
                .any(|value| value.as_str() == Some(invariant))
        {
            reasons.push(provider_contract_reason(
                "missing_contract_invariant",
                &provider,
                invariant,
            ));
        }
    }
    reasons
}

fn provider_contract_reason(code: &str, provider: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "code": code,
        "provider": provider,
        "message": message
    })
}

pub(crate) fn provider_contract_json(provider: &str) -> Result<serde_json::Value> {
    let provider = provider.trim();
    let provider_kind = parse_provider(provider)?;
    let doctor = doctor_provider(provider_kind)?;
    let matrix = provider_matrix_json()?;
    let matrix_entry = json_array(&matrix, "providers")
        .iter()
        .find(|entry| json_string(entry, "provider") == provider)
        .cloned()
        .context("provider missing from readiness matrix")?;
    let prompt_command = build_provider_prompt_command(
        provider_kind,
        "AO2 provider contract probe. Do not modify files.",
        "contract-probe",
        Some(DEFAULT_PROVIDER_TIMEOUT_SECONDS * 1000),
        None,
    )?;
    let phase = match provider {
        "scripted" => "phase_0",
        "codex" | "claude" | "antigravity" => "phase_1",
        _ => "unknown",
    };
    let guard_env = provider_smoke_guard_env(provider).unwrap_or("");

    Ok(serde_json::json!({
        "schema": "ao2.provider-contract.v1",
        "provider": provider,
        "phase": phase,
        "same_contract_as": if provider == "scripted" { "" } else { "scripted" },
        "doctor": doctor,
        "execution_boundary": json_string(&matrix_entry, "execution_boundary"),
        "side_effect_boundary": json_string(&matrix_entry, "side_effect_boundary"),
        "timeout_seconds": json_u64(&matrix_entry, "timeout_seconds"),
        "live_execution_guard_env": guard_env,
        "prompt_command": {
            "role_id": prompt_command.role_id,
            "command": prompt_command.command.display().to_string(),
            "args": prompt_command.args,
            "working_dir": prompt_command.working_dir.display().to_string(),
            "timeout_seconds": prompt_command.timeout_ms.unwrap_or_default() / 1000
        },
        "transcript_fields": json_array(&matrix_entry, "transcript_fields"),
        "policy_invariants": json_array(&matrix_entry, "policy_invariants"),
        "evidence_contract": [
            "provider transcript is persisted as evidence",
            "sandbox diff preview emits exact action digest",
            "patch apply requires matching digest approval",
            "replay and closure remain runtime-owned"
        ]
    }))
}
