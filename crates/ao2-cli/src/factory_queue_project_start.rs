use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{atomic_write_text, json_string, sanitize_greenfield_id, sha256_file};
use crate::factory_compat::{
    factory_ensure_target_repo, read_factory_compat_value, reject_factory_provider_api_key_auth,
};
use crate::factory_queue::{
    factory_queue_completion_contract_consumption_json, factory_queue_completion_contract_json,
    factory_queue_load, factory_queue_store,
};
use crate::factory_queue_execution::{factory_queue_run_next_json, FactoryQueueRunNextOptions};

pub(crate) struct FactoryQueueSubmitProjectStartOptions<'a> {
    pub(crate) target: &'a Path,
    pub(crate) project_spec: &'a Path,
    pub(crate) project_root: &'a Path,
    pub(crate) run_id: Option<String>,
    pub(crate) verifier_command: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt_dir: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) out_dir: Option<PathBuf>,
    pub(crate) handoff_bundle_out: Option<PathBuf>,
    pub(crate) handoff_bundle_report: Option<PathBuf>,
    pub(crate) receipt_out: Option<&'a Path>,
}

pub(crate) struct FactoryQueueProjectStartCompleteOptions<'a> {
    pub(crate) target: &'a Path,
    pub(crate) project_spec: &'a Path,
    pub(crate) project_root: &'a Path,
    pub(crate) run_id: Option<String>,
    pub(crate) verifier_command: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt_dir: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) out_dir: &'a Path,
    pub(crate) handoff_bundle_out: Option<PathBuf>,
    pub(crate) handoff_bundle_report: Option<PathBuf>,
}

pub(crate) fn factory_queue_submit_project_start_json(
    options: FactoryQueueSubmitProjectStartOptions<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(options.target)?;
    let target_root = fs::canonicalize(options.target)
        .with_context(|| format!("canonicalize factory target {}", options.target.display()))?;
    let absolutize = |path: PathBuf| -> PathBuf {
        if path.is_absolute() {
            path
        } else {
            target_root.join(path)
        }
    };
    let project_spec_path = absolutize(options.project_spec.to_path_buf());
    if !project_spec_path.is_file() {
        anyhow::bail!(
            "factory queue-submit-project-start requires readable --project-spec: {}",
            project_spec_path.display()
        );
    }
    let project_spec_path = fs::canonicalize(&project_spec_path).with_context(|| {
        format!(
            "canonicalize project-start spec {}",
            project_spec_path.display()
        )
    })?;
    let project_root = absolutize(options.project_root.to_path_buf());
    let run_id = options.run_id.unwrap_or_else(|| {
        format!(
            "factory-project-start-{}",
            Utc::now().format("%Y%m%d%H%M%S")
        )
    });
    let run_id = sanitize_greenfield_id(&run_id);
    let out_dir = options.out_dir.map(absolutize).unwrap_or_else(|| {
        target_root
            .join(".ao2")
            .join("factory-compat")
            .join("project-start-runs")
            .join(&run_id)
    });
    let handoff_bundle_out = options
        .handoff_bundle_out
        .map(absolutize)
        .unwrap_or_else(|| out_dir.join("project-start-handoff.tgz"));
    let handoff_bundle_report = options
        .handoff_bundle_report
        .map(absolutize)
        .unwrap_or_else(|| out_dir.join("factory-project-start-bundle.json"));
    let provider_prompt_dir = options.provider_prompt_dir.map(absolutize);
    let signing_key = options
        .signing_key
        .map(absolutize)
        .map(|path| fs::canonicalize(&path).unwrap_or(path));

    let mut queue = factory_queue_load(&target_root)?;
    let mut entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if entries
        .iter()
        .any(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(run_id.as_str()))
    {
        return Err(anyhow!("factory queue already contains run_id {run_id}"));
    }
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let entry = serde_json::json!({
        "schema_version": "ao2.factory-project-start-workbench-queue-entry.v1",
        "run_id": run_id,
        "job_kind": "factory_project_start",
        "status": "queued",
        "attempts": 0,
        "created_at": now,
        "updated_at": now,
        "project_start_request": {
            "project_spec": project_spec_path.display().to_string(),
            "project_spec_sha256": sha256_file(&project_spec_path)?,
            "project_root": project_root.display().to_string(),
            "verifier_command": options.verifier_command,
            "provider": options.provider,
            "provider_prompt_dir": provider_prompt_dir.as_ref().map(|path| path.display().to_string()),
            "signing_key": signing_key.as_ref().map(|path| path.display().to_string()),
            "signer_id": options.signer_id,
            "max_repair_attempts": options.max_repair_attempts,
            "out_dir": out_dir.display().to_string(),
            "handoff_bundle_out": handoff_bundle_out.display().to_string(),
            "handoff_bundle_report": handoff_bundle_report.display().to_string()
        },
        "parity_checklist_progress": {
            "ao2_persists_queue_history_cancel_retry_state": true,
            "ao2_queue_executes_project_start_handoff_job": true,
            "factory_v3_drives_workflow": false,
            "ao2_queue_owner": "ao2-workbench-queue"
        },
        "execution_contract": {
            "execution_owner": "ao2",
            "job_kind": "factory_project_start",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "transition_history": [{
            "at": now,
            "status": "queued",
            "reason": "submitted AO2 project-start handoff job to AO2-native persisted queue"
        }]
    });
    reject_factory_provider_api_key_auth("factory_queue_submit_project_start", &entry)?;
    entries.push(entry.clone());
    entries.sort_by(|left, right| {
        left.get("created_at")
            .and_then(|value| value.as_str())
            .cmp(&right.get("created_at").and_then(|value| value.as_str()))
    });
    queue["entries"] = serde_json::json!(entries);
    let queue_path = factory_queue_store(&target_root, &mut queue)?;
    let result = serde_json::json!({
        "schema_version": "ao2.factory-project-start-workbench-queue-submit.v1",
        "status": "queued",
        "job_kind": "factory_project_start",
        "run_id": json_string(&entry, "run_id"),
        "queue_path": queue_path.display().to_string(),
        "entry": entry,
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "ao2_decision_owner": "ao2-workbench-queue"
    });
    if let Some(out) = options.receipt_out {
        atomic_write_text(out, &serde_json::to_string_pretty(&result)?)?;
    }
    Ok(result)
}

pub(crate) fn factory_queue_project_start_complete_json(
    options: FactoryQueueProjectStartCompleteOptions<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(options.target)?;
    let target_root = fs::canonicalize(options.target)
        .with_context(|| format!("canonicalize factory target {}", options.target.display()))?;
    fs::create_dir_all(options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;

    let submit_path = options
        .out_dir
        .join("factory-queue-project-start-submit.json");
    let run_next_path = options
        .out_dir
        .join("factory-queue-project-start-run-next.json");
    let completion_contract_path = options
        .out_dir
        .join("factory-queue-project-start-completion-contract.json");
    let completion_contract_consumer_path = options
        .out_dir
        .join("factory-queue-project-start-completion-contract-consumer.json");

    let requested_run_id = options
        .run_id
        .as_ref()
        .map(|run_id| sanitize_greenfield_id(run_id));
    let queue_before = factory_queue_load(&target_root)?;
    let existing_entry = requested_run_id.as_deref().and_then(|run_id| {
        queue_before
            .get("entries")
            .and_then(|value| value.as_array())
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| {
                        entry.get("run_id").and_then(|value| value.as_str()) == Some(run_id)
                    })
                    .cloned()
            })
    });
    let signing_key_for_run = options.signing_key.clone();
    let signer_id_for_run = options.signer_id.clone();
    let max_repair_attempts = options.max_repair_attempts;

    let mut resume_mode = "submitted_new_queue_entry";
    let submitted = if let Some(entry) = existing_entry.clone() {
        let run_id = json_string(&entry, "run_id");
        if json_string(&entry, "job_kind") != "factory_project_start" {
            anyhow::bail!(
                "queue-project-start-complete run_id {run_id} exists but is not a factory_project_start job"
            );
        }
        let status = json_string(&entry, "status");
        if status != "queued" && status != "accepted" {
            anyhow::bail!(
                "queue-project-start-complete can only resume queued or accepted project-start entries, got {status} for run_id {run_id}"
            );
        }
        let project_spec_sha256 = sha256_file(options.project_spec).with_context(|| {
            format!(
                "hash queue-project-start-complete project spec {}",
                options.project_spec.display()
            )
        })?;
        let queued_spec_sha256 =
            json_string(&entry["project_start_request"], "project_spec_sha256");
        if queued_spec_sha256.trim().is_empty() || queued_spec_sha256 != project_spec_sha256 {
            anyhow::bail!(
                "queue-project-start-complete existing run_id {run_id} project spec digest mismatch"
            );
        }
        resume_mode = "reused_existing_queue_entry";
        let queue_path = json_string(&queue_before, "queue_path");
        let submit = if submit_path.is_file() {
            let value = read_factory_compat_value(&submit_path).with_context(|| {
                format!(
                    "read existing queue-project-start submit {}",
                    submit_path.display()
                )
            })?;
            if json_string(&value, "run_id") != run_id {
                anyhow::bail!(
                    "queue-project-start-complete existing submit artifact run_id mismatch: expected {run_id}, got {}",
                    json_string(&value, "run_id")
                );
            }
            value
        } else {
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-workbench-queue-submit.v1",
                "status": "queued",
                "job_kind": "factory_project_start",
                "run_id": run_id,
                "queue_path": queue_path,
                "entry": entry,
                "factory_v3_role": "parity_oracle_only",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "ao2_decision_owner": "ao2-workbench-queue",
                "resume": {
                    "rebuilt_missing_submit_artifact": true
                }
            })
        };
        atomic_write_text(&submit_path, &serde_json::to_string_pretty(&submit)?)?;
        submit
    } else {
        factory_queue_submit_project_start_json(FactoryQueueSubmitProjectStartOptions {
            target: &target_root,
            project_spec: options.project_spec,
            project_root: options.project_root,
            run_id: options.run_id,
            verifier_command: options.verifier_command,
            provider: options.provider,
            provider_prompt_dir: options.provider_prompt_dir,
            signing_key: options.signing_key.clone(),
            signer_id: options.signer_id.clone(),
            max_repair_attempts: options.max_repair_attempts,
            out_dir: Some(options.out_dir.to_path_buf()),
            handoff_bundle_out: options.handoff_bundle_out,
            handoff_bundle_report: options.handoff_bundle_report,
            receipt_out: Some(&submit_path),
        })?
    };
    let run_id = json_string(&submitted, "run_id");
    if run_id.trim().is_empty() {
        anyhow::bail!("queue-project-start-complete submit returned empty run_id");
    }

    let run_next = if let Some(entry) = existing_entry {
        let entry_status = json_string(&entry, "status");
        if entry_status == "queued" {
            factory_queue_run_next_json(FactoryQueueRunNextOptions {
                target: &target_root,
                provider: None,
                provider_prompt: None,
                provider_prompt_file: None,
                provider_max_budget_usd: None,
                factory_decision: None,
                signing_key: signing_key_for_run,
                signer_id: signer_id_for_run,
                max_repair_attempts,
                out: None,
            })?
        } else if run_next_path.is_file() {
            let value = read_factory_compat_value(&run_next_path).with_context(|| {
                format!(
                    "read existing queue-project-start run-next {}",
                    run_next_path.display()
                )
            })?;
            if json_string(&value, "run_id") != run_id {
                anyhow::bail!(
                    "queue-project-start-complete existing run-next artifact run_id mismatch: expected {run_id}, got {}",
                    json_string(&value, "run_id")
                );
            }
            value
        } else {
            serde_json::json!({
                "schema_version": "ao2.factory-project-start-workbench-queue-run-next.v1",
                "run_id": run_id,
                "job_kind": "factory_project_start",
                "status": entry_status,
                "queue_path": submitted["queue_path"].clone(),
                "claimed_entry": entry,
                "entry": entry,
                "resume": {
                    "rebuilt_missing_run_next_artifact": true
                },
                "parity_checklist_progress": {
                    "ao2_queue_executes_project_start_handoff_job": true,
                    "ao2_queue_verifies_project_start_handoff_bundle": true,
                    "ao2_queue_summarizes_project_start_handoff": true,
                    "ao2_queue_packages_project_start_closure": true,
                    "ao2_queue_verifies_project_start_closure": true,
                    "ao2_persists_queue_history_cancel_retry_state": true,
                    "factory_v3_drives_workflow": false,
                    "factory_v3_role": "parity_oracle_only",
                    "control_plane_role": "read_only_observer_after_signed_evidence",
                    "release_acceptance_owner": "factory-v3 evaluator-closer"
                },
                "ao2_decision_owner": "ao2-workbench-queue"
            })
        }
    } else {
        factory_queue_run_next_json(FactoryQueueRunNextOptions {
            target: &target_root,
            provider: None,
            provider_prompt: None,
            provider_prompt_file: None,
            provider_max_budget_usd: None,
            factory_decision: None,
            signing_key: signing_key_for_run,
            signer_id: signer_id_for_run,
            max_repair_attempts,
            out: None,
        })?
    };
    atomic_write_text(&run_next_path, &serde_json::to_string_pretty(&run_next)?)?;
    if json_string(&run_next, "run_id") != run_id {
        anyhow::bail!(
            "queue-project-start-complete expected run_id {run_id}, but queue-run-next executed {}",
            json_string(&run_next, "run_id")
        );
    }
    if json_string(&run_next, "status") != "accepted" {
        anyhow::bail!(
            "queue-project-start-complete run-next status must be accepted, got {}",
            json_string(&run_next, "status")
        );
    }

    let completion_contract =
        factory_queue_completion_contract_json(&target_root, Some(&run_id), false)?;
    atomic_write_text(
        &completion_contract_path,
        &serde_json::to_string_pretty(&completion_contract)?,
    )?;
    let completion_contract_consumer =
        factory_queue_completion_contract_consumption_json(&completion_contract_path)?;
    atomic_write_text(
        &completion_contract_consumer_path,
        &serde_json::to_string_pretty(&completion_contract_consumer)?,
    )?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-queue-complete.v1",
        "status": json_string(&completion_contract_consumer, "status"),
        "ready_for_operator_review": completion_contract_consumer["ready_for_operator_review"].clone(),
        "run_id": run_id,
        "queue_path": submitted["queue_path"].clone(),
        "queue_submit_status": submitted["status"].clone(),
        "queue_run_next_status": run_next["status"].clone(),
        "completion_contract_status": completion_contract["status"].clone(),
        "completion_contract_consumer_status": completion_contract_consumer["status"].clone(),
        "artifacts": {
            "queue_submit": submit_path.display().to_string(),
            "queue_run_next": run_next_path.display().to_string(),
            "completion_contract": completion_contract_path.display().to_string(),
            "completion_contract_consumer": completion_contract_consumer_path.display().to_string(),
            "project_start_bundle": completion_contract["artifacts"]["project_start_bundle"].clone(),
            "project_start_bundle_verification": completion_contract["artifacts"]["project_start_bundle_verification"].clone(),
            "project_start_operator_summary": completion_contract["artifacts"]["project_start_operator_summary"].clone(),
            "project_start_closure": completion_contract["artifacts"]["project_start_closure"].clone(),
            "project_start_closure_verification": completion_contract["artifacts"]["project_start_closure_verification"].clone()
        },
        "hermes_contract": {
            "front_end_reads_single_completion_record": true,
            "backend_used_bounded_ao2_queue": true,
            "requires_manual_command_sequence": false,
            "requires_manual_closure_commands": false,
            "completion_contract_consumed_contract_only": completion_contract_consumer["hermes_contract"]["consumed_contract_only"].clone()
        },
        "trust_boundary": completion_contract_consumer["trust_boundary"].clone(),
        "queue_submit": submitted,
        "queue_run_next": run_next,
        "completion_contract": completion_contract,
        "completion_contract_consumer": completion_contract_consumer,
        "resume": {
            "mode": resume_mode,
            "same_run_id_safe": true,
            "duplicates_queue_entries": false
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}
