use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::cli_util::{
    canonical_json_sha256, form_value_owned, json_string, percent_decode, query_value_owned,
};
use crate::factory_evidence::{factory_plan_json, FactoryPlanSigning};
use crate::factory_governance::factory_replacement_parity_status_json;
use crate::factory_queue_execution::{factory_queue_run_next_json, FactoryQueueRunNextOptions};
use crate::factory_queue_operator::{
    factory_project_start_hermes_flow_contract_json, factory_queue_project_start_next_action_json,
    factory_queue_project_start_publish_operator_record_json,
};
use crate::factory_queue_project_start::{
    factory_queue_submit_project_start_json, FactoryQueueSubmitProjectStartOptions,
};
use crate::factory_queue_recovery::{
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
use crate::greenfield_workflow::factory_greenfield_spec_ingest_json;
use crate::{
    factory_queue_load, factory_queue_path, factory_queue_project_start_completion_summary_json,
    WorkbenchSupportSigning,
};

pub(crate) fn workbench_factory_project_start_next_action_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let run_id = query_value_owned(query, "run_id").context("run_id is required")?;
    let out_dir = query_value_owned(query, "out_dir")
        .map(PathBuf::from)
        .context("out_dir is required")?;
    let contract = query_value_owned(query, "contract")
        .map(PathBuf::from)
        .context("contract is required")?;
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    factory_queue_project_start_next_action_json(&effective_target, &run_id, &out_dir, &contract)
}

pub(crate) fn workbench_factory_project_start_completion_summary_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let run_id = query_value_owned(query, "run_id").context("run_id is required")?;
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    factory_queue_project_start_completion_summary_json(&effective_target, &run_id)
}

pub(crate) fn workbench_factory_project_start_completion_summary_memory_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let run_id = form_value_owned(form, "run_id").context("run_id is required")?;
    let approval = form_value_owned(form, "approval_action_digest");
    factory_queue_project_start_completion_summary_memory_json(target, &run_id, approval.as_deref())
}

pub(crate) fn workbench_factory_project_start_completion_summary_memory_status_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let run_id = query_value_owned(query, "run_id").context("run_id is required")?;
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    factory_queue_project_start_completion_summary_memory_status_json(&effective_target, &run_id)
}

pub(crate) fn workbench_factory_project_start_recovery_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let run_id = query_value_owned(query, "run_id").context("run_id is required")?;
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    factory_queue_project_start_recovery_json(&effective_target, &run_id)
}

pub(crate) fn workbench_factory_project_start_latest_recovery_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    factory_queue_project_start_latest_recovery_json(&effective_target)
}

pub(crate) fn workbench_factory_project_start_recovery_action_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    factory_queue_project_start_recovery_action_json(&effective_target)
}

pub(crate) fn workbench_factory_project_start_recovery_resume_receipt_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    factory_queue_project_start_recovery_resume_receipt_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_checkpoint_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let queue_sha256 =
        form_value_owned(form, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = form_value_owned(form, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let approval = form_value_owned(form, "approval_action_digest");
    factory_queue_project_start_recovery_resume_checkpoint_json(
        target,
        &queue_sha256,
        &recovery_packet_sha256,
        approval.as_deref(),
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_checkpoint_status_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    factory_queue_project_start_recovery_resume_checkpoint_status_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_continuity_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    factory_queue_project_start_recovery_resume_continuity_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_plan_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    factory_queue_project_start_recovery_resume_plan_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_claim_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let queue_sha256 =
        form_value_owned(form, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = form_value_owned(form, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let approval = form_value_owned(form, "approval_plan_sha256");
    factory_queue_project_start_recovery_resume_claim_json(
        target,
        &queue_sha256,
        &recovery_packet_sha256,
        approval.as_deref(),
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_claim_status_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    factory_queue_project_start_recovery_resume_claim_status_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_continuation_contract_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 = query_value_owned(query, "claim_status_sha256")
        .context("claim_status_sha256 is required")?;
    factory_queue_project_start_recovery_resume_continuation_contract_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
        &claim_status_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_continue_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let queue_sha256 =
        form_value_owned(form, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = form_value_owned(form, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = form_value_owned(form, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 =
        form_value_owned(form, "claim_status_sha256").context("claim_status_sha256 is required")?;
    let approval = form_value_owned(form, "approval_claim_status_sha256");
    factory_queue_project_start_recovery_resume_continue_json(
        target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
        &claim_status_sha256,
        approval.as_deref(),
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_continuation_status_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 = query_value_owned(query, "claim_status_sha256")
        .context("claim_status_sha256 is required")?;
    factory_queue_project_start_recovery_resume_continuation_status_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
        &claim_status_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_action_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 = query_value_owned(query, "claim_status_sha256")
        .context("claim_status_sha256 is required")?;
    let continuation_status_sha256 = query_value_owned(query, "continuation_status_sha256")
        .context("continuation_status_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_action_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
        &claim_status_sha256,
        &continuation_status_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_execute_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let queue_sha256 =
        form_value_owned(form, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = form_value_owned(form, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = form_value_owned(form, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 =
        form_value_owned(form, "claim_status_sha256").context("claim_status_sha256 is required")?;
    let continuation_status_sha256 = form_value_owned(form, "continuation_status_sha256")
        .context("continuation_status_sha256 is required")?;
    let approval = form_value_owned(form, "approval_continuation_status_sha256");
    factory_queue_project_start_recovery_resume_post_continuation_execute_json(
        target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
        &claim_status_sha256,
        &continuation_status_sha256,
        approval.as_deref(),
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_execution_status_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 = query_value_owned(query, "claim_status_sha256")
        .context("claim_status_sha256 is required")?;
    let continuation_status_sha256 = query_value_owned(query, "continuation_status_sha256")
        .context("continuation_status_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_execution_status_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
        &claim_status_sha256,
        &continuation_status_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_next_action_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 = query_value_owned(query, "claim_status_sha256")
        .context("claim_status_sha256 is required")?;
    let continuation_status_sha256 = query_value_owned(query, "continuation_status_sha256")
        .context("continuation_status_sha256 is required")?;
    let post_continuation_execution_status_sha256 =
        query_value_owned(query, "post_continuation_execution_status_sha256")
            .context("post_continuation_execution_status_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_next_action_json(
        &effective_target,
        &queue_sha256,
        &recovery_packet_sha256,
        &plan_sha256,
        &claim_status_sha256,
        &continuation_status_sha256,
        &post_continuation_execution_status_sha256,
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_closure_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 = query_value_owned(query, "claim_status_sha256")
        .context("claim_status_sha256 is required")?;
    let continuation_status_sha256 = query_value_owned(query, "continuation_status_sha256")
        .context("continuation_status_sha256 is required")?;
    let post_continuation_execution_status_sha256 =
        query_value_owned(query, "post_continuation_execution_status_sha256")
            .context("post_continuation_execution_status_sha256 is required")?;
    let post_continuation_next_action_sha256 =
        query_value_owned(query, "post_continuation_next_action_sha256")
            .context("post_continuation_next_action_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_closure_json(
        RecoveryResumePostContinuationClosureArgs {
            target: &effective_target,
            queue_sha256: &queue_sha256,
            recovery_packet_sha256: &recovery_packet_sha256,
            plan_sha256: &plan_sha256,
            claim_status_sha256: &claim_status_sha256,
            continuation_status_sha256: &continuation_status_sha256,
            post_continuation_execution_status_sha256: &post_continuation_execution_status_sha256,
            post_continuation_next_action_sha256: &post_continuation_next_action_sha256,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_evaluator_decision_json(
    target: &Path,
    query: &str,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let support_signing = support_signing.context(
        "workbench support signing key is required for recovery evaluator decision artifacts",
    )?;
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let queue_sha256 =
        query_value_owned(query, "queue_sha256").context("queue_sha256 is required")?;
    let recovery_packet_sha256 = query_value_owned(query, "recovery_packet_sha256")
        .context("recovery_packet_sha256 is required")?;
    let plan_sha256 = query_value_owned(query, "plan_sha256").context("plan_sha256 is required")?;
    let claim_status_sha256 = query_value_owned(query, "claim_status_sha256")
        .context("claim_status_sha256 is required")?;
    let continuation_status_sha256 = query_value_owned(query, "continuation_status_sha256")
        .context("continuation_status_sha256 is required")?;
    let post_continuation_execution_status_sha256 =
        query_value_owned(query, "post_continuation_execution_status_sha256")
            .context("post_continuation_execution_status_sha256 is required")?;
    let post_continuation_next_action_sha256 =
        query_value_owned(query, "post_continuation_next_action_sha256")
            .context("post_continuation_next_action_sha256 is required")?;
    let closure_sha256 =
        query_value_owned(query, "closure_sha256").context("closure_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_evaluator_decision_json(
        RecoveryResumePostContinuationEvaluatorDecisionArgs {
            target: &effective_target,
            queue_sha256: &queue_sha256,
            recovery_packet_sha256: &recovery_packet_sha256,
            plan_sha256: &plan_sha256,
            claim_status_sha256: &claim_status_sha256,
            continuation_status_sha256: &continuation_status_sha256,
            post_continuation_execution_status_sha256: &post_continuation_execution_status_sha256,
            post_continuation_next_action_sha256: &post_continuation_next_action_sha256,
            closure_sha256: &closure_sha256,
            signing_key: &support_signing.key_path,
            signer_id: &support_signing.signer_id,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let decision = query_value_owned(query, "decision")
        .map(PathBuf::from)
        .context("decision is required")?;
    let signed_payload = query_value_owned(query, "signed_payload")
        .map(PathBuf::from)
        .context("signed_payload is required")?;
    let signature = query_value_owned(query, "signature")
        .map(PathBuf::from)
        .context("signature is required")?;
    let public_key = query_value_owned(query, "public_key")
        .map(PathBuf::from)
        .context("public_key is required")?;
    let closure_sha256 =
        query_value_owned(query, "closure_sha256").context("closure_sha256 is required")?;
    let decision_sha256 =
        query_value_owned(query, "decision_sha256").context("decision_sha256 is required")?;
    let out = query_value_owned(query, "out")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            effective_target
                .join(".ao2")
                .join("factory-compat")
                .join("recovery-release-handoffs")
                .join("recovery-resume-post-continuation-release-handoff.tgz")
        });
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_json(
        RecoveryResumePostContinuationReleaseHandoffArgs {
            target: &effective_target,
            decision: &decision,
            signed_payload: &signed_payload,
            signature: &signature,
            public_key: &public_key,
            closure_sha256: &closure_sha256,
            decision_sha256: &decision_sha256,
            out: &out,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let bundle = query_value_owned(query, "bundle")
        .map(PathBuf::from)
        .context("bundle is required")?;
    let closure_sha256 =
        query_value_owned(query, "closure_sha256").context("closure_sha256 is required")?;
    let decision_sha256 =
        query_value_owned(query, "decision_sha256").context("decision_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_json(
        RecoveryResumePostContinuationReleaseHandoffStatusArgs {
            target: &effective_target,
            bundle: &bundle,
            closure_sha256: &closure_sha256,
            decision_sha256: &decision_sha256,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_summary_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let status = query_value_owned(query, "status")
        .map(PathBuf::from)
        .context("status is required")?;
    let status_sha256 =
        query_value_owned(query, "status_sha256").context("status_sha256 is required")?;
    let out = query_value_owned(query, "out")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            effective_target
                .join(".ao2")
                .join("factory-compat")
                .join("recovery-release-handoff-status-summaries")
                .join("recovery-resume-post-continuation-release-handoff-status-summary.json")
        });
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_json(
        RecoveryResumePostContinuationReleaseHandoffStatusSummaryArgs {
            target: &effective_target,
            status: &status,
            status_sha256: &status_sha256,
            out: &out,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_summary_export_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let summary = query_value_owned(query, "summary")
        .map(PathBuf::from)
        .context("summary is required")?;
    let summary_sha256 =
        query_value_owned(query, "summary_sha256").context("summary_sha256 is required")?;
    let out = query_value_owned(query, "out")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            effective_target
                .join(".ao2")
                .join("factory-compat")
                .join("recovery-release-handoff-status-summary-exports")
                .join(
                    "recovery-resume-post-continuation-release-handoff-status-summary-export.json",
                )
        });
    factory_queue_project_start_recovery_resume_post_continuation_release_handoff_status_summary_export_json(
        RecoveryResumePostContinuationReleaseHandoffStatusSummaryExportArgs {
            target: &effective_target,
            summary: &summary,
            summary_sha256: &summary_sha256,
            out: &out,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_publication_readiness_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let export = query_value_owned(query, "export")
        .map(PathBuf::from)
        .context("export is required")?;
    let export_sha256 =
        query_value_owned(query, "export_sha256").context("export_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_readiness_json(
        RecoveryResumePostContinuationReleasePublicationReadinessArgs {
            target: &effective_target,
            export: &export,
            export_sha256: &export_sha256,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_publication_dispatch_plan_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let readiness = query_value_owned(query, "readiness")
        .map(PathBuf::from)
        .context("readiness is required")?;
    let readiness_sha256 =
        query_value_owned(query, "readiness_sha256").context("readiness_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_dispatch_plan_json(
        RecoveryResumePostContinuationReleasePublicationDispatchPlanArgs {
            target: &effective_target,
            readiness: &readiness,
            readiness_sha256: &readiness_sha256,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_publication_readback_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let dispatch_plan = query_value_owned(query, "dispatch_plan")
        .map(PathBuf::from)
        .context("dispatch_plan is required")?;
    let dispatch_plan_sha256 = query_value_owned(query, "dispatch_plan_sha256")
        .context("dispatch_plan_sha256 is required")?;
    let observation = query_value_owned(query, "observation")
        .map(PathBuf::from)
        .context("observation is required")?;
    let observation_sha256 =
        query_value_owned(query, "observation_sha256").context("observation_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_readback_json(
        RecoveryResumePostContinuationReleasePublicationReadbackArgs {
            target: &effective_target,
            dispatch_plan: &dispatch_plan,
            dispatch_plan_sha256: &dispatch_plan_sha256,
            observation: &observation,
            observation_sha256: &observation_sha256,
        },
    )
}

pub(crate) fn workbench_factory_project_start_recovery_resume_post_continuation_release_publication_closure_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let readback = query_value_owned(query, "readback")
        .map(PathBuf::from)
        .context("readback is required")?;
    let readback_sha256 =
        query_value_owned(query, "readback_sha256").context("readback_sha256 is required")?;
    factory_queue_project_start_recovery_resume_post_continuation_release_publication_closure_json(
        RecoveryResumePostContinuationReleasePublicationClosureArgs {
            target: &effective_target,
            readback: &readback,
            readback_sha256: &readback_sha256,
        },
    )
}

pub(crate) fn workbench_factory_replacement_parity_status_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let governed_run = query_value_owned(query, "governed_run")
        .map(PathBuf::from)
        .context("governed_run is required")?;
    let governed_run_sha256 = query_value_owned(query, "governed_run_sha256")
        .context("governed_run_sha256 is required")?;
    let three_os_gate = query_value_owned(query, "three_os_gate")
        .map(PathBuf::from)
        .context("three_os_gate is required")?;
    let three_os_gate_sha256 = query_value_owned(query, "three_os_gate_sha256")
        .context("three_os_gate_sha256 is required")?;
    factory_replacement_parity_status_json(
        &effective_target,
        &governed_run,
        &governed_run_sha256,
        &three_os_gate,
        &three_os_gate_sha256,
    )
}

pub(crate) fn workbench_factory_compat_plan_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let request = form_value_owned(form, "request")
        .map(PathBuf::from)
        .context("request is required")?;
    let profile = form_value_owned(form, "profile").map(PathBuf::from);
    let runspec = form_value_owned(form, "runspec").map(PathBuf::from);
    let role_contracts = form
        .get("role_contracts")
        .or_else(|| form.get("role_contract"))
        .map(|value| {
            value
                .split([',', '\n'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let signing_key = form_value_owned(form, "signing_key").map(PathBuf::from);
    let signer_id = form_value_owned(form, "signer_id")
        .unwrap_or_else(|| "ao2-workbench-factory-compat-plan".to_string());
    let out = form_value_owned(form, "out").map(PathBuf::from);
    factory_plan_json(
        &request,
        profile.as_deref(),
        runspec.as_deref(),
        &role_contracts,
        FactoryPlanSigning {
            key: signing_key.as_deref(),
            signer_id: &signer_id,
        },
        target,
        out.as_deref(),
    )
}

pub(crate) fn workbench_factory_project_start_hermes_flow_contract_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let effective_target = query_value_owned(query, "target").map_or_else(
        || target.to_path_buf(),
        |value| PathBuf::from(percent_decode(&value)),
    );
    let out = query_value_owned(query, "out")
        .map(|value| PathBuf::from(percent_decode(&value)))
        .context("out is required")?;
    factory_project_start_hermes_flow_contract_json(&effective_target, &out)
}

pub(crate) fn workbench_factory_greenfield_spec_ingest_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let spec = query_value_owned(query, "spec")
        .map(PathBuf::from)
        .context("spec is required")?;
    let effective_target = match query_value_owned(query, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target query parameter must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let run_id = query_value_owned(query, "run_id");
    let verifier_command = query_value_owned(query, "verifier_command")
        .unwrap_or_else(|| "npm run verify".to_string());
    factory_greenfield_spec_ingest_json(&spec, &effective_target, run_id, &verifier_command)
}

pub(crate) fn workbench_factory_greenfield_spec_ingest_submit_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let spec = form_value_owned(form, "spec")
        .map(PathBuf::from)
        .context("spec is required")?;
    let effective_target = match form_value_owned(form, "target").map(PathBuf::from) {
        Some(requested_target) => {
            let requested = fs::canonicalize(&requested_target).with_context(|| {
                format!(
                    "canonicalize requested workbench target {}",
                    requested_target.display()
                )
            })?;
            let served = fs::canonicalize(target)
                .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
            if requested != served {
                anyhow::bail!(
                    "target form field must match the served workbench target: requested {}, served {}",
                    requested.display(),
                    served.display()
                );
            }
            requested
        }
        None => target.to_path_buf(),
    };
    let run_id = form_value_owned(form, "run_id");
    let verifier_command =
        form_value_owned(form, "verifier_command").unwrap_or_else(|| "npm run verify".to_string());
    let preflight = factory_greenfield_spec_ingest_json(
        &spec,
        &effective_target,
        run_id.clone(),
        &verifier_command,
    )?;
    let digest_input = serde_json::json!({
        "action": "ao2.workbench-greenfield-spec-ingest-submit.v1",
        "preflight": preflight
    });
    let action_digest = canonical_json_sha256(&digest_input);
    let submitted_digest = form_value_owned(form, "approval_action_digest").unwrap_or_default();
    if submitted_digest != action_digest {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-greenfield-spec-ingest-submit-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_action_digest",
            "required_form_field": "approval_action_digest",
            "action_digest": action_digest,
            "preflight": digest_input["preflight"].clone(),
            "next_action": "submit approval_action_digest with the exact action_digest to submit the AO2 project-start queue entry",
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
    let max_repair_attempts = form
        .get("max_repair_attempts")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1);
    let queue_submit =
        factory_queue_submit_project_start_json(FactoryQueueSubmitProjectStartOptions {
            target: &effective_target,
            project_spec: &spec,
            project_root: &effective_target,
            run_id: Some(run_id.clone()),
            verifier_command,
            provider: form_value_owned(form, "provider"),
            provider_prompt_dir: form_value_owned(form, "provider_prompt_dir").map(PathBuf::from),
            signing_key: None,
            signer_id: "ao2-workbench".to_string(),
            max_repair_attempts,
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

pub(crate) fn workbench_factory_project_start_run_next_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let run_id = form_value_owned(form, "run_id").context("run_id is required")?;
    let target_root = fs::canonicalize(target)
        .with_context(|| format!("canonicalize workbench target {}", target.display()))?;
    let queue = factory_queue_load(&target_root)?;
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let queued_entry = entries
        .iter()
        .find(|entry| {
            json_string(entry, "run_id") == run_id
                && json_string(entry, "status") == "queued"
                && json_string(entry, "job_kind") == "factory_project_start"
        })
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "factory project-start queue has no queued factory_project_start entry for run_id {run_id}"
            )
        })?;
    let first_queued_run_id = entries
        .iter()
        .find(|entry| json_string(entry, "status") == "queued")
        .map(|entry| json_string(entry, "run_id"))
        .unwrap_or_default();
    if first_queued_run_id != run_id {
        anyhow::bail!(
            "requested run_id {run_id} is not the next queued entry; next queued run_id is {first_queued_run_id}"
        );
    }
    let trust_boundary = serde_json::json!({
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
    });
    let digest_input = serde_json::json!({
        "action": "ao2.workbench-project-start-run-next.v1",
        "run_id": run_id,
        "queue_path": factory_queue_path(&target_root).display().to_string(),
        "queued_entry": queued_entry,
        "trust_boundary": trust_boundary
    });
    let action_digest = canonical_json_sha256(&digest_input);
    let submitted_digest = form_value_owned(form, "approval_action_digest").unwrap_or_default();
    if submitted_digest != action_digest {
        return Ok(serde_json::json!({
            "schema_version": "ao2.factory-project-start-workbench-run-next-approval.v1",
            "status": if submitted_digest.is_empty() {
                "approval_required"
            } else {
                "approval_digest_mismatch"
            },
            "approval_mode": "exact_action_digest",
            "required_form_field": "approval_action_digest",
            "action_digest": action_digest,
            "run_id": digest_input["run_id"].clone(),
            "queue_path": digest_input["queue_path"].clone(),
            "queued_entry": digest_input["queued_entry"].clone(),
            "next_action": "submit approval_action_digest with the exact action_digest to run the next AO2 project-start queue entry",
            "side_effects": {
                "would_execute_queue_after_approval": true,
                "would_execute_provider": false,
                "would_mutate_control_plane": false,
                "would_write_queue_file": true
            },
            "trust_boundary": digest_input["trust_boundary"].clone(),
            "ao2_decision_owner": "ao2-workbench-queue"
        }));
    }

    let max_repair_attempts = form
        .get("max_repair_attempts")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1);
    let signer_id =
        form_value_owned(form, "signer_id").unwrap_or_else(|| "ao2-workbench".to_string());
    let queue_run_next = factory_queue_run_next_json(FactoryQueueRunNextOptions {
        target: &target_root,
        provider: None,
        provider_prompt: None,
        provider_prompt_file: None,
        provider_max_budget_usd: None,
        factory_decision: None,
        signing_key: None,
        signer_id,
        max_repair_attempts,
        out: None,
    })?;
    if json_string(&queue_run_next, "run_id") != json_string(&digest_input, "run_id") {
        anyhow::bail!(
            "AO2 queue-run-next returned run_id {}, expected {}",
            json_string(&queue_run_next, "run_id"),
            json_string(&digest_input, "run_id")
        );
    }
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-workbench-run-next.v1",
        "status": json_string(&queue_run_next, "status"),
        "run_id": digest_input["run_id"].clone(),
        "approval_status": "approved_exact_action_digest",
        "approval": {
            "schema_version": "ao2.factory-project-start-workbench-run-next-approval.v1",
            "status": "approved_exact_action_digest",
            "approval_mode": "exact_action_digest",
            "action_digest": action_digest
        },
        "queue_run_next": queue_run_next,
        "side_effects": {
            "executed_queue": true,
            "executed_provider": false,
            "wrote_queue_file": true,
            "mutated_control_plane": false
        },
        "trust_boundary": digest_input["trust_boundary"].clone(),
        "ao2_decision_owner": "ao2-workbench-queue"
    }))
}

pub(crate) fn workbench_factory_project_start_operator_record_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let run_id = form_value_owned(form, "run_id").context("run_id is required")?;
    let out_dir = form_value_owned(form, "out_dir")
        .map(PathBuf::from)
        .context("out_dir is required")?;
    let contract = form_value_owned(form, "contract")
        .map(PathBuf::from)
        .context("contract is required")?;
    let record_out = form_value_owned(form, "record_out")
        .map(PathBuf::from)
        .context("record_out is required")?;
    factory_queue_project_start_publish_operator_record_json(
        target,
        &run_id,
        &out_dir,
        &contract,
        &record_out,
    )
}
