use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_core::sha256_hex;
use chrono::{SecondsFormat, Utc};

use crate::cli_util::sanitize_greenfield_id;
use crate::factory_compat::{factory_ensure_target_repo, read_factory_compat_value};

fn atomic_write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    ao2_core::atomic_write(path, content)
        .with_context(|| format!("atomic write {}", path.display()))?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("open {}", path.display()))?;
    Ok(sha256_hex(bytes))
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn factory_queue_path(target: &Path) -> PathBuf {
    target
        .join(".ao2")
        .join("factory-compat")
        .join("queue.json")
}

pub(crate) fn factory_queue_load(target: &Path) -> Result<serde_json::Value> {
    let queue_path = factory_queue_path(target);
    if queue_path.is_file() {
        let mut queue = read_factory_compat_value(&queue_path)
            .with_context(|| format!("read AO2 factory queue {}", queue_path.display()))?;
        if queue["schema_version"] != "ao2.factory-v3-compat-workbench-queue.v1" {
            return Err(anyhow!(
                "factory queue requires ao2.factory-v3-compat-workbench-queue.v1: {}",
                queue_path.display()
            ));
        }
        queue["queue_path"] = serde_json::json!(queue_path.display().to_string());
        Ok(queue)
    } else {
        Ok(serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-workbench-queue.v1",
            "owner": "ao2-workbench-queue",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "queue_path": queue_path.display().to_string(),
            "entry_count": 0,
            "continuity_contract": {
                "survives_server_restart": true,
                "factory_v3_drives_workflow": false,
                "cancel_retry_state_owner": "ao2-workbench-queue",
                "history_owner": "ao2",
                "hermes_role": "front_end_scheduler_queue_and_memory_bookkeeping"
            },
            "entries": []
        }))
    }
}

pub(crate) fn factory_queue_store(target: &Path, queue: &mut serde_json::Value) -> Result<PathBuf> {
    let queue_path = factory_queue_path(target);
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    queue["queue_path"] = serde_json::json!(queue_path.display().to_string());
    queue["entry_count"] = serde_json::json!(entries.len());
    queue["updated_at"] = serde_json::json!(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true));
    atomic_write_text(&queue_path, &serde_json::to_string_pretty(queue)?)?;
    Ok(queue_path)
}

pub(crate) fn factory_queue_list_json(target: &Path) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let queue = factory_queue_load(target)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-workbench-queue-list.v1",
        "owner": "ao2-workbench-queue",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "queue_path": json_string(&queue, "queue_path"),
        "entry_count": queue["entry_count"],
        "continuity_contract": queue["continuity_contract"].clone(),
        "entries": queue["entries"].clone()
    }))
}

pub(crate) fn factory_queue_status_json(target: &Path, run_id: &str) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("--run-id must not be empty"));
    }
    let queue = factory_queue_load(&target_root)?;
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let entry = entries
        .iter()
        .find(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(trimmed))
        .cloned()
        .ok_or_else(|| anyhow!("factory queue does not contain run_id {trimmed}"))?;
    factory_queue_status_detail_json(&queue, entry, trimmed)
}

pub(crate) fn factory_queue_status_latest_completed_project_start_json(
    target: &Path,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let queue = factory_queue_load(&target_root)?;
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let entry = entries
        .iter()
        .rev()
        .find(|entry| {
            entry.get("job_kind").and_then(|value| value.as_str()) == Some("factory_project_start")
                && factory_queue_status_is_terminal(&json_string(entry, "status"))
        })
        .cloned()
        .ok_or_else(|| anyhow!("factory queue has no completed project-start entries"))?;
    let run_id = json_string(&entry, "run_id");
    if run_id.trim().is_empty() {
        return Err(anyhow!(
            "latest completed project-start queue entry is missing run_id"
        ));
    }
    factory_queue_status_detail_json(&queue, entry, &run_id)
}

pub(crate) fn factory_queue_completion_contract_json(
    target: &Path,
    run_id: Option<&str>,
    latest_completed_project_start: bool,
) -> Result<serde_json::Value> {
    if run_id.is_some() && latest_completed_project_start {
        anyhow::bail!("--run-id and --latest-completed-project-start are mutually exclusive");
    }
    let queue_status = if latest_completed_project_start {
        factory_queue_status_latest_completed_project_start_json(target)?
    } else {
        let run_id = run_id.ok_or_else(|| {
            anyhow!(
                "factory queue-completion-contract requires --run-id or --latest-completed-project-start"
            )
        })?;
        factory_queue_status_json(target, run_id)?
    };
    let entry = queue_status["entry"].clone();
    let job_kind = json_string(&entry, "job_kind");
    if job_kind != "factory_project_start" {
        return Err(anyhow!(
            "factory queue-completion-contract requires a factory_project_start entry, got {job_kind}"
        ));
    }
    let closure_checks = entry["project_start_closure_verification_checks"].clone();
    let replacement_packet_checks = entry["replacement_packet_verification_checks"].clone();
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-queue-completion-contract.v1",
        "status": json_string(&queue_status, "status"),
        "run_id": json_string(&queue_status, "run_id"),
        "job_kind": job_kind,
        "source_queue_status": {
            "schema_version": queue_status["schema_version"].clone(),
            "queue_path": queue_status["queue_path"].clone(),
            "status": queue_status["status"].clone(),
            "run_id": queue_status["run_id"].clone()
        },
        "artifacts": {
            "project_start": entry["project_start"].clone(),
            "project_acceptance_review": entry["project_acceptance_review"].clone(),
            "project_acceptance_review_sha256": entry["project_acceptance_review_sha256"].clone(),
            "project_start_bundle": entry["project_start_bundle"].clone(),
            "project_start_bundle_sha256": entry["project_start_bundle_sha256"].clone(),
            "project_start_bundle_verification": entry["project_start_bundle_verification"].clone(),
            "project_start_bundle_verification_sha256": entry["project_start_bundle_verification_sha256"].clone(),
            "project_start_operator_summary": entry["project_start_operator_summary"].clone(),
            "project_start_operator_summary_markdown": entry["project_start_operator_summary_markdown"].clone(),
            "project_start_operator_summary_sha256": entry["project_start_operator_summary_sha256"].clone(),
            "project_start_closure": entry["project_start_closure"].clone(),
            "project_start_closure_sha256": entry["project_start_closure_sha256"].clone(),
            "project_start_closure_json": entry["project_start_closure_json"].clone(),
            "project_start_closure_json_sha256": entry["project_start_closure_json_sha256"].clone(),
            "project_start_closure_verification": entry["project_start_closure_verification"].clone(),
            "project_start_closure_verification_sha256": entry["project_start_closure_verification_sha256"].clone(),
            "replacement_packet": entry["replacement_packet"].clone(),
            "replacement_packet_sha256": entry["replacement_packet_sha256"].clone(),
            "replacement_packet_archive": entry["replacement_packet_archive"].clone(),
            "replacement_packet_archive_sha256": entry["replacement_packet_archive_sha256"].clone(),
            "replacement_packet_verification": entry["replacement_packet_verification"].clone(),
            "replacement_packet_verification_sha256": entry["replacement_packet_verification_sha256"].clone()
        },
        "checks": {
            "project_start_status": entry["project_start_status"].clone(),
            "project_acceptance_review_status": entry["project_acceptance_review_status"].clone(),
            "project_acceptance_review_recommended_decision": entry["project_acceptance_review_recommended_decision"].clone(),
            "project_start_bundle_verification_status": entry["project_start_bundle_verification_status"].clone(),
            "project_start_operator_summary_status": entry["project_start_operator_summary_status"].clone(),
            "project_start_closure_status": entry["project_start_closure_status"].clone(),
            "project_start_closure_verification_status": entry["project_start_closure_verification_status"].clone(),
            "project_start_closure_verification_checksums_verified": closure_checks["checksums_verified"].clone(),
            "project_start_closure_verification_trust_boundary_verified": closure_checks["trust_boundary_verified"].clone(),
            "replacement_packet_status": entry["replacement_packet_status"].clone(),
            "replacement_packet_verification_status": entry["replacement_packet_verification_status"].clone(),
            "replacement_packet_verification_checksums_verified": replacement_packet_checks["checksums_verified"].clone(),
            "replacement_packet_verification_trust_boundary_verified": replacement_packet_checks["trust_boundary_verified"].clone(),
            "replacement_packet_verification_ao2_replacement_driver_verified": replacement_packet_checks["ao2_replacement_driver_verified"].clone(),
            "replacement_packet_verification_factory_v3_evaluator_closer_verified": replacement_packet_checks["factory_v3_evaluator_closer_verified"].clone(),
            "queue_status_detail_is_read_only": queue_status["parity_checklist_progress"]["ao2_queue_status_detail_is_read_only"].clone()
        },
        "hermes_contract": {
            "front_end_reads_single_completion_record": true,
            "requires_manual_closure_commands": false,
            "requires_manual_packet_commands": false,
            "queue_status_digest_checks_enforced": true,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "trust_boundary": queue_status["trust_boundary"].clone(),
        "entry": entry,
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) fn factory_queue_completion_contract_consumption_json(
    contract_path: &Path,
) -> Result<serde_json::Value> {
    let text = fs::read_to_string(contract_path)
        .with_context(|| format!("read {}", contract_path.display()))?;
    let contract: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("parse {}", contract_path.display()))?;

    factory_completion_contract_require_string(
        &contract,
        "schema_version",
        "ao2.factory-project-start-queue-completion-contract.v1",
    )?;
    factory_completion_contract_require_string(&contract, "status", "accepted")?;
    factory_completion_contract_require_string(&contract, "job_kind", "factory_project_start")?;
    factory_completion_contract_require_string(
        &contract["source_queue_status"],
        "schema_version",
        "ao2.factory-queue-status.v1",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "project_start_status",
        "accepted",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "project_acceptance_review_status",
        "accepted",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "project_acceptance_review_recommended_decision",
        "accept",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "project_start_bundle_verification_status",
        "accepted",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "project_start_operator_summary_status",
        "accepted",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "project_start_closure_status",
        "packaged",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "project_start_closure_verification_status",
        "accepted",
    )?;
    factory_completion_contract_require_bool(
        &contract["checks"],
        "project_start_closure_verification_checksums_verified",
        true,
    )?;
    factory_completion_contract_require_bool(
        &contract["checks"],
        "project_start_closure_verification_trust_boundary_verified",
        true,
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "replacement_packet_status",
        "packaged",
    )?;
    factory_completion_contract_require_string(
        &contract["checks"],
        "replacement_packet_verification_status",
        "accepted",
    )?;
    factory_completion_contract_require_bool(
        &contract["checks"],
        "replacement_packet_verification_checksums_verified",
        true,
    )?;
    factory_completion_contract_require_bool(
        &contract["checks"],
        "replacement_packet_verification_trust_boundary_verified",
        true,
    )?;
    factory_completion_contract_require_bool(
        &contract["checks"],
        "replacement_packet_verification_ao2_replacement_driver_verified",
        true,
    )?;
    factory_completion_contract_require_bool(
        &contract["checks"],
        "replacement_packet_verification_factory_v3_evaluator_closer_verified",
        true,
    )?;
    factory_completion_contract_require_bool(
        &contract["checks"],
        "queue_status_detail_is_read_only",
        true,
    )?;
    factory_completion_contract_require_bool(
        &contract["hermes_contract"],
        "front_end_reads_single_completion_record",
        true,
    )?;
    factory_completion_contract_require_bool(
        &contract["hermes_contract"],
        "requires_manual_closure_commands",
        false,
    )?;
    factory_completion_contract_require_bool(
        &contract["hermes_contract"],
        "requires_manual_packet_commands",
        false,
    )?;
    factory_completion_contract_require_bool(
        &contract["hermes_contract"],
        "queue_status_digest_checks_enforced",
        true,
    )?;
    factory_completion_contract_require_string(
        &contract["trust_boundary"],
        "release_acceptance_owner",
        "factory-v3 evaluator-closer",
    )?;
    factory_completion_contract_require_bool(
        &contract["trust_boundary"],
        "control_plane_approves_release",
        false,
    )?;
    factory_completion_contract_require_bool(
        &contract["trust_boundary"],
        "mutates_ao_artifacts",
        false,
    )?;

    for field in [
        "project_start_bundle",
        "project_start_bundle_sha256",
        "project_start_bundle_verification",
        "project_start_bundle_verification_sha256",
        "project_start_operator_summary",
        "project_start_operator_summary_sha256",
        "project_start_closure",
        "project_start_closure_sha256",
        "project_start_closure_json",
        "project_start_closure_json_sha256",
        "project_start_closure_verification",
        "project_start_closure_verification_sha256",
        "replacement_packet",
        "replacement_packet_sha256",
        "replacement_packet_archive",
        "replacement_packet_archive_sha256",
        "replacement_packet_verification",
        "replacement_packet_verification_sha256",
    ] {
        if json_string(&contract["artifacts"], field).trim().is_empty() {
            anyhow::bail!("{field} must be present in completion contract artifacts");
        }
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-queue-completion-contract-consumption.v1",
        "status": "accepted",
        "ready_for_operator_review": true,
        "run_id": json_string(&contract, "run_id"),
        "source_contract": contract_path.display().to_string(),
        "source_contract_schema": json_string(&contract, "schema_version"),
        "source_queue_status": contract["source_queue_status"].clone(),
        "artifacts": contract["artifacts"].clone(),
        "checks": {
            "project_start_status": contract["checks"]["project_start_status"].clone(),
            "project_acceptance_review_status": contract["checks"]["project_acceptance_review_status"].clone(),
            "project_acceptance_review_recommended_decision": contract["checks"]["project_acceptance_review_recommended_decision"].clone(),
            "project_start_bundle_verification_status": contract["checks"]["project_start_bundle_verification_status"].clone(),
            "project_start_operator_summary_status": contract["checks"]["project_start_operator_summary_status"].clone(),
            "project_start_closure_status": contract["checks"]["project_start_closure_status"].clone(),
            "project_start_closure_verification_status": contract["checks"]["project_start_closure_verification_status"].clone(),
            "project_start_closure_verification_checksums_verified": contract["checks"]["project_start_closure_verification_checksums_verified"].clone(),
            "project_start_closure_verification_trust_boundary_verified": contract["checks"]["project_start_closure_verification_trust_boundary_verified"].clone(),
            "replacement_packet_status": contract["checks"]["replacement_packet_status"].clone(),
            "replacement_packet_verification_status": contract["checks"]["replacement_packet_verification_status"].clone(),
            "replacement_packet_verification_checksums_verified": contract["checks"]["replacement_packet_verification_checksums_verified"].clone(),
            "replacement_packet_verification_trust_boundary_verified": contract["checks"]["replacement_packet_verification_trust_boundary_verified"].clone(),
            "replacement_packet_verification_ao2_replacement_driver_verified": contract["checks"]["replacement_packet_verification_ao2_replacement_driver_verified"].clone(),
            "replacement_packet_verification_factory_v3_evaluator_closer_verified": contract["checks"]["replacement_packet_verification_factory_v3_evaluator_closer_verified"].clone(),
            "queue_status_detail_is_read_only": contract["checks"]["queue_status_detail_is_read_only"].clone()
        },
        "hermes_contract": {
            "front_end_reads_single_completion_record": true,
            "consumed_contract_only": true,
            "requires_manual_closure_commands": false,
            "requires_manual_packet_commands": false,
            "queue_status_digest_checks_enforced_by_source_contract": true
        },
        "trust_boundary": contract["trust_boundary"].clone(),
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

fn factory_completion_contract_require_string(
    value: &serde_json::Value,
    field: &str,
    expected: &str,
) -> Result<()> {
    let actual = json_string(value, field);
    if actual != expected {
        anyhow::bail!("{field} must be {expected}, got {actual}");
    }
    Ok(())
}

fn factory_completion_contract_require_bool(
    value: &serde_json::Value,
    field: &str,
    expected: bool,
) -> Result<()> {
    let actual = value.get(field).and_then(serde_json::Value::as_bool);
    if actual != Some(expected) {
        anyhow::bail!("{field} must be {expected}");
    }
    Ok(())
}

pub(crate) fn factory_queue_status_detail_json(
    queue: &serde_json::Value,
    entry: serde_json::Value,
    run_id: &str,
) -> Result<serde_json::Value> {
    factory_queue_status_detail_json_with_options(queue, entry, run_id, true)
}

pub(crate) fn factory_queue_status_detail_json_with_options(
    queue: &serde_json::Value,
    entry: serde_json::Value,
    run_id: &str,
    verify_project_start_closure_sidecars: bool,
) -> Result<serde_json::Value> {
    let queue_path = json_string(queue, "queue_path");
    let status = json_string(&entry, "status");
    if !factory_queue_status_is_terminal(&status) {
        return Err(anyhow!(
            "factory queue entry {run_id} is not completed yet: status={status}"
        ));
    }

    let job_kind = json_string(&entry, "job_kind");
    if job_kind == "factory_project_start" {
        let summary_path = PathBuf::from(json_string(&entry, "project_start_operator_summary"));
        if !summary_path.is_file() {
            return Err(anyhow!(
                "factory queue entry {run_id} references missing project_start_operator_summary {}",
                summary_path.display()
            ));
        }
        let expected_summary_sha = json_string(&entry, "project_start_operator_summary_sha256");
        let actual_summary_sha = sha256_file(&summary_path).with_context(|| {
            format!(
                "hash project_start_operator_summary for queue entry {run_id}: {}",
                summary_path.display()
            )
        })?;
        if expected_summary_sha.trim().is_empty() || expected_summary_sha != actual_summary_sha {
            return Err(anyhow!(
                "factory queue entry {run_id} project_start_operator_summary digest mismatch: expected {expected_summary_sha}, got {actual_summary_sha}"
            ));
        }
        if verify_project_start_closure_sidecars {
            factory_queue_verify_entry_file_digest(
                &entry,
                run_id,
                "project_start_closure",
                "project_start_closure_sha256",
            )?;
            factory_queue_verify_entry_file_digest(
                &entry,
                run_id,
                "project_start_closure_json",
                "project_start_closure_json_sha256",
            )?;
            factory_queue_verify_entry_file_digest(
                &entry,
                run_id,
                "project_start_closure_verification",
                "project_start_closure_verification_sha256",
            )?;
            let closure_status = json_string(&entry, "project_start_closure_status");
            if closure_status != "packaged" {
                return Err(anyhow!(
                    "factory queue entry {run_id} project_start_closure_status must be packaged, got {closure_status}"
                ));
            }
            let closure_verification_status =
                json_string(&entry, "project_start_closure_verification_status");
            if closure_verification_status != "accepted" {
                return Err(anyhow!(
                    "factory queue entry {run_id} project_start_closure_verification_status must be accepted, got {closure_verification_status}"
                ));
            }
        }
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-queue-status.v1",
        "status": status,
        "run_id": run_id,
        "job_kind": job_kind,
        "queue_path": queue_path,
        "entry": entry,
        "continuity_contract": queue["continuity_contract"].clone(),
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "parity_checklist_progress": {
            "ao2_queue_status_detail_is_read_only": true,
            "ao2_persists_queue_history_cancel_retry_state": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) fn factory_queue_verify_entry_file_digest(
    entry: &serde_json::Value,
    run_id: &str,
    path_field: &str,
    sha_field: &str,
) -> Result<()> {
    let path_value = json_string(entry, path_field);
    if path_value.trim().is_empty() {
        return Err(anyhow!(
            "factory queue entry {run_id} is missing {path_field}"
        ));
    }
    let path = PathBuf::from(&path_value);
    if !path.is_file() {
        return Err(anyhow!(
            "factory queue entry {run_id} references missing {path_field} {}",
            path.display()
        ));
    }
    let expected_sha = json_string(entry, sha_field);
    let actual_sha = sha256_file(&path).with_context(|| {
        format!(
            "hash {path_field} for queue entry {run_id}: {}",
            path.display()
        )
    })?;
    if expected_sha.trim().is_empty() || expected_sha != actual_sha {
        return Err(anyhow!(
            "factory queue entry {run_id} {path_field} digest mismatch: expected {expected_sha}, got {actual_sha}"
        ));
    }
    Ok(())
}

fn factory_project_start_completion_summary_artifact(
    entry: &serde_json::Value,
    path_key: &str,
    sha_key: Option<&str>,
    status_key: Option<&str>,
) -> Result<serde_json::Value> {
    let path_value = json_string(entry, path_key);
    if path_value.trim().is_empty() {
        anyhow::bail!("completed project-start entry missing artifact path {path_key}");
    }
    let path = PathBuf::from(&path_value);
    if !path.is_file() {
        anyhow::bail!(
            "completed project-start artifact {path_key} missing at {}",
            path.display()
        );
    }
    let sha256 = sha256_file(&path)?;
    let expected_sha256 = sha_key
        .map(|key| json_string(entry, key))
        .filter(|value| !value.trim().is_empty());
    let digest_matches_expected = expected_sha256
        .as_ref()
        .map(|expected| expected == &sha256)
        .unwrap_or(true);
    if !digest_matches_expected {
        anyhow::bail!(
            "completed project-start artifact {path_key} digest mismatch: expected {}, got {sha256}",
            expected_sha256.unwrap_or_default()
        );
    }
    let status = status_key
        .map(|key| json_string(entry, key))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "present".to_string());
    Ok(serde_json::json!({
        "path": path.display().to_string(),
        "sha256": sha256,
        "expected_sha256": expected_sha256,
        "digest_matches_expected": digest_matches_expected,
        "exists": true,
        "status": status
    }))
}

fn factory_project_start_completion_summary_optional_artifact(
    entry: &serde_json::Value,
    path_key: &str,
    sha_key: Option<&str>,
    status_key: Option<&str>,
) -> Result<serde_json::Value> {
    let path_value = json_string(entry, path_key);
    if path_value.trim().is_empty() {
        return Ok(serde_json::json!({
            "path": serde_json::Value::Null,
            "sha256": serde_json::Value::Null,
            "expected_sha256": serde_json::Value::Null,
            "digest_matches_expected": false,
            "exists": false,
            "status": "missing"
        }));
    }
    factory_project_start_completion_summary_artifact(entry, path_key, sha_key, status_key)
}

pub(crate) fn factory_queue_project_start_completion_summary_json(
    target: &Path,
    run_id: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let run_id = sanitize_greenfield_id(run_id);
    if run_id.trim().is_empty() {
        anyhow::bail!("--run-id must not be empty");
    }
    let queue = factory_queue_load(&target_root)?;
    let queue_path = factory_queue_path(&target_root);
    let queue_sha256 = sha256_file(&queue_path)?;
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let entry = entries
        .iter()
        .find(|entry| json_string(entry, "run_id") == run_id)
        .cloned()
        .ok_or_else(|| anyhow!("factory queue has no project-start entry for run_id {run_id}"))?;
    if json_string(&entry, "job_kind") != "factory_project_start" {
        anyhow::bail!(
            "factory queue entry {run_id} is {}, not factory_project_start",
            json_string(&entry, "job_kind")
        );
    }
    if json_string(&entry, "status") != "accepted" {
        anyhow::bail!(
            "factory project-start completion summary requires accepted entry; run_id {run_id} status is {}",
            json_string(&entry, "status")
        );
    }

    let artifacts = serde_json::json!({
        "project_start": factory_project_start_completion_summary_artifact(
            &entry,
            "project_start",
            None,
            Some("project_start_status"),
        )?,
        "project_acceptance_review": factory_project_start_completion_summary_artifact(
            &entry,
            "project_acceptance_review",
            Some("project_acceptance_review_sha256"),
            Some("project_acceptance_review_status"),
        )?,
        "project_start_bundle": factory_project_start_completion_summary_artifact(
            &entry,
            "project_start_bundle",
            Some("project_start_bundle_sha256"),
            Some("project_start_status"),
        )?,
        "project_start_bundle_verification": factory_project_start_completion_summary_artifact(
            &entry,
            "project_start_bundle_verification",
            Some("project_start_bundle_verification_sha256"),
            Some("project_start_bundle_verification_status"),
        )?,
        "project_start_operator_summary": factory_project_start_completion_summary_artifact(
            &entry,
            "project_start_operator_summary",
            Some("project_start_operator_summary_sha256"),
            Some("project_start_operator_summary_status"),
        )?,
        "project_start_closure": factory_project_start_completion_summary_artifact(
            &entry,
            "project_start_closure",
            Some("project_start_closure_sha256"),
            Some("project_start_closure_status"),
        )?,
        "project_start_closure_verification": factory_project_start_completion_summary_artifact(
            &entry,
            "project_start_closure_verification",
            Some("project_start_closure_verification_sha256"),
            Some("project_start_closure_verification_status"),
        )?,
        "replacement_packet": factory_project_start_completion_summary_optional_artifact(
            &entry,
            "replacement_packet",
            Some("replacement_packet_sha256"),
            Some("replacement_packet_status"),
        )?,
        "replacement_packet_archive": factory_project_start_completion_summary_optional_artifact(
            &entry,
            "replacement_packet_archive",
            Some("replacement_packet_archive_sha256"),
            Some("replacement_packet_status"),
        )?,
        "replacement_packet_verification": factory_project_start_completion_summary_optional_artifact(
            &entry,
            "replacement_packet_verification",
            Some("replacement_packet_verification_sha256"),
            Some("replacement_packet_verification_status"),
        )?
    });
    let replacement_packet_checks = entry["replacement_packet_verification_checks"].clone();
    let replacement_packet_ready = artifacts["replacement_packet"]["exists"].as_bool()
        == Some(true)
        && artifacts["replacement_packet_archive"]["exists"].as_bool() == Some(true)
        && artifacts["replacement_packet_verification"]["exists"].as_bool() == Some(true)
        && json_string(&entry, "replacement_packet_status") == "packaged"
        && json_string(&entry, "replacement_packet_verification_status") == "accepted"
        && replacement_packet_checks["checksums_verified"].as_bool() == Some(true)
        && replacement_packet_checks["trust_boundary_verified"].as_bool() == Some(true)
        && replacement_packet_checks["ao2_replacement_driver_verified"].as_bool() == Some(true)
        && replacement_packet_checks["factory_v3_evaluator_closer_verified"].as_bool()
            == Some(true);
    let replacement_packet_handoff_status = if replacement_packet_ready {
        "ready_for_operator_review"
    } else {
        "missing_or_unverified"
    };
    let replacement_packet_next_action = if replacement_packet_ready {
        "record_replacement_packet_completion_summary"
    } else {
        "package_and_verify_replacement_packet"
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-completion-summary.v1",
        "status": "accepted",
        "run_id": run_id,
        "job_kind": "factory_project_start",
        "read_only": true,
        "queue": {
            "path": queue_path.display().to_string(),
            "sha256": queue_sha256,
            "status": entry["status"].clone(),
            "updated_at": entry["updated_at"].clone()
        },
        "artifacts": artifacts,
        "checks": {
            "project_acceptance_review_status": entry["project_acceptance_review_status"].clone(),
            "project_acceptance_review_recommended_decision": entry["project_acceptance_review_recommended_decision"].clone(),
            "project_start_bundle_verification_status": entry["project_start_bundle_verification_status"].clone(),
            "project_start_operator_summary_status": entry["project_start_operator_summary_status"].clone(),
            "project_start_closure_status": entry["project_start_closure_status"].clone(),
            "project_start_closure_verification_status": entry["project_start_closure_verification_status"].clone(),
            "replacement_packet_status": entry["replacement_packet_status"].clone(),
            "replacement_packet_verification_status": entry["replacement_packet_verification_status"].clone(),
            "replacement_packet_verification_checks": replacement_packet_checks.clone()
        },
        "replacement_packet_handoff": {
            "schema_version": "ao2.factory-replacement-packet-handoff-summary.v1",
            "status": replacement_packet_handoff_status,
            "run_id": run_id,
            "single_operator_handoff": replacement_packet_ready,
            "requires_manual_packet_command": false,
            "requires_manual_packet_verify_command": false,
            "packet": artifacts["replacement_packet"]["path"].clone(),
            "packet_sha256": artifacts["replacement_packet"]["sha256"].clone(),
            "archive": artifacts["replacement_packet_archive"]["path"].clone(),
            "archive_sha256": artifacts["replacement_packet_archive"]["sha256"].clone(),
            "verification": artifacts["replacement_packet_verification"]["path"].clone(),
            "verification_sha256": artifacts["replacement_packet_verification"]["sha256"].clone(),
            "checksums_verified": replacement_packet_checks["checksums_verified"].as_bool().unwrap_or(false),
            "trust_boundary_verified": replacement_packet_checks["trust_boundary_verified"].as_bool().unwrap_or(false),
            "ao2_replaces_factory_v3_workflow_driver": replacement_packet_checks["ao2_replacement_driver_verified"].as_bool().unwrap_or(false),
            "factory_v3_evaluator_closer_verified": replacement_packet_checks["factory_v3_evaluator_closer_verified"].as_bool().unwrap_or(false),
            "factory_v3_role": "evaluator_closer_and_sampling_auditor",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "next_recommended_action": replacement_packet_next_action
        },
        "hermes_memory": {
            "single_record_for_bookkeeping": true,
            "summary_is_compact": true,
            "raw_queue_json_scrape_required": false,
            "full_run_next_receipt_required": false,
            "replacement_packet_single_operator_handoff": replacement_packet_ready,
            "next_recommended_action": replacement_packet_next_action
        },
        "side_effects": {
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_rebuild_wrappers": false,
            "would_mutate_control_plane": false,
            "would_execute_provider": false,
            "would_write_files": false
        },
        "trust_boundary": {
            "hermes_role": "front_end_queue_cron_memory_bookkeeping",
            "ao2_role": "trusted_execution_queue_memory_replay_signed_evidence_producer",
            "execution_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) fn factory_project_start_completion_summary_memory_trust_boundary() -> serde_json::Value
{
    serde_json::json!({
        "hermes_role": "front_end_queue_cron_memory_bookkeeping",
        "ao2_role": "trusted_execution_queue_memory_replay_signed_evidence_producer",
        "execution_owner": "ao2",
        "factory_v3_role": "parity_oracle_only",
        "factory_v3_drives_workflow": false,
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    })
}

pub(crate) fn factory_queue_status_is_terminal(status: &str) -> bool {
    matches!(
        status,
        "accepted"
            | "accepted_with_concerns"
            | "rejected"
            | "blocked"
            | "failed"
            | "completed"
            | "cancelled"
    )
}

const FACTORY_CANCEL_AUTHORITY_QUEUE_LIST_SCHEMA: &str =
    "ao2.factory-v3-compat-workbench-queue-list.v1";
const FACTORY_CANCEL_AUTHORITY_ATTESTATION_SCHEMA: &str =
    "factory-v3/ao2-watchdog-no-active-ao2-runs-attestation/v1";
const FACTORY_CANCEL_AUTHORITY_FACTORY_V3_ROLE: &str = "parity_oracle_only";
const FACTORY_CANCEL_AUTHORITY_ACTIVE_STATUSES: &[&str] =
    &["queued", "running", "cancel_requested"];
const FACTORY_CANCEL_AUTHORITY_DEFAULT_REASON: &str = "AO2 factory queue-list snapshot reports no active entries; the overdue Hermes one-shot has no in-flight AO2 run to cancel";

pub(crate) fn factory_cancel_authority_json(
    queue_list_json: &Path,
    reason: Option<&str>,
    produced_at_ms: Option<i64>,
) -> Result<serde_json::Value> {
    let text = fs::read_to_string(queue_list_json).with_context(|| {
        format!(
            "--queue-list-json input unreadable: {}",
            queue_list_json.display()
        )
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "--queue-list-json input is not valid JSON: {}",
            queue_list_json.display()
        )
    })?;
    let queue = parsed.as_object().ok_or_else(|| {
        anyhow!(
            "--queue-list-json input did not parse to a JSON object: {}",
            queue_list_json.display()
        )
    })?;
    let schema_str = queue
        .get("schema_version")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if schema_str != FACTORY_CANCEL_AUTHORITY_QUEUE_LIST_SCHEMA {
        return Err(anyhow!(
            "queue-list schema_version must be {:?}; got {:?} in {}",
            FACTORY_CANCEL_AUTHORITY_QUEUE_LIST_SCHEMA,
            schema_str,
            queue_list_json.display()
        ));
    }
    build_factory_cancel_authority_attestation(queue, reason, produced_at_ms)
}

fn build_factory_cancel_authority_attestation(
    queue_list: &serde_json::Map<String, serde_json::Value>,
    reason: Option<&str>,
    produced_at_ms: Option<i64>,
) -> Result<serde_json::Value> {
    let empty: Vec<serde_json::Value> = Vec::new();
    let entries_raw = queue_list
        .get("entries")
        .and_then(|value| value.as_array())
        .unwrap_or(&empty);

    let mut active: Vec<(String, String)> = Vec::new();
    let mut status_counts: std::collections::BTreeMap<String, i64> =
        std::collections::BTreeMap::new();
    let mut seen_dict_entries: usize = 0;
    for entry in entries_raw {
        let Some(entry_obj) = entry.as_object() else {
            continue;
        };
        seen_dict_entries += 1;
        let raw_status = entry_obj
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let stripped = raw_status.trim();
        let status_label = if stripped.is_empty() {
            "unknown".to_string()
        } else {
            stripped.to_string()
        };
        *status_counts.entry(status_label.clone()).or_insert(0) += 1;
        if FACTORY_CANCEL_AUTHORITY_ACTIVE_STATUSES.contains(&status_label.as_str()) {
            let run_id = entry_obj
                .get("run_id")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            active.push((run_id.to_string(), status_label));
        }
    }
    if !active.is_empty() {
        let formatted: Vec<String> = active
            .into_iter()
            .map(|(run_id, status)| format!("{run_id}={status}"))
            .collect();
        return Err(anyhow!(
            "queue-list shows active AO2 queue entries; cannot emit no-active-ao2-runs attestation: {}",
            formatted.join(", ")
        ));
    }

    let entry_count_value = queue_list
        .get("entry_count")
        .and_then(|value| value.as_i64())
        .unwrap_or(seen_dict_entries as i64);
    let queue_path_value = queue_list
        .get("queue_path")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let timestamp = produced_at_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    let reason_value = reason
        .map(str::to_owned)
        .unwrap_or_else(|| FACTORY_CANCEL_AUTHORITY_DEFAULT_REASON.to_string());

    let mut status_counts_map = serde_json::Map::new();
    for (key, value) in status_counts {
        status_counts_map.insert(key, serde_json::json!(value));
    }

    Ok(serde_json::json!({
        "schema": FACTORY_CANCEL_AUTHORITY_ATTESTATION_SCHEMA,
        "factory_v3_role": FACTORY_CANCEL_AUTHORITY_FACTORY_V3_ROLE,
        "no_active_ao2_runs": true,
        "reason": reason_value,
        "produced_at_ms": timestamp,
        "source": {
            "schema_version": FACTORY_CANCEL_AUTHORITY_QUEUE_LIST_SCHEMA,
            "queue_path": queue_path_value,
            "entry_count": entry_count_value,
            "active_entry_count": 0,
            "status_counts": serde_json::Value::Object(status_counts_map),
        }
    }))
}

const FACTORY_CANCEL_TRANSITION_SCHEMA: &str =
    "ao2.factory-v3-compat-workbench-queue-transition.v1";
const FACTORY_CANCEL_TRANSITION_AO2_DECISION_OWNER: &str = "ao2-workbench-queue";

pub(crate) fn factory_cancel_transition_json(
    queue_list_json: &Path,
    run_id: &str,
    terminated_pid: i64,
    reason: Option<&str>,
    produced_at_ms: Option<i64>,
) -> Result<serde_json::Value> {
    if terminated_pid <= 0 {
        return Err(anyhow!(
            "--terminated-pid must be a positive integer; got {terminated_pid}"
        ));
    }
    if run_id.trim().is_empty() {
        return Err(anyhow!("--run-id must be non-empty"));
    }

    let text = fs::read_to_string(queue_list_json).with_context(|| {
        format!(
            "--queue-list-json input unreadable: {}",
            queue_list_json.display()
        )
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "--queue-list-json input is not valid JSON: {}",
            queue_list_json.display()
        )
    })?;
    let queue = parsed.as_object().ok_or_else(|| {
        anyhow!(
            "--queue-list-json input did not parse to a JSON object: {}",
            queue_list_json.display()
        )
    })?;
    let schema_str = queue
        .get("schema_version")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if schema_str != FACTORY_CANCEL_AUTHORITY_QUEUE_LIST_SCHEMA {
        return Err(anyhow!(
            "queue-list schema_version must be {:?}; got {:?} in {}",
            FACTORY_CANCEL_AUTHORITY_QUEUE_LIST_SCHEMA,
            schema_str,
            queue_list_json.display()
        ));
    }
    let queue_path_value = queue
        .get("queue_path")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .ok_or_else(|| anyhow!("queue-list entries must be an array"))?;
    let matching_entry = entries
        .iter()
        .find(|entry| {
            entry
                .as_object()
                .and_then(|object| object.get("run_id"))
                .and_then(|value| value.as_str())
                == Some(run_id)
        })
        .ok_or_else(|| {
            anyhow!(
                "queue-list does not contain an entry with run_id {:?}",
                run_id
            )
        })?;
    let entry_object = matching_entry
        .as_object()
        .ok_or_else(|| anyhow!("matching queue entry is not a JSON object"))?;
    let status = entry_object
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if status != "cancelled" {
        return Err(anyhow!(
            "matching queue entry for run_id {:?} must have status 'cancelled'; got {:?}",
            run_id,
            status
        ));
    }
    if !entry_covers_pid(entry_object, terminated_pid) {
        return Err(anyhow!(
            "matching queue entry for run_id {:?} does not record terminated_pid={} (checked entry.terminated_pid, entry.killed_pid, entry.pid, and entry.transition_history[].terminated_pid)",
            run_id,
            terminated_pid
        ));
    }

    let history = entry_object
        .get("transition_history")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let timestamp = produced_at_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let reason_value = reason.map(str::to_owned).unwrap_or_else(|| {
        format!(
            "AO2-native cancel-transition for run {} bound to terminated_pid {}",
            run_id, terminated_pid
        )
    });

    Ok(serde_json::json!({
        "schema_version": FACTORY_CANCEL_TRANSITION_SCHEMA,
        "factory_v3_role": FACTORY_CANCEL_AUTHORITY_FACTORY_V3_ROLE,
        "ao2_decision_owner": FACTORY_CANCEL_TRANSITION_AO2_DECISION_OWNER,
        "produced_at_ms": timestamp,
        "source": {
            "schema_version": FACTORY_CANCEL_AUTHORITY_QUEUE_LIST_SCHEMA,
            "queue_path": queue_path_value,
            "run_id": run_id,
            "terminated_pid": terminated_pid,
            "reason": reason_value,
        },
        "entry": {
            "status": "cancelled",
            "terminated_pid": terminated_pid,
            "run_id": run_id,
            "transition_history": history,
        },
    }))
}

fn entry_covers_pid(entry: &serde_json::Map<String, serde_json::Value>, pid: i64) -> bool {
    for key in ["terminated_pid", "killed_pid", "pid"] {
        if entry.get(key).and_then(|value| value.as_i64()) == Some(pid) {
            return true;
        }
    }
    if let Some(records) = entry
        .get("transition_history")
        .and_then(|value| value.as_array())
    {
        for record in records {
            let Some(record_obj) = record.as_object() else {
                continue;
            };
            for key in ["terminated_pid", "killed_pid", "pid"] {
                if record_obj.get(key).and_then(|value| value.as_i64()) == Some(pid) {
                    return true;
                }
            }
        }
    }
    false
}
pub(crate) fn factory_queue_submit_json(
    target: &Path,
    plan_path: &Path,
    run_id: Option<String>,
    receipt_out: Option<&Path>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let plan = read_factory_compat_value(plan_path)?;
    if plan["schema_version"] != "ao2.factory-v3-compat-governed-plan.v1" {
        return Err(anyhow!(
            "factory queue-submit requires ao2.factory-v3-compat-governed-plan.v1 plan: {}",
            plan_path.display()
        ));
    }
    if plan["parity_checklist_progress"]["factory_v3_drives_workflow"] != false
        || plan["ao2_native_plan"]["runnable_workflow"]["factory_v3_drives_workflow"] != false
    {
        return Err(anyhow!(
            "refusing to queue factory compat plan unless AO2 owns execution and factory_v3_drives_workflow=false"
        ));
    }
    let run_id =
        run_id.unwrap_or_else(|| format!("factory-compat-{}", Utc::now().format("%Y%m%d%H%M%S")));
    let mut queue = factory_queue_load(target)?;
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
    let workflow_path = json_string(&plan["ao2_native_plan"]["runnable_workflow"], "path");
    let canonical_plan_path = fs::canonicalize(plan_path)
        .with_context(|| format!("canonicalize queued plan {}", plan_path.display()))?;
    let entry = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-workbench-queue-entry.v1",
        "run_id": run_id,
        "status": "queued",
        "attempts": 0,
        "created_at": now,
        "updated_at": now,
        "plan_path": canonical_plan_path.display().to_string(),
        "plan_sha256": sha256_file(&canonical_plan_path)?,
        "workflow_path": workflow_path,
        "classification": plan["classification"].clone(),
        "parity_checklist_progress": {
            "ao2_persists_queue_history_cancel_retry_state": true,
            "factory_v3_drives_workflow": false,
            "ao2_queue_owner": "ao2-workbench-queue"
        },
        "execution_contract": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "transition_history": [{
            "at": now,
            "status": "queued",
            "reason": "submitted factory-v3-compatible governed plan to AO2-native persisted queue"
        }]
    });
    entries.push(entry.clone());
    entries.sort_by(|left, right| {
        left.get("created_at")
            .and_then(|value| value.as_str())
            .cmp(&right.get("created_at").and_then(|value| value.as_str()))
    });
    queue["entries"] = serde_json::json!(entries);
    let queue_path = factory_queue_store(target, &mut queue)?;
    let result = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-workbench-queue-submit.v1",
        "status": "queued",
        "run_id": json_string(&entry, "run_id"),
        "queue_path": queue_path.display().to_string(),
        "entry": entry,
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-workbench-queue"
    });
    if let Some(out) = receipt_out {
        atomic_write_text(out, &serde_json::to_string_pretty(&result)?)?;
    }
    Ok(result)
}
