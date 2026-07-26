#![recursion_limit = "256"]

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write as IoWrite};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use ao2_adapters::{
    apply_sandbox_patch, build_provider_prompt_command, doctor_provider, parse_provider,
    preview_sandbox_patch, run_provider_prompt_in_sandbox, AdapterRunRequest, LocalCliAdapter,
    ProviderPromptRequest, SandboxPatchApplyRequest, SandboxRunRequest,
    DEFAULT_PROVIDER_TIMEOUT_SECONDS,
};
use ao2_core::{
    annotate_obligation_ledger, check_obligation_ledger, extract_obligation_ledger, sha256_hex,
    ObligationEvidence, ObligationLedger, ObligationStatus,
};
use ao2_policy::redact_secrets;
use ao2_runtime::{
    approve_risky_pr_ticket, replay_run, run_risky_pr_with_provider_prompt, ApprovalOptions,
    ProviderRunOptions, RepairSourceContext, ReplayOptions,
};
use chrono::{SecondsFormat, Utc};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

mod artifact_safety;
mod cli_util;
mod contract_gate_signing;
mod control_plane_http;
mod control_plane_ops;
mod control_plane_snapshot;
mod doctor_cmd;
mod evidence_publish;
mod factory_app_run;
mod factory_bridge;
mod factory_compat;
mod factory_evaluator;
mod factory_evidence;
mod factory_governance;
mod factory_project_contract;
mod factory_project_execution;
mod factory_project_planning;
mod factory_project_start;
mod factory_project_start_summary;
mod factory_queue;
mod factory_queue_execution;
mod factory_queue_operator;
mod factory_queue_project_start;
mod factory_queue_recovery;
mod factory_queue_recovery_release;
mod factory_run_execution;
mod git_cmd;
mod github_issue_draft;
mod github_issue_intake;
mod greenfield_workflow;
mod install_cmd;
mod install_paths;
mod memory_store;
mod phase1_promotion;
mod plugin_adapter;
mod plugin_cli;
mod plugin_consumer;
mod plugin_contract;
mod plugin_distribution;
mod plugin_pulse;
mod plugin_release;
mod plugin_wrapper;
mod provider_contract;
mod provider_ops;
mod pulse_eval_loop;
mod pulse_run;
mod release_archive_contract;
mod release_assets;
mod release_comparison;
mod release_crypto;
mod release_gate;
mod release_handoff;
mod release_history;
mod release_installer_scripts;
mod release_package;
mod release_provenance;
mod release_summary;
mod release_summary_enrich;
mod release_support_bundle_ci;
mod release_verifier_scripts;
mod release_versioning;
mod risky_pr_readback;
mod run_execution;
mod run_reporting;
mod run_resume;
mod sdd_cmd;
mod support_bundle;
mod upgrade_cmd;
mod windows_input;
mod workbench_app;
mod workbench_contract;
mod workbench_evidence_delivery;
mod workbench_factory_api;
mod workbench_memory;
mod workbench_obligation;
mod workbench_provider_pilot;
mod workbench_provider_pilot_acceptance;
mod workbench_provider_pilot_history;
mod workbench_queue;
mod workbench_release;
mod workbench_release_latest;
mod workbench_render;
mod workbench_run_evidence;
mod workbench_server;
mod workbench_support;
mod workbench_support_latest;

pub(crate) use artifact_safety::{
    factory_app_run_bundle_reject_secret_fields, factory_app_run_bundle_reject_secret_markers,
};
use factory_evidence::{
    factory_pack_evidence_json, factory_plan_json, factory_verify_bridge_evidence_json,
    factory_verify_evaluator_decision_json, factory_verify_planning_evidence_json,
    FactoryPlanSigning,
};
use factory_governance::{
    factory_closer_decision_json, factory_closer_decision_verify_json, factory_governed_run_json,
    factory_replacement_parity_status_json, factory_replacement_smoke_gate_json,
    factory_replacement_smoke_json, factory_verify_handoff_json, factory_verify_run_result_json,
    greenfield_three_os_smoke_gate_json, FactoryCloserDecisionOptions, FactoryGovernedRunOptions,
    FactoryReplacementSmokeOptions,
};
use factory_queue_execution::{
    factory_queue_run_next_json, factory_queue_transition_json, FactoryQueueRunNextOptions,
};
use factory_queue_operator::{
    factory_project_start_hermes_context_json, factory_project_start_hermes_flow_contract_json,
    factory_queue_project_start_next_action_json,
    factory_queue_project_start_publish_operator_record_json,
};
use factory_queue_project_start::{
    factory_queue_project_start_complete_json, factory_queue_submit_project_start_json,
    FactoryQueueProjectStartCompleteOptions, FactoryQueueSubmitProjectStartOptions,
};
use factory_queue_recovery_release::{
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
use factory_run_execution::{factory_run_plan_json, FactoryRunPlanOptions};
use greenfield_workflow::{
    factory_greenfield_run_json, factory_greenfield_spec_ingest_json,
    factory_greenfield_spec_ingest_submit_json, greenfield_governed_run_json,
    greenfield_ingest_json, FactoryGreenfieldRunOptions, FactoryGreenfieldSpecIngestSubmitOptions,
    GreenfieldGovernedRunOptions, GreenfieldIngestOptions,
};
use phase1_promotion::{
    phase1_promotion_decision_build_json, phase1_promotion_decision_publish_to_control_plane_json,
    phase1_promotion_history_fetch_from_control_plane_json,
    phase1_promotion_inputs_publish_to_control_plane_json, phase1_promotion_inputs_verify_json,
    phase1_promotion_status_json, phase1_three_os_smoke_build_json,
    phase1_three_os_smoke_publish_to_control_plane_json,
};
use plugin_cli::plugin;
use plugin_distribution::{
    reject_secret_markers_in_bytes, validate_plugin_observer_trust_boundary,
    validate_plugin_provider_auth,
};
use workbench_app::workbench_export;
use workbench_contract::WorkbenchSupportSigning;
use workbench_queue::*;
use workbench_render::{render_workbench, WorkbenchRenderOptions};
use workbench_server::{
    parse_http_request_line, serve_cockpit, serve_workbench, split_path_query,
    ServeWorkbenchOptions,
};

use workbench_factory_api::{
    workbench_factory_compat_plan_json, workbench_factory_greenfield_spec_ingest_json,
    workbench_factory_greenfield_spec_ingest_submit_json,
    workbench_factory_project_start_completion_summary_json,
    workbench_factory_project_start_completion_summary_memory_json,
    workbench_factory_project_start_completion_summary_memory_status_json,
    workbench_factory_project_start_hermes_flow_contract_json,
    workbench_factory_project_start_latest_recovery_json,
    workbench_factory_project_start_next_action_json,
    workbench_factory_project_start_operator_record_json,
    workbench_factory_project_start_recovery_action_json,
    workbench_factory_project_start_recovery_json,
    workbench_factory_project_start_recovery_resume_checkpoint_json,
    workbench_factory_project_start_recovery_resume_checkpoint_status_json,
    workbench_factory_project_start_recovery_resume_claim_json,
    workbench_factory_project_start_recovery_resume_claim_status_json,
    workbench_factory_project_start_recovery_resume_continuation_contract_json,
    workbench_factory_project_start_recovery_resume_continuation_status_json,
    workbench_factory_project_start_recovery_resume_continue_json,
    workbench_factory_project_start_recovery_resume_continuity_json,
    workbench_factory_project_start_recovery_resume_plan_json,
    workbench_factory_project_start_recovery_resume_post_continuation_action_json,
    workbench_factory_project_start_recovery_resume_post_continuation_closure_json,
    workbench_factory_project_start_recovery_resume_post_continuation_evaluator_decision_json,
    workbench_factory_project_start_recovery_resume_post_continuation_execute_json,
    workbench_factory_project_start_recovery_resume_post_continuation_execution_status_json,
    workbench_factory_project_start_recovery_resume_post_continuation_next_action_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_summary_export_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_summary_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_publication_closure_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_publication_dispatch_plan_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_publication_readback_json,
    workbench_factory_project_start_recovery_resume_post_continuation_release_publication_readiness_json,
    workbench_factory_project_start_recovery_resume_receipt_json,
    workbench_factory_project_start_run_next_json,
    workbench_factory_replacement_parity_status_json,
};

use cli_util::{
    atomic_write_text, canonical_json_sha256, create_tar_gz, escape_html,
    fail_if_provider_api_key_env_present, json_array, json_bool, json_string, json_u64,
    json_value_text, now_unix_ms, read_json_file, read_prompt, sha256_bytes_hex, sha256_file,
    trimmed_required,
};
use contract_gate_signing::{
    contract_obligation_gate_signing_survey_json, contract_verify_obligation_gate_signing_json,
    emit_contract_gate_signed_wrapper,
};
use control_plane_ops::{
    control_plane, render_workbench_queue_failure_diagnostics_table,
    render_workbench_redaction_audit_section, workbench_support_bundle_import,
    workbench_support_bundle_inspect, workbench_support_bundle_verify,
    workbench_support_evidence_export_subject, workbench_support_keygen,
    write_workbench_support_metadata,
};
use control_plane_snapshot::cp;
use doctor_cmd::{doctor, doctor_report_json};
use evidence_publish::{evidence, EvidenceCommand};
use factory_app_run::{factory_app_run_bundle_json, factory_app_run_json, FactoryAppRunOptions};
use factory_compat::{
    classify_factory_shape, classify_factory_size, factory_classification_signals,
    factory_ensure_target_repo, reject_factory_provider_api_key_auth,
};
use factory_evaluator::{
    factory_evaluate_json, factory_evaluator_rubric_json, FactoryEvaluatorRubricOptions,
};
use factory_project_execution::{
    factory_project_acceptance_review_json, factory_project_run_json,
    FactoryProjectAcceptanceReviewOptions, FactoryProjectRunOptions,
};
use factory_project_planning::{
    factory_project_plan_json, factory_project_plan_validate_json, FactoryProjectPlanOptions,
    FactoryProjectPlanValidateOptions,
};
use factory_project_start::{
    factory_project_start_bundle_json, factory_project_start_bundle_verify_json,
    factory_project_start_closure_json, factory_project_start_closure_verify_json,
    factory_project_start_json, factory_replacement_packet_json,
    factory_replacement_packet_verify_json, FactoryProjectStartOptions,
    FactoryReplacementPacketOptions,
};
use factory_project_start_summary::{
    factory_project_start_summary_json, factory_project_start_summary_markdown,
};
use factory_queue::{
    factory_cancel_authority_json, factory_cancel_transition_json,
    factory_queue_completion_contract_consumption_json, factory_queue_completion_contract_json,
    factory_queue_list_json, factory_queue_load, factory_queue_path,
    factory_queue_project_start_completion_summary_json, factory_queue_status_json,
    factory_queue_status_latest_completed_project_start_json, factory_queue_submit_json,
};
use factory_queue_recovery::*;
use git_cmd::git;
use github_issue_intake::issue;
use install_cmd::{install, InstallCommand};
use memory_store::{memory, MemoryCommand};
use provider_contract::{provider_contract_json, provider_contract_verify_json};
use provider_ops::{
    materialize_template_workflow, provider, provider_matrix_json, provider_profiles,
    provider_profiles_json, provider_smoke_all_json, provider_warning_strings,
};
use pulse_eval_loop::{
    pulse_eval_loop_handoff_json, pulse_eval_loop_run_chain_json, pulse_eval_loop_run_once_json,
};
use pulse_run::{pulse_run_chain_json, pulse_run_once_json};
use release_comparison::{
    release_compare, release_compare_verify, release_evidence_bundle_json,
    release_evidence_bundle_verification_json, release_support_bundle_build,
    release_support_bundle_verify,
};
use release_crypto::{
    copy_dir_recursive, derive_public_key_from_private_key, sign_file_with_private_key,
    verify_file_signature,
};
use release_gate::release_gate;
use release_handoff::{
    release_evaluator_decision_build, release_evaluator_decision_markdown,
    release_handoff_checklist_build, release_handoff_checklist_markdown,
};
use release_package::package_release;
use release_provenance::{
    ensure_rsa_private_key, release_sign_provenance, release_verify_provenance,
};
use release_summary::{release_smoke_summary, resolve_cli_artifact_reference};
use release_summary_enrich::release_summary_enrich;
use run_execution::{print_run_summary, run, CliRunOptions};
use run_reporting::{cockpit_index, report, report_verify, runs_list, runs_show};
use upgrade_cmd::{upgrade, UpgradeCommand};
use workbench_memory::{
    workbench_memory_control_plane_dashboard_json, workbench_memory_export_json,
    workbench_memory_link_run_json, workbench_memory_publish_latest_json,
    workbench_memory_recent_json, workbench_memory_search_json,
};
use workbench_release::{
    workbench_release_comparison_json, workbench_release_comparison_verification_json,
    workbench_release_gate_artifact_json, workbench_release_gate_json,
    workbench_release_health_json, workbench_release_history_json,
    workbench_release_retention_prune_json, workbench_release_summary_enrich_json,
};
use workbench_release_latest::workbench_latest_release_comparison_json;
use workbench_run_evidence::{
    workbench_run_evidence_changes_json, workbench_run_evidence_diff_json,
    workbench_run_evidence_summary_json,
};
use workbench_support::{
    workbench_evidence_exports_for_support_bundle, workbench_support_bundle_path,
    workbench_support_bundle_redaction_audit, workbench_support_bundle_redaction_preview,
};

#[derive(Debug, Parser)]
#[command(name = "ao2")]
#[command(about = "AO2 local governed software-delivery runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init {
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Run {
        workflow: Option<PathBuf>,
        #[arg(long)]
        spec: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        pause_for_approval: bool,
        #[arg(long)]
        resume: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long)]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
    },
    Repair {
        #[command(subcommand)]
        command: RepairCommand,
    },
    Support {
        #[command(subcommand)]
        command: support_bundle::SupportCommand,
    },
    Status {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Approve {
        ticket_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long, default_value = "human:local-user")]
        approver: String,
    },
    Adapter {
        #[command(subcommand)]
        command: AdapterCommand,
    },
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    Plugin {
        #[command(subcommand)]
        command: Box<PluginCommand>,
    },
    SkillContractManifest {
        #[command(subcommand)]
        command: SkillContractManifestCommand,
    },
    Template {
        #[command(subcommand)]
        command: TemplateCommand,
    },
    Replay {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Report {
        #[command(subcommand)]
        command: Option<ReportCommand>,
        run_id: Option<String>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    Runs {
        #[command(subcommand)]
        command: RunsCommand,
    },
    Cockpit {
        #[command(subcommand)]
        command: CockpitCommand,
    },
    Pulse {
        #[command(subcommand)]
        command: PulseCommand,
    },
    Workbench {
        #[command(subcommand)]
        command: WorkbenchCommand,
    },
    ControlPlane {
        #[command(subcommand)]
        command: ControlPlaneCommand,
    },
    Contract {
        #[command(subcommand)]
        command: ContractCommand,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    Git {
        #[command(subcommand)]
        command: GitCommand,
    },
    Factory {
        #[command(subcommand)]
        command: FactoryCommand,
    },
    Greenfield {
        #[command(subcommand)]
        command: GreenfieldCommand,
    },
    Sdd {
        #[command(subcommand)]
        command: sdd_cmd::SddCommand,
    },
    Issue {
        #[command(subcommand)]
        command: IssueCommand,
    },
    Export {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Version {
        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        install_dir: Option<PathBuf>,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long)]
        release: Option<String>,
        #[arg(long)]
        release_asset_dir: Option<PathBuf>,
        #[arg(long, default_value = "uesugitorachiyo/ao2")]
        release_repo: String,
    },
    Upgrade {
        #[command(subcommand)]
        command: UpgradeCommand,
    },
    Install {
        #[command(subcommand)]
        command: InstallCommand,
    },
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    Cp {
        #[command(subcommand)]
        command: CpCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CpCommand {
    ProbeExtended {
        #[arg(long, default_value = "http://127.0.0.1:18745")]
        cp_url: String,
        #[arg(long)]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Fetch the release-evidence endpoints from a control-plane and
    /// emit a canonical `ao2.cp-release-snapshot.v1` bundle (per-endpoint
    /// status, schema, body length, body SHA256). Read-only; never
    /// mutates AO artifacts.
    ReleaseSnapshot {
        #[arg(long, default_value = "http://127.0.0.1:18745")]
        cp_url: String,
        #[arg(long)]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ReportCommand {
    Verify {
        #[arg(long)]
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long)]
        index: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum RepairCommand {
    Resume {
        #[arg(long)]
        evidence_pack: PathBuf,
        #[arg(long)]
        workflow: Option<PathBuf>,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long)]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum RunsCommand {
    List {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Show {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CockpitCommand {
    Index {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    Serve {
        run_id: Option<String>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8732)]
        port: u16,
        #[arg(long)]
        index: bool,
        #[arg(long)]
        once: bool,
    },
}

fn parse_bool(s: &str) -> std::result::Result<bool, String> {
    match s.to_lowercase().trim() {
        "true" | "1" | "yes" | "y" | "" => Ok(true),
        "false" | "0" | "no" | "n" => Ok(false),
        other => Err(format!("invalid boolean value: '{}'", other)),
    }
}

#[derive(Debug, Subcommand)]
enum PulseCommand {
    Run {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        chain: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long = "once-evidence")]
        once_evidence: Option<PathBuf>,
        #[arg(long = "chain-evidence")]
        chain_evidence: Option<PathBuf>,
        #[arg(long = "task-contract")]
        task_contract: Option<PathBuf>,
        #[arg(long = "dry-run-task")]
        dry_run_task: bool,
        #[arg(long = "apply-dry-run")]
        apply_dry_run: bool,
        #[arg(long = "dry-run-evidence")]
        dry_run_evidence: Option<PathBuf>,
        #[arg(long = "dry-run-sha256")]
        dry_run_sha256: Option<String>,
        #[arg(long = "apply-root", default_value = ".")]
        apply_root: PathBuf,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/hermes-governed-backend-control-plane/prompt.txt"
        )]
        packet: PathBuf,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/multi-agent-coordination/BOARD.md"
        )]
        board: PathBuf,
        #[arg(long, default_value = "target/ao2-pulse-once")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    EvalLoop {
        #[command(subcommand)]
        command: PulseEvalLoopCommand,
    },
    RunLoop {
        #[arg(long)]
        command: String,
        #[arg(long = "decision-file")]
        decision_file: Option<PathBuf>,
        #[arg(long = "max-chain-runs", default_value_t = 3)]
        max_chain_runs: u32,
        #[arg(long = "max-runtime-seconds", default_value_t = 2700)]
        max_runtime_seconds: u64,
        #[arg(long = "out-dir", default_value = "target/ao2-pulse-run-loop")]
        out_dir: PathBuf,
        #[arg(long = "stdout-fallback")]
        stdout_fallback: bool,
        #[arg(long = "apply-root", default_value = ".")]
        apply_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    AutoAdvance {
        #[arg(
            long,
            env = "AO2_PULSE_RESUME_JSON",
            default_value = ".ao2-local/pulse/latest/resume.json"
        )]
        resume_json: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_ROOT",
            default_value = "target/pulse-auto-advance/latest"
        )]
        out_dir: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_LEDGER",
            default_value = ".ao2-local/pulse/pulse-auto-advance-ledger.jsonl"
        )]
        ledger: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_STOP_FILE",
            default_value = ".ao2-local/pulse/STOP"
        )]
        stop_file: PathBuf,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_MAX_ITERATIONS")]
        max_iterations: Option<u32>,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_ALLOW_DUPLICATE", action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = parse_bool, default_value = "false")]
        allow_duplicate: bool,
        #[arg(long)]
        forever: bool,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_SLEEP_SECONDS",
            default_value = "30"
        )]
        sleep_seconds: u64,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT",
            default_value = "1"
        )]
        generate_next: u32,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT_SLEEP_SECONDS")]
        generate_next_sleep_seconds: Option<u64>,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE", default_value = "1")]
        pr_ci_gate: u32,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE",
            default_value = ".ao2-local/pulse/pr-ci-gate.json"
        )]
        pr_ci_gate_state: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_UPDATE",
            default_value = "1"
        )]
        pr_ci_gate_update: u32,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED", action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = parse_bool, default_value = "false")]
        local_only_while_pr_blocked: bool,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH", action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = parse_bool, default_value = "false")]
        direct_main_publish: bool,
        #[arg(long, default_value = ".")]
        apply_root: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum PulseEvalLoopCommand {
    Run {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        chain: bool,
        #[arg(long = "executor-evidence")]
        executor_evidence: Option<PathBuf>,
        #[arg(long = "executor-sha256")]
        executor_sha256: Option<String>,
        #[arg(long = "eval-loop-evidence")]
        eval_loop_evidence: Option<PathBuf>,
        #[arg(long = "eval-loop-sha256")]
        eval_loop_sha256: Option<String>,
        #[arg(long = "verification-command")]
        verification_command: String,
        #[arg(long = "verification-status")]
        verification_status: String,
        #[arg(long)]
        packet: PathBuf,
        #[arg(long)]
        board: PathBuf,
        #[arg(long, default_value = "target/ao2-pulse-eval-loop")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Handoff {
        #[arg(long = "eval-loop-evidence")]
        eval_loop_evidence: PathBuf,
        #[arg(long = "eval-loop-sha256")]
        eval_loop_sha256: String,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/hermes-governed-backend-control-plane/prompt.txt"
        )]
        packet: PathBuf,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/multi-agent-coordination/BOARD.md"
        )]
        board: PathBuf,
        #[arg(long, default_value = "target/ao2-pulse-eval-loop-handoff")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum WorkbenchCommand {
    Export {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
    },
    Serve {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8732)]
        port: u16,
        #[arg(long)]
        once: bool,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long)]
        api_token: Option<String>,
        #[arg(long = "operator-token")]
        operator_tokens: Vec<String>,
        #[arg(long)]
        enable_execution: bool,
        #[arg(long, default_value_t = 100)]
        queue_retention: usize,
        #[arg(long = "control-plane-url")]
        control_plane_url: Option<String>,
        #[arg(long)]
        support_signing_key: Option<PathBuf>,
        #[arg(long, default_value = "ao2-workbench")]
        support_signer_id: String,
    },
    SupportVerify {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportImport {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long, default_value = "workbench-support-cases")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportInspect {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportKeygen {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 2048)]
        bits: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ControlPlaneCommand {
    Ingest {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Export {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        snapshot: Option<PathBuf>,
        #[arg(long)]
        fleet: Option<PathBuf>,
        #[arg(long)]
        health_history: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    Serve {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        snapshot: Option<PathBuf>,
        #[arg(long)]
        fleet: Option<PathBuf>,
        #[arg(long)]
        health_history: Option<PathBuf>,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8733)]
        port: u16,
        #[arg(long)]
        once: bool,
        #[arg(long)]
        api_token: Option<String>,
    },
    Index {
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        #[arg(long = "snapshot")]
        snapshots: Vec<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Refresh {
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        #[arg(long)]
        sources: Option<PathBuf>,
        #[arg(long)]
        history: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Health {
        #[arg(long)]
        fleet: PathBuf,
        #[arg(long)]
        history: Option<PathBuf>,
        #[arg(long)]
        record: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    HealthTrend {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        json: bool,
    },
    HealthExport {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        json: bool,
    },
    HealthPrune {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        keep: usize,
        #[arg(long)]
        json: bool,
    },
    Sources {
        #[command(subcommand)]
        command: ControlPlaneSourcesCommand,
    },
    History {
        #[command(subcommand)]
        command: ControlPlaneHistoryCommand,
    },
    Bundle {
        #[arg(long)]
        fleet: PathBuf,
        #[arg(long)]
        health_history: Option<PathBuf>,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long)]
        signing_key: Option<PathBuf>,
        #[arg(long, default_value = "local-operator")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    BundleVerify {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    BundleImport {
        #[arg(long)]
        archive: Option<PathBuf>,
        #[arg(long)]
        bundle_dir: Option<PathBuf>,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    BundleInspect {
        #[arg(long)]
        archive: Option<PathBuf>,
        #[arg(long)]
        bundle_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ControlPlaneSourcesCommand {
    Save {
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ControlPlaneHistoryCommand {
    Diff {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        from_index: Option<usize>,
        #[arg(long)]
        to_index: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Prune {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        keep: usize,
        #[arg(long)]
        json: bool,
    },
    Export {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ContractCommand {
    Extract {
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Check {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Gate {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        stage: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
        /// When supplied, also emit an `ao2.workbench-evidence-export.v1`
        /// wrapper alongside the raw gate, sign it with the PEM private key
        /// (RSA/SHA-256), and drop the public key sidecar next to the
        /// wrapper. The wrapper, `.json.sig`, and
        /// `workbench-evidence-signing-public.pem` allow downstream observers
        /// to verify the gate via `ao2 contract verify-obligation-gate-signing`
        /// without operating a workbench HTTP serve loop. Default off
        /// preserves the unsigned legacy path.
        #[arg(long = "support-signing-key")]
        support_signing_key: Option<PathBuf>,
        /// Identifier recorded inside the signed wrapper's audit_event when
        /// `--support-signing-key` is in effect. Defaults to
        /// `ao2-contract-gate`.
        #[arg(long = "support-signer-id", default_value = "ao2-contract-gate")]
        support_signer_id: String,
        /// Operator role recorded inside the signed wrapper's audit_event.
        /// Must be non-empty for the verifier to flag the wrapper as
        /// `ao2_owned`. Defaults to `operator`.
        #[arg(long = "support-operator-role", default_value = "operator")]
        support_operator_role: String,
        /// Run identifier recorded inside the signed wrapper's audit_event.
        /// Defaults to `ao2-contract-gate-cli` (the wrapper is not tied to a
        /// workbench run; this field exists for observability parity with
        /// workbench-emitted wrappers).
        #[arg(long = "support-run-id", default_value = "ao2-contract-gate-cli")]
        support_run_id: String,
        /// Directory the signed wrapper + sidecars are written to when
        /// `--support-signing-key` is in effect. Defaults to the same
        /// directory as `--out` so the verifier finds them via the gate's
        /// parent-dir fallback.
        #[arg(long = "exports-dir")]
        exports_dir: Option<PathBuf>,
        /// Escape valve for legacy producers that have not yet provisioned
        /// an AO2 signing key. As of slice 18, `ao2 contract gate` defaults
        /// to fail-closed when `--support-signing-key` is not provided, so
        /// downstream `ao2 release gate` (slice 11, default-on signing
        /// required) and `/api/release-gate` (slice 17, HTTP default-on)
        /// never accept a silently-unsigned gate. Pass this flag to opt out
        /// and emit an unsigned raw gate only — the downstream release gate
        /// will still reject it unless `--allow-unsigned-obligation-gates`
        /// is also set there. Hidden because the principled path is to
        /// provision a signing key.
        #[arg(long = "allow-unsigned-obligation-gates", hide = true)]
        allow_unsigned_obligation_gates: bool,
    },
    /// Sign an existing raw `ao2.obligation-gate.v1` artifact by emitting the
    /// AO2 workbench evidence-export wrapper and signature sidecars expected
    /// by release gates and control-plane observers.
    SignObligationGate {
        #[arg(long = "gate")]
        gate: PathBuf,
        #[arg(long = "support-signing-key")]
        support_signing_key: PathBuf,
        #[arg(long = "support-signer-id", default_value = "ao2-contract-gate")]
        support_signer_id: String,
        #[arg(long = "support-operator-role", default_value = "operator")]
        support_operator_role: String,
        #[arg(long = "support-run-id", default_value = "ao2-contract-gate-cli")]
        support_run_id: String,
        #[arg(long = "exports-dir")]
        exports_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Annotate {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long = "obligation-id")]
        obligation_id: String,
        #[arg(long = "evidence-path")]
        evidence_path: Option<String>,
        #[arg(long = "evidence-line")]
        evidence_line: Option<usize>,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long)]
        waiver: Option<String>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Audit whether a raw `obligation-gate-<stage>.json` file was AO2-signed
    /// via the workbench evidence-export signing path.
    ///
    /// The closure verdict (and release verification) consumes raw obligation
    /// gate files. AO2 is the only producer that can sign these gates — the
    /// signing wrapper is `ao2.workbench-evidence-export.v1` with a sidecar
    /// `.json.sig` + directory-shared `workbench-evidence-signing-public.pem`.
    /// This subcommand searches `--evidence-exports-dir` for a wrapper whose
    /// embedded gate equals the supplied raw gate, then verifies the
    /// wrapper's RSA/SHA-256 signature. Emits a per-gate verdict observers
    /// (CI, release-gate hooks, control-plane displays) can fail closed on.
    ///
    /// Possible signing_status values:
    ///   - `signed-and-verified`: a matching wrapper exists and its signature
    ///     verifies against the on-disk public key
    ///   - `wrapper-not-found`: no wrapper in the directory embeds a gate
    ///     equal to the supplied raw gate (the gate is unsigned)
    ///   - `signature-missing`: matching wrapper exists but `.json.sig` or
    ///     public key is absent on disk
    ///   - `signature-invalid`: matching wrapper exists with sidecars but
    ///     RSA/SHA-256 verify fails (tampered wrapper, sidecar, or key)
    ///
    /// All factory-v3 paths are excluded — AO2 is the only legitimate signer
    /// for obligation-gate evidence exports; surfacing this is the point of
    /// the audit.
    VerifyObligationGateSigning {
        #[arg(long = "gate")]
        gate: PathBuf,
        #[arg(long = "evidence-exports-dir")]
        evidence_exports_dir: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Inventory every obligation gate observers would consume and audit
    /// each via `contract verify-obligation-gate-signing`. Two scan modes:
    ///
    /// - `--target <PATH>`: walk every raw `obligation-gate-*.json` under
    ///   `<target>/.ao2/runs/*/evidence-pack/` (the in-run gate layout).
    /// - `--summary <PATH>`: walk every `obligation_gates.gates[].path`
    ///   entry inside the supplied release summary JSON (the
    ///   release-gate-consumed gate layout, which may live outside any
    ///   `.ao2/runs/` tree — e.g., free-standing files referenced by a
    ///   factory-v3 nightly enriched summary).
    ///
    /// At least one of `--target` or `--summary` is required. When both
    /// are given, gates seen by both scans are deduplicated by literal
    /// path and the `source` field lists every scan that observed the
    /// gate.
    ///
    /// This is the fleet-migration on-ramp for
    /// `ao2 release gate --require-obligation-gate-signing` (which fails
    /// closed unless every release-summary-embedded gate is
    /// `signed-and-verified`). Operators run this survey first, remediate
    /// the unsigned gates by re-emitting them via
    /// `ao2 workbench obligation-gate --support-signing-key <PEM>`, then
    /// re-run the survey to confirm before flipping the release gate to
    /// signing-required.
    ///
    /// Read-only; exits 0 even when unsigned gates are found so it can be
    /// used in a non-failing inventory cron.
    ObligationGateSigningSurvey {
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        summary: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GitCommand {
    Status {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Diff {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        stat: bool,
        #[arg(long)]
        json: bool,
    },
    Commit {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        message: String,
        #[arg(long = "path")]
        paths: Vec<PathBuf>,
        #[arg(long)]
        approve_action_digest: Option<String>,
        #[arg(long, default_value = "human:local-operator")]
        approver: String,
        #[arg(long)]
        json: bool,
    },
    Tag {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        tag: String,
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        approve_action_digest: Option<String>,
        #[arg(long, default_value = "human:local-operator")]
        approver: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum IssueCommand {
    /// Canonicalize and classify one GitHub issue URL without network or GitHub writes.
    Intake {
        #[arg(long)]
        url: String,
        #[arg(long)]
        json: bool,
    },
    /// Plan isolated repository acquisition for one validated GitHub issue.
    Acquire {
        #[arg(long)]
        url: String,
        #[arg(long = "upstream-url")]
        upstream_url: String,
        #[arg(long = "default-branch", default_value = "main")]
        default_branch: String,
        #[arg(long = "target-commit")]
        target_commit: String,
        #[arg(long)]
        json: bool,
    },
    /// Build, verify, or exercise a bounded local draft pull request action.
    DraftPr {
        #[command(subcommand)]
        command: github_issue_draft::DraftPrCommand,
    },
}

#[derive(Debug, Subcommand)]
enum FactoryCommand {
    Plan {
        #[arg(long = "request")]
        request: PathBuf,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "runspec")]
        runspec: Option<PathBuf>,
        #[arg(long = "role-contract")]
        role_contracts: Vec<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-planner")]
        signer_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long = "plan")]
        plan: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-runner")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    ReplacementSmoke {
        #[arg(long = "request")]
        request: PathBuf,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "runspec")]
        runspec: PathBuf,
        #[arg(long = "role-contract")]
        role_contracts: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-replacement-smoke")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    GovernedRun {
        #[arg(long = "request")]
        request: PathBuf,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "runspec")]
        runspec: PathBuf,
        #[arg(long = "role-contract")]
        role_contracts: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-governed-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    GreenfieldRun {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-greenfield-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    GreenfieldSpecIngest {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        json: bool,
    },
    GreenfieldSpecIngestSubmit {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "approve-action-digest")]
        approve_action_digest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    AppRun {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-app-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    AppRunBundle {
        #[arg(long = "app-run")]
        app_run: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectPlan {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-acceptance-rubric")]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectPlanValidate {
        #[arg(long = "project-plan")]
        project_plan: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    EvaluatorRubric {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-native-evaluator-rubric")]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    CloserDecision {
        #[arg(long = "rubric")]
        rubric: PathBuf,
        #[arg(long = "rubric-sha256")]
        rubric_sha256: String,
        #[arg(long = "evidence")]
        evidence: PathBuf,
        #[arg(long = "evidence-sha256")]
        evidence_sha256: String,
        #[arg(long = "skill-contract-manifest")]
        skill_contract_manifest: PathBuf,
        #[arg(long = "skill-contract-manifest-sha256")]
        skill_contract_manifest_sha256: String,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-native-closer-decision")]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    CloserDecisionVerify {
        #[arg(long = "decision")]
        decision: PathBuf,
        #[arg(long = "decision-sha256")]
        decision_sha256: String,
        #[arg(long)]
        json: bool,
    },
    ProjectStart {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-project-start")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "handoff-bundle-out")]
        handoff_bundle_out: Option<PathBuf>,
        #[arg(long = "handoff-bundle-report")]
        handoff_bundle_report: Option<PathBuf>,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartHermesFlowContract {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartHermesContext {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartBundle {
        #[arg(long = "project-start")]
        project_start: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartBundleVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartSummary {
        #[arg(long = "project-start")]
        project_start: PathBuf,
        #[arg(long = "bundle-verification")]
        bundle_verification: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        markdown: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartClosure {
        #[arg(long = "queue-status")]
        queue_status: PathBuf,
        #[arg(long = "latest-queue-status")]
        latest_queue_status: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartClosureVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ReplacementPacket {
        #[arg(long = "queue-status")]
        queue_status: PathBuf,
        #[arg(long = "latest-queue-status")]
        latest_queue_status: PathBuf,
        #[arg(long)]
        closure: PathBuf,
        #[arg(long = "closure-verification")]
        closure_verification: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "cross-os-readback")]
        cross_os_readbacks: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    ReplacementPacketVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectRun {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-plan")]
        project_plan: Option<PathBuf>,
        #[arg(long = "resume-from")]
        resume_from: Option<PathBuf>,
        #[arg(long = "app-run")]
        app_runs: Vec<PathBuf>,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-project-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectAcceptanceReview {
        #[arg(long = "project-run")]
        project_run: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(
            long = "signer-id",
            default_value = "ao2-factory-project-acceptance-review"
        )]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ReplacementSmokeGate {
        #[arg(long = "smoke")]
        smokes: Vec<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    ReplacementParityStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "governed-run")]
        governed_run: PathBuf,
        #[arg(long = "governed-run-sha256")]
        governed_run_sha256: String,
        #[arg(long = "three-os-gate")]
        three_os_gate: PathBuf,
        #[arg(long = "three-os-gate-sha256")]
        three_os_gate_sha256: String,
        #[arg(long)]
        json: bool,
    },
    VerifyHandoff {
        #[arg(long = "handoff")]
        handoff: PathBuf,
        #[arg(long)]
        json: bool,
    },
    VerifyRunResult {
        #[arg(long = "run-result")]
        run_result: PathBuf,
        #[arg(long)]
        json: bool,
    },
    VerifyPlanningEvidence {
        #[arg(long = "evidence")]
        evidence: PathBuf,
        #[arg(long = "signed-payload")]
        signed_payload: Option<PathBuf>,
        #[arg(long = "signature")]
        signature: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    VerifyEvaluatorDecision {
        #[arg(long = "decision")]
        decision: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Evaluate {
        #[arg(long = "evidence-pack")]
        evidence_pack: PathBuf,
        #[arg(long = "report")]
        report: Option<PathBuf>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-native-evaluator-closer")]
        signer_id: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueSubmit {
        #[arg(long = "plan")]
        plan: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueSubmitProjectStart {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-project-start-queue")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: Option<PathBuf>,
        #[arg(long = "handoff-bundle-out")]
        handoff_bundle_out: Option<PathBuf>,
        #[arg(long = "handoff-bundle-report")]
        handoff_bundle_report: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartComplete {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(
            long = "signer-id",
            default_value = "ao2-factory-project-start-queue-complete"
        )]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long = "handoff-bundle-out")]
        handoff_bundle_out: Option<PathBuf>,
        #[arg(long = "handoff-bundle-report")]
        handoff_bundle_report: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompleteStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompletionSummary {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompletionSummaryMemory {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "approve-action-digest")]
        approve_action_digest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompletionSummaryMemoryStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecovery {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartLatestRecovery {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeReceipt {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeCheckpoint {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "approve-action-digest")]
        approve_action_digest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeCheckpointStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinuity {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePlan {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeClaim {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "approve-plan-sha256")]
        approve_plan_sha256: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeClaimStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinuationContract {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinue {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "approve-claim-status-sha256")]
        approve_claim_status_sha256: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinuationStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationExecute {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "approve-continuation-status-sha256")]
        approve_continuation_status_sha256: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationExecutionStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationNextAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "post-continuation-execution-status-sha256")]
        post_continuation_execution_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationClosure {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "post-continuation-execution-status-sha256")]
        post_continuation_execution_status_sha256: String,
        #[arg(long = "post-continuation-next-action-sha256")]
        post_continuation_next_action_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationEvaluatorDecision {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "post-continuation-execution-status-sha256")]
        post_continuation_execution_status_sha256: String,
        #[arg(long = "post-continuation-next-action-sha256")]
        post_continuation_next_action_sha256: String,
        #[arg(long = "closure-sha256")]
        closure_sha256: String,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-recovery-closure-evaluator")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoff {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        decision: PathBuf,
        #[arg(long = "signed-payload")]
        signed_payload: PathBuf,
        #[arg(long)]
        signature: PathBuf,
        #[arg(long = "public-key")]
        public_key: PathBuf,
        #[arg(long = "closure-sha256")]
        closure_sha256: String,
        #[arg(long = "decision-sha256")]
        decision_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long = "closure-sha256")]
        closure_sha256: String,
        #[arg(long = "decision-sha256")]
        decision_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatusSummary {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        status: PathBuf,
        #[arg(long = "status-sha256")]
        status_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatusSummaryExport {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationReadiness {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        export: PathBuf,
        #[arg(long = "export-sha256")]
        export_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationDispatchPlan {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long = "readiness-sha256")]
        readiness_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationReadback {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "dispatch-plan")]
        dispatch_plan: PathBuf,
        #[arg(long = "dispatch-plan-sha256")]
        dispatch_plan_sha256: String,
        #[arg(long)]
        observation: PathBuf,
        #[arg(long = "observation-sha256")]
        observation_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationClosure {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        readback: PathBuf,
        #[arg(long = "readback-sha256")]
        readback_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartNextAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartPublishOperatorRecord {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[arg(long = "record-out")]
        record_out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueList {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "latest-completed-project-start")]
        latest_completed_project_start: bool,
        #[arg(long)]
        json: bool,
    },
    QueueCompletionContract {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "latest-completed-project-start")]
        latest_completed_project_start: bool,
        #[arg(long)]
        json: bool,
    },
    QueueCompletionContractConsume {
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit a factory-v3/ao2-watchdog-no-active-ao2-runs-attestation/v1
    /// payload from a captured `ao2 factory queue-list --json` snapshot.
    /// AO2-native producer for Phase 2 exit-gate item #5 (cancel
    /// semantics owned by AO2).
    CancelAuthority {
        #[arg(long = "queue-list-json")]
        queue_list_json: PathBuf,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "produced-at-ms")]
        produced_at_ms: Option<i64>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Emit an `ao2.factory-v3-compat-workbench-queue-transition.v1`
    /// payload that binds a recorded cancellation in an `ao2 factory
    /// queue-list --json` snapshot to a specific pid the watchdog
    /// terminated. AO2-native producer for the second half of Phase 2
    /// exit-gate item #5 (cancel-transition counterpart to
    /// `cancel-authority`).
    CancelTransition {
        #[arg(long = "queue-list-json")]
        queue_list_json: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "terminated-pid")]
        terminated_pid: i64,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "produced-at-ms")]
        produced_at_ms: Option<i64>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueCancel {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueRetry {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueRunNext {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-queue-runner")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    PackEvidence {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-pack-evidence-signer")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Phase 2 exit-gate single-command bridge: canonicalize every role id in
    /// an AO Operator RunSpec via the deterministic AO Operator -> AO2
    /// provider-contract mapping, then emit an AO2-native bridge-evidence
    /// JSON. Mapping-only dry-run; no ao2 invocation is shelled out. The
    /// `factory-v3` Python bridge can defer to this subcommand via its
    /// `--ao2-native-passthrough` mode (Phase 2 exit-gate items #1 and #2,
    /// AO2-native producer).
    Bridge {
        #[arg(long = "runspec")]
        runspec: PathBuf,
        #[arg(long = "work-request")]
        work_request: Option<PathBuf>,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "role-contracts-dir")]
        role_contracts_dir: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-bridge")]
        signer_id: String,
        #[arg(
            long = "now-ms",
            help = "override timestamp (millis) for deterministic tests"
        )]
        now_ms: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Emit the canonicalized AO Operator -> AO2 provider-contract mapping
    /// table or its sha256 digest. Useful for parity checks against the
    /// factory-v3 Python module that originally owned this mapping.
    BridgeMapping {
        #[arg(long)]
        digest: bool,
    },
    /// Verify a signed AO2-native bridge evidence file end-to-end.
    ///
    /// Resolves the signed-payload, signature, and public-key sidecars from
    /// the `signature` block embedded in the evidence body by default; any of
    /// the three can be overridden via the matching `--signed-payload`,
    /// `--signature`, or `--public-key` flags. Runs four independent integrity
    /// checks before emitting a verdict:
    ///   1. every sha256 recorded in the body's `signature` block matches the
    ///      bytes on disk for that file;
    ///   2. the canonical body minus the `signature` field equals the signed
    ///      payload bytes (catches body tampering even when only the sidecar
    ///      is forged);
    ///   3. RSA/SHA-256 cryptographic verify of the signature against the
    ///      signed-payload bytes with the supplied public key;
    ///   4. the body's trust boundary still names AO2 as the bridge owner.
    ///
    /// All four must hold for `status: accepted`. AO2-owned verifier surface;
    /// factory-v3 and other observers can shell out to this to confirm
    /// bridge evidence without re-implementing the signature pattern.
    VerifyBridgeEvidence {
        #[arg(long = "evidence")]
        evidence: PathBuf,
        #[arg(long = "signed-payload")]
        signed_payload: Option<PathBuf>,
        #[arg(long = "signature")]
        signature: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GreenfieldCommand {
    Ingest {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-greenfield-ingest")]
        signer_id: String,
        #[arg(long = "out-dir")]
        out_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    GovernedRun {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-greenfield-governed-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ThreeOsSmokeGate {
        #[arg(long = "smoke")]
        smokes: Vec<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    Package {
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
        version: String,
        #[arg(long)]
        binary: Option<PathBuf>,
        #[arg(long)]
        target_label: Option<String>,
    },
    SmokeSummary {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long)]
        require_native_windows: bool,
    },
    SummaryEnrich {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long = "obligation-gate")]
        obligation_gates: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Gate {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long)]
        provenance_dir: PathBuf,
        #[arg(long)]
        macos_archive: Option<PathBuf>,
        #[arg(long)]
        linux_archive: PathBuf,
        #[arg(long = "linux-x86-64-archive")]
        linux_x86_64_archive: PathBuf,
        #[arg(long)]
        windows_archive: PathBuf,
        #[arg(long)]
        require_native_windows: bool,
        #[arg(long = "replacement-smoke-gate")]
        replacement_smoke_gate: Option<PathBuf>,
        #[arg(long = "greenfield-three-os-smoke-gate")]
        greenfield_three_os_smoke_gate: Option<PathBuf>,
        #[arg(long = "governed-run-evidence")]
        governed_run_evidence: Vec<PathBuf>,
        #[arg(long = "factory-project-run-summary")]
        factory_project_run_summaries: Vec<PathBuf>,
        /// Opt out of the default-on obligation-gate signing requirement.
        /// When set, the release gate skips the AO2 workbench
        /// evidence-export wrapper audit and omits the
        /// `obligation_gate_signing` block from the report. Intended for
        /// legacy callers that have not yet provisioned a signing key.
        #[arg(long = "allow-unsigned-obligation-gates")]
        allow_unsigned_obligation_gates: bool,
        /// Back-compat: accepted but no-op now that obligation-gate
        /// signing is required by default. Kept so existing scripts
        /// that explicitly opt in continue to parse.
        #[arg(long = "require-obligation-gate-signing", hide = true)]
        require_obligation_gate_signing: bool,
    },
    Compare {
        #[arg(long, default_value = "target/release-download")]
        release_download_dir: PathBuf,
        #[arg(long, default_value = "target/release-comparison-bundles")]
        out_dir: PathBuf,
        #[arg(long)]
        signing_key: Option<PathBuf>,
        #[arg(long, default_value = "ao2-release-operator")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    CompareVerify {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportBundleBuild {
        #[arg(long = "release-assembly")]
        release_assembly: PathBuf,
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long)]
        cockpit: PathBuf,
        #[arg(long = "evaluator-decision")]
        evaluator_decision: PathBuf,
        #[arg(long = "storage-support")]
        storage_support: PathBuf,
        #[arg(long)]
        replay: PathBuf,
        #[arg(long = "report-contract-verification")]
        report_contract_verification: Option<PathBuf>,
        #[arg(long = "install-verification")]
        install_verification: PathBuf,
        #[arg(long = "hosted-release-smoke")]
        hosted_release_smoke: PathBuf,
        #[arg(long = "report-target")]
        report_target: Option<PathBuf>,
        #[arg(long = "report-run-id")]
        report_run_id: Option<String>,
        #[arg(long = "report")]
        report: Option<PathBuf>,
        #[arg(long = "report-index")]
        report_index: Option<PathBuf>,
        #[arg(long = "operator-evidence")]
        operator_evidence: PathBuf,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportBundleVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        checksums: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    EvidenceBundle {
        #[arg(long)]
        out_dir: PathBuf,
        /// Artifact to include, in `<label>=<path>` form. Repeat for
        /// readiness, handoff, evaluator-decision, three-OS smoke,
        /// provider-acceptance, or other locally produced release evidence.
        #[arg(long = "artifact")]
        artifacts: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    EvidenceBundleVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Phase1DecisionBuild {
        #[arg(long = "release-gate")]
        release_gate: PathBuf,
        #[arg(long = "replacement-smoke-gate")]
        replacement_smoke_gate: Option<PathBuf>,
        #[arg(long = "governed-run-evidence")]
        governed_run_evidence: Vec<PathBuf>,
        #[arg(long = "factory-project-run-summary")]
        factory_project_run_summaries: Vec<PathBuf>,
        #[arg(long = "provider-acceptance-preservation")]
        provider_acceptance_preservation: Option<PathBuf>,
        #[arg(long)]
        operator: String,
        #[arg(long)]
        rationale: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "checklist-out")]
        checklist_out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Phase1DecisionPublish {
        #[arg(long)]
        decision: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-phase1-release")]
        signer_id: String,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Phase1ThreeOsSmokeBuild {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long)]
        provenance: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Phase1ThreeOsSmokePublish {
        #[arg(long)]
        smoke: PathBuf,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Phase1HistoryFetch {
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Phase1PromotionStatus {
        #[arg(long)]
        root: PathBuf,
        #[arg(long = "evidence-bundle")]
        evidence_bundle: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Phase1PromotionInputsVerify {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "preflight")]
        mode: String,
        #[arg(long)]
        json: bool,
    },
    Phase1PromotionInputsPublish {
        #[arg(long)]
        verification: PathBuf,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// AO2-native producer of `factory-v3/ao2-release-evaluator-decision/v1`.
    /// Designed for byte-equal parity (under canonical JSON serialisation)
    /// with `factory-v3/scripts/ao2_release_evaluator_decision.py`. The
    /// factory-v3 Python remains in a read-only audit role; AO2 is the
    /// canonical producer.
    EvaluatorDecisionBuild {
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long = "handoff-checklist")]
        handoff_checklist: PathBuf,
        #[arg(long = "support-bundle-status")]
        support_bundle_status: PathBuf,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long = "write-md")]
        write_md: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// AO2-native producer of `factory-v3/ao2-release-handoff-checklist/v1`.
    /// Byte-equal parity (under canonical JSON sort) with
    /// `factory-v3/scripts/ao2_release_handoff_checklist.py`. factory-v3
    /// remains in a read-only audit role.
    HandoffChecklistBuild {
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long = "write-md")]
        write_md: Option<PathBuf>,
        /// Expected repository HEAD in `<repo>=<head>` form. Repeat to
        /// pin multiple repositories.
        #[arg(long = "expected-repo-head")]
        expected_repo_head: Vec<String>,
        /// Allow input that does not contain an AO2 release-candidate
        /// handoff to produce a `planned`/`skipped` checklist instead of
        /// failing.
        #[arg(long = "allow-skipped")]
        allow_skipped: bool,
        #[arg(long)]
        json: bool,
    },
    SignProvenance {
        #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
        version: String,
        #[arg(long)]
        macos_archive: Option<PathBuf>,
        #[arg(long)]
        linux_archive: PathBuf,
        #[arg(long = "linux-x86-64-archive")]
        linux_x86_64_archive: PathBuf,
        #[arg(long)]
        windows_archive: PathBuf,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long, default_value = ".release-signing/ao2-release-signing-key.pem")]
        private_key: PathBuf,
        #[arg(long)]
        release_tag: Option<String>,
        #[arg(long)]
        json: bool,
    },
    VerifyProvenance {
        #[arg(long)]
        macos_archive: Option<PathBuf>,
        #[arg(long)]
        linux_archive: PathBuf,
        #[arg(long = "linux-x86-64-archive")]
        linux_x86_64_archive: PathBuf,
        #[arg(long)]
        windows_archive: PathBuf,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long)]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum TemplateCommand {
    List,
    Show { name: String },
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    List,
    Registry {
        #[arg(long = "control-plane-url")]
        control_plane_url: Option<String>,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-provider-registry")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(long, default_value = "scripted")]
        provider: String,
    },
    Matrix {
        #[arg(long)]
        json: bool,
    },
    Contract {
        #[arg(long, default_value = "scripted")]
        provider: String,
        #[arg(long)]
        verify: bool,
        #[arg(long = "require")]
        require: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    SmokeAll {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 90)]
        minimum_score: u64,
        #[arg(long = "live-provider")]
        live_provider: Vec<String>,
    },
    Gate {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "require")]
        require: Vec<String>,
        #[arg(long, default_value_t = 90)]
        minimum_score: u64,
        #[arg(long)]
        json: bool,
    },
    Pilot {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        provider: String,
        #[arg(long, default_value = "bug-fix")]
        template: String,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: PathBuf,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long, default_value_t = 90)]
        minimum_score: u64,
        #[arg(long)]
        json: bool,
    },
    CostLedger {
        #[arg(long, default_value = "target/provider-pilot-acceptance")]
        acceptance_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    CostTrend {
        #[arg(long, default_value = "target/provider-pilot-acceptance")]
        acceptance_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Score {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum PluginCommand {
    /// Emit the Codex/Claude wrapper readiness contract for AO2 production
    /// plugin integrations. This command is read-only and produces no provider
    /// calls, queue writes, memory writes, or AO artifact mutations.
    Readiness {
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Package a deterministic Codex/Claude plugin manifest with wrapper
    /// schema examples and local OAuth CLI-only smoke fixtures.
    Manifest {
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a packaged Codex/Claude plugin manifest by exact digest, file
    /// SHA256 metadata, local OAuth posture, and observer-only trust boundary.
    ManifestVerify {
        #[arg(long = "manifest-dir")]
        manifest_dir: PathBuf,
        #[arg(long = "manifest-sha256")]
        manifest_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Dry-run Codex/Claude plugin setup from a digest-pinned manifest
    /// verification artifact without executing providers or mutating queues.
    InstallSmoke {
        #[arg(long = "manifest-dir")]
        manifest_dir: PathBuf,
        #[arg(long)]
        verification: PathBuf,
        #[arg(long = "verification-sha256")]
        verification_sha256: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Bundle a verified Codex/Claude plugin manifest and install-smoke proof
    /// into one digest-addressed package for local wrapper distribution.
    Package {
        #[arg(long = "manifest-dir")]
        manifest_dir: PathBuf,
        #[arg(long = "manifest-verification")]
        manifest_verification: PathBuf,
        #[arg(long = "manifest-verification-sha256")]
        manifest_verification_sha256: String,
        #[arg(long = "install-smoke")]
        install_smoke: PathBuf,
        #[arg(long = "install-smoke-sha256")]
        install_smoke_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Codex/Claude plugin package summary and archive
    /// after distribution without executing providers or mutating queues.
    PackageVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Install a verified plugin package into clean Codex/Claude wrapper
    /// fixture directories and run the consumer lifecycle from installed paths.
    DistributionRehearsal {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify clean Codex/Claude wrapper sandboxes from installed package and
    /// adapter paths without provider execution or mutable side effects.
    ConsumerLifecycle {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "adapter-scaffold")]
        adapter_scaffold: PathBuf,
        #[arg(long = "adapter-scaffold-sha256")]
        adapter_scaffold_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Write a digest-pinned Windows PowerShell runner for the consumer
    /// lifecycle proof so recovery needs one command session after transfer.
    ConsumerLifecycleWindowsRecovery {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "adapter-scaffold")]
        adapter_scaffold: PathBuf,
        #[arg(long = "adapter-scaffold-sha256")]
        adapter_scaffold_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows consumer-lifecycle proofs into one
    /// digest-bound K37 read-only observer bundle.
    ConsumerLifecycleObserverBundle {
        #[arg(long = "macos-lifecycle")]
        macos_lifecycle: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-lifecycle")]
        ubuntu_lifecycle: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-lifecycle")]
        windows_lifecycle: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned consumer-lifecycle observer bundle after
    /// distribution without executing providers or mutating queues.
    ConsumerLifecycleObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Aggregate digest-pinned plugin shipment evidence into one local release
    /// candidate proof for factory-v3 parity audit and read-only observation.
    ReleaseCandidate {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "distribution-rehearsal")]
        distribution_rehearsal: PathBuf,
        #[arg(long = "distribution-rehearsal-sha256")]
        distribution_rehearsal_sha256: String,
        #[arg(long = "adapter-observer-bundle")]
        adapter_observer_bundle: PathBuf,
        #[arg(long = "adapter-observer-bundle-sha256")]
        adapter_observer_bundle_sha256: String,
        #[arg(long = "adapter-observer-archive")]
        adapter_observer_archive: PathBuf,
        #[arg(long = "adapter-observer-archive-sha256")]
        adapter_observer_archive_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-bundle")]
        adapter_install_smoke_observer_bundle: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-bundle-sha256")]
        adapter_install_smoke_observer_bundle_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-archive")]
        adapter_install_smoke_observer_archive: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-archive-sha256")]
        adapter_install_smoke_observer_archive_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-bundle")]
        consumer_lifecycle_observer_bundle: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-bundle-sha256")]
        consumer_lifecycle_observer_bundle_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-archive")]
        consumer_lifecycle_observer_archive: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-archive-sha256")]
        consumer_lifecycle_observer_archive_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-bundle")]
        release_gate_with_replacement_observer_bundle: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-bundle-sha256")]
        release_gate_with_replacement_observer_bundle_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-archive")]
        release_gate_with_replacement_observer_archive: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-archive-sha256")]
        release_gate_with_replacement_observer_archive_sha256: String,
        #[arg(long = "control-plane-fixture-handoff-verification")]
        control_plane_fixture_handoff_verification: PathBuf,
        #[arg(long = "control-plane-fixture-handoff-verification-sha256")]
        control_plane_fixture_handoff_verification_sha256: String,
        #[arg(long = "control-plane-readback-commit")]
        control_plane_readback_commit: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned plugin release-candidate summary after
    /// distribution without executing providers or mutating queues.
    ReleaseCandidateVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Write a digest-pinned Windows PowerShell runner for release-candidate
    /// proof recovery so the next attempt needs one command session.
    ReleaseCandidateWindowsRecovery {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "distribution-rehearsal")]
        distribution_rehearsal: PathBuf,
        #[arg(long = "distribution-rehearsal-sha256")]
        distribution_rehearsal_sha256: String,
        #[arg(long = "adapter-observer-bundle")]
        adapter_observer_bundle: PathBuf,
        #[arg(long = "adapter-observer-bundle-sha256")]
        adapter_observer_bundle_sha256: String,
        #[arg(long = "adapter-observer-archive")]
        adapter_observer_archive: PathBuf,
        #[arg(long = "adapter-observer-archive-sha256")]
        adapter_observer_archive_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-bundle")]
        adapter_install_smoke_observer_bundle: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-bundle-sha256")]
        adapter_install_smoke_observer_bundle_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-archive")]
        adapter_install_smoke_observer_archive: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-archive-sha256")]
        adapter_install_smoke_observer_archive_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-bundle")]
        consumer_lifecycle_observer_bundle: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-bundle-sha256")]
        consumer_lifecycle_observer_bundle_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-archive")]
        consumer_lifecycle_observer_archive: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-archive-sha256")]
        consumer_lifecycle_observer_archive_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-bundle")]
        release_gate_with_replacement_observer_bundle: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-bundle-sha256")]
        release_gate_with_replacement_observer_bundle_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-archive")]
        release_gate_with_replacement_observer_archive: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-archive-sha256")]
        release_gate_with_replacement_observer_archive_sha256: String,
        #[arg(long = "control-plane-fixture-handoff-verification")]
        control_plane_fixture_handoff_verification: PathBuf,
        #[arg(long = "control-plane-fixture-handoff-verification-sha256")]
        control_plane_fixture_handoff_verification_sha256: String,
        #[arg(long = "control-plane-readback-commit")]
        control_plane_readback_commit: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Windows release-candidate recovery package before
    /// transferring it to an unreliable Windows SSH host.
    ReleaseCandidateWindowsRecoveryVerify {
        #[arg(long)]
        recovery: PathBuf,
        #[arg(long = "recovery-sha256")]
        recovery_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package the verified Windows recovery runner and AO2 source archive
    /// into one digest-bound transfer bundle for a single Windows session.
    ReleaseCandidateWindowsTransferBundle {
        #[arg(long = "ao2-source-archive")]
        ao2_source_archive: PathBuf,
        #[arg(long = "ao2-source-archive-sha256")]
        ao2_source_archive_sha256: String,
        #[arg(long = "recovery-dir")]
        recovery_dir: PathBuf,
        #[arg(long)]
        recovery: PathBuf,
        #[arg(long = "recovery-sha256")]
        recovery_sha256: String,
        #[arg(long = "recovery-verification")]
        recovery_verification: PathBuf,
        #[arg(long = "recovery-verification-sha256")]
        recovery_verification_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows release-candidate verification
    /// proofs into one digest-bound K37 read-only observer bundle.
    ReleaseCandidateObserverBundle {
        #[arg(long = "macos-verification")]
        macos_verification: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-verification")]
        ubuntu_verification: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-verification")]
        windows_verification: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned release-candidate observer bundle after
    /// distribution without executing providers or mutating queues.
    ReleaseCandidateObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse apply-result proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseApplyObserverBundle {
        #[arg(long = "macos-apply-result")]
        macos_apply_result: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-apply-result")]
        ubuntu_apply_result: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-apply-result")]
        windows_apply_result: Option<PathBuf>,
        #[arg(long = "windows-sha256")]
        windows_sha256: Option<String>,
        #[arg(long = "windows-unavailable-reason")]
        windows_unavailable_reason: Option<String>,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse apply-result observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseApplyObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse once-mode proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseOnceObserverBundle {
        #[arg(long = "macos-once")]
        macos_once: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-once")]
        ubuntu_once: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-once")]
        windows_once: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse once-mode observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseOnceObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse chain-mode proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseChainObserverBundle {
        #[arg(long = "macos-chain")]
        macos_chain: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-chain")]
        ubuntu_chain: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-chain")]
        windows_chain: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse chain-mode observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseChainObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse eval-loop proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseEvalLoopObserverBundle {
        #[arg(long = "macos-eval-loop")]
        macos_eval_loop: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-eval-loop")]
        ubuntu_eval_loop: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-eval-loop")]
        windows_eval_loop: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse eval-loop observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseEvalLoopObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse executor, governed-task, and task-result proofs into one
    /// digest-bound K37 read-only observer bundle.
    PulseExecutorObserverBundle {
        #[arg(long = "macos-executor")]
        macos_executor: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-executor")]
        ubuntu_executor: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-executor")]
        windows_executor: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse executor observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseExecutorObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Prepare a digest-pinned Windows PowerShell runner for Pulse apply-result
    /// and K37 observer-bundle proof recovery.
    PulseApplyWindowsRecovery {
        #[arg(long = "apply-result")]
        apply_result: PathBuf,
        #[arg(long = "apply-result-sha256")]
        apply_result_sha256: String,
        #[arg(long = "observer-bundle")]
        observer_bundle: PathBuf,
        #[arg(long = "observer-bundle-sha256")]
        observer_bundle_sha256: String,
        #[arg(long = "observer-archive")]
        observer_archive: PathBuf,
        #[arg(long = "observer-archive-sha256")]
        observer_archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Prepare a digest-pinned release-candidate fixture handoff for
    /// ao2-control-plane read-only observation without editing control-plane
    /// source files.
    ReleaseCandidateControlPlaneFixtureHandoff {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned release-candidate control-plane fixture handoff
    /// after distribution without editing or mutating control-plane files.
    ReleaseCandidateControlPlaneFixtureHandoffVerify {
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long = "handoff-sha256")]
        handoff_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Produce a final digest-pinned Codex/Claude install transcript from a
    /// release-candidate observer bundle without executing providers.
    FinalInstallTranscript {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows Codex/Claude final install
    /// transcripts into one digest-bound K37 read-only observer bundle.
    FinalInstallTranscriptObserverBundle {
        #[arg(long = "macos-codex-transcript")]
        macos_codex_transcript: PathBuf,
        #[arg(long = "macos-codex-sha256")]
        macos_codex_sha256: String,
        #[arg(long = "macos-claude-transcript")]
        macos_claude_transcript: PathBuf,
        #[arg(long = "macos-claude-sha256")]
        macos_claude_sha256: String,
        #[arg(long = "ubuntu-codex-transcript")]
        ubuntu_codex_transcript: PathBuf,
        #[arg(long = "ubuntu-codex-sha256")]
        ubuntu_codex_sha256: String,
        #[arg(long = "ubuntu-claude-transcript")]
        ubuntu_claude_transcript: PathBuf,
        #[arg(long = "ubuntu-claude-sha256")]
        ubuntu_claude_sha256: String,
        #[arg(long = "windows-codex-transcript")]
        windows_codex_transcript: PathBuf,
        #[arg(long = "windows-codex-sha256")]
        windows_codex_sha256: String,
        #[arg(long = "windows-claude-transcript")]
        windows_claude_transcript: PathBuf,
        #[arg(long = "windows-claude-sha256")]
        windows_claude_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Aggregate digest-pinned plugin shipment evidence into one final
    /// operator-facing readiness handoff for Codex/Claude plugin release.
    ShipmentReadiness {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "adapter-observer-bundle")]
        adapter_observer_bundle: PathBuf,
        #[arg(long = "adapter-observer-bundle-sha256")]
        adapter_observer_bundle_sha256: String,
        #[arg(long = "adapter-observer-archive")]
        adapter_observer_archive: PathBuf,
        #[arg(long = "adapter-observer-archive-sha256")]
        adapter_observer_archive_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-bundle")]
        adapter_install_smoke_observer_bundle: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-bundle-sha256")]
        adapter_install_smoke_observer_bundle_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-archive")]
        adapter_install_smoke_observer_archive: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-archive-sha256")]
        adapter_install_smoke_observer_archive_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-bundle")]
        consumer_lifecycle_observer_bundle: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-bundle-sha256")]
        consumer_lifecycle_observer_bundle_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-archive")]
        consumer_lifecycle_observer_archive: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-archive-sha256")]
        consumer_lifecycle_observer_archive_sha256: String,
        #[arg(long = "release-candidate-observer-bundle")]
        release_candidate_observer_bundle: PathBuf,
        #[arg(long = "release-candidate-observer-bundle-sha256")]
        release_candidate_observer_bundle_sha256: String,
        #[arg(long = "release-candidate-observer-archive")]
        release_candidate_observer_archive: PathBuf,
        #[arg(long = "release-candidate-observer-archive-sha256")]
        release_candidate_observer_archive_sha256: String,
        #[arg(long = "final-install-transcript-observer-bundle")]
        final_install_transcript_observer_bundle: PathBuf,
        #[arg(long = "final-install-transcript-observer-bundle-sha256")]
        final_install_transcript_observer_bundle_sha256: String,
        #[arg(long = "final-install-transcript-observer-archive")]
        final_install_transcript_observer_archive: PathBuf,
        #[arg(long = "final-install-transcript-observer-archive-sha256")]
        final_install_transcript_observer_archive_sha256: String,
        #[arg(long = "control-plane-readback-commit")]
        control_plane_readback_commit: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Prepare a digest-pinned fixture handoff for ao2-control-plane read-only
    /// observation without editing control-plane source files.
    ControlPlaneFixtureHandoff {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned ao2-control-plane fixture handoff after
    /// distribution without editing or mutating control-plane files.
    ControlPlaneFixtureHandoffVerify {
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long = "handoff-sha256")]
        handoff_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows K37 observer inputs into one
    /// digest-bound bundle for read-only control-plane observation.
    DistributionObserverBundle {
        #[arg(long = "macos-observer")]
        macos_observer: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-observer")]
        ubuntu_observer: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-observer")]
        windows_observer: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows clean packaged-plugin rehearsal
    /// proofs into one digest-bound K37 read-only operator index.
    CleanPackageOperatorIndex {
        #[arg(long = "macos-rehearsal")]
        macos_rehearsal: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-rehearsal")]
        ubuntu_rehearsal: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-rehearsal")]
        windows_rehearsal: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows packaged-replacement hardening
    /// proofs into one digest-bound K37 read-only observer bundle.
    PackagedReplacementObserverBundle {
        #[arg(long = "macos-proof")]
        macos_proof: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-proof")]
        ubuntu_proof: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-proof")]
        windows_proof: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned packaged-replacement observer bundle after
    /// distribution without executing providers or mutating queues.
    PackagedReplacementObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows release-gate-with-replacement
    /// rollups into one digest-bound K37 read-only observer bundle.
    ReleaseGateWithReplacementObserverBundle {
        #[arg(long = "macos-rollup")]
        macos_rollup: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-rollup")]
        ubuntu_rollup: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-rollup")]
        windows_rollup: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Generate Codex/Claude local OAuth wrapper adapter scaffolds from a
    /// digest-pinned plugin package and K37 observer bundle.
    AdapterScaffold {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "k37-bundle")]
        k37_bundle: PathBuf,
        #[arg(long = "k37-bundle-sha256")]
        k37_bundle_sha256: String,
        #[arg(long = "k37-archive")]
        k37_archive: PathBuf,
        #[arg(long = "k37-archive-sha256")]
        k37_archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a generated Codex/Claude adapter scaffold by exact digest,
    /// adapter file SHA256s, local OAuth posture, and observer-only boundaries.
    AdapterScaffoldVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Dry-run Codex/Claude adapter installation from a digest-pinned scaffold
    /// without executing providers, mutating queues, or writing memory.
    AdapterInstallSmoke {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Codex/Claude adapter install-smoke artifact
    /// after distribution without executing providers or mutating queues.
    AdapterInstallSmokeVerify {
        #[arg(long)]
        smoke: PathBuf,
        #[arg(long = "smoke-sha256")]
        smoke_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows adapter install-smoke verification
    /// artifacts into one digest-bound K37 read-only observer bundle.
    AdapterInstallSmokeObserverBundle {
        #[arg(long = "macos-verification")]
        macos_verification: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-verification")]
        ubuntu_verification: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-verification")]
        windows_verification: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows adapter scaffold verification
    /// artifacts into one digest-bound K37 read-only observer bundle.
    AdapterObserverBundle {
        #[arg(long = "macos-verification")]
        macos_verification: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-verification")]
        ubuntu_verification: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-verification")]
        windows_verification: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Execute a digest-pinned AO2 factory app-run/project-run through the
    /// plugin wrapper contract and persist token-safe evidence for audit.
    WrapperHarness {
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long = "readiness-sha256")]
        readiness_sha256: String,
        #[arg(long = "args-file")]
        args_file: PathBuf,
        #[arg(long = "args-sha256")]
        args_sha256: String,
        #[arg(long = "run-kind")]
        run_kind: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify an existing plugin wrapper-harness evidence directory by digest.
    WrapperHarnessVerify {
        #[arg(long = "evidence-dir")]
        evidence_dir: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SkillContractManifestCommand {
    /// Generate the AO2-produced factory-v3 skill/contract migration manifest.
    Generate {
        #[arg(long = "factory-v3-root", default_value = "../factory-v3")]
        factory_v3_root: PathBuf,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned skill/contract migration manifest guardrail.
    Verify {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long = "manifest-sha256")]
        manifest_sha256: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AdapterCommand {
    Doctor {
        #[arg(long, default_value = "scripted")]
        provider: String,
    },
    Run {
        #[arg(long, default_value = "scripted")]
        provider: String,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        command: PathBuf,
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        args: String,
        #[arg(long, default_value = "adapter-cli")]
        role_id: String,
        #[arg(long)]
        keep_sandbox: bool,
        #[arg(long, default_value_t = DEFAULT_PROVIDER_TIMEOUT_SECONDS)]
        timeout_seconds: u64,
    },
    Prompt {
        #[arg(long, default_value = "scripted")]
        provider: String,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        prompt_file: Option<PathBuf>,
        #[arg(long, default_value = "adapter-prompt")]
        role_id: String,
        #[arg(long)]
        keep_sandbox: bool,
        #[arg(long, default_value_t = DEFAULT_PROVIDER_TIMEOUT_SECONDS)]
        timeout_seconds: u64,
        #[arg(long = "max-budget-usd")]
        max_budget_usd: Option<f64>,
    },
    Patch {
        #[command(subcommand)]
        command: AdapterPatchCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AdapterPatchCommand {
    Preview {
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        sandbox: PathBuf,
    },
    Apply {
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        sandbox: PathBuf,
        #[arg(long)]
        digest: String,
        #[arg(long, default_value = "human:local-user")]
        approver: String,
    },
}

fn main() -> Result<()> {
    std::thread::Builder::new()
        .name("ao2-cli-main".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(real_main)
        .context("spawn AO2 CLI main thread")?
        .join()
        .map_err(|_| anyhow::anyhow!("AO2 CLI main thread panicked"))?
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { target } => init(target),
        Command::Run {
            workflow,
            spec,
            dry_run,
            template,
            target,
            run_id,
            pause_for_approval,
            resume,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            max_repair_attempts,
        } => run(CliRunOptions {
            workflow,
            spec,
            dry_run,
            template,
            target,
            run_id,
            pause_for_approval,
            resume,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            max_repair_attempts,
        }),
        Command::Repair { command } => repair(command),
        Command::Status { run_id, target } => status(target, run_id),
        Command::Approve {
            ticket_id,
            target,
            approver,
        } => approve(target, ticket_id, approver),
        Command::Adapter { command } => adapter(command),
        Command::Provider { command } => provider(command),
        Command::Plugin { command } => plugin(*command),
        Command::SkillContractManifest { command } => skill_contract_manifest(command),
        Command::Template { command } => template(command),
        Command::Replay { run_id, target } => replay(target, run_id),
        Command::Report {
            command,
            run_id,
            target,
            out,
            open,
        } => match command {
            Some(ReportCommand::Verify {
                run_id,
                target,
                report,
                index,
            }) => report_verify(target, run_id, report, index),
            None => report(
                target,
                run_id.context("report run_id is required unless a report subcommand is used")?,
                out,
                open,
            ),
        },
        Command::Runs { command } => runs(command),
        Command::Cockpit { command } => cockpit(command),
        Command::Pulse { command } => pulse(command),
        Command::Workbench { command } => workbench(command),
        Command::ControlPlane { command } => control_plane(command),
        Command::Contract { command } => contract(command),
        Command::Memory { command } => memory(command),
        Command::Evidence { command } => evidence(command),
        Command::Git { command } => git(command),
        Command::Factory { command } => factory(command),
        Command::Greenfield { command } => greenfield(command),
        Command::Sdd { command } => sdd_cmd::run(command),
        Command::Issue { command } => issue(command),
        Command::Support { command } => support_bundle::run(command, canonical_json_sha256),
        Command::Export { run_id, target } => export(target, run_id),
        Command::Version { json } => version(json),
        Command::Doctor {
            json,
            install_dir,
            provenance_dir,
            release,
            release_asset_dir,
            release_repo,
        } => doctor(
            json,
            install_dir,
            provenance_dir,
            release,
            release_asset_dir,
            release_repo,
        ),
        Command::Upgrade { command } => upgrade(command),
        Command::Install { command } => install(command),
        Command::Release { command } => release(command),
        Command::Cp { command } => cp(command),
    }
}

fn pulse(command: PulseCommand) -> Result<()> {
    match command {
        PulseCommand::Run {
            once,
            chain,
            execute,
            once_evidence,
            chain_evidence,
            task_contract,
            dry_run_task,
            apply_dry_run,
            dry_run_evidence,
            dry_run_sha256,
            apply_root,
            packet,
            board,
            out_dir,
            json,
        } => {
            if [once, chain, execute]
                .into_iter()
                .filter(|enabled| *enabled)
                .count()
                != 1
            {
                anyhow::bail!(
                    "ao2 pulse run requires exactly one of --once, --chain, or --execute"
                );
            }
            let result = if once {
                if once_evidence.is_some() {
                    anyhow::bail!("--once-evidence is only valid with --chain");
                }
                if chain_evidence.is_some() {
                    anyhow::bail!("--chain-evidence is only valid with --execute");
                }
                if task_contract.is_some() {
                    anyhow::bail!("--task-contract is only valid with --execute");
                }
                if dry_run_task {
                    anyhow::bail!("--dry-run-task is only valid with --execute");
                }
                if apply_dry_run || dry_run_evidence.is_some() || dry_run_sha256.is_some() {
                    anyhow::bail!("--apply-dry-run is only valid with --execute");
                }
                pulse_run_once_json(&packet, &board, &out_dir)?
            } else if chain {
                if chain_evidence.is_some() {
                    anyhow::bail!("--chain-evidence is only valid with --execute");
                }
                if task_contract.is_some() {
                    anyhow::bail!("--task-contract is only valid with --execute");
                }
                if dry_run_task {
                    anyhow::bail!("--dry-run-task is only valid with --execute");
                }
                if apply_dry_run || dry_run_evidence.is_some() || dry_run_sha256.is_some() {
                    anyhow::bail!("--apply-dry-run is only valid with --execute");
                }
                let once_evidence = once_evidence
                    .as_deref()
                    .ok_or_else(|| anyhow!("ao2 pulse run --chain requires --once-evidence"))?;
                pulse_run_chain_json(&packet, &board, once_evidence, &out_dir)?
            } else {
                if once_evidence.is_some() {
                    anyhow::bail!("--once-evidence is only valid with --chain");
                }
                if apply_dry_run {
                    if dry_run_task {
                        anyhow::bail!("--dry-run-task cannot be combined with --apply-dry-run");
                    }
                    if chain_evidence.is_some() {
                        anyhow::bail!("--chain-evidence is not valid with --apply-dry-run");
                    }
                    if task_contract.is_some() {
                        anyhow::bail!("--task-contract is not valid with --apply-dry-run");
                    }
                    let dry_run_evidence = dry_run_evidence.as_deref().ok_or_else(|| {
                        anyhow!(
                            "ao2 pulse run --execute --apply-dry-run requires --dry-run-evidence"
                        )
                    })?;
                    let dry_run_sha256 = dry_run_sha256.as_deref().ok_or_else(|| {
                        anyhow!("ao2 pulse run --execute --apply-dry-run requires --dry-run-sha256")
                    })?;
                    pulse_run_apply_dry_run_json(
                        &packet,
                        &board,
                        dry_run_evidence,
                        dry_run_sha256,
                        &apply_root,
                        &out_dir,
                    )?
                } else {
                    if dry_run_evidence.is_some() || dry_run_sha256.is_some() {
                        anyhow::bail!("--dry-run-evidence is only valid with --apply-dry-run");
                    }
                    let chain_evidence = chain_evidence.as_deref().ok_or_else(|| {
                        anyhow!("ao2 pulse run --execute requires --chain-evidence")
                    })?;
                    let task_contract = task_contract.as_deref().ok_or_else(|| {
                        anyhow!("ao2 pulse run --execute requires --task-contract")
                    })?;
                    pulse_run_execute_json(
                        &packet,
                        &board,
                        chain_evidence,
                        task_contract,
                        &out_dir,
                        dry_run_task,
                    )?
                }
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "selected_task={}",
                    json_string(&result["selected_task"], "id")
                );
                println!(
                    "artifact={}",
                    json_string(
                        &result["artifacts"],
                        pulse_artifact_key(once, chain, execute)
                    )
                );
            }
            Ok(())
        }
        PulseCommand::RunLoop {
            command,
            decision_file,
            max_chain_runs,
            max_runtime_seconds,
            out_dir,
            stdout_fallback,
            apply_root,
            json,
        } => {
            let summary = ao2_runtime::pulse_event_loop::run_pulse_event_loop(
                &command,
                decision_file.as_deref(),
                max_chain_runs,
                max_runtime_seconds,
                &out_dir,
                stdout_fallback,
                &apply_root,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                println!("status={}", summary.status);
                println!("iterations={}", summary.iterations);
                println!("decision_source={}", summary.decision_source);
                if let Some(ref path) = summary.decision_path {
                    println!("decision_path={}", path);
                }
                if let Some(ref task_id) = summary.next_task_id {
                    println!("next_task_id={}", task_id);
                }
            }
            if summary.status == "failed" {
                anyhow::bail!("Pulse event-loop run failed");
            }
            Ok(())
        }
        PulseCommand::AutoAdvance {
            resume_json,
            out_dir,
            ledger,
            stop_file,
            max_iterations,
            allow_duplicate,
            forever,
            sleep_seconds,
            generate_next,
            generate_next_sleep_seconds,
            pr_ci_gate,
            pr_ci_gate_state,
            pr_ci_gate_update,
            local_only_while_pr_blocked,
            direct_main_publish,
            apply_root,
        } => {
            let result_str = ao2_runtime::pulse_event_loop::run_pulse_auto_advance(
                ao2_runtime::pulse_event_loop::PulseAutoAdvanceOptions {
                    resume_json: &resume_json,
                    out_dir: &out_dir,
                    ledger: &ledger,
                    stop_file: &stop_file,
                    max_iterations_opt: max_iterations,
                    allow_duplicate,
                    forever,
                    sleep_seconds,
                    generate_next,
                    generate_next_sleep_seconds_opt: generate_next_sleep_seconds,
                    pr_ci_gate,
                    pr_ci_gate_state: &pr_ci_gate_state,
                    pr_ci_gate_update,
                    local_only_while_pr_blocked,
                    direct_main_publish,
                    apply_root: &apply_root,
                },
            )?;
            let summary_path = out_dir.join("summary.json");
            println!("summary={}", summary_path.to_string_lossy());
            println!("status={}", result_str);
            if result_str != "passed" && result_str != "stopped" && result_str != "waiting" {
                anyhow::bail!("Pulse auto-advance failed with status: {}", result_str);
            }
            Ok(())
        }
        PulseCommand::EvalLoop { command } => pulse_eval_loop(command),
    }
}

fn pulse_eval_loop(command: PulseEvalLoopCommand) -> Result<()> {
    match command {
        PulseEvalLoopCommand::Run {
            once,
            chain,
            executor_evidence,
            executor_sha256,
            eval_loop_evidence,
            eval_loop_sha256,
            verification_command,
            verification_status,
            packet,
            board,
            out_dir,
            json,
        } => {
            if [once, chain].into_iter().filter(|enabled| *enabled).count() != 1 {
                anyhow::bail!("ao2 pulse eval-loop run requires exactly one of --once or --chain");
            }
            let result = if once {
                let executor_evidence = executor_evidence.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --once requires --executor-evidence")
                })?;
                let executor_sha256 = executor_sha256.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --once requires --executor-sha256")
                })?;
                if eval_loop_evidence.is_some() || eval_loop_sha256.is_some() {
                    anyhow::bail!("--eval-loop-evidence is only valid with --chain");
                }
                pulse_eval_loop_run_once_json(
                    executor_evidence,
                    executor_sha256,
                    &verification_command,
                    &verification_status,
                    &packet,
                    &board,
                    &out_dir,
                )?
            } else {
                let eval_loop_evidence = eval_loop_evidence.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --chain requires --eval-loop-evidence")
                })?;
                let eval_loop_sha256 = eval_loop_sha256.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --chain requires --eval-loop-sha256")
                })?;
                if executor_evidence.is_some() || executor_sha256.is_some() {
                    anyhow::bail!("--executor-evidence is only valid with --once");
                }
                pulse_eval_loop_run_chain_json(
                    eval_loop_evidence,
                    eval_loop_sha256,
                    &verification_command,
                    &verification_status,
                    &packet,
                    &board,
                    &out_dir,
                )?
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "recommended_next_task={}",
                    json_string(&result["recommended_next_task"], "id")
                );
                println!(
                    "artifact={}",
                    json_string(&result["artifacts"], "pulse_eval_loop")
                );
            }
            Ok(())
        }
        PulseEvalLoopCommand::Handoff {
            eval_loop_evidence,
            eval_loop_sha256,
            packet,
            board,
            out_dir,
            json,
        } => {
            let result = pulse_eval_loop_handoff_json(
                &eval_loop_evidence,
                &eval_loop_sha256,
                &packet,
                &board,
                &out_dir,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "task_contract={}",
                    json_string(&result["artifacts"], "task_contract")
                );
                println!(
                    "task_contract_sha256={}",
                    json_string(&result["artifacts"], "task_contract_sha256")
                );
            }
            Ok(())
        }
    }
}

fn pulse_artifact_key(once: bool, chain: bool, execute: bool) -> &'static str {
    if once {
        "pulse_once"
    } else if chain {
        "pulse_chain"
    } else if execute {
        "pulse_executor"
    } else {
        ""
    }
}

fn pulse_run_execute_json(
    packet: &Path,
    board: &Path,
    chain_evidence: &Path,
    task_contract: &Path,
    out_dir: &Path,
    dry_run_task: bool,
) -> Result<serde_json::Value> {
    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let chain_text = fs::read_to_string(chain_evidence)
        .with_context(|| format!("read chain evidence {}", chain_evidence.display()))?;
    let chain_json: serde_json::Value =
        serde_json::from_str(&chain_text).context("parse ao2 pulse chain evidence")?;
    if json_string(&chain_json, "schema_version") != "ao2.pulse-chain.v1" {
        anyhow::bail!("ao2 pulse run --execute requires ao2.pulse-chain.v1 evidence");
    }
    if json_string(&chain_json, "status") != "planned_without_execution" {
        anyhow::bail!("ao2 pulse run --execute requires planned chain evidence");
    }
    let chain_c85_status = json_string(&chain_json["c85"], "status");
    if !matches!(chain_c85_status.as_str(), "deferred" | "passed") {
        anyhow::bail!("ao2 pulse run --execute requires deferred or passed C85 chain evidence");
    }
    let task_contract_text = fs::read_to_string(task_contract)
        .with_context(|| format!("read task contract {}", task_contract.display()))?;
    let task_contract_json: serde_json::Value =
        serde_json::from_str(&task_contract_text).context("parse ao2 pulse task contract")?;
    validate_pulse_task_contract(&task_contract_json)?;
    let chain_sha256 = sha256_hex(chain_text.as_bytes());
    let task_contract_sha256 = sha256_hex(task_contract_text.as_bytes());

    let selected_step = chain_json["chain_steps"]
        .as_array()
        .and_then(|steps| {
            steps.iter().find(|step| {
                json_string(step, "status") == "planned"
                    && json_string(step, "id") != "refuse-c85-while-billing-blocked"
            })
        })
        .cloned()
        .ok_or_else(|| anyhow!("ao2 pulse run --execute found no planned non-C85 chain task"))?;

    let selected_task = serde_json::json!({
        "id": json_string(&task_contract_json, "id"),
        "title": json_string(&task_contract_json, "title"),
        "classification": json_string(&task_contract_json, "classification"),
        "shape": json_string(&task_contract_json, "shape"),
        "status": "selected",
        "c85": false,
        "source_status": json_string(&selected_step, "status"),
        "source_chain_step": json_string(&selected_step, "id"),
        "reason": json_string(&selected_step, "reason")
    });
    let evaluator_closer = serde_json::json!({
        "status": "accepted",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "evaluator_decision": "accept_non_c85_governed_task",
        "closer_decision": "accepted",
        "evidence_digest_required": true
    });
    let executed_task = serde_json::json!({
        "id": json_string(&task_contract_json, "id"),
        "title": json_string(&task_contract_json, "title"),
        "status": "executed",
        "c85": false,
        "execution_kind": "governed_task_contract",
        "provider_execution": false,
        "queue_execution": false,
        "memory_write": false,
        "mutates_ao_artifacts": false,
        "factory_v3_evaluator_closer_required": true,
        "evaluator_closer": evaluator_closer
    });

    let pulse_executor = out_dir.join("pulse-executor.json");
    let governed_task_evidence = out_dir.join("pulse-governed-task.json");
    let pulse_task_result = out_dir.join("pulse-task-result.json");
    let pulse_dry_run_task = out_dir.join("pulse-dry-run-task.json");
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let packet_mentions_c85_deferred = packet_text.contains("C85")
        && (packet_text.contains("deferred") || packet_text.contains("billing"));
    let packet_lower = packet_text.to_lowercase();
    let packet_mentions_c85_passed = packet_text.contains("C85")
        && !packet_mentions_c85_deferred
        && (packet_lower.contains("passed") || packet_lower.contains("green"));
    let c85 = if chain_c85_status == "passed" {
        serde_json::json!({
            "status": "passed",
            "reason": "prior chain evidence records hosted C85 Release Gate passed before Pulse execute evidence",
            "hosted_github_actions_checked": true,
            "rerun_allowed_without_user_billing_fix": true
        })
    } else {
        serde_json::json!({
            "status": "deferred",
            "reason": "hosted GitHub Actions billing/spending-limit funding is unavailable",
            "hosted_github_actions_checked": false,
            "rerun_allowed_without_user_billing_fix": false
        })
    };
    let task_evidence = serde_json::json!({
        "schema_version": "ao2.pulse-governed-task.v1",
        "status": "accepted",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "selected_task": selected_task.clone(),
        "executed_task": executed_task.clone(),
        "prior_chain": {
            "path": chain_evidence.display().to_string(),
            "sha256": chain_sha256.clone(),
            "schema_version": "ao2.pulse-chain.v1",
            "status": json_string(&chain_json, "status")
        },
        "c85": c85.clone(),
        "task_contract": {
            "path": task_contract.display().to_string(),
            "sha256": task_contract_sha256.clone(),
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": json_string(&task_contract_json, "id")
        },
        "evaluator": {
            "decision": "accept_non_c85_governed_task",
            "reason": "Selected task contract is non-C85, AO2-owned, evaluator/closer bounded, and forbidden side effects are false.",
            "factory_v3_evaluator_closer_reference": true
        },
        "closer": {
            "status": "accepted",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "evidence_digest_required": true,
            "blockers": []
        },
        "trust_boundary": {
            "ao2_execution_evidence_owner": true,
            "factory_v3_evaluator_closer_reference": true,
            "hermes_frontend_queue_memory_surface": true,
            "ao2_control_plane_read_only_observer": true,
            "control_plane_observer_only": true,
            "control_plane_approves_release": false,
            "control_plane_mutates_ao_artifacts": false
        },
        "side_effects": {
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        }
    });
    let task_evidence_text = serde_json::to_string_pretty(&task_evidence)?;
    let task_evidence_sha256 = sha256_hex(task_evidence_text.as_bytes());
    let task_result = serde_json::json!({
        "schema_version": "ao2.pulse-task-result.v1",
        "status": "accepted",
        "execution_mode": "deterministic_local_evidence",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "selected_task": selected_task.clone(),
        "executed_task": executed_task.clone(),
        "prior_chain": {
            "path": chain_evidence.display().to_string(),
            "sha256": chain_sha256.clone(),
            "schema_version": "ao2.pulse-chain.v1",
            "status": json_string(&chain_json, "status")
        },
        "c85": c85.clone(),
        "task_contract": {
            "path": task_contract.display().to_string(),
            "sha256": task_contract_sha256.clone(),
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": json_string(&task_contract_json, "id")
        },
        "governed_task_evidence": {
            "path": governed_task_evidence.display().to_string(),
            "sha256": task_evidence_sha256.clone(),
            "schema_version": "ao2.pulse-governed-task.v1",
            "status": "accepted"
        },
        "evaluator_closer": evaluator_closer.clone(),
        "trust_boundary": {
            "ao2_execution_evidence_owner": true,
            "factory_v3_evaluator_closer_reference": true,
            "hermes_frontend_queue_memory_surface": true,
            "ao2_control_plane_read_only_observer": true,
            "control_plane_observer_only": true,
            "control_plane_approves_release": false,
            "control_plane_mutates_ao_artifacts": false
        },
        "side_effects": {
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        }
    });
    let task_result_text = serde_json::to_string_pretty(&task_result)?;
    let task_result_sha256 = sha256_hex(task_result_text.as_bytes());
    let dry_run_task_artifact = if dry_run_task {
        let dry_run_task_json = serde_json::json!({
            "schema_version": "ao2.pulse-dry-run-task.v1",
            "status": "planned_without_mutation",
            "execution_mode": "dry_run_planned_file_operations",
            "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            "selected_task": selected_task.clone(),
            "executed_task": executed_task.clone(),
            "prior_chain": {
                "path": chain_evidence.display().to_string(),
                "sha256": chain_sha256.clone(),
                "schema_version": "ao2.pulse-chain.v1",
                "status": json_string(&chain_json, "status")
            },
            "task_contract": {
                "path": task_contract.display().to_string(),
                "sha256": task_contract_sha256.clone(),
                "schema_version": "ao2.pulse-task-contract.v1",
                "id": json_string(&task_contract_json, "id")
            },
            "governed_task_evidence": {
                "path": governed_task_evidence.display().to_string(),
                "sha256": task_evidence_sha256.clone(),
                "schema_version": "ao2.pulse-governed-task.v1",
                "status": "accepted"
            },
            "task_result": {
                "path": pulse_task_result.display().to_string(),
                "sha256": task_result_sha256.clone(),
                "schema_version": "ao2.pulse-task-result.v1",
                "status": "accepted"
            },
            "evaluator_closer": evaluator_closer.clone(),
            "planned_file_operations": [
                {
                    "operation": "inspect_current_plugin_readiness_line",
                    "path": "docs/PLUGIN-SHIPMENT-RUNBOOK.md",
                    "mode": "planned_only",
                    "executed": false,
                    "reason": "Read the current plugin readiness proof line before planning any operator-facing runbook update."
                },
                {
                    "operation": "write_dry_run_status_handoff",
                    "path": "docs/status/codex-cron-pulse-dry-run-task-final-<timestamp>.md",
                    "mode": "planned_only",
                    "executed": false,
                    "reason": "Record dry-run task evidence, pass/fail state, artifact paths, pushed commits, parity progress, and next lengthy task."
                },
                {
                    "operation": "mirror_factory_v3_evaluator_status",
                    "path": "docs/status/hermes-governed-backend-control-plane/codex-cron-pulse-dry-run-task-final-<timestamp>.md",
                    "mode": "planned_only",
                    "executed": false,
                    "reason": "Preserve factory-v3 evaluator/closer continuity without mutating AO artifacts or Hermes scheduler state."
                }
            ],
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false
            }
        });
        let dry_run_task_text = serde_json::to_string_pretty(&dry_run_task_json)?;
        let dry_run_task_sha256 = sha256_hex(dry_run_task_text.as_bytes());
        Some((dry_run_task_text, dry_run_task_sha256))
    } else {
        None
    };
    let mut result = serde_json::json!({
        "schema_version": "ao2.pulse-executor.v1",
        "status": if dry_run_task { "executed_dry_run_task" } else { "executed_governed_task" },
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "scheduler": {
            "active_runner": "codex-cron",
            "hermes_frontend_queue_memory_concept": true,
            "hermes_cron_mutated": false,
            "fixed_interval_loop_successor": if dry_run_task { "ao2 pulse run --execute --dry-run-task" } else { "ao2 pulse run --execute" }
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "packet_mentions_c85_deferred": packet_mentions_c85_deferred,
            "packet_mentions_c85_passed": packet_mentions_c85_passed,
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "prior_chain": {
            "path": chain_evidence.display().to_string(),
            "sha256": chain_sha256,
            "schema_version": "ao2.pulse-chain.v1",
            "status": json_string(&chain_json, "status")
        },
        "task_contract": {
            "path": task_contract.display().to_string(),
            "sha256": task_contract_sha256,
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": json_string(&task_contract_json, "id")
        },
        "selected_task": selected_task,
        "executed_tasks": [
            executed_task
        ],
        "c85": c85,
        "trust_boundary": {
            "ao2_execution_evidence_owner": true,
            "factory_v3_evaluator_closer_reference": true,
            "hermes_frontend_queue_memory_surface": true,
            "ao2_control_plane_read_only_observer": true,
            "control_plane_observer_only": true,
            "control_plane_approves_release": false,
            "control_plane_mutates_ao_artifacts": false
        },
        "side_effects": {
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        },
        "artifacts": {
            "pulse_executor": pulse_executor.display().to_string(),
            "governed_task_evidence": governed_task_evidence.display().to_string(),
            "governed_task_evidence_sha256": task_evidence_sha256,
            "pulse_task_result": pulse_task_result.display().to_string(),
            "pulse_task_result_sha256": task_result_sha256
        }
    });
    if let Some((dry_run_task_text, dry_run_task_sha256)) = dry_run_task_artifact {
        if let Some(artifacts) = result
            .get_mut("artifacts")
            .and_then(|value| value.as_object_mut())
        {
            artifacts.insert(
                "pulse_dry_run_task".to_string(),
                serde_json::Value::String(pulse_dry_run_task.display().to_string()),
            );
            artifacts.insert(
                "pulse_dry_run_task_sha256".to_string(),
                serde_json::Value::String(dry_run_task_sha256),
            );
        }
        atomic_write_text(&pulse_dry_run_task, &dry_run_task_text)?;
    }
    atomic_write_text(&governed_task_evidence, &task_evidence_text)?;
    atomic_write_text(&pulse_task_result, &task_result_text)?;
    atomic_write_text(&pulse_executor, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

fn pulse_run_apply_dry_run_json(
    packet: &Path,
    board: &Path,
    dry_run_evidence: &Path,
    expected_dry_run_sha256: &str,
    apply_root: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let dry_run_text = fs::read_to_string(dry_run_evidence)
        .with_context(|| format!("read dry-run evidence {}", dry_run_evidence.display()))?;
    let dry_run_sha256 = sha256_hex(dry_run_text.as_bytes());
    if dry_run_sha256 != expected_dry_run_sha256 {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run dry-run SHA256 mismatch");
    }
    let dry_run_json: serde_json::Value =
        serde_json::from_str(&dry_run_text).context("parse ao2 pulse dry-run task evidence")?;
    if json_string(&dry_run_json, "schema_version") != "ao2.pulse-dry-run-task.v1" {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run requires ao2.pulse-dry-run-task.v1");
    }
    if json_string(&dry_run_json, "status") != "planned_without_mutation" {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run requires planned dry-run evidence");
    }
    let planned_operations = dry_run_json["planned_file_operations"]
        .as_array()
        .ok_or_else(|| {
            anyhow!("ao2 pulse run --execute --apply-dry-run requires planned operations")
        })?;
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    fs::create_dir_all(apply_root).with_context(|| format!("create {}", apply_root.display()))?;

    let mut applied_operations = Vec::new();
    for operation in planned_operations {
        let operation_id = json_string(operation, "operation");
        let planned_path = json_string(operation, "path");
        let normalized_path = pulse_apply_normalized_path(&operation_id, &planned_path)?;
        let target_path = pulse_apply_target_path(apply_root, &normalized_path)?;
        let result = match operation_id.as_str() {
            "inspect_current_plugin_readiness_line" => {
                let existing_text = fs::read_to_string(&target_path).unwrap_or_default();
                let append = "\n## AO2 Pulse apply evidence\n\n- Applied bounded plugin/readiness maintenance through `ao2 pulse run --execute --apply-dry-run`.\n- C85 hosted GitHub Actions remains deferred until billing/spending-limit funding is fixed.\n- Hermes cron/watchdog jobs were not started or mutated.\n";
                let next_text = if existing_text.contains("AO2 Pulse apply evidence") {
                    existing_text
                } else {
                    format!("{existing_text}{append}")
                };
                atomic_write_text(&target_path, &next_text)?;
                serde_json::json!({
                    "operation": operation_id,
                    "path": normalized_path,
                    "planned_path": planned_path,
                    "mode": "applied",
                    "executed": true,
                    "allowed_by_dry_run": true,
                    "bytes_written": next_text.len()
                })
            }
            "write_dry_run_status_handoff" | "mirror_factory_v3_evaluator_status" => {
                let body = pulse_apply_status_body(&dry_run_json, &dry_run_sha256, &operation_id);
                atomic_write_text(&target_path, &body)?;
                serde_json::json!({
                    "operation": operation_id,
                    "path": normalized_path,
                    "planned_path": planned_path,
                    "mode": "applied",
                    "executed": true,
                    "allowed_by_dry_run": true,
                    "bytes_written": body.len()
                })
            }
            _ => anyhow::bail!(
                "ao2 pulse run --execute --apply-dry-run refuses unrecognized operation `{operation_id}`"
            ),
        };
        applied_operations.push(result);
    }

    let pulse_executor = out_dir.join("pulse-executor.json");
    let pulse_apply_result = out_dir.join("pulse-apply-result.json");
    let packet_mentions_c85_deferred = packet_text.contains("C85")
        && (packet_text.contains("deferred") || packet_text.contains("billing"));
    let apply_result = serde_json::json!({
        "schema_version": "ao2.pulse-apply-result.v1",
        "status": "accepted",
        "execution_mode": "bounded_planned_file_apply",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "selected_task": dry_run_json["selected_task"].clone(),
        "dry_run_task": {
            "path": dry_run_evidence.display().to_string(),
            "sha256": dry_run_sha256,
            "schema_version": "ao2.pulse-dry-run-task.v1",
            "status": "planned_without_mutation"
        },
        "prior_chain": dry_run_json["prior_chain"].clone(),
        "task_contract": dry_run_json["task_contract"].clone(),
        "governed_task_evidence": dry_run_json["governed_task_evidence"].clone(),
        "task_result": dry_run_json["task_result"].clone(),
        "evaluator_closer": dry_run_json["evaluator_closer"].clone(),
        "applied_file_operations": applied_operations,
        "trust_boundary": {
            "ao2_execution_evidence_owner": true,
            "factory_v3_evaluator_closer_reference": true,
            "hermes_frontend_queue_memory_surface": true,
            "ao2_control_plane_read_only_observer": true,
            "control_plane_observer_only": true,
            "control_plane_approves_release": false,
            "control_plane_mutates_ao_artifacts": false
        },
        "side_effects": {
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        }
    });
    let apply_result_text = serde_json::to_string_pretty(&apply_result)?;
    let apply_result_sha256 = sha256_hex(apply_result_text.as_bytes());
    let result = serde_json::json!({
        "schema_version": "ao2.pulse-executor.v1",
        "status": "applied_dry_run_task",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "scheduler": {
            "active_runner": "codex-cron",
            "hermes_frontend_queue_memory_concept": true,
            "hermes_cron_mutated": false,
            "fixed_interval_loop_successor": "ao2 pulse run --execute --apply-dry-run"
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "packet_mentions_c85_deferred": packet_mentions_c85_deferred,
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "selected_task": dry_run_json["selected_task"].clone(),
        "c85": {
            "status": "deferred",
            "reason": "hosted GitHub Actions billing/spending-limit funding is unavailable",
            "hosted_github_actions_checked": false,
            "rerun_allowed_without_user_billing_fix": false
        },
        "trust_boundary": {
            "ao2_execution_evidence_owner": true,
            "factory_v3_evaluator_closer_reference": true,
            "hermes_frontend_queue_memory_surface": true,
            "ao2_control_plane_read_only_observer": true,
            "control_plane_observer_only": true,
            "control_plane_approves_release": false,
            "control_plane_mutates_ao_artifacts": false
        },
        "side_effects": {
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        },
        "artifacts": {
            "pulse_executor": pulse_executor.display().to_string(),
            "pulse_apply_result": pulse_apply_result.display().to_string(),
            "pulse_apply_result_sha256": apply_result_sha256
        }
    });
    atomic_write_text(&pulse_apply_result, &apply_result_text)?;
    atomic_write_text(&pulse_executor, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

fn pulse_apply_normalized_path(operation: &str, planned_path: &str) -> Result<String> {
    let normalized = match operation {
        "inspect_current_plugin_readiness_line" => planned_path.to_string(),
        "write_dry_run_status_handoff" => {
            "docs/status/codex-cron-pulse-apply-result-final.md".to_string()
        }
        "mirror_factory_v3_evaluator_status" => {
            "docs/status/hermes-governed-backend-control-plane/codex-cron-pulse-apply-result-final.md"
                .to_string()
        }
        _ => anyhow::bail!(
            "ao2 pulse run --execute --apply-dry-run refuses unrecognized operation `{operation}`"
        ),
    };
    if normalized.starts_with('/') || normalized.contains("..") {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run refuses unsafe path `{normalized}`");
    }
    Ok(normalized)
}

fn pulse_apply_target_path(apply_root: &Path, relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        anyhow::bail!(
            "ao2 pulse run --execute --apply-dry-run refuses unsafe path `{relative_path}`"
        );
    }
    Ok(apply_root.join(path))
}

fn pulse_apply_status_body(
    dry_run_json: &serde_json::Value,
    dry_run_sha256: &str,
    operation_id: &str,
) -> String {
    format!(
        "# AO2 Pulse Apply Result\n\n- Operation: `{operation_id}`\n- Selected task: `{}`\n- Dry-run evidence SHA256: `{dry_run_sha256}`\n- Evaluator/closer status: `{}`\n- C85 hosted GitHub Actions: deferred until billing/spending-limit funding is fixed.\n- Hermes cron/watchdog mutation: false.\n- Provider, queue, memory, AO artifact, and control-plane mutation: false.\n",
        json_string(&dry_run_json["selected_task"], "id"),
        json_string(&dry_run_json["evaluator_closer"], "status")
    )
}

fn validate_pulse_task_contract(contract: &serde_json::Value) -> Result<()> {
    if json_string(contract, "schema_version") != "ao2.pulse-task-contract.v1" {
        anyhow::bail!("ao2 pulse run --execute requires ao2.pulse-task-contract.v1");
    }
    for field in ["id", "title", "classification", "shape"] {
        if json_string(contract, field).trim().is_empty() {
            anyhow::bail!("ao2 pulse run --execute requires task contract field `{field}`");
        }
    }
    if json_bool(contract, "c85") {
        anyhow::bail!("ao2 pulse run --execute refuses C85 task contracts");
    }
    if !json_bool(contract, "ao2_owned_execution") {
        anyhow::bail!("ao2 pulse run --execute requires AO2-owned task execution");
    }
    if !json_bool(contract, "factory_v3_evaluator_closer_required") {
        anyhow::bail!("ao2 pulse run --execute requires factory-v3 evaluator/closer acceptance");
    }
    let side_effects = contract
        .get("side_effects")
        .ok_or_else(|| anyhow!("ao2 pulse run --execute requires task contract side_effects"))?;
    for field in [
        "provider_execution",
        "queue_execution",
        "memory_write",
        "mutates_ao_artifacts",
        "hermes_cron_watchdog_mutation",
        "control_plane_mutation",
    ] {
        if json_bool(side_effects, field) {
            anyhow::bail!(
                "ao2 pulse run --execute refuses task contracts with forbidden side effect `{field}`"
            );
        }
    }
    Ok(())
}

fn contract(command: ContractCommand) -> Result<()> {
    match command {
        ContractCommand::Extract { spec, out, json } => {
            let content =
                fs::read_to_string(&spec).with_context(|| format!("read {}", spec.display()))?;
            let ledger = extract_obligation_ledger(&spec.to_string_lossy(), &content);
            let body = serde_json::to_string_pretty(&ledger)?;
            atomic_write_text(&out, &body)?;
            if json {
                println!("{body}");
            } else {
                println!("obligation_ledger={}", out.display());
                println!(
                    "verdict={}",
                    json_string(&serde_json::to_value(&ledger)?, "verdict")
                );
                println!("obligation_count={}", ledger.obligations.len());
            }
            Ok(())
        }
        ContractCommand::Check {
            ledger,
            target,
            out,
            json,
        } => {
            let content = fs::read_to_string(&ledger)
                .with_context(|| format!("read {}", ledger.display()))?;
            let ledger: ObligationLedger = serde_json::from_str(&content)
                .with_context(|| format!("parse {}", ledger.display()))?;
            let checked = check_obligation_ledger(&ledger, &target)
                .with_context(|| format!("check obligations under {}", target.display()))?;
            let body = serde_json::to_string_pretty(&checked)?;
            atomic_write_text(&out, &body)?;
            if json {
                println!("{body}");
            } else {
                println!("checked_obligation_ledger={}", out.display());
                println!(
                    "verdict={}",
                    json_string(&serde_json::to_value(&checked)?, "verdict")
                );
                println!("pass={}", checked.summary.pass);
                println!("fail={}", checked.summary.fail);
                println!("unverified={}", checked.summary.unverified);
            }
            if checked.verdict == ao2_core::ObligationVerdict::Accepted {
                Ok(())
            } else {
                Err(anyhow!(
                    "obligation check rejected: pass={} fail={} unverified={}",
                    checked.summary.pass,
                    checked.summary.fail,
                    checked.summary.unverified
                ))
            }
        }
        ContractCommand::Gate {
            ledger,
            target,
            stage,
            out,
            json,
            support_signing_key,
            support_signer_id,
            support_operator_role,
            support_run_id,
            exports_dir,
            allow_unsigned_obligation_gates,
        } => {
            let stage = stage.trim();
            if stage.is_empty() {
                return Err(anyhow!("--stage must not be empty"));
            }
            if support_signing_key.is_none() && !allow_unsigned_obligation_gates {
                return Err(anyhow!(
                    "`ao2 contract gate` requires --support-signing-key by default \
                     (slice 18 producer-side default-on, mirroring slice 11 release-gate \
                     consumer-side flip); pass --allow-unsigned-obligation-gates to opt \
                     out, but downstream `ao2 release gate` and POST /api/release-gate \
                     will still reject the unsigned gate unless their own escape valves \
                     are also set"
                ));
            }
            let content = fs::read_to_string(&ledger)
                .with_context(|| format!("read {}", ledger.display()))?;
            let ledger_value: ObligationLedger = serde_json::from_str(&content)
                .with_context(|| format!("parse {}", ledger.display()))?;
            let checked = check_obligation_ledger(&ledger_value, &target)
                .with_context(|| format!("gate obligations under {}", target.display()))?;
            let failed_obligations = checked
                .obligations
                .iter()
                .filter(|obligation| obligation.status == ObligationStatus::Fail)
                .cloned()
                .collect::<Vec<_>>();
            let unverified_obligations = checked
                .obligations
                .iter()
                .filter(|obligation| obligation.status == ObligationStatus::Unverified)
                .cloned()
                .collect::<Vec<_>>();
            let status = if checked.verdict == ao2_core::ObligationVerdict::Accepted {
                "passed"
            } else {
                "failed"
            };
            let gate = serde_json::json!({
                "schema_version": "ao2.obligation-gate.v1",
                "stage": stage,
                "status": status,
                "verdict": checked.verdict,
                "summary": checked.summary,
                "ledger_path": ledger.display().to_string(),
                "target": target.display().to_string(),
                "gate_path": out.display().to_string(),
                "checked_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                "failed_obligations": failed_obligations,
                "unverified_obligations": unverified_obligations,
                "checked_ledger": checked
            });
            let body = serde_json::to_string_pretty(&gate)?;
            atomic_write_text(&out, &body)?;

            let signing_evidence = if let Some(key_path) = support_signing_key.as_ref() {
                let operator_role = support_operator_role.trim();
                if operator_role.is_empty() {
                    return Err(anyhow!(
                        "--support-operator-role must be non-empty when --support-signing-key is set"
                    ));
                }
                let signer_id = support_signer_id.trim();
                if signer_id.is_empty() {
                    return Err(anyhow!(
                        "--support-signer-id must be non-empty when --support-signing-key is set"
                    ));
                }
                let resolved_exports_dir = exports_dir.clone().unwrap_or_else(|| {
                    out.parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| PathBuf::from("."))
                });
                let signed = emit_contract_gate_signed_wrapper(
                    &gate,
                    &resolved_exports_dir,
                    key_path,
                    signer_id,
                    operator_role,
                    support_run_id.trim(),
                )?;
                Some(signed)
            } else {
                None
            };

            if json {
                if let Some(signing) = signing_evidence.as_ref() {
                    let mut emitted = gate.clone();
                    if let Some(object) = emitted.as_object_mut() {
                        object.insert("support_signing_evidence".to_string(), signing.clone());
                    }
                    println!("{}", serde_json::to_string_pretty(&emitted)?);
                } else {
                    println!("{body}");
                }
            } else {
                println!("obligation_gate={}", out.display());
                println!("stage={}", json_string(&gate, "stage"));
                println!("status={}", json_string(&gate, "status"));
                println!("verdict={}", json_string(&gate, "verdict"));
                println!("fail={}", gate["summary"]["fail"].as_u64().unwrap_or(0));
                println!(
                    "unverified={}",
                    gate["summary"]["unverified"].as_u64().unwrap_or(0)
                );
                if let Some(signing) = signing_evidence.as_ref() {
                    println!("wrapper_path={}", json_string(signing, "wrapper_path"));
                    println!("signature_path={}", json_string(signing, "signature_path"));
                    println!(
                        "public_key_path={}",
                        json_string(signing, "public_key_path")
                    );
                    println!("signature_verified={}", signing["signature_verified"]);
                }
            }
            if json_string(&gate, "status") == "passed" {
                Ok(())
            } else {
                Err(anyhow!(
                    "obligation gate failed at {}: pass={} fail={} unverified={}",
                    json_string(&gate, "stage"),
                    gate["summary"]["pass"].as_u64().unwrap_or(0),
                    gate["summary"]["fail"].as_u64().unwrap_or(0),
                    gate["summary"]["unverified"].as_u64().unwrap_or(0)
                ))
            }
        }
        ContractCommand::SignObligationGate {
            gate,
            support_signing_key,
            support_signer_id,
            support_operator_role,
            support_run_id,
            exports_dir,
            json,
        } => {
            let gate_text =
                fs::read_to_string(&gate).with_context(|| format!("read {}", gate.display()))?;
            let gate_json: serde_json::Value = serde_json::from_str(&gate_text)
                .with_context(|| format!("parse {}", gate.display()))?;
            if json_string(&gate_json, "schema_version") != "ao2.obligation-gate.v1" {
                return Err(anyhow!(
                    "contract sign-obligation-gate requires ao2.obligation-gate.v1: {}",
                    gate.display()
                ));
            }
            let operator_role = support_operator_role.trim();
            if operator_role.is_empty() {
                return Err(anyhow!("--support-operator-role must be non-empty"));
            }
            let signer_id = support_signer_id.trim();
            if signer_id.is_empty() {
                return Err(anyhow!("--support-signer-id must be non-empty"));
            }
            let run_id = support_run_id.trim();
            if run_id.is_empty() {
                return Err(anyhow!("--support-run-id must be non-empty"));
            }
            let resolved_exports_dir = exports_dir.unwrap_or_else(|| {
                gate.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let signing = emit_contract_gate_signed_wrapper(
                &gate_json,
                &resolved_exports_dir,
                &support_signing_key,
                signer_id,
                operator_role,
                run_id,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&signing)?);
            } else {
                println!("wrapper_path={}", json_string(&signing, "wrapper_path"));
                println!("signature_path={}", json_string(&signing, "signature_path"));
                println!(
                    "public_key_path={}",
                    json_string(&signing, "public_key_path")
                );
                println!("signature_verified={}", signing["signature_verified"]);
            }
            Ok(())
        }
        ContractCommand::Annotate {
            ledger,
            obligation_id,
            evidence_path,
            evidence_line,
            detail,
            waiver,
            out,
            json,
        } => {
            let content = fs::read_to_string(&ledger)
                .with_context(|| format!("read {}", ledger.display()))?;
            let ledger: ObligationLedger = serde_json::from_str(&content)
                .with_context(|| format!("parse {}", ledger.display()))?;
            let evidence = match evidence_path {
                Some(path) => {
                    let line = evidence_line
                        .context("--evidence-line is required with --evidence-path")?;
                    if line == 0 {
                        return Err(anyhow!("--evidence-line must be greater than 0"));
                    }
                    Some(ObligationEvidence {
                        path,
                        line,
                        detail: detail
                            .filter(|detail| !detail.trim().is_empty())
                            .unwrap_or_else(|| "manual operator evidence".to_string()),
                    })
                }
                None => None,
            };
            let annotated = annotate_obligation_ledger(&ledger, &obligation_id, evidence, waiver)
                .map_err(|error| anyhow!(error))?;
            let body = serde_json::to_string_pretty(&annotated)?;
            atomic_write_text(&out, &body)?;
            if json {
                println!("{body}");
            } else {
                println!("annotated_obligation_ledger={}", out.display());
                println!(
                    "verdict={}",
                    json_string(&serde_json::to_value(&annotated)?, "verdict")
                );
                println!("pass={}", annotated.summary.pass);
                println!("waived={}", annotated.summary.waived);
                println!("unverified={}", annotated.summary.unverified);
            }
            Ok(())
        }
        ContractCommand::VerifyObligationGateSigning {
            gate,
            evidence_exports_dir,
            public_key,
            json,
        } => {
            let result = contract_verify_obligation_gate_signing_json(
                &gate,
                evidence_exports_dir.as_deref(),
                public_key.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("gate_path={}", json_string(&result, "gate_path"));
                println!("stage={}", json_string(&result, "stage"));
                println!("signing_status={}", json_string(&result, "signing_status"));
                println!("signature_verified={}", result["signature_verified"]);
                println!(
                    "matched_wrapper={}",
                    json_string(&result, "matched_wrapper_path")
                );
                println!("ao2_owned={}", result["ao2_owned"]);
            }
            if json_string(&result, "signing_status") != "signed-and-verified" {
                return Err(anyhow!(
                    "obligation gate {} is not signed-and-verified ({})",
                    gate.display(),
                    json_string(&result, "signing_status")
                ));
            }
            Ok(())
        }
        ContractCommand::ObligationGateSigningSurvey {
            target,
            summary,
            json,
        } => {
            let result = contract_obligation_gate_signing_survey_json(
                target.as_deref(),
                summary.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let sources = result["sources"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                println!("sources={sources}");
                if !json_string(&result, "target").is_empty() {
                    println!("target={}", json_string(&result, "target"));
                }
                if !json_string(&result, "summary").is_empty() {
                    println!("summary={}", json_string(&result, "summary"));
                }
                println!("total_gates={}", result["total_gates"]);
                println!("signed_and_verified={}", result["signed_and_verified"]);
                println!("unsigned={}", result["unsigned"]);
                println!("status={}", json_string(&result, "status"));
            }
            Ok(())
        }
    }
}

fn factory(command: FactoryCommand) -> Result<()> {
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

fn greenfield(command: GreenfieldCommand) -> Result<()> {
    match command {
        GreenfieldCommand::Ingest {
            spec,
            target,
            run_id,
            verifier_command,
            signing_key,
            signer_id,
            out_dir,
            json,
        } => {
            let result = greenfield_ingest_json(GreenfieldIngestOptions {
                spec: &spec,
                target: &target,
                run_id,
                verifier_command,
                signing_key: signing_key.as_deref(),
                signer_id: &signer_id,
                out_dir: out_dir.as_deref(),
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("greenfield_ingest={}", json_string(&result, "ingest_path"));
                println!("plan={}", json_string(&result["artifacts"], "plan"));
                println!(
                    "classification_shape={}",
                    json_string(&result["classification"], "shape")
                );
                println!(
                    "classification_size={}",
                    json_string(&result["classification"], "size")
                );
            }
            Ok(())
        }
        GreenfieldCommand::GovernedRun {
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
            let result = greenfield_governed_run_json(GreenfieldGovernedRunOptions {
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
                println!("run_id={}", json_string(&result, "run_id"));
                println!("status={}", json_string(&result, "status"));
                println!(
                    "greenfield_governed_run={}",
                    json_string(&result["artifacts"], "greenfield_governed_run")
                );
                println!(
                    "evidence_pack={}",
                    json_string(&result["governed_run"]["artifacts"], "packed_evidence")
                );
            }
            Ok(())
        }
        GreenfieldCommand::ThreeOsSmokeGate { smokes, out, json } => {
            let result = greenfield_three_os_smoke_gate_json(&smokes, out.as_deref())?;
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
                return Err(anyhow!("greenfield three-os-smoke-gate rejected"));
            }
            Ok(())
        }
    }
}

fn resolve_api_token(api_token: Option<&str>, api_token_env: Option<&str>) -> Result<String> {
    match (api_token, api_token_env) {
        (Some(_), Some(_)) => Err(anyhow!("use only one of --api-token or --api-token-env")),
        (Some(token), None) => trimmed_required("--api-token", token),
        (None, Some(env_name)) => {
            let env_name = trimmed_required("--api-token-env", env_name)?;
            let token = std::env::var(&env_name)
                .with_context(|| format!("read control-plane API token from ${env_name}"))?;
            trimmed_required(&format!("${env_name}"), &token)
        }
        (None, None) => Err(anyhow!("--api-token or --api-token-env is required")),
    }
}

fn init(target: PathBuf) -> Result<()> {
    let state = target.join(".ao2");
    fs::create_dir_all(&state).with_context(|| format!("create {}", state.display()))?;
    let readme = state.join("README.md");
    if !readme.exists() {
        fs::write(
            &readme,
            "# AO2 Local State\n\nRun artifacts are stored under `runs/<run-id>/`.\n",
        )?;
    }
    let profiles = state.join("provider-profiles.json");
    if !profiles.exists() {
        fs::write(&profiles, provider_profiles_json()?)?;
    }
    println!("initialized {}", state.display());
    Ok(())
}

fn repair(command: RepairCommand) -> Result<()> {
    match command {
        RepairCommand::Resume {
            evidence_pack,
            workflow,
            template,
            target,
            run_id,
            provider,
            provider_prompt,
            provider_prompt_file,
            provider_max_budget_usd,
            max_repair_attempts,
            json,
        } => {
            let workflow = workflow.map(Ok).unwrap_or_else(|| {
                let template = template
                    .as_deref()
                    .context("--workflow or --template is required")?;
                materialize_template_workflow(&target, template)
            })?;
            let provider = parse_provider(provider.as_deref().unwrap_or("scripted"))?;
            let prompt = read_prompt(provider_prompt, provider_prompt_file)?;
            let repair_source = repair_source_context_from_evidence_pack(&evidence_pack)?;
            let source_run_id = repair_source.source_run_id.clone();
            let summary = run_risky_pr_with_provider_prompt(ProviderRunOptions {
                target_repo: target,
                workflow_path: workflow,
                run_id,
                provider,
                prompt,
                max_repair_attempts,
                max_budget_usd: provider_max_budget_usd,
                repair_source: Some(repair_source),
            })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "ao2.repair-resume.v1",
                        "source_run_id": source_run_id,
                        "run_id": summary.run_id,
                        "status": summary.status,
                        "evidence_pack": summary.evidence_pack_path,
                        "report": summary.report_path,
                        "rejection_count": summary.rejection_count
                    }))?
                );
            } else {
                println!("source_run_id={source_run_id}");
                print_run_summary(&summary);
            }
            Ok(())
        }
    }
}

fn repair_source_context_from_evidence_pack(path: &Path) -> Result<RepairSourceContext> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let evidence: serde_json::Value =
        serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))?;
    let schema_version = json_string(&evidence, "schema_version");
    if schema_version != "ao2.evidence-pack.v1" {
        anyhow::bail!("repair resume requires ao2.evidence-pack.v1, got {schema_version}");
    }
    let source_verdict = json_string(&evidence, "verdict");
    if source_verdict == "accepted" {
        anyhow::bail!("repair resume requires a non-accepted source evidence pack");
    }
    let source_run_id = json_string(&evidence, "run_id");
    if source_run_id.is_empty() {
        anyhow::bail!("repair resume source evidence pack is missing run_id");
    }
    let run_health = evidence
        .get("run_health")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "schema_version": "ao2.run-health.v1" }));
    let mut unresolved_concerns = string_values(
        run_health
            .get("unresolved_concerns")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    );
    if unresolved_concerns.is_empty() {
        unresolved_concerns = unresolved_concerns_from_closures(&evidence);
    }
    let evidence_refs = string_values(
        run_health
            .get("evidence_refs")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    );
    Ok(RepairSourceContext {
        source_run_id,
        evidence_pack_path: path.to_path_buf(),
        source_verdict,
        run_health,
        unresolved_concerns,
        evidence_refs,
        latest_verifier_output: latest_artifact_content(&evidence, "test_log"),
    })
}

fn unresolved_concerns_from_closures(evidence: &serde_json::Value) -> Vec<String> {
    let mut concerns = BTreeSet::new();
    for closure in json_array(evidence, "closures") {
        for concern in json_array(closure, "unresolved_concerns") {
            if let Some(text) = concern.as_str() {
                concerns.insert(text.to_string());
            }
        }
    }
    concerns.into_iter().collect()
}

fn string_values(values: &[serde_json::Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn latest_artifact_content(evidence: &serde_json::Value, artifact_type: &str) -> Option<String> {
    json_array(evidence, "artifacts")
        .iter()
        .rev()
        .find(|artifact| json_string(artifact, "artifact_type") == artifact_type)
        .and_then(|artifact| {
            let uri = json_string(artifact, "uri");
            if uri.is_empty() {
                None
            } else {
                fs::read_to_string(uri).ok()
            }
        })
}

fn status(target: PathBuf, run_id: String) -> Result<()> {
    let path = target
        .join(".ao2")
        .join("runs")
        .join(&run_id)
        .join("run-record.json");
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn runs(command: RunsCommand) -> Result<()> {
    match command {
        RunsCommand::List { target, json } => runs_list(target, json),
        RunsCommand::Show {
            run_id,
            target,
            json,
        } => runs_show(target, run_id, json),
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

struct ApprovalRecoveryContext {
    ticket_id: String,
    run_id: String,
    action_digest: String,
    evidence_dir: PathBuf,
    target: PathBuf,
}

fn read_approval_recovery_context(
    target: &Path,
    approval_path: &Path,
) -> Option<ApprovalRecoveryContext> {
    let text = fs::read_to_string(approval_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ticket = value.get("ticket")?;
    let ticket_id = ticket.get("ticket_id")?.as_str()?.to_string();
    let run_id = ticket.get("run_id")?.as_str()?.to_string();
    let action_digest = ticket.get("action_digest")?.as_str()?.to_string();
    let evidence_dir = approval_path.parent()?.parent()?.to_path_buf();
    Some(ApprovalRecoveryContext {
        ticket_id,
        run_id,
        action_digest,
        evidence_dir,
        target: target.to_path_buf(),
    })
}

fn approval_recovery_context_by_ticket(
    target: &Path,
    ticket_id: &str,
) -> Option<ApprovalRecoveryContext> {
    let runs_dir = target.join(".ao2").join("runs");
    for entry in fs::read_dir(runs_dir).ok()? {
        let run_dir = entry.ok()?.path();
        let approval_path = run_dir.join("approvals").join(format!("{ticket_id}.json"));
        if approval_path.is_file() {
            return read_approval_recovery_context(target, &approval_path);
        }
    }
    None
}

fn pending_approval_recovery_context(
    target: &Path,
    run_id: &str,
) -> Option<ApprovalRecoveryContext> {
    let approvals_dir = target
        .join(".ao2")
        .join("runs")
        .join(run_id)
        .join("approvals");
    for entry in fs::read_dir(approvals_dir).ok()? {
        let approval_path = entry.ok()?.path();
        if approval_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&approval_path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        if value["ticket"]["status"].as_str() == Some("pending") {
            return read_approval_recovery_context(target, &approval_path);
        }
    }
    None
}

fn print_approval_recovery_context(
    context: &ApprovalRecoveryContext,
    approval_status: &str,
    digest_failure: Option<&str>,
) {
    eprintln!("approval_status={approval_status}");
    eprintln!("required_digest_field=action_digest");
    eprintln!("action_digest={}", context.action_digest);
    if let Some(digest_failure) = digest_failure {
        eprintln!("digest_failure={digest_failure}");
    }
    eprintln!("replay_state=waiting_for_approval");
    eprintln!("evidence_dir={}", context.evidence_dir.display());
    eprintln!(
        "next_step=ao2 approve {} --target {} --approver <operator>; ao2 run --resume {} --target {}",
        context.ticket_id,
        context.target.display(),
        context.run_id,
        context.target.display()
    );
    eprintln!("recovery=preserve the failing state and compare the required action_digest before retrying");
}

fn approve(target: PathBuf, ticket_id: String, approver: String) -> Result<()> {
    match approve_risky_pr_ticket(ApprovalOptions {
        target_repo: target.clone(),
        ticket_id: ticket_id.clone(),
        approver,
    }) {
        Ok(approval) => {
            println!("ticket_id={}", approval.ticket_id);
            println!("status={}", approval.status);
            println!("approver={}", approval.approver.unwrap_or_default());
            Ok(())
        }
        Err(error) => {
            if error.to_string().contains("approval digest mismatch") {
                if let Some(context) = approval_recovery_context_by_ticket(&target, &ticket_id) {
                    print_approval_recovery_context(
                        &context,
                        "rejected",
                        Some("approval digest mismatch"),
                    );
                }
            }
            Err(error)
        }
    }
}

fn replay(target: PathBuf, run_id: String) -> Result<()> {
    let summary = replay_run(ReplayOptions {
        target_repo: target,
        run_id,
    })?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

struct TemplateSpec {
    name: &'static str,
    description: &'static str,
    content: &'static str,
}

const TASK_TEMPLATES: &[TemplateSpec] = &[
    TemplateSpec {
        name: "bug-fix",
        description: "Minimal bug fix with regression test and replayable evidence.",
        content: include_str!("../../../examples/task-templates/bug-fix.yaml"),
    },
    TemplateSpec {
        name: "small-refactor",
        description: "Behavior-preserving refactor with verifier and evidence gates.",
        content: include_str!("../../../examples/task-templates/small-refactor.yaml"),
    },
    TemplateSpec {
        name: "dependency-upgrade",
        description: "Single dependency upgrade with compatibility checks.",
        content: include_str!("../../../examples/task-templates/dependency-upgrade.yaml"),
    },
    TemplateSpec {
        name: "test-generation",
        description: "High-value tests for existing behavior.",
        content: include_str!("../../../examples/task-templates/test-generation.yaml"),
    },
    TemplateSpec {
        name: "rust-cargo-bug-fix",
        description: "Rust crate bug fix with cargo test verifier evidence.",
        content: include_str!("../../../examples/task-templates/rust-cargo-bug-fix.yaml"),
    },
];

fn template(command: TemplateCommand) -> Result<()> {
    match command {
        TemplateCommand::List => {
            for template in TASK_TEMPLATES {
                println!("{}\t{}", template.name, template.description);
            }
            Ok(())
        }
        TemplateCommand::Show { name } => {
            let Some(template) = TASK_TEMPLATES.iter().find(|template| template.name == name)
            else {
                anyhow::bail!("unknown template: {name}");
            };
            print!("{}", template.content);
            Ok(())
        }
    }
}

fn skill_contract_manifest(command: SkillContractManifestCommand) -> Result<()> {
    match command {
        SkillContractManifestCommand::Generate {
            factory_v3_root,
            out_dir,
            json,
        } => skill_contract_manifest_generate(factory_v3_root, out_dir, json),
        SkillContractManifestCommand::Verify {
            manifest,
            manifest_sha256,
            json,
        } => skill_contract_manifest_verify(manifest, manifest_sha256, json),
    }
}

const SKILL_CONTRACT_REQUIRED_INVENTORY: [&str; 7] = [
    "intake",
    "closure_verification",
    "evaluator_closer_acceptance",
    "provider_auth_rules",
    "redaction_token_safety",
    "cross_platform_proof",
    "plugin_shipment_runbook_rules",
];

fn skill_contract_manifest_generate(
    factory_v3_root: PathBuf,
    out_dir: PathBuf,
    json_output: bool,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let factory_v3_root = fs::canonicalize(&factory_v3_root)
        .with_context(|| format!("canonicalize {}", factory_v3_root.display()))?;
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let entries = serde_json::json!([
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "intake",
            source_relative_path: "agents/intake.toml",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 factory app-run",
                "cli_factory_app_run_derives_evaluator_rubric_before_execution",
                "ao2.factory-app-run.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 owns bounded work intake and must preserve scoped reads/writes, success criteria, and sensitive-field handling.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "closure_verification",
            source_relative_path: "scripts/verify_closure.py",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 factory closer-decision",
                "cli_factory_closer_decision_signs_and_verifies_rubric_bound_closure",
                "ao2.factory-closer-decision.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 signs rubric-bound closer decisions while factory-v3 remains a parity auditor.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "evaluator_closer_acceptance",
            source_relative_path: "agents/evaluator-closer.toml",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 factory evaluator-rubric",
                "cli_factory_evaluator_rubric_emits_signed_acceptance_contract",
                "ao2.factory-acceptance-rubric.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 derives signed acceptance criteria while factory-v3 remains the parity auditor and acceptance-role reference.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "provider_auth_rules",
            source_relative_path: "scripts/factory_doctor.py",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin readiness",
                "cli_plugin_readiness_emits_codex_claude_wrapper_contract",
                "ao2.plugin-readiness.v1"
            )),
            blocker: None,
            trust_boundary_notes: "Provider execution must remain local OAuth CLI-only; provider API-key auth remains forbidden.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "redaction_token_safety",
            source_relative_path: "SETUP.md",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin package-verify",
                "cli_plugin_package_verify_accepts_distributed_archive",
                "ao2.plugin-package-verification.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 artifacts must reject credential-shaped output and preserve token-safe summaries.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "cross_platform_proof",
            source_relative_path: "docs/plans/ao2-factory-v3-replacement-parity-plan.md",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin packaged-replacement-observer-bundle",
                "cli_plugin_packaged_replacement_observer_bundle_packages_three_platform_proofs",
                "ao2.k37-packaged-replacement-hardening-observer-bundle.v1"
            )),
            blocker: None,
            trust_boundary_notes: "macOS, Ubuntu SSH, and direct Windows SSH evidence must be packaged by AO2 before read-only observation.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "plugin_shipment_runbook_rules",
            source_relative_path: "docs/plans/ao2-factory-v3-replacement-parity-plan.md",
            category: "plugin_packaging",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin shipment-readiness",
                "cli_plugin_shipment_readiness_aggregates_operator_handoff_evidence",
                "ao2.plugin-shipment-readiness.v1"
            )),
            blocker: None,
            trust_boundary_notes: "Codex/Claude plugin shipment keeps local OAuth CLI auth, digest gates, token-safe output, and observer-only control-plane boundaries.",
        })?
    ]);

    let manifest = serde_json::json!({
        "schema_version": "ao2.skill-contract-manifest.v1",
        "status": "accepted",
        "producer": "ao2",
        "work_source": "codex-cron AO2 factory-v3 replacement parity",
        "entry_count": entries.as_array().map(Vec::len).unwrap_or_default(),
        "required_inventory": SKILL_CONTRACT_REQUIRED_INVENTORY,
        "entries": entries,
        "entries_sha256": canonical_json_sha256(&entries),
        "guardrails": {
            "runtime_critical_checked": true,
            "runtime_critical_requires_enforcement_or_blocker": true,
            "raw_factory_v3_skill_copy_allowed": false
        },
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "provider_api_key_env_required": false
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        },
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "side_effects": {
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_write_memory": false,
            "would_mutate_control_plane": false,
            "would_mutate_ao_artifacts": false,
            "would_approve_release": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    validate_skill_contract_manifest(&manifest)?;

    let manifest_path = out_dir.join("skill-contract-manifest.json");
    let manifest_body = serde_json::to_string_pretty(&manifest)?;
    atomic_write_text(&manifest_path, &manifest_body)?;
    factory_app_run_bundle_reject_secret_markers(&manifest_path, "skill-contract-manifest.json")?;
    let manifest_sha256 = sha256_file(&manifest_path)?;

    let mut response = manifest;
    response["manifest_path"] = serde_json::json!(manifest_path.display().to_string());
    response["manifest_sha256"] = serde_json::json!(manifest_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if json_output {
        println!("{response_body}");
    } else {
        println!("status=accepted");
        println!("schema_version=ao2.skill-contract-manifest.v1");
        println!("manifest={}", manifest_path.display());
        println!("manifest_sha256={}", response["manifest_sha256"]);
    }
    Ok(())
}

fn skill_contract_manifest_verify(
    manifest: PathBuf,
    manifest_sha256: String,
    json_output: bool,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let actual_sha256 = sha256_file(&manifest)?;
    if actual_sha256 != manifest_sha256.trim() {
        anyhow::bail!(
            "skill-contract manifest sha256 mismatch for {}: expected {}, actual {}",
            manifest.display(),
            manifest_sha256,
            actual_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(&manifest, "skill-contract-manifest.json")?;
    let body =
        fs::read_to_string(&manifest).with_context(|| format!("read {}", manifest.display()))?;
    let manifest_json: serde_json::Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", manifest.display()))?;
    validate_skill_contract_manifest(&manifest_json)?;

    let response = serde_json::json!({
        "schema_version": "ao2.skill-contract-manifest-verification.v1",
        "status": "passed",
        "producer": "ao2",
        "source_schema_version": json_string(&manifest_json, "schema_version"),
        "manifest_path": manifest.display().to_string(),
        "manifest_sha256": actual_sha256,
        "entry_count": json_array(&manifest_json, "entries").len(),
        "runtime_critical_guardrail_verified": true,
        "provider_auth": manifest_json.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
        "trust_boundary": manifest_json.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
        "side_effects": manifest_json.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
        "token_safe_output_verified": json_bool(&manifest_json, "token_safe_output_verified"),
        "factory_v3_role": json_string(&manifest_json, "factory_v3_role")
    });
    let response_body = serde_json::to_string_pretty(&response)?;
    if json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.skill-contract-manifest-verification.v1");
        println!("manifest={}", manifest.display());
    }
    Ok(())
}

struct SkillContractManifestEntrySpec<'a> {
    name: &'a str,
    source_relative_path: &'a str,
    category: &'a str,
    ao2_disposition: &'a str,
    enforcement: Option<(&'a str, &'a str, &'a str)>,
    blocker: Option<&'a str>,
    trust_boundary_notes: &'a str,
}

fn skill_contract_manifest_entry(
    factory_v3_root: &Path,
    spec: SkillContractManifestEntrySpec<'_>,
) -> Result<serde_json::Value> {
    let source_path = factory_v3_root.join(spec.source_relative_path);
    if !source_path.is_file() {
        anyhow::bail!(
            "skill-contract source path is missing for {}: {}",
            spec.name,
            source_path.display()
        );
    }
    let source_sha256 = sha256_file(&source_path)?;
    let enforcement = match spec.enforcement {
        Some((ao2_command, ao2_test, ao2_artifact)) => serde_json::json!({
            "ao2_command": ao2_command,
            "ao2_test": ao2_test,
            "ao2_artifact": ao2_artifact
        }),
        None => serde_json::json!({}),
    };
    Ok(serde_json::json!({
        "name": spec.name,
        "source_repo": "factory-v3",
        "source_path": source_path.display().to_string(),
        "source_relative_path": spec.source_relative_path,
        "source_sha256": source_sha256,
        "category": spec.category,
        "ao2_disposition": spec.ao2_disposition,
        "enforcement": enforcement,
        "blocker": spec.blocker,
        "trust_boundary_notes": spec.trust_boundary_notes
    }))
}

fn validate_skill_contract_manifest(manifest: &serde_json::Value) -> Result<()> {
    if json_string(manifest, "schema_version") != "ao2.skill-contract-manifest.v1" {
        anyhow::bail!(
            "skill-contract manifest requires ao2.skill-contract-manifest.v1, got {}",
            json_string(manifest, "schema_version")
        );
    }
    if json_string(manifest, "producer") != "ao2" {
        anyhow::bail!("skill-contract manifest producer must be ao2");
    }
    if json_string(manifest, "status") != "accepted" {
        anyhow::bail!("skill-contract manifest status must be accepted");
    }
    validate_plugin_provider_auth(
        manifest
            .get("provider_auth")
            .context("skill-contract manifest missing provider_auth")?,
        "skill-contract manifest",
    )?;
    validate_plugin_observer_trust_boundary(
        manifest
            .get("trust_boundary")
            .context("skill-contract manifest missing trust_boundary")?,
        "skill-contract manifest",
    )?;
    let side_effects = manifest
        .get("side_effects")
        .context("skill-contract manifest missing side_effects")?;
    for key in [
        "would_execute_provider",
        "would_execute_queue",
        "would_write_memory",
        "would_mutate_control_plane",
        "would_mutate_ao_artifacts",
        "would_approve_release",
    ] {
        if json_bool(side_effects, key) {
            anyhow::bail!("skill-contract manifest side effect {key} must be false");
        }
    }
    if !json_bool(manifest, "token_safe_output_verified") {
        anyhow::bail!("skill-contract manifest must verify token-safe output");
    }

    let entries = json_array(manifest, "entries");
    if entries.len() != SKILL_CONTRACT_REQUIRED_INVENTORY.len() {
        anyhow::bail!(
            "skill-contract manifest requires {} entries, got {}",
            SKILL_CONTRACT_REQUIRED_INVENTORY.len(),
            entries.len()
        );
    }
    let mut names = BTreeSet::new();
    for entry in entries {
        let name = json_string(entry, "name");
        if name.is_empty() {
            anyhow::bail!("skill-contract manifest contains unnamed entry");
        }
        if !names.insert(name.clone()) {
            anyhow::bail!("skill-contract manifest contains duplicate entry {name}");
        }
        let category = json_string(entry, "category");
        if ![
            "runtime_critical",
            "docs_reference_only",
            "plugin_packaging",
            "deprecated_or_not_needed",
        ]
        .contains(&category.as_str())
        {
            anyhow::bail!("skill-contract entry {name} has invalid category {category}");
        }
        let disposition = json_string(entry, "ao2_disposition");
        if !["enforced", "referenced", "blocked", "not_migrated"].contains(&disposition.as_str()) {
            anyhow::bail!("skill-contract entry {name} has invalid AO2 disposition {disposition}");
        }
        let source_path = json_string(entry, "source_path");
        if source_path.is_empty() {
            anyhow::bail!("skill-contract entry {name} missing source path");
        }
        let source_sha256 = json_string(entry, "source_sha256");
        if source_sha256.len() != 64 || !source_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("skill-contract entry {name} source sha256 must be a hex digest");
        }
        let source_path_ref = Path::new(&source_path);
        if source_path_ref.is_file() {
            let actual = sha256_file(source_path_ref)?;
            if actual != source_sha256 {
                anyhow::bail!(
                    "skill-contract entry {name} source sha256 mismatch: expected {}, actual {}",
                    source_sha256,
                    actual
                );
            }
        }
        if json_string(entry, "trust_boundary_notes").is_empty() {
            anyhow::bail!("skill-contract entry {name} missing trust-boundary notes");
        }
        if category == "runtime_critical" {
            let enforcement = entry
                .get("enforcement")
                .and_then(serde_json::Value::as_object);
            let has_enforcement = enforcement
                .map(|enforcement| {
                    ["ao2_command", "ao2_test", "ao2_artifact"]
                        .iter()
                        .all(|key| {
                            enforcement
                                .get(*key)
                                .and_then(serde_json::Value::as_str)
                                .map(|text| !text.trim().is_empty())
                                .unwrap_or(false)
                        })
                })
                .unwrap_or(false);
            let has_blocker = entry
                .get("blocker")
                .and_then(serde_json::Value::as_str)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false);
            if !has_enforcement && !has_blocker {
                anyhow::bail!(
                    "runtime-critical skill-contract entry {name} lacks enforcement or blocker"
                );
            }
        }
    }
    for required in SKILL_CONTRACT_REQUIRED_INVENTORY {
        if !names.contains(required) {
            anyhow::bail!("skill-contract manifest missing required entry {required}");
        }
    }
    Ok(())
}

fn is_git_sha_prefix(value: &str) -> bool {
    (7..=40).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_release_gate_with_replacement_rollup(
    rollup: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(rollup, "schema_version") != "ao2.release-gate-with-replacement-parity.v1" {
        anyhow::bail!(
            "{platform} release-gate rollup requires ao2.release-gate-with-replacement-parity.v1, got {}",
            json_string(rollup, "schema_version")
        );
    }
    if json_string(rollup, "overall_verdict") != "PASS" {
        anyhow::bail!("{platform} release-gate rollup must be PASS");
    }
    let ao2_git_head = json_string(rollup, "ao2_git_head");
    if !is_git_sha_prefix(&ao2_git_head) || ao2_git_head.len() != 40 {
        anyhow::bail!("{platform} release-gate rollup ao2_git_head must be a full git sha");
    }
    let counts = rollup
        .get("counts")
        .context("release-gate rollup missing counts")?;
    let passed = json_u64(counts, "passed");
    let total = json_u64(counts, "total_stages");
    if passed == 0 || passed != total || json_u64(counts, "non_passed") != 0 {
        anyhow::bail!("{platform} release-gate rollup counts must be all passing");
    }
    let stages = rollup
        .get("stages")
        .and_then(serde_json::Value::as_array)
        .context("release-gate rollup missing stages")?;
    if stages.len() as u64 != total {
        anyhow::bail!("{platform} release-gate rollup stage count does not match totals");
    }
    if !stages
        .iter()
        .all(|stage| stage.get("status").and_then(serde_json::Value::as_str) == Some("PASS"))
    {
        anyhow::bail!("{platform} release-gate rollup has a non-PASS stage");
    }
    if !stages.iter().any(|stage| {
        stage.get("name").and_then(serde_json::Value::as_str) == Some("replacement_parity")
    }) {
        anyhow::bail!("{platform} release-gate rollup must include replacement_parity stage");
    }
    let trust = rollup
        .get("trust_boundary")
        .context("release-gate rollup missing trust_boundary")?;
    if json_string(trust, "ao2_role") != "canonical_producer"
        || json_string(trust, "factory_v3_role") != "parity_oracle_only"
        || json_bool(trust, "mutates_ao_artifacts")
        || json_bool(trust, "mutates_control_plane")
    {
        anyhow::bail!("{platform} release-gate rollup trust boundary is not observer-safe");
    }
    Ok(())
}

fn run_current_ao2_json_command(args: &[String]) -> Result<serde_json::Value> {
    let output =
        ProcessCommand::new(std::env::current_exe().context("resolve current ao2 binary")?)
            .args(args)
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .with_context(|| format!("run ao2 {}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        anyhow::bail!(
            "ao2 {} failed with exit code {:?}: {}",
            args.join(" "),
            output.status.code(),
            redact_secrets(&stderr)
        );
    }
    let value: serde_json::Value = serde_json::from_str(&stdout)
        .with_context(|| format!("parse JSON from ao2 {}", args.join(" ")))?;
    reject_secret_markers_in_bytes(stdout.as_bytes(), "ao2-json-command-stdout")?;
    reject_secret_markers_in_bytes(stderr.as_bytes(), "ao2-json-command-stderr")?;
    Ok(value)
}

fn cockpit(command: CockpitCommand) -> Result<()> {
    match command {
        CockpitCommand::Index { target, out, open } => cockpit_index(target, out, open),
        CockpitCommand::Serve {
            run_id,
            target,
            host,
            port,
            index,
            once,
        } => serve_cockpit(target, run_id, host, port, index, once),
    }
}

fn workbench(command: WorkbenchCommand) -> Result<()> {
    match command {
        WorkbenchCommand::Export {
            target,
            out,
            open,
            provenance_dir,
        } => {
            let html = render_workbench(
                &target,
                &provenance_dir,
                WorkbenchRenderOptions {
                    operator: None,
                    execution_enabled: false,
                    can_operate: false,
                    release_comparison_signing_enabled: false,
                    control_plane_url: None,
                    release_gate_artifact_path: None,
                },
            )?;
            workbench_export(target, out, open, html)
        }
        WorkbenchCommand::Serve {
            target,
            host,
            port,
            once,
            provenance_dir,
            api_token,
            operator_tokens,
            enable_execution,
            queue_retention,
            control_plane_url,
            support_signing_key,
            support_signer_id,
        } => serve_workbench(ServeWorkbenchOptions {
            target,
            host,
            port,
            once,
            provenance_dir,
            api_token,
            operator_tokens,
            enable_execution,
            queue_retention,
            control_plane_url,
            support_signing_key,
            support_signer_id,
        }),
        WorkbenchCommand::SupportVerify { bundle_dir, json } => {
            workbench_support_bundle_verify(bundle_dir, json)
        }
        WorkbenchCommand::SupportImport {
            bundle_dir,
            out_dir,
            json,
        } => workbench_support_bundle_import(bundle_dir, out_dir, json),
        WorkbenchCommand::SupportInspect { bundle_dir, json } => {
            workbench_support_bundle_inspect(bundle_dir, json)
        }
        WorkbenchCommand::SupportKeygen { out, bits, json } => {
            workbench_support_keygen(out, bits, json)
        }
    }
}

pub(crate) fn query_value_owned(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (query_key, value) = part.split_once('=')?;
        (query_key == key)
            .then(|| percent_decode(value).trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

pub(crate) fn form_value_owned(
    form: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Option<String> {
    form.get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn shell_quote(input: &str) -> String {
    if input
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        input.to_string()
    } else {
        format!("'{}'", input.replace('\'', "'\\''"))
    }
}

fn format_budget_usd(max_budget_usd: f64) -> Result<String> {
    if !max_budget_usd.is_finite() || max_budget_usd <= 0.0 {
        anyhow::bail!("provider max budget USD must be a positive finite number");
    }
    Ok(format!("{max_budget_usd:.2}"))
}

fn http_html_response(html: String) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    )
}

fn http_json_response(status: u16, json: serde_json::Value) -> Result<String> {
    let body = serde_json::to_string_pretty(&json)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "OK",
    };
    Ok(format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    ))
}

fn http_text_response(status: u16, reason: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn render_workbench_job_detail_page(detail: &serde_json::Value) -> String {
    let job = &detail["job"];
    let run_id = json_string(job, "run_id");
    let status = json_string(job, "status");
    let evidence_pack = json_string(job, "evidence_pack");
    let cockpit = json_string(job, "cockpit");
    let stdout = detail["stdout"].as_str().unwrap_or("");
    let stderr = detail["stderr"].as_str().unwrap_or("");
    let diagnosis = &detail["diagnosis"];
    let recovery_actions = json_array(diagnosis, "recovery_actions")
        .iter()
        .map(|action| format!("<li>{}</li>", escape_html(action.as_str().unwrap_or(""))))
        .collect::<Vec<_>>()
        .join("");
    let evidence_link = workbench_file_anchor("Open Evidence", &evidence_pack);
    let cockpit_link = workbench_file_anchor("Open Cockpit", &cockpit);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>AO2 Queue Job {run_id}</title>
  <style>
    body {{ margin: 0; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; color: #18202f; background: #f6f7f9; }}
    main {{ max-width: 1120px; margin: 0 auto; padding: 32px 20px 48px; }}
    h1 {{ margin: 0 0 4px; font-size: 30px; line-height: 1.15; }}
    .muted {{ color: #5f6b7a; font-size: 14px; }}
    .toolbar {{ display: flex; gap: 10px; margin: 18px 0 24px; flex-wrap: wrap; }}
    .toolbar a {{ border: 1px solid #cbd3dc; border-radius: 6px; color: #152238; padding: 8px 10px; text-decoration: none; background: #fff; }}
    .metrics {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 10px; margin: 18px 0; }}
    .metric {{ background: #fff; border: 1px solid #dbe1e8; border-radius: 8px; padding: 12px; }}
    .metric span {{ display: block; color: #596677; font-size: 12px; margin-bottom: 6px; }}
    .metric strong {{ font-size: 18px; }}
    pre {{ background: #111827; color: #f3f4f6; border-radius: 8px; padding: 14px; overflow: auto; white-space: pre-wrap; min-height: 80px; }}
    .diagnosis {{ background: #fff; border: 1px solid #d8e0ea; border-radius: 8px; padding: 14px; }}
    .diagnosis ul {{ margin: 8px 0 0; padding-left: 20px; }}
    section {{ margin-top: 24px; }}
  </style>
</head>
<body>
  <main class="queue-detail-page">
    <h1>{run_id}</h1>
    <div class="muted">Job {job_id} / {status}</div>
    <div class="toolbar">{evidence_link}{cockpit_link}</div>
    <section class="metrics" aria-label="Runtime metrics">
      <div class="metric"><span>Queue Wait</span><strong>{queue_wait_ms} ms</strong></div>
      <div class="metric"><span>Duration</span><strong>{duration_ms} ms</strong></div>
      <div class="metric"><span>Exit Code</span><strong>{exit_code}</strong></div>
      <div class="metric"><span>Retry Count</span><strong>{retry_count}</strong></div>
    </section>
    <section class="diagnosis">
      <h2>Failure Diagnosis</h2>
      <div class="muted">kind={failure_kind} timed_out={timed_out}</div>
      <p>{primary_error}</p>
      <ul>{recovery_actions}</ul>
      <h3>Stderr Excerpt</h3>
      <pre>{stderr_excerpt}</pre>
      <h3>Stdout Excerpt</h3>
      <pre>{stdout_excerpt}</pre>
    </section>
    <section>
      <h2>Stdout</h2>
      <pre>{stdout}</pre>
    </section>
    <section>
      <h2>Stderr</h2>
      <pre>{stderr}</pre>
    </section>
  </main>
</body>
</html>"#,
        run_id = escape_html(&run_id),
        job_id = escape_html(&json_string(job, "job_id")),
        status = escape_html(&status),
        evidence_link = evidence_link,
        cockpit_link = cockpit_link,
        queue_wait_ms = json_u64(job, "queue_wait_ms"),
        duration_ms = json_u64(job, "duration_ms"),
        exit_code = job["exit_code"].as_i64().unwrap_or(-1),
        retry_count = json_u64(job, "retry_count"),
        failure_kind = escape_html(&json_string(diagnosis, "failure_kind")),
        timed_out = diagnosis
            .get("timed_out")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        primary_error = escape_html(&json_string(diagnosis, "primary_error")),
        recovery_actions = recovery_actions,
        stderr_excerpt = escape_html(&json_string(diagnosis, "stderr_excerpt")),
        stdout_excerpt = escape_html(&json_string(diagnosis, "stdout_excerpt")),
        stdout = escape_html(stdout),
        stderr = escape_html(stderr)
    )
}

fn workbench_file_anchor(label: &str, path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    format!(
        r#"<a href="file://{href}">{label}</a>"#,
        href = escape_html(path),
        label = escape_html(label)
    )
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'+' {
            output.push(b' ');
            index += 1;
        } else if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &input[index + 1..index + 3];
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                output.push(value);
                index += 3;
            } else {
                output.push(bytes[index]);
                index += 1;
            }
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn percent_encode(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn generate_api_token() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{now:x}{pid:x}")
}

fn adapter(command: AdapterCommand) -> Result<()> {
    match command {
        AdapterCommand::Doctor { provider } => {
            let provider = parse_provider(&provider)?;
            let report = doctor_provider(provider)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        AdapterCommand::Run {
            provider,
            target,
            command,
            args,
            role_id,
            keep_sandbox,
            timeout_seconds,
        } => {
            let provider = parse_provider(&provider)?;
            let adapter = LocalCliAdapter::new(provider);
            let result = adapter.run_in_sandbox(SandboxRunRequest {
                target_repo: target,
                keep_sandbox,
                request: AdapterRunRequest {
                    role_id,
                    command,
                    args: split_tab_args(&args),
                    working_dir: PathBuf::from("."),
                    stdin: None,
                    timeout_ms: Some(timeout_seconds * 1_000),
                },
            })?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        AdapterCommand::Prompt {
            provider,
            target,
            prompt,
            prompt_file,
            role_id,
            keep_sandbox,
            timeout_seconds,
            max_budget_usd,
        } => {
            let provider = parse_provider(&provider)?;
            let prompt = read_prompt(prompt, prompt_file)?;
            let result = run_provider_prompt_in_sandbox(ProviderPromptRequest {
                provider,
                target_repo: target,
                prompt,
                role_id,
                keep_sandbox,
                timeout_ms: Some(timeout_seconds * 1_000),
                max_budget_usd,
            })?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        AdapterCommand::Patch { command } => adapter_patch(command),
    }
}

fn adapter_patch(command: AdapterPatchCommand) -> Result<()> {
    match command {
        AdapterPatchCommand::Preview { target, sandbox } => {
            let preview = preview_sandbox_patch(&target, &sandbox)?;
            println!("{}", serde_json::to_string_pretty(&preview)?);
            Ok(())
        }
        AdapterPatchCommand::Apply {
            target,
            sandbox,
            digest,
            approver,
        } => {
            let preview = preview_sandbox_patch(&target, &sandbox)?;
            let applied = apply_sandbox_patch(SandboxPatchApplyRequest {
                target_repo: target,
                sandbox_path: sandbox,
                expected_subject: preview.approval_subject,
                expected_digest: digest,
                approver,
            })?;
            println!("{}", serde_json::to_string_pretty(&applied)?);
            Ok(())
        }
    }
}

fn split_tab_args(args: &str) -> Vec<String> {
    if args.is_empty() {
        Vec::new()
    } else {
        args.split('\t').map(str::to_string).collect()
    }
}

fn export(target: PathBuf, run_id: String) -> Result<()> {
    let path = target
        .join(".ao2")
        .join("runs")
        .join(&run_id)
        .join("evidence-pack")
        .join("evidence-pack.json");
    if !path.exists() {
        anyhow::bail!("evidence pack not found: {}", path.display());
    }
    println!("{}", path.display());
    Ok(())
}

fn version(json: bool) -> Result<()> {
    let target = runtime_target_label();
    let git_commit = runtime_git_commit();
    if json {
        let version = serde_json::json!({
            "package": "ao2",
            "version": env!("CARGO_PKG_VERSION"),
            "target": target,
            "git_commit": git_commit,
            "build_profile": option_env!("AO2_BUILD_PROFILE").unwrap_or("unknown"),
            "release_manifest_schema": "ao2.release-manifest.v1",
            "release_provenance_schema": "ao2.release-provenance.v1"
        });
        println!("{}", serde_json::to_string_pretty(&version)?);
    } else {
        println!("ao2 {}", env!("CARGO_PKG_VERSION"));
        println!("target={target}");
        println!("git_commit={git_commit}");
    }
    Ok(())
}

fn runtime_git_commit() -> String {
    option_env!("AO2_GIT_COMMIT")
        .unwrap_or("unknown")
        .to_string()
}

fn release(command: ReleaseCommand) -> Result<()> {
    match command {
        ReleaseCommand::Package {
            out_dir,
            version,
            binary,
            target_label,
        } => package_release(out_dir, version, binary, target_label),
        ReleaseCommand::SmokeSummary {
            summary,
            require_native_windows,
        } => release_smoke_summary(summary, require_native_windows),
        ReleaseCommand::SummaryEnrich {
            summary,
            target,
            run_id,
            obligation_gates,
            out,
            json,
        } => release_summary_enrich(summary, target, run_id, obligation_gates, out, json),
        ReleaseCommand::Gate {
            summary,
            provenance_dir,
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            require_native_windows,
            replacement_smoke_gate,
            greenfield_three_os_smoke_gate,
            governed_run_evidence,
            factory_project_run_summaries,
            allow_unsigned_obligation_gates,
            require_obligation_gate_signing: _legacy_require_obligation_gate_signing,
        } => release_gate(
            summary,
            provenance_dir,
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            require_native_windows,
            replacement_smoke_gate,
            greenfield_three_os_smoke_gate,
            governed_run_evidence,
            factory_project_run_summaries,
            !allow_unsigned_obligation_gates,
        ),
        ReleaseCommand::Compare {
            release_download_dir,
            out_dir,
            signing_key,
            signer_id,
            json,
        } => release_compare(release_download_dir, out_dir, signing_key, signer_id, json),
        ReleaseCommand::CompareVerify { bundle_dir, json } => {
            release_compare_verify(bundle_dir, json)
        }
        ReleaseCommand::SupportBundleBuild {
            release_assembly,
            readiness,
            handoff,
            cockpit,
            evaluator_decision,
            storage_support,
            replay,
            report_contract_verification,
            install_verification,
            hosted_release_smoke,
            report_target,
            report_run_id,
            report,
            report_index,
            operator_evidence,
            out_dir,
            json,
        } => release_support_bundle_build(
            release_assembly,
            readiness,
            handoff,
            cockpit,
            evaluator_decision,
            storage_support,
            replay,
            report_contract_verification,
            install_verification,
            hosted_release_smoke,
            report_target,
            report_run_id,
            report,
            report_index,
            operator_evidence,
            out_dir,
            json,
        ),
        ReleaseCommand::SupportBundleVerify {
            bundle,
            checksums,
            json,
        } => release_support_bundle_verify(bundle, checksums, json),
        ReleaseCommand::EvidenceBundle {
            out_dir,
            artifacts,
            json,
        } => {
            let result = release_evidence_bundle_json(out_dir, &artifacts)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("archive={}", json_string(&result, "archive"));
                println!("sha256={}", json_string(&result, "sha256"));
                println!("artifact_count={}", json_u64(&result, "artifact_count"));
            }
            Ok(())
        }
        ReleaseCommand::EvidenceBundleVerify { bundle, json } => {
            let report = release_evidence_bundle_verification_json(&bundle)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "release_evidence_bundle_verification={}",
                    json_string(&report, "status")
                );
                println!("bundle={}", bundle.display());
                println!(
                    "manifest_verified={}",
                    report["manifest_verified"].as_bool().unwrap_or(false)
                );
                println!(
                    "trust_boundary_verified={}",
                    report["trust_boundary_verified"].as_bool().unwrap_or(false)
                );
                println!(
                    "secret_scan_passed={}",
                    report["secret_scan_passed"].as_bool().unwrap_or(false)
                );
                println!("failure_count={}", json_u64(&report, "failure_count"));
            }
            if json_string(&report, "status") != "verified" {
                anyhow::bail!("release evidence bundle verification failed");
            }
            Ok(())
        }
        ReleaseCommand::Phase1DecisionBuild {
            release_gate,
            replacement_smoke_gate,
            governed_run_evidence,
            factory_project_run_summaries,
            provider_acceptance_preservation,
            operator,
            rationale,
            out,
            checklist_out,
            json,
        } => {
            let result = phase1_promotion_decision_build_json(
                &release_gate,
                replacement_smoke_gate.as_deref(),
                &governed_run_evidence,
                &factory_project_run_summaries,
                provider_acceptance_preservation.as_deref(),
                &operator,
                &rationale,
                &out,
                checklist_out.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("decision={}", json_string(&result, "decision_path"));
                println!("checklist={}", json_string(&result, "checklist_path"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1DecisionPublish {
            decision,
            signing_key,
            signer_id,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let api_token = resolve_api_token(api_token.as_deref(), api_token_env.as_deref())?;
            let result = phase1_promotion_decision_publish_to_control_plane_json(
                &decision,
                &signing_key,
                &signer_id,
                &control_plane_url,
                &api_token,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("decision={}", json_string(&result, "decision_path"));
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
                println!("signature_url={}", json_string(&result, "signature_url"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1ThreeOsSmokeBuild {
            summary,
            provenance,
            out,
            json,
        } => {
            let result = phase1_three_os_smoke_build_json(&summary, &provenance, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("smoke={}", json_string(&result, "smoke_path"));
                println!("summary={}", json_string(&result, "summary_path"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1ThreeOsSmokePublish {
            smoke,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let result = phase1_three_os_smoke_publish_to_control_plane_json(
                &smoke,
                &control_plane_url,
                api_token.as_deref(),
                api_token_env.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("smoke={}", json_string(&result, "smoke_path"));
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1HistoryFetch {
            control_plane_url,
            api_token,
            api_token_env,
            out,
            json,
        } => {
            let result = phase1_promotion_history_fetch_from_control_plane_json(
                &control_plane_url,
                api_token.as_deref(),
                api_token_env.as_deref(),
                out.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("dashboard={}", json_string(&result, "dashboard_url"));
                if let Some(path) = out {
                    println!("out={}", path.display());
                }
                println!(
                    "checklists={}",
                    json_u64(&result["history"]["counts"], "checklists")
                );
                println!(
                    "signed_decisions={}",
                    json_u64(&result["history"]["counts"], "signed_decisions")
                );
                println!(
                    "three_os_smokes={}",
                    json_u64(&result["history"]["counts"], "three_os_smokes")
                );
            }
            Ok(())
        }
        ReleaseCommand::Phase1PromotionStatus {
            root,
            evidence_bundle,
            json,
        } => {
            let result = phase1_promotion_status_json(&root, evidence_bundle.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("root={}", root.display());
                println!(
                    "release_gate={}",
                    json_string(&result["artifacts"], "release_gate")
                );
                println!("decision={}", json_string(&result["artifacts"], "decision"));
                println!(
                    "checklist={}",
                    json_string(&result["artifacts"], "checklist")
                );
                println!(
                    "evidence_bundle={}",
                    json_string(&result["artifacts"], "evidence_bundle")
                );
                println!(
                    "dashboard_snapshot={}",
                    json_string(&result["checks"], "dashboard_snapshot")
                );
                println!(
                    "dashboard_snapshot_index={}",
                    json_string(&result["artifacts"], "dashboard_snapshot_index")
                );
                println!("failure_count={}", json_u64(&result, "failure_count"));
            }
            if json_string(&result, "status") != "ready" {
                anyhow::bail!("Phase 1 promotion status is not ready");
            }
            Ok(())
        }
        ReleaseCommand::Phase1PromotionInputsVerify {
            manifest,
            out,
            mode,
            json,
        } => {
            let result = phase1_promotion_inputs_verify_json(&manifest, out.as_deref(), &mode)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("manifest={}", manifest.display());
                if let Some(path) = out {
                    println!("out={}", path.display());
                }
                println!(
                    "missing_required_inputs={}",
                    result["missing_required_inputs"]
                        .as_array()
                        .map(|items| items.len())
                        .unwrap_or(0)
                );
            }
            if json_string(&result, "status") != "accepted" {
                anyhow::bail!("Phase 1 promotion inputs verification failed");
            }
            Ok(())
        }
        ReleaseCommand::Phase1PromotionInputsPublish {
            verification,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let result = phase1_promotion_inputs_publish_to_control_plane_json(
                &verification,
                &control_plane_url,
                api_token.as_deref(),
                api_token_env.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("verification={}", json_string(&result, "verification_path"));
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
            }
            Ok(())
        }
        ReleaseCommand::HandoffChecklistBuild {
            handoff,
            write_json,
            write_md,
            expected_repo_head,
            allow_skipped,
            json,
        } => {
            let payload =
                release_handoff_checklist_build(&handoff, &expected_repo_head, allow_skipped)?;
            if let Some(path) = write_json.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let mut text = serde_json::to_string_pretty(&payload)?;
                text.push('\n');
                fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
            }
            if let Some(path) = write_md.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let md = release_handoff_checklist_markdown(&payload);
                fs::write(path, md).with_context(|| format!("write {}", path.display()))?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                println!("{}", json_string(&payload, "status"));
            }
            Ok(())
        }
        ReleaseCommand::EvaluatorDecisionBuild {
            readiness,
            handoff_checklist,
            support_bundle_status,
            write_json,
            write_md,
            json,
        } => {
            let payload = release_evaluator_decision_build(
                &readiness,
                &handoff_checklist,
                &support_bundle_status,
            )?;
            if let Some(path) = write_json.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let mut text = serde_json::to_string_pretty(&payload)?;
                text.push('\n');
                fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
            }
            if let Some(path) = write_md.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let md = release_evaluator_decision_markdown(&payload);
                fs::write(path, md).with_context(|| format!("write {}", path.display()))?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                println!("{}", json_string(&payload, "status"));
            }
            Ok(())
        }
        ReleaseCommand::SignProvenance {
            version,
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            private_key,
            release_tag,
            json,
        } => release_sign_provenance(
            version,
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            private_key,
            release_tag,
            json,
        ),
        ReleaseCommand::VerifyProvenance {
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            public_key,
            json,
        } => release_verify_provenance(
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            public_key,
            json,
        ),
    }
}

pub(crate) fn runtime_target_label() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod unix_ms_conversion_tests {
    use crate::cli_util::unix_ms_from_duration;
    use std::time::Duration;

    #[test]
    fn saturates_millisecond_values_that_exceed_u64() {
        let duration = Duration::new(u64::MAX, 999_999_999);

        assert_eq!(unix_ms_from_duration(duration), u64::MAX);
    }
}

#[cfg(test)]
mod http_request_line_tests {
    use super::{parse_http_request_line, query_value_owned, split_path_query};

    #[test]
    fn preserves_spaces_inside_request_target() {
        let request_target = "/api/factory/project-start/next-action?token=viewer-token&out_dir=C:\\ao2-public-test\\AI Agent Teams\\ao2\\out&contract=C:\\ao2-public-test\\AI Agent Teams\\ao2\\docs\\contract.json";
        let request_line = format!("GET {request_target} HTTP/1.1");
        let (method, raw_path) = parse_http_request_line(&request_line);
        let (path, query) = split_path_query(raw_path);

        assert_eq!(method, "GET");
        assert_eq!(raw_path, request_target);
        assert_eq!(path, "/api/factory/project-start/next-action");
        assert_eq!(
            query_value_owned(query, "out_dir").as_deref(),
            Some("C:\\ao2-public-test\\AI Agent Teams\\ao2\\out")
        );
        assert_eq!(
            query_value_owned(query, "contract").as_deref(),
            Some("C:\\ao2-public-test\\AI Agent Teams\\ao2\\docs\\contract.json")
        );
    }

    #[test]
    fn still_handles_standard_request_lines() {
        let (method, raw_path) = parse_http_request_line("POST /api/queue/retry?token=t HTTP/1.1");

        assert_eq!(method, "POST");
        assert_eq!(raw_path, "/api/queue/retry?token=t");
    }
}

#[cfg(test)]
mod workbench_log_excerpt_tests {
    use super::workbench_log_excerpt;

    #[test]
    fn returns_trimmed_input_when_within_limit() {
        assert_eq!(workbench_log_excerpt("  hello \n"), "hello");
    }

    #[test]
    fn keeps_a_valid_suffix_without_panicking_on_a_multibyte_boundary() {
        // 700 three-byte chars = 2100 bytes. The naive `len - 2000` cut lands at
        // byte 100, which is mid-codepoint (not a multiple of 3) and panics when
        // sliced directly. The excerpt must stay on a char boundary.
        let input = "€".repeat(700);
        let excerpt = workbench_log_excerpt(&input);
        assert!(excerpt.starts_with("..."), "excerpt should be elided");
        let tail = &excerpt[3..];
        assert!(
            input.ends_with(tail),
            "excerpt tail must be a suffix of the original input"
        );
        assert!(
            !tail.is_empty() && tail.chars().all(|c| c == '€'),
            "excerpt tail must contain only whole, undamaged characters"
        );
    }
}

#[cfg(test)]
mod atomic_write_text_tests {
    use super::atomic_write_text;
    use super::plugin_distribution::plugin_package_normalized_archive_path;
    use std::fs;
    use std::path::Path;

    #[test]
    fn round_trips_content_and_creates_missing_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("nested").join("run-record.json");
        atomic_write_text(&target, "{\"k\":1}").expect("write");
        assert_eq!(fs::read_to_string(&target).expect("read"), "{\"k\":1}");
    }

    #[test]
    fn overwrite_replaces_previous_content_completely() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("f.txt");
        atomic_write_text(&target, "longer original content").expect("write1");
        atomic_write_text(&target, "short").expect("write2");
        assert_eq!(fs::read_to_string(&target).expect("read"), "short");
    }

    #[test]
    fn failed_write_leaves_no_temp_litter_beside_target() {
        // A durable atomic writer must clean up its temp file when the rename
        // cannot complete, so a failed/crashed write never strews half-written
        // temporaries beside the evidence file. Force the rename to fail by
        // making the target an existing directory (a regular file cannot be
        // renamed onto a directory), then assert nothing but that directory
        // remains in the parent.
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("occupied");
        fs::create_dir(&target).expect("make target a directory");

        let result = atomic_write_text(&target, "payload");
        assert!(result.is_err(), "writing a file over a directory must fail");

        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .expect("read parent")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .filter(|name| name != "occupied")
            .collect();
        assert!(
            leftovers.is_empty(),
            "failed atomic write left temp litter: {leftovers:?}"
        );
    }

    #[test]
    fn plugin_package_archive_paths_are_platform_neutral() {
        let normalized =
            plugin_package_normalized_archive_path(Path::new("manifest\\ao2-plugin-manifest.json"))
                .expect("normalize windows-style tar path");
        assert_eq!(normalized, "manifest/ao2-plugin-manifest.json");
    }
}
