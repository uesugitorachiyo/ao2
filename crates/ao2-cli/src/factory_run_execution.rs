use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_adapters::parse_provider;
use ao2_runtime::{
    replay_run, run_risky_pr_provider_free, run_risky_pr_with_provider_prompt, ProviderRunOptions,
    ReplayOptions, RunOptions,
};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{atomic_write_text, json_string, read_prompt, sha256_file};
use crate::factory_compat::{factory_ensure_target_repo, read_factory_compat_value};
use crate::factory_evaluator::{
    factory_evaluator_parity_comparison, factory_native_evaluator_decision,
};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};
use crate::run_resume::approve_and_resume_persisted_sandbox_patches;

pub(crate) struct FactoryRunPlanOptions<'a> {
    pub(crate) plan: &'a Path,
    pub(crate) target: &'a Path,
    pub(crate) run_id: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt: Option<String>,
    pub(crate) provider_prompt_file: Option<PathBuf>,
    pub(crate) provider_max_budget_usd: Option<f64>,
    pub(crate) factory_decision: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) out: Option<PathBuf>,
}

pub(crate) fn factory_run_plan_json(
    options: FactoryRunPlanOptions<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(options.target)?;
    let target_root = fs::canonicalize(options.target)
        .with_context(|| format!("canonicalize factory target {}", options.target.display()))?;
    let plan_value = read_factory_compat_value(options.plan)?;
    if plan_value["schema_version"] != "ao2.factory-v3-compat-governed-plan.v1" {
        return Err(anyhow!(
            "factory run requires ao2.factory-v3-compat-governed-plan.v1 plan: {}",
            options.plan.display()
        ));
    }
    if plan_value["parity_checklist_progress"]["factory_v3_drives_workflow"] != false {
        return Err(anyhow!(
            "refusing factory compat run whose plan does not prove factory_v3_drives_workflow=false"
        ));
    }
    if plan_value["ao2_native_plan"]["runnable_workflow"]["factory_v3_drives_workflow"] != false {
        return Err(anyhow!(
            "refusing factory compat run whose runnable workflow is factory-v3 driven"
        ));
    }
    let workflow_path = plan_value["ao2_native_plan"]["runnable_workflow"]["path"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .with_context(|| {
            format!(
                "plan {} does not reference a readable runnable workflow",
                options.plan.display()
            )
        })?;
    let provider_mode = options.provider.is_some()
        || options.provider_prompt.is_some()
        || options.provider_prompt_file.is_some();
    let provider_execution = serde_json::json!({
        "mode": if provider_mode { "provider-backed" } else { "provider-free" },
        "provider": options.provider.as_deref().unwrap_or(if provider_mode { "scripted" } else { "none" }),
        "provider_prompt_supplied": options.provider_prompt.is_some() || options.provider_prompt_file.is_some(),
        "provider_prompt_file": options
            .provider_prompt_file
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        "provider_budget_owner": if options.provider_max_budget_usd.is_some() { "ao2" } else { "not_supplied" },
        "factory_v3_drives_workflow": false
    });
    let mut summary = if provider_mode {
        let provider = parse_provider(options.provider.as_deref().unwrap_or("scripted"))?;
        let prompt = read_prompt(options.provider_prompt, options.provider_prompt_file)?;
        run_risky_pr_with_provider_prompt(ProviderRunOptions {
            target_repo: target_root.clone(),
            workflow_path: workflow_path.clone(),
            run_id: options.run_id,
            provider,
            prompt,
            max_repair_attempts: options.max_repair_attempts,
            max_budget_usd: options.provider_max_budget_usd,
            repair_source: None,
        })?
    } else {
        run_risky_pr_provider_free(RunOptions {
            target_repo: target_root.clone(),
            workflow_path: workflow_path.clone(),
            run_id: options.run_id,
        })?
    };
    if provider_mode && summary.status == ao2_runtime::RunStatus::WaitingForApproval {
        if let Some(resumed) = approve_and_resume_persisted_sandbox_patches(
            &target_root,
            &summary.run_id,
            "human:factory-compat-operator",
        )? {
            summary = resumed;
        }
    }
    let replay = replay_run(ReplayOptions {
        target_repo: target_root.clone(),
        run_id: summary.run_id.clone(),
    })?;
    let evidence_pack_value = read_factory_compat_value(&summary.evidence_pack_path)?;
    let native_midpoint_gate_decision =
        factory_native_midpoint_gate_decision(&evidence_pack_value, &replay);
    let native_evaluator_decision =
        factory_native_evaluator_decision(&summary.report_path, &summary.evidence_pack_path)?;
    let provider_adapter_contract =
        evidence_pack_value["runtime_contract"]["provider_adapter_contract"].clone();
    let provider_adapter_contract_fulfilled = provider_adapter_contract["fulfilled"]
        .as_bool()
        .unwrap_or(false);
    let parity_comparison = match options.factory_decision.as_ref() {
        Some(path) => factory_evaluator_parity_comparison(path, &native_evaluator_decision)?,
        None => serde_json::json!({
            "schema_version": "ao2.factory-v3-evaluator-parity-comparison.v1",
            "status": "not_requested",
            "factory_v3_role": "parity_oracle_only",
            "ao2_decision_owner": "ao2-native-evaluator-closer"
        }),
    };
    let run_result_path = options
        .out
        .clone()
        .unwrap_or_else(|| summary.run_dir.join("factory-compat-run-result.json"));
    let handoff_evidence_path = run_result_path.with_extension("handoff.json");
    let memory_summary_path = summary
        .run_dir
        .join("factory-compat-hermes-memory-summary.json");
    let history_path = target_root
        .join(".ao2")
        .join("factory-compat")
        .join("history.json");
    let replay_digest_failures_empty = replay.digest_failures.is_empty();
    let parity_checklist_progress = serde_json::json!({
        "ao2_executes_generated_factory_compat_plan": true,
        "factory_v3_drives_workflow": false,
        "ao2_owns_midpoint_gate_decision": true,
        "ao2_owns_evaluator_closer_decision": true,
        "factory_v3_evaluator_compared_when_supplied": options.factory_decision.is_some(),
        "ao2_replay_completed": replay_digest_failures_empty,
        "ao2_exports_hermes_memory_summary": true,
        "ao2_persists_factory_compat_run_result": true,
        "ao2_persists_restart_safe_factory_compat_history": true,
        "ao2_produces_factory_compat_handoff_evidence": true,
        "ao2_can_sign_factory_compat_handoff_evidence": options.signing_key.is_some(),
        "ao2_provider_adapter_contract_hardened": provider_adapter_contract_fulfilled
    });
    let memory_summary = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-hermes-memory-summary.v1",
        "run_id": summary.run_id.clone(),
        "status": format!("{:?}", summary.status),
        "owner": "ao2",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "evidence_pack": summary.evidence_pack_path.display().to_string(),
        "report": summary.report_path.display().to_string(),
        "run_result_path": run_result_path.display().to_string(),
        "replacement_parity_progress": parity_checklist_progress.clone(),
        "native_midpoint_gate_verdict": native_midpoint_gate_decision["verdict"].clone(),
        "recommended_memory_scope": "bookkeeping-summary-only-no-secrets-no-stale-commit-shas",
        "secret_redaction": "paths-and-status-only; provider credentials and tokens are not serialized"
    });
    atomic_write_text(
        &memory_summary_path,
        &serde_json::to_string_pretty(&memory_summary)?,
    )?;
    let result = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-run-result.v1",
        "plan_path": options.plan.display().to_string(),
        "workflow_path": workflow_path.display().to_string(),
        "run_id": summary.run_id.clone(),
        "status": format!("{:?}", summary.status),
        "run_dir": summary.run_dir.display().to_string(),
        "run_result_path": run_result_path.display().to_string(),
        "memory_summary_path": memory_summary_path.display().to_string(),
        "history_path": history_path.display().to_string(),
        "handoff_evidence_path": handoff_evidence_path.display().to_string(),
        "evidence_pack": summary.evidence_pack_path.display().to_string(),
        "report": summary.report_path.display().to_string(),
        "rejection_count": summary.rejection_count,
        "approval_count": summary.approvals.len(),
        "denied_action_count": summary.denied_actions.len(),
        "native_midpoint_gate_decision": native_midpoint_gate_decision.clone(),
        "native_evaluator_decision": native_evaluator_decision,
        "factory_v3_evaluator_parity": parity_comparison,
        "provider_adapter_contract": provider_adapter_contract,
        "provider_execution": provider_execution,
        "replay": {
            "status": format!("{:?}", replay.status),
            "event_count": replay.event_count,
            "artifact_count": replay.artifact_count,
            "digest_failures": replay.digest_failures
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "factory_compat_handoff": {
            "schema_version": "ao2.factory-v3-compat-run-handoff-ref.v1",
            "handoff_evidence_path": handoff_evidence_path.display().to_string(),
            "signing_requested": options.signing_key.is_some(),
            "signer_id": options.signer_id.clone(),
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "parity_checklist_progress": parity_checklist_progress
    });
    persist_factory_compat_run_history(
        &history_path,
        serde_json::json!({
            "run_id": summary.run_id.clone(),
            "status": format!("{:?}", summary.status),
            "recorded_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            "plan_path": options.plan.display().to_string(),
            "workflow_path": workflow_path.display().to_string(),
            "run_dir": summary.run_dir.display().to_string(),
            "run_result_path": run_result_path.display().to_string(),
            "memory_summary_path": memory_summary_path.display().to_string(),
            "evidence_pack": summary.evidence_pack_path.display().to_string(),
            "report": summary.report_path.display().to_string(),
            "replay": {
                "status": format!("{:?}", replay.status),
                "event_count": replay.event_count,
                "artifact_count": replay.artifact_count,
                "digest_failure_count": replay.digest_failures.len()
            },
            "evaluator": {
                "midpoint_gate_owner": "ao2-native-midpoint-gate",
                "midpoint_gate_verdict": native_midpoint_gate_decision["verdict"].clone(),
                "owner": "ao2-native-evaluator-closer",
                "factory_v3_role": "parity_oracle_only",
                "verdict": native_evaluator_decision["verdict"].clone()
            },
            "continuity": {
                "survives_server_restart": true,
                "history_owner": "ao2",
                "factory_v3_drives_workflow": false,
                "cancel_retry_state_owner": "ao2-workbench-queue",
                "bookkeeping_safe_for_hermes": true
            }
        }),
    )?;
    atomic_write_text(&run_result_path, &serde_json::to_string_pretty(&result)?)?;
    let handoff = factory_run_handoff_evidence_json(
        &result,
        &run_result_path,
        &handoff_evidence_path,
        options.signing_key.as_deref(),
        &options.signer_id,
    )?;
    let mut result = result;
    if let Some(object) = result.as_object_mut() {
        object.insert("factory_compat_handoff_evidence".to_string(), handoff);
    }
    Ok(result)
}

fn factory_native_midpoint_gate_decision(
    evidence_pack: &serde_json::Value,
    replay: &ao2_runtime::ReplaySummary,
) -> serde_json::Value {
    let provider_adapter_contract = &evidence_pack["runtime_contract"]["provider_adapter_contract"];
    let provider_contract_fulfilled = provider_adapter_contract["fulfilled"]
        .as_bool()
        .unwrap_or(false);
    let replay_digest_clean = replay.digest_failures.is_empty();
    let evidence_pack_owner_ok = evidence_pack["runtime_contract"]["execution_owner"] == "ao2"
        && evidence_pack["runtime_contract"]["factory_v3_drives_workflow"] == false
        && evidence_pack["runtime_contract"]["factory_v3_role"] == "parity_oracle_only";
    let has_artifacts = evidence_pack["artifacts"]
        .as_array()
        .map(|artifacts| !artifacts.is_empty())
        .unwrap_or(false);
    let accepted = provider_contract_fulfilled
        && replay_digest_clean
        && evidence_pack_owner_ok
        && has_artifacts
        && replay.event_count > 0;
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-native-midpoint-gate.v1",
        "owner": "ao2-native-midpoint-gate",
        "verdict": if accepted { "accepted" } else { "blocked" },
        "factory_v3_required_to_decide": false,
        "factory_v3_role": "parity_oracle_only",
        "checks": {
            "provider_adapter_contract_fulfilled": provider_contract_fulfilled,
            "digest_replay_clean": replay_digest_clean,
            "evidence_pack_owner_ok": evidence_pack_owner_ok,
            "artifact_evidence_present": has_artifacts,
            "event_log_replay_present": replay.event_count > 0
        },
        "provider_adapter_contract": provider_adapter_contract,
        "replay": {
            "status": format!("{:?}", replay.status),
            "event_count": replay.event_count,
            "artifact_count": replay.artifact_count,
            "digest_failure_count": replay.digest_failures.len()
        },
        "required_contracts": [
            "evidence",
            "concerns",
            "blockers",
            "changed_files",
            "sandbox",
            "secret_redaction",
            "digest_replay",
            "ao2_owned_evidence_pack"
        ]
    })
}

fn factory_run_handoff_evidence_json(
    result: &serde_json::Value,
    run_result_path: &Path,
    handoff_evidence_path: &Path,
    signing_key: Option<&Path>,
    signer_id: &str,
) -> Result<serde_json::Value> {
    let run_result_sha256 = sha256_file(run_result_path)?;
    let mut handoff = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-run-handoff-evidence.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "run_id": json_string(result, "run_id"),
        "status": json_string(result, "status"),
        "verdict": result["native_evaluator_decision"]["verdict"].clone(),
        "run_result_path": run_result_path.display().to_string(),
        "run_result_sha256": run_result_sha256,
        "evidence_pack": json_string(result, "evidence_pack"),
        "report": json_string(result, "report"),
        "memory_summary_path": json_string(result, "memory_summary_path"),
        "history_path": json_string(result, "history_path"),
        "parity_checklist_progress": result["parity_checklist_progress"].clone(),
        "native_midpoint_gate_decision": result["native_midpoint_gate_decision"].clone(),
        "factory_v3_evaluator_parity": result["factory_v3_evaluator_parity"].clone(),
        "trust_boundary": result["trust_boundary"].clone(),
        "release_handoff_contract": {
            "primary_evidence_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "hermes_role": "front_end_scheduler_queue_and_memory_bookkeeping",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    let signature = match signing_key {
        Some(key_path) => {
            let signature_path = run_result_path.with_extension("json.sig");
            let public_key_path = run_result_path.with_extension("public.pem");
            derive_public_key_from_private_key(key_path, &public_key_path)?;
            sign_file_with_private_key(key_path, run_result_path, &signature_path)?;
            let signature_verified =
                verify_file_signature(run_result_path, &signature_path, &public_key_path)?;
            serde_json::json!({
                "schema_version": "ao2.factory-v3-compat-run-result-signature.v1",
                "signed_payload": "run_result",
                "signature_algorithm": "RSA/SHA-256",
                "signer_id": signer_id,
                "signature_path": signature_path.display().to_string(),
                "signature_sha256": sha256_file(&signature_path)?,
                "public_key_path": public_key_path.display().to_string(),
                "public_key_sha256": sha256_file(&public_key_path)?,
                "signature_verified": signature_verified
            })
        }
        None => serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-run-result-signature.v1",
            "signed_payload": "run_result",
            "signature_verified": false,
            "signature_status": "unsigned"
        }),
    };
    handoff["signature"] = signature;
    atomic_write_text(
        handoff_evidence_path,
        &serde_json::to_string_pretty(&handoff)?,
    )?;
    handoff["handoff_sha256"] = serde_json::json!(sha256_file(handoff_evidence_path)?);
    atomic_write_text(
        handoff_evidence_path,
        &serde_json::to_string_pretty(&handoff)?,
    )?;
    Ok(handoff)
}

fn persist_factory_compat_run_history(history_path: &Path, entry: serde_json::Value) -> Result<()> {
    let mut entries = if history_path.is_file() {
        read_factory_compat_value(history_path)
            .with_context(|| format!("read factory compat run history {}", history_path.display()))?
            .get("entries")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let run_id = entry
        .get("run_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    entries.retain(|existing| {
        existing.get("run_id").and_then(|value| value.as_str()) != Some(run_id.as_str())
    });
    entries.push(entry);
    entries.sort_by(|left, right| {
        left.get("recorded_at")
            .and_then(|value| value.as_str())
            .cmp(&right.get("recorded_at").and_then(|value| value.as_str()))
    });
    let history = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-run-history.v1",
        "owner": "ao2",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "history_path": history_path.display().to_string(),
        "entry_count": entries.len(),
        "continuity_contract": {
            "survives_server_restart": true,
            "factory_v3_drives_workflow": false,
            "cancel_retry_state_owner": "ao2-workbench-queue",
            "hermes_role": "front_end_scheduler_queue_and_memory_bookkeeping"
        },
        "entries": entries
    });
    atomic_write_text(history_path, &serde_json::to_string_pretty(&history)?)?;
    Ok(())
}
