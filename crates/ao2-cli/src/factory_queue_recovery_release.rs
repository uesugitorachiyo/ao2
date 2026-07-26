use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{
    atomic_write_text, canonical_json_sha256, create_tar_gz, json_string, now_unix_ms,
    sanitize_greenfield_id, sha256_file,
};
use crate::factory_compat::factory_ensure_target_repo;
use crate::factory_queue::factory_project_start_completion_summary_memory_trust_boundary;
use crate::factory_queue_recovery::factory_queue_project_start_recovery_resume_continuation_status_json;
use crate::memory_store::{
    append_jsonl, memory_link_run_json, memory_records_path, memory_run_links_path,
    memory_write_record_json, read_jsonl_values,
};
use crate::release_comparison::checksum_manifest_map;
use crate::release_crypto::{
    derive_public_key_from_private_key, extract_tar_gz, sign_file_with_private_key,
    verify_file_signature,
};

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_action_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
    claim_status_sha256: &str,
    continuation_status_sha256: &str,
) -> Result<serde_json::Value> {
    let continuation_status = factory_queue_project_start_recovery_resume_continuation_status_json(
        target,
        queue_sha256,
        recovery_packet_sha256,
        plan_sha256,
        claim_status_sha256,
    )?;
    let actual_continuation_status_sha256 = canonical_json_sha256(&continuation_status);
    let supplied_continuation_status_sha256 = continuation_status_sha256.trim().to_string();
    let continuation_status_digest_matches =
        supplied_continuation_status_sha256 == actual_continuation_status_sha256;
    let continuation_status_ready =
        continuation_status["status"] == "ready" && continuation_status["read_only"] == true;
    let continuation_status_blockers = continuation_status
        .get("blockers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut blockers = Vec::new();
    if !continuation_status_digest_matches {
        blockers.push(serde_json::json!({
            "code": "continuation_status_digest_mismatch",
            "severity": "blocker",
            "message": "Supplied continuation_status_sha256 does not match the recomputed C64 continuation-status packet."
        }));
    }
    if !continuation_status_ready {
        blockers.push(serde_json::json!({
            "code": "continuation_status_not_ready",
            "severity": "blocker",
            "message": "AO2 refused to issue a post-continuation next-action contract until C64 reports ready."
        }));
    }
    if !continuation_status_blockers.is_empty() {
        blockers.push(serde_json::json!({
            "code": "continuation_status_blockers_present",
            "severity": "blocker",
            "message": "C64 continuation-status reported blockers; continue only after AO2 resolves duplicate, missing, or mismatched continuation evidence."
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
            "code": "recovery_resume_post_continuation_action_blocked",
            "severity": "high",
            "message": "Hermes must not advance governed recovery until AO2 reports a digest-bound ready post-continuation action contract."
        })]
    };
    let classification = serde_json::json!({
        "size": "bounded",
        "shape": "bug-fix",
        "reason": "Post-continuation recovery advances a previously interrupted governed workflow under exact C64 continuation-status evidence."
    });
    let next_bounded_action = serde_json::json!({
        "action": "resume_governed_project_start_after_continuation_status",
        "read_only": false,
        "mutates_queue_or_memory": true,
        "requires_exact_continuation_status_sha256": true,
        "required_continuation_status_sha256": actual_continuation_status_sha256,
        "executor_command": "ao2 factory queue-project-start-recovery-resume-post-continuation-execute --approve-continuation-status-sha256 <sha>"
    });
    let post_continuation_action = serde_json::json!({
        "required_prior_schema_version": "ao2.factory-project-start-recovery-resume-continuation-status.v1",
        "required_prior_status": "ready",
        "required_continuation_status_sha256": actual_continuation_status_sha256,
        "supplied_continuation_status_sha256": supplied_continuation_status_sha256,
        "continuation_status_digest_matches": continuation_status_digest_matches,
        "continuation_status_ready": continuation_status_ready,
        "current_contract_is_read_only": true,
        "next_bounded_action": next_bounded_action,
        "ao2_role": "trusted_queue_memory_replay_owner",
        "hermes_role": "front_end_scheduler_queue_memory_bookkeeping",
        "factory_v3_role": "parity_oracle_and_evaluator_closer",
        "control_plane_role": "read_only_observer"
    });
    let run_id = json_string(&continuation_status, "run_id");
    let continuation_memory_id =
        json_string(&continuation_status["continuation_memory_record"], "id");

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-action.v1",
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "claim_status_sha256": claim_status_sha256,
        "continuation_status_sha256": supplied_continuation_status_sha256,
        "expected_continuation_status_sha256": actual_continuation_status_sha256,
        "continuation_status_digest_bound": continuation_status_digest_matches,
        "classification": classification,
        "post_continuation_action": post_continuation_action,
        "continuation_status": continuation_status,
        "evidence": [
            {
                "kind": "recovery_resume_continuation_status",
                "schema_version": "ao2.factory-project-start-recovery-resume-continuation-status.v1",
                "sha256": actual_continuation_status_sha256,
                "status": continuation_status["status"].clone()
            },
            {
                "kind": "recovery_resume_continuation_memory_record",
                "id": continuation_memory_id,
                "approved_claim_status_sha256": continuation_status["approved_claim_status_sha256"].clone()
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": [],
        "hermes_memory": {
            "single_post_continuation_action_for_bookkeeping": true,
            "continuation_status_bound_to_sha256": continuation_status_digest_matches,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "submit_exact_continuation_status_digest_to_ao2_post_continuation_executor"
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
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-action"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_execute_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
    claim_status_sha256: &str,
    continuation_status_sha256: &str,
    approve_continuation_status_sha256: Option<&str>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let action = factory_queue_project_start_recovery_resume_post_continuation_action_json(
        &target_root,
        queue_sha256,
        recovery_packet_sha256,
        plan_sha256,
        claim_status_sha256,
        continuation_status_sha256,
    )?;
    let run_id = json_string(&action, "run_id");
    let expected_continuation_status_sha256 =
        json_string(&action, "expected_continuation_status_sha256");
    let supplied_continuation_status_sha256 = json_string(&action, "continuation_status_sha256");
    let submitted_digest = approve_continuation_status_sha256.unwrap_or("").trim();
    let trust_boundary = factory_project_start_completion_summary_memory_trust_boundary();
    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records = read_jsonl_values(&records_path)?;
    let existing_records = records
        .iter()
        .filter(|record| {
            record["schema_version"] == "ao2.memory-record.v1"
                && record["kind"] == "project-start-recovery-resume-post-continuation-execute"
                && json_string(&record["source"], "run_id") == run_id
                && json_string(&record["source"], "path_sha256")
                    == supplied_continuation_status_sha256
        })
        .cloned()
        .collect::<Vec<_>>();

    if !existing_records.is_empty() && submitted_digest == supplied_continuation_status_sha256 {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execute.v1",
            "status": "blocked",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "claim_status_sha256": claim_status_sha256,
            "continuation_status_sha256": supplied_continuation_status_sha256,
            "approved_continuation_status_sha256": supplied_continuation_status_sha256,
            "post_continuation_action": action,
            "post_continuation_execution_record_count": existing_records.len(),
            "evidence": [{
                "kind": "recovery_resume_post_continuation_execution_memory_record",
                "count": existing_records.len(),
                "approved_continuation_status_sha256": supplied_continuation_status_sha256
            }],
            "concerns": [{
                "code": "post_continuation_execution_already_recorded",
                "severity": "high",
                "message": "AO2 refused to record a duplicate post-continuation recovery execution for the same C64 digest."
            }],
            "blockers": [{
                "code": "duplicate_recovery_resume_post_continuation_execution_records",
                "severity": "blocker",
                "message": "A post-continuation recovery execution memory record already exists for the approved C64 continuation-status digest."
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
            "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-executor"
        }));
    }

    if json_string(&action, "status") != "ready"
        || action
            .get("blockers")
            .and_then(|value| value.as_array())
            .is_some_and(|blockers| !blockers.is_empty())
    {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execute.v1",
            "status": "blocked",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "claim_status_sha256": claim_status_sha256,
            "continuation_status_sha256": supplied_continuation_status_sha256,
            "expected_continuation_status_sha256": expected_continuation_status_sha256,
            "post_continuation_action": action,
            "evidence": [],
            "concerns": [{
                "code": "recovery_resume_post_continuation_action_not_ready",
                "severity": "high",
                "message": "AO2 refused to execute post-continuation recovery without a ready C65 action packet."
            }],
            "blockers": [{
                "code": "recovery_resume_post_continuation_action_not_ready",
                "severity": "blocker",
                "message": "Run the C65 post-continuation action readback and resolve blockers before execution."
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
            "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-executor"
        }));
    }

    if submitted_digest != expected_continuation_status_sha256 {
        let blocker_code = if submitted_digest.is_empty() {
            "operator_continuation_status_digest_approval_required"
        } else {
            "continuation_status_approval_digest_mismatch"
        };
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execute-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_continuation_status_sha256",
            "required_flag": "--approve-continuation-status-sha256",
            "required_form_field": "approval_continuation_status_sha256",
            "run_id": run_id,
            "queue_sha256": queue_sha256,
            "recovery_packet_sha256": recovery_packet_sha256,
            "plan_sha256": plan_sha256,
            "claim_status_sha256": claim_status_sha256,
            "continuation_status_sha256": supplied_continuation_status_sha256,
            "expected_continuation_status_sha256": expected_continuation_status_sha256,
            "submitted_continuation_status_sha256": submitted_digest,
            "post_continuation_action": action,
            "evidence": [{
                "kind": "recovery_resume_post_continuation_action",
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-action.v1",
                "sha256": canonical_json_sha256(&action),
                "status": "ready"
            }],
            "concerns": [{
                "code": "operator_review_required",
                "severity": "high",
                "message": "AO2 requires the exact C64 continuation-status digest before executing post-continuation recovery."
            }],
            "blockers": [{
                "code": blocker_code,
                "severity": "blocker",
                "message": "Submit the exact C64 continuation_status_sha256 to allow AO2 to record the bounded post-continuation recovery execution."
            }],
            "next_action": "submit approval_continuation_status_sha256 or --approve-continuation-status-sha256 with the exact continuation_status_sha256 to execute AO2 post-continuation recovery",
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
            "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-executor"
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
    let continuation_memory_id = json_string(
        &action["continuation_status"]["continuation_memory_record"],
        "id",
    );
    let body = format!(
        "Project-start recovery resume post-continuation execution recorded for run_id={run_id}\ncontinuation_status_sha256={expected_continuation_status_sha256}\nclaim_status_sha256={claim_status_sha256}\nplan_sha256={plan_sha256}\nqueue_sha256={queue_sha256}\nrecovery_packet_sha256={recovery_packet_sha256}\ncontinuation_memory_record_id={continuation_memory_id}"
    );
    let mut post_continuation_memory_record = memory_write_record_json(
        &target_root,
        "project-start-recovery-resume-post-continuation-execute".to_string(),
        format!("Project-start recovery resume post-continuation execution: {run_id}"),
        body,
        vec![
            "hermes".to_string(),
            "ao2".to_string(),
            "project-start".to_string(),
            "recovery".to_string(),
            "resume-post-continuation".to_string(),
        ],
        Some(run_id.clone()),
        None,
    )?;
    post_continuation_memory_record["source"]["path"] = serde_json::json!(
        "inline:ao2.factory-project-start-recovery-resume-continuation-status.v1"
    );
    post_continuation_memory_record["source"]["path_sha256"] =
        serde_json::json!(expected_continuation_status_sha256.clone());
    append_jsonl(&records_path, &post_continuation_memory_record)?;
    let post_continuation_memory_link = memory_link_run_json(
        &target_root,
        json_string(&post_continuation_memory_record, "id"),
        run_id.clone(),
        "project-start-recovery-resume-post-continuation-execute".to_string(),
    )?;
    append_jsonl(&links_path, &post_continuation_memory_link)?;
    let records_sha256_after = sha256_file(&records_path)?;
    let links_sha256_after = sha256_file(&links_path)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execute.v1",
        "status": "executed",
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "claim_status_sha256": claim_status_sha256,
        "continuation_status_sha256": expected_continuation_status_sha256,
        "approved_continuation_status_sha256": expected_continuation_status_sha256,
        "post_continuation_action": action,
        "approval": {
            "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execute-approval.v1",
            "status": "approved_exact_continuation_status_sha256",
            "approval_mode": "exact_continuation_status_sha256",
            "continuation_status_sha256": expected_continuation_status_sha256
        },
        "evidence": [
            {
                "kind": "recovery_resume_post_continuation_action",
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-action.v1",
                "sha256": canonical_json_sha256(&action),
                "status": "ready"
            },
            {
                "kind": "recovery_resume_post_continuation_execution_memory_record",
                "id": json_string(&post_continuation_memory_record, "id"),
                "continuation_status_sha256": expected_continuation_status_sha256
            }
        ],
        "concerns": [],
        "blockers": [],
        "post_continuation_memory_record": post_continuation_memory_record,
        "post_continuation_memory_link": post_continuation_memory_link,
        "changed_files": [
            {
                "path": records_path.display().to_string(),
                "sha256_before": records_sha256_before,
                "sha256_after": records_sha256_after,
                "reason": "recorded AO2 post-continuation recovery execution under approved continuation-status digest"
            },
            {
                "path": links_path.display().to_string(),
                "sha256_before": links_sha256_before,
                "sha256_after": links_sha256_after,
                "reason": "linked AO2 post-continuation recovery execution memory record to run"
            }
        ],
        "hermes_memory": {
            "single_post_continuation_execution_record_for_bookkeeping": true,
            "post_continuation_execution_bound_to_continuation_status_sha256": true,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "read_recovery_resume_post_continuation_execution_evidence"
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
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-executor"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_execution_status_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
    claim_status_sha256: &str,
    continuation_status_sha256: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize factory target {}", target.display()))?;
    let post_continuation_action =
        factory_queue_project_start_recovery_resume_post_continuation_action_json(
            &target_root,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
        )?;
    let action_sha256 = canonical_json_sha256(&post_continuation_action);
    let run_id = json_string(&post_continuation_action, "run_id");
    let supplied_continuation_status_sha256 = continuation_status_sha256.trim().to_string();
    let expected_continuation_status_sha256 = json_string(
        &post_continuation_action,
        "expected_continuation_status_sha256",
    );
    let continuation_status_digest_matches_current =
        supplied_continuation_status_sha256 == expected_continuation_status_sha256;
    let continuation_memory_id = json_string(
        &post_continuation_action["continuation_status"]["continuation_memory_record"],
        "id",
    );
    let records_path = memory_records_path(&target_root);
    let links_path = memory_run_links_path(&target_root);
    let records = read_jsonl_values(&records_path)?;
    let links = read_jsonl_values(&links_path)?;

    let run_execution_records = records
        .iter()
        .filter(|record| {
            record["schema_version"] == "ao2.memory-record.v1"
                && record["kind"] == "project-start-recovery-resume-post-continuation-execute"
                && json_string(&record["source"], "run_id") == run_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let matching_execution_records = run_execution_records
        .iter()
        .filter(|record| {
            json_string(&record["source"], "path_sha256") == supplied_continuation_status_sha256
        })
        .cloned()
        .collect::<Vec<_>>();
    let execution_memory_record = matching_execution_records
        .first()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let execution_memory_id = json_string(&execution_memory_record, "id");
    let matching_execution_links = links
        .iter()
        .filter(|link| {
            link["schema_version"] == "ao2.memory-run-link.v1"
                && json_string(link, "memory_id") == execution_memory_id
                && json_string(link, "run_id") == run_id
                && json_string(link, "relationship")
                    == "project-start-recovery-resume-post-continuation-execute"
        })
        .cloned()
        .collect::<Vec<_>>();
    let execution_memory_link = matching_execution_links
        .first()
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let execution_body = json_string(&execution_memory_record, "body");
    let execution_record_is_unique = matching_execution_records.len() == 1;
    let execution_run_link_is_unique = matching_execution_links.len() == 1;
    let record_source_binds_continuation_status_sha256 =
        json_string(&execution_memory_record["source"], "path_sha256")
            == supplied_continuation_status_sha256;
    let body_binds_continuation_status_sha256 = execution_body.contains(&format!(
        "continuation_status_sha256={supplied_continuation_status_sha256}"
    ));
    let body_binds_claim_status_sha256 =
        execution_body.contains(&format!("claim_status_sha256={claim_status_sha256}"));
    let body_binds_plan_sha256 = execution_body.contains(&format!("plan_sha256={plan_sha256}"));
    let body_binds_queue_sha256 = execution_body.contains(&format!("queue_sha256={queue_sha256}"));
    let body_binds_recovery_packet_sha256 =
        execution_body.contains(&format!("recovery_packet_sha256={recovery_packet_sha256}"));
    let body_binds_continuation_memory_record_id = execution_body.contains(&format!(
        "continuation_memory_record_id={continuation_memory_id}"
    ));
    let body_binds_all_upstream_digests = body_binds_continuation_status_sha256
        && body_binds_claim_status_sha256
        && body_binds_plan_sha256
        && body_binds_queue_sha256
        && body_binds_recovery_packet_sha256
        && body_binds_continuation_memory_record_id;
    let execution_link_targets_record = !execution_memory_id.is_empty()
        && json_string(&execution_memory_link, "memory_id") == execution_memory_id;

    let mut blockers = Vec::new();
    if matching_execution_records.is_empty() {
        blockers.push(serde_json::json!({
            "code": "missing_recovery_resume_post_continuation_execution_record",
            "severity": "blocker",
            "message": "No C66 post-continuation execution memory record is bound to the approved C64 continuation-status digest."
        }));
    } else if matching_execution_records.len() > 1 {
        blockers.push(serde_json::json!({
            "code": "duplicate_recovery_resume_post_continuation_execution_records",
            "severity": "blocker",
            "message": "Multiple C66 post-continuation execution memory records match the same approved C64 continuation-status digest."
        }));
    }
    if matching_execution_links.is_empty() {
        blockers.push(serde_json::json!({
            "code": "missing_recovery_resume_post_continuation_execution_run_link",
            "severity": "blocker",
            "message": "No C66 post-continuation execution memory run link targets the selected run."
        }));
    } else if matching_execution_links.len() > 1 {
        blockers.push(serde_json::json!({
            "code": "duplicate_recovery_resume_post_continuation_execution_run_links",
            "severity": "blocker",
            "message": "Multiple C66 post-continuation execution memory run links target the selected run."
        }));
    }
    if !matching_execution_records.is_empty() && !record_source_binds_continuation_status_sha256 {
        blockers.push(serde_json::json!({
            "code": "post_continuation_execution_source_digest_mismatch",
            "severity": "blocker",
            "message": "The C66 post-continuation execution source digest is not bound to the approved C64 continuation-status digest."
        }));
    }
    if !matching_execution_records.is_empty() && !body_binds_all_upstream_digests {
        blockers.push(serde_json::json!({
            "code": "post_continuation_execution_body_digest_binding_mismatch",
            "severity": "blocker",
            "message": "The C66 post-continuation execution body does not replay the expected continuation-status, claim-status, plan, queue, recovery-packet, and continuation-memory identifiers."
        }));
    }
    if !matching_execution_links.is_empty() && !execution_link_targets_record {
        blockers.push(serde_json::json!({
            "code": "post_continuation_execution_run_link_record_mismatch",
            "severity": "blocker",
            "message": "The C66 post-continuation execution run link does not target the selected memory record."
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
            "code": "post_continuation_execution_replay_not_trusted",
            "severity": "high",
            "message": "Hermes must not advance governed recovery bookkeeping until AO2 reports a unique replayable C66 post-continuation execution."
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
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execution-status.v1",
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "claim_status_sha256": claim_status_sha256,
        "continuation_status_sha256": supplied_continuation_status_sha256,
        "expected_continuation_status_sha256": expected_continuation_status_sha256,
        "approved_continuation_status_sha256": supplied_continuation_status_sha256,
        "continuation_status_digest_matches_current": continuation_status_digest_matches_current,
        "post_continuation_execution_record_count": matching_execution_records.len(),
        "post_continuation_execution_run_link_count": matching_execution_links.len(),
        "all_post_continuation_execution_record_count_for_run": run_execution_records.len(),
        "post_continuation_execution": {
            "memory_record_id": execution_memory_id,
            "memory_run_link": execution_memory_link,
            "record_source_binds_continuation_status_sha256": record_source_binds_continuation_status_sha256,
            "body_binds_continuation_status_sha256": body_binds_continuation_status_sha256,
            "body_binds_claim_status_sha256": body_binds_claim_status_sha256,
            "body_binds_plan_sha256": body_binds_plan_sha256,
            "body_binds_queue_sha256": body_binds_queue_sha256,
            "body_binds_recovery_packet_sha256": body_binds_recovery_packet_sha256,
            "body_binds_continuation_memory_record_id": body_binds_continuation_memory_record_id,
            "body_binds_all_upstream_digests": body_binds_all_upstream_digests,
            "run_link_targets_record": execution_link_targets_record,
            "workbench_restart_replayable": workbench_restart_replayable
        },
        "post_continuation_memory_record": execution_memory_record,
        "post_continuation_memory_link": execution_memory_link,
        "post_continuation_action": post_continuation_action,
        "replay_verification": {
            "post_continuation_action_sha256": action_sha256,
            "continuation_status_digest_matches_current": continuation_status_digest_matches_current,
            "execution_record_is_unique": execution_record_is_unique,
            "execution_run_link_is_unique": execution_run_link_is_unique,
            "record_source_binds_continuation_status_sha256": record_source_binds_continuation_status_sha256,
            "body_binds_all_upstream_digests": body_binds_all_upstream_digests,
            "execution_link_targets_record": execution_link_targets_record,
            "workbench_restart_replayable": workbench_restart_replayable
        },
        "evidence": [
            {
                "kind": "recovery_resume_post_continuation_action_replay",
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-action.v1",
                "sha256": action_sha256,
                "status": post_continuation_action["status"].clone()
            },
            {
                "kind": "recovery_resume_post_continuation_execution_memory_record",
                "id": json_string(&matching_execution_records.first().cloned().unwrap_or(serde_json::Value::Null), "id"),
                "count": matching_execution_records.len(),
                "approved_continuation_status_sha256": supplied_continuation_status_sha256
            },
            {
                "kind": "recovery_resume_post_continuation_execution_memory_run_link",
                "count": matching_execution_links.len(),
                "relationship": "project-start-recovery-resume-post-continuation-execute"
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": [
            {
                "path": records_path.display().to_string(),
                "sha256": records_sha256,
                "role": "durable C66 post-continuation execution memory record store",
                "observed_after_post_continuation_execution": true
            },
            {
                "path": links_path.display().to_string(),
                "sha256": links_sha256,
                "role": "durable C66 post-continuation execution run-link store",
                "observed_after_post_continuation_execution": true
            }
        ],
        "memory_paths": {
            "records_jsonl": records_path.display().to_string(),
            "run_links_jsonl": links_path.display().to_string()
        },
        "hermes_memory": {
            "single_post_continuation_execution_status_packet_for_bookkeeping": true,
            "post_continuation_execution_bound_to_continuation_status_sha256": record_source_binds_continuation_status_sha256,
            "workbench_restart_replayable": workbench_restart_replayable,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "observe_recovery_resume_post_continuation_execution_status_then_continue_governed_recovery"
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
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-execution-status"
    }))
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_next_action_json(
    target: &Path,
    queue_sha256: &str,
    recovery_packet_sha256: &str,
    plan_sha256: &str,
    claim_status_sha256: &str,
    continuation_status_sha256: &str,
    post_continuation_execution_status_sha256: &str,
) -> Result<serde_json::Value> {
    let execution_status =
        factory_queue_project_start_recovery_resume_post_continuation_execution_status_json(
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
        )?;
    let actual_execution_status_sha256 = canonical_json_sha256(&execution_status);
    let supplied_execution_status_sha256 =
        post_continuation_execution_status_sha256.trim().to_string();
    let execution_status_digest_matches =
        supplied_execution_status_sha256 == actual_execution_status_sha256;
    let execution_status_ready =
        execution_status["status"] == "ready" && execution_status["read_only"] == true;
    let execution_status_blockers = execution_status
        .get("blockers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut blockers = Vec::new();
    if !execution_status_digest_matches {
        blockers.push(serde_json::json!({
            "code": "post_continuation_execution_status_digest_mismatch",
            "severity": "blocker",
            "message": "Supplied post_continuation_execution_status_sha256 does not match the recomputed C67 post-continuation execution-status packet."
        }));
    }
    if !execution_status_ready {
        blockers.push(serde_json::json!({
            "code": "post_continuation_execution_status_not_ready",
            "severity": "blocker",
            "message": "AO2 refused to issue the next-action contract until C67 reports ready."
        }));
    }
    if !execution_status_blockers.is_empty() {
        blockers.push(serde_json::json!({
            "code": "post_continuation_execution_status_blockers_present",
            "severity": "blocker",
            "message": "C67 post-continuation execution-status reported blockers; continue only after AO2 resolves duplicate, missing, or mismatched execution evidence."
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
            "code": "recovery_resume_post_continuation_next_action_blocked",
            "severity": "high",
            "message": "Hermes must not close or advance governed recovery until AO2 reports a digest-bound ready C68 next-action contract."
        })]
    };
    let classification = serde_json::json!({
        "size": "bounded",
        "shape": "bug-fix",
        "reason": "The next recovery step closes or routes a previously interrupted governed workflow after C67 proves the C66 post-continuation execution is durable and replayable."
    });
    let next_bounded_action = serde_json::json!({
        "action": "close_recovery_resume_post_continuation_after_operator_review",
        "read_only": true,
        "requires_exact_digest_approval": false,
        "mutates_queue_or_memory": false,
        "closure_or_handoff_state": "recovery_resume_post_continuation_ready_for_operator_handoff",
        "required_post_continuation_execution_status_sha256": actual_execution_status_sha256,
        "next_ao2_command": null,
        "reason": "C67 is ready and digest-bound; no further AO2 mutation is required for this recovery chain before operator/evaluator handoff."
    });
    let run_id = json_string(&execution_status, "run_id");

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-next-action.v1",
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "claim_status_sha256": claim_status_sha256,
        "continuation_status_sha256": continuation_status_sha256,
        "post_continuation_execution_status_sha256": supplied_execution_status_sha256,
        "expected_post_continuation_execution_status_sha256": actual_execution_status_sha256,
        "execution_status_digest_matches_current": execution_status_digest_matches,
        "execution_status_ready": execution_status_ready,
        "classification": classification,
        "next_bounded_action": next_bounded_action,
        "post_continuation_execution_status": execution_status,
        "evidence": [
            {
                "kind": "recovery_resume_post_continuation_execution_status",
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execution-status.v1",
                "sha256": actual_execution_status_sha256,
                "status": execution_status["status"].clone()
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": [],
        "hermes_memory": {
            "single_post_continuation_next_action_for_bookkeeping": true,
            "post_continuation_execution_status_bound_to_sha256": execution_status_digest_matches,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": "close_recovery_resume_post_continuation_or_route_next_governed_step"
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
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-next-action"
    }))
}

pub(crate) struct RecoveryResumePostContinuationClosureArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) queue_sha256: &'a str,
    pub(crate) recovery_packet_sha256: &'a str,
    pub(crate) plan_sha256: &'a str,
    pub(crate) claim_status_sha256: &'a str,
    pub(crate) continuation_status_sha256: &'a str,
    pub(crate) post_continuation_execution_status_sha256: &'a str,
    pub(crate) post_continuation_next_action_sha256: &'a str,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_closure_json(
    args: RecoveryResumePostContinuationClosureArgs<'_>,
) -> Result<serde_json::Value> {
    let target = args.target;
    let queue_sha256 = args.queue_sha256;
    let recovery_packet_sha256 = args.recovery_packet_sha256;
    let plan_sha256 = args.plan_sha256;
    let claim_status_sha256 = args.claim_status_sha256;
    let continuation_status_sha256 = args.continuation_status_sha256;
    let post_continuation_execution_status_sha256 = args.post_continuation_execution_status_sha256;
    let post_continuation_next_action_sha256 = args.post_continuation_next_action_sha256;
    let next_action =
        factory_queue_project_start_recovery_resume_post_continuation_next_action_json(
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
            post_continuation_execution_status_sha256,
        )?;
    let actual_next_action_sha256 = canonical_json_sha256(&next_action);
    let supplied_next_action_sha256 = post_continuation_next_action_sha256.trim().to_string();
    let next_action_digest_matches = supplied_next_action_sha256 == actual_next_action_sha256;
    let next_action_ready = next_action["status"] == "ready" && next_action["read_only"] == true;
    let next_action_blockers = next_action
        .get("blockers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut blockers = Vec::new();
    if !next_action_digest_matches {
        blockers.push(serde_json::json!({
            "code": "post_continuation_next_action_digest_mismatch",
            "severity": "blocker",
            "message": "Supplied post_continuation_next_action_sha256 does not match the recomputed C68 next-action packet."
        }));
    }
    if !next_action_ready {
        blockers.push(serde_json::json!({
            "code": "post_continuation_next_action_not_ready",
            "severity": "blocker",
            "message": "AO2 refused to issue the closure handoff until C68 reports ready."
        }));
    }
    if !next_action_blockers.is_empty() {
        blockers.push(serde_json::json!({
            "code": "post_continuation_next_action_blockers_present",
            "severity": "blocker",
            "message": "C68 post-continuation next-action reported blockers; continue only after AO2 resolves the recovery chain evidence."
        }));
    }

    let closure_ready = blockers.is_empty();
    let status = if closure_ready { "ready" } else { "blocked" };
    let concerns = if closure_ready {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "code": "recovery_resume_post_continuation_closure_blocked",
            "severity": "high",
            "message": "Hermes and evaluator-closer must not close the governed recovery chain until AO2 reports a digest-bound ready C69 closure handoff."
        })]
    };
    let next_ao2_owned_step = if closure_ready {
        serde_json::Value::Null
    } else {
        serde_json::json!({
            "required": true,
            "reason": "AO2 must repair or regenerate the exact-digest recovery chain before evaluator-closer handoff.",
            "recommended_action": "rerun_or_repair_recovery_resume_post_continuation_chain"
        })
    };
    let run_id = json_string(&next_action, "run_id");

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-closure.v1",
        "status": status,
        "read_only": true,
        "closure_ready": closure_ready,
        "run_id": run_id,
        "queue_sha256": queue_sha256,
        "recovery_packet_sha256": recovery_packet_sha256,
        "plan_sha256": plan_sha256,
        "claim_status_sha256": claim_status_sha256,
        "continuation_status_sha256": continuation_status_sha256,
        "post_continuation_execution_status_sha256": post_continuation_execution_status_sha256,
        "post_continuation_next_action_sha256": supplied_next_action_sha256,
        "expected_post_continuation_next_action_sha256": actual_next_action_sha256,
        "next_action_digest_matches_current": next_action_digest_matches,
        "next_action_ready": next_action_ready,
        "post_continuation_next_action": next_action,
        "evidence_chain": {
            "continuity": {
                "schema_version": "ao2.factory-project-start-recovery-resume-continuity.v1",
                "queue_sha256": queue_sha256,
                "recovery_packet_sha256": recovery_packet_sha256
            },
            "plan": {
                "schema_version": "ao2.factory-project-start-recovery-resume-plan.v1",
                "sha256": plan_sha256
            },
            "claim_status": {
                "schema_version": "ao2.factory-project-start-recovery-resume-claim-status.v1",
                "sha256": claim_status_sha256
            },
            "continuation_status": {
                "schema_version": "ao2.factory-project-start-recovery-resume-continuation-status.v1",
                "sha256": continuation_status_sha256
            },
            "post_continuation_execution_status": {
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-execution-status.v1",
                "sha256": post_continuation_execution_status_sha256
            },
            "post_continuation_next_action": {
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-next-action.v1",
                "sha256": actual_next_action_sha256,
                "supplied_sha256": supplied_next_action_sha256,
                "status": next_action["status"].clone()
            }
        },
        "handoff": {
            "handoff_packet": "recovery_resume_post_continuation_closure",
            "consumer": "Hermes and factory-v3 evaluator-closer",
            "closure_state": if closure_ready {
                "ready_for_evaluator_closer_review"
            } else {
                "requires_ao2_owned_exact_digest_step"
            },
            "raw_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "factory_v3_role": "evaluator-closer parity oracle",
            "control_plane_role": "read_only_observer"
        },
        "next_ao2_owned_step": next_ao2_owned_step,
        "evidence": [
            {
                "kind": "recovery_resume_post_continuation_next_action",
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-next-action.v1",
                "sha256": actual_next_action_sha256,
                "status": next_action["status"].clone()
            }
        ],
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": [],
        "hermes_memory": {
            "single_post_continuation_closure_packet_for_bookkeeping": true,
            "post_continuation_next_action_bound_to_sha256": next_action_digest_matches,
            "closure_ready": closure_ready,
            "raw_memory_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": if closure_ready {
                "send_recovery_resume_closure_packet_to_evaluator_closer"
            } else {
                "route_recovery_resume_closure_blocker_to_ao2_repair"
            }
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
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-closure"
    }))
}

pub(crate) struct RecoveryResumePostContinuationEvaluatorDecisionArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) queue_sha256: &'a str,
    pub(crate) recovery_packet_sha256: &'a str,
    pub(crate) plan_sha256: &'a str,
    pub(crate) claim_status_sha256: &'a str,
    pub(crate) continuation_status_sha256: &'a str,
    pub(crate) post_continuation_execution_status_sha256: &'a str,
    pub(crate) post_continuation_next_action_sha256: &'a str,
    pub(crate) closure_sha256: &'a str,
    pub(crate) signing_key: &'a Path,
    pub(crate) signer_id: &'a str,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_evaluator_decision_json(
    args: RecoveryResumePostContinuationEvaluatorDecisionArgs<'_>,
) -> Result<serde_json::Value> {
    if args.signer_id.trim().is_empty() {
        anyhow::bail!("signer-id must not be empty");
    }
    let closure = factory_queue_project_start_recovery_resume_post_continuation_closure_json(
        RecoveryResumePostContinuationClosureArgs {
            target: args.target,
            queue_sha256: args.queue_sha256,
            recovery_packet_sha256: args.recovery_packet_sha256,
            plan_sha256: args.plan_sha256,
            claim_status_sha256: args.claim_status_sha256,
            continuation_status_sha256: args.continuation_status_sha256,
            post_continuation_execution_status_sha256: args
                .post_continuation_execution_status_sha256,
            post_continuation_next_action_sha256: args.post_continuation_next_action_sha256,
        },
    )?;
    let actual_closure_sha256 = canonical_json_sha256(&closure);
    let supplied_closure_sha256 = args.closure_sha256.trim().to_string();
    let closure_digest_matches = supplied_closure_sha256 == actual_closure_sha256;
    let closure_ready = closure["status"] == "ready"
        && closure["read_only"] == true
        && closure["closure_ready"] == true;
    let closure_blockers = closure
        .get("blockers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let factory_v3_expectations = serde_json::json!({
        "closure_handoff_consumer": closure["handoff"]["consumer"] == "Hermes and factory-v3 evaluator-closer",
        "factory_v3_role_is_parity_oracle": closure["handoff"]["factory_v3_role"] == "evaluator-closer parity oracle",
        "control_plane_is_observer_only": closure["handoff"]["control_plane_role"] == "read_only_observer",
        "raw_jsonl_scrape_not_required": closure["handoff"]["raw_jsonl_scrape_required"] == false,
        "raw_queue_json_scrape_not_required": closure["handoff"]["raw_queue_json_scrape_required"] == false,
        "closure_ready": closure_ready
    });
    let expectations_satisfied = factory_v3_expectations
        .as_object()
        .map(|expectations| expectations.values().all(|value| value == true))
        .unwrap_or(false);

    let mut blockers = Vec::new();
    if !closure_digest_matches {
        blockers.push(serde_json::json!({
            "code": "post_continuation_closure_digest_mismatch",
            "severity": "blocker",
            "message": "Supplied closure_sha256 does not match the recomputed C69 closure handoff packet."
        }));
    }
    if !closure_ready {
        blockers.push(serde_json::json!({
            "code": "post_continuation_closure_not_ready",
            "severity": "blocker",
            "message": "AO2 refused to issue an evaluator-style decision until C69 reports closure-ready."
        }));
    }
    if !closure_blockers.is_empty() {
        blockers.push(serde_json::json!({
            "code": "post_continuation_closure_blockers_present",
            "severity": "blocker",
            "message": "C69 closure handoff reported blockers; evaluator-style decision evidence must remain blocked until AO2 repairs the chain."
        }));
    }
    if !expectations_satisfied {
        blockers.push(serde_json::json!({
            "code": "factory_v3_evaluator_closer_expectation_mismatch",
            "severity": "blocker",
            "message": "The C69 closure handoff does not satisfy the factory-v3 evaluator-closer parity-oracle expectations."
        }));
    }

    let accepted = blockers.is_empty();
    let status = if accepted { "accepted" } else { "blocked" };
    let verdict = if accepted {
        "accept_recovery_closure_evidence"
    } else {
        "block_recovery_closure_evidence"
    };
    let run_id = json_string(&closure, "run_id");
    let decision_dir = args
        .target
        .join(".ao2")
        .join("factory-compat")
        .join("recovery-evaluator-decisions");
    fs::create_dir_all(&decision_dir)
        .with_context(|| format!("create {}", decision_dir.display()))?;
    let decision_path = decision_dir.join(format!(
        "{}-post-continuation-evaluator-decision.json",
        sanitize_greenfield_id(&run_id)
    ));
    let signed_payload_path = decision_path.with_extension("signed-payload.json");
    let signature_path = decision_path.with_extension("json.sig");
    let public_key_path = decision_path.with_extension("public.pem");
    let mut result = serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-evaluator-decision.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "queue_sha256": args.queue_sha256,
        "recovery_packet_sha256": args.recovery_packet_sha256,
        "plan_sha256": args.plan_sha256,
        "claim_status_sha256": args.claim_status_sha256,
        "continuation_status_sha256": args.continuation_status_sha256,
        "post_continuation_execution_status_sha256": args.post_continuation_execution_status_sha256,
        "post_continuation_next_action_sha256": args.post_continuation_next_action_sha256,
        "closure_sha256": supplied_closure_sha256,
        "expected_closure_sha256": actual_closure_sha256,
        "closure_digest_matches_current": closure_digest_matches,
        "closure_ready": closure_ready,
        "post_continuation_closure": closure,
        "decision": {
            "owner": "ao2-workbench-recovery-post-continuation-evaluator",
            "verdict": verdict,
            "evaluator_style": true,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "ao2_release_approval": false,
            "factory_v3_required_to_drive_workflow": false,
            "factory_v3_may_compare_as_parity_oracle": true
        },
        "factory_v3_parity_oracle": {
            "schema_version": "ao2.factory-v3-evaluator-closer-parity-oracle.v1",
            "role": "compare AO2 closure decision evidence against evaluator-closer expectations",
            "expectations": factory_v3_expectations,
            "expectations_satisfied": expectations_satisfied,
            "factory_v3_drives_workflow": false
        },
        "hermes_support_artifact": {
            "decision_path": decision_path.display().to_string(),
            "signed_payload_path": signed_payload_path.display().to_string(),
            "signature_path": signature_path.display().to_string(),
            "public_key_path": public_key_path.display().to_string(),
            "raw_jsonl_scrape_required": false,
            "raw_queue_json_scrape_required": false,
            "next_recommended_action": if accepted {
                "factory_v3_evaluator_closer_compare_c70_signed_decision_and_prepare_ao2_native_recovery_release_handoff"
            } else {
                "route_c70_blocker_to_ao2_recovery_repair"
            }
        },
        "evidence": [
            {
                "kind": "recovery_resume_post_continuation_closure",
                "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-closure.v1",
                "sha256": actual_closure_sha256,
                "supplied_sha256": supplied_closure_sha256,
                "status": status
            }
        ],
        "concerns": if accepted {
            serde_json::json!([])
        } else {
            serde_json::json!([{
                "code": "recovery_resume_post_continuation_evaluator_decision_blocked",
                "severity": "high",
                "message": "Hermes and evaluator-closer must not accept the recovery closure until AO2 produces a digest-matched signed evaluator decision."
            }])
        },
        "blockers": blockers,
        "changed_files": [],
        "side_effects": {
            "would_write_decision_support_artifact": true,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": {
            "decision_owner": "ao2",
            "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "parity_checklist_progress": {
            "ao2_owns_evaluator_style_recovery_closure_decision": true,
            "factory_v3_drives_workflow": false,
            "signed_support_artifact_written": true,
            "release_acceptance_still_owned_by_evaluator_closer": true
        },
        "decision_path": decision_path.display().to_string(),
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-evaluator"
    });
    atomic_write_text(
        &signed_payload_path,
        &serde_json::to_string_pretty(&result)?,
    )?;
    derive_public_key_from_private_key(args.signing_key, &public_key_path)?;
    sign_file_with_private_key(args.signing_key, &signed_payload_path, &signature_path)?;
    let signature_verified =
        verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?;
    let signature = serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-evaluator-decision-signature.v1",
        "signature_algorithm": "RSA/SHA-256",
        "signer_id": args.signer_id.trim(),
        "signed_payload": "post_continuation_evaluator_decision_without_signature_field",
        "signed_payload_path": signed_payload_path.display().to_string(),
        "signed_payload_sha256": sha256_file(&signed_payload_path)?,
        "signature_path": signature_path.display().to_string(),
        "signature_sha256": sha256_file(&signature_path)?,
        "public_key_path": public_key_path.display().to_string(),
        "public_key_sha256": sha256_file(&public_key_path)?,
        "signature_verified": signature_verified
    });
    if let Some(object) = result.as_object_mut() {
        object.insert("signature".to_string(), signature);
    }
    atomic_write_text(&decision_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) struct RecoveryResumePostContinuationReleaseHandoffArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) decision: &'a Path,
    pub(crate) signed_payload: &'a Path,
    pub(crate) signature: &'a Path,
    pub(crate) public_key: &'a Path,
    pub(crate) closure_sha256: &'a str,
    pub(crate) decision_sha256: &'a str,
    pub(crate) out: &'a Path,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_handoff_json(
    args: RecoveryResumePostContinuationReleaseHandoffArgs<'_>,
) -> Result<serde_json::Value> {
    for (label, path) in [
        ("decision", args.decision),
        ("signed-payload", args.signed_payload),
        ("signature", args.signature),
        ("public-key", args.public_key),
    ] {
        if !path.is_file() {
            anyhow::bail!("missing {label} artifact: {}", path.display());
        }
    }

    let decision_text = fs::read_to_string(args.decision)
        .with_context(|| format!("read decision {}", args.decision.display()))?;
    let decision: serde_json::Value = serde_json::from_str(&decision_text)
        .with_context(|| format!("parse decision {}", args.decision.display()))?;
    if json_string(&decision, "schema_version")
        != "ao2.factory-project-start-recovery-resume-post-continuation-evaluator-decision.v1"
    {
        anyhow::bail!("decision must be a C70 recovery evaluator decision artifact");
    }
    if json_string(&decision, "status") != "accepted" {
        anyhow::bail!("decision must be accepted before release handoff packaging");
    }

    let supplied_closure_sha256 = args.closure_sha256.trim().to_string();
    let supplied_decision_sha256 = args.decision_sha256.trim().to_string();
    let actual_decision_sha256 = sha256_file(args.decision)?;
    let decision_digest_matches = supplied_decision_sha256 == actual_decision_sha256;
    let closure_digest_matches = json_string(&decision, "closure_sha256")
        == supplied_closure_sha256
        && json_string(&decision, "expected_closure_sha256") == supplied_closure_sha256
        && decision["closure_digest_matches_current"] == true;
    if !decision_digest_matches {
        anyhow::bail!("supplied decision_sha256 does not match the decision artifact");
    }
    if !closure_digest_matches {
        anyhow::bail!("supplied closure_sha256 does not match the C70 decision digest chain");
    }

    let signature_verified =
        verify_file_signature(args.signed_payload, args.signature, args.public_key)?;
    if !signature_verified {
        anyhow::bail!("C70 evaluator decision signature verification failed");
    }
    let signature = decision
        .get("signature")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if json_string(&signature, "signed_payload_sha256") != sha256_file(args.signed_payload)? {
        anyhow::bail!("signed payload digest does not match the C70 decision metadata");
    }
    if json_string(&signature, "signature_sha256") != sha256_file(args.signature)? {
        anyhow::bail!("signature digest does not match the C70 decision metadata");
    }
    if json_string(&signature, "public_key_sha256") != sha256_file(args.public_key)? {
        anyhow::bail!("public key digest does not match the C70 decision metadata");
    }

    let parent = args
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let stem = args
        .out
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("recovery-release-handoff.tgz");
    let stage_dir = parent.join(format!(".{stem}.stage-{}", now_unix_ms()));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove stale {}", stage_dir.display()))?;
    }
    let artifact_dir = stage_dir.join("artifacts").join("evaluator-decision");
    fs::create_dir_all(&artifact_dir)
        .with_context(|| format!("create {}", artifact_dir.display()))?;

    let staged_decision = artifact_dir.join("evaluator-decision.json");
    let staged_signed_payload = artifact_dir.join("signed-payload.json");
    let staged_signature = artifact_dir.join("signature.sig");
    let staged_public_key = artifact_dir.join("public.pem");
    fs::copy(args.decision, &staged_decision)
        .with_context(|| format!("copy {}", args.decision.display()))?;
    fs::copy(args.signed_payload, &staged_signed_payload)
        .with_context(|| format!("copy {}", args.signed_payload.display()))?;
    fs::copy(args.signature, &staged_signature)
        .with_context(|| format!("copy {}", args.signature.display()))?;
    fs::copy(args.public_key, &staged_public_key)
        .with_context(|| format!("copy {}", args.public_key.display()))?;

    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let run_id = json_string(&decision, "run_id");
    let trust_boundary = serde_json::json!({
        "evidence_owner": "ao2",
        "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let handoff = serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff.v1",
        "created_at": created_at,
        "status": "bundled",
        "read_only": true,
        "run_id": run_id,
        "target": args.target.display().to_string(),
        "closure_sha256": supplied_closure_sha256,
        "decision_sha256": supplied_decision_sha256,
        "expected_decision_sha256": actual_decision_sha256,
        "decision_digest_matches_current": decision_digest_matches,
        "closure_digest_matches_decision": closure_digest_matches,
        "signature_verified": signature_verified,
        "signature_algorithm": json_string(&signature, "signature_algorithm"),
        "signer_id": json_string(&signature, "signer_id"),
        "verifier_metadata": {
            "decision_path": args.decision.display().to_string(),
            "signed_payload_path": args.signed_payload.display().to_string(),
            "signature_path": args.signature.display().to_string(),
            "public_key_path": args.public_key.display().to_string(),
            "decision_sha256": supplied_decision_sha256,
            "signed_payload_sha256": sha256_file(args.signed_payload)?,
            "signature_sha256": sha256_file(args.signature)?,
            "public_key_sha256": sha256_file(args.public_key)?,
            "c70_signature_verified_before_packaging": true
        },
        "factory_v3_parity_oracle": {
            "schema_version": "ao2.factory-v3-evaluator-closer-release-handoff-parity.v1",
            "ready_for_comparison": true,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "expectations": {
                "decision_status_accepted": decision["status"] == "accepted",
                "decision_signature_verified": signature_verified,
                "closure_digest_bound": closure_digest_matches,
                "decision_digest_bound": decision_digest_matches,
                "ao2_release_approval": false,
                "control_plane_observer_only": true
            }
        },
        "concerns": [],
        "blockers": [],
        "changed_files": decision.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "side_effects": {
            "would_write_release_handoff_bundle": true,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": trust_boundary
    });
    let handoff_path = stage_dir.join("release-handoff.json");
    atomic_write_text(&handoff_path, &serde_json::to_string_pretty(&handoff)?)?;

    let mut checksum_entries = vec![
        (
            "artifacts/evaluator-decision/evaluator-decision.json".to_string(),
            sha256_file(&staged_decision)?,
        ),
        (
            "artifacts/evaluator-decision/signed-payload.json".to_string(),
            sha256_file(&staged_signed_payload)?,
        ),
        (
            "artifacts/evaluator-decision/signature.sig".to_string(),
            sha256_file(&staged_signature)?,
        ),
        (
            "artifacts/evaluator-decision/public.pem".to_string(),
            sha256_file(&staged_public_key)?,
        ),
        (
            "release-handoff.json".to_string(),
            sha256_file(&handoff_path)?,
        ),
    ];
    let manifest = serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff.v1",
        "created_at": handoff["created_at"].clone(),
        "status": "bundled",
        "run_id": handoff["run_id"].clone(),
        "closure_sha256": handoff["closure_sha256"].clone(),
        "decision_sha256": handoff["decision_sha256"].clone(),
        "signature_verified": signature_verified,
        "factory_v3_parity_oracle": handoff["factory_v3_parity_oracle"].clone(),
        "files": checksum_entries.iter().map(|(path, sha256)| {
            serde_json::json!({
                "path": path,
                "sha256": sha256
            })
        }).collect::<Vec<_>>(),
        "trust_boundary": handoff["trust_boundary"].clone()
    });
    let manifest_path = stage_dir.join("manifest.json");
    atomic_write_text(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
    checksum_entries.push(("manifest.json".to_string(), sha256_file(&manifest_path)?));
    checksum_entries.sort_by(|left, right| left.0.cmp(&right.0));
    let checksum_text = checksum_entries
        .iter()
        .map(|(path, sha256)| format!("{sha256}  {path}\n"))
        .collect::<String>();
    atomic_write_text(&stage_dir.join("SHA256SUMS"), &checksum_text)?;

    create_tar_gz(&stage_dir, args.out)?;
    fs::remove_dir_all(&stage_dir).with_context(|| format!("remove {}", stage_dir.display()))?;
    let archive_sha256 = sha256_file(args.out)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff.v1",
        "created_at": handoff["created_at"].clone(),
        "status": "bundled",
        "read_only": true,
        "run_id": handoff["run_id"].clone(),
        "archive": args.out,
        "archive_sha256": archive_sha256,
        "manifest_entry": "manifest.json",
        "checksum_entry": "SHA256SUMS",
        "release_handoff_entry": "release-handoff.json",
        "closure_sha256": handoff["closure_sha256"].clone(),
        "decision_sha256": handoff["decision_sha256"].clone(),
        "expected_decision_sha256": handoff["expected_decision_sha256"].clone(),
        "decision_digest_matches_current": decision_digest_matches,
        "closure_digest_matches_decision": closure_digest_matches,
        "signature_verified": signature_verified,
        "verifier_metadata": handoff["verifier_metadata"].clone(),
        "factory_v3_parity_oracle": handoff["factory_v3_parity_oracle"].clone(),
        "concerns": [],
        "blockers": [],
        "changed_files": handoff["changed_files"].clone(),
        "side_effects": handoff["side_effects"].clone(),
        "trust_boundary": handoff["trust_boundary"].clone()
    }))
}

pub(crate) struct RecoveryResumePostContinuationReleaseHandoffStatusArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) bundle: &'a Path,
    pub(crate) closure_sha256: &'a str,
    pub(crate) decision_sha256: &'a str,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_json(
    args: RecoveryResumePostContinuationReleaseHandoffStatusArgs<'_>,
) -> Result<serde_json::Value> {
    if !args.bundle.is_file() {
        anyhow::bail!(
            "missing recovery release handoff bundle: {}",
            args.bundle.display()
        );
    }

    let bundle_sha256 = sha256_file(args.bundle)?;
    let extract_dir = std::env::temp_dir().join(format!(
        "ao2-recovery-release-handoff-status-{}-{}",
        std::process::id(),
        now_unix_ms()
    ));
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir)
            .with_context(|| format!("remove stale {}", extract_dir.display()))?;
    }
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("create {}", extract_dir.display()))?;

    let mut blockers = Vec::new();
    let mut concerns = Vec::new();
    if let Err(error) = extract_tar_gz(args.bundle, &extract_dir) {
        blockers.push(serde_json::json!({
            "code": "release_handoff_bundle_extract_failed",
            "severity": "blocker",
            "message": format!("extract C71 release handoff bundle: {error}")
        }));
    }

    let manifest_path = extract_dir.join("manifest.json");
    let checksum_path = extract_dir.join("SHA256SUMS");
    let handoff_path = extract_dir.join("release-handoff.json");
    let decision_path = extract_dir
        .join("artifacts")
        .join("evaluator-decision")
        .join("evaluator-decision.json");
    let signed_payload_path = extract_dir
        .join("artifacts")
        .join("evaluator-decision")
        .join("signed-payload.json");
    let signature_path = extract_dir
        .join("artifacts")
        .join("evaluator-decision")
        .join("signature.sig");
    let public_key_path = extract_dir
        .join("artifacts")
        .join("evaluator-decision")
        .join("public.pem");

    let manifest = read_json_file_or_null(&manifest_path, &mut blockers, "manifest");
    let handoff = read_json_file_or_null(&handoff_path, &mut blockers, "release_handoff");
    let decision = read_json_file_or_null(&decision_path, &mut blockers, "evaluator_decision");

    let mut checksum_reasons = Vec::new();
    let checksum_manifest = match fs::read_to_string(&checksum_path) {
        Ok(body) => checksum_manifest_map(&body, &mut checksum_reasons),
        Err(error) => {
            blockers.push(serde_json::json!({
                "code": "sha256sums_missing",
                "severity": "blocker",
                "message": format!("read SHA256SUMS: {error}")
            }));
            BTreeMap::new()
        }
    };
    for reason in checksum_reasons {
        blockers.push(recovery_release_handoff_status_blocker(
            "sha256sums_invalid",
            reason,
        ));
    }

    let required_entries = [
        "manifest.json",
        "SHA256SUMS",
        "release-handoff.json",
        "artifacts/evaluator-decision/evaluator-decision.json",
        "artifacts/evaluator-decision/signed-payload.json",
        "artifacts/evaluator-decision/signature.sig",
        "artifacts/evaluator-decision/public.pem",
    ];
    let mut required_manifest_entries_present = true;
    for entry in required_entries {
        let covered_by_checksum = entry == "SHA256SUMS" || checksum_manifest.contains_key(entry);
        if !extract_dir.join(entry).is_file() || !covered_by_checksum {
            required_manifest_entries_present = false;
            blockers.push(serde_json::json!({
                "code": "required_release_handoff_entry_missing",
                "severity": "blocker",
                "path": entry,
                "message": "C71 release handoff bundle is missing a required artifact or checksum entry."
            }));
        }
    }

    let mut sha256sums_verified = !checksum_manifest.is_empty();
    let mut files_checked = 0_usize;
    for (relative_path, expected_sha256) in &checksum_manifest {
        if !recovery_release_handoff_relative_path_allowed(relative_path) {
            sha256sums_verified = false;
            blockers.push(serde_json::json!({
                "code": "unsafe_release_handoff_path",
                "severity": "blocker",
                "path": relative_path,
                "message": "SHA256SUMS contains an absolute or parent-directory path."
            }));
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if !file_path.is_file() {
            sha256sums_verified = false;
            blockers.push(serde_json::json!({
                "code": "checksummed_release_handoff_file_missing",
                "severity": "blocker",
                "path": relative_path,
                "message": "SHA256SUMS references a missing bundle file."
            }));
            continue;
        }
        match sha256_file(&file_path) {
            Ok(actual_sha256) if actual_sha256 == *expected_sha256 => {
                files_checked += 1;
            }
            Ok(actual_sha256) => {
                sha256sums_verified = false;
                blockers.push(serde_json::json!({
                    "code": "release_handoff_sha256_mismatch",
                    "severity": "blocker",
                    "path": relative_path,
                    "expected": expected_sha256,
                    "actual": actual_sha256,
                    "message": "Bundle file digest does not match SHA256SUMS."
                }));
            }
            Err(error) => {
                sha256sums_verified = false;
                blockers.push(serde_json::json!({
                    "code": "release_handoff_sha256_unreadable",
                    "severity": "blocker",
                    "path": relative_path,
                    "message": format!("hash bundle file: {error}")
                }));
            }
        }
    }

    let supplied_closure_sha256 = args.closure_sha256.trim().to_string();
    let supplied_decision_sha256 = args.decision_sha256.trim().to_string();
    let decision_file_sha256 = sha256_file(&decision_path).unwrap_or_default();
    let signature_verified =
        verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)
            .unwrap_or(false);
    let decision_signature = decision
        .get("signature")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let signature_metadata_verified = signature_verified
        && json_string(&decision_signature, "signed_payload_sha256")
            == sha256_file(&signed_payload_path).unwrap_or_default()
        && json_string(&decision_signature, "signature_sha256")
            == sha256_file(&signature_path).unwrap_or_default()
        && json_string(&decision_signature, "public_key_sha256")
            == sha256_file(&public_key_path).unwrap_or_default();
    if !signature_metadata_verified {
        blockers.push(serde_json::json!({
            "code": "release_handoff_signature_verification_failed",
            "severity": "blocker",
            "message": "Bundled C70 signature, signed payload, and public key did not verify against the C70 decision metadata."
        }));
    }

    let closure_digest_chain_verified = json_string(&handoff, "closure_sha256")
        == supplied_closure_sha256
        && json_string(&manifest, "closure_sha256") == supplied_closure_sha256
        && json_string(&decision, "closure_sha256") == supplied_closure_sha256
        && json_string(&decision, "expected_closure_sha256") == supplied_closure_sha256
        && decision["closure_digest_matches_current"] == true
        && handoff["closure_digest_matches_decision"] == true;
    if !closure_digest_chain_verified {
        blockers.push(serde_json::json!({
            "code": "release_handoff_closure_digest_chain_mismatch",
            "severity": "blocker",
            "message": "Supplied C69 closure digest does not match the bundled C70/C71 digest chain."
        }));
    }

    let decision_digest_chain_verified = supplied_decision_sha256 == decision_file_sha256
        && json_string(&handoff, "decision_sha256") == supplied_decision_sha256
        && json_string(&manifest, "decision_sha256") == supplied_decision_sha256
        && json_string(&handoff, "expected_decision_sha256") == supplied_decision_sha256
        && handoff["decision_digest_matches_current"] == true;
    if !decision_digest_chain_verified {
        blockers.push(serde_json::json!({
            "code": "release_handoff_decision_digest_chain_mismatch",
            "severity": "blocker",
            "message": "Supplied C70 decision digest does not match the bundled handoff and manifest chain."
        }));
    }

    let factory_v3_parity_expectations_verified = manifest["factory_v3_parity_oracle"]
        ["ready_for_comparison"]
        == true
        && handoff["factory_v3_parity_oracle"]["ready_for_comparison"] == true
        && handoff["factory_v3_parity_oracle"]["factory_v3_drives_workflow"] == false
        && json_string(
            &handoff["factory_v3_parity_oracle"],
            "release_acceptance_owner",
        ) == "factory-v3 evaluator-closer"
        && handoff["factory_v3_parity_oracle"]["expectations"]["decision_status_accepted"] == true
        && handoff["factory_v3_parity_oracle"]["expectations"]["decision_signature_verified"]
            == true
        && handoff["factory_v3_parity_oracle"]["expectations"]["closure_digest_bound"] == true
        && handoff["factory_v3_parity_oracle"]["expectations"]["decision_digest_bound"] == true
        && handoff["factory_v3_parity_oracle"]["expectations"]["ao2_release_approval"] == false
        && handoff["factory_v3_parity_oracle"]["expectations"]["control_plane_observer_only"]
            == true
        && handoff["trust_boundary"]["control_plane_approves_release"] == false
        && handoff["trust_boundary"]["mutates_ao_artifacts"] == false;
    if !factory_v3_parity_expectations_verified {
        blockers.push(serde_json::json!({
            "code": "factory_v3_release_handoff_parity_expectation_mismatch",
            "severity": "blocker",
            "message": "Bundled C71 release handoff does not preserve evaluator-closer parity expectations."
        }));
    }

    let secret_scan_passed =
        recovery_release_handoff_secret_scan(&extract_dir, checksum_manifest.keys(), &mut concerns);
    let checks = serde_json::json!({
        "archive_extracted": blockers.iter().all(|blocker| blocker["code"] != "release_handoff_bundle_extract_failed"),
        "sha256sums_verified": sha256sums_verified,
        "required_manifest_entries_present": required_manifest_entries_present,
        "signature_verified": signature_metadata_verified,
        "closure_digest_chain_verified": closure_digest_chain_verified,
        "decision_digest_chain_verified": decision_digest_chain_verified,
        "factory_v3_parity_expectations_verified": factory_v3_parity_expectations_verified,
        "secret_scan_passed": secret_scan_passed
    });
    let verified = checks
        .as_object()
        .map(|object| object.values().all(|value| value == true))
        .unwrap_or(false)
        && blockers.is_empty();
    let status = if verified { "verified" } else { "blocked" };
    let run_id = json_string(&handoff, "run_id");
    let hermes_next = if verified {
        "factory_v3_evaluator_closer_compare_c72_verified_release_handoff_status_and_prepare_control_plane_observer_readback"
    } else {
        "route_c72_release_handoff_status_blockers_to_ao2_recovery_repair"
    };

    let _ = fs::remove_dir_all(&extract_dir);
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "read_only": true,
        "run_id": run_id,
        "target": args.target.display().to_string(),
        "bundle": args.bundle.display().to_string(),
        "bundle_sha256": bundle_sha256,
        "closure_sha256": supplied_closure_sha256,
        "decision_sha256": supplied_decision_sha256,
        "files_checked": files_checked,
        "checks": checks,
        "manifest": {
            "schema_version": json_string(&manifest, "schema_version"),
            "status": json_string(&manifest, "status"),
            "signature_verified": manifest["signature_verified"].clone(),
            "file_count": manifest.get("files").and_then(|files| files.as_array()).map(|files| files.len()).unwrap_or(0)
        },
        "release_handoff": {
            "schema_version": json_string(&handoff, "schema_version"),
            "status": json_string(&handoff, "status"),
            "signature_algorithm": json_string(&handoff, "signature_algorithm"),
            "signer_id": json_string(&handoff, "signer_id")
        },
        "factory_v3_parity_oracle": {
            "schema_version": "ao2.factory-v3-evaluator-closer-release-handoff-status-parity.v1",
            "ready_for_comparison": factory_v3_parity_expectations_verified,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_observer_only": true
        },
        "hermes_status": {
            "compact_status_packet": true,
            "raw_archive_interpretation_required": false,
            "factory_v3_required_to_drive_workflow": false,
            "next_recommended_action": hermes_next
        },
        "concerns": concerns,
        "blockers": blockers,
        "changed_files": handoff.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "side_effects": {
            "would_extract_archive_to_temp_status_dir": true,
            "would_write_release_handoff_bundle": false,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": {
            "decision_owner": "ao2",
            "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-release-handoff-status"
    }))
}

pub(crate) struct RecoveryResumePostContinuationReleaseHandoffStatusSummaryArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) status: &'a Path,
    pub(crate) status_sha256: &'a str,
    pub(crate) out: &'a Path,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_json(
    args: RecoveryResumePostContinuationReleaseHandoffStatusSummaryArgs<'_>,
) -> Result<serde_json::Value> {
    if !args.status.is_file() {
        anyhow::bail!(
            "missing recovery release handoff status packet: {}",
            args.status.display()
        );
    }
    let supplied_status_sha256 = args.status_sha256.trim();
    let actual_status_sha256 = sha256_file(args.status)?;
    if supplied_status_sha256 != actual_status_sha256 {
        anyhow::bail!(
            "status_sha256 mismatch for {}: expected {}, actual {}",
            args.status.display(),
            supplied_status_sha256,
            actual_status_sha256
        );
    }

    let status_packet: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(args.status)
            .with_context(|| format!("read {}", args.status.display()))?,
    )
    .with_context(|| format!("parse {}", args.status.display()))?;
    let schema = json_string(&status_packet, "schema_version");
    if schema
        != "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status.v1"
    {
        anyhow::bail!("release handoff status summary requires C72 status schema, got {schema}");
    }

    let mut blockers = status_packet
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if json_string(&status_packet, "status") != "verified" {
        blockers.push(serde_json::json!({
            "code": "release_handoff_status_not_verified",
            "severity": "blocker",
            "message": "C73 bookkeeping summary requires a verified C72 release handoff status packet."
        }));
    }
    let status = if blockers.is_empty() {
        "recorded"
    } else {
        "blocked"
    };
    let next_recommended_action =
        json_string(&status_packet["hermes_status"], "next_recommended_action");
    let trust_boundary = status_packet
        .get("trust_boundary")
        .cloned()
        .unwrap_or_else(|| {
            serde_json::json!({
                "decision_owner": "ao2",
                "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "control_plane_approves_release": false,
                "mutates_ao_artifacts": false,
                "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
            })
        });
    let summary = serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status-summary.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "target": args.target.display().to_string(),
        "status_path": args.status.display().to_string(),
        "status_sha256": actual_status_sha256,
        "summary_path": args.out.display().to_string(),
        "status_packet": {
            "schema_version": schema,
            "status": json_string(&status_packet, "status"),
            "run_id": json_string(&status_packet, "run_id"),
            "bundle_sha256": json_string(&status_packet, "bundle_sha256"),
            "closure_sha256": json_string(&status_packet, "closure_sha256"),
            "decision_sha256": json_string(&status_packet, "decision_sha256"),
            "files_checked": status_packet.get("files_checked").cloned().unwrap_or_else(|| serde_json::json!(0))
        },
        "status_checks": status_packet.get("checks").cloned().unwrap_or_else(|| serde_json::json!({})),
        "hermes_bookkeeping": {
            "compact_memory_summary": true,
            "exact_status_digest_required": true,
            "exact_next_action_recorded": true,
            "next_recommended_action": next_recommended_action,
            "raw_archive_interpretation_required": false,
            "raw_status_chain_recompute_required": false,
            "survives_scheduler_ticks": true
        },
        "evidence": [
            {
                "kind": "c72_release_handoff_status_packet",
                "path": args.status.display().to_string(),
                "sha256": actual_status_sha256
            }
        ],
        "concerns": status_packet.get("concerns").cloned().unwrap_or_else(|| serde_json::json!([])),
        "blockers": blockers,
        "changed_files": status_packet.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "factory_v3_parity_owner": "factory-v3 evaluator-closer",
        "factory_v3_parity_oracle": status_packet.get("factory_v3_parity_oracle").cloned().unwrap_or_else(|| serde_json::json!({
            "ready_for_comparison": false,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_observer_only": true
        })),
        "side_effects": {
            "wrote_bookkeeping_summary_artifact": true,
            "would_reinterpret_archive": false,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-release-handoff-status-summary"
    });
    atomic_write_text(args.out, &serde_json::to_string_pretty(&summary)?)?;
    Ok(summary)
}

pub(crate) struct RecoveryResumePostContinuationReleaseHandoffStatusSummaryExportArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) summary: &'a Path,
    pub(crate) summary_sha256: &'a str,
    pub(crate) out: &'a Path,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_export_json(
    args: RecoveryResumePostContinuationReleaseHandoffStatusSummaryExportArgs<'_>,
) -> Result<serde_json::Value> {
    if !args.summary.is_file() {
        anyhow::bail!(
            "missing recovery release handoff status summary: {}",
            args.summary.display()
        );
    }
    let supplied_summary_sha256 = args.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(args.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "summary_sha256 mismatch for {}: expected {}, actual {}",
            args.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(args.summary)
            .with_context(|| format!("read {}", args.summary.display()))?,
    )
    .with_context(|| format!("parse {}", args.summary.display()))?;
    let schema = json_string(&summary, "schema_version");
    if schema
        != "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status-summary.v1"
    {
        anyhow::bail!("release handoff status summary export requires C73 summary schema, got {schema}");
    }

    let mut blockers = summary
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if json_string(&summary, "status") != "recorded" {
        blockers.push(serde_json::json!({
            "code": "release_handoff_status_summary_not_recorded",
            "severity": "blocker",
            "message": "C74 observer export requires a recorded C73 release handoff status summary."
        }));
    }
    let status_sha256 = json_string(&summary, "status_sha256");
    if status_sha256.is_empty() {
        blockers.push(serde_json::json!({
            "code": "release_handoff_status_digest_missing",
            "severity": "blocker",
            "message": "C74 observer export requires the C72 status digest preserved by C73."
        }));
    }
    let status = if blockers.is_empty() {
        "exported"
    } else {
        "blocked"
    };
    let next_recommended_action =
        json_string(&summary["hermes_bookkeeping"], "next_recommended_action");
    let trust_boundary = summary.get("trust_boundary").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "decision_owner": "ao2",
            "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        })
    });
    let observer_fixture = serde_json::json!({
        "schema_version": "ao2.control-plane.recovery-release-handoff-status-summary-observer-fixture.v1",
        "producer": "ao2",
        "consumer": "ao2-control-plane K37",
        "status": status,
        "target": args.target.display().to_string(),
        "summary_path": args.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256.clone(),
        "status_sha256": status_sha256.clone(),
        "run_id": json_string(&summary["status_packet"], "run_id"),
        "bundle_sha256": json_string(&summary["status_packet"], "bundle_sha256"),
        "closure_sha256": json_string(&summary["status_packet"], "closure_sha256"),
        "decision_sha256": json_string(&summary["status_packet"], "decision_sha256"),
        "next_recommended_action": next_recommended_action.clone(),
        "status_checks": summary.get("status_checks").cloned().unwrap_or_else(|| serde_json::json!({})),
        "concerns": summary.get("concerns").cloned().unwrap_or_else(|| serde_json::json!([])),
        "blockers": blockers.clone(),
        "changed_files": summary.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "trust_boundary": trust_boundary.clone(),
        "factory_v3_parity_owner": "factory-v3 evaluator-closer",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_may_produce_evidence": false,
        "control_plane_may_approve_release": false,
        "control_plane_may_mutate_ao_artifacts": false
    });
    let observer_fixture_sha256 = canonical_json_sha256(&observer_fixture);
    let export = serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status-summary-export.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "target": args.target.display().to_string(),
        "summary_path": args.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256.clone(),
        "status_sha256": status_sha256.clone(),
        "export_path": args.out.display().to_string(),
        "observer_fixture": observer_fixture,
        "observer_fixture_sha256": observer_fixture_sha256.clone(),
        "publication_contract": {
            "digest_bound": true,
            "control_plane_observer_fixture": true,
            "producer": "ao2",
            "consumer": "ao2-control-plane K37",
            "summary_digest_required": true,
            "status_digest_preserved": true,
            "control_plane_may_recompute_recovery_chain": false,
            "control_plane_may_produce_evidence": false,
            "control_plane_may_approve_release": false,
            "control_plane_may_mutate_ao_artifacts": false,
            "factory_v3_accepts_release": true
        },
        "hermes_publication": {
            "portable_observer_fixture": true,
            "memory_publication_ready": blockers.is_empty(),
            "next_recommended_action": next_recommended_action.clone(),
            "raw_archive_interpretation_required": false,
            "raw_status_chain_recompute_required": false,
            "control_plane_write_required": false
        },
        "status_packet": summary.get("status_packet").cloned().unwrap_or_else(|| serde_json::json!({})),
        "status_checks": summary.get("status_checks").cloned().unwrap_or_else(|| serde_json::json!({})),
        "evidence": [
            {
                "kind": "c72_release_handoff_status_packet",
                "sha256": status_sha256.clone()
            },
            {
                "kind": "c73_release_handoff_status_summary",
                "path": args.summary.display().to_string(),
                "sha256": actual_summary_sha256.clone()
            },
            {
                "kind": "c74_release_handoff_status_summary_observer_fixture",
                "sha256": observer_fixture_sha256.clone()
            }
        ],
        "concerns": summary.get("concerns").cloned().unwrap_or_else(|| serde_json::json!([])),
        "blockers": blockers.clone(),
        "changed_files": summary.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "factory_v3_parity_owner": "factory-v3 evaluator-closer",
        "factory_v3_parity_oracle": summary.get("factory_v3_parity_oracle").cloned().unwrap_or_else(|| serde_json::json!({
            "ready_for_comparison": false,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_observer_only": true
        })),
        "side_effects": {
            "wrote_summary_export_artifact": true,
            "would_reinterpret_archive": false,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": trust_boundary.clone(),
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-release-handoff-status-summary-export"
    });
    atomic_write_text(args.out, &serde_json::to_string_pretty(&export)?)?;
    Ok(export)
}

pub(crate) struct RecoveryResumePostContinuationReleasePublicationReadinessArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) export: &'a Path,
    pub(crate) export_sha256: &'a str,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_publication_readiness_json(
    args: RecoveryResumePostContinuationReleasePublicationReadinessArgs<'_>,
) -> Result<serde_json::Value> {
    if !args.export.is_file() {
        anyhow::bail!(
            "missing recovery release handoff status summary export: {}",
            args.export.display()
        );
    }
    let supplied_export_sha256 = args.export_sha256.trim();
    let actual_export_sha256 = sha256_file(args.export)?;
    if supplied_export_sha256 != actual_export_sha256 {
        anyhow::bail!(
            "export_sha256 mismatch for {}: expected {}, actual {}",
            args.export.display(),
            supplied_export_sha256,
            actual_export_sha256
        );
    }

    let export: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(args.export)
            .with_context(|| format!("read {}", args.export.display()))?,
    )
    .with_context(|| format!("parse {}", args.export.display()))?;
    let schema = json_string(&export, "schema_version");
    if schema
        != "ao2.factory-project-start-recovery-resume-post-continuation-release-handoff-status-summary-export.v1"
    {
        anyhow::bail!("release publication readiness requires C74 summary export schema, got {schema}");
    }

    let mut blockers = export
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if json_string(&export, "status") != "exported" {
        blockers.push(serde_json::json!({
            "code": "release_handoff_status_summary_export_not_exported",
            "severity": "blocker",
            "message": "C75 publication readiness requires an exported C74 release handoff status summary export."
        }));
    }
    let observer_fixture = export
        .get("observer_fixture")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let expected_observer_fixture_sha256 = json_string(&export, "observer_fixture_sha256");
    let actual_observer_fixture_sha256 = if observer_fixture.is_object() {
        canonical_json_sha256(&observer_fixture)
    } else {
        String::new()
    };
    if expected_observer_fixture_sha256.is_empty() {
        blockers.push(serde_json::json!({
            "code": "observer_fixture_digest_missing",
            "severity": "blocker",
            "message": "C75 publication readiness requires the C74 observer fixture digest."
        }));
    } else if expected_observer_fixture_sha256 != actual_observer_fixture_sha256 {
        blockers.push(serde_json::json!({
            "code": "observer_fixture_digest_mismatch",
            "severity": "blocker",
            "message": "C75 publication readiness requires the recomputed observer fixture digest to match C74."
        }));
    }

    let summary_sha256 = json_string(&export, "summary_sha256");
    let status_sha256 = json_string(&export, "status_sha256");
    if summary_sha256.is_empty() {
        blockers.push(serde_json::json!({
            "code": "summary_digest_missing",
            "severity": "blocker",
            "message": "C75 publication readiness requires the C73 summary digest preserved by C74."
        }));
    }
    if status_sha256.is_empty() {
        blockers.push(serde_json::json!({
            "code": "status_digest_missing",
            "severity": "blocker",
            "message": "C75 publication readiness requires the C72 status digest preserved by C74."
        }));
    }

    let publication_contract = export
        .get("publication_contract")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let control_plane_observer_fixture = publication_contract
        .get("control_plane_observer_fixture")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let control_plane_may_approve_release = publication_contract
        .get("control_plane_may_approve_release")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let control_plane_may_mutate_ao_artifacts = publication_contract
        .get("control_plane_may_mutate_ao_artifacts")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    if !control_plane_observer_fixture
        || control_plane_may_approve_release
        || control_plane_may_mutate_ao_artifacts
    {
        blockers.push(serde_json::json!({
            "code": "publication_contract_boundary_invalid",
            "severity": "blocker",
            "message": "C75 publication readiness requires C74 to remain an observer-only control-plane fixture."
        }));
    }

    let hermes_publication = export
        .get("hermes_publication")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let c74_memory_ready = hermes_publication
        .get("memory_publication_ready")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !c74_memory_ready {
        blockers.push(serde_json::json!({
            "code": "hermes_memory_publication_not_ready",
            "severity": "blocker",
            "message": "C75 publication readiness requires C74 to report Hermes memory publication readiness."
        }));
    }

    let status = if blockers.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    let trust_boundary = export.get("trust_boundary").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "decision_owner": "ao2",
            "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        })
    });
    let next_recommended_action = json_string(&hermes_publication, "next_recommended_action");
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-readiness.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "target": args.target.display().to_string(),
        "export_path": args.export.display().to_string(),
        "export_sha256": actual_export_sha256,
        "summary_sha256": summary_sha256,
        "status_sha256": status_sha256,
        "observer_fixture_sha256": expected_observer_fixture_sha256,
        "checks": {
            "exact_export_digest_verified": true,
            "observer_fixture_digest_verified": expected_observer_fixture_sha256 == actual_observer_fixture_sha256 && !expected_observer_fixture_sha256.is_empty(),
            "summary_digest_preserved": !json_string(&export, "summary_sha256").is_empty(),
            "status_digest_preserved": !json_string(&export, "status_sha256").is_empty(),
            "control_plane_observer_only": control_plane_observer_fixture && !control_plane_may_approve_release && !control_plane_may_mutate_ao_artifacts,
            "factory_v3_release_acceptance_owner_preserved": json_string(&trust_boundary, "release_acceptance_owner") == "factory-v3 evaluator-closer",
            "hermes_memory_publication_ready": c74_memory_ready
        },
        "publication_contract": {
            "digest_bound": true,
            "requires_exact_c74_export_digest": true,
            "recomputed_observer_fixture_digest": true,
            "producer": "ao2",
            "consumer": "Hermes memory bookkeeping and ao2-control-plane K37 read-only observer",
            "hermes_may_publish_memory_bookkeeping": blockers.is_empty(),
            "control_plane_may_read_fixture": blockers.is_empty(),
            "control_plane_may_produce_evidence": false,
            "control_plane_may_approve_release": false,
            "control_plane_may_mutate_ao_artifacts": false,
            "factory_v3_accepts_release": true
        },
        "hermes_publication": {
            "memory_bookkeeping_ready": blockers.is_empty(),
            "control_plane_bookkeeping_ready": blockers.is_empty(),
            "portable_observer_fixture": true,
            "next_recommended_action": next_recommended_action,
            "raw_archive_interpretation_required": false,
            "raw_status_chain_recompute_required": false,
            "control_plane_write_required": false
        },
        "status_packet": export.get("status_packet").cloned().unwrap_or_else(|| serde_json::json!({})),
        "status_checks": export.get("status_checks").cloned().unwrap_or_else(|| serde_json::json!({})),
        "evidence": [
            {
                "kind": "c72_release_handoff_status_packet",
                "sha256": json_string(&export, "status_sha256")
            },
            {
                "kind": "c73_release_handoff_status_summary",
                "path": json_string(&export, "summary_path"),
                "sha256": json_string(&export, "summary_sha256")
            },
            {
                "kind": "c74_release_handoff_status_summary_export",
                "path": args.export.display().to_string(),
                "sha256": actual_export_sha256
            },
            {
                "kind": "c74_release_handoff_status_summary_observer_fixture",
                "sha256": expected_observer_fixture_sha256
            }
        ],
        "concerns": export.get("concerns").cloned().unwrap_or_else(|| serde_json::json!([])),
        "blockers": blockers,
        "changed_files": export.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "factory_v3_parity_owner": "factory-v3 evaluator-closer",
        "factory_v3_parity_oracle": export.get("factory_v3_parity_oracle").cloned().unwrap_or_else(|| serde_json::json!({
            "ready_for_comparison": false,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_observer_only": true
        })),
        "side_effects": {
            "would_write_publication_readiness_artifact": false,
            "would_reinterpret_archive": false,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-release-publication-readiness"
    }))
}

pub(crate) struct RecoveryResumePostContinuationReleasePublicationDispatchPlanArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) readiness: &'a Path,
    pub(crate) readiness_sha256: &'a str,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_publication_dispatch_plan_json(
    args: RecoveryResumePostContinuationReleasePublicationDispatchPlanArgs<'_>,
) -> Result<serde_json::Value> {
    if !args.readiness.is_file() {
        anyhow::bail!(
            "missing recovery release publication readiness packet: {}",
            args.readiness.display()
        );
    }
    let supplied_readiness_sha256 = args.readiness_sha256.trim();
    let actual_readiness_sha256 = sha256_file(args.readiness)?;
    if supplied_readiness_sha256 != actual_readiness_sha256 {
        anyhow::bail!(
            "readiness_sha256 mismatch for {}: expected {}, actual {}",
            args.readiness.display(),
            supplied_readiness_sha256,
            actual_readiness_sha256
        );
    }

    let readiness: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(args.readiness)
            .with_context(|| format!("read {}", args.readiness.display()))?,
    )
    .with_context(|| format!("parse {}", args.readiness.display()))?;
    let schema = json_string(&readiness, "schema_version");
    if schema
        != "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-readiness.v1"
    {
        anyhow::bail!("release publication dispatch plan requires C75 readiness schema, got {schema}");
    }

    let mut blockers = readiness
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if json_string(&readiness, "status") != "ready" {
        blockers.push(serde_json::json!({
            "code": "release_publication_readiness_not_ready",
            "severity": "blocker",
            "message": "C76 dispatch plan requires a ready C75 publication readiness packet."
        }));
    }
    let c75_checks = readiness
        .get("checks")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let readiness_packet_ready = json_string(&readiness, "status") == "ready";
    let observer_fixture_digest_verified = c75_checks
        .get("observer_fixture_digest_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !observer_fixture_digest_verified {
        blockers.push(serde_json::json!({
            "code": "observer_fixture_digest_not_verified",
            "severity": "blocker",
            "message": "C76 dispatch plan requires C75 to verify the nested observer fixture digest."
        }));
    }
    let hermes_publication = readiness
        .get("hermes_publication")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let memory_bookkeeping_ready = hermes_publication
        .get("memory_bookkeeping_ready")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let control_plane_bookkeeping_ready = hermes_publication
        .get("control_plane_bookkeeping_ready")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !memory_bookkeeping_ready || !control_plane_bookkeeping_ready {
        blockers.push(serde_json::json!({
            "code": "publication_bookkeeping_not_ready",
            "severity": "blocker",
            "message": "C76 dispatch plan requires C75 to mark Hermes memory and control-plane bookkeeping ready."
        }));
    }

    let status = if blockers.is_empty() {
        "planned"
    } else {
        "blocked"
    };
    let trust_boundary = readiness.get("trust_boundary").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "decision_owner": "ao2",
            "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        })
    });
    let next_recommended_action = json_string(&hermes_publication, "next_recommended_action");
    let export_sha256 = json_string(&readiness, "export_sha256");
    let summary_sha256 = json_string(&readiness, "summary_sha256");
    let status_sha256 = json_string(&readiness, "status_sha256");
    let observer_fixture_sha256 = json_string(&readiness, "observer_fixture_sha256");
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-dispatch-plan.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "target": args.target.display().to_string(),
        "readiness_path": args.readiness.display().to_string(),
        "readiness_sha256": actual_readiness_sha256,
        "export_sha256": export_sha256,
        "summary_sha256": summary_sha256,
        "status_sha256": status_sha256,
        "observer_fixture_sha256": observer_fixture_sha256,
        "checks": {
            "exact_readiness_digest_verified": true,
            "readiness_packet_ready": readiness_packet_ready,
            "observer_fixture_digest_verified": observer_fixture_digest_verified,
            "memory_bookkeeping_ready": memory_bookkeeping_ready,
            "control_plane_bookkeeping_ready": control_plane_bookkeeping_ready,
            "control_plane_observer_only": json_string(&trust_boundary, "control_plane_role") == "read_only_observer_after_signed_evidence" && !trust_boundary.get("control_plane_approves_release").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "factory_v3_release_acceptance_owner_preserved": json_string(&trust_boundary, "release_acceptance_owner") == "factory-v3 evaluator-closer"
        },
        "dispatch_plan": {
            "mode": "planned_only",
            "producer": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "hermes_memory_bookkeeping": {
                "mode": "planned_only",
                "surface": "Hermes front end, queue, cron, and memory bookkeeping",
                "ready": blockers.is_empty(),
                "source_readiness_sha256": actual_readiness_sha256,
                "source_export_sha256": export_sha256,
                "would_write_memory": false
            },
            "control_plane_readback": {
                "mode": "planned_only",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "ready": blockers.is_empty(),
                "source_readiness_sha256": actual_readiness_sha256,
                "source_observer_fixture_sha256": observer_fixture_sha256,
                "would_mutate_control_plane": false,
                "would_approve_release": false
            },
            "scheduler_policy": {
                "governed_scheduler_required": true,
                "direct_mutation_allowed": false,
                "factory_v3_evaluator_closer_sampling_required": true,
                "next_recommended_action": next_recommended_action
            }
        },
        "publication_contract": {
            "digest_bound": true,
            "requires_exact_c75_readiness_digest": true,
            "producer": "ao2",
            "consumer": "Hermes governed scheduler and ao2-control-plane read-only observer",
            "hermes_may_use_as_dispatch_plan": blockers.is_empty(),
            "control_plane_may_read_fixture": blockers.is_empty(),
            "control_plane_may_produce_evidence": false,
            "control_plane_may_approve_release": false,
            "control_plane_may_mutate_ao_artifacts": false,
            "factory_v3_accepts_release": true
        },
        "status_packet": readiness.get("status_packet").cloned().unwrap_or_else(|| serde_json::json!({})),
        "status_checks": readiness.get("status_checks").cloned().unwrap_or_else(|| serde_json::json!({})),
        "evidence": [
            {
                "kind": "c72_release_handoff_status_packet",
                "sha256": status_sha256
            },
            {
                "kind": "c73_release_handoff_status_summary",
                "sha256": summary_sha256
            },
            {
                "kind": "c74_release_handoff_status_summary_export",
                "sha256": export_sha256
            },
            {
                "kind": "c75_release_publication_readiness",
                "path": args.readiness.display().to_string(),
                "sha256": actual_readiness_sha256
            }
        ],
        "concerns": readiness.get("concerns").cloned().unwrap_or_else(|| serde_json::json!([])),
        "blockers": blockers,
        "changed_files": readiness.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "factory_v3_parity_owner": "factory-v3 evaluator-closer",
        "factory_v3_parity_oracle": readiness.get("factory_v3_parity_oracle").cloned().unwrap_or_else(|| serde_json::json!({
            "ready_for_comparison": false,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_observer_only": true
        })),
        "side_effects": {
            "would_write_dispatch_plan_artifact": false,
            "would_reinterpret_archive": false,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-release-publication-dispatch-plan"
    }))
}

pub(crate) struct RecoveryResumePostContinuationReleasePublicationReadbackArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) dispatch_plan: &'a Path,
    pub(crate) dispatch_plan_sha256: &'a str,
    pub(crate) observation: &'a Path,
    pub(crate) observation_sha256: &'a str,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_publication_readback_json(
    args: RecoveryResumePostContinuationReleasePublicationReadbackArgs<'_>,
) -> Result<serde_json::Value> {
    if !args.dispatch_plan.is_file() {
        anyhow::bail!(
            "missing recovery release publication dispatch plan: {}",
            args.dispatch_plan.display()
        );
    }
    if !args.observation.is_file() {
        anyhow::bail!(
            "missing recovery release publication observation: {}",
            args.observation.display()
        );
    }
    let supplied_dispatch_plan_sha256 = args.dispatch_plan_sha256.trim();
    let actual_dispatch_plan_sha256 = sha256_file(args.dispatch_plan)?;
    if supplied_dispatch_plan_sha256 != actual_dispatch_plan_sha256 {
        anyhow::bail!(
            "dispatch_plan_sha256 mismatch for {}: expected {}, actual {}",
            args.dispatch_plan.display(),
            supplied_dispatch_plan_sha256,
            actual_dispatch_plan_sha256
        );
    }
    let supplied_observation_sha256 = args.observation_sha256.trim();
    let actual_observation_sha256 = sha256_file(args.observation)?;
    if supplied_observation_sha256 != actual_observation_sha256 {
        anyhow::bail!(
            "observation_sha256 mismatch for {}: expected {}, actual {}",
            args.observation.display(),
            supplied_observation_sha256,
            actual_observation_sha256
        );
    }

    let dispatch_plan: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(args.dispatch_plan)
            .with_context(|| format!("read {}", args.dispatch_plan.display()))?,
    )
    .with_context(|| format!("parse {}", args.dispatch_plan.display()))?;
    let dispatch_schema = json_string(&dispatch_plan, "schema_version");
    if dispatch_schema
        != "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-dispatch-plan.v1"
    {
        anyhow::bail!("release publication readback requires C76 dispatch plan schema, got {dispatch_schema}");
    }
    let observation: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(args.observation)
            .with_context(|| format!("read {}", args.observation.display()))?,
    )
    .with_context(|| format!("parse {}", args.observation.display()))?;
    let observation_schema = json_string(&observation, "schema_version");
    if observation_schema != "ao2.hermes-recovery-publication-observation.v1" {
        anyhow::bail!(
            "release publication readback requires Hermes publication observation schema, got {observation_schema}"
        );
    }

    let mut blockers = dispatch_plan
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    blockers.extend(
        observation
            .get("blockers")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default(),
    );
    if json_string(&dispatch_plan, "status") != "planned" {
        blockers.push(serde_json::json!({
            "code": "release_publication_dispatch_plan_not_planned",
            "severity": "blocker",
            "message": "C77 publication readback requires a planned C76 dispatch packet."
        }));
    }
    if json_string(&observation, "status") != "observed" {
        blockers.push(serde_json::json!({
            "code": "release_publication_observation_not_observed",
            "severity": "blocker",
            "message": "C77 publication readback requires an observed external publication observation."
        }));
    }
    if json_string(&observation, "dispatch_plan_sha256") != actual_dispatch_plan_sha256 {
        blockers.push(serde_json::json!({
            "code": "observation_dispatch_plan_digest_mismatch",
            "severity": "blocker",
            "message": "C77 publication readback requires the observation to bind to the exact C76 dispatch plan digest."
        }));
    }

    let readiness_sha256 = json_string(&dispatch_plan, "readiness_sha256");
    let export_sha256 = json_string(&dispatch_plan, "export_sha256");
    let summary_sha256 = json_string(&dispatch_plan, "summary_sha256");
    let status_sha256 = json_string(&dispatch_plan, "status_sha256");
    let observer_fixture_sha256 = json_string(&dispatch_plan, "observer_fixture_sha256");
    for (field, expected) in [
        ("readiness_sha256", readiness_sha256.as_str()),
        ("export_sha256", export_sha256.as_str()),
        ("observer_fixture_sha256", observer_fixture_sha256.as_str()),
    ] {
        let actual = json_string(&observation, field);
        if actual != expected {
            blockers.push(serde_json::json!({
                "code": format!("observation_{field}_mismatch"),
                "severity": "blocker",
                "message": format!("C77 publication readback requires observation {field} to match the C76 dispatch plan.")
            }));
        }
    }

    let memory_observed = observation["hermes_memory_bookkeeping"]
        .get("published")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        && json_string(
            &observation["hermes_memory_bookkeeping"],
            "source_dispatch_plan_sha256",
        ) == actual_dispatch_plan_sha256
        && !observation["hermes_memory_bookkeeping"]
            .get("would_write_memory_from_readback")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
    if !memory_observed {
        blockers.push(serde_json::json!({
            "code": "hermes_memory_bookkeeping_not_observed",
            "severity": "blocker",
            "message": "C77 publication readback requires externally observed Hermes memory bookkeeping for the C76 dispatch plan."
        }));
    }
    let control_plane_observed = observation["control_plane_readback"]
        .get("observed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        && json_string(
            &observation["control_plane_readback"],
            "source_dispatch_plan_sha256",
        ) == actual_dispatch_plan_sha256
        && json_string(&observation["control_plane_readback"], "control_plane_role")
            == "read_only_observer_after_signed_evidence"
        && !observation["control_plane_readback"]
            .get("would_mutate_control_plane_from_readback")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
        && !observation["control_plane_readback"]
            .get("would_approve_release_from_readback")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
    if !control_plane_observed {
        blockers.push(serde_json::json!({
            "code": "control_plane_readback_not_observed",
            "severity": "blocker",
            "message": "C77 publication readback requires externally observed read-only control-plane readback for the C76 dispatch plan."
        }));
    }

    let status = if blockers.is_empty() {
        "verified"
    } else {
        "blocked"
    };
    let trust_boundary = dispatch_plan
        .get("trust_boundary")
        .cloned()
        .unwrap_or_else(|| {
            serde_json::json!({
                "decision_owner": "ao2",
                "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "control_plane_approves_release": false,
                "mutates_ao_artifacts": false,
                "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
            })
        });
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-readback.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "target": args.target.display().to_string(),
        "dispatch_plan_path": args.dispatch_plan.display().to_string(),
        "dispatch_plan_sha256": actual_dispatch_plan_sha256,
        "observation_path": args.observation.display().to_string(),
        "observation_sha256": actual_observation_sha256,
        "readiness_sha256": readiness_sha256,
        "export_sha256": export_sha256,
        "summary_sha256": summary_sha256,
        "status_sha256": status_sha256,
        "observer_fixture_sha256": observer_fixture_sha256,
        "checks": {
            "exact_dispatch_plan_digest_verified": true,
            "exact_observation_digest_verified": true,
            "dispatch_plan_planned": json_string(&dispatch_plan, "status") == "planned",
            "observation_observed": json_string(&observation, "status") == "observed",
            "observation_binds_dispatch_plan": json_string(&observation, "dispatch_plan_sha256") == actual_dispatch_plan_sha256,
            "hermes_memory_bookkeeping_observed": memory_observed,
            "control_plane_readback_observed": control_plane_observed,
            "control_plane_observer_only": json_string(&trust_boundary, "control_plane_role") == "read_only_observer_after_signed_evidence" && !trust_boundary.get("control_plane_approves_release").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "factory_v3_release_acceptance_owner_preserved": json_string(&trust_boundary, "release_acceptance_owner") == "factory-v3 evaluator-closer"
        },
        "publication_readback": {
            "mode": "external_observation_only",
            "verified": blockers.is_empty(),
            "hermes_memory_bookkeeping": observation.get("hermes_memory_bookkeeping").cloned().unwrap_or_else(|| serde_json::json!({})),
            "control_plane_readback": observation.get("control_plane_readback").cloned().unwrap_or_else(|| serde_json::json!({})),
            "would_write_memory": false,
            "would_mutate_control_plane": false,
            "would_approve_release": false
        },
        "publication_contract": {
            "digest_bound": true,
            "requires_exact_c76_dispatch_plan_digest": true,
            "requires_exact_external_observation_digest": true,
            "producer": "ao2",
            "consumer": "Hermes governed scheduler and ao2-control-plane read-only observer",
            "observation_is_external": true,
            "control_plane_may_produce_evidence": false,
            "control_plane_may_approve_release": false,
            "control_plane_may_mutate_ao_artifacts": false,
            "factory_v3_accepts_release": true
        },
        "status_packet": dispatch_plan.get("status_packet").cloned().unwrap_or_else(|| serde_json::json!({})),
        "status_checks": dispatch_plan.get("status_checks").cloned().unwrap_or_else(|| serde_json::json!({})),
        "evidence": [
            {
                "kind": "c72_release_handoff_status_packet",
                "sha256": status_sha256
            },
            {
                "kind": "c73_release_handoff_status_summary",
                "sha256": summary_sha256
            },
            {
                "kind": "c74_release_handoff_status_summary_export",
                "sha256": export_sha256
            },
            {
                "kind": "c75_release_publication_readiness",
                "sha256": readiness_sha256
            },
            {
                "kind": "c76_release_publication_dispatch_plan",
                "path": args.dispatch_plan.display().to_string(),
                "sha256": actual_dispatch_plan_sha256
            },
            {
                "kind": "c77_external_publication_observation",
                "path": args.observation.display().to_string(),
                "sha256": actual_observation_sha256
            }
        ],
        "concerns": dispatch_plan.get("concerns").cloned().unwrap_or_else(|| serde_json::json!([])),
        "blockers": blockers,
        "changed_files": dispatch_plan.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "factory_v3_parity_owner": "factory-v3 evaluator-closer",
        "factory_v3_parity_oracle": dispatch_plan.get("factory_v3_parity_oracle").cloned().unwrap_or_else(|| serde_json::json!({
            "ready_for_comparison": false,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_observer_only": true
        })),
        "side_effects": {
            "would_write_readback_artifact": false,
            "would_reinterpret_archive": false,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_approve_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-release-publication-readback"
    }))
}

pub(crate) struct RecoveryResumePostContinuationReleasePublicationClosureArgs<'a> {
    pub(crate) target: &'a Path,
    pub(crate) readback: &'a Path,
    pub(crate) readback_sha256: &'a str,
}

pub(crate) fn factory_queue_project_start_recovery_resume_post_continuation_release_publication_closure_json(
    args: RecoveryResumePostContinuationReleasePublicationClosureArgs<'_>,
) -> Result<serde_json::Value> {
    if !args.readback.is_file() {
        anyhow::bail!(
            "missing recovery release publication readback packet: {}",
            args.readback.display()
        );
    }
    let supplied_readback_sha256 = args.readback_sha256.trim();
    let actual_readback_sha256 = sha256_file(args.readback)?;
    if supplied_readback_sha256 != actual_readback_sha256 {
        anyhow::bail!(
            "readback_sha256 mismatch for {}: expected {}, actual {}",
            args.readback.display(),
            supplied_readback_sha256,
            actual_readback_sha256
        );
    }

    let readback: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(args.readback)
            .with_context(|| format!("read {}", args.readback.display()))?,
    )
    .with_context(|| format!("parse {}", args.readback.display()))?;
    let readback_schema = json_string(&readback, "schema_version");
    if readback_schema
        != "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-readback.v1"
    {
        anyhow::bail!(
            "release publication closure requires C77 readback schema, got {readback_schema}"
        );
    }

    let mut blockers = readback
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let readback_verified = json_string(&readback, "status") == "verified";
    if !readback_verified {
        blockers.push(serde_json::json!({
            "code": "release_publication_readback_not_verified",
            "severity": "blocker",
            "message": "C78 publication closure requires a verified C77 publication readback packet."
        }));
    }

    let trust_boundary = readback.get("trust_boundary").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "decision_owner": "ao2",
            "factory_v3_role": "evaluator-closer parity oracle and release acceptance owner",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        })
    });
    let factory_v3_release_acceptance_owner_preserved =
        json_string(&trust_boundary, "release_acceptance_owner") == "factory-v3 evaluator-closer";
    if !factory_v3_release_acceptance_owner_preserved {
        blockers.push(serde_json::json!({
            "code": "factory_v3_release_acceptance_owner_not_preserved",
            "severity": "blocker",
            "message": "C78 publication closure requires factory-v3 evaluator-closer to remain the release acceptance owner."
        }));
    }
    let control_plane_observer_only = json_string(&trust_boundary, "control_plane_role")
        == "read_only_observer_after_signed_evidence"
        && !trust_boundary
            .get("control_plane_approves_release")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
        && !trust_boundary
            .get("mutates_ao_artifacts")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
    if !control_plane_observer_only {
        blockers.push(serde_json::json!({
            "code": "control_plane_observer_boundary_not_preserved",
            "severity": "blocker",
            "message": "C78 publication closure requires ao2-control-plane to remain a read-only observer."
        }));
    }

    let status = if blockers.is_empty() {
        "closed"
    } else {
        "blocked"
    };
    let dispatch_plan_sha256 = json_string(&readback, "dispatch_plan_sha256");
    let observation_sha256 = json_string(&readback, "observation_sha256");
    let readiness_sha256 = json_string(&readback, "readiness_sha256");
    let export_sha256 = json_string(&readback, "export_sha256");
    let summary_sha256 = json_string(&readback, "summary_sha256");
    let status_sha256 = json_string(&readback, "status_sha256");
    let observer_fixture_sha256 = json_string(&readback, "observer_fixture_sha256");
    let operator_summary = if blockers.is_empty() {
        "recovery publication observed with no blockers"
    } else {
        "recovery publication closure blocked; inspect blockers"
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-recovery-resume-post-continuation-release-publication-closure.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "status": status,
        "target": args.target.display().to_string(),
        "readback_path": args.readback.display().to_string(),
        "readback_sha256": actual_readback_sha256,
        "dispatch_plan_sha256": dispatch_plan_sha256,
        "observation_sha256": observation_sha256,
        "readiness_sha256": readiness_sha256,
        "export_sha256": export_sha256,
        "summary_sha256": summary_sha256,
        "status_sha256": status_sha256,
        "observer_fixture_sha256": observer_fixture_sha256,
        "checks": {
            "exact_readback_digest_verified": true,
            "readback_verified": readback_verified,
            "no_blockers": blockers.is_empty(),
            "control_plane_observer_only": control_plane_observer_only,
            "factory_v3_release_acceptance_owner_preserved": factory_v3_release_acceptance_owner_preserved
        },
        "scheduler_closure": {
            "mode": "read_only_summary",
            "operator_summary": operator_summary,
            "closure_status": status,
            "hermes_surface": "front end, queue, cron, and memory bookkeeping",
            "ready_for_scheduler_archive": blockers.is_empty(),
            "ready_for_operator_follow_up": true,
            "next_recommended_lengthy_task": "ao2-control-plane K37: observe the AO2-owned C58-C78 recovery publication evidence chain as read-only fixtures without producing evidence, mutating AO artifacts, running queues/providers, writing memory, or approving releases.",
            "raw_chain_rediscovery_required": false,
            "factory_v3_release_acceptance_owner": "factory-v3 evaluator-closer"
        },
        "publication_contract": {
            "digest_bound": true,
            "requires_exact_c77_readback_digest": true,
            "producer": "ao2",
            "consumer": "Hermes governed scheduler and ao2-control-plane read-only observer",
            "hermes_may_archive_closure_summary": blockers.is_empty(),
            "control_plane_may_read_fixture": blockers.is_empty(),
            "control_plane_may_produce_evidence": false,
            "control_plane_may_approve_release": false,
            "control_plane_may_mutate_ao_artifacts": false,
            "factory_v3_accepts_release": true
        },
        "publication_readback": readback.get("publication_readback").cloned().unwrap_or_else(|| serde_json::json!({})),
        "status_packet": readback.get("status_packet").cloned().unwrap_or_else(|| serde_json::json!({})),
        "status_checks": readback.get("status_checks").cloned().unwrap_or_else(|| serde_json::json!({})),
        "evidence": [
            {
                "kind": "c72_release_handoff_status_packet",
                "sha256": status_sha256
            },
            {
                "kind": "c73_release_handoff_status_summary",
                "sha256": summary_sha256
            },
            {
                "kind": "c74_release_handoff_status_summary_export",
                "sha256": export_sha256
            },
            {
                "kind": "c75_release_publication_readiness",
                "sha256": readiness_sha256
            },
            {
                "kind": "c76_release_publication_dispatch_plan",
                "sha256": dispatch_plan_sha256
            },
            {
                "kind": "c77_release_publication_readback",
                "path": args.readback.display().to_string(),
                "sha256": actual_readback_sha256
            },
            {
                "kind": "c77_external_publication_observation",
                "sha256": observation_sha256
            }
        ],
        "concerns": readback.get("concerns").cloned().unwrap_or_else(|| serde_json::json!([])),
        "blockers": blockers,
        "changed_files": readback.get("changed_files").cloned().unwrap_or_else(|| serde_json::json!([])),
        "factory_v3_parity_owner": "factory-v3 evaluator-closer",
        "factory_v3_parity_oracle": readback.get("factory_v3_parity_oracle").cloned().unwrap_or_else(|| serde_json::json!({
            "ready_for_comparison": false,
            "factory_v3_drives_workflow": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_observer_only": true
        })),
        "side_effects": {
            "would_write_closure_artifact": false,
            "would_reinterpret_archive": false,
            "would_write_memory": false,
            "would_write_memory_run_link": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_mutate_control_plane": false,
            "would_write_queue_file": false,
            "would_mutate_ao_artifacts": false,
            "would_approve_release": false
        },
        "trust_boundary": trust_boundary,
        "ao2_decision_owner": "ao2-workbench-recovery-post-continuation-release-publication-closure"
    }))
}

fn read_json_file_or_null(
    path: &Path,
    blockers: &mut Vec<serde_json::Value>,
    label: &str,
) -> serde_json::Value {
    match fs::read_to_string(path) {
        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                blockers.push(serde_json::json!({
                    "code": format!("{label}_json_invalid"),
                    "severity": "blocker",
                    "message": format!("parse {}: {error}", path.display())
                }));
                serde_json::Value::Null
            }
        },
        Err(error) => {
            blockers.push(serde_json::json!({
                "code": format!("{label}_missing"),
                "severity": "blocker",
                "message": format!("read {}: {error}", path.display())
            }));
            serde_json::Value::Null
        }
    }
}

fn recovery_release_handoff_status_blocker(
    code: &str,
    reason: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "code": code,
        "severity": "blocker",
        "reason": reason,
        "message": "C71 release handoff SHA256SUMS is invalid."
    })
}

fn recovery_release_handoff_relative_path_allowed(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !Path::new(path).is_absolute()
        && !path.split('/').any(|part| part == ".." || part.is_empty())
        && !path.split('\\').any(|part| part == ".." || part.is_empty())
}

fn recovery_release_handoff_secret_scan<'a>(
    extract_dir: &Path,
    paths: impl Iterator<Item = &'a String>,
    concerns: &mut Vec<serde_json::Value>,
) -> bool {
    let markers = [
        "bearer ",
        "authorization",
        "openai_api_key",
        "anthropic_api_key",
    ];
    let mut passed = true;
    for relative_path in paths {
        if !recovery_release_handoff_relative_path_allowed(relative_path) {
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if !file_path.is_file() {
            continue;
        }
        let Ok(body) = fs::read_to_string(&file_path) else {
            continue;
        };
        let lower = body.to_ascii_lowercase();
        if markers.iter().any(|marker| lower.contains(marker)) {
            passed = false;
            concerns.push(serde_json::json!({
                "code": "release_handoff_secret_marker_present",
                "severity": "high",
                "path": relative_path,
                "message": "C71 release handoff status found a forbidden secret marker in bundled text."
            }));
        }
    }
    passed
}
