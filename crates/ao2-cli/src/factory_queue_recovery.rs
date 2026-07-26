use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli_util::{
    canonical_json_sha256, json_string, json_u64, sanitize_greenfield_id, sha256_file,
    trimmed_required,
};
use crate::factory_compat::{factory_ensure_target_repo, read_factory_compat_value};
use crate::factory_queue::{
    factory_project_start_completion_summary_memory_trust_boundary, factory_queue_load,
    factory_queue_path, factory_queue_project_start_completion_summary_json,
    factory_queue_status_is_terminal,
};
use crate::memory_store::{
    append_jsonl, memory_link_run_json, memory_records_path, memory_run_links_path,
    memory_write_record_json, read_jsonl_values,
};

pub(crate) fn factory_queue_project_start_complete_status_json(
    target: &Path,
    run_id: &str,
    out_dir: &Path,
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
    let queue_sha256 = if queue_path.is_file() {
        Some(sha256_file(&queue_path)?)
    } else {
        None
    };
    let submit_path = out_dir.join("factory-queue-project-start-submit.json");
    let run_next_path = out_dir.join("factory-queue-project-start-run-next.json");
    let completion_contract_path =
        out_dir.join("factory-queue-project-start-completion-contract.json");
    let completion_contract_consumer_path =
        out_dir.join("factory-queue-project-start-completion-contract-consumer.json");
    let artifact_paths = serde_json::json!({
        "queue_submit": submit_path.display().to_string(),
        "queue_run_next": run_next_path.display().to_string(),
        "completion_contract": completion_contract_path.display().to_string(),
        "completion_contract_consumer": completion_contract_consumer_path.display().to_string()
    });
    let artifact_presence = serde_json::json!({
        "queue_submit": submit_path.is_file(),
        "queue_run_next": run_next_path.is_file(),
        "completion_contract": completion_contract_path.is_file(),
        "completion_contract_consumer": completion_contract_consumer_path.is_file()
    });
    let trust_boundary = serde_json::json!({
        "factory_v3_role": "evaluator-closer / parity oracle",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false
    });
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let entry = entries
        .iter()
        .find(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(run_id.as_str()))
        .cloned();
    let mut status = "missing".to_string();
    let mut completion_record_state = "missing_queue_entry".to_string();
    let mut ready_for_operator_review = false;
    let mut blockers = Vec::<String>::new();
    let mut blocker_codes = Vec::<String>::new();
    macro_rules! push_blocker {
        ($code:expr, $detail:expr) => {{
            blocker_codes.push(($code).to_string());
            blockers.push(($detail).to_string());
        }};
    }
    let queue_entry_status = entry
        .as_ref()
        .map(|entry| json_string(entry, "status"))
        .unwrap_or_default();

    if !queue_path.is_file() {
        status = "missing".to_string();
        completion_record_state = "missing_queue_file".to_string();
        push_blocker!(
            "missing_queue_file",
            format!("factory queue file missing at {}", queue_path.display())
        );
    } else if entry.is_none() {
        status = "missing".to_string();
        completion_record_state = "missing_queue_entry".to_string();
        push_blocker!(
            "missing_queue_entry",
            format!("run_id {run_id} is not present in the factory project-start queue")
        );
    } else if let Some(entry) = entry.as_ref() {
        if json_string(entry, "job_kind") != "factory_project_start" {
            status = "blocked".to_string();
            completion_record_state = "wrong_job_kind".to_string();
            push_blocker!(
                "wrong_job_kind",
                format!(
                    "run_id {run_id} is {}, not factory_project_start",
                    json_string(entry, "job_kind")
                )
            );
        } else if queue_entry_status != "accepted" {
            status = queue_entry_status.clone();
            completion_record_state = queue_entry_status.clone();
            let normalized_status: String = if queue_entry_status.trim().is_empty() {
                "missing".to_string()
            } else {
                queue_entry_status
                    .chars()
                    .map(|ch| {
                        if ch.is_ascii_alphanumeric() {
                            ch.to_ascii_lowercase()
                        } else {
                            '_'
                        }
                    })
                    .collect()
            };
            push_blocker!(
                format!("queue_entry_status_{normalized_status}"),
                format!(
                    "queue entry status is {}; mutating backend execution is required or the prior run failed",
                    if queue_entry_status.trim().is_empty() {
                        "missing"
                    } else {
                        queue_entry_status.as_str()
                    }
                )
            );
        } else if !submit_path.is_file()
            || !run_next_path.is_file()
            || !completion_contract_path.is_file()
            || !completion_contract_consumer_path.is_file()
        {
            status = "incomplete".to_string();
            completion_record_state = "missing_compact_artifact".to_string();
            for (label, path) in [
                ("queue_submit", &submit_path),
                ("queue_run_next", &run_next_path),
                ("completion_contract", &completion_contract_path),
                (
                    "completion_contract_consumer",
                    &completion_contract_consumer_path,
                ),
            ] {
                if !path.is_file() {
                    push_blocker!(
                        format!("missing_compact_artifact_{label}"),
                        format!("{label} missing at {}", path.display())
                    );
                }
            }
        } else {
            let submit = read_factory_compat_value(&submit_path)
                .with_context(|| format!("read {}", submit_path.display()))?;
            let run_next = read_factory_compat_value(&run_next_path)
                .with_context(|| format!("read {}", run_next_path.display()))?;
            let completion_contract = read_factory_compat_value(&completion_contract_path)
                .with_context(|| format!("read {}", completion_contract_path.display()))?;
            let completion_contract_consumer = read_factory_compat_value(
                &completion_contract_consumer_path,
            )
            .with_context(|| format!("read {}", completion_contract_consumer_path.display()))?;

            for (label, value) in [("queue_submit", &submit), ("queue_run_next", &run_next)] {
                if json_string(value, "run_id") != run_id {
                    push_blocker!(
                        format!("artifact_run_id_mismatch_{label}"),
                        format!(
                            "{label} run_id mismatch: expected {run_id}, got {}",
                            json_string(value, "run_id")
                        )
                    );
                }
            }
            let contract_run_id = json_string(&completion_contract, "run_id");
            if !contract_run_id.trim().is_empty() && contract_run_id != run_id {
                push_blocker!(
                    "artifact_run_id_mismatch_completion_contract",
                    format!(
                        "completion_contract run_id mismatch: expected {run_id}, got {contract_run_id}"
                    )
                );
            }
            if json_string(&completion_contract, "status") != "accepted" {
                push_blocker!(
                    "artifact_status_mismatch_completion_contract",
                    format!(
                        "completion_contract status is {}",
                        json_string(&completion_contract, "status")
                    )
                );
            }
            if json_string(&completion_contract_consumer, "status") != "accepted" {
                push_blocker!(
                    "artifact_status_mismatch_completion_contract_consumer",
                    format!(
                        "completion_contract_consumer status is {}",
                        json_string(&completion_contract_consumer, "status")
                    )
                );
            }
            if completion_contract_consumer["trust_boundary"]["release_acceptance_owner"]
                != "factory-v3 evaluator-closer"
                || completion_contract_consumer["trust_boundary"]["control_plane_approves_release"]
                    != false
                || completion_contract_consumer["trust_boundary"]["mutates_ao_artifacts"] != false
            {
                push_blocker!(
                    "trust_boundary_mismatch_completion_contract_consumer",
                    "completion_contract_consumer trust boundary mismatch"
                );
            }

            if blockers.is_empty() {
                status = "accepted".to_string();
                completion_record_state = "complete".to_string();
                ready_for_operator_review = completion_contract_consumer
                    ["ready_for_operator_review"]
                    .as_bool()
                    .unwrap_or(false);
            } else {
                status = "blocked".to_string();
                completion_record_state = "artifact_mismatch".to_string();
            }
        }
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-queue-complete-status.v1",
        "status": status,
        "completion_record_state": completion_record_state,
        "ready_for_operator_review": ready_for_operator_review,
        "run_id": run_id,
        "queue_path": queue_path.display().to_string(),
        "queue_sha256": queue_sha256,
        "queue_entry_status": queue_entry_status,
        "read_only": true,
        "would_execute_queue": false,
        "would_submit_queue_entry": false,
        "would_rebuild_wrappers": false,
        "artifact_paths": artifact_paths,
        "artifact_presence": artifact_presence,
        "blocker_codes": blocker_codes,
        "blockers": blockers,
        "hermes_contract": {
            "front_end_can_poll_without_backend_execution": true,
            "backend_used_bounded_ao2_queue": false,
            "requires_manual_command_sequence": false,
            "requires_manual_closure_commands": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) fn factory_queue_project_start_completion_summary_memory_json(
    target: &Path,
    run_id: &str,
    approve_action_digest: Option<&str>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let summary = factory_queue_project_start_completion_summary_json(&target_root, run_id)?;
    let run_id = json_string(&summary, "run_id");
    let summary_sha256 = canonical_json_sha256(&summary);
    let queue_sha256 = json_string(&summary["queue"], "sha256");
    let trust_boundary = factory_project_start_completion_summary_memory_trust_boundary();
    let digest_input = serde_json::json!({
        "action": "ao2.project-start-completion-summary-memory-checkpoint.v1",
        "run_id": run_id,
        "summary_sha256": summary_sha256,
        "queue_sha256": queue_sha256,
        "trust_boundary": trust_boundary
    });
    let action_digest = canonical_json_sha256(&digest_input);
    let submitted_digest = approve_action_digest.unwrap_or("").trim();
    if submitted_digest != action_digest {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-completion-summary-memory-checkpoint-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_action_digest",
            "required_flag": "--approve-action-digest",
            "required_form_field": "approval_action_digest",
            "action_digest": action_digest,
            "run_id": run_id,
            "summary_sha256": summary_sha256,
            "queue_sha256": queue_sha256,
            "summary": summary,
            "next_action": "submit approval_action_digest or --approve-action-digest with the exact action_digest to record the AO2 memory checkpoint",
            "side_effects": {
                "would_write_memory_after_approval": true,
                "would_write_memory_run_link_after_approval": true,
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_mutate_control_plane": false,
                "would_write_queue_file": false
            },
            "trust_boundary": trust_boundary,
            "ao2_decision_owner": "ao2-memory"
        }));
    }

    let body = format!(
        "Project-start completion summary recorded for run_id={run_id}\nsummary_sha256={summary_sha256}\nqueue_sha256={queue_sha256}\nnext_recommended_action={}",
        json_string(&summary["hermes_memory"], "next_recommended_action")
    );
    let memory_record = memory_write_record_json(
        &target_root,
        "project-start-completion-summary".to_string(),
        format!("Project-start completion summary: {run_id}"),
        body,
        vec![
            "hermes".to_string(),
            "ao2".to_string(),
            "project-start".to_string(),
            "completion-summary".to_string(),
        ],
        Some(run_id.clone()),
        None,
    )?;
    append_jsonl(&memory_records_path(&target_root), &memory_record)?;
    let memory_link = memory_link_run_json(
        &target_root,
        json_string(&memory_record, "id"),
        run_id.clone(),
        "project-start-completion-summary".to_string(),
    )?;
    append_jsonl(&memory_run_links_path(&target_root), &memory_link)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-completion-summary-memory-checkpoint.v1",
        "status": "recorded",
        "run_id": run_id,
        "approval": {
            "schema_version": "ao2.factory-project-start-completion-summary-memory-checkpoint-approval.v1",
            "status": "approved_exact_action_digest",
            "approval_mode": "exact_action_digest",
            "action_digest": action_digest
        },
        "summary_sha256": summary_sha256,
        "queue_sha256": queue_sha256,
        "summary": summary,
        "memory_record": memory_record,
        "memory_link": memory_link,
        "memory_paths": {
            "records_jsonl": memory_records_path(&target_root).display().to_string(),
            "run_links_jsonl": memory_run_links_path(&target_root).display().to_string()
        },
        "hermes_memory": {
            "single_record_for_bookkeeping": true,
            "summary_is_compact": true,
            "checkpoint_is_durable": true,
            "next_recommended_action": "read_memory_checkpoint"
        },
        "side_effects": {
            "wrote_memory_record": true,
            "wrote_memory_run_link": true,
            "executed_provider": false,
            "executed_queue": false,
            "submitted_queue_entry": false,
            "wrote_queue_file": false,
            "mutated_control_plane": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-memory"
    }))
}

fn completion_summary_memory_body_field(record: &serde_json::Value, key: &str) -> Result<String> {
    let prefix = format!("{key}=");
    let body = json_string(record, "body");
    body.lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .with_context(|| {
            format!(
                "memory record {} is missing {key}",
                json_string(record, "id")
            )
        })
}

pub(crate) fn factory_queue_project_start_completion_summary_memory_status_json(
    target: &Path,
    run_id: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let run_id = trimmed_required("--run-id", run_id)?;
    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records = read_jsonl_values(&records_path)?;
    let memory_record = records
        .iter()
        .filter(|record| {
            record["schema_version"] == "ao2.memory-record.v1"
                && record["kind"] == "project-start-completion-summary"
                && record["source"]["run_id"] == run_id
        })
        .max_by_key(|record| json_u64(record, "created_at_ms"))
        .cloned()
        .with_context(|| {
            format!("missing project-start completion-summary memory record for run_id {run_id}")
        })?;
    let memory_id = json_string(&memory_record, "id");
    let links = read_jsonl_values(&links_path)?;
    let memory_link = links
        .iter()
        .find(|link| {
            link["schema_version"] == "ao2.memory-run-link.v1"
                && json_string(link, "memory_id") == memory_id
                && json_string(link, "run_id") == run_id
                && json_string(link, "relationship") == "project-start-completion-summary"
        })
        .cloned()
        .with_context(|| {
            format!(
                "missing project-start completion-summary memory run link for run_id {run_id} memory_id {memory_id}"
            )
        })?;
    let summary_sha256 = completion_summary_memory_body_field(&memory_record, "summary_sha256")?;
    let queue_sha256 = completion_summary_memory_body_field(&memory_record, "queue_sha256")?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-completion-summary-memory-status.v1",
        "status": "ready",
        "run_id": run_id,
        "read_only": true,
        "summary_sha256": summary_sha256,
        "queue_sha256": queue_sha256,
        "memory_record": {
            "schema_version": memory_record["schema_version"].clone(),
            "id": memory_record["id"].clone(),
            "created_at_ms": memory_record["created_at_ms"].clone(),
            "kind": memory_record["kind"].clone(),
            "title": memory_record["title"].clone(),
            "source": memory_record["source"].clone(),
            "tags": memory_record["tags"].clone()
        },
        "memory_link": memory_link,
        "memory_paths": {
            "records_jsonl": records_path.display().to_string(),
            "run_links_jsonl": links_path.display().to_string()
        },
        "hermes_memory": {
            "checkpoint_is_durable": true,
            "raw_memory_jsonl_scrape_required": false,
            "single_record_for_bookkeeping": true,
            "next_recommended_action": "read_memory_checkpoint"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_execute_provider": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-memory"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_json(
    target: &Path,
    run_id: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let run_id = trimmed_required("--run-id", run_id)?;
    let summary = factory_queue_project_start_completion_summary_json(&target_root, &run_id)?;
    let memory_status =
        factory_queue_project_start_completion_summary_memory_status_json(&target_root, &run_id)?;
    let summary_sha256 = canonical_json_sha256(&summary);
    let summary_queue_sha256 = json_string(&summary["queue"], "sha256");
    let memory_summary_sha256 = json_string(&memory_status, "summary_sha256");
    let memory_queue_sha256 = json_string(&memory_status, "queue_sha256");
    let memory_record_id = json_string(&memory_status["memory_record"], "id");
    let memory_link_id = json_string(&memory_status["memory_link"], "memory_id");
    if memory_summary_sha256 != summary_sha256 {
        anyhow::bail!(
            "completion summary sha mismatch for run_id {run_id}: summary {summary_sha256}, memory {memory_summary_sha256}"
        );
    }
    if memory_queue_sha256 != summary_queue_sha256 {
        anyhow::bail!(
            "queue sha mismatch for run_id {run_id}: summary {summary_queue_sha256}, memory {memory_queue_sha256}"
        );
    }
    if memory_record_id != memory_link_id {
        anyhow::bail!(
            "memory link mismatch for run_id {run_id}: record {memory_record_id}, link {memory_link_id}"
        );
    }
    if json_string(&memory_status["memory_link"], "relationship")
        != "project-start-completion-summary"
    {
        anyhow::bail!("unexpected memory link relationship for run_id {run_id}");
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery.v1",
        "status": "ready",
        "run_id": run_id,
        "read_only": true,
        "queue": {
            "path": summary["queue"]["path"].clone(),
            "sha256": summary_queue_sha256,
            "status": summary["queue"]["status"].clone(),
            "updated_at": summary["queue"]["updated_at"].clone()
        },
        "project_start_queue": {
            "submit_surface_present": true,
            "run_next_completion_present": true,
            "job_kind": summary["job_kind"].clone(),
            "terminal_status": summary["status"].clone()
        },
        "completion_summary": {
            "schema_version": summary["schema_version"].clone(),
            "status": summary["status"].clone(),
            "run_id": summary["run_id"].clone(),
            "sha256": summary_sha256,
            "queue_sha256": summary["queue"]["sha256"].clone(),
            "checks": summary["checks"].clone(),
            "artifacts": summary["artifacts"].clone()
        },
        "memory_checkpoint_status": memory_status,
        "surface_status": {
            "queue_entry": {
                "present": true,
                "status": summary["queue"]["status"].clone()
            },
            "run_next_completion": {
                "present": true,
                "status": summary["status"].clone()
            },
            "completion_summary": {
                "present": true,
                "sha256": summary_sha256
            },
            "memory_checkpoint": {
                "present": true,
                "status": "ready",
                "memory_record_id": memory_record_id,
                "relationship": "project-start-completion-summary"
            },
            "recovery_packet": {
                "present": true,
                "status": "ready"
            }
        },
        "hermes_memory": {
            "single_recovery_packet_for_bookkeeping": true,
            "raw_queue_json_scrape_required": false,
            "raw_memory_jsonl_scrape_required": false,
            "manual_multi_command_recovery_required": false,
            "next_recommended_action": "resume_from_recovery_packet"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_execute_provider": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-workbench-recovery"
    }))
}

pub(crate) fn factory_queue_project_start_latest_recovery_json(
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
    let mut skipped_candidates = Vec::new();

    for entry in entries.iter().rev() {
        if entry.get("job_kind").and_then(|value| value.as_str()) != Some("factory_project_start") {
            continue;
        }
        let status = json_string(entry, "status");
        if !factory_queue_status_is_terminal(&status) {
            continue;
        }
        let run_id = json_string(entry, "run_id");
        if run_id.trim().is_empty() {
            skipped_candidates.push(serde_json::json!({
                "run_id": "",
                "status": status,
                "reason": "terminal project-start entry is missing run_id"
            }));
            continue;
        }
        match factory_queue_project_start_recovery_json(&target_root, &run_id) {
            Ok(recovery_packet) => {
                let recovery_packet_sha256 = canonical_json_sha256(&recovery_packet);
                let queue_sha256 = json_string(&recovery_packet["queue"], "sha256");
                return Ok(serde_json::json!({
                    "schema_version": "ao2.factory-project-start-latest-recovery.v1",
                    "status": "ready",
                    "read_only": true,
                    "selected": {
                        "run_id": run_id,
                        "selection_reason": "latest_terminal_project_start_with_complete_recovery",
                        "queue_sha256": queue_sha256,
                        "recovery_packet_sha256": recovery_packet_sha256
                    },
                    "recovery_packet": recovery_packet,
                    "skipped_candidates": skipped_candidates,
                    "surface_status": {
                        "latest_recovery_selector": {
                            "present": true,
                            "status": "ready"
                        },
                        "recovery_packet": {
                            "present": true,
                            "status": "ready"
                        }
                    },
                    "hermes_memory": {
                        "single_latest_recovery_packet_for_bookkeeping": true,
                        "run_id_memory_required": false,
                        "raw_queue_json_scrape_required": false,
                        "raw_memory_jsonl_scrape_required": false,
                        "manual_multi_command_recovery_required": false,
                        "next_recommended_action": "resume_from_latest_recovery_packet"
                    },
                    "side_effects": {
                        "would_write_memory": false,
                        "would_write_memory_run_link": false,
                        "would_execute_queue": false,
                        "would_submit_queue_entry": false,
                        "would_execute_provider": false,
                        "would_mutate_control_plane": false,
                        "would_write_queue_file": false,
                        "would_approve_release": false
                    },
                    "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
                    "ao2_decision_owner": "ao2-workbench-recovery"
                }));
            }
            Err(error) => {
                skipped_candidates.push(serde_json::json!({
                    "run_id": run_id,
                    "status": status,
                    "reason": error.to_string()
                }));
            }
        }
    }

    anyhow::bail!(
        "no terminal factory_project_start entry has a complete recovery packet: {}",
        serde_json::to_string(&skipped_candidates)?
    )
}

fn factory_recovery_action_allowed_actions_json() -> serde_json::Value {
    serde_json::json!([
        {
            "action": "resume_from_latest_recovery_packet",
            "when": "latest terminal factory_project_start entry has a complete recovery packet",
            "mutating": false,
            "requires_exact_digest": true
        },
        {
            "action": "wait_for_queue_terminal",
            "when": "latest factory_project_start entry is queued or running",
            "mutating": false,
            "requires_exact_digest": false
        },
        {
            "action": "record_completion_summary_memory",
            "when": "latest terminal factory_project_start entry has completion summary but no durable AO2 memory checkpoint",
            "mutating": true,
            "must_call_ao2_backend": true,
            "requires_exact_digest": true
        },
        {
            "action": "operator_attention_required",
            "when": "AO2 cannot prove a safe automated recovery action",
            "mutating": false,
            "requires_exact_digest": false
        }
    ])
}

fn factory_recovery_action_side_effects_json() -> serde_json::Value {
    serde_json::json!({
        "would_write_memory": false,
        "would_write_memory_run_link": false,
        "would_execute_queue": false,
        "would_submit_queue_entry": false,
        "would_execute_provider": false,
        "would_mutate_control_plane": false,
        "would_write_queue_file": false,
        "would_approve_release": false
    })
}

fn factory_recovery_action_hermes_contract_json() -> serde_json::Value {
    serde_json::json!({
        "front_end_can_poll_without_backend_execution": true,
        "front_end_reads_one_action_contract": true,
        "front_end_must_call_ao2_backend_for_mutating_action": true,
        "front_end_must_not_scrape_raw_queue_json": true,
        "front_end_must_not_scan_raw_memory_jsonl": true,
        "raw_queue_json_scrape_required": false,
        "raw_memory_jsonl_scrape_required": false,
        "requires_manual_command_sequence": false
    })
}

pub(crate) fn factory_queue_project_start_recovery_action_json(
    target: &Path,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    match factory_queue_project_start_latest_recovery_json(&target_root) {
        Ok(latest_recovery_selector) => {
            let selected = latest_recovery_selector["selected"].clone();
            Ok(serde_json::json!({
                "schema_version": "ao2.factory-project-start-recovery-action.v1",
                "status": "ready",
                "read_only": true,
                "recommended_action": "resume_from_latest_recovery_packet",
                "selected": selected,
                "blockers": [],
                "allowed_actions": factory_recovery_action_allowed_actions_json(),
                "exact_digest_requirements": {
                    "resume_from_latest_recovery_packet": {
                        "queue_sha256": latest_recovery_selector["selected"]["queue_sha256"].clone(),
                        "recovery_packet_sha256": latest_recovery_selector["selected"]["recovery_packet_sha256"].clone(),
                        "digest_source": "ao2.factory-project-start-latest-recovery.v1.selected"
                    },
                    "record_completion_summary_memory": {
                        "requires_ao2_memory_checkpoint_approval": true,
                        "approval_mode": "exact_action_digest",
                        "digest_source": "ao2.factory-project-start-completion-summary-memory-checkpoint-approval.v1.action_digest"
                    }
                },
                "latest_recovery_selector": latest_recovery_selector,
                "hermes_contract": factory_recovery_action_hermes_contract_json(),
                "side_effects": factory_recovery_action_side_effects_json(),
                "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
                "ao2_decision_owner": "ao2-workbench-recovery"
            }))
        }
        Err(error) => {
            let (recommended_action, blockers) =
                factory_queue_project_start_recovery_action_blockers(&target_root, error)?;
            Ok(serde_json::json!({
                "schema_version": "ao2.factory-project-start-recovery-action.v1",
                "status": "action_required",
                "read_only": true,
                "recommended_action": recommended_action,
                "selected": serde_json::Value::Null,
                "blockers": blockers,
                "allowed_actions": factory_recovery_action_allowed_actions_json(),
                "exact_digest_requirements": {
                    "resume_from_latest_recovery_packet": {
                        "requires_selected_latest_recovery_packet": true,
                        "digest_source": "ao2.factory-project-start-latest-recovery.v1.selected"
                    },
                    "record_completion_summary_memory": {
                        "requires_ao2_memory_checkpoint_approval": true,
                        "approval_mode": "exact_action_digest",
                        "digest_source": "ao2.factory-project-start-completion-summary-memory-checkpoint-approval.v1.action_digest"
                    }
                },
                "latest_recovery_selector": serde_json::Value::Null,
                "hermes_contract": factory_recovery_action_hermes_contract_json(),
                "side_effects": factory_recovery_action_side_effects_json(),
                "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
                "ao2_decision_owner": "ao2-workbench-recovery"
            }))
        }
    }
}

fn factory_queue_project_start_recovery_action_blockers(
    target_root: &Path,
    latest_error: anyhow::Error,
) -> Result<(String, serde_json::Value)> {
    let queue = factory_queue_load(target_root)?;
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(entry) = entries.iter().rev().find(|entry| {
        entry.get("job_kind").and_then(|value| value.as_str()) == Some("factory_project_start")
    }) else {
        return Ok((
            "operator_attention_required".to_string(),
            serde_json::json!([{
                "code": "no_project_start_entries",
                "message": latest_error.to_string()
            }]),
        ));
    };
    let status = json_string(entry, "status");
    let run_id = json_string(entry, "run_id");
    let latest_error = latest_error.to_string();
    let action = if matches!(status.as_str(), "queued" | "running" | "cancel_requested") {
        "wait_for_queue_terminal"
    } else if factory_queue_status_is_terminal(&status)
        && latest_error.contains("memory")
        && !run_id.trim().is_empty()
    {
        "record_completion_summary_memory"
    } else {
        "operator_attention_required"
    };
    Ok((
        action.to_string(),
        serde_json::json!([{
            "code": "latest_complete_recovery_unavailable",
            "run_id": run_id,
            "queue_status": status,
            "message": latest_error
        }]),
    ))
}

pub(crate) fn factory_queue_project_start_recovery_resume_receipt_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let action_contract = factory_queue_project_start_recovery_action_json(&target_root)?;
    if json_string(&action_contract, "status") != "ready" {
        anyhow::bail!(
            "recovery resume receipt requires a ready recovery action contract: status={}",
            json_string(&action_contract, "status")
        );
    }
    if json_string(&action_contract, "recommended_action") != "resume_from_latest_recovery_packet" {
        anyhow::bail!(
            "recovery resume receipt requires recommended_action=resume_from_latest_recovery_packet: recommended_action={}",
            json_string(&action_contract, "recommended_action")
        );
    }

    let requirements =
        &action_contract["exact_digest_requirements"]["resume_from_latest_recovery_packet"];
    let expected_queue_sha256 = json_string(requirements, "queue_sha256");
    let expected_recovery_packet_sha256 = json_string(requirements, "recovery_packet_sha256");
    if queue_sha256 != expected_queue_sha256 {
        anyhow::bail!(
            "queue_sha256 digest drift: expected {}, got {}",
            expected_queue_sha256,
            queue_sha256
        );
    }
    if recovery_packet_sha256 != expected_recovery_packet_sha256 {
        anyhow::bail!(
            "recovery_packet_sha256 digest drift: expected {}, got {}",
            expected_recovery_packet_sha256,
            recovery_packet_sha256
        );
    }

    let selected = action_contract["selected"].clone();
    let recovery_packet = action_contract["latest_recovery_selector"]["recovery_packet"].clone();
    let completion_summary_sha256 = json_string(&recovery_packet["completion_summary"], "sha256");
    let memory_record_id = json_string(
        &recovery_packet["memory_checkpoint_status"]["memory_record"],
        "id",
    );
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-receipt.v1",
        "status": "ready",
        "read_only": true,
        "action": "resume_from_latest_recovery_packet",
        "selected": selected,
        "digest_verification": {
            "digest_source": "ao2.factory-project-start-recovery-action.v1.exact_digest_requirements.resume_from_latest_recovery_packet",
            "queue_sha256_expected": expected_queue_sha256,
            "queue_sha256_provided": queue_sha256,
            "queue_sha256_matches": true,
            "recovery_packet_sha256_expected": expected_recovery_packet_sha256,
            "recovery_packet_sha256_provided": recovery_packet_sha256,
            "recovery_packet_sha256_matches": true
        },
        "backend_resume_payload": {
            "schema_version": "ao2.factory-project-start-recovery-resume-payload.v1",
            "action": "resume_from_latest_recovery_packet",
            "run_id": json_string(&action_contract["selected"], "run_id"),
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "completion_summary_sha256": completion_summary_sha256,
            "memory_record_id": memory_record_id,
            "requires_backend_digest_recheck": true
        },
        "action_contract": action_contract,
        "recovery_packet": recovery_packet,
        "hermes_contract": {
            "front_end_reads_one_resume_receipt": true,
            "front_end_can_submit_backend_resume_payload": true,
            "backend_must_verify_exact_digests": true,
            "front_end_must_not_execute_provider": true,
            "front_end_must_not_run_queue_entry": true,
            "front_end_must_not_write_memory": true,
            "raw_queue_json_scrape_required": false,
            "raw_memory_jsonl_scrape_required": false,
            "requires_manual_command_sequence": false
        },
        "side_effects": factory_recovery_action_side_effects_json(),
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-workbench-recovery"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_checkpoint_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    approve_action_digest: Option<&str>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let receipt = factory_queue_project_start_recovery_resume_receipt_json(
        &target_root,
        queue_sha256,
        recovery_packet_sha256,
    )?;
    let run_id = json_string(&receipt["selected"], "run_id");
    let receipt_sha256 = canonical_json_sha256(&receipt);
    let completion_summary_sha256 = json_string(
        &receipt["backend_resume_payload"],
        "completion_summary_sha256",
    );
    let prior_memory_record_id =
        json_string(&receipt["backend_resume_payload"], "memory_record_id");
    let trust_boundary = factory_project_start_completion_summary_memory_trust_boundary();
    let digest_input = serde_json::json!({
        "action": "ao2.project-start-recovery-resume-checkpoint.v1",
        "run_id": run_id,
        "receipt_sha256": receipt_sha256,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "completion_summary_sha256": completion_summary_sha256,
        "prior_memory_record_id": prior_memory_record_id,
        "trust_boundary": trust_boundary
    });
    let action_digest = canonical_json_sha256(&digest_input);
    let submitted_digest = approve_action_digest.unwrap_or("").trim();
    if submitted_digest != action_digest {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-checkpoint-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_action_digest",
            "required_flag": "--approve-action-digest",
            "required_form_field": "approval_action_digest",
            "action_digest": action_digest,
            "run_id": run_id,
            "receipt_sha256": receipt_sha256,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "completion_summary_sha256": completion_summary_sha256,
            "prior_memory_record_id": prior_memory_record_id,
            "receipt": receipt,
            "next_action": "submit approval_action_digest or --approve-action-digest with the exact action_digest to record the AO2 recovery resume checkpoint",
            "side_effects": {
                "would_write_memory_after_approval": true,
                "would_write_memory_run_link_after_approval": true,
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_submit_queue_entry": false,
                "would_mutate_control_plane": false,
                "would_write_queue_file": false,
                "would_approve_release": false
            },
            "trust_boundary": trust_boundary,
            "ao2_decision_owner": "ao2-memory"
        }));
    }

    let body = format!(
        "Project-start recovery resume checkpoint recorded for run_id={run_id}\nreceipt_sha256={receipt_sha256}\nqueue_sha256={queue_sha256}\nrecovery_packet_sha256={recovery_packet_sha256}\ncompletion_summary_sha256={completion_summary_sha256}\nprior_memory_record_id={prior_memory_record_id}"
    );
    let mut memory_record = memory_write_record_json(
        &target_root,
        "project-start-recovery-resume-checkpoint".to_string(),
        format!("Project-start recovery resume checkpoint: {run_id}"),
        body,
        vec![
            "hermes".to_string(),
            "ao2".to_string(),
            "project-start".to_string(),
            "recovery".to_string(),
            "resume-checkpoint".to_string(),
        ],
        Some(run_id.clone()),
        None,
    )?;
    memory_record["source"]["path"] =
        serde_json::json!("inline:ao2.factory-project-start-recovery-resume-receipt.v1");
    memory_record["source"]["path_sha256"] = serde_json::json!(receipt_sha256.clone());
    append_jsonl(&memory_records_path(&target_root), &memory_record)?;
    let memory_link = memory_link_run_json(
        &target_root,
        json_string(&memory_record, "id"),
        run_id.clone(),
        "project-start-recovery-resume-checkpoint".to_string(),
    )?;
    append_jsonl(&memory_run_links_path(&target_root), &memory_link)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-checkpoint.v1",
        "status": "recorded",
        "run_id": run_id,
        "approval": {
            "schema_version": "ao2.factory-project-start-recovery-resume-checkpoint-approval.v1",
            "status": "approved_exact_action_digest",
            "approval_mode": "exact_action_digest",
            "action_digest": action_digest
        },
        "receipt_sha256": receipt_sha256,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "completion_summary_sha256": completion_summary_sha256,
        "prior_memory_record_id": prior_memory_record_id,
        "receipt": receipt,
        "memory_record": memory_record,
        "memory_link": memory_link,
        "memory_paths": {
            "records_jsonl": memory_records_path(&target_root).display().to_string(),
            "run_links_jsonl": memory_run_links_path(&target_root).display().to_string()
        },
        "hermes_memory": {
            "single_record_for_bookkeeping": true,
            "checkpoint_is_durable": true,
            "resume_receipt_recorded": true,
            "raw_queue_json_scrape_required": false,
            "raw_memory_jsonl_scrape_required": false,
            "next_recommended_action": "read_recovery_resume_checkpoint"
        },
        "side_effects": {
            "wrote_memory_record": true,
            "wrote_memory_run_link": true,
            "executed_provider": false,
            "executed_queue": false,
            "submitted_queue_entry": false,
            "wrote_queue_file": false,
            "mutated_control_plane": false,
            "approved_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-memory"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_checkpoint_status_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let receipt = factory_queue_project_start_recovery_resume_receipt_json(
        &target_root,
        queue_sha256,
        recovery_packet_sha256,
    )?;
    let run_id = json_string(&receipt["selected"], "run_id");
    let receipt_sha256 = canonical_json_sha256(&receipt);
    let completion_summary_sha256 = json_string(
        &receipt["backend_resume_payload"],
        "completion_summary_sha256",
    );
    let prior_memory_record_id =
        json_string(&receipt["backend_resume_payload"], "memory_record_id");

    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records = read_jsonl_values(&records_path)?;
    let memory_record = records
        .iter()
        .filter(|record| {
            record["schema_version"] == "ao2.memory-record.v1"
                && record["kind"] == "project-start-recovery-resume-checkpoint"
                && record["source"]["run_id"] == run_id
                && json_string(&record["source"], "path_sha256") == receipt_sha256
        })
        .max_by_key(|record| json_u64(record, "created_at_ms"))
        .cloned()
        .with_context(|| {
            format!(
                "missing project-start recovery resume checkpoint memory record for run_id {run_id} receipt_sha256 {receipt_sha256}"
            )
        })?;
    let memory_id = json_string(&memory_record, "id");
    let links = read_jsonl_values(&links_path)?;
    let memory_link = links
        .iter()
        .find(|link| {
            link["schema_version"] == "ao2.memory-run-link.v1"
                && json_string(link, "memory_id") == memory_id
                && json_string(link, "run_id") == run_id
                && json_string(link, "relationship") == "project-start-recovery-resume-checkpoint"
        })
        .cloned()
        .with_context(|| {
            format!(
                "missing project-start recovery resume checkpoint memory run link for run_id {run_id} memory_id {memory_id}"
            )
        })?;

    let recorded_receipt_sha256 =
        completion_summary_memory_body_field(&memory_record, "receipt_sha256")?;
    let recorded_queue_sha256 =
        completion_summary_memory_body_field(&memory_record, "queue_sha256")?;
    let recorded_recovery_packet_sha256 =
        completion_summary_memory_body_field(&memory_record, "recovery_packet_sha256")?;
    let recorded_completion_summary_sha256 =
        completion_summary_memory_body_field(&memory_record, "completion_summary_sha256")?;
    let recorded_prior_memory_record_id =
        completion_summary_memory_body_field(&memory_record, "prior_memory_record_id")?;
    if recorded_receipt_sha256 != receipt_sha256
        || recorded_queue_sha256 != queue_sha256
        || recorded_recovery_packet_sha256 != recovery_packet_sha256
        || recorded_completion_summary_sha256 != completion_summary_sha256
        || recorded_prior_memory_record_id != prior_memory_record_id
    {
        anyhow::bail!("project-start recovery resume checkpoint memory record digest binding mismatch for run_id {run_id}");
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-checkpoint-status.v1",
        "status": "ready",
        "run_id": run_id,
        "read_only": true,
        "receipt_sha256": receipt_sha256,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "completion_summary_sha256": completion_summary_sha256,
        "prior_memory_record_id": prior_memory_record_id,
        "receipt": receipt,
        "memory_record": {
            "schema_version": memory_record["schema_version"].clone(),
            "id": memory_record["id"].clone(),
            "created_at_ms": memory_record["created_at_ms"].clone(),
            "kind": memory_record["kind"].clone(),
            "title": memory_record["title"].clone(),
            "source": memory_record["source"].clone(),
            "tags": memory_record["tags"].clone()
        },
        "memory_link": memory_link,
        "memory_paths": {
            "records_jsonl": records_path.display().to_string(),
            "run_links_jsonl": links_path.display().to_string()
        },
        "hermes_memory": {
            "checkpoint_is_durable": true,
            "resume_receipt_recorded": true,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "single_record_for_bookkeeping": true,
            "next_recommended_action": "read_recovery_resume_checkpoint_status"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_execute_provider": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-memory"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_continuity_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
) -> Result<serde_json::Value> {
    let checkpoint_status = factory_queue_project_start_recovery_resume_checkpoint_status_json(
        target,
        queue_sha256,
        recovery_packet_sha256,
    )?;
    let receipt = checkpoint_status["receipt"].clone();
    let action_contract = receipt["action_contract"].clone();
    let action_requirements =
        &action_contract["exact_digest_requirements"]["resume_from_latest_recovery_packet"];
    let action_queue_sha256 = json_string(action_requirements, "queue_sha256");
    let action_recovery_packet_sha256 = json_string(action_requirements, "recovery_packet_sha256");
    let receipt_sha256 = canonical_json_sha256(&receipt);
    let checkpoint_status_sha256 = canonical_json_sha256(&checkpoint_status);
    let action_contract_sha256 = canonical_json_sha256(&action_contract);
    let run_id = json_string(&checkpoint_status, "run_id");
    let completion_summary_sha256 = json_string(&checkpoint_status, "completion_summary_sha256");
    let prior_memory_record_id = json_string(&checkpoint_status, "prior_memory_record_id");

    let action_contract_ready = json_string(&action_contract, "status") == "ready"
        && json_string(&action_contract, "recommended_action")
            == "resume_from_latest_recovery_packet";
    let resume_receipt_ready = json_string(&receipt, "status") == "ready"
        && json_string(&receipt, "action") == "resume_from_latest_recovery_packet";
    let checkpoint_status_ready = json_string(&checkpoint_status, "status") == "ready";
    let checkpoint_is_durable = checkpoint_status["hermes_memory"]["checkpoint_is_durable"] == true
        && checkpoint_status["memory_link"]["memory_id"]
            == checkpoint_status["memory_record"]["id"];
    let exact_digest_chain_matches = action_queue_sha256 == queue_sha256
        && action_recovery_packet_sha256 == recovery_packet_sha256
        && json_string(&receipt["digest_verification"], "queue_sha256_expected") == queue_sha256
        && json_string(&receipt["digest_verification"], "queue_sha256_provided") == queue_sha256
        && json_string(
            &receipt["digest_verification"],
            "recovery_packet_sha256_expected",
        ) == recovery_packet_sha256
        && json_string(
            &receipt["digest_verification"],
            "recovery_packet_sha256_provided",
        ) == recovery_packet_sha256
        && json_string(&checkpoint_status, "queue_sha256") == queue_sha256
        && json_string(&checkpoint_status, "recovery_packet_sha256") == recovery_packet_sha256
        && json_string(&checkpoint_status, "receipt_sha256") == receipt_sha256
        && json_string(&checkpoint_status["memory_record"]["source"], "path_sha256")
            == receipt_sha256;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-continuity.v1",
        "status": "ready",
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "completion_summary_sha256": completion_summary_sha256,
        "prior_memory_record_id": prior_memory_record_id,
        "receipt_sha256": receipt_sha256,
        "action_contract_sha256": action_contract_sha256,
        "checkpoint_status_sha256": checkpoint_status_sha256,
        "checkpoint_status": checkpoint_status,
        "chain_verification": {
            "action_contract_ready": action_contract_ready,
            "resume_receipt_ready": resume_receipt_ready,
            "checkpoint_status_ready": checkpoint_status_ready,
            "checkpoint_is_durable": checkpoint_is_durable,
            "exact_digest_chain_matches": exact_digest_chain_matches,
            "queue_sha256_matches": true,
            "recovery_packet_sha256_matches": true,
            "memory_record_receipt_binding_matches": true
        },
        "continuity_packet": {
            "action_contract": action_contract,
            "resume_receipt": receipt,
            "resume_checkpoint_status": checkpoint_status,
            "digests": {
                "action_contract_sha256": action_contract_sha256,
                "resume_receipt_sha256": receipt_sha256,
                "resume_checkpoint_status_sha256": checkpoint_status_sha256,
                "queue_sha256": queue_sha256,
                "recovery_packet_sha256": recovery_packet_sha256,
                "completion_summary_sha256": completion_summary_sha256,
                "prior_memory_record_id": prior_memory_record_id
            }
        },
        "hermes_memory": {
            "single_continuity_packet_for_bookkeeping": true,
            "contains_action_contract": true,
            "contains_resume_receipt": true,
            "contains_checkpoint_status": true,
            "checkpoint_is_durable": checkpoint_is_durable,
            "resume_receipt_recorded": true,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "requires_manual_command_sequence": false,
            "next_recommended_action": "read_recovery_resume_continuity"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_execute_provider": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-workbench-recovery"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_plan_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
) -> Result<serde_json::Value> {
    let continuity = factory_queue_project_start_recovery_resume_continuity_json(
        target,
        queue_sha256,
        recovery_packet_sha256,
    )?;
    let run_id = json_string(&continuity, "run_id");
    let receipt_sha256 = json_string(&continuity, "receipt_sha256");
    let completion_summary_sha256 = json_string(&continuity, "completion_summary_sha256");
    let prior_memory_record_id = json_string(&continuity, "prior_memory_record_id");
    let checkpoint_memory_record_id =
        json_string(&continuity["checkpoint_status"]["memory_record"], "id");
    let checkpoint_run_link_matches = continuity["checkpoint_status"]["memory_link"]["memory_id"]
        == continuity["checkpoint_status"]["memory_record"]["id"];
    let continuity_packet_sha256 = canonical_json_sha256(&continuity);
    let chain = &continuity["chain_verification"];

    let mut blockers = Vec::new();
    for (code, ok) in [
        (
            "recovery_continuity_not_ready",
            continuity["status"] == "ready" && continuity["read_only"] == true,
        ),
        (
            "action_contract_not_ready",
            chain["action_contract_ready"] == true,
        ),
        (
            "resume_receipt_not_ready",
            chain["resume_receipt_ready"] == true,
        ),
        (
            "checkpoint_status_not_ready",
            chain["checkpoint_status_ready"] == true,
        ),
        (
            "checkpoint_not_durable",
            chain["checkpoint_is_durable"] == true,
        ),
        (
            "digest_chain_mismatch",
            chain["exact_digest_chain_matches"] == true,
        ),
        ("checkpoint_run_link_mismatch", checkpoint_run_link_matches),
    ] {
        if !ok {
            blockers.push(serde_json::json!({
                "code": code,
                "severity": "blocker",
                "message": "Recovery resume plan is blocked until the C58 continuity chain is proven."
            }));
        }
    }

    let status = if blockers.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    let concerns = if blockers.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "code": "operator_review_required",
            "severity": "high",
            "message": "Hermes must not execute the recovery plan until AO2 reports ready continuity."
        })]
    };
    let classification = serde_json::json!({
        "size": "bounded",
        "shape": "bug-fix",
        "reason": "Recovery resume continues a previously interrupted governed project-start workflow without creating a new product surface."
    });
    let governed_recovery_resume_plan = serde_json::json!({
        "action": "resume_from_latest_recovery_packet",
        "selected_run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "receipt_sha256": receipt_sha256,
        "completion_summary_sha256": completion_summary_sha256,
        "prior_memory_record_id": prior_memory_record_id,
        "checkpoint_memory_record_id": checkpoint_memory_record_id,
        "checkpoint_run_link_matches": checkpoint_run_link_matches,
        "continuity_packet_sha256": continuity_packet_sha256,
        "factory_v3_role": "parity_oracle_and_evaluator_closer",
        "hermes_role": "front_end_scheduler_queue_memory_bookkeeping",
        "control_plane_role": "read_only_observer"
    });
    let plan_body = serde_json::json!({
        "classification": classification,
        "governed_recovery_resume_plan": governed_recovery_resume_plan,
        "evidence": [
            {
                "kind": "recovery_continuity_packet",
                "schema_version": continuity["schema_version"].clone(),
                "sha256": continuity_packet_sha256,
                "status": continuity["status"].clone()
            },
            {
                "kind": "checkpoint_memory_record",
                "id": checkpoint_memory_record_id,
                "receipt_sha256": receipt_sha256
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary()
    });
    let plan_sha256 = canonical_json_sha256(&plan_body);

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-plan.v1",
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "receipt_sha256": receipt_sha256,
        "completion_summary_sha256": completion_summary_sha256,
        "prior_memory_record_id": prior_memory_record_id,
        "checkpoint_memory_record_id": checkpoint_memory_record_id,
        "continuity_packet_sha256": continuity_packet_sha256,
        "plan_digest_bound": true,
        "plan_sha256": plan_sha256,
        "classification": plan_body["classification"].clone(),
        "governed_recovery_resume_plan": plan_body["governed_recovery_resume_plan"].clone(),
        "evidence": plan_body["evidence"].clone(),
        "concerns": plan_body["concerns"].clone(),
        "blockers": plan_body["blockers"].clone(),
        "continuity_packet": continuity,
        "hermes_memory": {
            "single_governed_recovery_resume_plan_for_bookkeeping": true,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "requires_manual_command_sequence": false,
            "next_recommended_action": "execute_governed_recovery_resume_plan_after_operator_review"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_execute_provider": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-workbench-recovery-planner"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_claim_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    approve_plan_sha256: Option<&str>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let plan = factory_queue_project_start_recovery_resume_plan_json(
        &target_root,
        queue_sha256,
        recovery_packet_sha256,
    )?;
    let plan_sha256 = json_string(&plan, "plan_sha256");
    let run_id = json_string(&plan, "run_id");
    let checkpoint_memory_record_id = json_string(&plan, "checkpoint_memory_record_id");
    let trust_boundary = factory_project_start_completion_summary_memory_trust_boundary();
    let submitted_digest = approve_plan_sha256.unwrap_or("").trim();

    if json_string(&plan, "status") != "ready"
        || plan
            .get("blockers")
            .and_then(|value| value.as_array())
            .is_some_and(|blockers| !blockers.is_empty())
    {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-claim.v1",
            "status": "blocked",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "approved_plan_sha256": submitted_digest,
            "plan": plan,
            "evidence": [],
            "concerns": [{
                "code": "recovery_resume_plan_not_ready",
                "severity": "high",
                "message": "AO2 refused to claim a recovery resume plan that is not ready."
            }],
            "blockers": [{
                "code": "recovery_resume_plan_not_ready",
                "severity": "blocker",
                "message": "Run the recovery resume plan readback and resolve its blockers before claiming."
            }],
            "changed_files": [],
            "side_effects": {
                "wrote_memory_record": false,
                "wrote_memory_run_link": false,
                "executed_provider": false,
                "executed_queue": false,
                "submitted_queue_entry": false,
                "wrote_queue_file": false,
                "mutated_control_plane": false,
                "approved_release": false
            },
            "trust_boundary": trust_boundary,
            "ao2_decision_owner": "ao2-workbench-recovery-claim"
        }));
    }

    if submitted_digest != plan_sha256 {
        let blocker_code = if submitted_digest.is_empty() {
            "operator_plan_digest_approval_required"
        } else {
            "plan_digest_mismatch"
        };
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-claim-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_plan_sha256",
            "required_flag": "--approve-plan-sha256",
            "required_form_field": "approval_plan_sha256",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "submitted_plan_sha256": submitted_digest,
            "plan": plan,
            "evidence": [{
                "kind": "recovery_resume_plan",
                "schema_version": "ao2.factory-project-start-recovery-resume-plan.v1",
                "sha256": plan_sha256,
                "status": "ready"
            }],
            "concerns": [{
                "code": "operator_review_required",
                "severity": "high",
                "message": "AO2 requires an exact plan digest before recording a recovery resume claim."
            }],
            "blockers": [{
                "code": blocker_code,
                "severity": "blocker",
                "message": "Submit the exact C59 plan_sha256 to allow AO2 to record the bounded recovery claim."
            }],
            "next_action": "submit approval_plan_sha256 or --approve-plan-sha256 with the exact plan_sha256 to record the AO2 recovery resume claim",
            "side_effects": {
                "would_write_memory_after_approval": true,
                "would_write_memory_run_link_after_approval": true,
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_submit_queue_entry": false,
                "would_mutate_control_plane": false,
                "would_write_queue_file": false,
                "would_approve_release": false
            },
            "trust_boundary": trust_boundary,
            "ao2_decision_owner": "ao2-workbench-recovery-claim"
        }));
    }

    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records_sha256_before = if records_path.is_file() {
        Some(sha256_file(&records_path)?)
    } else {
        None
    };
    let links_sha256_before = if links_path.is_file() {
        Some(sha256_file(&links_path)?)
    } else {
        None
    };
    let body = format!(
        "Project-start recovery resume claim recorded for run_id={run_id}\nplan_sha256={plan_sha256}\nqueue_sha256={queue_sha256}\nrecovery_packet_sha256={recovery_packet_sha256}\ncheckpoint_memory_record_id={checkpoint_memory_record_id}"
    );
    let mut memory_record = memory_write_record_json(
        &target_root,
        "project-start-recovery-resume-claim".to_string(),
        format!("Project-start recovery resume claim: {run_id}"),
        body,
        vec![
            "hermes".to_string(),
            "ao2".to_string(),
            "project-start".to_string(),
            "recovery".to_string(),
            "resume-claim".to_string(),
        ],
        Some(run_id.clone()),
        None,
    )?;
    memory_record["source"]["path"] =
        serde_json::json!("inline:ao2.factory-project-start-recovery-resume-plan.v1");
    memory_record["source"]["path_sha256"] = serde_json::json!(plan_sha256.clone());
    append_jsonl(&records_path, &memory_record)?;
    let memory_link = memory_link_run_json(
        &target_root,
        json_string(&memory_record, "id"),
        run_id.clone(),
        "project-start-recovery-resume-claim".to_string(),
    )?;
    append_jsonl(&links_path, &memory_link)?;
    let records_sha256_after = sha256_file(&records_path)?;
    let links_sha256_after = sha256_file(&links_path)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-claim.v1",
        "status": "claimed",
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "approved_plan_sha256": plan_sha256,
        "plan_sha256": plan_sha256,
        "checkpoint_memory_record_id": checkpoint_memory_record_id,
        "plan": plan,
        "approval": {
            "schema_version": "ao2.factory-project-start-recovery-resume-claim-approval.v1",
            "status": "approved_exact_plan_sha256",
            "approval_mode": "exact_plan_sha256",
            "plan_sha256": plan_sha256
        },
        "evidence": [
            {
                "kind": "recovery_resume_plan",
                "schema_version": "ao2.factory-project-start-recovery-resume-plan.v1",
                "sha256": plan_sha256,
                "status": "ready"
            },
            {
                "kind": "recovery_resume_claim_memory_record",
                "id": json_string(&memory_record, "id"),
                "plan_sha256": plan_sha256
            }
        ],
        "concerns": [],
        "blockers": [],
        "memory_record": memory_record,
        "memory_link": memory_link,
        "changed_files": [
            {
                "path": records_path.display().to_string(),
                "sha256_before": records_sha256_before,
                "sha256_after": records_sha256_after,
                "reason": "recorded AO2 recovery resume claim under approved plan digest"
            },
            {
                "path": links_path.display().to_string(),
                "sha256_before": links_sha256_before,
                "sha256_after": links_sha256_after,
                "reason": "linked AO2 recovery resume claim memory record to run"
            }
        ],
        "hermes_memory": {
            "single_claim_record_for_bookkeeping": true,
            "claim_bound_to_plan_sha256": true,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "read_recovery_resume_claim_evidence"
        },
        "side_effects": {
            "wrote_memory_record": true,
            "wrote_memory_run_link": true,
            "executed_provider": false,
            "executed_queue": false,
            "submitted_queue_entry": false,
            "wrote_queue_file": false,
            "mutated_control_plane": false,
            "approved_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-recovery-claim"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_claim_status_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let plan = factory_queue_project_start_recovery_resume_plan_json(
        &target_root,
        queue_sha256,
        recovery_packet_sha256,
    )?;
    let expected_plan_sha256 = json_string(&plan, "plan_sha256");
    let supplied_plan_sha256 = plan_sha256.trim().to_string();
    let run_id = json_string(&plan, "run_id");
    let checkpoint_memory_record_id = json_string(&plan, "checkpoint_memory_record_id");
    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records = read_jsonl_values(&records_path)?;
    let links = read_jsonl_values(&links_path)?;

    let run_claim_records = records
        .iter()
        .filter(|record| {
            record["schema_version"] == "ao2.memory-record.v1"
                && record["kind"] == "project-start-recovery-resume-claim"
                && json_string(&record["source"], "run_id") == run_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let matching_claim_records = run_claim_records
        .iter()
        .filter(|record| json_string(&record["source"], "path_sha256") == supplied_plan_sha256)
        .cloned()
        .collect::<Vec<_>>();
    let claim_memory_record = matching_claim_records
        .first()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let claim_memory_id = json_string(&claim_memory_record, "id");
    let matching_claim_links = links
        .iter()
        .filter(|link| {
            link["schema_version"] == "ao2.memory-run-link.v1"
                && json_string(link, "memory_id") == claim_memory_id
                && json_string(link, "run_id") == run_id
                && json_string(link, "relationship") == "project-start-recovery-resume-claim"
        })
        .cloned()
        .collect::<Vec<_>>();
    let claim_memory_link = matching_claim_links
        .first()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let claim_body = json_string(&claim_memory_record, "body");
    let plan_sha256_matches = supplied_plan_sha256 == expected_plan_sha256;
    let claim_record_is_unique = matching_claim_records.len() == 1;
    let claim_run_link_is_unique = matching_claim_links.len() == 1;
    let claim_source_binds_approved_plan =
        json_string(&claim_memory_record["source"], "path_sha256") == expected_plan_sha256;
    let claim_body_binds_plan = claim_body.contains(&format!("plan_sha256={expected_plan_sha256}"))
        && claim_body.contains(&format!("queue_sha256={queue_sha256}"))
        && claim_body.contains(&format!("recovery_packet_sha256={recovery_packet_sha256}"))
        && claim_body.contains(&format!(
            "checkpoint_memory_record_id={checkpoint_memory_record_id}"
        ));
    let claim_link_targets_record = !claim_memory_id.is_empty()
        && json_string(&claim_memory_link, "memory_id") == claim_memory_id;

    let mut blockers = Vec::new();
    if !plan_sha256_matches {
        blockers.push(serde_json::json!({
            "code": "plan_digest_mismatch",
            "severity": "blocker",
            "message": "Supplied plan_sha256 does not match the recomputed C59 recovery resume plan."
        }));
    }
    if matching_claim_records.is_empty() {
        blockers.push(serde_json::json!({
            "code": "missing_recovery_resume_claim_record",
            "severity": "blocker",
            "message": "No C60 recovery resume claim memory record is bound to the approved C59 plan digest."
        }));
    } else if matching_claim_records.len() > 1 {
        blockers.push(serde_json::json!({
            "code": "duplicate_recovery_resume_claim_records",
            "severity": "blocker",
            "message": "Multiple C60 recovery resume claim memory records match the same approved C59 plan digest."
        }));
    }
    if matching_claim_links.is_empty() {
        blockers.push(serde_json::json!({
            "code": "missing_recovery_resume_claim_run_link",
            "severity": "blocker",
            "message": "No C60 recovery resume claim memory run link targets the selected run."
        }));
    } else if matching_claim_links.len() > 1 {
        blockers.push(serde_json::json!({
            "code": "duplicate_recovery_resume_claim_run_links",
            "severity": "blocker",
            "message": "Multiple C60 recovery resume claim memory run links target the selected run."
        }));
    }
    if !matching_claim_records.is_empty() && !claim_source_binds_approved_plan {
        blockers.push(serde_json::json!({
            "code": "claim_source_plan_digest_mismatch",
            "severity": "blocker",
            "message": "The C60 recovery resume claim source digest is not bound to the approved C59 plan digest."
        }));
    }
    if !matching_claim_records.is_empty() && !claim_body_binds_plan {
        blockers.push(serde_json::json!({
            "code": "claim_body_digest_binding_mismatch",
            "severity": "blocker",
            "message": "The C60 recovery resume claim body does not replay the expected plan, queue, recovery packet, and checkpoint digests."
        }));
    }
    if !matching_claim_links.is_empty() && !claim_link_targets_record {
        blockers.push(serde_json::json!({
            "code": "claim_run_link_record_mismatch",
            "severity": "blocker",
            "message": "The C60 recovery resume claim run link does not target the selected memory record."
        }));
    }
    let status = if blockers.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    let concerns = if blockers.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "code": "claim_replay_not_trusted",
            "severity": "high",
            "message": "Hermes must not continue governed recovery until AO2 reports a unique replayable C60 claim."
        })]
    };
    let records_sha256 = if records_path.is_file() {
        serde_json::json!(sha256_file(&records_path)?)
    } else {
        serde_json::Value::Null
    };
    let links_sha256 = if links_path.is_file() {
        serde_json::json!(sha256_file(&links_path)?)
    } else {
        serde_json::Value::Null
    };
    let workbench_restart_replayable = blockers.is_empty();

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-claim-status.v1",
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": supplied_plan_sha256,
        "expected_plan_sha256": expected_plan_sha256,
        "approved_plan_sha256": supplied_plan_sha256,
        "checkpoint_memory_record_id": checkpoint_memory_record_id,
        "claim_record_count": matching_claim_records.len(),
        "claim_link_count": matching_claim_links.len(),
        "all_claim_record_count_for_run": run_claim_records.len(),
        "claim_memory_record": claim_memory_record,
        "claim_memory_link": claim_memory_link,
        "plan": plan,
        "replay_verification": {
            "plan_sha256_matches": plan_sha256_matches,
            "claim_record_is_unique": claim_record_is_unique,
            "claim_run_link_is_unique": claim_run_link_is_unique,
            "claim_source_binds_approved_plan": claim_source_binds_approved_plan,
            "claim_body_binds_plan": claim_body_binds_plan,
            "claim_link_targets_record": claim_link_targets_record,
            "workbench_restart_replayable": workbench_restart_replayable
        },
        "evidence": [
            {
                "kind": "recovery_resume_plan",
                "schema_version": "ao2.factory-project-start-recovery-resume-plan.v1",
                "sha256": expected_plan_sha256,
                "status": plan["status"].clone()
            },
            {
                "kind": "recovery_resume_claim_memory_record",
                "id": json_string(&matching_claim_records.first().cloned().unwrap_or(serde_json::Value::Null), "id"),
                "count": matching_claim_records.len(),
                "approved_plan_sha256": supplied_plan_sha256
            },
            {
                "kind": "recovery_resume_claim_memory_run_link",
                "count": matching_claim_links.len(),
                "relationship": "project-start-recovery-resume-claim"
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": [
            {
                "path": records_path.display().to_string(),
                "sha256": records_sha256,
                "role": "durable C60 claim memory record store",
                "observed_after_claim": true
            },
            {
                "path": links_path.display().to_string(),
                "sha256": links_sha256,
                "role": "durable C60 claim run-link store",
                "observed_after_claim": true
            }
        ],
        "memory_paths": {
            "records_jsonl": records_path.display().to_string(),
            "run_links_jsonl": links_path.display().to_string()
        },
        "hermes_memory": {
            "single_claim_status_packet_for_bookkeeping": true,
            "claim_bound_to_plan_sha256": claim_source_binds_approved_plan,
            "workbench_restart_replayable": workbench_restart_replayable,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "observe_recovery_resume_claim_status_then_continue_governed_recovery"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-workbench-recovery-claim-status"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_continuation_contract_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
    claim_status_sha256: &str,
) -> Result<serde_json::Value> {
    let claim_status = factory_queue_project_start_recovery_resume_claim_status_json(
        target,
        queue_sha256,
        recovery_packet_sha256,
        plan_sha256,
    )?;
    let actual_claim_status_sha256 = canonical_json_sha256(&claim_status);
    let supplied_claim_status_sha256 = claim_status_sha256.trim().to_string();
    let claim_status_digest_matches = supplied_claim_status_sha256 == actual_claim_status_sha256;
    let claim_status_ready = claim_status["status"] == "ready" && claim_status["read_only"] == true;
    let claim_status_blockers = claim_status
        .get("blockers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut blockers = Vec::new();
    if !claim_status_digest_matches {
        blockers.push(serde_json::json!({
            "code": "claim_status_digest_mismatch",
            "severity": "blocker",
            "message": "Supplied claim_status_sha256 does not match the recomputed C61 claim-status packet."
        }));
    }
    if !claim_status_ready {
        blockers.push(serde_json::json!({
            "code": "claim_status_not_ready",
            "severity": "blocker",
            "message": "AO2 refused to issue a recovery resume continuation contract until C61 reports ready."
        }));
    }
    if !claim_status_blockers.is_empty() {
        blockers.push(serde_json::json!({
            "code": "claim_status_blockers_present",
            "severity": "blocker",
            "message": "C61 claim-status reported blockers; continue only after AO2 resolves duplicate, missing, or mismatched claim evidence."
        }));
    }

    let status = if blockers.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    let concerns = if blockers.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "code": "recovery_resume_continuation_blocked",
            "severity": "high",
            "message": "Hermes must not advance governed recovery until AO2 reports a digest-bound ready continuation contract."
        })]
    };
    let classification = serde_json::json!({
        "size": "bounded",
        "shape": "bug-fix",
        "reason": "Recovery resume continuation advances a previously interrupted governed workflow under exact C61 claim-status evidence."
    });
    let next_bounded_action = serde_json::json!({
        "action": "execute_recovery_resume_continuation_after_exact_status_digest_approval",
        "read_only": false,
        "mutates_queue_or_memory": true,
        "requires_exact_claim_status_sha256": true,
        "required_claim_status_sha256": actual_claim_status_sha256,
        "executor_command": "ao2 factory queue-project-start-recovery-resume-continue --approve-claim-status-sha256 <sha>"
    });
    let continuation_contract = serde_json::json!({
        "required_prior_schema_version": "ao2.factory-project-start-recovery-resume-claim-status.v1",
        "required_prior_status": "ready",
        "required_claim_status_sha256": actual_claim_status_sha256,
        "supplied_claim_status_sha256": supplied_claim_status_sha256,
        "claim_status_digest_matches": claim_status_digest_matches,
        "claim_status_ready": claim_status_ready,
        "current_contract_is_read_only": true,
        "next_bounded_action": next_bounded_action,
        "ao2_role": "trusted_queue_memory_replay_owner",
        "hermes_role": "front_end_scheduler_queue_memory_bookkeeping",
        "factory_v3_role": "parity_oracle_and_evaluator_closer",
        "control_plane_role": "read_only_observer"
    });
    let run_id = json_string(&claim_status, "run_id");
    let approved_plan_sha256 = json_string(&claim_status, "approved_plan_sha256");
    let claim_memory_id = json_string(&claim_status["claim_memory_record"], "id");

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-continuation-contract.v1",
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "approved_plan_sha256": approved_plan_sha256,
        "claim_status_sha256": supplied_claim_status_sha256,
        "expected_claim_status_sha256": actual_claim_status_sha256,
        "claim_status_digest_bound": claim_status_digest_matches,
        "classification": classification,
        "continuation_contract": continuation_contract,
        "claim_status": claim_status,
        "evidence": [
            {
                "kind": "recovery_resume_claim_status",
                "schema_version": "ao2.factory-project-start-recovery-resume-claim-status.v1",
                "sha256": actual_claim_status_sha256,
                "status": claim_status["status"].clone()
            },
            {
                "kind": "recovery_resume_claim_memory_record",
                "id": claim_memory_id,
                "approved_plan_sha256": approved_plan_sha256
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": [],
        "hermes_memory": {
            "single_continuation_contract_for_bookkeeping": true,
            "claim_status_bound_to_sha256": claim_status_digest_matches,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "submit_exact_claim_status_digest_to_ao2_recovery_resume_continuation_executor"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-workbench-recovery-continuation-contract"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_continue_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
    claim_status_sha256: &str,
    approve_claim_status_sha256: Option<&str>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let contract = factory_queue_project_start_recovery_resume_continuation_contract_json(
        &target_root,
        queue_sha256,
        recovery_packet_sha256,
        plan_sha256,
        claim_status_sha256,
    )?;
    let run_id = json_string(&contract, "run_id");
    let expected_claim_status_sha256 = json_string(&contract, "expected_claim_status_sha256");
    let supplied_claim_status_sha256 = json_string(&contract, "claim_status_sha256");
    let submitted_digest = approve_claim_status_sha256.unwrap_or("").trim();
    let trust_boundary = factory_project_start_completion_summary_memory_trust_boundary();
    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records = read_jsonl_values(&records_path)?;
    let existing_records = records
        .iter()
        .filter(|record| {
            record["schema_version"] == "ao2.memory-record.v1"
                && record["kind"] == "project-start-recovery-resume-continuation"
                && json_string(&record["source"], "run_id") == run_id
                && json_string(&record["source"], "path_sha256") == supplied_claim_status_sha256
        })
        .cloned()
        .collect::<Vec<_>>();

    if !existing_records.is_empty() && submitted_digest == supplied_claim_status_sha256 {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-continue.v1",
            "status": "blocked",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "claim_status_sha256": supplied_claim_status_sha256,
            "approved_claim_status_sha256": supplied_claim_status_sha256,
            "contract": contract,
            "continuation_record_count": existing_records.len(),
            "evidence": [{
                "kind": "recovery_resume_continuation_memory_record",
                "count": existing_records.len(),
                "approved_claim_status_sha256": supplied_claim_status_sha256
            }],
            "concerns": [{
                "code": "continuation_already_recorded",
                "severity": "high",
                "message": "AO2 refused to record a duplicate recovery resume continuation for the same C61 digest."
            }],
            "blockers": [{
                "code": "duplicate_recovery_resume_continuation_records",
                "severity": "blocker",
                "message": "A recovery resume continuation memory record already exists for the approved C61 claim-status digest."
            }],
            "changed_files": [],
            "side_effects": {
                "wrote_memory_record": false,
                "wrote_memory_run_link": false,
                "executed_provider": false,
                "executed_queue": false,
                "submitted_queue_entry": false,
                "wrote_queue_file": false,
                "mutated_control_plane": false,
                "approved_release": false
            },
            "trust_boundary": trust_boundary,
            "ao2_decision_owner": "ao2-workbench-recovery-continuation-executor"
        }));
    }

    if json_string(&contract, "status") != "ready"
        || contract
            .get("blockers")
            .and_then(|value| value.as_array())
            .is_some_and(|blockers| !blockers.is_empty())
    {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-continue.v1",
            "status": "blocked",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "claim_status_sha256": supplied_claim_status_sha256,
            "expected_claim_status_sha256": expected_claim_status_sha256,
            "contract": contract,
            "evidence": [],
            "concerns": [{
                "code": "recovery_resume_continuation_contract_not_ready",
                "severity": "high",
                "message": "AO2 refused to execute a recovery resume continuation without a ready C62 contract."
            }],
            "blockers": [{
                "code": "recovery_resume_continuation_contract_not_ready",
                "severity": "blocker",
                "message": "Run the C62 continuation contract readback and resolve blockers before continuation."
            }],
            "changed_files": [],
            "side_effects": {
                "wrote_memory_record": false,
                "wrote_memory_run_link": false,
                "executed_provider": false,
                "executed_queue": false,
                "submitted_queue_entry": false,
                "wrote_queue_file": false,
                "mutated_control_plane": false,
                "approved_release": false
            },
            "trust_boundary": trust_boundary,
            "ao2_decision_owner": "ao2-workbench-recovery-continuation-executor"
        }));
    }

    if submitted_digest != expected_claim_status_sha256 {
        let blocker_code = if submitted_digest.is_empty() {
            "operator_claim_status_digest_approval_required"
        } else {
            "claim_status_approval_digest_mismatch"
        };
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-continue-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_claim_status_sha256",
            "required_flag": "--approve-claim-status-sha256",
            "required_form_field": "approval_claim_status_sha256",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "claim_status_sha256": supplied_claim_status_sha256,
            "expected_claim_status_sha256": expected_claim_status_sha256,
            "submitted_claim_status_sha256": submitted_digest,
            "contract": contract,
            "evidence": [{
                "kind": "recovery_resume_continuation_contract",
                "schema_version": "ao2.factory-project-start-recovery-resume-continuation-contract.v1",
                "sha256": canonical_json_sha256(&contract),
                "status": "ready"
            }],
            "concerns": [{
                "code": "operator_review_required",
                "severity": "high",
                "message": "AO2 requires the exact C61 claim-status digest before executing recovery resume continuation."
            }],
            "blockers": [{
                "code": blocker_code,
                "severity": "blocker",
                "message": "Submit the exact C61 claim_status_sha256 to allow AO2 to record the bounded continuation."
            }],
            "next_action": "submit approval_claim_status_sha256 or --approve-claim-status-sha256 with the exact claim_status_sha256 to execute AO2 recovery resume continuation",
            "side_effects": {
                "would_write_memory_after_approval": true,
                "would_write_memory_run_link_after_approval": true,
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_submit_queue_entry": false,
                "would_mutate_control_plane": false,
                "would_write_queue_file": false,
                "would_approve_release": false
            },
            "trust_boundary": trust_boundary,
            "ao2_decision_owner": "ao2-workbench-recovery-continuation-executor"
        }));
    }

    let records_sha256_before = if records_path.is_file() {
        Some(sha256_file(&records_path)?)
    } else {
        None
    };
    let links_sha256_before = if links_path.is_file() {
        Some(sha256_file(&links_path)?)
    } else {
        None
    };
    let claim_memory_id = json_string(&contract["claim_status"]["claim_memory_record"], "id");
    let body = format!(
        "Project-start recovery resume continuation recorded for run_id={run_id}\nclaim_status_sha256={expected_claim_status_sha256}\nplan_sha256={plan_sha256}\nqueue_sha256={queue_sha256}\nrecovery_packet_sha256={recovery_packet_sha256}\nclaim_memory_record_id={claim_memory_id}"
    );
    let mut continuation_memory_record = memory_write_record_json(
        &target_root,
        "project-start-recovery-resume-continuation".to_string(),
        format!("Project-start recovery resume continuation: {run_id}"),
        body,
        vec![
            "hermes".to_string(),
            "ao2".to_string(),
            "project-start".to_string(),
            "recovery".to_string(),
            "resume-continuation".to_string(),
        ],
        Some(run_id.clone()),
        None,
    )?;
    continuation_memory_record["source"]["path"] =
        serde_json::json!("inline:ao2.factory-project-start-recovery-resume-claim-status.v1");
    continuation_memory_record["source"]["path_sha256"] =
        serde_json::json!(expected_claim_status_sha256.clone());
    append_jsonl(&records_path, &continuation_memory_record)?;
    let continuation_memory_link = memory_link_run_json(
        &target_root,
        json_string(&continuation_memory_record, "id"),
        run_id.clone(),
        "project-start-recovery-resume-continuation".to_string(),
    )?;
    append_jsonl(&links_path, &continuation_memory_link)?;
    let records_sha256_after = sha256_file(&records_path)?;
    let links_sha256_after = sha256_file(&links_path)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-continue.v1",
        "status": "continued",
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "claim_status_sha256": expected_claim_status_sha256,
        "approved_claim_status_sha256": expected_claim_status_sha256,
        "contract": contract,
        "approval": {
            "schema_version": "ao2.factory-project-start-recovery-resume-continue-approval.v1",
            "status": "approved_exact_claim_status_sha256",
            "approval_mode": "exact_claim_status_sha256",
            "claim_status_sha256": expected_claim_status_sha256
        },
        "evidence": [
            {
                "kind": "recovery_resume_continuation_contract",
                "schema_version": "ao2.factory-project-start-recovery-resume-continuation-contract.v1",
                "sha256": canonical_json_sha256(&contract),
                "status": "ready"
            },
            {
                "kind": "recovery_resume_continuation_memory_record",
                "id": json_string(&continuation_memory_record, "id"),
                "claim_status_sha256": expected_claim_status_sha256
            }
        ],
        "concerns": [],
        "blockers": [],
        "continuation_memory_record": continuation_memory_record,
        "continuation_memory_link": continuation_memory_link,
        "changed_files": [
            {
                "path": records_path.display().to_string(),
                "sha256_before": records_sha256_before,
                "sha256_after": records_sha256_after,
                "reason": "recorded AO2 recovery resume continuation under approved claim-status digest"
            },
            {
                "path": links_path.display().to_string(),
                "sha256_before": links_sha256_before,
                "sha256_after": links_sha256_after,
                "reason": "linked AO2 recovery resume continuation memory record to run"
            }
        ],
        "hermes_memory": {
            "single_continuation_record_for_bookkeeping": true,
            "continuation_bound_to_claim_status_sha256": true,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "read_recovery_resume_continuation_evidence"
        },
        "side_effects": {
            "wrote_memory_record": true,
            "wrote_memory_run_link": true,
            "executed_provider": false,
            "executed_queue": false,
            "submitted_queue_entry": false,
            "wrote_queue_file": false,
            "mutated_control_plane": false,
            "approved_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-recovery-continuation-executor"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_continuation_status_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
    claim_status_sha256: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let claim_status = factory_queue_project_start_recovery_resume_claim_status_json(
        &target_root,
        queue_sha256,
        recovery_packet_sha256,
        plan_sha256,
    )?;
    let current_claim_status_sha256 = canonical_json_sha256(&claim_status);
    let run_id = json_string(&claim_status, "run_id");
    let expected_claim_status_sha256 = claim_status_sha256.trim().to_string();
    let supplied_claim_status_sha256 = claim_status_sha256.trim().to_string();
    let claim_memory_id = json_string(&claim_status["claim_memory_record"], "id");
    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records = read_jsonl_values(&records_path)?;
    let links = read_jsonl_values(&links_path)?;

    let run_continuation_records = records
        .iter()
        .filter(|record| {
            record["schema_version"] == "ao2.memory-record.v1"
                && record["kind"] == "project-start-recovery-resume-continuation"
                && json_string(&record["source"], "run_id") == run_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let matching_continuation_records = run_continuation_records
        .iter()
        .filter(|record| {
            json_string(&record["source"], "path_sha256") == supplied_claim_status_sha256
        })
        .cloned()
        .collect::<Vec<_>>();
    let continuation_memory_record = matching_continuation_records
        .first()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let continuation_memory_id = json_string(&continuation_memory_record, "id");
    let matching_continuation_links = links
        .iter()
        .filter(|link| {
            link["schema_version"] == "ao2.memory-run-link.v1"
                && json_string(link, "memory_id") == continuation_memory_id
                && json_string(link, "run_id") == run_id
                && json_string(link, "relationship") == "project-start-recovery-resume-continuation"
        })
        .cloned()
        .collect::<Vec<_>>();
    let continuation_memory_link = matching_continuation_links
        .first()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let continuation_body = json_string(&continuation_memory_record, "body");
    let claim_status_current_sha256_matches_approved =
        supplied_claim_status_sha256 == current_claim_status_sha256;
    let claim_status_replay_ready =
        claim_status["status"] == "ready" && claim_status["read_only"] == true;
    let continuation_record_is_unique = matching_continuation_records.len() == 1;
    let continuation_run_link_is_unique = matching_continuation_links.len() == 1;
    let continuation_source_binds_claim_status =
        json_string(&continuation_memory_record["source"], "path_sha256")
            == expected_claim_status_sha256;
    let continuation_body_binds_claim_status = continuation_body.contains(&format!(
        "claim_status_sha256={expected_claim_status_sha256}"
    )) && continuation_body
        .contains(&format!("plan_sha256={plan_sha256}"))
        && continuation_body.contains(&format!("queue_sha256={queue_sha256}"))
        && continuation_body.contains(&format!("recovery_packet_sha256={recovery_packet_sha256}"))
        && continuation_body.contains(&format!("claim_memory_record_id={claim_memory_id}"));
    let continuation_link_targets_record = !continuation_memory_id.is_empty()
        && json_string(&continuation_memory_link, "memory_id") == continuation_memory_id;

    let mut blockers = Vec::new();
    if !claim_status_replay_ready {
        blockers.push(serde_json::json!({
            "code": "claim_status_replay_not_ready",
            "severity": "blocker",
            "message": "AO2 refused to accept C63 continuation evidence until the selected claim-status replay is ready."
        }));
    }
    if matching_continuation_records.is_empty() {
        blockers.push(serde_json::json!({
            "code": "missing_recovery_resume_continuation_record",
            "severity": "blocker",
            "message": "No C63 recovery resume continuation memory record is bound to the approved C61 claim-status digest."
        }));
    } else if matching_continuation_records.len() > 1 {
        blockers.push(serde_json::json!({
            "code": "duplicate_recovery_resume_continuation_records",
            "severity": "blocker",
            "message": "Multiple C63 recovery resume continuation memory records match the same approved C61 claim-status digest."
        }));
    }
    if matching_continuation_links.is_empty() {
        blockers.push(serde_json::json!({
            "code": "missing_recovery_resume_continuation_run_link",
            "severity": "blocker",
            "message": "No C63 recovery resume continuation memory run link targets the selected run."
        }));
    } else if matching_continuation_links.len() > 1 {
        blockers.push(serde_json::json!({
            "code": "duplicate_recovery_resume_continuation_run_links",
            "severity": "blocker",
            "message": "Multiple C63 recovery resume continuation memory run links target the selected run."
        }));
    }
    if !matching_continuation_records.is_empty() && !continuation_source_binds_claim_status {
        blockers.push(serde_json::json!({
            "code": "continuation_source_claim_status_digest_mismatch",
            "severity": "blocker",
            "message": "The C63 recovery resume continuation source digest is not bound to the approved C61 claim-status digest."
        }));
    }
    if !matching_continuation_records.is_empty() && !continuation_body_binds_claim_status {
        blockers.push(serde_json::json!({
            "code": "continuation_body_digest_binding_mismatch",
            "severity": "blocker",
            "message": "The C63 recovery resume continuation body does not replay the expected claim-status, plan, queue, recovery packet, and claim-memory digests."
        }));
    }
    if !matching_continuation_links.is_empty() && !continuation_link_targets_record {
        blockers.push(serde_json::json!({
            "code": "continuation_run_link_record_mismatch",
            "severity": "blocker",
            "message": "The C63 recovery resume continuation run link does not target the selected memory record."
        }));
    }

    let status = if blockers.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    let concerns = if blockers.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "code": "continuation_replay_not_trusted",
            "severity": "high",
            "message": "Hermes must not advance governed recovery bookkeeping until AO2 reports a unique replayable C63 continuation."
        })]
    };
    let records_sha256 = if records_path.is_file() {
        serde_json::json!(sha256_file(&records_path)?)
    } else {
        serde_json::Value::Null
    };
    let links_sha256 = if links_path.is_file() {
        serde_json::json!(sha256_file(&links_path)?)
    } else {
        serde_json::Value::Null
    };
    let workbench_restart_replayable = blockers.is_empty();

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-continuation-status.v1",
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "claim_status_sha256": supplied_claim_status_sha256,
        "expected_claim_status_sha256": expected_claim_status_sha256,
        "approved_claim_status_sha256": supplied_claim_status_sha256,
        "continuation_record_count": matching_continuation_records.len(),
        "continuation_link_count": matching_continuation_links.len(),
        "all_continuation_record_count_for_run": run_continuation_records.len(),
        "continuation_memory_record": continuation_memory_record,
        "continuation_memory_link": continuation_memory_link,
        "claim_status": claim_status,
        "replay_verification": {
            "claim_status_current_sha256_matches_approved": claim_status_current_sha256_matches_approved,
            "claim_status_replay_ready": claim_status_replay_ready,
            "continuation_record_is_unique": continuation_record_is_unique,
            "continuation_run_link_is_unique": continuation_run_link_is_unique,
            "continuation_source_binds_claim_status": continuation_source_binds_claim_status,
            "continuation_body_binds_claim_status": continuation_body_binds_claim_status,
            "continuation_link_targets_record": continuation_link_targets_record,
            "workbench_restart_replayable": workbench_restart_replayable
        },
        "evidence": [
            {
                "kind": "recovery_resume_claim_status_replay",
                "schema_version": "ao2.factory-project-start-recovery-resume-claim-status.v1",
                "sha256": current_claim_status_sha256,
                "approved_claim_status_sha256": supplied_claim_status_sha256,
                "status": claim_status["status"].clone()
            },
            {
                "kind": "recovery_resume_continuation_memory_record",
                "id": json_string(&matching_continuation_records.first().cloned().unwrap_or(serde_json::Value::Null), "id"),
                "count": matching_continuation_records.len(),
                "approved_claim_status_sha256": supplied_claim_status_sha256
            },
            {
                "kind": "recovery_resume_continuation_memory_run_link",
                "count": matching_continuation_links.len(),
                "relationship": "project-start-recovery-resume-continuation"
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": [
            {
                "path": records_path.display().to_string(),
                "sha256": records_sha256,
                "role": "durable C63 continuation memory record store",
                "observed_after_continuation": true
            },
            {
                "path": links_path.display().to_string(),
                "sha256": links_sha256,
                "role": "durable C63 continuation run-link store",
                "observed_after_continuation": true
            }
        ],
        "memory_paths": {
            "records_jsonl": records_path.display().to_string(),
            "run_links_jsonl": links_path.display().to_string()
        },
        "hermes_memory": {
            "single_continuation_status_packet_for_bookkeeping": true,
            "continuation_bound_to_claim_status_sha256": continuation_source_binds_claim_status,
            "workbench_restart_replayable": workbench_restart_replayable,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "observe_recovery_resume_continuation_status_then_continue_governed_recovery"
        },
        "side_effects": {
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": factory_project_start_completion_summary_memory_trust_boundary(),
        "ao2_decision_owner": "ao2-workbench-recovery-continuation-status"
    }))
}
