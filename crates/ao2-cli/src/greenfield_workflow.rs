use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_core::{extract_obligation_ledger, sha256_hex};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{canonical_json_sha256, json_string, sha256_bytes_hex};
use crate::workbench_queue::atomic_write_text;
use crate::{
    classify_factory_shape, classify_factory_size, factory_classification_signals,
    factory_ensure_target_repo, factory_governed_run_json, factory_plan_json,
    factory_queue_submit_project_start_json, reject_factory_provider_api_key_auth,
    sanitize_greenfield_id, FactoryGovernedRunOptions, FactoryPlanSigning,
    FactoryQueueSubmitProjectStartOptions,
};

pub(crate) struct GreenfieldIngestOptions<'a> {
    pub(crate) spec: &'a Path,
    pub(crate) target: &'a Path,
    pub(crate) run_id: Option<String>,
    pub(crate) verifier_command: String,
    pub(crate) signing_key: Option<&'a Path>,
    pub(crate) signer_id: &'a str,
    pub(crate) out_dir: Option<&'a Path>,
}

pub(crate) struct GreenfieldGovernedRunOptions<'a> {
    pub(crate) spec: &'a Path,
    pub(crate) target: &'a Path,
    pub(crate) run_id: String,
    pub(crate) verifier_command: String,
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

pub(crate) struct FactoryGreenfieldRunOptions<'a> {
    pub(crate) spec: &'a Path,
    pub(crate) target: &'a Path,
    pub(crate) run_id: String,
    pub(crate) verifier_command: String,
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

fn greenfield_title_and_acceptance(spec_text: &str) -> (String, Vec<String>) {
    let mut title = String::new();
    let mut acceptance = Vec::<String>::new();
    let mut in_acceptance = false;
    for line in spec_text.lines() {
        let trimmed = line.trim();
        if title.is_empty() && trimmed.starts_with('#') {
            title = trimmed.trim_start_matches('#').trim().to_string();
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.trim_end_matches(':') == "acceptance" || lower.starts_with("acceptance:") {
            in_acceptance = true;
            continue;
        }
        if in_acceptance {
            if trimmed.starts_with('#') {
                break;
            }
            if trimmed.is_empty() {
                continue;
            }
            if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .map(str::trim)
                .filter(|item| !item.is_empty())
            {
                acceptance.push(item.to_string());
            } else if !acceptance.is_empty() {
                break;
            }
        }
    }
    if title.is_empty() {
        title = "Greenfield AO2 work".to_string();
    }
    if acceptance.is_empty() {
        acceptance.push("AO2 materializes a governed plan from the plain spec.".to_string());
        acceptance.push(
            "AO2 executes the generated plan without factory-v3 driving workflow.".to_string(),
        );
        acceptance
            .push("Evaluator closure evidence is produced before release acceptance.".to_string());
    }
    (title, acceptance)
}

pub(crate) fn factory_greenfield_spec_ingest_json(
    spec: &Path,
    target: &Path,
    run_id: Option<String>,
    verifier_command: &str,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    if !spec.is_file() {
        return Err(anyhow!(
            "greenfield spec does not exist: {}",
            spec.display()
        ));
    }
    let spec_text = fs::read_to_string(spec)
        .with_context(|| format!("read greenfield spec {}", spec.display()))?;
    reject_factory_provider_api_key_auth(
        "greenfield_spec",
        &serde_json::json!({ "spec": spec_text }),
    )?;
    let spec_bytes = fs::read(spec).with_context(|| format!("read {}", spec.display()))?;
    let spec_sha256 = sha256_bytes_hex(&spec_bytes);
    let (title, acceptance) = greenfield_title_and_acceptance(&spec_text);
    let run_id = run_id
        .as_deref()
        .map(sanitize_greenfield_id)
        .unwrap_or_else(|| {
            spec.file_stem()
                .and_then(|stem| stem.to_str())
                .map(sanitize_greenfield_id)
                .unwrap_or_else(|| "greenfield-run".to_string())
        });
    let classification_text = serde_json::to_string(&serde_json::json!({
        "title": title,
        "acceptance": acceptance,
        "spec": spec_text,
        "shape": "greenfield"
    }))?
    .to_lowercase();
    let shape = classify_factory_shape(&classification_text);
    let size = classify_factory_size(&classification_text, false, false, 0);
    let planned_out_dir = target
        .join(".ao2")
        .join("factory-project-start")
        .join(&run_id);
    let project_plan = planned_out_dir
        .join("project-plan")
        .join("project-plan.json");
    let project_start = planned_out_dir.join(format!("{run_id}-factory-project-start.json"));
    let project_start_handoff = planned_out_dir.join("project-start-handoff.tgz");

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-greenfield-spec-ingest.v1",
        "status": "ready",
        "run_id": run_id,
        "source_spec": {
            "path": spec.display().to_string(),
            "sha256": spec_sha256,
            "title": title,
            "acceptance": acceptance
        },
        "target": target.display().to_string(),
        "verifier": {
            "command": verifier_command
        },
        "classification": {
            "size": size,
            "shape": shape,
            "owner": "ao2-native-classifier",
            "source": "ao2-greenfield-spec-ingest",
            "factory_v3_required_before_classification": false,
            "signals": factory_classification_signals(&classification_text)
        },
        "preflight": {
            "read_only": true,
            "queue_submission_ready": true,
            "missing_required_inputs": [],
            "planned_out_dir": planned_out_dir.display().to_string(),
            "planned_project_root": target.display().to_string()
        },
        "planned_ao2_producer_commands": [
            {
                "command": "ao2 factory project-plan",
                "writes_when_executed": [
                    "ao2.factory-project-plan.v1",
                    "ao2.factory-acceptance-rubric.v1"
                ],
                "args": {
                    "--project-spec": spec.display().to_string(),
                    "--project-root": target.display().to_string(),
                    "--run-id": run_id,
                    "--verifier-command": verifier_command,
                    "--out": project_plan.display().to_string()
                }
            },
            {
                "command": "ao2 factory project-start",
                "writes_when_executed": [
                    "ao2.factory-project-start.v1",
                    "ao2.factory-project-start-bundle.v1"
                ],
                "args": {
                    "--project-spec": spec.display().to_string(),
                    "--project-root": target.display().to_string(),
                    "--run-id": run_id,
                    "--verifier-command": verifier_command,
                    "--out-dir": planned_out_dir.display().to_string(),
                    "--handoff-bundle-out": project_start_handoff.display().to_string()
                }
            },
            {
                "command": "ao2 factory queue-submit-project-start",
                "writes_when_executed": [
                    "ao2.factory-project-start-workbench-queue-submit.v1"
                ],
                "args": {
                    "--target": target.display().to_string(),
                    "--project-start": project_start.display().to_string()
                }
            }
        ],
        "expected_artifact_schemas": [
            "ao2.factory-project-plan.v1",
            "ao2.factory-acceptance-rubric.v1",
            "ao2.factory-project-start.v1",
            "ao2.factory-project-start-bundle.v1",
            "ao2.factory-project-start-workbench-queue-submit.v1"
        ],
        "side_effects": {
            "would_write_files": false,
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_submit_queue_entry": false,
            "would_rebuild_wrappers": false,
            "would_mutate_control_plane": false
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
        }
    }))
}

pub(crate) struct FactoryGreenfieldSpecIngestSubmitOptions<'a> {
    pub(crate) spec: &'a Path,
    pub(crate) target: &'a Path,
    pub(crate) run_id: Option<String>,
    pub(crate) verifier_command: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt_dir: Option<PathBuf>,
    pub(crate) max_repair_attempts: usize,
    pub(crate) approval_action_digest: Option<String>,
    pub(crate) signer_id: String,
    pub(crate) digest_action: &'a str,
}

pub(crate) fn factory_greenfield_spec_ingest_submit_json(
    options: FactoryGreenfieldSpecIngestSubmitOptions<'_>,
) -> Result<serde_json::Value> {
    let preflight = factory_greenfield_spec_ingest_json(
        options.spec,
        options.target,
        options.run_id.clone(),
        &options.verifier_command,
    )?;
    let digest_input = serde_json::json!({
        "action": options.digest_action,
        "preflight": preflight
    });
    let action_digest = canonical_json_sha256(&digest_input);
    let submitted_digest = options.approval_action_digest.unwrap_or_default();
    if submitted_digest != action_digest {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-greenfield-spec-ingest-submit-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_action_digest",
            "required_flag": "--approve-action-digest",
            "required_form_field": "approval_action_digest",
            "action_digest": action_digest,
            "preflight": digest_input["preflight"].clone(),
            "next_action": "submit approval_action_digest or --approve-action-digest with the exact action_digest to submit the AO2 project-start queue entry",
            "side_effects": {
                "would_write_queue_file_after_approval": true,
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_mutate_control_plane": false
            },
            "trust_boundary": digest_input["preflight"]["trust_boundary"].clone()
        }));
    }

    let run_id = json_string(&digest_input["preflight"], "run_id");
    let out_dir = PathBuf::from(json_string(
        &digest_input["preflight"]["preflight"],
        "planned_out_dir",
    ));
    let queue_submit =
        factory_queue_submit_project_start_json(FactoryQueueSubmitProjectStartOptions {
            target: options.target,
            project_spec: options.spec,
            project_root: options.target,
            run_id: Some(run_id.clone()),
            verifier_command: options.verifier_command,
            provider: options.provider,
            provider_prompt_dir: options.provider_prompt_dir,
            signing_key: None,
            signer_id: options.signer_id,
            max_repair_attempts: options.max_repair_attempts,
            out_dir: Some(out_dir),
            handoff_bundle_out: None,
            handoff_bundle_report: None,
            receipt_out: None,
        })?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-greenfield-spec-ingest-submit.v1",
        "status": json_string(&queue_submit, "status"),
        "run_id": run_id,
        "approval": {
            "schema_version": "ao2.factory-greenfield-spec-ingest-submit-approval.v1",
            "status": "approved_exact_action_digest",
            "approval_mode": "exact_action_digest",
            "action_digest": action_digest
        },
        "preflight": digest_input["preflight"].clone(),
        "queue_submit": queue_submit,
        "side_effects": {
            "submitted_queue_entry": true,
            "wrote_queue_file": true,
            "executed_provider": false,
            "executed_queue": false,
            "mutated_control_plane": false
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

pub(crate) fn greenfield_ingest_json(
    options: GreenfieldIngestOptions<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(options.target)?;
    if !options.spec.is_file() {
        return Err(anyhow!(
            "greenfield spec does not exist: {}",
            options.spec.display()
        ));
    }
    let spec_text = fs::read_to_string(options.spec)
        .with_context(|| format!("read greenfield spec {}", options.spec.display()))?;
    reject_factory_provider_api_key_auth(
        "greenfield_spec",
        &serde_json::json!({ "spec": spec_text }),
    )?;
    let (title, acceptance) = greenfield_title_and_acceptance(&spec_text);
    let run_id = options
        .run_id
        .as_deref()
        .map(sanitize_greenfield_id)
        .unwrap_or_else(|| {
            options
                .spec
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(sanitize_greenfield_id)
                .unwrap_or_else(|| "greenfield-run".to_string())
        });
    let out_dir = options
        .out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| options.target.join(".ao2").join("greenfield").join(&run_id));
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create greenfield out dir {}", out_dir.display()))?;
    let out_dir = fs::canonicalize(&out_dir)
        .with_context(|| format!("canonicalize greenfield out dir {}", out_dir.display()))?;

    let spec_bytes =
        fs::read(options.spec).with_context(|| format!("read {}", options.spec.display()))?;
    let spec_sha256 = sha256_hex(&spec_bytes);
    let spec_intake_path = out_dir.join(format!("{run_id}-spec-intake.json"));
    let work_request_path = out_dir.join(format!("{run_id}-work-request.json"));
    let runspec_path = out_dir.join(format!("{run_id}-runspec.json"));
    let obligation_ledger_path = out_dir.join(format!("{run_id}-obligation-ledger.json"));
    let ingest_path = out_dir.join(format!("{run_id}-greenfield-ingest.json"));
    let plan_path = out_dir.join(format!("{run_id}-plan.json"));

    let spec_intake = serde_json::json!({
        "schema_version": "ao2.greenfield-spec-intake.v1",
        "run_id": run_id,
        "source_spec": options.spec.display().to_string(),
        "source_spec_sha256": spec_sha256,
        "title": title,
        "acceptance": acceptance,
        "verifier": {
            "command": options.verifier_command
        },
        "shape": "greenfield",
        "producer": "ao2 greenfield ingest",
        "target": options.target.display().to_string(),
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    let work_request = serde_json::json!({
        "schema_version": "ao2.greenfield-work-request.v1",
        "title": spec_intake["title"],
        "objective": format!("Build from greenfield spec: {}", json_string(&spec_intake, "title")),
        "acceptance": spec_intake["acceptance"],
        "shape": "greenfield",
        "size": "medium",
        "source_spec": options.spec.display().to_string(),
        "source_spec_sha256": spec_sha256,
        "factory_v3_required_before_classification": false,
        "factory_v3_role": "parity_oracle_only",
        "ao2_execution_owner": true,
        "control_plane_role": "read_only_observer_after_signed_evidence"
    });
    let runspec = serde_json::json!({
        "apiVersion": "ao.dev/v1",
        "kind": "Run",
        "metadata": {
            "name": run_id
        },
        "verifier": {
            "command": spec_intake["verifier"]["command"]
        },
        "spec": {
            "tasks": [
                {
                    "id": "planner-intake",
                    "kind": "agent",
                    "deps": [],
                    "spec": {
                        "provider": "scripted",
                        "agent": "planner-intake",
                        "policyProfile": "ao2-greenfield-planner"
                    }
                },
                {
                    "id": "implementer",
                    "kind": "agent",
                    "deps": ["planner-intake"],
                    "spec": {
                        "provider": "scripted",
                        "agent": "implementer",
                        "policyProfile": "ao2-greenfield-implementer"
                    }
                },
                {
                    "id": "evaluator-closer",
                    "kind": "agent",
                    "deps": ["implementer"],
                    "spec": {
                        "provider": "scripted",
                        "agent": "evaluator-closer",
                        "policyProfile": "ao2-greenfield-evaluator-closer"
                    }
                }
            ]
        }
    });
    let obligation_ledger = extract_obligation_ledger(&options.spec.to_string_lossy(), &spec_text);

    atomic_write_text(
        &spec_intake_path,
        &serde_json::to_string_pretty(&spec_intake)?,
    )?;
    atomic_write_text(
        &work_request_path,
        &serde_json::to_string_pretty(&work_request)?,
    )?;
    atomic_write_text(&runspec_path, &serde_json::to_string_pretty(&runspec)?)?;
    atomic_write_text(
        &obligation_ledger_path,
        &serde_json::to_string_pretty(&obligation_ledger)?,
    )?;

    let plan = factory_plan_json(
        &work_request_path,
        None,
        Some(&runspec_path),
        &[],
        FactoryPlanSigning {
            key: options.signing_key,
            signer_id: options.signer_id,
        },
        options.target,
        Some(&plan_path),
    )?;
    let result = serde_json::json!({
        "schema_version": "ao2.greenfield-ingest.v1",
        "status": "planned",
        "run_id": run_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "ingest_path": ingest_path.display().to_string(),
        "artifacts": {
            "spec_intake": spec_intake_path.display().to_string(),
            "work_request": work_request_path.display().to_string(),
            "runspec": runspec_path.display().to_string(),
            "obligation_ledger": obligation_ledger_path.display().to_string(),
            "plan": json_string(&plan, "plan_path"),
            "workflow": json_string(&plan, "workflow_path"),
            "planning_evidence": json_string(&plan, "planning_evidence_path"),
            "greenfield_ingest": ingest_path.display().to_string()
        },
        "spec_intake": spec_intake,
        "work_request": work_request,
        "runspec": runspec,
        "obligation_ledger": obligation_ledger,
        "plan": plan,
        "classification": plan["classification"],
        "greenfield_checklist": {
            "ao2_ingested_plain_spec": true,
            "ao2_generated_work_request": true,
            "ao2_generated_runspec": true,
            "ao2_extracted_obligation_ledger": true,
            "ao2_materialized_governed_plan": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    atomic_write_text(&ingest_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn greenfield_governed_run_json(
    options: GreenfieldGovernedRunOptions<'_>,
) -> Result<serde_json::Value> {
    fs::create_dir_all(options.out_dir).with_context(|| {
        format!(
            "create greenfield governed run out dir {}",
            options.out_dir.display()
        )
    })?;
    let out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize greenfield governed run out dir {}",
            options.out_dir.display()
        )
    })?;
    let ingest_dir = out_dir.join("ingest");
    let governed_dir = out_dir.join("governed-run");
    let ingest = greenfield_ingest_json(GreenfieldIngestOptions {
        spec: options.spec,
        target: options.target,
        run_id: Some(options.run_id.clone()),
        verifier_command: options.verifier_command.clone(),
        signing_key: options.signing_key.as_deref(),
        signer_id: &options.signer_id,
        out_dir: Some(&ingest_dir),
    })?;
    let work_request = PathBuf::from(json_string(&ingest["artifacts"], "work_request"));
    let runspec = PathBuf::from(json_string(&ingest["artifacts"], "runspec"));
    let governed_run = factory_governed_run_json(FactoryGovernedRunOptions {
        request: &work_request,
        profile: None,
        runspec: &runspec,
        role_contracts: &[],
        target: options.target,
        run_id: options.run_id.clone(),
        provider: options.provider,
        provider_prompt: options.provider_prompt,
        provider_prompt_file: options.provider_prompt_file,
        provider_max_budget_usd: options.provider_max_budget_usd,
        factory_decision: options.factory_decision,
        signing_key: options.signing_key,
        signer_id: options.signer_id,
        max_repair_attempts: options.max_repair_attempts,
        out_dir: &governed_dir,
    })?;
    let result_path = out_dir.join(format!("{}-greenfield-governed-run.json", options.run_id));
    let status = if ingest["status"] == "planned" && governed_run["status"] == "accepted" {
        "accepted"
    } else {
        "rejected"
    };
    let result = serde_json::json!({
        "schema_version": "ao2.greenfield-governed-run.v1",
        "status": status,
        "run_id": options.run_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "artifacts": {
            "greenfield_ingest": json_string(&ingest["artifacts"], "greenfield_ingest"),
            "plan": json_string(&ingest["artifacts"], "plan"),
            "governed_run": json_string(&governed_run["artifacts"], "governed_run"),
            "packed_evidence": json_string(&governed_run["artifacts"], "packed_evidence"),
            "evaluator_decision": json_string(&governed_run["artifacts"], "evaluator_decision"),
            "greenfield_governed_run": result_path.display().to_string()
        },
        "ingest": ingest,
        "governed_run": governed_run,
        "greenfield_governed_run_checklist": {
            "ao2_ingested_plain_spec": true,
            "ao2_generated_work_request": true,
            "ao2_generated_runspec": true,
            "ao2_executed_generated_governed_plan": status == "accepted",
            "ao2_verified_primary_run_result": status == "accepted",
            "ao2_packed_primary_evidence": status == "accepted",
            "ao2_signed_evaluator_closure": status == "accepted",
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    atomic_write_text(&result_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn factory_greenfield_run_json(
    options: FactoryGreenfieldRunOptions<'_>,
) -> Result<serde_json::Value> {
    let greenfield = greenfield_governed_run_json(GreenfieldGovernedRunOptions {
        spec: options.spec,
        target: options.target,
        run_id: options.run_id.clone(),
        verifier_command: options.verifier_command,
        provider: options.provider,
        provider_prompt: options.provider_prompt,
        provider_prompt_file: options.provider_prompt_file,
        provider_max_budget_usd: options.provider_max_budget_usd,
        factory_decision: options.factory_decision,
        signing_key: options.signing_key,
        signer_id: options.signer_id,
        max_repair_attempts: options.max_repair_attempts,
        out_dir: options.out_dir,
    })?;
    let out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize factory greenfield run out dir {}",
            options.out_dir.display()
        )
    })?;
    let result_path = out_dir.join(format!(
        "{}-factory-greenfield-run.json",
        sanitize_greenfield_id(&options.run_id)
    ));
    let result = serde_json::json!({
        "schema_version": "ao2.factory-greenfield-run.v1",
        "status": greenfield["status"],
        "run_id": options.run_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "factory_replacement_boundary": {
            "ao2_execution_owner": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "artifacts": {
            "factory_greenfield_run": result_path.display().to_string(),
            "greenfield_governed_run": json_string(&greenfield["artifacts"], "greenfield_governed_run"),
            "greenfield_ingest": json_string(&greenfield["artifacts"], "greenfield_ingest"),
            "plan": json_string(&greenfield["artifacts"], "plan"),
            "governed_run": json_string(&greenfield["artifacts"], "governed_run"),
            "evidence_pack": json_string(&greenfield["artifacts"], "packed_evidence"),
            "evaluator_decision": json_string(&greenfield["artifacts"], "evaluator_decision")
        },
        "greenfield": greenfield,
        "factory_greenfield_run_checklist": {
            "ao2_ingested_plain_spec": greenfield["greenfield_governed_run_checklist"]["ao2_ingested_plain_spec"],
            "ao2_generated_work_request": greenfield["greenfield_governed_run_checklist"]["ao2_generated_work_request"],
            "ao2_generated_runspec": greenfield["greenfield_governed_run_checklist"]["ao2_generated_runspec"],
            "ao2_executed_generated_governed_plan": greenfield["greenfield_governed_run_checklist"]["ao2_executed_generated_governed_plan"],
            "ao2_verified_primary_run_result": greenfield["greenfield_governed_run_checklist"]["ao2_verified_primary_run_result"],
            "ao2_packed_primary_evidence": greenfield["greenfield_governed_run_checklist"]["ao2_packed_primary_evidence"],
            "ao2_signed_evaluator_closure": greenfield["greenfield_governed_run_checklist"]["ao2_signed_evaluator_closure"],
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    atomic_write_text(&result_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}
