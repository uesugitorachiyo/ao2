use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{atomic_write_text, json_string, sha256_file};
use crate::factory_compat::{factory_ensure_target_repo, read_factory_compat_value};
use crate::factory_evaluator::factory_evaluate_json;
use crate::factory_evidence::{
    factory_pack_evidence_json, factory_plan_json, factory_verify_evaluator_decision_json,
    FactoryPlanSigning,
};
use crate::factory_queue::factory_queue_submit_json;
use crate::factory_queue_execution::{factory_queue_run_next_json, FactoryQueueRunNextOptions};
use crate::release_crypto::verify_file_signature;

pub(crate) const GREENFIELD_THREE_OS_REQUIRED_OS: [&str; 3] = ["macos", "ubuntu", "windows"];

pub(crate) fn greenfield_three_os_smoke_gate_json(
    smokes: &[String],
    out: Option<&Path>,
) -> Result<serde_json::Value> {
    let mut smoke_paths = BTreeMap::<String, PathBuf>::new();
    let mut duplicate_os = Vec::<String>::new();
    let mut unknown_os = Vec::<String>::new();
    let mut input_errors = Vec::<serde_json::Value>::new();

    for spec in smokes {
        let Some((label, path)) = spec.split_once('=') else {
            input_errors.push(serde_json::json!({
                "input": spec,
                "error": "expected --smoke <os>=<path>"
            }));
            continue;
        };
        let label = match normalize_factory_replacement_smoke_os(label.trim()) {
            Some(label) => label,
            None => {
                unknown_os.push(label.trim().to_string());
                continue;
            }
        };
        if smoke_paths.contains_key(label) {
            duplicate_os.push(label.to_string());
            continue;
        }
        let path = path.trim();
        if path.is_empty() {
            input_errors.push(serde_json::json!({
                "input": spec,
                "error": "smoke path must not be empty"
            }));
            continue;
        }
        smoke_paths.insert(label.to_string(), PathBuf::from(path));
    }

    let mut missing_os = Vec::<String>::new();
    let mut accepted_os = Vec::<String>::new();
    let mut per_os = Vec::<serde_json::Value>::new();

    for required_os in GREENFIELD_THREE_OS_REQUIRED_OS {
        let Some(path) = smoke_paths.get(required_os) else {
            missing_os.push(required_os.to_string());
            continue;
        };
        let mut reasons = Vec::<String>::new();
        let smoke = match read_factory_compat_value(path) {
            Ok(value) => value,
            Err(error) => {
                reasons.push(format!(
                    "failed to read greenfield governed-run artifact: {error}"
                ));
                serde_json::json!({})
            }
        };
        validate_greenfield_governed_run_smoke(required_os, &smoke, &mut reasons);
        let status = if reasons.is_empty() {
            accepted_os.push(required_os.to_string());
            "accepted"
        } else {
            "rejected"
        };
        per_os.push(serde_json::json!({
            "os": required_os,
            "status": status,
            "artifact": path.display().to_string(),
            "run_id": json_string(&smoke, "run_id"),
            "reasons": reasons,
        }));
    }

    let status = if missing_os.is_empty()
        && duplicate_os.is_empty()
        && unknown_os.is_empty()
        && input_errors.is_empty()
        && accepted_os.len() == GREENFIELD_THREE_OS_REQUIRED_OS.len()
    {
        "accepted"
    } else {
        "rejected"
    };

    let result = serde_json::json!({
        "schema_version": "ao2.greenfield-three-os-smoke-gate.v1",
        "status": status,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "required_os": GREENFIELD_THREE_OS_REQUIRED_OS,
        "observed_os": smoke_paths.keys().cloned().collect::<Vec<_>>(),
        "accepted_os": accepted_os,
        "missing_os": missing_os,
        "duplicate_os": duplicate_os,
        "unknown_os": unknown_os,
        "input_errors": input_errors,
        "per_os": per_os,
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-greenfield-three-os-smoke-gate",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "artifact_path_policy": "content-based gate; referenced paths inside per-OS greenfield artifacts are not required to exist on this machine",
        "three_os_contract": {
            "path_separator_safe_artifacts": status == "accepted",
            "requires_native_windows_smoke": true,
            "requires_ubuntu_smoke": true,
            "requires_macos_smoke": true,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        }
    });

    if let Some(path) = out {
        atomic_write_text(path, &serde_json::to_string_pretty(&result)?)?;
    }
    Ok(result)
}

fn validate_greenfield_governed_run_smoke(
    os_label: &str,
    smoke: &serde_json::Value,
    reasons: &mut Vec<String>,
) {
    require_json_eq(
        smoke,
        &["schema_version"],
        "ao2.greenfield-governed-run.v1",
        reasons,
    );
    require_json_eq(smoke, &["status"], "accepted", reasons);
    require_json_bool(
        smoke,
        &[
            "greenfield_governed_run_checklist",
            "ao2_ingested_plain_spec",
        ],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &[
            "greenfield_governed_run_checklist",
            "ao2_executed_generated_governed_plan",
        ],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &[
            "greenfield_governed_run_checklist",
            "ao2_signed_evaluator_closure",
        ],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &[
            "greenfield_governed_run_checklist",
            "factory_v3_drives_workflow",
        ],
        false,
        reasons,
    );
    require_json_eq(
        smoke,
        &["trust_boundary", "execution_owner"],
        "ao2",
        reasons,
    );
    require_json_eq(
        smoke,
        &["trust_boundary", "release_acceptance_owner"],
        "factory-v3 evaluator-closer",
        reasons,
    );
    require_json_eq(
        smoke,
        &["trust_boundary", "control_plane_role"],
        "read_only_observer_after_signed_evidence",
        reasons,
    );
    require_json_bool(
        smoke,
        &["trust_boundary", "control_plane_approves_release"],
        false,
        reasons,
    );
    require_json_bool(
        smoke,
        &["trust_boundary", "mutates_ao_artifacts"],
        false,
        reasons,
    );
    require_json_eq(
        smoke,
        &["trust_boundary", "provider_auth"],
        "local OAuth CLI only; API-key provider auth forbidden",
        reasons,
    );
    require_json_eq(
        smoke,
        &["greenfield_governed_run_checklist", "factory_v3_role"],
        "parity_oracle_only",
        reasons,
    );
    require_json_eq(
        smoke,
        &["greenfield_governed_run_checklist", "control_plane_role"],
        "read_only_observer_after_signed_evidence",
        reasons,
    );
    if json_string(smoke, "run_id").trim().is_empty() {
        reasons.push(format!("{os_label} run_id must not be empty"));
    }
}

pub(crate) struct FactoryReplacementSmokeOptions<'a> {
    pub(crate) request: &'a Path,
    pub(crate) profile: Option<&'a Path>,
    pub(crate) runspec: &'a Path,
    pub(crate) role_contracts: &'a [PathBuf],
    pub(crate) target: &'a Path,
    pub(crate) run_id: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt: Option<String>,
    pub(crate) provider_prompt_file: Option<PathBuf>,
    pub(crate) provider_max_budget_usd: Option<f64>,
    pub(crate) factory_decision: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) out_dir: &'a Path,
}

pub(crate) struct FactoryGovernedRunOptions<'a> {
    pub(crate) request: &'a Path,
    pub(crate) profile: Option<&'a Path>,
    pub(crate) runspec: &'a Path,
    pub(crate) role_contracts: &'a [PathBuf],
    pub(crate) target: &'a Path,
    pub(crate) run_id: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt: Option<String>,
    pub(crate) provider_prompt_file: Option<PathBuf>,
    pub(crate) provider_max_budget_usd: Option<f64>,
    pub(crate) factory_decision: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) out_dir: &'a Path,
}

pub(crate) fn factory_replacement_smoke_json(
    options: FactoryReplacementSmokeOptions<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(options.target)?;
    validate_factory_replacement_smoke_run_id(&options.run_id)?;
    fs::create_dir_all(options.out_dir).with_context(|| {
        format!(
            "create replacement smoke out dir {}",
            options.out_dir.display()
        )
    })?;
    let out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize replacement smoke out dir {}",
            options.out_dir.display()
        )
    })?;
    let plan_path = out_dir.join(format!("{}-plan.json", options.run_id));
    let queue_receipt_path = out_dir.join(format!("{}-queue-submit.json", options.run_id));
    let run_result_path = out_dir.join(format!("{}-run-result.json", options.run_id));
    let run_result_verification_path =
        out_dir.join(format!("{}-run-result-verification.json", options.run_id));
    let packed_evidence_path = out_dir.join(format!("{}-evidence-pack.json", options.run_id));
    let smoke_result_path = out_dir.join(format!("{}-replacement-smoke.json", options.run_id));

    let plan = factory_plan_json(
        options.request,
        options.profile,
        Some(options.runspec),
        options.role_contracts,
        FactoryPlanSigning {
            key: options.signing_key.as_deref(),
            signer_id: &options.signer_id,
        },
        options.target,
        Some(&plan_path),
    )?;
    let queue_submit = factory_queue_submit_json(
        options.target,
        &plan_path,
        Some(options.run_id.clone()),
        Some(&queue_receipt_path),
    )?;
    let queue_run_next = factory_queue_run_next_json(FactoryQueueRunNextOptions {
        target: options.target,
        provider: options.provider,
        provider_prompt: options.provider_prompt,
        provider_prompt_file: options.provider_prompt_file,
        provider_max_budget_usd: options.provider_max_budget_usd,
        factory_decision: options.factory_decision,
        signing_key: options.signing_key.clone(),
        signer_id: options.signer_id.clone(),
        max_repair_attempts: options.max_repair_attempts,
        out: Some(run_result_path.clone()),
    })?;
    let run_result_verification = factory_verify_run_result_json(&run_result_path)?;
    atomic_write_text(
        &run_result_verification_path,
        &serde_json::to_string_pretty(&run_result_verification)?,
    )?;
    let pack_evidence = factory_pack_evidence_json(
        options.target,
        Some(&options.run_id),
        &packed_evidence_path,
        FactoryPlanSigning {
            key: options.signing_key.as_deref(),
            signer_id: &options.signer_id,
        },
    )?;

    let status = if queue_run_next["status"] == "accepted"
        && run_result_verification["status"] == "accepted"
        && pack_evidence["status"] == "produced"
    {
        "accepted"
    } else {
        "rejected"
    };
    let provider_execution = queue_run_next["run_result"]["provider_execution"].clone();
    let provider_backed = provider_execution["mode"] == "provider-backed";
    let result = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-replacement-smoke.v1",
        "status": status,
        "run_id": options.run_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "artifacts": {
            "plan": plan_path.display().to_string(),
            "queue_submit": queue_receipt_path.display().to_string(),
            "run_result": run_result_path.display().to_string(),
            "run_result_verification": run_result_verification_path.display().to_string(),
            "packed_evidence": packed_evidence_path.display().to_string(),
            "replacement_smoke": smoke_result_path.display().to_string()
        },
        "plan": plan,
        "queue_submit": queue_submit,
        "queue_run_next": queue_run_next,
        "run_result_verification": run_result_verification,
        "pack_evidence": pack_evidence,
        "provider_execution": provider_execution,
        "replacement_checklist": {
            "ao2_planned_factory_compat_workflow": true,
            "ao2_provider_backed_replacement_workflow": provider_backed,
            "ao2_queue_executed_factory_compat_workflow": status == "accepted",
            "ao2_verified_primary_run_result": status == "accepted",
            "ao2_packed_primary_evidence": status == "accepted",
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "three_os_contract": {
            "path_separator_safe_artifacts": true,
            "requires_native_windows_smoke": true,
            "requires_ubuntu_smoke": true,
            "requires_macos_smoke": true,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-replacement-smoke",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    });
    atomic_write_text(&smoke_result_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn factory_governed_run_json(
    options: FactoryGovernedRunOptions<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(options.target)?;
    validate_factory_replacement_smoke_run_id(&options.run_id)?;
    fs::create_dir_all(options.out_dir)
        .with_context(|| format!("create governed run out dir {}", options.out_dir.display()))?;
    let out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize governed run out dir {}",
            options.out_dir.display()
        )
    })?;
    let plan_path = out_dir.join(format!("{}-plan.json", options.run_id));
    let queue_receipt_path = out_dir.join(format!("{}-queue-submit.json", options.run_id));
    let run_result_path = out_dir.join(format!("{}-run-result.json", options.run_id));
    let run_result_verification_path =
        out_dir.join(format!("{}-run-result-verification.json", options.run_id));
    let packed_evidence_path = out_dir.join(format!("{}-evidence-pack.json", options.run_id));
    let evaluator_decision_path =
        out_dir.join(format!("{}-evaluator-decision.json", options.run_id));
    let evaluator_decision_verification_path = out_dir.join(format!(
        "{}-evaluator-decision-verification.json",
        options.run_id
    ));
    let governed_run_path = out_dir.join(format!("{}-governed-run.json", options.run_id));

    let plan = factory_plan_json(
        options.request,
        options.profile,
        Some(options.runspec),
        options.role_contracts,
        FactoryPlanSigning {
            key: options.signing_key.as_deref(),
            signer_id: &options.signer_id,
        },
        options.target,
        Some(&plan_path),
    )?;
    let queue_submit = factory_queue_submit_json(
        options.target,
        &plan_path,
        Some(options.run_id.clone()),
        Some(&queue_receipt_path),
    )?;
    let queue_run_next = factory_queue_run_next_json(FactoryQueueRunNextOptions {
        target: options.target,
        provider: options.provider,
        provider_prompt: options.provider_prompt,
        provider_prompt_file: options.provider_prompt_file,
        provider_max_budget_usd: options.provider_max_budget_usd,
        factory_decision: options.factory_decision.clone(),
        signing_key: options.signing_key.clone(),
        signer_id: options.signer_id.clone(),
        max_repair_attempts: options.max_repair_attempts,
        out: Some(run_result_path.clone()),
    })?;
    let run_result_verification = factory_verify_run_result_json(&run_result_path)?;
    atomic_write_text(
        &run_result_verification_path,
        &serde_json::to_string_pretty(&run_result_verification)?,
    )?;
    let pack_evidence = factory_pack_evidence_json(
        options.target,
        Some(&options.run_id),
        &packed_evidence_path,
        FactoryPlanSigning {
            key: options.signing_key.as_deref(),
            signer_id: &options.signer_id,
        },
    )?;
    let evaluator_decision = factory_evaluate_json(
        &packed_evidence_path,
        queue_run_next["entry"]["report"].as_str().map(Path::new),
        options.factory_decision.as_deref(),
        options.signing_key.as_deref(),
        &options.signer_id,
        Some(&evaluator_decision_path),
    )?;
    let evaluator_decision_verification =
        factory_verify_evaluator_decision_json(&evaluator_decision_path)?;
    atomic_write_text(
        &evaluator_decision_verification_path,
        &serde_json::to_string_pretty(&evaluator_decision_verification)?,
    )?;

    let status = if queue_run_next["status"] == "accepted"
        && run_result_verification["status"] == "accepted"
        && pack_evidence["status"] == "produced"
        && evaluator_decision_verification["status"] == "accepted"
    {
        "accepted"
    } else {
        "rejected"
    };
    let provider_execution = queue_run_next["run_result"]["provider_execution"].clone();
    let provider_backed = provider_execution["mode"] == "provider-backed";
    let role_contract_discovery = plan["ao2_native_plan"]["role_contract_discovery"].clone();
    let auto_loaded_role_contracts = role_contract_discovery["mode"]
        == "auto_discovered_from_ao_runspec_layout"
        && role_contract_discovery["loaded_count"]
            .as_u64()
            .is_some_and(|count| count > 0);
    let result = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-governed-run.v1",
        "status": status,
        "run_id": options.run_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "artifacts": {
            "plan": plan_path.display().to_string(),
            "queue_submit": queue_receipt_path.display().to_string(),
            "run_result": run_result_path.display().to_string(),
            "run_result_verification": run_result_verification_path.display().to_string(),
            "packed_evidence": packed_evidence_path.display().to_string(),
            "evaluator_decision": evaluator_decision_path.display().to_string(),
            "evaluator_decision_verification": evaluator_decision_verification_path.display().to_string(),
            "governed_run": governed_run_path.display().to_string()
        },
        "plan": plan,
        "queue_submit": queue_submit,
        "queue_run_next": queue_run_next,
        "run_result_verification": run_result_verification,
        "pack_evidence": pack_evidence,
        "evaluator_decision": evaluator_decision,
        "evaluator_decision_verification": evaluator_decision_verification,
        "provider_execution": provider_execution,
        "role_contract_discovery": role_contract_discovery,
        "governed_run_checklist": {
            "smoke_only_wrapper": false,
            "ao2_planned_factory_compat_workflow": true,
            "ao2_auto_loaded_role_contracts": auto_loaded_role_contracts,
            "ao2_provider_backed_governed_workflow": provider_backed,
            "ao2_queue_executed_factory_compat_workflow": status == "accepted",
            "ao2_verified_primary_run_result": status == "accepted",
            "ao2_packed_primary_evidence": status == "accepted",
            "ao2_signed_evaluator_closure": evaluator_decision_verification["status"] == "accepted",
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "hermes_role": "front_end_scheduler_queue_and_memory_bookkeeping"
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "decision_owner": "ao2",
            "front_end": "Hermes may launch and observe, but does not bypass AO2 gates",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-governed-run",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    });
    atomic_write_text(&governed_run_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) const FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS: [&str; 3] = ["macos", "ubuntu", "windows"];

pub(crate) fn factory_replacement_smoke_gate_json(
    smokes: &[String],
    out: Option<&Path>,
) -> Result<serde_json::Value> {
    let mut smoke_paths = BTreeMap::<String, PathBuf>::new();
    let mut duplicate_os = Vec::<String>::new();
    let mut unknown_os = Vec::<String>::new();
    let mut input_errors = Vec::<serde_json::Value>::new();

    for spec in smokes {
        let Some((label, path)) = spec.split_once('=') else {
            input_errors.push(serde_json::json!({
                "input": spec,
                "error": "expected --smoke <os>=<path>"
            }));
            continue;
        };
        let label = match normalize_factory_replacement_smoke_os(label.trim()) {
            Some(label) => label,
            None => {
                unknown_os.push(label.trim().to_string());
                continue;
            }
        };
        if smoke_paths.contains_key(label) {
            duplicate_os.push(label.to_string());
            continue;
        }
        let path = path.trim();
        if path.is_empty() {
            input_errors.push(serde_json::json!({
                "input": spec,
                "error": "smoke path must not be empty"
            }));
            continue;
        }
        smoke_paths.insert(label.to_string(), PathBuf::from(path));
    }

    let mut missing_os = Vec::<String>::new();
    let mut accepted_os = Vec::<String>::new();
    let mut per_os = Vec::<serde_json::Value>::new();

    for required_os in FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS {
        let Some(path) = smoke_paths.get(required_os) else {
            missing_os.push(required_os.to_string());
            continue;
        };
        let mut reasons = Vec::<String>::new();
        let smoke = match read_factory_compat_value(path) {
            Ok(value) => value,
            Err(error) => {
                reasons.push(format!("failed to read smoke artifact: {error}"));
                serde_json::json!({})
            }
        };
        validate_factory_replacement_smoke(required_os, &smoke, &mut reasons);
        let status = if reasons.is_empty() {
            accepted_os.push(required_os.to_string());
            "accepted"
        } else {
            "rejected"
        };
        per_os.push(serde_json::json!({
            "os": required_os,
            "status": status,
            "artifact": path.display().to_string(),
            "run_id": json_string(&smoke, "run_id"),
            "reasons": reasons,
        }));
    }

    let status = if missing_os.is_empty()
        && duplicate_os.is_empty()
        && unknown_os.is_empty()
        && input_errors.is_empty()
        && accepted_os.len() == FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS.len()
    {
        "accepted"
    } else {
        "rejected"
    };

    let result = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1",
        "status": status,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "required_os": FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS,
        "observed_os": smoke_paths.keys().cloned().collect::<Vec<_>>(),
        "accepted_os": accepted_os,
        "missing_os": missing_os,
        "duplicate_os": duplicate_os,
        "unknown_os": unknown_os,
        "input_errors": input_errors,
        "per_os": per_os,
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-three-os-replacement-smoke-gate",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "artifact_path_policy": "content-based gate; referenced paths inside per-OS smoke artifacts are not required to exist on this machine",
        "three_os_contract": {
            "path_separator_safe_artifacts": status == "accepted",
            "requires_native_windows_smoke": true,
            "requires_ubuntu_smoke": true,
            "requires_macos_smoke": true,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });

    if let Some(path) = out {
        atomic_write_text(path, &serde_json::to_string_pretty(&result)?)?;
    }
    Ok(result)
}

pub(crate) fn normalize_factory_replacement_smoke_os(label: &str) -> Option<&'static str> {
    match label.to_ascii_lowercase().as_str() {
        "macos" | "mac" | "darwin" => Some("macos"),
        "ubuntu" => Some("ubuntu"),
        "windows" | "win" | "win32" => Some("windows"),
        _ => None,
    }
}

fn validate_factory_replacement_smoke(
    os_label: &str,
    smoke: &serde_json::Value,
    reasons: &mut Vec<String>,
) {
    require_json_eq(
        smoke,
        &["schema_version"],
        "ao2.factory-v3-compat-replacement-smoke.v1",
        reasons,
    );
    require_json_eq(smoke, &["status"], "accepted", reasons);
    require_json_eq(smoke, &["factory_v3_role"], "parity_oracle_only", reasons);
    require_json_eq(
        smoke,
        &["ao2_decision_owner"],
        "ao2-native-replacement-smoke",
        reasons,
    );
    require_json_eq(
        smoke,
        &["control_plane_role"],
        "read_only_observer_after_signed_evidence",
        reasons,
    );
    require_json_bool(
        smoke,
        &[
            "replacement_checklist",
            "ao2_planned_factory_compat_workflow",
        ],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &[
            "replacement_checklist",
            "ao2_queue_executed_factory_compat_workflow",
        ],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &["replacement_checklist", "ao2_verified_primary_run_result"],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &["replacement_checklist", "ao2_packed_primary_evidence"],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &["replacement_checklist", "factory_v3_drives_workflow"],
        false,
        reasons,
    );
    require_json_eq(
        smoke,
        &["replacement_checklist", "factory_v3_role"],
        "parity_oracle_only",
        reasons,
    );
    require_json_eq(
        smoke,
        &["replacement_checklist", "control_plane_role"],
        "read_only_observer_after_signed_evidence",
        reasons,
    );
    require_json_eq(
        smoke,
        &["run_result_verification", "status"],
        "accepted",
        reasons,
    );
    require_json_bool(
        smoke,
        &["run_result_verification", "ao2_primary_run_result_ok"],
        true,
        reasons,
    );
    require_json_eq(smoke, &["pack_evidence", "status"], "produced", reasons);
    require_json_eq(
        smoke,
        &["pack_evidence", "evidence_pack_execution_owner"],
        "ao2",
        reasons,
    );
    require_json_eq(
        smoke,
        &["pack_evidence", "factory_v3_role"],
        "parity_oracle_only",
        reasons,
    );
    require_json_eq(
        smoke,
        &["pack_evidence", "control_plane_role"],
        "read_only_observer_after_signed_evidence",
        reasons,
    );
    require_json_bool(
        smoke,
        &["pack_evidence", "signature", "signature_verified"],
        true,
        reasons,
    );
    require_json_bool(
        smoke,
        &["three_os_contract", "path_separator_safe_artifacts"],
        true,
        reasons,
    );
    let os_required_key = match os_label {
        "windows" => "requires_native_windows_smoke",
        "ubuntu" => "requires_ubuntu_smoke",
        "macos" => "requires_macos_smoke",
        _ => "requires_unknown_smoke",
    };
    require_json_bool(
        smoke,
        &["three_os_contract", os_required_key],
        true,
        reasons,
    );
    require_json_eq(
        smoke,
        &["three_os_contract", "provider_auth"],
        "local OAuth CLI only; API-key provider auth forbidden",
        reasons,
    );
}

pub(crate) fn factory_replacement_parity_status_json(
    target: &Path,
    governed_run_path: &Path,
    expected_governed_run_sha256: &str,
    three_os_gate_path: &Path,
    expected_three_os_gate_sha256: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let actual_governed_run_sha256 = sha256_file(governed_run_path)
        .with_context(|| format!("sha256 {}", governed_run_path.display()))?;
    if actual_governed_run_sha256 != expected_governed_run_sha256 {
        anyhow::bail!(
            "governed-run sha256 mismatch: expected {}, got {}",
            expected_governed_run_sha256,
            actual_governed_run_sha256
        );
    }
    let actual_three_os_gate_sha256 = sha256_file(three_os_gate_path)
        .with_context(|| format!("sha256 {}", three_os_gate_path.display()))?;
    if actual_three_os_gate_sha256 != expected_three_os_gate_sha256 {
        anyhow::bail!(
            "three-os-gate sha256 mismatch: expected {}, got {}",
            expected_three_os_gate_sha256,
            actual_three_os_gate_sha256
        );
    }

    let governed_run = read_factory_compat_value(governed_run_path)?;
    let three_os_gate = read_factory_compat_value(three_os_gate_path)?;
    let accepted_os_count = three_os_gate["accepted_os"]
        .as_array()
        .map(|items| items.len())
        .unwrap_or(0);
    let missing_os_count = three_os_gate["missing_os"]
        .as_array()
        .map(|items| items.len())
        .unwrap_or(usize::MAX);
    let accepts_and_classifies = governed_run["plan"]["schema_version"]
        == "ao2.factory-v3-compat-governed-plan.v1"
        && json_path_bool(
            &governed_run,
            &[
                "plan",
                "parity_checklist_progress",
                "ao2_accepts_request_and_classifies",
            ],
        )
        && !json_path_bool(
            &governed_run,
            &[
                "plan",
                "classification",
                "factory_v3_required_before_classification",
            ],
        );
    let loads_runspec_profiles_roles = json_path_bool(
        &governed_run,
        &[
            "plan",
            "parity_checklist_progress",
            "ao2_loads_factory_v3_runspec_profiles_roles",
        ],
    ) || json_path_bool(
        &governed_run,
        &["governed_run_checklist", "ao2_auto_loaded_role_contracts"],
    );
    let provider_adapter_contract_hardened =
        json_path_bool(
            &governed_run,
            &[
                "parity_checklist_progress",
                "ao2_provider_adapter_contract_hardened",
            ],
        ) || json_path_bool(&governed_run, &["provider_adapter_contract", "fulfilled"]);
    let midpoint_and_closure_gates_native = json_path_bool(
        &governed_run,
        &[
            "parity_checklist_progress",
            "ao2_owns_midpoint_gate_decision",
        ],
    ) && json_path_bool(
        &governed_run,
        &["governed_run_checklist", "ao2_signed_evaluator_closure"],
    );
    let evaluator_closer_decision_native = json_path_bool(
        &governed_run,
        &[
            "parity_checklist_progress",
            "ao2_owns_evaluator_closer_decision",
        ],
    );
    let queue_history_restart_safe = json_path_bool(
        &governed_run,
        &[
            "governed_run_checklist",
            "ao2_queue_executed_factory_compat_workflow",
        ],
    ) && json_path_bool(
        &governed_run,
        &[
            "parity_checklist_progress",
            "ao2_persists_restart_safe_factory_compat_history",
        ],
    );
    let signed_evidence_replay_memory = json_path_bool(
        &governed_run,
        &["governed_run_checklist", "ao2_packed_primary_evidence"],
    ) && json_path_bool(
        &governed_run,
        &["parity_checklist_progress", "ao2_replay_completed"],
    ) && json_path_bool(
        &governed_run,
        &[
            "parity_checklist_progress",
            "ao2_exports_hermes_memory_summary",
        ],
    ) && json_path_bool(
        &governed_run,
        &[
            "parity_checklist_progress",
            "ao2_can_sign_factory_compat_handoff_evidence",
        ],
    );
    let three_os_validated = three_os_gate["schema_version"]
        == "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1"
        && three_os_gate["status"] == "accepted"
        && accepted_os_count == FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS.len()
        && missing_os_count == 0
        && json_path_bool(
            &three_os_gate,
            &["three_os_contract", "path_separator_safe_artifacts"],
        );
    let release_handoff_support_bundle_native = json_path_bool(
        &governed_run,
        &[
            "parity_checklist_progress",
            "ao2_produces_factory_compat_handoff_evidence",
        ],
    ) && json_path_bool(
        &governed_run,
        &[
            "parity_checklist_progress",
            "ao2_can_sign_factory_compat_handoff_evidence",
        ],
    );
    let factory_v3_parity_oracle_only = governed_run["schema_version"]
        == "ao2.factory-v3-compat-governed-run.v1"
        && governed_run["status"] == "accepted"
        && !json_path_bool(
            &governed_run,
            &["governed_run_checklist", "factory_v3_drives_workflow"],
        )
        && governed_run["factory_v3_role"] == "parity_oracle_only"
        && governed_run["control_plane_role"] == "read_only_observer_after_signed_evidence"
        && three_os_gate["factory_v3_role"] == "parity_oracle_only"
        && three_os_gate["control_plane_role"] == "read_only_observer_after_signed_evidence";

    let checklist = vec![
        (
            "accepts_and_classifies_work_request",
            accepts_and_classifies,
            "AO2 must accept and classify work requests before factory-v3 does",
        ),
        (
            "loads_runspec_profiles_roles",
            loads_runspec_profiles_roles,
            "AO2 must load or translate factory-v3 RunSpecs, profiles, and role contracts",
        ),
        (
            "provider_adapter_contract_hardened",
            provider_adapter_contract_hardened,
            "AO2 provider adapters must satisfy evidence, concern, blocker, changed-files, sandbox, and redaction contracts",
        ),
        (
            "midpoint_and_closure_gates_native",
            midpoint_and_closure_gates_native,
            "AO2 must own midpoint and closure gates natively",
        ),
        (
            "evaluator_closer_decision_native",
            evaluator_closer_decision_native,
            "AO2 must own evaluator/closer decision logic for the governed run",
        ),
        (
            "queue_history_restart_safe",
            queue_history_restart_safe,
            "AO2 must persist queue, history, cancel, retry, and restart-safe state",
        ),
        (
            "signed_evidence_replay_memory",
            signed_evidence_replay_memory,
            "AO2 must produce signed evidence, deterministic replay, and Hermes memory summaries",
        ),
        (
            "three_os_validated",
            three_os_validated,
            "AO2 replacement workflow must be validated on macOS, Ubuntu, and Windows",
        ),
        (
            "release_handoff_support_bundle_native",
            release_handoff_support_bundle_native,
            "AO2 must produce release-candidate handoff/support/verifier evidence",
        ),
        (
            "factory_v3_parity_oracle_only",
            factory_v3_parity_oracle_only,
            "factory-v3 must be parity oracle only, not workflow driver",
        ),
    ];
    let remaining_gaps = checklist
        .iter()
        .filter(|(_, passed, _)| !*passed)
        .map(|(name, _, reason)| {
            serde_json::json!({
                "check": name,
                "reason": reason,
            })
        })
        .collect::<Vec<_>>();
    let checklist_json = checklist
        .iter()
        .map(|(name, passed, _)| ((*name).to_string(), serde_json::json!(passed)))
        .collect::<serde_json::Map<_, _>>();
    let status = if remaining_gaps.is_empty() {
        "ready_for_parity_oracle"
    } else {
        "blocked"
    };
    let next_recommended_lengthy_task = if remaining_gaps.is_empty() {
        "Run factory-v3 parity-oracle comparison against this AO2 replacement-parity status, then let ao2-control-plane K37 observe the signed AO2 evidence chain read-only."
    } else {
        "Close the remaining AO2 replacement parity gaps listed in remaining_gaps before advancing ao2-control-plane observer work."
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-replacement-parity-status.v1",
        "status": status,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "target": target.display().to_string(),
        "governed_run": governed_run_path.display().to_string(),
        "governed_run_sha256": actual_governed_run_sha256,
        "three_os_gate": three_os_gate_path.display().to_string(),
        "three_os_gate_sha256": actual_three_os_gate_sha256,
        "checklist": serde_json::Value::Object(checklist_json),
        "remaining_gaps": remaining_gaps,
        "checked_artifacts": {
            "governed_run_schema": json_string(&governed_run, "schema_version"),
            "governed_run_status": json_string(&governed_run, "status"),
            "three_os_gate_schema": json_string(&three_os_gate, "schema_version"),
            "three_os_gate_status": json_string(&three_os_gate, "status"),
            "accepted_os_count": accepted_os_count,
            "missing_os_count": missing_os_count
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "decision_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_mutate_control_plane": false,
            "would_approve_release": false,
            "would_mutate_ao_artifacts": false
        },
        "next_recommended_lengthy_task": next_recommended_lengthy_task
    }))
}

fn json_path_bool(value: &serde_json::Value, path: &[&str]) -> bool {
    json_path(value, path)
        .and_then(|item| item.as_bool())
        .unwrap_or(false)
}

pub(crate) fn require_json_eq(
    value: &serde_json::Value,
    path: &[&str],
    expected: &str,
    reasons: &mut Vec<String>,
) {
    let actual = json_path(value, path);
    if actual.and_then(serde_json::Value::as_str) != Some(expected) {
        reasons.push(format!("{} must be {expected}", path.join(".")));
    }
}

pub(crate) fn require_json_bool(
    value: &serde_json::Value,
    path: &[&str],
    expected: bool,
    reasons: &mut Vec<String>,
) {
    let actual = json_path(value, path);
    if actual.and_then(serde_json::Value::as_bool) != Some(expected) {
        reasons.push(format!("{} must be {expected}", path.join(".")));
    }
}

pub(crate) fn json_path<'a>(
    value: &'a serde_json::Value,
    path: &[&str],
) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

pub(crate) fn validate_factory_replacement_smoke_run_id(run_id: &str) -> Result<()> {
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("--run-id must not be empty"));
    }
    if trimmed != run_id
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(anyhow!(
            "--run-id must be portable across Windows/macOS/Ubuntu and contain only ASCII letters, digits, '.', '-', or '_'"
        ));
    }
    Ok(())
}

pub(crate) fn factory_verify_handoff_json(handoff_path: &Path) -> Result<serde_json::Value> {
    let handoff = read_factory_compat_value(handoff_path)
        .with_context(|| format!("read factory compat handoff {}", handoff_path.display()))?;
    if handoff["schema_version"] != "ao2.factory-v3-compat-run-handoff-evidence.v1" {
        return Err(anyhow!(
            "factory handoff verify requires ao2.factory-v3-compat-run-handoff-evidence.v1: {}",
            handoff_path.display()
        ));
    }
    let handoff_base = handoff_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve_handoff_path = |value: &str| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            handoff_base.join(path)
        }
    };
    let run_result_path = resolve_handoff_path(&json_string(&handoff, "run_result_path"));
    if !run_result_path.is_file() {
        return Err(anyhow!(
            "factory handoff run result is not readable: {}",
            run_result_path.display()
        ));
    }
    let expected_run_result_sha256 = json_string(&handoff, "run_result_sha256");
    let actual_run_result_sha256 = sha256_file(&run_result_path)?;
    let run_result_digest_match = expected_run_result_sha256 == actual_run_result_sha256;
    let signature = &handoff["signature"];
    let signature_declared_verified = signature
        .get("signature_verified")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let signature_path = signature
        .get("signature_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(&resolve_handoff_path);
    let public_key_path = signature
        .get("public_key_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(resolve_handoff_path);
    let signed_payload = signature
        .get("signed_payload")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let signature_status = if signature_path.is_some() || public_key_path.is_some() {
        "signed"
    } else {
        "unsigned"
    };
    let public_key_digest_match = match public_key_path.as_ref() {
        Some(path) if path.is_file() => signature
            .get("public_key_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let signature_verified = match (signature_path.as_ref(), public_key_path.as_ref()) {
        (Some(signature_path), Some(public_key_path))
            if signature_path.is_file()
                && public_key_path.is_file()
                && signed_payload == "run_result"
                && public_key_digest_match =>
        {
            verify_file_signature(&run_result_path, signature_path, public_key_path)?
        }
        _ => false,
    };
    let signature_requirement_satisfied = signature_status == "signed" && signature_verified;
    let trust_boundary_ok = handoff["trust_boundary"]["execution_owner"] == "ao2"
        && handoff["trust_boundary"]["factory_v3_role"] == "parity_oracle_only"
        && handoff["trust_boundary"]["control_plane_role"]
            == "read_only_observer_after_signed_evidence";
    let release_handoff_contract_ok = handoff["release_handoff_contract"]["primary_evidence_owner"]
        == "ao2"
        && handoff["release_handoff_contract"]["factory_v3_role"] == "parity_oracle_only"
        && handoff["release_handoff_contract"]["control_plane_role"]
            == "read_only_observer_after_signed_evidence"
        && handoff["release_handoff_contract"]["hermes_role"]
            == "front_end_scheduler_queue_and_memory_bookkeeping"
        && handoff["release_handoff_contract"]["provider_auth"]
            == "local OAuth CLI only; API-key provider auth forbidden";
    let accepted = run_result_digest_match
        && signature_requirement_satisfied
        && trust_boundary_ok
        && release_handoff_contract_ok;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-run-handoff-verification.v1",
        "status": if accepted { "accepted" } else { "rejected" },
        "handoff_path": handoff_path.display().to_string(),
        "run_result_path": run_result_path.display().to_string(),
        "run_result_sha256_expected": expected_run_result_sha256,
        "run_result_sha256_actual": actual_run_result_sha256,
        "run_result_digest_match": run_result_digest_match,
        "signature_status": signature_status,
        "signature_declared_verified": signature_declared_verified,
        "signature_verified": signature_verified,
        "signed_payload": signed_payload,
        "public_key_digest_match": public_key_digest_match,
        "signature_requirement_satisfied": signature_requirement_satisfied,
        "trust_boundary_ok": trust_boundary_ok,
        "release_handoff_contract_ok": release_handoff_contract_ok,
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-factory-handoff-verifier",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    }))
}

pub(crate) fn factory_verify_run_result_json(run_result_path: &Path) -> Result<serde_json::Value> {
    let run_result = read_factory_compat_value(run_result_path)
        .with_context(|| format!("read AO2 factory run result {}", run_result_path.display()))?;
    if run_result["schema_version"] != "ao2.factory-v3-compat-run-result.v1" {
        return Err(anyhow!(
            "factory run-result verify requires ao2.factory-v3-compat-run-result.v1: {}",
            run_result_path.display()
        ));
    }
    let run_result_base = run_result_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve_run_result_path = |value: &str| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            run_result_base.join(path)
        }
    };
    let declared_run_result_path =
        resolve_run_result_path(&json_string(&run_result, "run_result_path"));
    let run_result_path_matches_input = declared_run_result_path == run_result_path;
    let evidence_pack_path = resolve_run_result_path(&json_string(&run_result, "evidence_pack"));
    let report_path = resolve_run_result_path(&json_string(&run_result, "report"));
    let memory_summary_path =
        resolve_run_result_path(&json_string(&run_result, "memory_summary_path"));
    let history_path = resolve_run_result_path(&json_string(&run_result, "history_path"));
    let handoff_evidence_path =
        resolve_run_result_path(&json_string(&run_result, "handoff_evidence_path"));

    let evidence_pack = if evidence_pack_path.is_file() {
        read_factory_compat_value(&evidence_pack_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    let evidence_pack_schema_ok = evidence_pack["schema_version"] == "ao2.evidence-pack.v1";
    let evidence_pack_owner_ok = evidence_pack["runtime_contract"]["execution_owner"] == "ao2"
        && evidence_pack["runtime_contract"]["factory_v3_drives_workflow"] == false
        && evidence_pack["runtime_contract"]["factory_v3_role"] == "parity_oracle_only";
    let replay_digest_clean = run_result["replay"]["digest_failures"]
        .as_array()
        .map(|failures| failures.is_empty())
        .unwrap_or(false);
    let midpoint_gate_accepted = run_result["native_midpoint_gate_decision"]["verdict"]
        == "accepted"
        && run_result["native_midpoint_gate_decision"]["factory_v3_required_to_decide"] == false;
    let native_evaluator_accepted = run_result["native_evaluator_decision"]["verdict"]
        == "accepted"
        && run_result["native_evaluator_decision"]["factory_v3_required_to_decide"] == false
        && run_result["native_evaluator_decision"]["owner"] == "ao2-native-evaluator-closer";
    let parity_status = run_result["factory_v3_evaluator_parity"]["status"]
        .as_str()
        .unwrap_or("");
    let factory_v3_parity_ok = matches!(parity_status, "matched" | "not_requested");
    let trust_boundary_ok = run_result["trust_boundary"]["execution_owner"] == "ao2"
        && run_result["trust_boundary"]["factory_v3_role"] == "parity_oracle_only"
        && run_result["trust_boundary"]["control_plane_role"]
            == "read_only_observer_after_signed_evidence"
        && run_result["trust_boundary"]["provider_auth"]
            == "local OAuth CLI only; API-key provider auth forbidden";
    let parity_checklist = &run_result["parity_checklist_progress"];
    let parity_checklist_ok = parity_checklist["ao2_executes_generated_factory_compat_plan"]
        == true
        && parity_checklist["factory_v3_drives_workflow"] == false
        && parity_checklist["ao2_owns_midpoint_gate_decision"] == true
        && parity_checklist["ao2_owns_evaluator_closer_decision"] == true
        && parity_checklist["ao2_replay_completed"] == true
        && parity_checklist["ao2_produces_factory_compat_handoff_evidence"] == true;
    let required_files_present = evidence_pack_path.is_file()
        && report_path.is_file()
        && memory_summary_path.is_file()
        && history_path.is_file()
        && handoff_evidence_path.is_file();
    let handoff_verification = if handoff_evidence_path.is_file() {
        factory_verify_handoff_json(&handoff_evidence_path).unwrap_or_else(|error| {
            serde_json::json!({
                "schema_version": "ao2.factory-v3-compat-run-handoff-verification.v1",
                "status": "rejected",
                "error": error.to_string()
            })
        })
    } else {
        serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-run-handoff-verification.v1",
            "status": "rejected",
            "error": "handoff evidence is missing"
        })
    };
    let handoff_verification_accepted = handoff_verification["status"] == "accepted";
    let ao2_primary_run_result_ok = run_result_path_matches_input
        && required_files_present
        && evidence_pack_schema_ok
        && evidence_pack_owner_ok
        && replay_digest_clean
        && midpoint_gate_accepted
        && native_evaluator_accepted
        && factory_v3_parity_ok
        && trust_boundary_ok
        && parity_checklist_ok
        && handoff_verification_accepted;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-run-result-verification.v1",
        "status": if ao2_primary_run_result_ok { "accepted" } else { "rejected" },
        "run_result_path": run_result_path.display().to_string(),
        "declared_run_result_path": declared_run_result_path.display().to_string(),
        "run_result_path_matches_input": run_result_path_matches_input,
        "run_id": run_result["run_id"].clone(),
        "run_status": run_result["status"].clone(),
        "required_files_present": required_files_present,
        "evidence_pack_path": evidence_pack_path.display().to_string(),
        "report_path": report_path.display().to_string(),
        "memory_summary_path": memory_summary_path.display().to_string(),
        "history_path": history_path.display().to_string(),
        "handoff_evidence_path": handoff_evidence_path.display().to_string(),
        "evidence_pack_schema_ok": evidence_pack_schema_ok,
        "evidence_pack_owner_ok": evidence_pack_owner_ok,
        "replay_digest_clean": replay_digest_clean,
        "midpoint_gate_accepted": midpoint_gate_accepted,
        "native_evaluator_accepted": native_evaluator_accepted,
        "factory_v3_parity_ok": factory_v3_parity_ok,
        "factory_v3_parity_status": parity_status,
        "trust_boundary_ok": trust_boundary_ok,
        "parity_checklist_ok": parity_checklist_ok,
        "handoff_verification_status": handoff_verification["status"].clone(),
        "handoff_signature_verified": handoff_verification["signature_verified"].clone(),
        "handoff_verification": handoff_verification,
        "ao2_primary_run_result_ok": ao2_primary_run_result_ok,
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-run-result-verifier",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    }))
}
