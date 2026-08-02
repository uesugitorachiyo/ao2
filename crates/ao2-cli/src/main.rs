#![recursion_limit = "256"]
use anyhow::{anyhow, Context, Result};
use ao2_adapters::{build_provider_prompt_command, doctor_provider, parse_provider};
use ao2_policy::redact_secrets;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write as IoWrite};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
mod artifact_safety;
mod build_identity;
mod cli;
mod cli_util;
mod contract_dispatch;
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
mod release_dispatch;
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
mod state_commands;
mod support_bundle;
mod template_commands;
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
use build_identity::{runtime_git_commit, runtime_target_label, version};
use cli::{quality::quality, Cli, Command, ReportCommand};
use cli_util::{
    atomic_write_text, canonical_json_sha256, create_tar_gz, is_git_sha_prefix, is_sha256_hex,
    json_array, json_bool, json_string, json_u64, json_value_text, now_unix_ms, read_json_file,
    sha256_bytes_hex, sha256_file, trimmed_required,
};
use contract_dispatch::contract;
use control_plane_ops::{
    control_plane, render_workbench_queue_failure_diagnostics_table,
    render_workbench_redaction_audit_section, workbench_support_evidence_export_subject,
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
use factory_queue::{
    factory_queue_load, factory_queue_path, factory_queue_project_start_completion_summary_json,
};
use git_cmd::git;
use github_issue_intake::issue;
use greenfield_workflow::greenfield;
use install_cmd::install;
use memory_store::memory;
use plugin_cli::plugin;
use plugin_distribution::validate_plugin_observer_trust_boundary;
use provider_contract::{provider_contract_json, provider_contract_verify_json};
use provider_ops::{
    adapter, provider, provider_matrix_json, provider_profiles, provider_smoke_all_json,
    provider_warning_strings,
};
use pulse_run::pulse;
use release_crypto::copy_dir_recursive;
use release_dispatch::release;
use release_provenance::ensure_rsa_private_key;
use release_summary::resolve_cli_artifact_reference;
use run_execution::{run, CliRunOptions};
use run_reporting::{cockpit, report, report_verify, runs};
use run_resume::{approve, repair, replay};
use skill_contract_manifest::skill_contract_manifest;
use state_commands::{export, init, status};
use template_commands::template;
pub(crate) use template_commands::TASK_TEMPLATES;
use upgrade_cmd::upgrade;
use workbench_app::workbench;
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
use workbench_run_evidence::{
    workbench_run_evidence_changes_json, workbench_run_evidence_diff_json,
    workbench_run_evidence_summary_json,
};
use workbench_server::{parse_http_request_line, split_path_query};
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
        Command::Quality(args) => quality(args.command),
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
