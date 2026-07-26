use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{atomic_write_text, json_string, sha256_file};
use crate::factory_compat::factory_ensure_target_repo;
use crate::factory_project_start::{
    factory_project_start_bundle_raw_path, factory_project_start_bundle_verify_json,
    factory_project_start_closure_json, factory_project_start_closure_verify_json,
    factory_project_start_json, factory_replacement_packet_json,
    factory_replacement_packet_verify_json, FactoryProjectStartOptions,
    FactoryReplacementPacketOptions,
};
use crate::factory_project_start_summary::{
    factory_project_start_summary_json, factory_project_start_summary_markdown,
};
use crate::factory_queue::{
    factory_queue_load, factory_queue_status_detail_json_with_options,
    factory_queue_status_is_terminal, factory_queue_store,
};
use crate::factory_run_execution::{factory_run_plan_json, FactoryRunPlanOptions};

pub(crate) fn factory_queue_transition_json(
    target: &Path,
    run_id: &str,
    status: &str,
    reason: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let mut queue = factory_queue_load(target)?;
    let mut entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let Some(index) = entries
        .iter()
        .position(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(run_id))
    else {
        return Err(anyhow!("factory queue does not contain run_id {run_id}"));
    };
    let previous_status = json_string(&entries[index], "status");
    let mut entry = entries[index].clone();
    entry["status"] = serde_json::json!(status);
    entry["updated_at"] = serde_json::json!(now.clone());
    if status == "queued" && previous_status != "queued" {
        let attempts = entry
            .get("attempts")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            + 1;
        entry["attempts"] = serde_json::json!(attempts);
    }
    let mut history = entry
        .get("transition_history")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    history.push(serde_json::json!({
        "at": now,
        "from": previous_status,
        "status": status,
        "reason": reason
    }));
    entry["transition_history"] = serde_json::json!(history);
    entries[index] = entry.clone();
    queue["entries"] = serde_json::json!(entries);
    let queue_path = factory_queue_store(target, &mut queue)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-workbench-queue-transition.v1",
        "status": status,
        "run_id": run_id,
        "queue_path": queue_path.display().to_string(),
        "entry": entry,
        "continuity_contract": queue["continuity_contract"].clone(),
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) struct FactoryQueueRunNextOptions<'a> {
    pub(crate) target: &'a Path,
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

pub(crate) fn factory_queue_run_next_json(
    options: FactoryQueueRunNextOptions<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(options.target)?;
    let target_root = fs::canonicalize(options.target)
        .with_context(|| format!("canonicalize factory target {}", options.target.display()))?;
    let queue = factory_queue_load(&target_root)?;
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let next = entries
        .iter()
        .find(|entry| entry.get("status").and_then(|value| value.as_str()) == Some("queued"))
        .cloned()
        .ok_or_else(|| anyhow!("factory queue has no queued AO2-governed runs to execute"))?;
    let run_id = json_string(&next, "run_id");
    if json_string(&next, "job_kind") == "factory_project_start" {
        return factory_queue_run_next_project_start_json(options, &target_root, next);
    }
    let plan_path = PathBuf::from(json_string(&next, "plan_path"));
    if run_id.trim().is_empty() || !plan_path.is_file() {
        if !run_id.trim().is_empty() {
            let _ = factory_queue_transition_json(
                &target_root,
                &run_id,
                "blocked",
                "AO2 queue-run-next refused unreadable queued plan path before execution",
            );
        }
        return Err(anyhow!(
            "queued factory run entry is missing a readable plan path for run_id {run_id}"
        ));
    }
    let queued_plan_sha256 = json_string(&next, "plan_sha256");
    let current_plan_sha256 = match sha256_file(&plan_path) {
        Ok(digest) => digest,
        Err(error) => {
            let _ = factory_queue_transition_json(
                &target_root,
                &run_id,
                "blocked",
                "AO2 queue-run-next refused unreadable queued plan path before execution",
            );
            return Err(anyhow!(
                "queued factory run plan path is not readable for run_id {run_id}: {error}"
            ));
        }
    };
    if queued_plan_sha256.trim().is_empty() || current_plan_sha256 != queued_plan_sha256 {
        let _ = factory_queue_transition_json(
            &target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next refused queued plan because persisted plan digest changed before execution",
        );
        return Err(anyhow!(
            "queued factory run plan digest mismatch for run_id {run_id}: expected {queued_plan_sha256}, got {current_plan_sha256}"
        ));
    }

    let running = factory_queue_transition_json(
        &target_root,
        &run_id,
        "running",
        "AO2 queue-run-next claimed persisted governed run for native execution",
    )?;

    let run_result = match factory_run_plan_json(FactoryRunPlanOptions {
        plan: &plan_path,
        target: &target_root,
        run_id: Some(run_id.clone()),
        provider: options.provider,
        provider_prompt: options.provider_prompt,
        provider_prompt_file: options.provider_prompt_file,
        provider_max_budget_usd: options.provider_max_budget_usd,
        factory_decision: options.factory_decision,
        signing_key: options.signing_key,
        signer_id: options.signer_id,
        max_repair_attempts: options.max_repair_attempts,
        out: options.out,
    }) {
        Ok(result) => result,
        Err(error) => {
            let _ = factory_queue_transition_json(
                &target_root,
                &run_id,
                "blocked",
                "AO2 queue-run-next failed before closure; inspect local run logs without serializing secrets",
            );
            return Err(error).with_context(|| format!("execute queued AO2 governed run {run_id}"));
        }
    };

    let final_status = match json_string(&run_result, "status").as_str() {
        "Accepted" => "accepted",
        "AcceptedWithConcerns" => "accepted_with_concerns",
        "Rejected" => "rejected",
        "Blocked" => "blocked",
        "Failed" => "failed",
        _ => "completed",
    };
    let mut queue = factory_queue_load(&target_root)?;
    let mut entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut final_entry = None;
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(run_id.as_str()))
    {
        let previous_status = json_string(entry, "status");
        entry["status"] = serde_json::json!(final_status);
        entry["updated_at"] = serde_json::json!(now.clone());
        entry["run_result_path"] = run_result["run_result_path"].clone();
        entry["evidence_pack"] = run_result["evidence_pack"].clone();
        entry["report"] = run_result["report"].clone();
        entry["memory_summary_path"] = run_result["memory_summary_path"].clone();
        entry["handoff_evidence_path"] = run_result["handoff_evidence_path"].clone();
        entry["provider_execution"] = run_result["provider_execution"].clone();
        entry["provider_adapter_contract"] = run_result["provider_adapter_contract"].clone();
        entry["native_evaluator_verdict"] =
            run_result["native_evaluator_decision"]["verdict"].clone();
        entry["replay"] = run_result["replay"].clone();
        let mut history = entry
            .get("transition_history")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        history.push(serde_json::json!({
            "at": now,
            "from": previous_status,
            "status": final_status,
            "reason": "AO2 queue-run-next completed native governed execution and persisted evidence references"
        }));
        entry["transition_history"] = serde_json::json!(history);
        final_entry = Some(entry.clone());
    }
    queue["entries"] = serde_json::json!(entries);
    let queue_path = factory_queue_store(&target_root, &mut queue)?;
    let refreshed = factory_queue_load(&target_root)?;
    let entry = final_entry.ok_or_else(|| {
        anyhow!("factory queue lost run_id {run_id} while persisting final evidence references")
    })?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-workbench-queue-run-next.v1",
        "run_id": run_id,
        "status": final_status,
        "queue_path": queue_path.display().to_string(),
        "claimed_entry": running["entry"].clone(),
        "entry": entry,
        "run_result": run_result,
        "continuity_contract": refreshed["continuity_contract"].clone(),
        "parity_checklist_progress": {
            "ao2_queue_can_execute_persisted_factory_compat_run": true,
            "ao2_persists_queue_history_cancel_retry_state": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

fn factory_queue_run_next_project_start_json(
    options: FactoryQueueRunNextOptions<'_>,
    target_root: &Path,
    next: serde_json::Value,
) -> Result<serde_json::Value> {
    let run_id = json_string(&next, "run_id");
    let request = &next["project_start_request"];
    let project_spec = PathBuf::from(json_string(request, "project_spec"));
    if run_id.trim().is_empty() || !project_spec.is_file() {
        if !run_id.trim().is_empty() {
            let _ = factory_queue_transition_json(
                target_root,
                &run_id,
                "blocked",
                "AO2 queue-run-next refused unreadable project-start spec before execution",
            );
        }
        return Err(anyhow!(
            "queued factory project-start entry is missing a readable project spec for run_id {run_id}"
        ));
    }
    let queued_spec_sha256 = json_string(request, "project_spec_sha256");
    let current_spec_sha256 = sha256_file(&project_spec).with_context(|| {
        format!(
            "read queued factory project-start spec {}",
            project_spec.display()
        )
    })?;
    if queued_spec_sha256.trim().is_empty() || queued_spec_sha256 != current_spec_sha256 {
        let _ = factory_queue_transition_json(
            target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next refused project-start because persisted spec digest changed before execution",
        );
        return Err(anyhow!(
            "queued factory project-start spec digest mismatch for run_id {run_id}: expected {queued_spec_sha256}, got {current_spec_sha256}"
        ));
    }

    let running = factory_queue_transition_json(
        target_root,
        &run_id,
        "running",
        "AO2 queue-run-next claimed project-start handoff job for native execution",
    )?;
    let optional_path = |key: &str| {
        let raw = json_string(request, key);
        if raw.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(raw))
        }
    };
    let out_dir = PathBuf::from(json_string(request, "out_dir"));
    let project_root = PathBuf::from(json_string(request, "project_root"));
    let signer_id = {
        let queued = json_string(request, "signer_id");
        if queued.trim().is_empty() {
            options.signer_id
        } else {
            queued
        }
    };
    let max_repair_attempts = request["max_repair_attempts"]
        .as_u64()
        .map(|value| value as usize)
        .unwrap_or(options.max_repair_attempts);
    let mut project_start = match factory_project_start_json(FactoryProjectStartOptions {
        project_spec: &project_spec,
        project_root: &project_root,
        run_id: run_id.clone(),
        verifier_command: json_string(request, "verifier_command"),
        provider: request["provider"].as_str().map(str::to_string),
        provider_prompt_dir: optional_path("provider_prompt_dir"),
        signing_key: optional_path("signing_key"),
        signer_id,
        max_repair_attempts,
        handoff_bundle_out: optional_path("handoff_bundle_out"),
        handoff_bundle_report: optional_path("handoff_bundle_report"),
        out_dir: &out_dir,
    }) {
        Ok(result) => result,
        Err(error) => {
            let _ = factory_queue_transition_json(
                target_root,
                &run_id,
                "blocked",
                "AO2 queue-run-next project-start failed before handoff bundle completion",
            );
            return Err(error)
                .with_context(|| format!("execute queued AO2 project-start job {run_id}"));
        }
    };
    let project_start_bundle_path = factory_project_start_bundle_raw_path(
        &out_dir,
        &json_string(
            &project_start["hermes_queue_handoff"],
            "project_start_bundle",
        ),
    )
    .with_context(|| format!("resolve queued project-start handoff bundle for {run_id}"))?;
    let project_start_bundle_verification_path =
        out_dir.join("factory-project-start-bundle-verification.json");
    let project_start_bundle_verification =
        match factory_project_start_bundle_verify_json(&project_start_bundle_path) {
            Ok(result) => result,
            Err(error) => {
                let _ = factory_queue_transition_json(
                    target_root,
                    &run_id,
                    "blocked",
                    "AO2 queue-run-next project-start handoff bundle verification errored",
                );
                return Err(error).with_context(|| {
                    format!(
                        "verify queued AO2 project-start handoff bundle {}",
                        project_start_bundle_path.display()
                    )
                });
            }
        };
    atomic_write_text(
        &project_start_bundle_verification_path,
        &serde_json::to_string_pretty(&project_start_bundle_verification)?,
    )?;
    let project_start_bundle_verification_sha256 =
        sha256_file(&project_start_bundle_verification_path)?;
    if json_string(&project_start_bundle_verification, "status") != "accepted" {
        let _ = factory_queue_transition_json(
            target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next project-start handoff bundle verification rejected the bundle",
        );
        return Err(anyhow!(
            "queued factory project-start handoff bundle verification failed for run_id {run_id}: {}",
            project_start_bundle_verification_path.display()
        ));
    }
    let project_start_path = factory_project_start_bundle_raw_path(
        &out_dir,
        &json_string(&project_start["artifacts"], "factory_project_start"),
    )
    .with_context(|| format!("resolve queued project-start result for {run_id}"))?;
    let project_start_operator_summary_path =
        out_dir.join("factory-project-start-operator-summary.json");
    let project_start_operator_summary_markdown_path =
        out_dir.join("factory-project-start-operator-summary.md");
    let project_start_operator_summary = match factory_project_start_summary_json(
        &project_start_path,
        &project_start_bundle_verification_path,
    ) {
        Ok(result) => result,
        Err(error) => {
            let _ = factory_queue_transition_json(
                target_root,
                &run_id,
                "blocked",
                "AO2 queue-run-next project-start operator summary errored",
            );
            return Err(error).with_context(|| {
                format!(
                    "summarize queued AO2 project-start handoff {}",
                    project_start_path.display()
                )
            });
        }
    };
    atomic_write_text(
        &project_start_operator_summary_path,
        &serde_json::to_string_pretty(&project_start_operator_summary)?,
    )?;
    atomic_write_text(
        &project_start_operator_summary_markdown_path,
        &factory_project_start_summary_markdown(&project_start_operator_summary),
    )?;
    let project_start_operator_summary_sha256 = sha256_file(&project_start_operator_summary_path)?;
    if json_string(&project_start_operator_summary, "status") != "accepted" {
        let _ = factory_queue_transition_json(
            target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next project-start operator summary rejected the handoff",
        );
        return Err(anyhow!(
            "queued factory project-start operator summary failed for run_id {run_id}: {}",
            project_start_operator_summary_path.display()
        ));
    }
    project_start["checks"]["project_start_bundle_verification_status"] =
        project_start_bundle_verification["status"].clone();
    project_start["checks"]["project_start_operator_summary_status"] =
        project_start_operator_summary["status"].clone();
    project_start["artifacts"]["project_start_bundle_verification"] =
        serde_json::json!(project_start_bundle_verification_path.display().to_string());
    project_start["artifacts"]["project_start_bundle_verification_sha256"] =
        serde_json::json!(project_start_bundle_verification_sha256.clone());
    project_start["artifacts"]["project_start_operator_summary"] =
        serde_json::json!(project_start_operator_summary_path.display().to_string());
    project_start["artifacts"]["project_start_operator_summary_sha256"] =
        serde_json::json!(project_start_operator_summary_sha256.clone());
    project_start["artifacts"]["project_start_operator_summary_markdown"] =
        serde_json::json!(project_start_operator_summary_markdown_path
            .display()
            .to_string());

    let final_status = match json_string(&project_start, "status").as_str() {
        "accepted" => "accepted",
        "accepted_with_concerns" => "accepted_with_concerns",
        "rejected" => "rejected",
        "blocked" => "blocked",
        "failed" => "failed",
        _ => "completed",
    };
    let mut queue = factory_queue_load(target_root)?;
    let mut entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut final_entry = None;
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(run_id.as_str()))
    {
        let previous_status = json_string(entry, "status");
        entry["status"] = serde_json::json!(final_status);
        entry["updated_at"] = serde_json::json!(now.clone());
        entry["project_start"] = project_start["artifacts"]["factory_project_start"].clone();
        entry["project_start_status"] = project_start["status"].clone();
        entry["project_acceptance_review"] =
            project_start["artifacts"]["project_acceptance_review"].clone();
        entry["project_acceptance_review_sha256"] =
            project_start["artifacts"]["project_acceptance_review_sha256"].clone();
        entry["project_acceptance_review_status"] =
            project_start["checks"]["project_acceptance_review_status"].clone();
        entry["project_acceptance_review_recommended_decision"] =
            project_start["checks"]["project_acceptance_review_recommended_decision"].clone();
        entry["project_start_bundle"] =
            project_start["hermes_queue_handoff"]["project_start_bundle"].clone();
        entry["project_start_bundle_sha256"] =
            project_start["hermes_queue_handoff"]["project_start_bundle_sha256"].clone();
        entry["project_start_bundle_verification"] =
            serde_json::json!(project_start_bundle_verification_path.display().to_string());
        entry["project_start_bundle_verification_sha256"] =
            serde_json::json!(project_start_bundle_verification_sha256);
        entry["project_start_bundle_verification_status"] =
            project_start_bundle_verification["status"].clone();
        entry["project_start_bundle_verification_checks"] =
            project_start_bundle_verification["checks"].clone();
        entry["project_start_operator_summary"] =
            serde_json::json!(project_start_operator_summary_path.display().to_string());
        entry["project_start_operator_summary_markdown"] =
            serde_json::json!(project_start_operator_summary_markdown_path
                .display()
                .to_string());
        entry["project_start_operator_summary_sha256"] =
            serde_json::json!(project_start_operator_summary_sha256);
        entry["project_start_operator_summary_status"] =
            project_start_operator_summary["status"].clone();
        entry["project_start_operator_summary_checks"] =
            project_start_operator_summary["checks"].clone();
        entry["project_start_operator_summary_result"] = project_start_operator_summary.clone();
        entry["hermes_queue_handoff"] = project_start["hermes_queue_handoff"].clone();
        entry["project_start_bundle_verification_result"] =
            project_start_bundle_verification.clone();
        entry["project_start_result"] = project_start.clone();
        entry["parity_checklist_progress"]["ao2_queue_executed_project_start_handoff_job"] =
            serde_json::json!(true);
        entry["parity_checklist_progress"]["ao2_queue_verifies_project_start_handoff_bundle"] =
            serde_json::json!(true);
        entry["parity_checklist_progress"]["ao2_queue_summarizes_project_start_handoff"] =
            serde_json::json!(true);
        let mut history = entry
            .get("transition_history")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        history.push(serde_json::json!({
            "at": now,
            "from": previous_status,
            "status": final_status,
            "reason": "AO2 queue-run-next completed project-start handoff job and persisted bundle and summary references"
        }));
        entry["transition_history"] = serde_json::json!(history);
        final_entry = Some(entry.clone());
    }
    queue["entries"] = serde_json::json!(entries.clone());
    let entry_for_closure = final_entry.ok_or_else(|| {
        anyhow!(
            "factory queue lost project-start run_id {run_id} while preparing closure references"
        )
    })?;
    let project_start_queue_status_path = out_dir.join("factory-queue-project-start-status.json");
    let project_start_latest_queue_status_path =
        out_dir.join("factory-queue-project-start-latest-status.json");
    let project_start_closure_path = out_dir.join("project-start-closure.tgz");
    let project_start_closure_json_path = out_dir.join("factory-project-start-closure.json");
    let project_start_closure_verification_path =
        out_dir.join("factory-project-start-closure-verification.json");
    let replacement_packet_archive_path = out_dir.join("factory-replacement-packet.tgz");
    let replacement_packet_json_path = out_dir.join("factory-replacement-packet.json");
    let replacement_packet_verification_path =
        out_dir.join("factory-replacement-packet-verification.json");
    let project_start_queue_status = factory_queue_status_detail_json_with_options(
        &queue,
        entry_for_closure.clone(),
        &run_id,
        false,
    )?;
    let latest_project_start_entry = entries
        .iter()
        .rev()
        .find(|entry| {
            entry.get("job_kind").and_then(|value| value.as_str()) == Some("factory_project_start")
                && factory_queue_status_is_terminal(&json_string(entry, "status"))
        })
        .cloned()
        .ok_or_else(|| anyhow!("factory queue has no completed project-start entry to close"))?;
    let latest_project_start_run_id = json_string(&latest_project_start_entry, "run_id");
    let project_start_latest_queue_status = factory_queue_status_detail_json_with_options(
        &queue,
        latest_project_start_entry,
        &latest_project_start_run_id,
        false,
    )?;
    atomic_write_text(
        &project_start_queue_status_path,
        &serde_json::to_string_pretty(&project_start_queue_status)?,
    )?;
    atomic_write_text(
        &project_start_latest_queue_status_path,
        &serde_json::to_string_pretty(&project_start_latest_queue_status)?,
    )?;
    let project_start_closure = match factory_project_start_closure_json(
        &project_start_queue_status_path,
        &project_start_latest_queue_status_path,
        &project_start_closure_path,
    ) {
        Ok(result) => result,
        Err(error) => {
            let _ = factory_queue_transition_json(
                target_root,
                &run_id,
                "blocked",
                "AO2 queue-run-next project-start closure packaging errored",
            );
            return Err(error).with_context(|| {
                format!(
                    "package queued AO2 project-start closure {}",
                    project_start_closure_path.display()
                )
            });
        }
    };
    atomic_write_text(
        &project_start_closure_json_path,
        &serde_json::to_string_pretty(&project_start_closure)?,
    )?;
    let project_start_closure_sha256 = sha256_file(&project_start_closure_path)?;
    let project_start_closure_json_sha256 = sha256_file(&project_start_closure_json_path)?;
    if json_string(&project_start_closure, "status") != "packaged" {
        let _ = factory_queue_transition_json(
            target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next project-start closure package was not packaged",
        );
        return Err(anyhow!(
            "queued factory project-start closure packaging failed for run_id {run_id}: {}",
            project_start_closure_json_path.display()
        ));
    }
    let project_start_closure_verification =
        match factory_project_start_closure_verify_json(&project_start_closure_path) {
            Ok(result) => result,
            Err(error) => {
                let _ = factory_queue_transition_json(
                    target_root,
                    &run_id,
                    "blocked",
                    "AO2 queue-run-next project-start closure verification errored",
                );
                return Err(error).with_context(|| {
                    format!(
                        "verify queued AO2 project-start closure {}",
                        project_start_closure_path.display()
                    )
                });
            }
        };
    atomic_write_text(
        &project_start_closure_verification_path,
        &serde_json::to_string_pretty(&project_start_closure_verification)?,
    )?;
    let project_start_closure_verification_sha256 =
        sha256_file(&project_start_closure_verification_path)?;
    if json_string(&project_start_closure_verification, "status") != "accepted" {
        let _ = factory_queue_transition_json(
            target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next project-start closure verification rejected the package",
        );
        return Err(anyhow!(
            "queued factory project-start closure verification failed for run_id {run_id}: {}",
            project_start_closure_verification_path.display()
        ));
    }
    let project_start_queue_status_sha256 = sha256_file(&project_start_queue_status_path)?;
    let project_start_latest_queue_status_sha256 =
        sha256_file(&project_start_latest_queue_status_path)?;
    let replacement_packet =
        match factory_replacement_packet_json(FactoryReplacementPacketOptions {
            queue_status: &project_start_queue_status_path,
            latest_queue_status: &project_start_latest_queue_status_path,
            closure: &project_start_closure_path,
            closure_verification: &project_start_closure_verification_path,
            cross_os_readbacks: &[],
            out: &replacement_packet_archive_path,
        }) {
            Ok(result) => result,
            Err(error) => {
                let _ = factory_queue_transition_json(
                    target_root,
                    &run_id,
                    "blocked",
                    "AO2 queue-run-next project-start replacement packet packaging errored",
                );
                return Err(error).with_context(|| {
                    format!(
                        "package queued AO2 replacement packet {}",
                        replacement_packet_archive_path.display()
                    )
                });
            }
        };
    atomic_write_text(
        &replacement_packet_json_path,
        &serde_json::to_string_pretty(&replacement_packet)?,
    )?;
    let replacement_packet_sha256 = sha256_file(&replacement_packet_json_path)?;
    let replacement_packet_archive_sha256 = sha256_file(&replacement_packet_archive_path)?;
    if json_string(&replacement_packet, "status") != "packaged" {
        let _ = factory_queue_transition_json(
            target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next project-start replacement packet was not packaged",
        );
        return Err(anyhow!(
            "queued factory project-start replacement packet packaging failed for run_id {run_id}: {}",
            replacement_packet_json_path.display()
        ));
    }
    let replacement_packet_verification =
        match factory_replacement_packet_verify_json(&replacement_packet_archive_path) {
            Ok(result) => result,
            Err(error) => {
                let _ = factory_queue_transition_json(
                    target_root,
                    &run_id,
                    "blocked",
                    "AO2 queue-run-next project-start replacement packet verification errored",
                );
                return Err(error).with_context(|| {
                    format!(
                        "verify queued AO2 replacement packet {}",
                        replacement_packet_archive_path.display()
                    )
                });
            }
        };
    atomic_write_text(
        &replacement_packet_verification_path,
        &serde_json::to_string_pretty(&replacement_packet_verification)?,
    )?;
    let replacement_packet_verification_sha256 =
        sha256_file(&replacement_packet_verification_path)?;
    if json_string(&replacement_packet_verification, "status") != "accepted" {
        let _ = factory_queue_transition_json(
            target_root,
            &run_id,
            "blocked",
            "AO2 queue-run-next project-start replacement packet verification rejected the package",
        );
        return Err(anyhow!(
            "queued factory project-start replacement packet verification failed for run_id {run_id}: {}",
            replacement_packet_verification_path.display()
        ));
    }
    let mut final_entry = None;
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(run_id.as_str()))
    {
        entry["project_start_queue_status"] =
            serde_json::json!(project_start_queue_status_path.display().to_string());
        entry["project_start_queue_status_sha256"] =
            serde_json::json!(project_start_queue_status_sha256);
        entry["project_start_latest_queue_status"] =
            serde_json::json!(project_start_latest_queue_status_path.display().to_string());
        entry["project_start_latest_queue_status_sha256"] =
            serde_json::json!(project_start_latest_queue_status_sha256);
        entry["project_start_closure"] =
            serde_json::json!(project_start_closure_path.display().to_string());
        entry["project_start_closure_json"] =
            serde_json::json!(project_start_closure_json_path.display().to_string());
        entry["project_start_closure_sha256"] = serde_json::json!(project_start_closure_sha256);
        entry["project_start_closure_json_sha256"] =
            serde_json::json!(project_start_closure_json_sha256);
        entry["project_start_closure_status"] = project_start_closure["status"].clone();
        entry["project_start_closure_result"] = project_start_closure.clone();
        entry["project_start_closure_verification"] =
            serde_json::json!(project_start_closure_verification_path
                .display()
                .to_string());
        entry["project_start_closure_verification_sha256"] =
            serde_json::json!(project_start_closure_verification_sha256);
        entry["project_start_closure_verification_status"] =
            project_start_closure_verification["status"].clone();
        entry["project_start_closure_verification_checks"] =
            project_start_closure_verification["checks"].clone();
        entry["project_start_closure_verification_result"] =
            project_start_closure_verification.clone();
        entry["replacement_packet"] =
            serde_json::json!(replacement_packet_json_path.display().to_string());
        entry["replacement_packet_sha256"] = serde_json::json!(replacement_packet_sha256);
        entry["replacement_packet_archive"] =
            serde_json::json!(replacement_packet_archive_path.display().to_string());
        entry["replacement_packet_archive_sha256"] =
            serde_json::json!(replacement_packet_archive_sha256);
        entry["replacement_packet_status"] = replacement_packet["status"].clone();
        entry["replacement_packet_result"] = replacement_packet.clone();
        entry["replacement_packet_verification"] =
            serde_json::json!(replacement_packet_verification_path.display().to_string());
        entry["replacement_packet_verification_sha256"] =
            serde_json::json!(replacement_packet_verification_sha256);
        entry["replacement_packet_verification_status"] =
            replacement_packet_verification["status"].clone();
        entry["replacement_packet_verification_checks"] =
            replacement_packet_verification["checks"].clone();
        entry["replacement_packet_verification_result"] = replacement_packet_verification.clone();
        entry["parity_checklist_progress"]["ao2_queue_packages_project_start_closure"] =
            serde_json::json!(true);
        entry["parity_checklist_progress"]["ao2_queue_verifies_project_start_closure"] =
            serde_json::json!(true);
        entry["parity_checklist_progress"]["ao2_queue_packages_replacement_packet"] =
            serde_json::json!(true);
        entry["parity_checklist_progress"]["ao2_queue_verifies_replacement_packet"] =
            serde_json::json!(true);
        final_entry = Some(entry.clone());
    }
    queue["entries"] = serde_json::json!(entries);
    let queue_path = factory_queue_store(target_root, &mut queue)?;
    let refreshed = factory_queue_load(target_root)?;
    let entry = final_entry.ok_or_else(|| {
        anyhow!(
            "factory queue lost project-start run_id {run_id} while persisting bundle references"
        )
    })?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-workbench-queue-run-next.v1",
        "run_id": run_id,
        "job_kind": "factory_project_start",
        "status": final_status,
        "queue_path": queue_path.display().to_string(),
        "claimed_entry": running["entry"].clone(),
        "entry": entry,
        "project_start": project_start,
        "hermes_queue_handoff_schema": "ao2.hermes-project-start-handoff.v1",
        "continuity_contract": refreshed["continuity_contract"].clone(),
        "parity_checklist_progress": {
            "ao2_queue_executes_project_start_handoff_job": true,
            "ao2_queue_verifies_project_start_handoff_bundle": true,
            "ao2_queue_summarizes_project_start_handoff": true,
            "ao2_queue_packages_project_start_closure": true,
            "ao2_queue_verifies_project_start_closure": true,
            "ao2_queue_packages_replacement_packet": true,
            "ao2_queue_verifies_replacement_packet": true,
            "ao2_persists_queue_history_cancel_retry_state": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}
