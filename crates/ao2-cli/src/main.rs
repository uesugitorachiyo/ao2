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

use anyhow::{anyhow, Context, Result};
use ao2_adapters::{
    apply_sandbox_patch, build_provider_prompt_command, doctor_provider, parse_provider,
    preview_sandbox_patch, run_provider_prompt_in_sandbox, AdapterRunRequest, LocalCliAdapter,
    ProviderPromptRequest, SandboxPatchApplyRequest, SandboxRunRequest,
};
use ao2_core::{
    annotate_obligation_ledger, check_obligation_ledger, extract_obligation_ledger,
    ObligationEvidence, ObligationLedger, ObligationStatus,
};
use ao2_policy::redact_secrets;
use ao2_runtime::{run_risky_pr_with_provider_prompt, ProviderRunOptions, RepairSourceContext};
use chrono::{SecondsFormat, Utc};
use clap::Parser;
use serde::{Deserialize, Serialize};

mod artifact_safety;
mod cli;
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
mod factory_dispatch;
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
mod skill_contract_manifest;
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
use cli::{
    AdapterCommand, AdapterPatchCommand, Cli, CockpitCommand, Command, ContractCommand,
    GreenfieldCommand, PulseCommand, PulseEvalLoopCommand, ReleaseCommand, RepairCommand,
    ReportCommand, RunsCommand, TemplateCommand, WorkbenchCommand,
};
use cli_util::{
    atomic_write_text, canonical_json_sha256, create_tar_gz, json_array, json_bool, json_string,
    json_u64, json_value_text, now_unix_ms, read_json_file, read_prompt, sha256_bytes_hex,
    sha256_file, trimmed_required,
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
use evidence_publish::evidence;
use factory_compat::{
    classify_factory_shape, classify_factory_size, factory_classification_signals,
    factory_ensure_target_repo, reject_factory_provider_api_key_auth,
};
use factory_dispatch::factory;
use factory_governance::greenfield_three_os_smoke_gate_json;
use factory_queue::{
    factory_queue_load, factory_queue_path, factory_queue_project_start_completion_summary_json,
};
use git_cmd::git;
use github_issue_intake::issue;
use greenfield_workflow::{
    greenfield_governed_run_json, greenfield_ingest_json, GreenfieldGovernedRunOptions,
    GreenfieldIngestOptions,
};
use install_cmd::install;
use memory_store::memory;
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
};
use provider_contract::{provider_contract_json, provider_contract_verify_json};
use provider_ops::{
    materialize_template_workflow, provider, provider_matrix_json, provider_profiles,
    provider_profiles_json, provider_smoke_all_json, provider_warning_strings,
};
use pulse_eval_loop::{
    pulse_eval_loop_handoff_json, pulse_eval_loop_run_chain_json, pulse_eval_loop_run_once_json,
};
use pulse_run::{
    pulse_artifact_key, pulse_run_apply_dry_run_json, pulse_run_chain_json, pulse_run_execute_json,
    pulse_run_once_json,
};
use release_comparison::{
    release_compare, release_compare_verify, release_evidence_bundle_json,
    release_evidence_bundle_verification_json, release_support_bundle_build,
    release_support_bundle_verify,
};
use release_crypto::copy_dir_recursive;
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
use run_resume::{approve, replay};
use skill_contract_manifest::skill_contract_manifest;
use upgrade_cmd::upgrade;
use workbench_app::workbench_export;
use workbench_contract::WorkbenchSupportSigning;
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
use workbench_memory::{
    workbench_memory_control_plane_dashboard_json, workbench_memory_export_json,
    workbench_memory_link_run_json, workbench_memory_publish_latest_json,
    workbench_memory_recent_json, workbench_memory_search_json,
};
use workbench_queue::*;
use workbench_release::{
    workbench_release_comparison_json, workbench_release_comparison_verification_json,
    workbench_release_gate_artifact_json, workbench_release_gate_json,
    workbench_release_health_json, workbench_release_history_json,
    workbench_release_retention_prune_json, workbench_release_summary_enrich_json,
};
use workbench_release_latest::workbench_latest_release_comparison_json;
use workbench_render::{render_workbench, WorkbenchRenderOptions};
use workbench_run_evidence::{
    workbench_run_evidence_changes_json, workbench_run_evidence_diff_json,
    workbench_run_evidence_summary_json,
};
use workbench_server::{
    parse_http_request_line, serve_cockpit, serve_workbench, split_path_query,
    ServeWorkbenchOptions,
};
use workbench_support::{
    workbench_evidence_exports_for_support_bundle, workbench_support_bundle_path,
    workbench_support_bundle_redaction_audit, workbench_support_bundle_redaction_preview,
};

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
    use super::{parse_http_request_line, split_path_query};
    use crate::cli_util::query_value_owned;

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
