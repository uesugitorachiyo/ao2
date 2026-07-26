use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::cli_util::{atomic_write_text, json_string, now_unix_ms, sha256_bytes_hex, sha256_file};
use crate::factory_compat::{factory_ensure_target_repo, read_factory_compat_value};
use crate::factory_queue_recovery::factory_queue_project_start_complete_status_json;
use crate::workbench_support_latest::latest_workbench_support_packet_json;

fn factory_project_start_next_action_required_blocker_codes() -> &'static [&'static str] {
    &[
        "missing_queue_file",
        "missing_queue_entry",
        "queue_entry_status_queued",
        "queue_entry_status_running",
        "queue_entry_status_rejected",
        "queue_entry_status_missing",
        "wrong_job_kind",
        "missing_compact_artifact_queue_submit",
        "missing_compact_artifact_queue_run_next",
        "missing_compact_artifact_completion_contract",
        "missing_compact_artifact_completion_contract_consumer",
        "artifact_run_id_mismatch_queue_submit",
        "artifact_run_id_mismatch_queue_run_next",
        "artifact_run_id_mismatch_completion_contract",
        "artifact_status_mismatch_completion_contract",
        "artifact_status_mismatch_completion_contract_consumer",
        "trust_boundary_mismatch_completion_contract_consumer",
    ]
}

pub(crate) fn factory_queue_project_start_next_action_json(
    target: &Path,
    run_id: &str,
    out_dir: &Path,
    contract_path: &Path,
) -> Result<serde_json::Value> {
    let status_probe = factory_queue_project_start_complete_status_json(target, run_id, out_dir)?;
    let contract = read_factory_compat_value(contract_path).with_context(|| {
        format!(
            "read Hermes project-start contract {}",
            contract_path.display()
        )
    })?;
    if contract["schema_version"] != "ao2.hermes-project-start-poll-act-contract.v1" {
        anyhow::bail!(
            "Hermes project-start contract requires ao2.hermes-project-start-poll-act-contract.v1: {}",
            contract_path.display()
        );
    }
    if contract["trust_boundary"]["release_acceptance_owner"] != "factory-v3 evaluator-closer"
        || contract["trust_boundary"]["control_plane_approves_release"] != false
        || contract["trust_boundary"]["mutates_ao_artifacts"] != false
    {
        anyhow::bail!("Hermes project-start contract trust boundary mismatch");
    }

    let mut decisions = BTreeMap::<String, String>::new();
    for row in contract
        .get("decision_table")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
    {
        if let (Some(code), Some(decision)) = (
            row.get("blocker_code").and_then(|value| value.as_str()),
            row.get("decision").and_then(|value| value.as_str()),
        ) {
            decisions.insert(code.to_string(), decision.to_string());
        }
    }
    for required in factory_project_start_next_action_required_blocker_codes() {
        if !decisions.contains_key(*required) {
            anyhow::bail!(
                "Hermes project-start contract omits blocker_code {required}: {}",
                contract_path.display()
            );
        }
    }

    let blocker_codes = status_probe
        .get("blocker_codes")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut mapped_decisions = Vec::<String>::new();
    for code_value in &blocker_codes {
        let code = code_value
            .as_str()
            .ok_or_else(|| anyhow!("status probe blocker_codes must contain only strings"))?;
        let decision = decisions.get(code).ok_or_else(|| {
            anyhow!(
                "Hermes project-start contract has no decision for observed blocker_code {code}"
            )
        })?;
        mapped_decisions.push(decision.clone());
    }

    let next_action = if json_string(&status_probe, "status") == "accepted"
        && json_string(&status_probe, "completion_record_state") == "complete"
        && status_probe["ready_for_operator_review"]
            .as_bool()
            .unwrap_or(false)
        && blocker_codes.is_empty()
    {
        "publish_operator_record".to_string()
    } else if mapped_decisions
        .iter()
        .any(|decision| decision == "operator_review_required")
    {
        "operator_review_required".to_string()
    } else if mapped_decisions
        .iter()
        .any(|decision| decision == "wait_and_poll")
    {
        "wait_and_poll".to_string()
    } else if mapped_decisions
        .iter()
        .any(|decision| decision == "call_queue_project_start_complete")
    {
        "call_queue_project_start_complete".to_string()
    } else {
        "operator_review_required".to_string()
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-next-action.v1",
        "status": if next_action == "publish_operator_record" { "ready" } else { "action_required" },
        "run_id": status_probe["run_id"].clone(),
        "next_action": next_action,
        "contract_path": contract_path.display().to_string(),
        "read_only": true,
        "would_execute_queue": false,
        "would_submit_queue_entry": false,
        "would_rebuild_wrappers": false,
        "status_probe": status_probe,
        "contract": {
            "schema_version": contract["schema_version"].clone(),
            "decision_table_path": contract_path.display().to_string(),
            "known_blocker_codes_covered": true
        },
        "hermes_contract": {
            "front_end_can_preview_next_action_without_backend_execution": true,
            "front_end_must_call_ao2_backend_for_mutating_action": true,
            "front_end_must_not_scrape_raw_queue_json": true,
            "front_end_must_not_rebuild_wrappers": true
        },
        "trust_boundary": {
            "hermes_role": "front_end_queue_cron_memory_bookkeeping",
            "ao2_role": "trusted_execution_queue_memory_replay_signed_evidence_producer",
            "factory_v3_role": "evaluator-closer / parity oracle",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

fn factory_project_start_operator_record_artifact(path: &Path) -> Result<serde_json::Value> {
    let body =
        read_factory_compat_value(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::json!({
        "path": path.display().to_string(),
        "sha256": sha256_file(path)?,
        "schema_version": body["schema_version"].clone(),
        "status": body["status"].clone()
    }))
}

fn factory_project_start_hermes_flow_contract_payload(target: &Path) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.hermes-project-start-flow-contract.v1",
        "status": "ready",
        "target": target.display().to_string(),
        "flow": "project-start-next-action-to-operator-record",
        "workflow": {
            "preview": {
                "method": "GET",
                "endpoint": "/api/factory/project-start/next-action",
                "minimum_role": "viewer",
                "query_fields": ["token", "run_id", "out_dir", "contract"],
                "allowed_next_actions": [
                    "call_queue_project_start_complete",
                    "wait_and_poll",
                    "operator_review_required",
                    "publish_operator_record"
                ],
                "success_status": "ready",
                "fail_closed": true
            },
            "publish": {
                "method": "POST",
                "endpoint": "/api/factory/project-start/operator-record",
                "minimum_role": "operator",
                "content_type": "application/x-www-form-urlencoded",
                "form_fields": ["run_id", "out_dir", "contract", "record_out"],
                "only_when_next_action": "publish_operator_record",
                "success_schema": "ao2.factory-project-start-operator-record.v1",
                "writes": ["record_out"],
                "fail_closed": true
            }
        },
        "hermes_contract": {
            "role": "front_end_queue_cron_memory_bookkeeping",
            "front_end_can_preview_next_action_without_backend_execution": true,
            "front_end_must_call_ao2_backend_for_mutating_action": true,
            "front_end_reads_single_operator_record": true,
            "raw_queue_json_scrape_required": false,
            "requires_manual_command_sequence": false,
            "requires_manual_closure_commands": false
        },
        "side_effects": {
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_rebuild_wrappers": false,
            "would_mutate_control_plane": false,
            "writes_only_explicit_record_out": true
        },
        "trust_boundary": {
            "hermes_role": "front_end_queue_cron_memory_bookkeeping",
            "ao2_role": "trusted_execution_queue_memory_replay_signed_evidence_producer",
            "factory_v3_role": "evaluator-closer / parity oracle",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) fn factory_project_start_hermes_flow_contract_json(
    target: &Path,
    out: &Path,
) -> Result<serde_json::Value> {
    let contract = factory_project_start_hermes_flow_contract_payload(target)?;
    atomic_write_text(out, &serde_json::to_string_pretty(&contract)?)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.hermes-project-start-flow-contract.v1",
        "status": "ready",
        "contract_path": out.display().to_string(),
        "contract_sha256": sha256_file(out)?,
        "workflow": contract["workflow"].clone(),
        "hermes_contract": contract["hermes_contract"].clone(),
        "side_effects": contract["side_effects"].clone(),
        "trust_boundary": contract["trust_boundary"].clone(),
        "contract": contract
    }))
}

pub(crate) fn embedded_project_start_hermes_flow_contract_json(
    target: &Path,
) -> Result<serde_json::Value> {
    let contract = factory_project_start_hermes_flow_contract_payload(target)?;
    let contract_bytes = serde_json::to_string_pretty(&contract)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.hermes-project-start-flow-contract.v1",
        "status": "ready",
        "embedded": true,
        "contract_sha256": sha256_bytes_hex(contract_bytes.as_bytes()),
        "workflow": contract["workflow"].clone(),
        "hermes_contract": contract["hermes_contract"].clone(),
        "side_effects": contract["side_effects"].clone(),
        "trust_boundary": contract["trust_boundary"].clone(),
        "contract": contract
    }))
}

fn embedded_greenfield_spec_ingest_entrypoint_json() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.hermes-greenfield-spec-ingest-entrypoint.v1",
        "status": "ready",
        "purpose": "Expose AO2-owned greenfield spec preflight and approved queue submission to Hermes without shelling out manually.",
        "preview": {
            "method": "GET",
            "path": "/api/factory/greenfield-spec-ingest",
            "minimum_role": "viewer",
            "required_query": ["spec"],
            "optional_query": ["target", "run_id", "verifier_command"],
            "schema_version": "ao2.factory-greenfield-spec-ingest.v1",
            "read_only": true
        },
        "submit": {
            "method": "POST",
            "path": "/api/factory/greenfield-spec-ingest/submit",
            "minimum_role": "operator",
            "required_form": ["spec", "approval_action_digest"],
            "optional_form": ["target", "run_id", "verifier_command", "max_repair_attempts"],
            "approval_mode": "exact_action_digest",
            "approval_schema_version": "ao2.factory-greenfield-spec-ingest-submit-approval.v1",
            "submit_schema_version": "ao2.factory-greenfield-spec-ingest-submit.v1"
        },
        "side_effects": {
            "would_write_files": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry_after_approval": true,
            "would_rebuild_wrappers": false,
            "would_mutate_control_plane": false
        },
        "trust_boundary": {
            "hermes_role": "front_end_queue_cron_memory_bookkeeping",
            "ao2_role": "trusted_execution_queue_memory_replay_signed_evidence_producer",
            "factory_v3_role": "evaluator-closer / parity oracle",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "forbidden_hermes_behaviors": [
            "edit raw queue JSON",
            "mutate generated project files directly",
            "execute providers during preview",
            "submit queue entries without exact action digest approval",
            "write control-plane approval",
            "use provider API-key authentication",
            "log bearer tokens, cookies, PEM material, or credentials"
        ],
        "ao2_decision_owner": "ao2-workbench-queue"
    })
}

pub(crate) fn factory_project_start_hermes_context_json(
    target: &Path,
) -> Result<serde_json::Value> {
    let flow_contract = embedded_project_start_hermes_flow_contract_json(target)?;
    let latest_support_packet = latest_workbench_support_packet_json(target)?;
    let greenfield_spec_ingest = embedded_greenfield_spec_ingest_entrypoint_json();
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-hermes-context.v1",
        "status": "ready",
        "target": target.display().to_string(),
        "flow": "project-start-next-action-to-operator-record",
        "generated_at_ms": now_unix_ms(),
        "flow_contract": flow_contract,
        "greenfield_spec_ingest": greenfield_spec_ingest,
        "latest_support_packet": latest_support_packet,
        "side_effects": {
            "would_write_files": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_rebuild_wrappers": false,
            "would_mutate_control_plane": false
        },
        "trust_boundary": {
            "hermes_role": "front_end_queue_cron_memory_bookkeeping",
            "ao2_role": "trusted_execution_queue_memory_replay_signed_evidence_producer",
            "factory_v3_role": "evaluator-closer / parity oracle",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        },
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) fn factory_queue_project_start_publish_operator_record_json(
    target: &Path,
    run_id: &str,
    out_dir: &Path,
    contract_path: &Path,
    record_out: &Path,
) -> Result<serde_json::Value> {
    let preflight =
        factory_queue_project_start_next_action_json(target, run_id, out_dir, contract_path)?;
    let next_action = json_string(&preflight, "next_action");
    if next_action != "publish_operator_record" {
        anyhow::bail!(
            "project-start operator record publish requires next action is publish_operator_record; next action is {next_action}"
        );
    }
    let status_probe = preflight
        .get("status_probe")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let queue_path = PathBuf::from(json_string(&status_probe, "queue_path"));
    let queue_sha256 = status_probe["queue_sha256"].clone();
    let submit_path = out_dir.join("factory-queue-project-start-submit.json");
    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let completion_contract_path =
        out_dir.join("factory-queue-project-start-completion-contract.json");
    let completion_contract_consumer_path =
        out_dir.join("factory-queue-project-start-completion-contract-consumer.json");
    let completion_contract_consumer =
        read_factory_compat_value(&completion_contract_consumer_path)
            .with_context(|| format!("read {}", completion_contract_consumer_path.display()))?;
    let record = serde_json::json!({
        "schema_version": "ao2.factory-project-start-operator-record.v1",
        "status": "ready_for_operator_review",
        "run_id": json_string(&preflight, "run_id"),
        "generated_at_ms": now_unix_ms(),
        "queue_path": queue_path.display().to_string(),
        "queue_sha256": queue_sha256,
        "next_action": next_action,
        "completion_record_state": json_string(&status_probe, "completion_record_state"),
        "ready_for_operator_review": status_probe["ready_for_operator_review"].clone(),
        "source_artifacts": {
            "queue_submit": factory_project_start_operator_record_artifact(&submit_path)?,
            "queue_run_next": factory_project_start_operator_record_artifact(&run_next_path)?,
            "completion_contract": factory_project_start_operator_record_artifact(&completion_contract_path)?,
            "completion_contract_consumer": factory_project_start_operator_record_artifact(&completion_contract_consumer_path)?
        },
        "hermes_contract": {
            "front_end_reads_single_operator_record": true,
            "raw_queue_json_scrape_required": false,
            "backend_used_bounded_ao2_queue": true,
            "requires_manual_command_sequence": false,
            "requires_manual_closure_commands": false
        },
        "trust_boundary": completion_contract_consumer["trust_boundary"].clone(),
        "ao2_decision_owner": "ao2-workbench-queue"
    });
    atomic_write_text(record_out, &serde_json::to_string_pretty(&record)?)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-operator-record.v1",
        "status": "published",
        "run_id": json_string(&record, "run_id"),
        "record_path": record_out.display().to_string(),
        "record_sha256": sha256_file(record_out)?,
        "would_execute_queue": false,
        "would_submit_queue_entry": false,
        "would_rebuild_wrappers": false,
        "would_mutate_control_plane": false,
        "read_only_preflight": preflight,
        "record": record,
        "trust_boundary": {
            "hermes_role": "front_end_queue_cron_memory_bookkeeping",
            "ao2_role": "trusted_execution_queue_memory_replay_signed_evidence_producer",
            "factory_v3_role": "evaluator-closer / parity oracle",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        }
    }))
}
