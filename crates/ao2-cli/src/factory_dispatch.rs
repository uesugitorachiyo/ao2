use crate::cli::FactoryCommand;
use crate::cli_util::{atomic_write_text, json_string, json_u64, sha256_file};
use crate::factory_app_run::{
    factory_app_run_bundle_json, factory_app_run_json, FactoryAppRunOptions,
};
use crate::factory_bridge;
use crate::factory_evaluator::{
    factory_evaluate_json, factory_evaluator_rubric_json, FactoryEvaluatorRubricOptions,
};
use crate::factory_evidence::{
    factory_pack_evidence_json, factory_plan_json, factory_verify_bridge_evidence_json,
    factory_verify_evaluator_decision_json, factory_verify_planning_evidence_json,
    FactoryPlanSigning,
};
use crate::factory_governance::{
    factory_closer_decision_json, factory_closer_decision_verify_json, factory_governed_run_json,
    factory_replacement_parity_status_json, factory_replacement_smoke_gate_json,
    factory_replacement_smoke_json, factory_verify_handoff_json, factory_verify_run_result_json,
    FactoryCloserDecisionOptions, FactoryGovernedRunOptions, FactoryReplacementSmokeOptions,
};
use crate::factory_project_execution::{
    factory_project_acceptance_review_json, factory_project_run_json,
    FactoryProjectAcceptanceReviewOptions, FactoryProjectRunOptions,
};
use crate::factory_project_planning::{
    factory_project_plan_json, factory_project_plan_validate_json, FactoryProjectPlanOptions,
    FactoryProjectPlanValidateOptions,
};
use crate::factory_project_start::{
    factory_project_start_bundle_json, factory_project_start_bundle_verify_json,
    factory_project_start_closure_json, factory_project_start_closure_verify_json,
    factory_project_start_json, factory_replacement_packet_json,
    factory_replacement_packet_verify_json, FactoryProjectStartOptions,
    FactoryReplacementPacketOptions,
};
use crate::factory_project_start_summary::{
    factory_project_start_summary_json, factory_project_start_summary_markdown,
};
use crate::factory_queue::{
    factory_cancel_authority_json, factory_cancel_transition_json,
    factory_queue_completion_contract_consumption_json, factory_queue_completion_contract_json,
    factory_queue_list_json, factory_queue_project_start_completion_summary_json,
    factory_queue_status_json, factory_queue_status_latest_completed_project_start_json,
    factory_queue_submit_json,
};
use crate::factory_queue_execution::{
    factory_queue_run_next_json, factory_queue_transition_json, FactoryQueueRunNextOptions,
};
use crate::factory_queue_operator::{
    factory_project_start_hermes_context_json, factory_project_start_hermes_flow_contract_json,
    factory_queue_project_start_next_action_json,
    factory_queue_project_start_publish_operator_record_json,
};
use crate::factory_queue_project_start::{
    factory_queue_project_start_complete_json, factory_queue_submit_project_start_json,
    FactoryQueueProjectStartCompleteOptions, FactoryQueueSubmitProjectStartOptions,
};
use crate::factory_queue_recovery::{
    factory_queue_project_start_complete_status_json,
    factory_queue_project_start_completion_summary_memory_json,
    factory_queue_project_start_completion_summary_memory_status_json,
    factory_queue_project_start_latest_recovery_json,
    factory_queue_project_start_recovery_action_json, factory_queue_project_start_recovery_json,
    factory_queue_project_start_recovery_resume_checkpoint_json,
    factory_queue_project_start_recovery_resume_checkpoint_status_json,
    factory_queue_project_start_recovery_resume_claim_json,
    factory_queue_project_start_recovery_resume_claim_status_json,
    factory_queue_project_start_recovery_resume_continuation_contract_json,
    factory_queue_project_start_recovery_resume_continuation_status_json,
    factory_queue_project_start_recovery_resume_continue_json,
    factory_queue_project_start_recovery_resume_continuity_json,
    factory_queue_project_start_recovery_resume_plan_json,
    factory_queue_project_start_recovery_resume_receipt_json,
};
use crate::factory_queue_recovery_release::{
    factory_queue_project_start_recovery_resume_post_continuation_action_json,
    factory_queue_project_start_recovery_resume_post_continuation_closure_json,
    factory_queue_project_start_recovery_resume_post_continuation_evaluator_decision_json,
    factory_queue_project_start_recovery_resume_post_continuation_execute_json,
    factory_queue_project_start_recovery_resume_post_continuation_execution_status_json,
    factory_queue_project_start_recovery_resume_post_continuation_next_action_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_export_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_closure_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_dispatch_plan_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_readback_json,
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_readiness_json,
    RecoveryResumePostContinuationClosureArgs, RecoveryResumePostContinuationEvaluatorDecisionArgs,
    RecoveryResumePostContinuationReleaseHandoffArgs,
    RecoveryResumePostContinuationReleaseHandoffStatusArgs,
    RecoveryResumePostContinuationReleaseHandoffStatusSummaryArgs,
    RecoveryResumePostContinuationReleaseHandoffStatusSummaryExportArgs,
    RecoveryResumePostContinuationReleasePublicationClosureArgs,
    RecoveryResumePostContinuationReleasePublicationDispatchPlanArgs,
    RecoveryResumePostContinuationReleasePublicationReadbackArgs,
    RecoveryResumePostContinuationReleasePublicationReadinessArgs,
};
use crate::factory_run_execution::{factory_run_plan_json, FactoryRunPlanOptions};
use crate::greenfield_workflow::{
    factory_greenfield_run_json, factory_greenfield_spec_ingest_json,
    factory_greenfield_spec_ingest_submit_json, FactoryGreenfieldRunOptions,
    FactoryGreenfieldSpecIngestSubmitOptions,
};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};
use anyhow::{anyhow, Context, Result};
use std::fs;
pub(crate) fn factory(command: FactoryCommand) -> Result<()> {
    match command {
        FactoryCommand::Plan {
            request,
            profile,
            runspec,
            role_contracts,
            signing_key,
            signer_id,
            target,
            out,
            json,
        } => {
            let result = factory_plan_json(
                &request,
                profile.as_deref(),
                runspec.as_deref(),
                &role_contracts,
                FactoryPlanSigning {
                    key: signing_key.as_deref(),
                    signer_id: &signer_id,
                },
                &target,
                out.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("plan={}", json_string(&result, "plan_path"));
                println!(
                    "classification_size={}",
                    json_string(&result["classification"], "size")
                );
                println!(
                    "classification_shape={}",
                    json_string(&result["classification"], "shape")
                );
                println!(
                    "evidence={}",
                    json_string(&result, "planning_evidence_path")
                );
            }
            Ok(())
        }
        FactoryCommand::Run {
            plan,
            target,
            run_id,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            factory_decision,
            signing_key,
            signer_id,
            max_repair_attempts,
            out,
            json,
        } => {
            let result = factory_run_plan_json(FactoryRunPlanOptions {
                plan: &plan,
                target: &target,
                run_id,
                provider,
                provider_prompt,
                provider_prompt_file,
                provider_max_budget_usd,
                factory_decision,
                signing_key,
                signer_id,
                max_repair_attempts,
                out,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("evidence_pack={}", json_string(&result, "evidence_pack"));
                println!("report={}", json_string(&result, "report"));
                println!(
                    "replay_digest_failures={}",
                    result["replay"]["digest_failures"]
                        .as_array()
                        .map(|items| items.len())
                        .unwrap_or(0)
                );
            }
            Ok(())
        }
        FactoryCommand::ReplacementSmoke {
            request,
            profile,
            runspec,
            role_contracts,
            target,
            run_id,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            factory_decision,
            signing_key,
            signer_id,
            max_repair_attempts,
            out_dir,
            json,
        } => {
            let result = factory_replacement_smoke_json(FactoryReplacementSmokeOptions {
                request: &request,
                profile: profile.as_deref(),
                runspec: &runspec,
                role_contracts: &role_contracts,
                target: &target,
                run_id,
                provider,
                provider_prompt,
                provider_prompt_file,
                provider_max_budget_usd,
                factory_decision,
                signing_key,
                signer_id,
                max_repair_attempts,
                out_dir: &out_dir,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "run_result_verification={}",
                    result["run_result_verification"]["status"]
                );
                println!("packed_evidence={}", result["pack_evidence"]["status"]);
            }
            Ok(())
        }
        FactoryCommand::GovernedRun {
            request,
            profile,
            runspec,
            role_contracts,
            target,
            run_id,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            factory_decision,
            signing_key,
            signer_id,
            max_repair_attempts,
            out_dir,
            json,
        } => {
            let result = factory_governed_run_json(FactoryGovernedRunOptions {
                request: &request,
                profile: profile.as_deref(),
                runspec: &runspec,
                role_contracts: &role_contracts,
                target: &target,
                run_id,
                provider,
                provider_prompt,
                provider_prompt_file,
                provider_max_budget_usd,
                factory_decision,
                signing_key,
                signer_id,
                max_repair_attempts,
                out_dir: &out_dir,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "evaluator_decision_verification={}",
                    result["evaluator_decision_verification"]["status"]
                );
                println!("packed_evidence={}", result["pack_evidence"]["status"]);
            }
            Ok(())
        }
        FactoryCommand::GreenfieldRun {
            spec,
            target,
            run_id,
            verifier_command,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            factory_decision,
            signing_key,
            signer_id,
            max_repair_attempts,
            out_dir,
            json,
        } => {
            let result = factory_greenfield_run_json(FactoryGreenfieldRunOptions {
                spec: &spec,
                target: &target,
                run_id,
                verifier_command,
                provider,
                provider_prompt,
                provider_prompt_file,
                provider_max_budget_usd,
                factory_decision,
                signing_key,
                signer_id,
                max_repair_attempts,
                out_dir: &out_dir,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "greenfield_governed_run={}",
                    json_string(&result["artifacts"], "greenfield_governed_run")
                );
                println!(
                    "evidence_pack={}",
                    json_string(&result["artifacts"], "evidence_pack")
                );
            }
            Ok(())
        }
        FactoryCommand::GreenfieldSpecIngest {
            spec,
            target,
            run_id,
            verifier_command,
            json,
        } => {
            let result =
                factory_greenfield_spec_ingest_json(&spec, &target, run_id, &verifier_command)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("greenfield_spec_ingest={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "classification_shape={}",
                    json_string(&result["classification"], "shape")
                );
            }
            Ok(())
        }
        FactoryCommand::GreenfieldSpecIngestSubmit {
            spec,
            target,
            run_id,
            verifier_command,
            provider,
            provider_prompt_dir,
            max_repair_attempts,
            approve_action_digest,
            json,
        } => {
            let result = factory_greenfield_spec_ingest_submit_json(
                FactoryGreenfieldSpecIngestSubmitOptions {
                    spec: &spec,
                    target: &target,
                    run_id,
                    verifier_command,
                    provider,
                    provider_prompt_dir,
                    max_repair_attempts,
                    approval_action_digest: approve_action_digest,
                    signer_id: "ao2-greenfield-spec-ingest".to_string(),
                    digest_action: "ao2.factory-greenfield-spec-ingest-submit.v1",
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("greenfield_spec_ingest_submit={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                if let Some(action_digest) =
                    result.get("action_digest").and_then(|value| value.as_str())
                {
                    println!("action_digest={action_digest}");
                }
            }
            Ok(())
        }
        FactoryCommand::AppRun {
            spec,
            target,
            run_id,
            verifier_command,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            factory_decision,
            signing_key,
            signer_id,
            max_repair_attempts,
            out_dir,
            json,
        } => {
            let result = factory_app_run_json(FactoryAppRunOptions {
                spec: &spec,
                target: &target,
                run_id,
                verifier_command,
                provider,
                provider_prompt,
                provider_prompt_file,
                provider_max_budget_usd,
                factory_decision,
                signing_key,
                signer_id,
                max_repair_attempts,
                out_dir: &out_dir,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "app_run={}",
                    json_string(&result["artifacts"], "factory_app_run")
                );
                println!(
                    "evidence_pack={}",
                    json_string(&result["artifacts"], "evidence_pack")
                );
            }
            Ok(())
        }
        FactoryCommand::AppRunBundle { app_run, out, json } => {
            let result = factory_app_run_bundle_json(&app_run, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("archive={}", json_string(&result, "archive"));
                println!("sha256={}", json_string(&result, "sha256"));
                println!(
                    "artifact_count={}",
                    result["artifact_count"].as_u64().unwrap_or_default()
                );
            }
            Ok(())
        }
        FactoryCommand::ProjectPlan {
            project_spec,
            project_root,
            run_id,
            verifier_command,
            provider,
            provider_prompt_dir,
            signing_key,
            signer_id,
            out,
            json,
        } => {
            let result = factory_project_plan_json(FactoryProjectPlanOptions {
                project_spec: &project_spec,
                project_root: &project_root,
                run_id,
                verifier_command,
                provider,
                provider_prompt_dir,
                signing_key,
                signer_id,
                out: &out,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "project_plan={}",
                    json_string(&result["artifacts"], "project_plan")
                );
                println!(
                    "app_step_count={}",
                    result["app_steps"]
                        .as_array()
                        .map(|steps| steps.len())
                        .unwrap_or(0)
                );
            }
            Ok(())
        }
        FactoryCommand::ProjectPlanValidate {
            project_plan,
            project_root,
            out,
            json,
        } => {
            let result = factory_project_plan_validate_json(FactoryProjectPlanValidateOptions {
                project_plan: &project_plan,
                project_root: &project_root,
                out: &out,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "app_step_count={}",
                    result["app_step_count"].as_u64().unwrap_or(0)
                );
                println!(
                    "validation={}",
                    json_string(&result["artifacts"], "validation")
                );
            }
            Ok(())
        }
        FactoryCommand::EvaluatorRubric {
            spec,
            run_id,
            verifier_command,
            signing_key,
            signer_id,
            out,
            json,
        } => {
            let result = factory_evaluator_rubric_json(FactoryEvaluatorRubricOptions {
                spec: &spec,
                run_id,
                verifier_command,
                signing_key,
                signer_id,
                out: &out,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("rubric={}", json_string(&result["artifacts"], "rubric"));
                println!("rubric_sha256={}", json_string(&result, "rubric_sha256"));
            }
            Ok(())
        }
        FactoryCommand::CloserDecision {
            rubric,
            rubric_sha256,
            evidence,
            evidence_sha256,
            skill_contract_manifest,
            skill_contract_manifest_sha256,
            signing_key,
            signer_id,
            out,
            json,
        } => {
            let result = factory_closer_decision_json(FactoryCloserDecisionOptions {
                rubric: &rubric,
                rubric_sha256,
                evidence: &evidence,
                evidence_sha256,
                skill_contract_manifest: &skill_contract_manifest,
                skill_contract_manifest_sha256,
                signing_key: &signing_key,
                signer_id,
                out: &out,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("decision={}", json_string(&result, "decision"));
                println!("rubric_sha256={}", json_string(&result, "rubric_sha256"));
                println!("decision_sha256={}", json_string(&result, "decision_sha256"));
            }
            Ok(())
        }
        FactoryCommand::CloserDecisionVerify {
            decision,
            decision_sha256,
            json,
        } => {
            let result = factory_closer_decision_verify_json(&decision, &decision_sha256)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("decision={}", decision.display());
                println!(
                    "signature_verified={}",
                    result["signature_verified"].as_bool().unwrap_or(false)
                );
            }
            if json_string(&result, "status") != "accepted" {
                anyhow::bail!("factory closer decision verification failed");
            }
            Ok(())
        }
        FactoryCommand::ProjectStart {
            project_spec,
            project_root,
            run_id,
            verifier_command,
            provider,
            provider_prompt_dir,
            signing_key,
            signer_id,
            max_repair_attempts,
            handoff_bundle_out,
            handoff_bundle_report,
            out_dir,
            json,
        } => {
            let result = factory_project_start_json(FactoryProjectStartOptions {
                project_spec: &project_spec,
                project_root: &project_root,
                run_id,
                verifier_command,
                provider,
                provider_prompt_dir,
                signing_key,
                signer_id,
                max_repair_attempts,
                handoff_bundle_out,
                handoff_bundle_report,
                out_dir: &out_dir,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "project_start={}",
                    json_string(&result["artifacts"], "factory_project_start")
                );
                println!(
                    "project_run={}",
                    json_string(&result["artifacts"], "factory_project_run")
                );
                println!(
                    "release_review_package={}",
                    json_string(&result["artifacts"], "release_review_package")
                );
                if result.get("hermes_queue_handoff").is_some() {
                    println!(
                        "project_start_bundle={}",
                        json_string(&result["hermes_queue_handoff"], "project_start_bundle")
                    );
                    println!(
                        "project_start_bundle_sha256={}",
                        json_string(
                            &result["hermes_queue_handoff"],
                            "project_start_bundle_sha256"
                        )
                    );
                    println!(
                        "handoff_entry={}",
                        json_string(&result["hermes_queue_handoff"], "handoff_entry")
                    );
                    println!(
                        "manifest_entry={}",
                        json_string(&result["hermes_queue_handoff"], "manifest_entry")
                    );
                    println!(
                        "checksum_entry={}",
                        json_string(&result["hermes_queue_handoff"], "checksum_entry")
                    );
                    println!(
                        "factory_v3_role={}",
                        json_string(&result["hermes_queue_handoff"], "factory_v3_role")
                    );
                    println!(
                        "control_plane_role={}",
                        json_string(&result["hermes_queue_handoff"], "control_plane_role")
                    );
                    println!(
                        "release_acceptance_owner={}",
                        json_string(&result["hermes_queue_handoff"], "release_acceptance_owner")
                    );
                }
            }
            Ok(())
        }
        FactoryCommand::ProjectStartHermesFlowContract { target, out, json } => {
            let result = factory_project_start_hermes_flow_contract_json(&target, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("contract_path={}", json_string(&result, "contract_path"));
                println!(
                    "contract_sha256={}",
                    json_string(&result, "contract_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::ProjectStartHermesContext { target, json } => {
            let result = factory_project_start_hermes_context_json(&target)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "flow_contract_sha256={}",
                    json_string(&result["flow_contract"], "contract_sha256")
                );
                println!(
                    "support_packet_present={}",
                    result["latest_support_packet"]["present"]
                        .as_bool()
                        .unwrap_or(false)
                );
            }
            Ok(())
        }
        FactoryCommand::ProjectStartBundle {
            project_start,
            out,
            json,
        } => {
            let result = factory_project_start_bundle_json(&project_start, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("archive={}", json_string(&result, "archive"));
                println!("sha256={}", json_string(&result, "sha256"));
                println!(
                    "artifact_count={}",
                    result["artifact_count"].as_u64().unwrap_or_default()
                );
            }
            Ok(())
        }
        FactoryCommand::ProjectStartBundleVerify { bundle, json } => {
            let result = factory_project_start_bundle_verify_json(&bundle)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "project_start_bundle_verification={}",
                    json_string(&result, "status")
                );
                println!("bundle={}", bundle.display());
                println!("failure_count={}", json_u64(&result, "failure_count"));
            }
            if json_string(&result, "status") != "accepted" {
                anyhow::bail!("factory project-start bundle verification failed");
            }
            Ok(())
        }
        FactoryCommand::ProjectStartSummary {
            project_start,
            bundle_verification,
            out,
            markdown,
            json,
        } => {
            let result = factory_project_start_summary_json(&project_start, &bundle_verification)?;
            atomic_write_text(&out, &serde_json::to_string_pretty(&result)?)?;
            atomic_write_text(&markdown, &factory_project_start_summary_markdown(&result))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("summary={}", out.display());
                println!("markdown={}", markdown.display());
                println!("failure_count={}", json_u64(&result, "failure_count"));
            }
            if json_string(&result, "status") != "accepted" {
                anyhow::bail!("factory project-start summary failed validation");
            }
            Ok(())
        }
        FactoryCommand::ProjectStartClosure {
            queue_status,
            latest_queue_status,
            out,
            json,
        } => {
            let result =
                factory_project_start_closure_json(&queue_status, &latest_queue_status, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("archive={}", json_string(&result, "archive"));
                println!("sha256={}", json_string(&result, "sha256"));
                println!(
                    "latest_selector_matches_run_id_selector={}",
                    result["latest_selector_matches_run_id_selector"]
                        .as_bool()
                        .unwrap_or(false)
                );
            }
            Ok(())
        }
        FactoryCommand::ProjectStartClosureVerify { bundle, json } => {
            let result = factory_project_start_closure_verify_json(&bundle)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("bundle={}", bundle.display());
                println!("run_id={}", json_string(&result, "run_id"));
                println!("failure_count={}", json_u64(&result, "failure_count"));
            }
            if json_string(&result, "status") != "accepted" {
                anyhow::bail!("factory project-start closure verification failed");
            }
            Ok(())
        }
        FactoryCommand::ReplacementPacket {
            queue_status,
            latest_queue_status,
            closure,
            closure_verification,
            out,
            cross_os_readbacks,
            json,
        } => {
            let result = factory_replacement_packet_json(FactoryReplacementPacketOptions {
                queue_status: &queue_status,
                latest_queue_status: &latest_queue_status,
                closure: &closure,
                closure_verification: &closure_verification,
                cross_os_readbacks: &cross_os_readbacks,
                out: &out,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("archive={}", json_string(&result, "archive"));
                println!("sha256={}", json_string(&result, "sha256"));
                println!(
                    "artifact_count={}",
                    result["artifact_count"].as_u64().unwrap_or_default()
                );
            }
            Ok(())
        }
        FactoryCommand::ReplacementPacketVerify { bundle, json } => {
            let result = factory_replacement_packet_verify_json(&bundle)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("bundle={}", bundle.display());
                println!("run_id={}", json_string(&result, "run_id"));
                println!("failure_count={}", json_u64(&result, "failure_count"));
            }
            if json_string(&result, "status") != "accepted" {
                anyhow::bail!("factory replacement packet verification failed");
            }
            Ok(())
        }
        FactoryCommand::ProjectRun {
            project_spec,
            project_plan,
            resume_from,
            app_runs,
            run_id,
            signing_key,
            signer_id,
            max_repair_attempts,
            out_dir,
            json,
        } => {
            let result = factory_project_run_json(FactoryProjectRunOptions {
                project_spec: &project_spec,
                project_plan: project_plan.as_deref(),
                resume_from: resume_from.as_deref(),
                app_runs: &app_runs,
                run_id,
                signing_key,
                signer_id,
                max_repair_attempts,
                out_dir: &out_dir,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "project_run={}",
                    json_string(&result["artifacts"], "factory_project_run")
                );
                println!(
                    "release_review_package={}",
                    json_string(&result["artifacts"], "release_review_package")
                );
                println!(
                    "app_run_count={}",
                    result["app_run_count"].as_u64().unwrap_or_default()
                );
            }
            Ok(())
        }
        FactoryCommand::ReplacementSmokeGate { smokes, out, json } => {
            let result = factory_replacement_smoke_gate_json(&smokes, out.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "accepted_os_count={}",
                    result["accepted_os"]
                        .as_array()
                        .map(|items| items.len())
                        .unwrap_or(0)
                );
                println!(
                    "missing_os_count={}",
                    result["missing_os"]
                        .as_array()
                        .map(|items| items.len())
                        .unwrap_or(0)
                );
            }
            if json_string(&result, "status") != "accepted" {
                return Err(anyhow!("replacement-smoke-gate rejected"));
            }
            Ok(())
        }
        FactoryCommand::ReplacementParityStatus {
            target,
            governed_run,
            governed_run_sha256,
            three_os_gate,
            three_os_gate_sha256,
            json,
        } => {
            let result = factory_replacement_parity_status_json(
                &target,
                &governed_run,
                &governed_run_sha256,
                &three_os_gate,
                &three_os_gate_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("remaining_gap_count={}", result["remaining_gaps"].as_array().map(|items| items.len()).unwrap_or(0));
                println!(
                    "next_recommended_lengthy_task={}",
                    json_string(&result, "next_recommended_lengthy_task")
                );
            }
            Ok(())
        }
        FactoryCommand::ProjectAcceptanceReview {
            project_run,
            signing_key,
            signer_id,
            out,
            json,
        } => {
            let result =
                factory_project_acceptance_review_json(FactoryProjectAcceptanceReviewOptions {
                    project_run: &project_run,
                    signing_key,
                    signer_id,
                    out: &out,
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "recommended_decision={}",
                    json_string(&result, "recommended_decision")
                );
                println!("review={}", json_string(&result["artifacts"], "review"));
                println!("rubric_sha256={}", json_string(&result, "rubric_sha256"));
            }
            Ok(())
        }
        FactoryCommand::VerifyHandoff { handoff, json } => {
            let result = factory_verify_handoff_json(&handoff)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "run_result_digest_match={}",
                    result["run_result_digest_match"]
                );
                println!("signature_verified={}", result["signature_verified"]);
            }
            Ok(())
        }
        FactoryCommand::VerifyRunResult { run_result, json } => {
            let result = factory_verify_run_result_json(&run_result)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "ao2_primary_run_result_ok={}",
                    result["ao2_primary_run_result_ok"]
                );
                println!("trust_boundary_ok={}", result["trust_boundary_ok"]);
            }
            Ok(())
        }
        FactoryCommand::VerifyPlanningEvidence {
            evidence,
            signed_payload,
            signature,
            public_key,
            json,
        } => {
            let result = factory_verify_planning_evidence_json(
                &evidence,
                signed_payload.as_deref(),
                signature.as_deref(),
                public_key.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("signature_verified={}", result["signature_verified"]);
                println!(
                    "evidence_body_matches_signed_payload={}",
                    result["evidence_body_matches_signed_payload"]
                );
                println!("trust_boundary_ok={}", result["trust_boundary_ok"]);
            }
            if json_string(&result, "status") != "accepted" {
                return Err(anyhow!(
                    "ao2 factory verify-planning-evidence rejected {}",
                    evidence.display()
                ));
            }
            Ok(())
        }
        FactoryCommand::VerifyEvaluatorDecision { decision, json } => {
            let result = factory_verify_evaluator_decision_json(&decision)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("signature_verified={}", result["signature_verified"]);
                println!("trust_boundary_ok={}", result["trust_boundary_ok"]);
            }
            Ok(())
        }
        FactoryCommand::Evaluate {
            evidence_pack,
            report,
            factory_decision,
            signing_key,
            signer_id,
            out,
            json,
        } => {
            let result = factory_evaluate_json(
                &evidence_pack,
                report.as_deref(),
                factory_decision.as_deref(),
                signing_key.as_deref(),
                &signer_id,
                out.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("verdict={}", json_string(&result, "verdict"));
                println!("decision_path={}", json_string(&result, "decision_path"));
            }
            Ok(())
        }
        FactoryCommand::QueueSubmit {
            plan,
            target,
            run_id,
            out,
            json,
        } => {
            let result = factory_queue_submit_json(&target, &plan, run_id, out.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("queue_path={}", json_string(&result, "queue_path"));
            }
            Ok(())
        }
        FactoryCommand::QueueSubmitProjectStart {
            project_spec,
            project_root,
            target,
            run_id,
            verifier_command,
            provider,
            provider_prompt_dir,
            signing_key,
            signer_id,
            max_repair_attempts,
            out_dir,
            handoff_bundle_out,
            handoff_bundle_report,
            out,
            json,
        } => {
            let result =
                factory_queue_submit_project_start_json(FactoryQueueSubmitProjectStartOptions {
                    target: &target,
                    project_spec: &project_spec,
                    project_root: &project_root,
                    run_id,
                    verifier_command,
                    provider,
                    provider_prompt_dir,
                    signing_key,
                    signer_id,
                    max_repair_attempts,
                    out_dir,
                    handoff_bundle_out,
                    handoff_bundle_report,
                    receipt_out: out.as_deref(),
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("job_kind={}", json_string(&result, "job_kind"));
                println!("queue_path={}", json_string(&result, "queue_path"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartComplete {
            project_spec,
            project_root,
            target,
            run_id,
            verifier_command,
            provider,
            provider_prompt_dir,
            signing_key,
            signer_id,
            max_repair_attempts,
            out_dir,
            handoff_bundle_out,
            handoff_bundle_report,
            json,
        } => {
            let result = factory_queue_project_start_complete_json(
                FactoryQueueProjectStartCompleteOptions {
                    target: &target,
                    project_spec: &project_spec,
                    project_root: &project_root,
                    run_id,
                    verifier_command,
                    provider,
                    provider_prompt_dir,
                    signing_key,
                    signer_id,
                    max_repair_attempts,
                    out_dir: &out_dir,
                    handoff_bundle_out,
                    handoff_bundle_report,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "ready_for_operator_review={}",
                    result["ready_for_operator_review"]
                );
                println!(
                    "completion_contract_consumer_status={}",
                    json_string(&result, "completion_contract_consumer_status")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartCompleteStatus {
            target,
            run_id,
            out_dir,
            json,
        } => {
            let result =
                factory_queue_project_start_complete_status_json(&target, &run_id, &out_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "completion_record_state={}",
                    json_string(&result, "completion_record_state")
                );
                println!(
                    "ready_for_operator_review={}",
                    result["ready_for_operator_review"]
                        .as_bool()
                        .unwrap_or(false)
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartCompletionSummary {
            target,
            run_id,
            json,
        } => {
            let result = factory_queue_project_start_completion_summary_json(&target, &run_id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "next_recommended_action={}",
                    json_string(&result["hermes_memory"], "next_recommended_action")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartCompletionSummaryMemory {
            target,
            run_id,
            approve_action_digest,
            json,
        } => {
            let result = factory_queue_project_start_completion_summary_memory_json(
                &target,
                &run_id,
                approve_action_digest.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("memory_id={}", json_string(&result["memory_record"], "id"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartCompletionSummaryMemoryStatus {
            target,
            run_id,
            json,
        } => {
            let result = factory_queue_project_start_completion_summary_memory_status_json(
                &target, &run_id,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("memory_id={}", json_string(&result["memory_record"], "id"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecovery {
            target,
            run_id,
            json,
        } => {
            let result = factory_queue_project_start_recovery_json(&target, &run_id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "next_recommended_action={}",
                    json_string(&result["hermes_memory"], "next_recommended_action")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartLatestRecovery { target, json } => {
            let result = factory_queue_project_start_latest_recovery_json(&target)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result["selected"], "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "next_recommended_action={}",
                    json_string(&result["hermes_memory"], "next_recommended_action")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryAction { target, json } => {
            let result = factory_queue_project_start_recovery_action_json(&target)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "recommended_action={}",
                    json_string(&result, "recommended_action")
                );
                println!("run_id={}", json_string(&result["selected"], "run_id"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeReceipt {
            target,
            queue_sha256,
            recovery_packet_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_receipt_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("action={}", json_string(&result, "action"));
                println!("run_id={}", json_string(&result["selected"], "run_id"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeCheckpoint {
            target,
            queue_sha256,
            recovery_packet_sha256,
            approve_action_digest,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_checkpoint_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
                approve_action_digest.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                if !json_string(&result, "action_digest").is_empty() {
                    println!("action_digest={}", json_string(&result, "action_digest"));
                }
                println!("run_id={}", json_string(&result, "run_id"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeCheckpointStatus {
            target,
            queue_sha256,
            recovery_packet_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_checkpoint_status_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "memory_record_id={}",
                    json_string(&result["memory_record"], "id")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeContinuity {
            target,
            queue_sha256,
            recovery_packet_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_continuity_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "checkpoint_memory_record_id={}",
                    json_string(&result["checkpoint_status"]["memory_record"], "id")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePlan {
            target,
            queue_sha256,
            recovery_packet_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_plan_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("plan_sha256={}", json_string(&result, "plan_sha256"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeClaim {
            target,
            queue_sha256,
            recovery_packet_sha256,
            approve_plan_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_claim_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
                approve_plan_sha256.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                if !json_string(&result, "plan_sha256").is_empty() {
                    println!("plan_sha256={}", json_string(&result, "plan_sha256"));
                }
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeClaimStatus {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_claim_status_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
                &plan_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("plan_sha256={}", json_string(&result, "plan_sha256"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeContinuationContract {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_continuation_contract_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
                &plan_sha256,
                &claim_status_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("plan_sha256={}", json_string(&result, "plan_sha256"));
                println!(
                    "claim_status_sha256={}",
                    json_string(&result, "claim_status_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeContinue {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            approve_claim_status_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_continue_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
                &plan_sha256,
                &claim_status_sha256,
                approve_claim_status_sha256.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("plan_sha256={}", json_string(&result, "plan_sha256"));
                println!(
                    "claim_status_sha256={}",
                    json_string(&result, "claim_status_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumeContinuationStatus {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_continuation_status_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
                &plan_sha256,
                &claim_status_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("plan_sha256={}", json_string(&result, "plan_sha256"));
                println!(
                    "claim_status_sha256={}",
                    json_string(&result, "claim_status_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationAction {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
            json,
        } => {
            let result = factory_queue_project_start_recovery_resume_post_continuation_action_json(
                &target,
                &queue_sha256,
                &recovery_packet_sha256,
                &plan_sha256,
                &claim_status_sha256,
                &continuation_status_sha256,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("plan_sha256={}", json_string(&result, "plan_sha256"));
                println!(
                    "continuation_status_sha256={}",
                    json_string(&result, "continuation_status_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationExecute {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
            approve_continuation_status_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_execute_json(
                    &target,
                    &queue_sha256,
                    &recovery_packet_sha256,
                    &plan_sha256,
                    &claim_status_sha256,
                    &continuation_status_sha256,
                    approve_continuation_status_sha256.as_deref(),
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "continuation_status_sha256={}",
                    json_string(&result, "continuation_status_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationExecutionStatus {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_execution_status_json(
                    &target,
                    &queue_sha256,
                    &recovery_packet_sha256,
                    &plan_sha256,
                    &claim_status_sha256,
                    &continuation_status_sha256,
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "continuation_status_sha256={}",
                    json_string(&result, "continuation_status_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationNextAction {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
            post_continuation_execution_status_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_next_action_json(
                    &target,
                    &queue_sha256,
                    &recovery_packet_sha256,
                    &plan_sha256,
                    &claim_status_sha256,
                    &continuation_status_sha256,
                    &post_continuation_execution_status_sha256,
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "post_continuation_execution_status_sha256={}",
                    json_string(&result, "post_continuation_execution_status_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationClosure {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
            post_continuation_execution_status_sha256,
            post_continuation_next_action_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_closure_json(
                    RecoveryResumePostContinuationClosureArgs {
                        target: &target,
                        queue_sha256: &queue_sha256,
                        recovery_packet_sha256: &recovery_packet_sha256,
                        plan_sha256: &plan_sha256,
                        claim_status_sha256: &claim_status_sha256,
                        continuation_status_sha256: &continuation_status_sha256,
                        post_continuation_execution_status_sha256:
                            &post_continuation_execution_status_sha256,
                        post_continuation_next_action_sha256: &post_continuation_next_action_sha256,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!(
                    "post_continuation_next_action_sha256={}",
                    json_string(&result, "post_continuation_next_action_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationEvaluatorDecision {
            target,
            queue_sha256,
            recovery_packet_sha256,
            plan_sha256,
            claim_status_sha256,
            continuation_status_sha256,
            post_continuation_execution_status_sha256,
            post_continuation_next_action_sha256,
            closure_sha256,
            signing_key,
            signer_id,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_evaluator_decision_json(
                    RecoveryResumePostContinuationEvaluatorDecisionArgs {
                        target: &target,
                        queue_sha256: &queue_sha256,
                        recovery_packet_sha256: &recovery_packet_sha256,
                        plan_sha256: &plan_sha256,
                        claim_status_sha256: &claim_status_sha256,
                        continuation_status_sha256: &continuation_status_sha256,
                        post_continuation_execution_status_sha256:
                            &post_continuation_execution_status_sha256,
                        post_continuation_next_action_sha256: &post_continuation_next_action_sha256,
                        closure_sha256: &closure_sha256,
                        signing_key: &signing_key,
                        signer_id: &signer_id,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("run_id={}", json_string(&result, "run_id"));
                println!("closure_sha256={}", json_string(&result, "closure_sha256"));
                println!("decision_path={}", json_string(&result, "decision_path"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleaseHandoff {
            target,
            decision,
            signed_payload,
            signature,
            public_key,
            closure_sha256,
            decision_sha256,
            out,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_handoff_json(
                    RecoveryResumePostContinuationReleaseHandoffArgs {
                        target: &target,
                        decision: &decision,
                        signed_payload: &signed_payload,
                        signature: &signature,
                        public_key: &public_key,
                        closure_sha256: &closure_sha256,
                        decision_sha256: &decision_sha256,
                        out: &out,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("archive={}", json_string(&result, "archive"));
                println!(
                    "signature_verified={}",
                    result["signature_verified"].as_bool().unwrap_or(false)
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatus {
            target,
            bundle,
            closure_sha256,
            decision_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_json(
                    RecoveryResumePostContinuationReleaseHandoffStatusArgs {
                        target: &target,
                        bundle: &bundle,
                        closure_sha256: &closure_sha256,
                        decision_sha256: &decision_sha256,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("bundle_sha256={}", json_string(&result, "bundle_sha256"));
                println!(
                    "signature_verified={}",
                    result["checks"]["signature_verified"]
                        .as_bool()
                        .unwrap_or(false)
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatusSummary {
            target,
            status,
            status_sha256,
            out,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_json(
                    RecoveryResumePostContinuationReleaseHandoffStatusSummaryArgs {
                        target: &target,
                        status: &status,
                        status_sha256: &status_sha256,
                        out: &out,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("status_sha256={}", json_string(&result, "status_sha256"));
                println!("summary_path={}", json_string(&result, "summary_path"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatusSummaryExport {
            target,
            summary,
            summary_sha256,
            out,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_export_json(
                    RecoveryResumePostContinuationReleaseHandoffStatusSummaryExportArgs {
                        target: &target,
                        summary: &summary,
                        summary_sha256: &summary_sha256,
                        out: &out,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("summary_sha256={}", json_string(&result, "summary_sha256"));
                println!("export_path={}", json_string(&result, "export_path"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleasePublicationReadiness {
            target,
            export,
            export_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_publication_readiness_json(
                    RecoveryResumePostContinuationReleasePublicationReadinessArgs {
                        target: &target,
                        export: &export,
                        export_sha256: &export_sha256,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("export_sha256={}", json_string(&result, "export_sha256"));
                println!(
                    "observer_fixture_sha256={}",
                    json_string(&result, "observer_fixture_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleasePublicationDispatchPlan {
            target,
            readiness,
            readiness_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_publication_dispatch_plan_json(
                    RecoveryResumePostContinuationReleasePublicationDispatchPlanArgs {
                        target: &target,
                        readiness: &readiness,
                        readiness_sha256: &readiness_sha256,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("readiness_sha256={}", json_string(&result, "readiness_sha256"));
                println!("export_sha256={}", json_string(&result, "export_sha256"));
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleasePublicationReadback {
            target,
            dispatch_plan,
            dispatch_plan_sha256,
            observation,
            observation_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_publication_readback_json(
                    RecoveryResumePostContinuationReleasePublicationReadbackArgs {
                        target: &target,
                        dispatch_plan: &dispatch_plan,
                        dispatch_plan_sha256: &dispatch_plan_sha256,
                        observation: &observation,
                        observation_sha256: &observation_sha256,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "dispatch_plan_sha256={}",
                    json_string(&result, "dispatch_plan_sha256")
                );
                println!(
                    "observation_sha256={}",
                    json_string(&result, "observation_sha256")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartRecoveryResumePostContinuationReleasePublicationClosure {
            target,
            readback,
            readback_sha256,
            json,
        } => {
            let result =
                factory_queue_project_start_recovery_resume_post_continuation_release_publication_closure_json(
                    RecoveryResumePostContinuationReleasePublicationClosureArgs {
                        target: &target,
                        readback: &readback,
                        readback_sha256: &readback_sha256,
                    },
                )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "readback_sha256={}",
                    json_string(&result, "readback_sha256")
                );
                println!(
                    "operator_summary={}",
                    json_string(&result["scheduler_closure"], "operator_summary")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartNextAction {
            target,
            run_id,
            out_dir,
            contract,
            json,
        } => {
            let result = factory_queue_project_start_next_action_json(
                &target, &run_id, &out_dir, &contract,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("next_action={}", json_string(&result, "next_action"));
                println!(
                    "completion_record_state={}",
                    json_string(&result["status_probe"], "completion_record_state")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueProjectStartPublishOperatorRecord {
            target,
            run_id,
            out_dir,
            contract,
            record_out,
            json,
        } => {
            let result = factory_queue_project_start_publish_operator_record_json(
                &target,
                &run_id,
                &out_dir,
                &contract,
                &record_out,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("record_path={}", json_string(&result, "record_path"));
            }
            Ok(())
        }
        FactoryCommand::QueueList { target, json } => {
            let result = factory_queue_list_json(&target)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("queue_path={}", json_string(&result, "queue_path"));
                println!("entry_count={}", result["entry_count"]);
            }
            Ok(())
        }
        FactoryCommand::QueueStatus {
            target,
            run_id,
            latest_completed_project_start,
            json,
        } => {
            if run_id.is_some() && latest_completed_project_start {
                anyhow::bail!(
                    "--run-id and --latest-completed-project-start are mutually exclusive"
                );
            }
            let result = if latest_completed_project_start {
                factory_queue_status_latest_completed_project_start_json(&target)?
            } else {
                let run_id = run_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("factory queue-status requires --run-id or --latest-completed-project-start"))?;
                factory_queue_status_json(&target, run_id)?
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("queue_path={}", json_string(&result, "queue_path"));
                println!(
                    "project_start_operator_summary_status={}",
                    json_string(&result["entry"], "project_start_operator_summary_status")
                );
            }
            Ok(())
        }
        FactoryCommand::QueueCompletionContract {
            target,
            run_id,
            latest_completed_project_start,
            json,
        } => {
            let result = factory_queue_completion_contract_json(
                &target,
                run_id.as_deref(),
                latest_completed_project_start,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "project_start_bundle={}",
                    json_string(&result["artifacts"], "project_start_bundle")
                );
                println!(
                    "project_start_closure_status={}",
                    json_string(&result["checks"], "project_start_closure_status")
                );
                println!(
                    "project_start_closure_verification_status={}",
                    json_string(
                        &result["checks"],
                        "project_start_closure_verification_status"
                    )
                );
            }
            Ok(())
        }
        FactoryCommand::QueueCompletionContractConsume { contract, json } => {
            let result = factory_queue_completion_contract_consumption_json(&contract)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "ready_for_operator_review={}",
                    result["ready_for_operator_review"]
                );
                println!(
                    "consumed_contract_only={}",
                    result["hermes_contract"]["consumed_contract_only"]
                );
            }
            Ok(())
        }
        FactoryCommand::CancelAuthority {
            queue_list_json,
            reason,
            produced_at_ms,
            out,
            json,
        } => {
            let result =
                factory_cancel_authority_json(&queue_list_json, reason.as_deref(), produced_at_ms)?;
            let serialized = serde_json::to_string_pretty(&result)?;
            if let Some(path) = out.as_ref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!("create attestation parent dir {}", parent.display())
                        })?;
                    }
                }
                let mut text = serialized.clone();
                text.push('\n');
                fs::write(path, text)
                    .with_context(|| format!("write attestation to {}", path.display()))?;
            }
            if json {
                println!("{serialized}");
            } else if let Some(path) = out.as_ref() {
                println!("attestation_path={}", path.display());
            } else {
                println!("schema={}", json_string(&result, "schema"));
                println!("no_active_ao2_runs={}", result["no_active_ao2_runs"]);
                println!("entry_count={}", result["source"]["entry_count"]);
            }
            Ok(())
        }
        FactoryCommand::CancelTransition {
            queue_list_json,
            run_id,
            terminated_pid,
            reason,
            produced_at_ms,
            out,
            json,
        } => {
            let result = factory_cancel_transition_json(
                &queue_list_json,
                &run_id,
                terminated_pid,
                reason.as_deref(),
                produced_at_ms,
            )?;
            let serialized = serde_json::to_string_pretty(&result)?;
            if let Some(path) = out.as_ref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!("create transition parent dir {}", parent.display())
                        })?;
                    }
                }
                let mut text = serialized.clone();
                text.push('\n');
                fs::write(path, text)
                    .with_context(|| format!("write transition to {}", path.display()))?;
            }
            if json {
                println!("{serialized}");
            } else if let Some(path) = out.as_ref() {
                println!("transition_path={}", path.display());
            } else {
                println!("schema_version={}", json_string(&result, "schema_version"));
                println!("run_id={}", json_string(&result["entry"], "run_id"));
                println!("terminated_pid={}", result["entry"]["terminated_pid"]);
            }
            Ok(())
        }
        FactoryCommand::QueueCancel {
            target,
            run_id,
            reason,
            json,
        } => {
            let result = factory_queue_transition_json(
                &target,
                &run_id,
                "cancelled",
                reason
                    .as_deref()
                    .unwrap_or("operator cancelled queued governed run"),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
            }
            Ok(())
        }
        FactoryCommand::QueueRetry {
            target,
            run_id,
            reason,
            json,
        } => {
            let result = factory_queue_transition_json(
                &target,
                &run_id,
                "queued",
                reason
                    .as_deref()
                    .unwrap_or("operator retried governed run from AO2 queue"),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
            }
            Ok(())
        }
        FactoryCommand::QueueRunNext {
            target,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            factory_decision,
            signing_key,
            signer_id,
            max_repair_attempts,
            out,
            json,
        } => {
            let result = factory_queue_run_next_json(FactoryQueueRunNextOptions {
                target: &target,
                provider,
                provider_prompt,
                provider_prompt_file,
                provider_max_budget_usd,
                factory_decision,
                signing_key,
                signer_id,
                max_repair_attempts,
                out,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!("queue_path={}", json_string(&result, "queue_path"));
                println!(
                    "evidence_pack={}",
                    json_string(&result["run_result"], "evidence_pack")
                );
            }
            Ok(())
        }
        FactoryCommand::PackEvidence {
            target,
            run_id,
            out,
            signing_key,
            signer_id,
            json,
        } => {
            let result = factory_pack_evidence_json(
                &target,
                run_id.as_deref(),
                &out,
                FactoryPlanSigning {
                    key: signing_key.as_deref(),
                    signer_id: &signer_id,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "evidence_pack_out={}",
                    json_string(&result, "evidence_pack_out")
                );
                println!(
                    "evidence_pack_source={}",
                    json_string(&result, "evidence_pack_source")
                );
            }
            Ok(())
        }
        FactoryCommand::Bridge {
            runspec,
            work_request,
            profile,
            role_contracts_dir,
            out,
            signing_key,
            signer_id,
            now_ms,
            json,
        } => {
            factory_bridge::audit_static_tables()?;
            if signing_key.is_some() && out.is_none() {
                return Err(anyhow!(
                    "ao2 factory bridge --signing-key requires --out so signed payload, signature, and public key sidecars have stable paths"
                ));
            }
            let mut evidence =
                factory_bridge::build_bridge_evidence(factory_bridge::BridgeOptions {
                    runspec_path: &runspec,
                    work_request_path: work_request.as_deref(),
                    profile_path: profile.as_deref(),
                    role_contracts_dir: role_contracts_dir.as_deref(),
                    now_ms,
                    env_keys_override: None,
                })?;
            if let (Some(key_path), Some(path)) = (signing_key.as_ref(), out.as_ref()) {
                let signed_payload_path = path.with_extension("signed-payload.json");
                let signature_path = path.with_extension("json.sig");
                let public_key_path = path.with_extension("public.pem");
                if let Some(parent) = signed_payload_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!("create bridge sidecar parent dir {}", parent.display())
                        })?;
                    }
                }
                fs::write(
                    &signed_payload_path,
                    factory_bridge::evidence_pretty(&evidence)?,
                )
                .with_context(|| {
                    format!(
                        "write bridge signed payload to {}",
                        signed_payload_path.display()
                    )
                })?;
                derive_public_key_from_private_key(key_path, &public_key_path)?;
                sign_file_with_private_key(key_path, &signed_payload_path, &signature_path)?;
                evidence["signed_evidence_status"] =
                    serde_json::json!("signed-and-verified-bridge-evidence");
                evidence["signature"] = serde_json::json!({
                    "schema_version": "ao2.factory-bridge-evidence-signature.v1",
                    "signature_algorithm": "RSA/SHA-256",
                    "signer_id": signer_id,
                    "signed_payload": "bridge_evidence_without_signature_field",
                    "signed_payload_path": signed_payload_path.display().to_string(),
                    "signed_payload_sha256": sha256_file(&signed_payload_path)?,
                    "signature_path": signature_path.display().to_string(),
                    "signature_sha256": sha256_file(&signature_path)?,
                    "public_key_path": public_key_path.display().to_string(),
                    "public_key_sha256": sha256_file(&public_key_path)?,
                    "signature_verified": verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?
                });
            } else {
                evidence["signed_evidence_status"] = serde_json::json!("unsigned-bridge-evidence");
            }
            let serialized = factory_bridge::evidence_pretty(&evidence)?;
            if let Some(path) = out.as_ref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!("create bridge-evidence parent dir {}", parent.display())
                        })?;
                    }
                }
                fs::write(path, &serialized)
                    .with_context(|| format!("write bridge-evidence to {}", path.display()))?;
            }
            if json {
                print!("{serialized}");
            } else {
                println!("schema={}", json_string(&evidence, "schema"));
                println!("status={}", json_string(&evidence, "status"));
                println!(
                    "input_runspec_sha256={}",
                    json_string(&evidence["input_runspec"], "sha256")
                );
                println!(
                    "mapping_digest={}",
                    json_string(&evidence["mapping"], "digest")
                );
                println!(
                    "resolved_role_count={}",
                    evidence["resolved_roles"]
                        .as_array()
                        .map(|items| items.len())
                        .unwrap_or(0)
                );
                println!(
                    "unknown_role_count={}",
                    evidence["unknown_roles"]
                        .as_array()
                        .map(|items| items.len())
                        .unwrap_or(0)
                );
                if let Some(path) = out.as_ref() {
                    println!("evidence_path={}", path.display());
                }
            }
            if json_string(&evidence, "status") == "blocked_unknown_roles" {
                return Err(anyhow!(
                    "bridge blocked: unknown roles {:?}",
                    evidence["unknown_roles"]
                ));
            }
            Ok(())
        }
        FactoryCommand::BridgeMapping { digest } => {
            factory_bridge::audit_static_tables()?;
            if digest {
                println!("{}", factory_bridge::mapping_digest());
            } else {
                print!("{}", factory_bridge::mapping_table_pretty()?);
            }
            Ok(())
        }
        FactoryCommand::VerifyBridgeEvidence {
            evidence,
            signed_payload,
            signature,
            public_key,
            json,
        } => {
            let result = factory_verify_bridge_evidence_json(
                &evidence,
                signed_payload.as_deref(),
                signature.as_deref(),
                public_key.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "signature_status={}",
                    json_string(&result, "signature_status")
                );
                println!("signature_verified={}", result["signature_verified"]);
                println!(
                    "evidence_body_matches_signed_payload={}",
                    result["evidence_body_matches_signed_payload"]
                );
                println!("trust_boundary_ok={}", result["trust_boundary_ok"]);
            }
            if json_string(&result, "status") != "accepted" {
                return Err(anyhow!(
                    "ao2 factory verify-bridge-evidence rejected {}",
                    evidence.display()
                ));
            }
            Ok(())
        }
    }
}
