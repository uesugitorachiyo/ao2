use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};

use super::cli_util::json_u64;
use super::plugin_adapter::{
    validate_plugin_control_plane_observation, validate_plugin_side_effects_false,
};
use super::plugin_cli;
use super::plugin_distribution::{
    plugin_package_archive_json, read_plugin_package_archive_files, sha256_archive_file,
};
use super::{
    atomic_write_text, canonical_json_sha256, create_tar_gz,
    factory_app_run_bundle_reject_secret_markers, fail_if_provider_api_key_env_present, json_bool,
    json_string, sha256_file, validate_plugin_observer_trust_boundary,
};

pub(super) fn plugin_pulse_apply_observer_bundle(
    options: plugin_cli::PluginPulseApplyObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let windows_state = match (options.windows_apply_result, options.windows_sha256) {
        (Some(path), Some(sha256)) => Some((path, sha256.trim().to_string())),
        (None, None) => None,
        _ => anyhow::bail!(
            "pulse apply observer bundle requires both --windows-apply-result and --windows-sha256 when Windows proof is supplied"
        ),
    };
    let windows_unavailable_reason = options
        .windows_unavailable_reason
        .unwrap_or_default()
        .trim()
        .to_string();
    if windows_state.is_none() && windows_unavailable_reason.is_empty() {
        anyhow::bail!(
            "pulse apply observer bundle requires --windows-unavailable-reason when Windows proof is not supplied"
        );
    }

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let mut inputs = vec![
        (
            "macos",
            options.macos_apply_result,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_apply_result,
            options.ubuntu_sha256.trim().to_string(),
        ),
    ];
    if let Some((windows_apply_result, windows_sha256)) = windows_state {
        inputs.push(("windows", windows_apply_result, windows_sha256));
    }

    let mut platforms = Vec::new();
    let mut platform_apply_results = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} pulse apply-result sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} pulse-apply-result.json"),
        )?;
        let apply_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let apply_result: serde_json::Value = serde_json::from_str(&apply_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_pulse_apply_result_artifact(&apply_result, platform)?;

        let bundled_path = platforms_root
            .join(platform)
            .join("pulse-apply-result.json");
        atomic_write_text(&bundled_path, &apply_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled pulse-apply-result.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} pulse apply-result changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platforms.push(platform.to_string());
        platform_apply_results.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&apply_result, "schema_version"),
                "status": json_string(&apply_result, "status"),
                "execution_mode": json_string(&apply_result, "execution_mode"),
                "selected_task": apply_result.get("selected_task").cloned().unwrap_or_else(|| serde_json::json!({})),
                "dry_run_task": apply_result.get("dry_run_task").cloned().unwrap_or_else(|| serde_json::json!({})),
                "governed_task_evidence": apply_result.get("governed_task_evidence").cloned().unwrap_or_else(|| serde_json::json!({})),
                "task_result": apply_result.get("task_result").cloned().unwrap_or_else(|| serde_json::json!({})),
                "evaluator_closer": apply_result.get("evaluator_closer").cloned().unwrap_or_else(|| serde_json::json!({})),
                "applied_file_operations": apply_result.get("applied_file_operations").cloned().unwrap_or_else(|| serde_json::json!([])),
                "trust_boundary": apply_result.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": apply_result.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({}))
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-pulse-apply-result-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-pulse-apply-result-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-pulse-apply-result-observer-bundle.json");
    let platform_apply_results_value = serde_json::Value::Object(platform_apply_results);
    let platform_apply_results_sha256 = canonical_json_sha256(&platform_apply_results_value);
    let unavailable_platforms = if platforms.iter().any(|platform| platform == "windows") {
        serde_json::json!({})
    } else {
        serde_json::json!({
            "windows": {
                "status": "unavailable",
                "reason": windows_unavailable_reason,
                "proof_required_when_reachable": true
            }
        })
    };
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-pulse-apply-result-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 Pulse apply evidence",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": platforms.len(),
        "platforms": platforms,
        "unavailable_platforms": unavailable_platforms,
        "observed_evidence_scope": ["ao2.pulse-apply-result.v1"],
        "platform_apply_results": platform_apply_results_value,
        "platform_apply_results_sha256": platform_apply_results_sha256,
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
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
    let summary_body = serde_json::to_string_pretty(&summary)?;
    atomic_write_text(&summary_path, &summary_body)?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "k37-pulse-apply-result-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-pulse-apply-result-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_pulse_apply_observer_bundle_verify(
    options: plugin_cli::PluginPulseApplyObserverBundleVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "pulse apply observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "pulse apply observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-pulse-apply-result-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-pulse-apply-result-observer-bundle.tar.gz",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_pulse_apply_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_apply_results = summary
        .get("platform_apply_results")
        .and_then(serde_json::Value::as_object)
        .context("pulse apply observer bundle missing platform_apply_results")?;
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse apply observer bundle missing platforms")?;
    for platform_value in platforms {
        let platform = platform_value
            .as_str()
            .ok_or_else(|| anyhow!("pulse apply observer bundle platform must be a string"))?;
        let archive_path = format!("platforms/{platform}/pulse-apply-result.json");
        let apply_result =
            plugin_package_archive_json(&archive_files, &archive_path, "bundled apply result")?;
        validate_pulse_apply_result_artifact(&apply_result, platform)?;
        let apply_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        let summary_apply = platform_apply_results
            .get(platform)
            .with_context(|| format!("pulse apply observer bundle missing {platform}"))?;
        if apply_sha256 != json_string(summary_apply, "sha256") {
            anyhow::bail!(
                "{platform} pulse apply observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_apply, "sha256"),
                apply_sha256
            );
        }
        if json_string(summary_apply, "schema_version") != "ao2.pulse-apply-result.v1"
            || json_string(summary_apply, "status") != "accepted"
            || json_string(summary_apply, "execution_mode") != "bounded_planned_file_apply"
        {
            anyhow::bail!("{platform} pulse apply observer bundle summary metadata is invalid");
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-pulse-apply-result-observer-bundle-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": json_u64(&summary, "platform_count"),
        "platforms": summary.get("platforms").cloned().unwrap_or_else(|| serde_json::json!([])),
        "unavailable_platforms": summary.get("unavailable_platforms").cloned().unwrap_or_else(|| serde_json::json!({})),
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "platform_apply_results_sha256": json_string(&summary, "platform_apply_results_sha256"),
        "archive_contents_verified": true,
        "platform_apply_results_verified": true,
        "token_safe_output_verified": true,
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.k37-pulse-apply-result-observer-bundle-verification.v1");
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_pulse_once_observer_bundle(
    options: plugin_cli::PluginPulseOnceObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = vec![
        (
            "macos",
            options.macos_once,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_once,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_once,
            options.windows_sha256.trim().to_string(),
        ),
    ];

    let mut platforms = Vec::new();
    let mut platform_once = serde_json::Map::new();
    for (platform, once_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&once_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} pulse once sha256 mismatch for {}: expected {}, actual {}",
                once_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &once_path,
            &format!("{platform} pulse-once.json"),
        )?;
        let once_text = fs::read_to_string(&once_path)
            .with_context(|| format!("read {}", once_path.display()))?;
        let once: serde_json::Value = serde_json::from_str(&once_text)
            .with_context(|| format!("parse {}", once_path.display()))?;
        validate_pulse_once_platform_evidence(platform, &once, &actual_sha256)?;

        let platform_root = platforms_root.join(platform);
        fs::create_dir_all(&platform_root)
            .with_context(|| format!("create {}", platform_root.display()))?;
        let bundled_once = platform_root.join("pulse-once.json");
        atomic_write_text(&bundled_once, &once_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_once,
            &format!("{platform} bundled pulse-once.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_once)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} pulse once changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platforms.push(platform.to_string());
        platform_once.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": once_path.display().to_string(),
                "bundled_paths": {
                    "pulse_once": bundled_once.display().to_string()
                },
                "sha256": actual_sha256,
                "schema_version": json_string(&once, "schema_version"),
                "status": json_string(&once, "status"),
                "scheduler": once.get("scheduler").cloned().unwrap_or_else(|| serde_json::json!({})),
                "observed_inputs": once.get("observed_inputs").cloned().unwrap_or_else(|| serde_json::json!({})),
                "selected_task": once.get("selected_task").cloned().unwrap_or_else(|| serde_json::json!({})),
                "c85": once.get("c85").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": once.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": once.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({}))
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-pulse-once-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-pulse-once-observer-bundle.tar.gz",
    )?;

    let summary_path = options.out_dir.join("k37-pulse-once-observer-bundle.json");
    let platform_once_value = serde_json::Value::Object(platform_once);
    let platform_once_sha256 = canonical_json_sha256(&platform_once_value);
    let current_ao2_head = current_git_head_string().unwrap_or_else(|_| "unknown".to_string());
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-pulse-once-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 Pulse once-mode evidence",
        "current_ao2_head": current_ao2_head,
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": platforms,
        "observed_evidence_scope": ["ao2.pulse-once.v1"],
        "platform_once": platform_once_value,
        "platform_once_sha256": platform_once_sha256,
        "platform_progress": {
            "schema_version": "ao2.pulse-platform-progress.v1",
            "status": "closure_ready",
            "required_platforms": ["macos", "ubuntu", "windows"],
            "blocked_platforms": [],
            "macos": {
                "current_state": "closure_ready",
                "state_history": ["pending", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "ubuntu": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "windows": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            }
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
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
            "would_approve_release": false,
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "k37-pulse-once-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-pulse-once-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_pulse_once_observer_bundle_verify(
    options: plugin_cli::PluginPulseOnceObserverBundleVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "pulse once observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "pulse once observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-pulse-once-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-pulse-once-observer-bundle.tar.gz",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_pulse_once_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_once = summary
        .get("platform_once")
        .and_then(serde_json::Value::as_object)
        .context("pulse once observer bundle missing platform_once")?;
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse once observer bundle missing platforms")?;
    for platform_value in platforms {
        let platform = platform_value
            .as_str()
            .ok_or_else(|| anyhow!("pulse once observer bundle platform must be a string"))?;
        let archive_path = format!("platforms/{platform}/pulse-once.json");
        let once = plugin_package_archive_json(&archive_files, &archive_path, "pulse once")?;
        let once_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        validate_pulse_once_platform_evidence(platform, &once, &once_sha256)?;
        let summary_once = platform_once
            .get(platform)
            .with_context(|| format!("pulse once observer bundle missing {platform}"))?;
        if json_string(summary_once, "sha256") != once_sha256 {
            anyhow::bail!(
                "{platform} pulse once observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_once, "sha256"),
                once_sha256
            );
        }
        if json_string(summary_once, "schema_version") != "ao2.pulse-once.v1"
            || json_string(summary_once, "status") != "ready_for_operator_execution"
        {
            anyhow::bail!("{platform} pulse once observer bundle summary metadata is invalid");
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-pulse-once-observer-bundle-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": json_u64(&summary, "platform_count"),
        "platforms": summary.get("platforms").cloned().unwrap_or_else(|| serde_json::json!([])),
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "platform_once_sha256": json_string(&summary, "platform_once_sha256"),
        "archive_contents_verified": true,
        "platform_once_verified": true,
        "token_safe_output_verified": true,
        "platform_progress": summary["platform_progress"].clone(),
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.k37-pulse-once-observer-bundle-verification.v1");
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_pulse_chain_observer_bundle(
    options: plugin_cli::PluginPulseChainObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = vec![
        (
            "macos",
            options.macos_chain,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_chain,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_chain,
            options.windows_sha256.trim().to_string(),
        ),
    ];

    let mut platforms = Vec::new();
    let mut platform_chain = serde_json::Map::new();
    for (platform, chain_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&chain_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} pulse chain sha256 mismatch for {}: expected {}, actual {}",
                chain_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &chain_path,
            &format!("{platform} pulse-chain.json"),
        )?;
        let chain_text = fs::read_to_string(&chain_path)
            .with_context(|| format!("read {}", chain_path.display()))?;
        let chain: serde_json::Value = serde_json::from_str(&chain_text)
            .with_context(|| format!("parse {}", chain_path.display()))?;
        validate_pulse_chain_platform_evidence(platform, &chain, &actual_sha256)?;

        let platform_root = platforms_root.join(platform);
        fs::create_dir_all(&platform_root)
            .with_context(|| format!("create {}", platform_root.display()))?;
        let bundled_chain = platform_root.join("pulse-chain.json");
        atomic_write_text(&bundled_chain, &chain_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_chain,
            &format!("{platform} bundled pulse-chain.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_chain)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} pulse chain changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platforms.push(platform.to_string());
        platform_chain.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": chain_path.display().to_string(),
                "bundled_paths": {
                    "pulse_chain": bundled_chain.display().to_string()
                },
                "sha256": actual_sha256,
                "schema_version": json_string(&chain, "schema_version"),
                "status": json_string(&chain, "status"),
                "scheduler": chain.get("scheduler").cloned().unwrap_or_else(|| serde_json::json!({})),
                "observed_inputs": chain.get("observed_inputs").cloned().unwrap_or_else(|| serde_json::json!({})),
                "prior_once": chain.get("prior_once").cloned().unwrap_or_else(|| serde_json::json!({})),
                "chain_steps": chain.get("chain_steps").cloned().unwrap_or_else(|| serde_json::json!([])),
                "c85": chain.get("c85").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": chain.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": chain.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({}))
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-pulse-chain-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-pulse-chain-observer-bundle.tar.gz",
    )?;

    let summary_path = options.out_dir.join("k37-pulse-chain-observer-bundle.json");
    let platform_chain_value = serde_json::Value::Object(platform_chain);
    let platform_chain_sha256 = canonical_json_sha256(&platform_chain_value);
    let current_ao2_head = current_git_head_string().unwrap_or_else(|_| "unknown".to_string());
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-pulse-chain-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 Pulse chain-mode evidence",
        "current_ao2_head": current_ao2_head,
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": platforms,
        "observed_evidence_scope": ["ao2.pulse-chain.v1"],
        "platform_chain": platform_chain_value,
        "platform_chain_sha256": platform_chain_sha256,
        "platform_progress": {
            "schema_version": "ao2.pulse-platform-progress.v1",
            "status": "closure_ready",
            "required_platforms": ["macos", "ubuntu", "windows"],
            "blocked_platforms": [],
            "macos": {
                "current_state": "closure_ready",
                "state_history": ["pending", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "ubuntu": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "windows": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            }
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
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
            "would_approve_release": false,
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "k37-pulse-chain-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-pulse-chain-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_pulse_chain_observer_bundle_verify(
    options: plugin_cli::PluginPulseChainObserverBundleVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "pulse chain observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "pulse chain observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-pulse-chain-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-pulse-chain-observer-bundle.tar.gz",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_pulse_chain_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_chain = summary
        .get("platform_chain")
        .and_then(serde_json::Value::as_object)
        .context("pulse chain observer bundle missing platform_chain")?;
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse chain observer bundle missing platforms")?;
    for platform_value in platforms {
        let platform = platform_value
            .as_str()
            .ok_or_else(|| anyhow!("pulse chain observer bundle platform must be a string"))?;
        let archive_path = format!("platforms/{platform}/pulse-chain.json");
        let chain = plugin_package_archive_json(&archive_files, &archive_path, "pulse chain")?;
        let chain_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        validate_pulse_chain_platform_evidence(platform, &chain, &chain_sha256)?;
        let summary_chain = platform_chain
            .get(platform)
            .with_context(|| format!("pulse chain observer bundle missing {platform}"))?;
        if json_string(summary_chain, "sha256") != chain_sha256 {
            anyhow::bail!(
                "{platform} pulse chain observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_chain, "sha256"),
                chain_sha256
            );
        }
        if json_string(summary_chain, "schema_version") != "ao2.pulse-chain.v1"
            || json_string(summary_chain, "status") != "planned_without_execution"
        {
            anyhow::bail!("{platform} pulse chain observer bundle summary metadata is invalid");
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-pulse-chain-observer-bundle-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": json_u64(&summary, "platform_count"),
        "platforms": summary.get("platforms").cloned().unwrap_or_else(|| serde_json::json!([])),
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "platform_chain_sha256": json_string(&summary, "platform_chain_sha256"),
        "archive_contents_verified": true,
        "platform_chain_verified": true,
        "token_safe_output_verified": true,
        "platform_progress": summary["platform_progress"].clone(),
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.k37-pulse-chain-observer-bundle-verification.v1");
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_pulse_eval_loop_observer_bundle(
    options: plugin_cli::PluginPulseEvalLoopObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = vec![
        (
            "macos",
            options.macos_eval_loop,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_eval_loop,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_eval_loop,
            options.windows_sha256.trim().to_string(),
        ),
    ];

    let mut platforms = Vec::new();
    let mut platform_eval_loop = serde_json::Map::new();
    for (platform, eval_loop_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&eval_loop_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} pulse eval-loop sha256 mismatch for {}: expected {}, actual {}",
                eval_loop_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &eval_loop_path,
            &format!("{platform} pulse-eval-loop.json"),
        )?;
        let eval_loop_text = fs::read_to_string(&eval_loop_path)
            .with_context(|| format!("read {}", eval_loop_path.display()))?;
        let eval_loop: serde_json::Value = serde_json::from_str(&eval_loop_text)
            .with_context(|| format!("parse {}", eval_loop_path.display()))?;
        validate_pulse_eval_loop_platform_evidence(platform, &eval_loop, &actual_sha256)?;

        let platform_root = platforms_root.join(platform);
        fs::create_dir_all(&platform_root)
            .with_context(|| format!("create {}", platform_root.display()))?;
        let bundled_eval_loop = platform_root.join("pulse-eval-loop.json");
        atomic_write_text(&bundled_eval_loop, &eval_loop_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_eval_loop,
            &format!("{platform} bundled pulse-eval-loop.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_eval_loop)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} pulse eval-loop changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platforms.push(platform.to_string());
        platform_eval_loop.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": eval_loop_path.display().to_string(),
                "bundled_paths": {
                    "pulse_eval_loop": bundled_eval_loop.display().to_string()
                },
                "sha256": actual_sha256,
                "schema_version": json_string(&eval_loop, "schema_version"),
                "status": json_string(&eval_loop, "status"),
                "mode": json_string(&eval_loop, "mode"),
                "loop": eval_loop.get("loop").cloned().unwrap_or_else(|| serde_json::json!({})),
                "observed_inputs": eval_loop.get("observed_inputs").cloned().unwrap_or_else(|| serde_json::json!({})),
                "prior_eval_loop": eval_loop.get("prior_eval_loop").cloned().unwrap_or_else(|| serde_json::json!({})),
                "verification": eval_loop.get("verification").cloned().unwrap_or_else(|| serde_json::json!({})),
                "evaluator": eval_loop.get("evaluator").cloned().unwrap_or_else(|| serde_json::json!({})),
                "recommended_next_task": eval_loop.get("recommended_next_task").cloned().unwrap_or_else(|| serde_json::json!({})),
                "c85": eval_loop.get("c85").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": eval_loop.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": eval_loop.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({}))
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-pulse-eval-loop-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-pulse-eval-loop-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-pulse-eval-loop-observer-bundle.json");
    let platform_eval_loop_value = serde_json::Value::Object(platform_eval_loop);
    let platform_eval_loop_sha256 = canonical_json_sha256(&platform_eval_loop_value);
    let current_ao2_head = current_git_head_string().unwrap_or_else(|_| "unknown".to_string());
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-pulse-eval-loop-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 Pulse eval-loop chain evidence",
        "current_ao2_head": current_ao2_head,
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": platforms,
        "observed_evidence_scope": ["ao2.pulse-eval-loop.v1"],
        "platform_eval_loop": platform_eval_loop_value,
        "platform_eval_loop_sha256": platform_eval_loop_sha256,
        "platform_progress": {
            "schema_version": "ao2.pulse-platform-progress.v1",
            "status": "closure_ready",
            "required_platforms": ["macos", "ubuntu", "windows"],
            "blocked_platforms": [],
            "macos": {
                "current_state": "closure_ready",
                "state_history": ["pending", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "ubuntu": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "windows": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            }
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
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
            "would_approve_release": false,
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false,
            "repo_apply": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "k37-pulse-eval-loop-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-pulse-eval-loop-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_pulse_eval_loop_observer_bundle_verify(
    options: plugin_cli::PluginPulseEvalLoopObserverBundleVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "pulse eval-loop observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "pulse eval-loop observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-pulse-eval-loop-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-pulse-eval-loop-observer-bundle.tar.gz",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_pulse_eval_loop_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_eval_loop = summary
        .get("platform_eval_loop")
        .and_then(serde_json::Value::as_object)
        .context("pulse eval-loop observer bundle missing platform_eval_loop")?;
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse eval-loop observer bundle missing platforms")?;
    for platform_value in platforms {
        let platform = platform_value
            .as_str()
            .ok_or_else(|| anyhow!("pulse eval-loop observer bundle platform must be a string"))?;
        let archive_path = format!("platforms/{platform}/pulse-eval-loop.json");
        let eval_loop =
            plugin_package_archive_json(&archive_files, &archive_path, "pulse eval-loop")?;
        let eval_loop_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        validate_pulse_eval_loop_platform_evidence(platform, &eval_loop, &eval_loop_sha256)?;
        let summary_eval_loop = platform_eval_loop
            .get(platform)
            .with_context(|| format!("pulse eval-loop observer bundle missing {platform}"))?;
        if json_string(summary_eval_loop, "sha256") != eval_loop_sha256 {
            anyhow::bail!(
                "{platform} pulse eval-loop observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_eval_loop, "sha256"),
                eval_loop_sha256
            );
        }
        if json_string(summary_eval_loop, "schema_version") != "ao2.pulse-eval-loop.v1"
            || json_string(summary_eval_loop, "status") != "ready_for_next_pulse_task"
        {
            anyhow::bail!("{platform} pulse eval-loop observer bundle summary metadata is invalid");
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-pulse-eval-loop-observer-bundle-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": json_u64(&summary, "platform_count"),
        "platforms": summary.get("platforms").cloned().unwrap_or_else(|| serde_json::json!([])),
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "platform_eval_loop_sha256": json_string(&summary, "platform_eval_loop_sha256"),
        "archive_contents_verified": true,
        "platform_eval_loop_verified": true,
        "token_safe_output_verified": true,
        "platform_progress": summary["platform_progress"].clone(),
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.k37-pulse-eval-loop-observer-bundle-verification.v1");
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_pulse_executor_observer_bundle(
    options: plugin_cli::PluginPulseExecutorObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = vec![
        (
            "macos",
            options.macos_executor,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_executor,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_executor,
            options.windows_sha256.trim().to_string(),
        ),
    ];

    let mut platforms = Vec::new();
    let mut platform_evidence = serde_json::Map::new();
    let mut bundle_c85: Option<serde_json::Value> = None;
    for (platform, executor_path, supplied_sha256) in inputs {
        let actual_executor_sha256 = sha256_file(&executor_path)?;
        if actual_executor_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} pulse executor sha256 mismatch for {}: expected {}, actual {}",
                executor_path.display(),
                supplied_sha256,
                actual_executor_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &executor_path,
            &format!("{platform} pulse-executor.json"),
        )?;
        let executor_text = fs::read_to_string(&executor_path)
            .with_context(|| format!("read {}", executor_path.display()))?;
        let executor: serde_json::Value = serde_json::from_str(&executor_text)
            .with_context(|| format!("parse {}", executor_path.display()))?;

        let governed_task_path = pulse_executor_artifact_path(
            &executor,
            &executor_path,
            "governed_task_evidence",
            "pulse-governed-task.json",
        )?;
        let governed_task_sha256 = sha256_file(&governed_task_path)?;
        let governed_task_text = fs::read_to_string(&governed_task_path)
            .with_context(|| format!("read {}", governed_task_path.display()))?;
        let governed_task: serde_json::Value = serde_json::from_str(&governed_task_text)
            .with_context(|| format!("parse {}", governed_task_path.display()))?;

        let task_result_path = pulse_executor_artifact_path(
            &executor,
            &executor_path,
            "pulse_task_result",
            "pulse-task-result.json",
        )?;
        let task_result_sha256 = sha256_file(&task_result_path)?;
        let task_result_text = fs::read_to_string(&task_result_path)
            .with_context(|| format!("read {}", task_result_path.display()))?;
        let task_result: serde_json::Value = serde_json::from_str(&task_result_text)
            .with_context(|| format!("parse {}", task_result_path.display()))?;

        validate_pulse_executor_platform_evidence(
            platform,
            &executor,
            &actual_executor_sha256,
            &governed_task,
            &governed_task_sha256,
            &task_result,
            &task_result_sha256,
        )?;
        let executor_c85 = executor
            .get("c85")
            .cloned()
            .ok_or_else(|| anyhow!("{platform} pulse executor missing c85"))?;
        if let Some(existing_c85) = &bundle_c85 {
            if json_string(existing_c85, "status") != json_string(&executor_c85, "status") {
                anyhow::bail!(
                    "pulse executor observer bundle requires consistent C85 status across platforms"
                );
            }
        } else {
            bundle_c85 = Some(executor_c85);
        }

        let platform_root = platforms_root.join(platform);
        fs::create_dir_all(&platform_root)
            .with_context(|| format!("create {}", platform_root.display()))?;
        let bundled_executor = platform_root.join("pulse-executor.json");
        let bundled_governed_task = platform_root.join("pulse-governed-task.json");
        let bundled_task_result = platform_root.join("pulse-task-result.json");
        atomic_write_text(&bundled_executor, &executor_text)?;
        atomic_write_text(&bundled_governed_task, &governed_task_text)?;
        atomic_write_text(&bundled_task_result, &task_result_text)?;
        for (path, expected, label) in [
            (
                bundled_executor.as_path(),
                actual_executor_sha256.as_str(),
                "pulse executor",
            ),
            (
                bundled_governed_task.as_path(),
                governed_task_sha256.as_str(),
                "pulse governed task",
            ),
            (
                bundled_task_result.as_path(),
                task_result_sha256.as_str(),
                "pulse task result",
            ),
        ] {
            let bundled_sha256 = sha256_file(path)?;
            if bundled_sha256 != expected {
                anyhow::bail!(
                    "{platform} {label} changed while bundling: expected {expected}, bundled {bundled_sha256}"
                );
            }
            factory_app_run_bundle_reject_secret_markers(
                path,
                &format!("{platform} bundled {label}"),
            )?;
        }

        let mut evidence = executor.clone();
        evidence["path"] = serde_json::json!(executor_path.display().to_string());
        evidence["sha256"] = serde_json::json!(actual_executor_sha256);
        evidence["governed_task_evidence"] = {
            let mut value = governed_task.clone();
            value["path"] = serde_json::json!(governed_task_path.display().to_string());
            value["sha256"] = serde_json::json!(governed_task_sha256.clone());
            value
        };
        evidence["pulse_task_result"] = {
            let mut value = task_result.clone();
            value["path"] = serde_json::json!(task_result_path.display().to_string());
            value["sha256"] = serde_json::json!(task_result_sha256.clone());
            value
        };
        evidence["bundled_paths"] = serde_json::json!({
            "pulse_executor": bundled_executor.display().to_string(),
            "governed_task_evidence": bundled_governed_task.display().to_string(),
            "pulse_task_result": bundled_task_result.display().to_string()
        });
        evidence["artifacts"]["governed_task_evidence_sha256"] =
            serde_json::json!(governed_task_sha256);
        evidence["artifacts"]["pulse_task_result_sha256"] = serde_json::json!(task_result_sha256);

        platforms.push(platform.to_string());
        platform_evidence.insert(platform.to_string(), evidence);
    }

    let archive_path = options
        .out_dir
        .join("k37-pulse-executor-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-pulse-executor-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-pulse-executor-observer-bundle.json");
    let platform_evidence_value = serde_json::Value::Object(platform_evidence);
    let mut summary_task_contract = platform_evidence_value["macos"]["task_contract"].clone();
    summary_task_contract["c85"] = serde_json::json!(false);
    summary_task_contract["ao2_owned_execution"] = serde_json::json!(true);
    summary_task_contract["factory_v3_evaluator_closer_required"] = serde_json::json!(true);
    let platform_evidence_sha256 = canonical_json_sha256(&platform_evidence_value);
    let current_ao2_head = current_git_head_string().unwrap_or_else(|_| "unknown".to_string());
    let not_collected_reason = "This observer bundle observes current ao2.pulse-executor.v1 governed-task and task-result evidence only; apply-result evidence is observed by the separate pulse-apply-result observer bundle.";
    let bundle_c85 = bundle_c85.context("pulse executor observer bundle missing C85 metadata")?;
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-pulse-executor-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "current_ao2_head": current_ao2_head,
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "work_source": "codex-cron AO2 Pulse executor evidence",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": platforms.len(),
        "platforms": platforms,
        "observed_evidence_scope": [
            "ao2.pulse-executor.v1",
            "ao2.pulse-governed-task.v1",
            "ao2.pulse-task-result.v1"
        ],
        "platform_evidence": platform_evidence_value,
        "platform_evidence_sha256": platform_evidence_sha256,
        "platform_progress": {
            "schema_version": "ao2.pulse-platform-progress.v1",
            "status": "closure_ready",
            "required_platforms": ["macos", "ubuntu", "windows"],
            "blocked_platforms": [],
            "macos": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "ubuntu": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            },
            "windows": {
                "current_state": "closure_ready",
                "state_history": ["pending", "reachable", "staged", "running", "passed", "evidence_collected", "closure_ready"]
            }
        },
        "task_contract": summary_task_contract,
        "task_result_observation": {
            "schema_version": "ao2.k37-pulse-task-result-observation.v1",
            "status": "ready_for_k37_observation",
            "required_schema_version": "ao2.pulse-task-result.v1",
            "source_ao2_head": current_ao2_head,
            "observed_platforms": ["macos", "ubuntu", "windows"],
            "unavailable_platforms": {}
        },
        "dry_run_task_observation": {
            "schema_version": "ao2.k37-pulse-dry-run-task-observation.v1",
            "status": "not_collected_in_current_executor_refresh",
            "required_schema_version": "ao2.pulse-dry-run-task.v1",
            "source_ao2_head": current_ao2_head,
            "observed_platforms": [],
            "unavailable_platforms": {
                "macos": {"status": "not_collected_in_current_executor_refresh", "reason": not_collected_reason},
                "ubuntu": {"status": "not_collected_in_current_executor_refresh", "reason": not_collected_reason},
                "windows": {"status": "not_collected_in_current_executor_refresh", "reason": not_collected_reason}
            }
        },
        "apply_result_observation": {
            "schema_version": "ao2.k37-pulse-apply-result-observation.v1",
            "status": "not_collected_in_current_executor_refresh",
            "required_schema_version": "ao2.pulse-apply-result.v1",
            "source_ao2_head": current_ao2_head,
            "observed_platforms": [],
            "unavailable_platforms": {
                "macos": {"status": "not_collected_in_current_executor_refresh", "reason": not_collected_reason},
                "ubuntu": {"status": "not_collected_in_current_executor_refresh", "reason": not_collected_reason},
                "windows": {"status": "not_collected_in_current_executor_refresh", "reason": not_collected_reason}
            }
        },
        "c85": bundle_c85,
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
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
            "would_approve_release": false,
            "provider_execution": false,
            "queue_execution": false,
            "memory_write": false,
            "mutates_ao_artifacts": false,
            "hermes_cron_watchdog_mutation": false,
            "control_plane_mutation": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "k37-pulse-executor-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-pulse-executor-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_pulse_executor_observer_bundle_verify(
    options: plugin_cli::PluginPulseExecutorObserverBundleVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "pulse executor observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "pulse executor observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-pulse-executor-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-pulse-executor-observer-bundle.tar.gz",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_pulse_executor_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_evidence = summary
        .get("platform_evidence")
        .and_then(serde_json::Value::as_object)
        .context("pulse executor observer bundle missing platform_evidence")?;
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse executor observer bundle missing platforms")?;
    for platform_value in platforms {
        let platform = platform_value
            .as_str()
            .ok_or_else(|| anyhow!("pulse executor observer bundle platform must be a string"))?;
        let executor_archive_path = format!("platforms/{platform}/pulse-executor.json");
        let governed_archive_path = format!("platforms/{platform}/pulse-governed-task.json");
        let task_result_archive_path = format!("platforms/{platform}/pulse-task-result.json");
        let executor =
            plugin_package_archive_json(&archive_files, &executor_archive_path, "pulse executor")?;
        let governed_task = plugin_package_archive_json(
            &archive_files,
            &governed_archive_path,
            "pulse governed task",
        )?;
        let task_result = plugin_package_archive_json(
            &archive_files,
            &task_result_archive_path,
            "pulse task result",
        )?;
        let executor_sha256 = sha256_archive_file(&archive_files, &executor_archive_path)?;
        let governed_task_sha256 = sha256_archive_file(&archive_files, &governed_archive_path)?;
        let task_result_sha256 = sha256_archive_file(&archive_files, &task_result_archive_path)?;
        validate_pulse_executor_platform_evidence(
            platform,
            &executor,
            &executor_sha256,
            &governed_task,
            &governed_task_sha256,
            &task_result,
            &task_result_sha256,
        )?;
        let summary_evidence = platform_evidence
            .get(platform)
            .with_context(|| format!("pulse executor observer bundle missing {platform}"))?;
        if json_string(summary_evidence, "sha256") != executor_sha256 {
            anyhow::bail!(
                "{platform} pulse executor observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_evidence, "sha256"),
                executor_sha256
            );
        }
        if json_string(&summary_evidence["governed_task_evidence"], "sha256")
            != governed_task_sha256
            || json_string(&summary_evidence["pulse_task_result"], "sha256") != task_result_sha256
        {
            anyhow::bail!(
                "{platform} pulse executor observer bundle embedded artifact digests mismatch archive"
            );
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-pulse-executor-observer-bundle-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": json_u64(&summary, "platform_count"),
        "platforms": summary.get("platforms").cloned().unwrap_or_else(|| serde_json::json!([])),
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "platform_evidence_sha256": json_string(&summary, "platform_evidence_sha256"),
        "platform_progress": summary["platform_progress"].clone(),
        "archive_contents_verified": true,
        "platform_executor_evidence_verified": true,
        "token_safe_output_verified": true,
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.k37-pulse-executor-observer-bundle-verification.v1");
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

fn pulse_executor_artifact_path(
    executor: &serde_json::Value,
    executor_path: &Path,
    artifact_key: &str,
    fallback_file_name: &str,
) -> Result<PathBuf> {
    let from_artifact = executor["artifacts"][artifact_key]
        .as_str()
        .unwrap_or("")
        .trim();
    if !from_artifact.is_empty() {
        let candidate = PathBuf::from(from_artifact);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let fallback = executor_path
        .parent()
        .context("pulse executor path missing parent")?
        .join(fallback_file_name);
    if fallback.is_file() {
        return Ok(fallback);
    }
    anyhow::bail!(
        "pulse executor missing readable {artifact_key}; checked artifact path {:?} and fallback {}",
        from_artifact,
        fallback.display()
    );
}

fn validate_pulse_executor_observer_bundle_summary(
    summary: &serde_json::Value,
    archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.k37-pulse-executor-observer-bundle.v1" {
        anyhow::bail!(
            "pulse executor observer bundle requires ao2.k37-pulse-executor-observer-bundle.v1"
        );
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!("pulse executor observer bundle status must be ready_for_k37_observation");
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("pulse executor observer bundle producer must be ao2");
    }
    if json_string(summary, "archive_sha256") != archive_sha256 {
        anyhow::bail!("pulse executor observer bundle archive sha256 does not match");
    }
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse executor observer bundle missing platforms")?;
    if json_u64(summary, "platform_count") != 3 || platforms.len() != 3 {
        anyhow::bail!("pulse executor observer bundle platform_count must be 3");
    }
    for required in ["macos", "ubuntu", "windows"] {
        if !platforms
            .iter()
            .any(|platform| platform.as_str() == Some(required))
        {
            anyhow::bail!("pulse executor observer bundle missing required platform {required}");
        }
    }
    let observed_scope = summary
        .get("observed_evidence_scope")
        .and_then(serde_json::Value::as_array)
        .context("pulse executor observer bundle missing observed_evidence_scope")?;
    for required in [
        "ao2.pulse-executor.v1",
        "ao2.pulse-governed-task.v1",
        "ao2.pulse-task-result.v1",
    ] {
        if !observed_scope
            .iter()
            .any(|entry| entry.as_str() == Some(required))
        {
            anyhow::bail!("pulse executor observer bundle missing scope {required}");
        }
    }
    let platform_evidence = summary
        .get("platform_evidence")
        .context("pulse executor observer bundle missing platform_evidence")?;
    if json_string(summary, "platform_evidence_sha256") != canonical_json_sha256(platform_evidence)
    {
        anyhow::bail!("pulse executor observer bundle platform evidence digest mismatch");
    }
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("pulse executor observer bundle missing trust_boundary")?,
        "pulse executor observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("pulse executor observer bundle missing control_plane_observation")?,
        "pulse executor observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("pulse executor observer bundle missing side_effects")?,
        "pulse executor observer bundle",
    )?;
    let c85_status = json_string(&summary["c85"], "status");
    match c85_status.as_str() {
        "passed" => {
            if !json_bool(&summary["c85"], "hosted_github_actions_checked")
                || !json_bool(&summary["c85"], "rerun_allowed_without_user_billing_fix")
            {
                anyhow::bail!("pulse executor observer bundle passed C85 metadata is invalid");
            }
        }
        "deferred" => {
            if json_bool(&summary["c85"], "hosted_github_actions_checked")
                || json_bool(&summary["c85"], "rerun_allowed_without_user_billing_fix")
            {
                anyhow::bail!("pulse executor observer bundle deferred C85 metadata is invalid");
            }
        }
        _ => anyhow::bail!("pulse executor observer bundle C85 status is invalid"),
    }
    let platform_progress = summary
        .get("platform_progress")
        .context("pulse executor observer bundle missing platform_progress")?;
    if json_string(platform_progress, "schema_version") != "ao2.pulse-platform-progress.v1"
        || json_string(platform_progress, "status") != "closure_ready"
        || json_string(&platform_progress["windows"], "current_state") != "closure_ready"
        || !platform_progress["blocked_platforms"]
            .as_array()
            .map(|blocked| blocked.is_empty())
            .unwrap_or(false)
    {
        anyhow::bail!("pulse executor observer bundle platform progress is not closure-ready");
    }
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("pulse executor observer bundle token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("pulse executor observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

fn validate_pulse_once_observer_bundle_summary(
    summary: &serde_json::Value,
    archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.k37-pulse-once-observer-bundle.v1" {
        anyhow::bail!("pulse once observer bundle requires ao2.k37-pulse-once-observer-bundle.v1");
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!("pulse once observer bundle status must be ready_for_k37_observation");
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("pulse once observer bundle producer must be ao2");
    }
    if json_string(summary, "archive_sha256") != archive_sha256 {
        anyhow::bail!("pulse once observer bundle archive sha256 does not match");
    }
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse once observer bundle missing platforms")?;
    if json_u64(summary, "platform_count") != 3 || platforms.len() != 3 {
        anyhow::bail!("pulse once observer bundle platform_count must be 3");
    }
    for required in ["macos", "ubuntu", "windows"] {
        if !platforms
            .iter()
            .any(|platform| platform.as_str() == Some(required))
        {
            anyhow::bail!("pulse once observer bundle missing required platform {required}");
        }
    }
    let observed_scope = summary
        .get("observed_evidence_scope")
        .and_then(serde_json::Value::as_array)
        .context("pulse once observer bundle missing observed_evidence_scope")?;
    if !observed_scope
        .iter()
        .any(|entry| entry.as_str() == Some("ao2.pulse-once.v1"))
    {
        anyhow::bail!("pulse once observer bundle missing once-mode scope");
    }
    let platform_once = summary
        .get("platform_once")
        .context("pulse once observer bundle missing platform_once")?;
    if json_string(summary, "platform_once_sha256") != canonical_json_sha256(platform_once) {
        anyhow::bail!("pulse once observer bundle platform digest mismatch");
    }
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("pulse once observer bundle missing trust_boundary")?,
        "pulse once observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("pulse once observer bundle missing control_plane_observation")?,
        "pulse once observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("pulse once observer bundle missing side_effects")?,
        "pulse once observer bundle",
    )?;
    let platform_progress = summary
        .get("platform_progress")
        .context("pulse once observer bundle missing platform_progress")?;
    if json_string(platform_progress, "schema_version") != "ao2.pulse-platform-progress.v1"
        || json_string(platform_progress, "status") != "closure_ready"
        || json_string(&platform_progress["windows"], "current_state") != "closure_ready"
        || !platform_progress["blocked_platforms"]
            .as_array()
            .map(|blocked| blocked.is_empty())
            .unwrap_or(false)
    {
        anyhow::bail!("pulse once observer bundle platform progress is not closure-ready");
    }
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("pulse once observer bundle token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("pulse once observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

fn validate_pulse_chain_observer_bundle_summary(
    summary: &serde_json::Value,
    archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.k37-pulse-chain-observer-bundle.v1" {
        anyhow::bail!(
            "pulse chain observer bundle requires ao2.k37-pulse-chain-observer-bundle.v1"
        );
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!("pulse chain observer bundle status must be ready_for_k37_observation");
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("pulse chain observer bundle producer must be ao2");
    }
    if json_string(summary, "archive_sha256") != archive_sha256 {
        anyhow::bail!("pulse chain observer bundle archive sha256 does not match");
    }
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse chain observer bundle missing platforms")?;
    if json_u64(summary, "platform_count") != 3 || platforms.len() != 3 {
        anyhow::bail!("pulse chain observer bundle platform_count must be 3");
    }
    for required in ["macos", "ubuntu", "windows"] {
        if !platforms
            .iter()
            .any(|platform| platform.as_str() == Some(required))
        {
            anyhow::bail!("pulse chain observer bundle missing required platform {required}");
        }
    }
    let observed_scope = summary
        .get("observed_evidence_scope")
        .and_then(serde_json::Value::as_array)
        .context("pulse chain observer bundle missing observed_evidence_scope")?;
    if !observed_scope
        .iter()
        .any(|entry| entry.as_str() == Some("ao2.pulse-chain.v1"))
    {
        anyhow::bail!("pulse chain observer bundle missing chain-mode scope");
    }
    let platform_chain = summary
        .get("platform_chain")
        .context("pulse chain observer bundle missing platform_chain")?;
    if json_string(summary, "platform_chain_sha256") != canonical_json_sha256(platform_chain) {
        anyhow::bail!("pulse chain observer bundle platform digest mismatch");
    }
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("pulse chain observer bundle missing trust_boundary")?,
        "pulse chain observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("pulse chain observer bundle missing control_plane_observation")?,
        "pulse chain observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("pulse chain observer bundle missing side_effects")?,
        "pulse chain observer bundle",
    )?;
    let platform_progress = summary
        .get("platform_progress")
        .context("pulse chain observer bundle missing platform_progress")?;
    if json_string(platform_progress, "schema_version") != "ao2.pulse-platform-progress.v1"
        || json_string(platform_progress, "status") != "closure_ready"
        || json_string(&platform_progress["windows"], "current_state") != "closure_ready"
        || !platform_progress["blocked_platforms"]
            .as_array()
            .map(|blocked| blocked.is_empty())
            .unwrap_or(false)
    {
        anyhow::bail!("pulse chain observer bundle platform progress is not closure-ready");
    }
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("pulse chain observer bundle token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("pulse chain observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

fn validate_pulse_eval_loop_observer_bundle_summary(
    summary: &serde_json::Value,
    archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.k37-pulse-eval-loop-observer-bundle.v1" {
        anyhow::bail!(
            "pulse eval-loop observer bundle requires ao2.k37-pulse-eval-loop-observer-bundle.v1"
        );
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!("pulse eval-loop observer bundle status must be ready_for_k37_observation");
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("pulse eval-loop observer bundle producer must be ao2");
    }
    if json_string(summary, "archive_sha256") != archive_sha256 {
        anyhow::bail!("pulse eval-loop observer bundle archive sha256 does not match");
    }
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse eval-loop observer bundle missing platforms")?;
    if json_u64(summary, "platform_count") != 3 || platforms.len() != 3 {
        anyhow::bail!("pulse eval-loop observer bundle platform_count must be 3");
    }
    for required in ["macos", "ubuntu", "windows"] {
        if !platforms
            .iter()
            .any(|platform| platform.as_str() == Some(required))
        {
            anyhow::bail!("pulse eval-loop observer bundle missing required platform {required}");
        }
    }
    let observed_scope = summary
        .get("observed_evidence_scope")
        .and_then(serde_json::Value::as_array)
        .context("pulse eval-loop observer bundle missing observed_evidence_scope")?;
    if !observed_scope
        .iter()
        .any(|entry| entry.as_str() == Some("ao2.pulse-eval-loop.v1"))
    {
        anyhow::bail!("pulse eval-loop observer bundle missing eval-loop scope");
    }
    let platform_eval_loop = summary
        .get("platform_eval_loop")
        .context("pulse eval-loop observer bundle missing platform_eval_loop")?;
    if json_string(summary, "platform_eval_loop_sha256")
        != canonical_json_sha256(platform_eval_loop)
    {
        anyhow::bail!("pulse eval-loop observer bundle platform digest mismatch");
    }
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("pulse eval-loop observer bundle missing trust_boundary")?,
        "pulse eval-loop observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("pulse eval-loop observer bundle missing control_plane_observation")?,
        "pulse eval-loop observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("pulse eval-loop observer bundle missing side_effects")?,
        "pulse eval-loop observer bundle",
    )?;
    if json_bool(&summary["side_effects"], "repo_apply") {
        anyhow::bail!("pulse eval-loop observer bundle repo_apply must be false");
    }
    let platform_progress = summary
        .get("platform_progress")
        .context("pulse eval-loop observer bundle missing platform_progress")?;
    if json_string(platform_progress, "schema_version") != "ao2.pulse-platform-progress.v1"
        || json_string(platform_progress, "status") != "closure_ready"
        || json_string(&platform_progress["windows"], "current_state") != "closure_ready"
        || !platform_progress["blocked_platforms"]
            .as_array()
            .map(|blocked| blocked.is_empty())
            .unwrap_or(false)
    {
        anyhow::bail!("pulse eval-loop observer bundle platform progress is not closure-ready");
    }
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("pulse eval-loop observer bundle token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("pulse eval-loop observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

fn validate_pulse_once_platform_evidence(
    platform: &str,
    once: &serde_json::Value,
    once_sha256: &str,
) -> Result<()> {
    if once_sha256.len() != 64 {
        anyhow::bail!("{platform} pulse once digest metadata is invalid");
    }
    if json_string(once, "schema_version") != "ao2.pulse-once.v1" {
        anyhow::bail!("{platform} pulse once requires ao2.pulse-once.v1");
    }
    if json_string(once, "status") != "ready_for_operator_execution" {
        anyhow::bail!("{platform} pulse once status must be ready_for_operator_execution");
    }
    if json_string(&once["scheduler"], "active_runner") != "codex-cron"
        || json_bool(&once["scheduler"], "hermes_cron_mutated")
        || json_string(&once["scheduler"], "fixed_interval_loop_successor")
            != "ao2 pulse run --once"
    {
        anyhow::bail!("{platform} pulse once scheduler metadata is invalid");
    }
    if json_string(&once["selected_task"], "classification") != "COMPLEX"
        || json_string(&once["selected_task"], "shape").is_empty()
        || !json_string(&once["selected_task"], "recommended_command")
            .contains("ao2 pulse run --once")
    {
        anyhow::bail!("{platform} pulse once selected task metadata is invalid");
    }
    let c85_status = json_string(&once["c85"], "status");
    if c85_status != "passed" && c85_status != "deferred" {
        anyhow::bail!("{platform} pulse once c85 status is invalid");
    }
    validate_pulse_execution_side_effects_false(&once["side_effects"], platform, "once")?;
    validate_pulse_execution_trust_boundary(&once["trust_boundary"], platform, "once")?;
    Ok(())
}

fn validate_pulse_chain_platform_evidence(
    platform: &str,
    chain: &serde_json::Value,
    chain_sha256: &str,
) -> Result<()> {
    if chain_sha256.len() != 64 {
        anyhow::bail!("{platform} pulse chain digest metadata is invalid");
    }
    if json_string(chain, "schema_version") != "ao2.pulse-chain.v1" {
        anyhow::bail!("{platform} pulse chain requires ao2.pulse-chain.v1");
    }
    if json_string(chain, "status") != "planned_without_execution" {
        anyhow::bail!("{platform} pulse chain status must be planned_without_execution");
    }
    if json_string(&chain["scheduler"], "active_runner") != "codex-cron"
        || json_bool(&chain["scheduler"], "hermes_cron_mutated")
        || json_string(&chain["scheduler"], "fixed_interval_loop_successor")
            != "ao2 pulse run --chain"
    {
        anyhow::bail!("{platform} pulse chain scheduler metadata is invalid");
    }
    if json_string(&chain["prior_once"], "schema_version") != "ao2.pulse-once.v1"
        || json_string(&chain["prior_once"], "status") != "ready_for_operator_execution"
        || json_string(&chain["prior_once"], "sha256").len() != 64
    {
        anyhow::bail!("{platform} pulse chain prior_once metadata is invalid");
    }
    let chain_steps = chain["chain_steps"]
        .as_array()
        .ok_or_else(|| anyhow!("{platform} pulse chain requires chain_steps"))?;
    if !chain_steps.iter().any(|step| {
        json_string(step, "id") == "observe-pulse-once-and-select-next-safe-task"
            && !json_bool(step, "executes_task")
    }) {
        anyhow::bail!("{platform} pulse chain missing read-only observation step");
    }
    let c85_status = json_string(&chain["c85"], "status");
    if c85_status != "passed" && c85_status != "deferred" {
        anyhow::bail!("{platform} pulse chain c85 status is invalid");
    }
    validate_pulse_execution_side_effects_false(&chain["side_effects"], platform, "chain")?;
    validate_pulse_execution_trust_boundary(&chain["trust_boundary"], platform, "chain")?;
    Ok(())
}

fn validate_pulse_eval_loop_platform_evidence(
    platform: &str,
    eval_loop: &serde_json::Value,
    eval_loop_sha256: &str,
) -> Result<()> {
    if eval_loop_sha256.len() != 64 {
        anyhow::bail!("{platform} pulse eval-loop digest metadata is invalid");
    }
    if json_string(eval_loop, "schema_version") != "ao2.pulse-eval-loop.v1" {
        anyhow::bail!("{platform} pulse eval-loop requires ao2.pulse-eval-loop.v1");
    }
    if json_string(eval_loop, "status") != "ready_for_next_pulse_task" {
        anyhow::bail!("{platform} pulse eval-loop status must be ready_for_next_pulse_task");
    }
    if json_string(eval_loop, "mode") != "recommendation_only" {
        anyhow::bail!("{platform} pulse eval-loop mode must be recommendation_only");
    }
    let loop_state = eval_loop
        .get("loop")
        .context("pulse eval-loop missing loop state")?;
    if !json_bool(loop_state, "bounded")
        || !json_bool(loop_state, "terminal")
        || json_bool(loop_state, "continues_automatically")
        || json_u64(loop_state, "max_iterations") != 1
        || json_u64(loop_state, "chain_depth") < 1
        || json_string(loop_state, "fixed_interval_loop_successor")
            != "ao2 pulse eval-loop run --chain"
    {
        anyhow::bail!("{platform} pulse eval-loop loop metadata is invalid");
    }
    if json_string(&eval_loop["prior_eval_loop"], "schema_version") != "ao2.pulse-eval-loop.v1"
        || json_string(&eval_loop["prior_eval_loop"], "status") != "ready_for_next_pulse_task"
        || json_string(&eval_loop["prior_eval_loop"], "sha256").len() != 64
    {
        anyhow::bail!("{platform} pulse eval-loop prior_eval_loop metadata is invalid");
    }
    if json_string(&eval_loop["verification"], "status") != "passed"
        || !json_bool(&eval_loop["verification"], "required_for_recommendation")
    {
        anyhow::bail!("{platform} pulse eval-loop verification metadata is invalid");
    }
    if json_string(&eval_loop["evaluator"], "decision") != "recommend_next_task"
        || json_string(&eval_loop["evaluator"], "release_acceptance_owner")
            != "factory-v3 evaluator-closer"
        || !json_bool(&eval_loop["evaluator"], "evidence_digest_required")
    {
        anyhow::bail!("{platform} pulse eval-loop evaluator metadata is invalid");
    }
    if json_string(&eval_loop["recommended_next_task"], "id")
        != "ao2-pulse-eval-loop-chain-next-task"
        || json_string(&eval_loop["recommended_next_task"], "classification") != "COMPLEX"
        || !json_bool(
            &eval_loop["recommended_next_task"],
            "requires_operator_or_follow_on",
        )
    {
        anyhow::bail!("{platform} pulse eval-loop recommended task metadata is invalid");
    }
    let c85_status = json_string(&eval_loop["c85"], "status");
    if c85_status != "passed" && c85_status != "deferred" && c85_status != "unknown" {
        anyhow::bail!("{platform} pulse eval-loop c85 status is invalid");
    }
    validate_pulse_execution_side_effects_false(&eval_loop["side_effects"], platform, "eval-loop")?;
    if json_bool(&eval_loop["side_effects"], "repo_apply") {
        anyhow::bail!("{platform} pulse eval-loop repo_apply must be false");
    }
    validate_pulse_execution_trust_boundary(&eval_loop["trust_boundary"], platform, "eval-loop")?;
    Ok(())
}

fn validate_pulse_executor_platform_evidence(
    platform: &str,
    executor: &serde_json::Value,
    executor_sha256: &str,
    governed_task: &serde_json::Value,
    governed_task_sha256: &str,
    task_result: &serde_json::Value,
    task_result_sha256: &str,
) -> Result<()> {
    if executor_sha256.len() != 64
        || governed_task_sha256.len() != 64
        || task_result_sha256.len() != 64
    {
        anyhow::bail!("{platform} pulse executor evidence digest metadata is invalid");
    }
    if json_string(executor, "schema_version") != "ao2.pulse-executor.v1" {
        anyhow::bail!("{platform} pulse executor requires ao2.pulse-executor.v1");
    }
    if json_string(executor, "status") != "executed_governed_task" {
        anyhow::bail!("{platform} pulse executor status must be executed_governed_task");
    }
    if json_bool(&executor["selected_task"], "c85") {
        anyhow::bail!("{platform} pulse executor selected task must not be C85");
    }
    let c85_status = json_string(&executor["c85"], "status");
    if c85_status != "passed" && c85_status != "deferred" {
        anyhow::bail!("{platform} pulse executor c85 status is invalid");
    }
    if json_string(&executor["selected_task"], "classification") != "COMPLEX" {
        anyhow::bail!("{platform} pulse executor classification must be COMPLEX");
    }
    if json_string(&executor["selected_task"], "shape").is_empty() {
        anyhow::bail!("{platform} pulse executor selected task missing shape");
    }
    validate_pulse_execution_side_effects_false(&executor["side_effects"], platform, "executor")?;
    validate_pulse_execution_trust_boundary(&executor["trust_boundary"], platform, "executor")?;

    let task_contract = executor
        .get("task_contract")
        .context("pulse executor missing task_contract")?;
    if json_string(task_contract, "schema_version") != "ao2.pulse-task-contract.v1"
        || json_string(task_contract, "sha256").len() != 64
        || json_string(task_contract, "id") != json_string(&executor["selected_task"], "id")
    {
        anyhow::bail!("{platform} pulse executor task_contract metadata is invalid");
    }
    if json_string(&executor["artifacts"], "governed_task_evidence_sha256") != governed_task_sha256
        || json_string(&executor["artifacts"], "pulse_task_result_sha256") != task_result_sha256
    {
        anyhow::bail!("{platform} pulse executor artifact digests do not match supplied evidence");
    }

    if json_string(governed_task, "schema_version") != "ao2.pulse-governed-task.v1"
        || json_string(governed_task, "status") != "accepted"
    {
        anyhow::bail!(
            "{platform} governed task evidence must be accepted ao2.pulse-governed-task.v1"
        );
    }
    if json_string(&governed_task["task_contract"], "sha256")
        != json_string(task_contract, "sha256")
        || json_string(&governed_task["executed_task"], "execution_kind")
            != "governed_task_contract"
        || json_string(
            &governed_task["executed_task"]["evaluator_closer"],
            "release_acceptance_owner",
        ) != "factory-v3 evaluator-closer"
    {
        anyhow::bail!("{platform} governed task evidence is not evaluator/closer bound");
    }
    if json_string(&governed_task["c85"], "status") != c85_status {
        anyhow::bail!("{platform} governed task C85 status must match executor");
    }
    validate_pulse_execution_trust_boundary(
        &governed_task["trust_boundary"],
        platform,
        "governed task",
    )?;

    if json_string(task_result, "schema_version") != "ao2.pulse-task-result.v1"
        || json_string(task_result, "status") != "accepted"
        || json_string(task_result, "execution_mode") != "deterministic_local_evidence"
    {
        anyhow::bail!("{platform} pulse task result metadata is invalid");
    }
    if json_string(&task_result["task_contract"], "sha256") != json_string(task_contract, "sha256")
        || json_string(&task_result["governed_task_evidence"], "sha256") != governed_task_sha256
        || json_string(&task_result["evaluator_closer"], "release_acceptance_owner")
            != "factory-v3 evaluator-closer"
        || json_bool(&task_result["selected_task"], "c85")
    {
        anyhow::bail!("{platform} pulse task result is not evaluator/closer bound");
    }
    if json_string(&task_result["c85"], "status") != c85_status {
        anyhow::bail!("{platform} pulse task result C85 status must match executor");
    }
    validate_pulse_execution_side_effects_false(
        &task_result["side_effects"],
        platform,
        "task result",
    )?;
    validate_pulse_execution_trust_boundary(
        &task_result["trust_boundary"],
        platform,
        "task result",
    )?;
    Ok(())
}

fn validate_pulse_execution_side_effects_false(
    side_effects: &serde_json::Value,
    platform: &str,
    label: &str,
) -> Result<()> {
    for key in [
        "provider_execution",
        "queue_execution",
        "memory_write",
        "mutates_ao_artifacts",
        "hermes_cron_watchdog_mutation",
        "control_plane_mutation",
    ] {
        if json_bool(side_effects, key) {
            anyhow::bail!("{platform} pulse {label} side effect {key} must be false");
        }
    }
    Ok(())
}

fn validate_pulse_execution_trust_boundary(
    trust_boundary: &serde_json::Value,
    platform: &str,
    label: &str,
) -> Result<()> {
    if !json_bool(trust_boundary, "ao2_execution_evidence_owner")
        || !json_bool(trust_boundary, "factory_v3_evaluator_closer_reference")
        || !json_bool(trust_boundary, "control_plane_observer_only")
        || json_bool(trust_boundary, "control_plane_approves_release")
        || json_bool(trust_boundary, "control_plane_mutates_ao_artifacts")
    {
        anyhow::bail!("{platform} pulse {label} trust boundary is invalid");
    }
    Ok(())
}

fn current_git_head_string() -> Result<String> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("run git rev-parse HEAD")?;
    if !output.status.success() {
        anyhow::bail!("git rev-parse HEAD failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn plugin_pulse_apply_windows_recovery(
    options: plugin_cli::PluginPulseApplyWindowsRecoveryOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_apply_result_sha256 = options.apply_result_sha256.trim();
    let actual_apply_result_sha256 = sha256_file(&options.apply_result)?;
    if supplied_apply_result_sha256 != actual_apply_result_sha256 {
        anyhow::bail!(
            "pulse apply-result sha256 mismatch for {}: expected {}, actual {}",
            options.apply_result.display(),
            supplied_apply_result_sha256,
            actual_apply_result_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(&options.apply_result, "pulse-apply-result.json")?;
    let apply_result_text = fs::read_to_string(&options.apply_result)
        .with_context(|| format!("read {}", options.apply_result.display()))?;
    let apply_result: serde_json::Value = serde_json::from_str(&apply_result_text)
        .with_context(|| format!("parse {}", options.apply_result.display()))?;
    validate_pulse_apply_result_artifact(&apply_result, "windows")?;

    let supplied_observer_bundle_sha256 = options.observer_bundle_sha256.trim();
    let actual_observer_bundle_sha256 = sha256_file(&options.observer_bundle)?;
    if supplied_observer_bundle_sha256 != actual_observer_bundle_sha256 {
        anyhow::bail!(
            "pulse apply observer bundle sha256 mismatch for {}: expected {}, actual {}",
            options.observer_bundle.display(),
            supplied_observer_bundle_sha256,
            actual_observer_bundle_sha256
        );
    }
    let supplied_observer_archive_sha256 = options.observer_archive_sha256.trim();
    let actual_observer_archive_sha256 = sha256_file(&options.observer_archive)?;
    if supplied_observer_archive_sha256 != actual_observer_archive_sha256 {
        anyhow::bail!(
            "pulse apply observer archive sha256 mismatch for {}: expected {}, actual {}",
            options.observer_archive.display(),
            supplied_observer_archive_sha256,
            actual_observer_archive_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(
        &options.observer_bundle,
        "k37-pulse-apply-result-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.observer_archive,
        "k37-pulse-apply-result-observer-bundle.tar.gz",
    )?;
    let observer_bundle_text = fs::read_to_string(&options.observer_bundle)
        .with_context(|| format!("read {}", options.observer_bundle.display()))?;
    let observer_bundle: serde_json::Value = serde_json::from_str(&observer_bundle_text)
        .with_context(|| format!("parse {}", options.observer_bundle.display()))?;
    validate_pulse_apply_observer_bundle_summary(
        &observer_bundle,
        &actual_observer_archive_sha256,
    )?;
    let platform_apply_results = observer_bundle
        .get("platform_apply_results")
        .and_then(serde_json::Value::as_object)
        .context("pulse apply observer bundle missing platform_apply_results")?;
    let macos_apply_result_sha256 = json_string(
        platform_apply_results
            .get("macos")
            .context("pulse apply observer bundle missing macos apply result")?,
        "sha256",
    );
    let ubuntu_apply_result_sha256 = json_string(
        platform_apply_results
            .get("ubuntu")
            .context("pulse apply observer bundle missing ubuntu apply result")?,
        "sha256",
    );
    if macos_apply_result_sha256.len() != 64 || ubuntu_apply_result_sha256.len() != 64 {
        anyhow::bail!("pulse apply observer bundle platform sha256 metadata is invalid");
    }

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let inputs_dir = options.out_dir.join("inputs");
    fs::create_dir_all(&inputs_dir).with_context(|| format!("create {}", inputs_dir.display()))?;

    let copy_input = |source: &Path, file_name: &str, expected_sha256: &str, label: &str| {
        let target = inputs_dir.join(file_name);
        fs::copy(source, &target)
            .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
        let copied_sha256 = sha256_file(&target)?;
        if copied_sha256 != expected_sha256 {
            anyhow::bail!(
                "{label} digest changed while preparing pulse apply Windows recovery input: expected {expected_sha256}, copied {copied_sha256}"
            );
        }
        factory_app_run_bundle_reject_secret_markers(&target, label)?;
        Ok::<PathBuf, anyhow::Error>(target)
    };
    copy_input(
        &options.apply_result,
        "pulse-apply-result.json",
        &actual_apply_result_sha256,
        "pulse apply Windows recovery apply result",
    )?;
    copy_input(
        &options.observer_bundle,
        "k37-pulse-apply-result-observer-bundle.json",
        &actual_observer_bundle_sha256,
        "pulse apply Windows recovery observer bundle",
    )?;
    copy_input(
        &options.observer_archive,
        "k37-pulse-apply-result-observer-bundle.tar.gz",
        &actual_observer_archive_sha256,
        "pulse apply Windows recovery observer archive",
    )?;

    let script_path = options.out_dir.join("run-pulse-apply-proof.ps1");
    let script = format!(
        r#"param(
    [string]$Ao2 = "ao2",
    [string]$OutDir = (Join-Path $PSScriptRoot "pulse-apply-observer-bundle")
)

$ErrorActionPreference = "Stop"
$InputRoot = Join-Path $PSScriptRoot "inputs"
$InputSummary = Join-Path $InputRoot "k37-pulse-apply-result-observer-bundle.json"
$InputArchive = Join-Path $InputRoot "k37-pulse-apply-result-observer-bundle.tar.gz"
& $Ao2 plugin pulse-apply-observer-bundle-verify `
    --summary $InputSummary `
    --summary-sha256 "{actual_observer_bundle_sha256}" `
    --archive $InputArchive `
    --archive-sha256 "{actual_observer_archive_sha256}" `
    --json
if ($LASTEXITCODE -ne 0) {{
    exit $LASTEXITCODE
}}

$ExtractRoot = Join-Path $PSScriptRoot "observer-bundle"
if (Test-Path $ExtractRoot) {{
    Remove-Item -Recurse -Force $ExtractRoot
}}
New-Item -ItemType Directory -Force -Path $ExtractRoot | Out-Null
tar -xzf $InputArchive -C $ExtractRoot

$MacosApplyResult = Join-Path $ExtractRoot "platforms\macos\pulse-apply-result.json"
$UbuntuApplyResult = Join-Path $ExtractRoot "platforms\ubuntu\pulse-apply-result.json"
$WindowsApplyResult = Join-Path $InputRoot "pulse-apply-result.json"
& $Ao2 plugin pulse-apply-observer-bundle `
    --macos-apply-result $MacosApplyResult `
    --macos-sha256 "{macos_apply_result_sha256}" `
    --ubuntu-apply-result $UbuntuApplyResult `
    --ubuntu-sha256 "{ubuntu_apply_result_sha256}" `
    --windows-apply-result $WindowsApplyResult `
    --windows-sha256 "{actual_apply_result_sha256}" `
    --out-dir $OutDir `
    --json
if ($LASTEXITCODE -ne 0) {{
    exit $LASTEXITCODE
}}

$Summary = Join-Path $OutDir "k37-pulse-apply-result-observer-bundle.json"
$Archive = Join-Path $OutDir "k37-pulse-apply-result-observer-bundle.tar.gz"
$SummarySha256 = (Get-FileHash -Algorithm SHA256 $Summary).Hash.ToLowerInvariant()
$ArchiveSha256 = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant()
& $Ao2 plugin pulse-apply-observer-bundle-verify `
    --summary $Summary `
    --summary-sha256 $SummarySha256 `
    --archive $Archive `
    --archive-sha256 $ArchiveSha256 `
    --json
if ($LASTEXITCODE -ne 0) {{
    exit $LASTEXITCODE
}}
"#,
        macos_apply_result_sha256 = macos_apply_result_sha256,
        ubuntu_apply_result_sha256 = ubuntu_apply_result_sha256,
        actual_apply_result_sha256 = actual_apply_result_sha256,
        actual_observer_bundle_sha256 = actual_observer_bundle_sha256,
        actual_observer_archive_sha256 = actual_observer_archive_sha256
    );
    atomic_write_text(&script_path, &script)?;
    factory_app_run_bundle_reject_secret_markers(&script_path, "run-pulse-apply-proof.ps1")?;
    let script_sha256 = sha256_file(&script_path)?;

    let manifest_path = options.out_dir.join("windows-pulse-apply-recovery.json");
    let side_effects = serde_json::json!({
        "provider_execution_started": false,
        "queue_mutated": false,
        "memory_written": false,
        "control_plane_mutated": false,
        "ao_artifacts_mutated": false,
        "release_approved": false
    });
    let manifest = serde_json::json!({
        "schema_version": "ao2.pulse-apply-windows-recovery.v1",
        "status": "ready_for_windows_execution",
        "platform": "windows",
        "manifest_path": manifest_path.display().to_string(),
        "script_path": script_path.display().to_string(),
        "script_sha256": script_sha256,
        "execution": {
            "runner": "powershell",
            "single_session_command": "powershell -ExecutionPolicy Bypass -File .\\run-pulse-apply-proof.ps1",
            "ao2_argument": "-Ao2 <path-to-ao2.exe-or-ao2>",
            "output_argument": "-OutDir <windows-output-dir>",
            "produces": [
                "ao2.pulse-apply-result.v1",
                "ao2.k37-pulse-apply-result-observer-bundle.v1"
            ]
        },
        "pulse_apply_result": {
            "source_path": options.apply_result.display().to_string(),
            "source_sha256": actual_apply_result_sha256,
            "portable_path": "inputs/pulse-apply-result.json",
            "schema_version": json_string(&apply_result, "schema_version"),
            "status": json_string(&apply_result, "status"),
            "execution_mode": json_string(&apply_result, "execution_mode")
        },
        "observer_bundle": {
            "summary_path": "inputs/k37-pulse-apply-result-observer-bundle.json",
            "summary_sha256": actual_observer_bundle_sha256,
            "archive_path": "inputs/k37-pulse-apply-result-observer-bundle.tar.gz",
            "archive_sha256": actual_observer_archive_sha256,
            "source_schema_version": json_string(&observer_bundle, "schema_version"),
            "source_platforms": observer_bundle.get("platforms").cloned().unwrap_or_else(|| serde_json::json!([])),
            "macos_apply_result_sha256": macos_apply_result_sha256,
            "ubuntu_apply_result_sha256": ubuntu_apply_result_sha256
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
            "control_plane_approves_release": false
        },
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "side_effects": side_effects,
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    atomic_write_text(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
    factory_app_run_bundle_reject_secret_markers(
        &manifest_path,
        "windows-pulse-apply-recovery.json",
    )?;
    let manifest_sha256 = sha256_file(&manifest_path)?;

    let mut response = manifest;
    response["manifest_sha256"] = serde_json::json!(manifest_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_windows_execution");
        println!("schema_version=ao2.pulse-apply-windows-recovery.v1");
        println!("manifest={}", manifest_path.display());
        println!("script={}", script_path.display());
    }
    Ok(())
}

fn validate_pulse_apply_observer_bundle_summary(
    summary: &serde_json::Value,
    archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.k37-pulse-apply-result-observer-bundle.v1" {
        anyhow::bail!(
            "pulse apply observer bundle requires ao2.k37-pulse-apply-result-observer-bundle.v1, got {}",
            json_string(summary, "schema_version")
        );
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!("pulse apply observer bundle status must be ready_for_k37_observation");
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("pulse apply observer bundle producer must be ao2");
    }
    if json_string(summary, "archive_sha256") != archive_sha256 {
        anyhow::bail!("pulse apply observer bundle archive sha256 does not match");
    }
    let platforms = summary
        .get("platforms")
        .and_then(serde_json::Value::as_array)
        .context("pulse apply observer bundle missing platforms")?;
    let platform_count = json_u64(summary, "platform_count");
    if platform_count != platforms.len() as u64 || !(platform_count == 2 || platform_count == 3) {
        anyhow::bail!("pulse apply observer bundle platform_count must be 2 or 3");
    }
    for required in ["macos", "ubuntu"] {
        if !platforms
            .iter()
            .any(|platform| platform.as_str() == Some(required))
        {
            anyhow::bail!("pulse apply observer bundle missing required platform {required}");
        }
    }
    let has_windows = platforms
        .iter()
        .any(|platform| platform.as_str() == Some("windows"));
    if !has_windows {
        let reason = summary["unavailable_platforms"]["windows"]["reason"]
            .as_str()
            .unwrap_or("")
            .trim();
        if reason.is_empty() {
            anyhow::bail!(
                "pulse apply observer bundle must record Windows unavailable reason when absent"
            );
        }
    }
    let observed_scope = summary
        .get("observed_evidence_scope")
        .and_then(serde_json::Value::as_array)
        .context("pulse apply observer bundle missing observed_evidence_scope")?;
    if !observed_scope
        .iter()
        .any(|entry| entry.as_str() == Some("ao2.pulse-apply-result.v1"))
    {
        anyhow::bail!("pulse apply observer bundle missing apply-result scope");
    }
    let platform_apply_results = summary
        .get("platform_apply_results")
        .context("pulse apply observer bundle missing platform_apply_results")?;
    if json_string(summary, "platform_apply_results_sha256")
        != canonical_json_sha256(platform_apply_results)
    {
        anyhow::bail!("pulse apply observer bundle platform digest mismatch");
    }
    for platform in platforms {
        let platform = platform
            .as_str()
            .ok_or_else(|| anyhow!("pulse apply observer bundle platform must be a string"))?;
        if platform_apply_results.get(platform).is_none() {
            anyhow::bail!("pulse apply observer bundle missing {platform}");
        }
    }
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("pulse apply observer bundle missing trust_boundary")?,
        "pulse apply observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("pulse apply observer bundle missing control_plane_observation")?,
        "pulse apply observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("pulse apply observer bundle missing side_effects")?,
        "pulse apply observer bundle",
    )?;
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("pulse apply observer bundle token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("pulse apply observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

fn validate_pulse_apply_result_artifact(
    apply_result: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(apply_result, "schema_version") != "ao2.pulse-apply-result.v1" {
        anyhow::bail!(
            "{platform} pulse apply-result requires ao2.pulse-apply-result.v1, got {}",
            json_string(apply_result, "schema_version")
        );
    }
    if json_string(apply_result, "status") != "accepted" {
        anyhow::bail!("{platform} pulse apply-result must be accepted");
    }
    if json_string(apply_result, "execution_mode") != "bounded_planned_file_apply" {
        anyhow::bail!("{platform} pulse apply-result execution_mode is invalid");
    }
    if json_bool(&apply_result["selected_task"], "c85") {
        anyhow::bail!("{platform} pulse apply-result must not be C85");
    }
    if json_string(&apply_result["evaluator_closer"], "status") != "accepted"
        || json_string(
            &apply_result["evaluator_closer"],
            "release_acceptance_owner",
        ) != "factory-v3 evaluator-closer"
    {
        anyhow::bail!("{platform} pulse apply-result evaluator/closer is not accepted");
    }
    for field in [
        "dry_run_task",
        "prior_chain",
        "task_contract",
        "governed_task_evidence",
        "task_result",
    ] {
        if apply_result.get(field).is_none() {
            anyhow::bail!("{platform} pulse apply-result missing {field}");
        }
    }
    let operations = apply_result
        .get("applied_file_operations")
        .and_then(serde_json::Value::as_array)
        .context("pulse apply-result missing applied_file_operations")?;
    if operations.is_empty() {
        anyhow::bail!("{platform} pulse apply-result must include applied operations");
    }
    for operation in operations {
        if !json_bool(operation, "allowed_by_dry_run") || !json_bool(operation, "executed") {
            anyhow::bail!("{platform} pulse apply-result operation was not allowed/executed");
        }
    }
    let trust_boundary = apply_result
        .get("trust_boundary")
        .context("pulse apply-result missing trust_boundary")?;
    if !json_bool(trust_boundary, "ao2_execution_evidence_owner")
        || !json_bool(trust_boundary, "factory_v3_evaluator_closer_reference")
        || !json_bool(trust_boundary, "control_plane_observer_only")
        || json_bool(trust_boundary, "control_plane_approves_release")
        || json_bool(trust_boundary, "control_plane_mutates_ao_artifacts")
    {
        anyhow::bail!("{platform} pulse apply-result trust boundary is invalid");
    }
    let side_effects = apply_result
        .get("side_effects")
        .context("pulse apply-result missing side_effects")?;
    for field in [
        "provider_execution",
        "queue_execution",
        "memory_write",
        "mutates_ao_artifacts",
        "hermes_cron_watchdog_mutation",
        "control_plane_mutation",
    ] {
        if json_bool(side_effects, field) {
            anyhow::bail!("{platform} pulse apply-result side effect must be false: {field}");
        }
    }
    Ok(())
}
