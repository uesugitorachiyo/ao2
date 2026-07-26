use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::cli_util::{canonical_json_sha256, fail_if_provider_api_key_env_present, json_u64};
use super::plugin_cli;
use super::plugin_contract::{
    validate_k37_plugin_observer_bundle, validate_k37_plugin_observer_input,
    validate_plugin_adapter_file, validate_plugin_adapter_install_smoke_contract,
    validate_plugin_adapter_install_smoke_verification, validate_plugin_adapter_scaffold_summary,
    validate_plugin_adapter_scaffold_verification, validate_plugin_consumer_lifecycle_contract,
    validate_plugin_consumer_lifecycle_observer_bundle_summary,
    validate_plugin_control_plane_fixture_handoff, validate_plugin_control_plane_observation,
    validate_plugin_distribution_rehearsal_summary,
    validate_plugin_packaged_replacement_hardening_proof,
    validate_plugin_packaged_replacement_observer_bundle_summary,
    validate_plugin_side_effects_false,
};
use super::plugin_distribution::{
    plugin_package_archive_json, read_plugin_package_archive_files, sha256_archive_file,
    validate_plugin_observer_trust_boundary, validate_plugin_package_contract,
    validate_plugin_provider_auth,
};
use super::{
    atomic_write_text, create_tar_gz, factory_app_run_bundle_reject_secret_markers, is_sha256_hex,
    json_bool, json_string, sha256_file, validate_release_gate_with_replacement_rollup,
};

pub(super) fn plugin_control_plane_fixture_handoff(
    options: plugin_cli::PluginControlPlaneFixtureHandoffOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "consumer lifecycle observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "consumer lifecycle observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-plugin-consumer-lifecycle-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-plugin-consumer-lifecycle-observer-bundle.tar.gz",
    )?;
    let summary_text = fs::read_to_string(&options.summary)
        .with_context(|| format!("read {}", options.summary.display()))?;
    let summary: serde_json::Value = serde_json::from_str(&summary_text)
        .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_consumer_lifecycle_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_lifecycles = summary
        .get("platform_lifecycles")
        .and_then(serde_json::Value::as_object)
        .context("consumer lifecycle observer bundle missing platform_lifecycles")?;
    for platform in ["macos", "ubuntu", "windows"] {
        let archive_path = format!("platforms/{platform}/plugin-consumer-lifecycle.json");
        let lifecycle =
            plugin_package_archive_json(&archive_files, &archive_path, "bundled lifecycle")?;
        validate_plugin_consumer_lifecycle_contract(&lifecycle, platform)?;
        let lifecycle_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        let summary_lifecycle = platform_lifecycles
            .get(platform)
            .with_context(|| format!("consumer lifecycle observer bundle missing {platform}"))?;
        if lifecycle_sha256 != json_string(summary_lifecycle, "sha256") {
            anyhow::bail!(
                "{platform} consumer lifecycle observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_lifecycle, "sha256"),
                lifecycle_sha256
            );
        }
    }

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let fixture_dir = options.out_dir.join("control-plane-fixture");
    fs::create_dir_all(&fixture_dir)
        .with_context(|| format!("create {}", fixture_dir.display()))?;
    let fixture_path = fixture_dir.join("consumer-lifecycle-observer-bundle.json");
    atomic_write_text(&fixture_path, &summary_text)?;
    factory_app_run_bundle_reject_secret_markers(
        &fixture_path,
        "consumer-lifecycle-observer-bundle.json",
    )?;
    let fixture_sha256 = sha256_file(&fixture_path)?;
    if fixture_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "control-plane fixture digest mismatch: source {}, fixture {}",
            actual_summary_sha256,
            fixture_sha256
        );
    }

    let recommended_fixture_path =
        "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/consumer-lifecycle-observer-bundle.json";
    let recommended_test_name =
        "consumer_lifecycle_observer_bundle_is_read_only_three_platform_evidence";
    let handoff_path = options
        .out_dir
        .join("ao2-control-plane-fixture-handoff.json");
    let handoff = serde_json::json!({
        "schema_version": "ao2.control-plane-fixture-handoff.v1",
        "status": "ready_for_control_plane_readback",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "source_schema_version": "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "handoff_path": handoff_path.display().to_string(),
        "fixture": {
            "path": fixture_path.display().to_string(),
            "sha256": fixture_sha256,
            "recommended_control_plane_path": recommended_fixture_path
        },
        "recommended_control_plane_fixture_path": recommended_fixture_path,
        "recommended_control_plane_test_name": recommended_test_name,
        "expected_schema_version": "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
        "expected_status": "ready_for_k37_observation",
        "expected_platforms": ["macos", "ubuntu", "windows"],
        "expected_platform_count": 3,
        "expected_observed_evidence_scope": ["ao2.plugin-consumer-lifecycle.v1"],
        "control_plane_readback_assertions": {
            "assert_platform_lifecycles": ["macos", "ubuntu", "windows"],
            "assert_target_pass_states": ["codex", "claude"],
            "assert_provider_auth_local_oauth_cli_only": true,
            "assert_provider_api_key_auth_allowed": false,
            "assert_control_plane_role": "read_only_observer",
            "assert_mutates_ao_artifacts": false,
            "assert_control_plane_approves_release": false,
            "assert_false_side_effects": [
                "would_execute_provider",
                "would_execute_queue",
                "would_write_memory",
                "would_mutate_control_plane",
                "would_mutate_ao_artifacts",
                "would_approve_release"
            ]
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
    let handoff_body = serde_json::to_string_pretty(&handoff)?;
    atomic_write_text(&handoff_path, &handoff_body)?;
    factory_app_run_bundle_reject_secret_markers(
        &handoff_path,
        "ao2-control-plane-fixture-handoff.json",
    )?;
    let handoff_sha256 = sha256_file(&handoff_path)?;

    let mut response = handoff;
    response["handoff_sha256"] = serde_json::json!(handoff_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_control_plane_readback");
        println!("schema_version=ao2.control-plane-fixture-handoff.v1");
        println!("handoff={}", handoff_path.display());
        println!("fixture={}", fixture_path.display());
    }
    Ok(())
}

pub(super) fn plugin_control_plane_fixture_handoff_verify(
    options: plugin_cli::PluginControlPlaneFixtureHandoffVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_handoff_sha256 = options.handoff_sha256.trim();
    let actual_handoff_sha256 = sha256_file(&options.handoff)?;
    if supplied_handoff_sha256 != actual_handoff_sha256 {
        anyhow::bail!(
            "control-plane fixture handoff sha256 mismatch for {}: expected {}, actual {}",
            options.handoff.display(),
            supplied_handoff_sha256,
            actual_handoff_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.handoff,
        "ao2-control-plane-fixture-handoff.json",
    )?;
    let handoff_text = fs::read_to_string(&options.handoff)
        .with_context(|| format!("read {}", options.handoff.display()))?;
    let handoff: serde_json::Value = serde_json::from_str(&handoff_text)
        .with_context(|| format!("parse {}", options.handoff.display()))?;
    validate_plugin_control_plane_fixture_handoff(&handoff)?;

    let summary_path = PathBuf::from(json_string(&handoff, "summary_path"));
    let archive_path = PathBuf::from(json_string(&handoff, "archive_path"));
    let fixture_path = PathBuf::from(json_string(&handoff["fixture"], "path"));
    let actual_summary_sha256 = sha256_file(&summary_path)?;
    if actual_summary_sha256 != json_string(&handoff, "summary_sha256") {
        anyhow::bail!(
            "control-plane fixture handoff summary sha256 mismatch: expected {}, actual {}",
            json_string(&handoff, "summary_sha256"),
            actual_summary_sha256
        );
    }
    let actual_archive_sha256 = sha256_file(&archive_path)?;
    if actual_archive_sha256 != json_string(&handoff, "archive_sha256") {
        anyhow::bail!(
            "control-plane fixture handoff archive sha256 mismatch: expected {}, actual {}",
            json_string(&handoff, "archive_sha256"),
            actual_archive_sha256
        );
    }
    let fixture_sha256 = sha256_file(&fixture_path)?;
    if fixture_sha256 != json_string(&handoff["fixture"], "sha256") {
        anyhow::bail!(
            "control-plane fixture handoff fixture sha256 mismatch: expected {}, actual {}",
            json_string(&handoff["fixture"], "sha256"),
            fixture_sha256
        );
    }
    if fixture_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "control-plane fixture handoff fixture must match source summary digest: fixture {}, summary {}",
            fixture_sha256,
            actual_summary_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "k37-plugin-consumer-lifecycle-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-consumer-lifecycle-observer-bundle.tar.gz",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &fixture_path,
        "consumer-lifecycle-observer-bundle.json",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&summary_path)
            .with_context(|| format!("read {}", summary_path.display()))?,
    )
    .with_context(|| format!("parse {}", summary_path.display()))?;
    validate_plugin_consumer_lifecycle_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    if let Some(parent) = options.out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
    }
    let verification = serde_json::json!({
        "schema_version": "ao2.control-plane-fixture-handoff-verification.v1",
        "status": "passed",
        "handoff_path": options.handoff.display().to_string(),
        "handoff_sha256": actual_handoff_sha256,
        "source_schema_version": json_string(&handoff, "source_schema_version"),
        "source_summary": {
            "path": summary_path.display().to_string(),
            "sha256": actual_summary_sha256
        },
        "source_archive": {
            "path": archive_path.display().to_string(),
            "sha256": actual_archive_sha256
        },
        "fixture": {
            "path": fixture_path.display().to_string(),
            "sha256": fixture_sha256,
            "recommended_control_plane_path": json_string(&handoff["fixture"], "recommended_control_plane_path")
        },
        "recommended_control_plane_fixture_path": json_string(&handoff, "recommended_control_plane_fixture_path"),
        "recommended_control_plane_test_name": json_string(&handoff, "recommended_control_plane_test_name"),
        "expected_schema_version": json_string(&handoff, "expected_schema_version"),
        "expected_status": json_string(&handoff, "expected_status"),
        "expected_platforms": handoff.get("expected_platforms").cloned().unwrap_or_else(|| serde_json::json!([])),
        "expected_platform_count": json_u64(&handoff, "expected_platform_count"),
        "expected_observed_evidence_scope": handoff.get("expected_observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "control_plane_readback_assertions": handoff.get("control_plane_readback_assertions").cloned().unwrap_or_else(|| serde_json::json!({})),
        "provider_auth": handoff["provider_auth"].clone(),
        "trust_boundary": handoff["trust_boundary"].clone(),
        "control_plane_observation": handoff["control_plane_observation"].clone(),
        "side_effects": handoff["side_effects"].clone(),
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor",
        "verification_path": options.out.display().to_string()
    });
    let body = serde_json::to_string_pretty(&verification)?;
    atomic_write_text(&options.out, &body)?;
    factory_app_run_bundle_reject_secret_markers(
        &options.out,
        "control-plane-fixture-handoff-verification.json",
    )?;
    let verification_sha256 = sha256_file(&options.out)?;

    let mut response = verification;
    response["verification_sha256"] = serde_json::json!(verification_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.control-plane-fixture-handoff-verification.v1");
        println!("verification={}", options.out.display());
    }
    Ok(())
}

pub(super) fn plugin_distribution_observer_bundle(
    options: plugin_cli::PluginDistributionObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = [
        (
            "macos",
            options.macos_observer,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_observer,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_observer,
            options.windows_sha256.trim().to_string(),
        ),
    ];
    let mut platform_inputs = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} observer input sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} k37-plugin-observer-input.json"),
        )?;
        let input_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let input: serde_json::Value = serde_json::from_str(&input_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_k37_plugin_observer_input(&input, platform)?;

        let bundled_path = platforms_root
            .join(platform)
            .join("k37-plugin-observer-input.json");
        atomic_write_text(&bundled_path, &serde_json::to_string_pretty(&input)?)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled k37-plugin-observer-input.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} observer input changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_inputs.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&input, "schema_version"),
                "status": json_string(&input, "status"),
                "producer": json_string(&input, "producer"),
                "package_summary_sha256": json_string(&input, "package_summary_sha256"),
                "package_archive_sha256": json_string(&input, "package_archive_sha256"),
                "target_results": input.get("target_results").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": input.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": input.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({})),
                "factory_v3_role": json_string(&input, "factory_v3_role")
            }),
        );
    }

    let archive_path = options.out_dir.join("k37-plugin-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-observer-bundle.tar.gz",
    )?;

    let summary_path = options.out_dir.join("k37-plugin-observer-bundle.json");
    let platform_inputs_value = serde_json::Value::Object(platform_inputs);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-plugin-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "platform_inputs": platform_inputs_value,
        "platform_inputs_sha256": canonical_json_sha256(&platform_inputs_value),
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
    factory_app_run_bundle_reject_secret_markers(&summary_path, "k37-plugin-observer-bundle.json")?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-plugin-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_clean_package_operator_index(
    options: plugin_cli::PluginCleanPackageOperatorIndexOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = [
        (
            "macos",
            options.macos_rehearsal,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_rehearsal,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_rehearsal,
            options.windows_sha256.trim().to_string(),
        ),
    ];
    let mut platform_rehearsals = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} clean package rehearsal sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} plugin-distribution-rehearsal.json"),
        )?;
        let input_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let rehearsal: serde_json::Value = serde_json::from_str(&input_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_plugin_distribution_rehearsal_summary(&rehearsal, platform)?;

        let platform_dir = platforms_root.join(platform);
        fs::create_dir_all(&platform_dir)
            .with_context(|| format!("create {}", platform_dir.display()))?;
        let bundled_path = platform_dir.join("plugin-distribution-rehearsal.json");
        atomic_write_text(&bundled_path, &input_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled plugin-distribution-rehearsal.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} clean package rehearsal changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_rehearsals.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&rehearsal, "schema_version"),
                "status": json_string(&rehearsal, "status"),
                "targets": rehearsal.get("targets").cloned().unwrap_or_else(|| serde_json::json!([])),
                "target_results": rehearsal.get("target_results").cloned().unwrap_or_else(|| serde_json::json!({})),
                "package_summary_sha256": json_string(&rehearsal, "summary_sha256"),
                "package_archive_sha256": json_string(&rehearsal, "archive_sha256"),
                "observer_input": rehearsal.get("observer_input").cloned().unwrap_or_else(|| serde_json::json!({})),
                "provider_auth": rehearsal.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": rehearsal.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": rehearsal.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({})),
                "factory_v3_role": json_string(&rehearsal, "factory_v3_role"),
                "token_safe_output_verified": json_bool(&rehearsal, "token_safe_output_verified")
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-clean-package-operator-index.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-clean-package-operator-index.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-clean-package-operator-index.json");
    let platform_rehearsals_value = serde_json::Value::Object(platform_rehearsals);
    let provider_auth = serde_json::json!({
        "local_oauth_cli_only": true,
        "provider_api_key_auth_allowed": false,
        "provider_api_key_env_required": false
    });
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-clean-package-operator-index.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "plugin_targets": ["codex", "claude"],
        "observed_evidence_scope": [
            "ao2.plugin-distribution-rehearsal.v1",
            "ao2.k37-plugin-observer-input.v1",
            "ao2.plugin-package-verification.v1",
            "ao2.plugin-manifest-verification.v1",
            "ao2.plugin-install-smoke.v1"
        ],
        "platform_rehearsals": platform_rehearsals_value,
        "platform_rehearsals_sha256": canonical_json_sha256(&platform_rehearsals_value),
        "provider_auth": provider_auth,
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
        "k37-clean-package-operator-index.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-clean-package-operator-index.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_packaged_replacement_observer_bundle(
    options: plugin_cli::PluginPackagedReplacementObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = [
        (
            "macos",
            options.macos_proof,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_proof,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_proof,
            options.windows_sha256.trim().to_string(),
        ),
    ];
    let mut platform_proofs = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} packaged replacement proof sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} packaged-replacement-hardening.json"),
        )?;
        let input_bytes =
            fs::read(&source_path).with_context(|| format!("read {}", source_path.display()))?;
        let raw_input_text = String::from_utf8(input_bytes)
            .with_context(|| format!("read UTF-8 {}", source_path.display()))?;
        let input_text = raw_input_text.trim_start_matches('\u{feff}').to_string();
        let normalized_utf8_bom = input_text.len() != raw_input_text.len();
        let proof: serde_json::Value = serde_json::from_str(&input_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_plugin_packaged_replacement_hardening_proof(&proof, platform)?;

        let platform_dir = platforms_root.join(platform);
        fs::create_dir_all(&platform_dir)
            .with_context(|| format!("create {}", platform_dir.display()))?;
        let bundled_path = platform_dir.join("packaged-replacement-hardening.json");
        atomic_write_text(&bundled_path, &input_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled packaged-replacement-hardening.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if !normalized_utf8_bom && bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} packaged replacement proof changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_proofs.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "source_sha256": actual_sha256,
                "sha256": bundled_sha256,
                "normalized_utf8_bom": normalized_utf8_bom,
                "schema_version": json_string(&proof, "schema_version"),
                "status": json_string(&proof, "status"),
                "package": proof.get("package").cloned().unwrap_or_else(|| serde_json::json!({})),
                "factory_replacement": proof.get("factory_replacement").cloned().unwrap_or_else(|| serde_json::json!({})),
                "closer_decision": proof.get("closer_decision").cloned().unwrap_or_else(|| serde_json::json!({})),
                "provider_auth": proof.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": proof.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": proof.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({
                    "role": "read_only_observer",
                    "may_observe_evidence_bundle_path": true,
                    "may_mutate_evidence": false,
                    "may_approve_release": false
                })),
                "side_effects": proof.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
                "token_safe_output": proof.get("token_safe_output").cloned().unwrap_or_else(|| serde_json::json!({}))
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-packaged-replacement-hardening-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-packaged-replacement-hardening-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-packaged-replacement-hardening-observer-bundle.json");
    let platform_proofs_value = serde_json::Value::Object(platform_proofs);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-packaged-replacement-hardening-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 factory-v3 replacement hardening",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": [
            "ao2.packaged-replacement-hardening.v1",
            "ao2.factory-closer-decision.v1",
            "ao2.factory-closer-decision-verification.v1"
        ],
        "platform_proofs": platform_proofs_value,
        "platform_proofs_sha256": canonical_json_sha256(&platform_proofs_value),
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
        "k37-packaged-replacement-hardening-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-packaged-replacement-hardening-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_packaged_replacement_observer_bundle_verify(
    options: plugin_cli::PluginPackagedReplacementObserverBundleVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "packaged replacement observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }
    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "packaged replacement observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-packaged-replacement-hardening-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-packaged-replacement-hardening-observer-bundle.tar.gz",
    )?;
    let summary_text = fs::read_to_string(&options.summary)
        .with_context(|| format!("read {}", options.summary.display()))?;
    let summary: serde_json::Value = serde_json::from_str(&summary_text)
        .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_packaged_replacement_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_proofs = summary
        .get("platform_proofs")
        .and_then(serde_json::Value::as_object)
        .context("packaged replacement observer bundle missing platform_proofs")?;
    for platform in ["macos", "ubuntu", "windows"] {
        let archive_path = format!("platforms/{platform}/packaged-replacement-hardening.json");
        let proof = plugin_package_archive_json(
            &archive_files,
            &archive_path,
            "bundled packaged replacement proof",
        )?;
        validate_plugin_packaged_replacement_hardening_proof(&proof, platform)?;
        let proof_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        let summary_proof = platform_proofs
            .get(platform)
            .with_context(|| format!("packaged replacement observer bundle missing {platform}"))?;
        if proof_sha256 != json_string(summary_proof, "sha256") {
            anyhow::bail!(
                "{platform} packaged replacement archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_proof, "sha256"),
                proof_sha256
            );
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-packaged-replacement-hardening-observer-bundle-verification.v1",
        "status": "passed",
        "producer": "ao2",
        "source_schema_version": "ao2.k37-packaged-replacement-hardening-observer-bundle.v1",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "platform_proofs_sha256": json_string(&summary, "platform_proofs_sha256"),
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
    let response_body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!(
            "schema_version=ao2.k37-packaged-replacement-hardening-observer-bundle-verification.v1"
        );
        println!("summary={}", options.summary.display());
        println!("archive={}", options.archive.display());
    }
    Ok(())
}

pub(super) fn plugin_release_gate_with_replacement_observer_bundle(
    options: plugin_cli::PluginReleaseGateWithReplacementObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = [
        (
            "macos",
            options.macos_rollup,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_rollup,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_rollup,
            options.windows_sha256.trim().to_string(),
        ),
    ];
    let mut platform_rollups = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} release-gate-with-replacement rollup sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} release-gate-with-replacement-rollup.json"),
        )?;
        let input_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let rollup: serde_json::Value = serde_json::from_str(&input_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_release_gate_with_replacement_rollup(&rollup, platform)?;

        let platform_dir = platforms_root.join(platform);
        fs::create_dir_all(&platform_dir)
            .with_context(|| format!("create {}", platform_dir.display()))?;
        let bundled_path = platform_dir.join("release-gate-with-replacement-rollup.json");
        atomic_write_text(&bundled_path, &input_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled release-gate-with-replacement-rollup.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} release-gate rollup changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_rollups.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&rollup, "schema_version"),
                "overall_verdict": json_string(&rollup, "overall_verdict"),
                "ao2_git_head": json_string(&rollup, "ao2_git_head"),
                "counts": rollup.get("counts").cloned().unwrap_or_else(|| serde_json::json!({})),
                "stages": rollup.get("stages").cloned().unwrap_or_else(|| serde_json::json!([])),
                "trust_boundary": rollup.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({}))
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-release-gate-with-replacement-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-release-gate-with-replacement-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-release-gate-with-replacement-observer-bundle.json");
    let platform_rollups_value = serde_json::Value::Object(platform_rollups);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron release-gate self-contained scratch proof",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": [
            "ao2.release-gate-with-replacement-parity.v1"
        ],
        "platform_rollups": platform_rollups_value,
        "platform_rollups_sha256": canonical_json_sha256(&platform_rollups_value),
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
        "k37-release-gate-with-replacement-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-release-gate-with-replacement-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_adapter_scaffold(
    options: plugin_cli::PluginAdapterScaffoldOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_package_summary_sha256 = options.package_summary_sha256.trim();
    let actual_package_summary_sha256 = sha256_file(&options.package_summary)?;
    if supplied_package_summary_sha256 != actual_package_summary_sha256 {
        anyhow::bail!(
            "plugin package summary sha256 mismatch for {}: expected {}, actual {}",
            options.package_summary.display(),
            supplied_package_summary_sha256,
            actual_package_summary_sha256
        );
    }
    let supplied_package_archive_sha256 = options.package_archive_sha256.trim();
    let actual_package_archive_sha256 = sha256_file(&options.package_archive)?;
    if supplied_package_archive_sha256 != actual_package_archive_sha256 {
        anyhow::bail!(
            "plugin package archive sha256 mismatch for {}: expected {}, actual {}",
            options.package_archive.display(),
            supplied_package_archive_sha256,
            actual_package_archive_sha256
        );
    }
    let package_summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.package_summary)
            .with_context(|| format!("read {}", options.package_summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.package_summary.display()))?;
    validate_plugin_package_contract(&package_summary)?;
    if json_string(
        package_summary
            .get("archive")
            .context("plugin package summary missing archive")?,
        "sha256",
    ) != actual_package_archive_sha256
    {
        anyhow::bail!("plugin package summary archive.sha256 does not match package archive");
    }

    let supplied_k37_bundle_sha256 = options.k37_bundle_sha256.trim();
    let actual_k37_bundle_sha256 = sha256_file(&options.k37_bundle)?;
    if supplied_k37_bundle_sha256 != actual_k37_bundle_sha256 {
        anyhow::bail!(
            "K37 observer bundle sha256 mismatch for {}: expected {}, actual {}",
            options.k37_bundle.display(),
            supplied_k37_bundle_sha256,
            actual_k37_bundle_sha256
        );
    }
    let supplied_k37_archive_sha256 = options.k37_archive_sha256.trim();
    let actual_k37_archive_sha256 = sha256_file(&options.k37_archive)?;
    if supplied_k37_archive_sha256 != actual_k37_archive_sha256 {
        anyhow::bail!(
            "K37 observer archive sha256 mismatch for {}: expected {}, actual {}",
            options.k37_archive.display(),
            supplied_k37_archive_sha256,
            actual_k37_archive_sha256
        );
    }
    let k37_bundle: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.k37_bundle)
            .with_context(|| format!("read {}", options.k37_bundle.display()))?,
    )
    .with_context(|| format!("parse {}", options.k37_bundle.display()))?;
    validate_k37_plugin_observer_bundle(&k37_bundle)?;
    if json_string(&k37_bundle, "archive_sha256") != actual_k37_archive_sha256 {
        anyhow::bail!("K37 observer bundle archive_sha256 does not match archive");
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.package_summary,
        "ao2-plugin-package.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.k37_bundle,
        "k37-plugin-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.package_archive,
        "ao2-plugin-package.tar.gz",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.k37_archive,
        "k37-plugin-observer-bundle.tar.gz",
    )?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;

    let commands = serde_json::json!({
        "readiness": "ao2 plugin readiness --json",
        "package_verify": "ao2 plugin package-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "distribution_observer_bundle": "ao2 plugin distribution-observer-bundle --macos-observer <path> --macos-sha256 <sha256> --ubuntu-observer <path> --ubuntu-sha256 <sha256> --windows-observer <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "consumer_lifecycle_observer_bundle": "ao2 plugin consumer-lifecycle-observer-bundle --macos-lifecycle <path> --macos-sha256 <sha256> --ubuntu-lifecycle <path> --ubuntu-sha256 <sha256> --windows-lifecycle <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "consumer_lifecycle_observer_bundle_verify": "ao2 plugin consumer-lifecycle-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "control_plane_fixture_handoff": "ao2 plugin control-plane-fixture-handoff --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "control_plane_fixture_handoff_verify": "ao2 plugin control-plane-fixture-handoff-verify --handoff <path> --handoff-sha256 <sha256> --out <path> --json",
        "release_candidate": "ao2 plugin release-candidate --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --distribution-rehearsal <path> --distribution-rehearsal-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-gate-with-replacement-observer-bundle <path> --release-gate-with-replacement-observer-bundle-sha256 <sha256> --release-gate-with-replacement-observer-archive <path> --release-gate-with-replacement-observer-archive-sha256 <sha256> --control-plane-fixture-handoff-verification <path> --control-plane-fixture-handoff-verification-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "release_candidate_verify": "ao2 plugin release-candidate-verify --summary <path> --summary-sha256 <sha256> --json",
        "release_candidate_windows_recovery": "ao2 plugin release-candidate-windows-recovery --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --distribution-rehearsal <path> --distribution-rehearsal-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-gate-with-replacement-observer-bundle <path> --release-gate-with-replacement-observer-bundle-sha256 <sha256> --release-gate-with-replacement-observer-archive <path> --release-gate-with-replacement-observer-archive-sha256 <sha256> --control-plane-fixture-handoff-verification <path> --control-plane-fixture-handoff-verification-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "release_candidate_windows_recovery_verify": "ao2 plugin release-candidate-windows-recovery-verify --recovery <path> --recovery-sha256 <sha256> --out <path> --json",
        "release_candidate_windows_transfer_bundle": "ao2 plugin release-candidate-windows-transfer-bundle --ao2-source-archive <path> --ao2-source-archive-sha256 <sha256> --recovery-dir <dir> --recovery <path> --recovery-sha256 <sha256> --recovery-verification <path> --recovery-verification-sha256 <sha256> --out-dir <dir> --json",
        "release_candidate_observer_bundle": "ao2 plugin release-candidate-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "release_candidate_observer_bundle_verify": "ao2 plugin release-candidate-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "release_candidate_control_plane_fixture_handoff": "ao2 plugin release-candidate-control-plane-fixture-handoff --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "release_candidate_control_plane_fixture_handoff_verify": "ao2 plugin release-candidate-control-plane-fixture-handoff-verify --handoff <path> --handoff-sha256 <sha256> --out <path> --json",
        "final_install_transcript": "ao2 plugin final-install-transcript --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "final_install_transcript_observer_bundle": "ao2 plugin final-install-transcript-observer-bundle --macos-codex-transcript <path> --macos-codex-sha256 <sha256> --macos-claude-transcript <path> --macos-claude-sha256 <sha256> --ubuntu-codex-transcript <path> --ubuntu-codex-sha256 <sha256> --ubuntu-claude-transcript <path> --ubuntu-claude-sha256 <sha256> --windows-codex-transcript <path> --windows-codex-sha256 <sha256> --windows-claude-transcript <path> --windows-claude-sha256 <sha256> --out-dir <dir> --json",
        "closer_decision": "ao2 factory closer-decision --rubric <path> --rubric-sha256 <sha256> --evidence <path> --evidence-sha256 <sha256> --skill-contract-manifest <path> --skill-contract-manifest-sha256 <sha256> --signing-key <path> --signer-id <id> --out <path> --json",
        "closer_decision_verify": "ao2 factory closer-decision-verify --decision <path> --decision-sha256 <sha256> --json",
        "shipment_readiness": "ao2 plugin shipment-readiness --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-candidate-observer-bundle <path> --release-candidate-observer-bundle-sha256 <sha256> --release-candidate-observer-archive <path> --release-candidate-observer-archive-sha256 <sha256> --final-install-transcript-observer-bundle <path> --final-install-transcript-observer-bundle-sha256 <sha256> --final-install-transcript-observer-archive <path> --final-install-transcript-observer-archive-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "clean_package_operator_index": "ao2 plugin clean-package-operator-index --macos-rehearsal <path> --macos-sha256 <sha256> --ubuntu-rehearsal <path> --ubuntu-sha256 <sha256> --windows-rehearsal <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "packaged_replacement_observer_bundle": "ao2 plugin packaged-replacement-observer-bundle --macos-proof <path> --macos-sha256 <sha256> --ubuntu-proof <path> --ubuntu-sha256 <sha256> --windows-proof <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "packaged_replacement_observer_bundle_verify": "ao2 plugin packaged-replacement-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "adapter_observer_bundle": "ao2 plugin adapter-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "adapter_install_smoke_verify": "ao2 plugin adapter-install-smoke-verify --smoke <path> --smoke-sha256 <sha256> --json",
        "adapter_install_smoke_observer_bundle": "ao2 plugin adapter-install-smoke-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "wrapper_harness": "ao2 plugin wrapper-harness --readiness <path> --readiness-sha256 <sha256> --args-file <path> --args-sha256 <sha256> --run-kind <app-run|project-run> --out-dir <dir> --json",
        "wrapper_harness_verify": "ao2 plugin wrapper-harness-verify --evidence-dir <dir> --summary-sha256 <sha256> --json"
    });
    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "factory_v3_role": "parity_auditor",
        "control_plane_role": "read_only_observer",
        "mutates_ao_artifacts": false,
        "control_plane_approves_release": false
    });
    let provider_auth = serde_json::json!({
        "local_oauth_cli_only": true,
        "provider_api_key_auth_allowed": false,
        "provider_api_key_env_required": false
    });

    let mut adapter_files = serde_json::Map::new();
    for target in ["codex", "claude"] {
        let target_dir = options.out_dir.join(target);
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("create {}", target_dir.display()))?;
        let adapter_path = target_dir.join("ao2-plugin-adapter.json");
        let adapter = serde_json::json!({
            "schema_version": "ao2.plugin-adapter.v1",
            "status": "ready_for_local_oauth_wrapper_integration",
            "target": target,
            "provider_auth": provider_auth,
            "inputs": {
                "package_summary_path": options.package_summary.display().to_string(),
                "package_summary_sha256": actual_package_summary_sha256,
                "package_archive_path": options.package_archive.display().to_string(),
                "package_archive_sha256": actual_package_archive_sha256,
                "k37_bundle_path": options.k37_bundle.display().to_string(),
                "k37_bundle_sha256": actual_k37_bundle_sha256,
                "k37_archive_path": options.k37_archive.display().to_string(),
                "k37_archive_sha256": actual_k37_archive_sha256
            },
            "commands": commands,
            "digest_gates": {
                "package_summary_sha256_verified": true,
                "package_archive_sha256_verified": true,
                "k37_bundle_sha256_verified": true,
                "k37_archive_sha256_verified": true,
                "wrapper_inputs_must_be_sha256_pinned": true
            },
            "side_effects": {
                "provider_execution_started": false,
                "queue_mutated": false,
                "memory_written": false,
                "ao_artifacts_mutated": false,
                "control_plane_mutated": false,
                "release_approved": false
            },
            "trust_boundary": trust_boundary,
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            },
            "factory_v3_role": "parity_auditor",
            "token_safe_output": {
                "redaction_policy": "paths_status_and_digests_only",
                "bearer_tokens_serialized": false,
                "cookies_serialized": false,
                "private_keys_serialized": false
            }
        });
        atomic_write_text(&adapter_path, &serde_json::to_string_pretty(&adapter)?)?;
        factory_app_run_bundle_reject_secret_markers(
            &adapter_path,
            &format!("{target} ao2-plugin-adapter.json"),
        )?;
        adapter_files.insert(
            target.to_string(),
            serde_json::json!({
                "path": adapter_path.display().to_string(),
                "sha256": sha256_file(&adapter_path)?,
                "schema_version": "ao2.plugin-adapter.v1",
                "status": "ready_for_local_oauth_wrapper_integration"
            }),
        );
    }

    let summary_path = options.out_dir.join("plugin-adapter-scaffold.json");
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-adapter-scaffold.v1",
        "status": "ready_for_local_oauth_wrapper_integration",
        "summary_path": summary_path.display().to_string(),
        "targets": ["codex", "claude"],
        "package": {
            "summary_path": options.package_summary.display().to_string(),
            "summary_sha256": actual_package_summary_sha256,
            "archive_path": options.package_archive.display().to_string(),
            "archive_sha256": actual_package_archive_sha256,
            "schema_version": json_string(&package_summary, "schema_version")
        },
        "k37_observer_bundle": {
            "summary_path": options.k37_bundle.display().to_string(),
            "summary_sha256": actual_k37_bundle_sha256,
            "archive_path": options.k37_archive.display().to_string(),
            "archive_sha256": actual_k37_archive_sha256,
            "schema_version": json_string(&k37_bundle, "schema_version")
        },
        "adapter_files": serde_json::Value::Object(adapter_files),
        "digest_gates": {
            "package_summary_sha256_verified": true,
            "package_archive_sha256_verified": true,
            "k37_bundle_sha256_verified": true,
            "k37_archive_sha256_verified": true,
            "wrapper_inputs_must_be_sha256_pinned": true
        },
        "provider_auth": provider_auth,
        "trust_boundary": trust_boundary,
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "side_effects": {
            "provider_execution_started": false,
            "queue_mutated": false,
            "memory_written": false,
            "ao_artifacts_mutated": false,
            "control_plane_mutated": false,
            "release_approved": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    let summary_body = serde_json::to_string_pretty(&summary)?;
    atomic_write_text(&summary_path, &summary_body)?;
    factory_app_run_bundle_reject_secret_markers(&summary_path, "plugin-adapter-scaffold.json")?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_local_oauth_wrapper_integration");
        println!("schema_version=ao2.plugin-adapter-scaffold.v1");
        println!("summary={}", summary_path.display());
    }
    Ok(())
}

pub(super) fn plugin_adapter_scaffold_verify(
    options: plugin_cli::PluginAdapterScaffoldVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "plugin adapter scaffold summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_adapter_scaffold_summary(&summary)?;
    factory_app_run_bundle_reject_secret_markers(&options.summary, "plugin-adapter-scaffold.json")?;

    let adapter_files = summary
        .get("adapter_files")
        .and_then(serde_json::Value::as_object)
        .context("plugin adapter scaffold missing adapter_files")?;
    for target in ["codex", "claude"] {
        let entry = adapter_files
            .get(target)
            .with_context(|| format!("plugin adapter scaffold missing {target} adapter file"))?;
        let path = PathBuf::from(json_string(entry, "path"));
        if path.as_os_str().is_empty() {
            anyhow::bail!("plugin adapter scaffold {target} adapter path is required");
        }
        let expected_sha256 = json_string(entry, "sha256");
        if !is_sha256_hex(&expected_sha256) {
            anyhow::bail!("plugin adapter scaffold {target} adapter sha256 must be a digest");
        }
        let actual_sha256 = sha256_file(&path)?;
        if expected_sha256 != actual_sha256 {
            anyhow::bail!(
                "plugin adapter scaffold {target} adapter sha256 mismatch: expected {expected_sha256}, actual {actual_sha256}"
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &path,
            &format!("{target} ao2-plugin-adapter.json"),
        )?;
        let adapter: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;
        validate_plugin_adapter_file(&adapter, target, &summary)?;
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-adapter-scaffold-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "targets": ["codex", "claude"],
        "adapter_files_verified": true,
        "digest_gates_verified": true,
        "provider_auth": summary["provider_auth"].clone(),
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-adapter-scaffold-verification.v1");
        println!("summary_sha256={actual_summary_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_adapter_install_smoke(
    options: plugin_cli::PluginAdapterInstallSmokeOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "plugin adapter scaffold summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_adapter_scaffold_summary(&summary)?;
    factory_app_run_bundle_reject_secret_markers(&options.summary, "plugin-adapter-scaffold.json")?;

    let required_commands = [
        "readiness",
        "package_verify",
        "distribution_observer_bundle",
        "consumer_lifecycle_observer_bundle",
        "consumer_lifecycle_observer_bundle_verify",
        "control_plane_fixture_handoff",
        "control_plane_fixture_handoff_verify",
        "release_candidate",
        "release_candidate_verify",
        "release_candidate_windows_recovery",
        "release_candidate_windows_recovery_verify",
        "release_candidate_windows_transfer_bundle",
        "release_candidate_observer_bundle",
        "release_candidate_observer_bundle_verify",
        "release_candidate_control_plane_fixture_handoff",
        "release_candidate_control_plane_fixture_handoff_verify",
        "final_install_transcript",
        "final_install_transcript_observer_bundle",
        "closer_decision",
        "closer_decision_verify",
        "shipment_readiness",
        "adapter_install_smoke_verify",
        "adapter_install_smoke_observer_bundle",
        "wrapper_harness",
        "wrapper_harness_verify",
    ];
    let adapter_files = summary
        .get("adapter_files")
        .and_then(serde_json::Value::as_object)
        .context("plugin adapter scaffold missing adapter_files")?;
    let mut target_results = serde_json::Map::new();
    for target in ["codex", "claude"] {
        let entry = adapter_files
            .get(target)
            .with_context(|| format!("plugin adapter scaffold missing {target} adapter file"))?;
        let path = PathBuf::from(json_string(entry, "path"));
        if path.as_os_str().is_empty() {
            anyhow::bail!("plugin adapter scaffold {target} adapter path is required");
        }
        let expected_sha256 = json_string(entry, "sha256");
        if !is_sha256_hex(&expected_sha256) {
            anyhow::bail!("plugin adapter scaffold {target} adapter sha256 must be a digest");
        }
        let actual_sha256 = sha256_file(&path)?;
        if expected_sha256 != actual_sha256 {
            anyhow::bail!(
                "plugin adapter scaffold {target} adapter sha256 mismatch: expected {expected_sha256}, actual {actual_sha256}"
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &path,
            &format!("{target} ao2-plugin-adapter.json"),
        )?;
        let adapter: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;
        validate_plugin_adapter_file(&adapter, target, &summary)?;
        let commands = adapter
            .get("commands")
            .with_context(|| format!("plugin adapter {target} missing commands"))?;
        for command in &required_commands {
            let command_text = json_string(commands, command);
            if command_text.is_empty() {
                anyhow::bail!("plugin adapter {target} missing command {command}");
            }
            if !command_text.contains("ao2 plugin") && !command_text.contains("ao2 factory") {
                anyhow::bail!(
                    "plugin adapter {target} command {command} must call ao2 plugin or ao2 factory"
                );
            }
        }

        target_results.insert(
            target.to_string(),
            serde_json::json!({
                "status": "passed",
                "adapter_path": path.display().to_string(),
                "adapter_sha256": actual_sha256,
                "adapter_schema_version": json_string(&adapter, "schema_version"),
                "commands_verified": required_commands
            }),
        );
    }

    let smoke = serde_json::json!({
        "schema_version": "ao2.plugin-adapter-install-smoke.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "targets": ["codex", "claude"],
        "adapter_files_verified": true,
        "digest_gates_verified": true,
        "command_surface_verified": true,
        "target_results": serde_json::Value::Object(target_results),
        "provider_auth": summary["provider_auth"].clone(),
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    validate_plugin_provider_auth(
        smoke
            .get("provider_auth")
            .context("plugin adapter install smoke missing provider_auth")?,
        "plugin adapter install smoke",
    )?;
    validate_plugin_observer_trust_boundary(
        smoke
            .get("trust_boundary")
            .context("plugin adapter install smoke missing trust_boundary")?,
        "plugin adapter install smoke",
    )?;
    validate_plugin_control_plane_observation(
        smoke
            .get("control_plane_observation")
            .context("plugin adapter install smoke missing control_plane_observation")?,
        "plugin adapter install smoke",
    )?;
    validate_plugin_side_effects_false(
        smoke
            .get("side_effects")
            .context("plugin adapter install smoke missing side_effects")?,
        "plugin adapter install smoke",
    )?;

    let body = serde_json::to_string_pretty(&smoke)?;
    let mut response = smoke;
    if let Some(out) = options.out {
        atomic_write_text(&out, &body)?;
        factory_app_run_bundle_reject_secret_markers(&out, "plugin-adapter-install-smoke.json")?;
        response["artifact_path"] = serde_json::json!(out.display().to_string());
        response["artifact_sha256"] = serde_json::json!(sha256_file(&out)?);
    }

    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-adapter-install-smoke.v1");
        println!("summary_sha256={actual_summary_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_adapter_install_smoke_verify(
    options: plugin_cli::PluginAdapterInstallSmokeVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_smoke_sha256 = options.smoke_sha256.trim();
    let actual_smoke_sha256 = sha256_file(&options.smoke)?;
    if supplied_smoke_sha256 != actual_smoke_sha256 {
        anyhow::bail!(
            "plugin adapter install smoke sha256 mismatch for {}: expected {}, actual {}",
            options.smoke.display(),
            supplied_smoke_sha256,
            actual_smoke_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.smoke,
        "plugin-adapter-install-smoke.json",
    )?;
    let smoke: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.smoke)
            .with_context(|| format!("read {}", options.smoke.display()))?,
    )
    .with_context(|| format!("parse {}", options.smoke.display()))?;
    validate_plugin_adapter_install_smoke_contract(&smoke)?;

    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-adapter-install-smoke-verification.v1",
        "status": "passed",
        "smoke_path": options.smoke.display().to_string(),
        "smoke_sha256": actual_smoke_sha256,
        "adapter_install_smoke_schema_version": json_string(&smoke, "schema_version"),
        "targets": smoke.get("targets").cloned().unwrap_or_else(|| serde_json::json!([])),
        "adapter_files_verified": json_bool(&smoke, "adapter_files_verified"),
        "digest_gates_verified": json_bool(&smoke, "digest_gates_verified"),
        "command_surface_verified": json_bool(&smoke, "command_surface_verified"),
        "provider_auth": smoke["provider_auth"].clone(),
        "trust_boundary": smoke["trust_boundary"].clone(),
        "control_plane_observation": smoke["control_plane_observation"].clone(),
        "side_effects": smoke["side_effects"].clone(),
        "token_safe_output_verified": json_bool(&smoke, "token_safe_output_verified"),
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-adapter-install-smoke-verification.v1");
        println!("smoke_sha256={actual_smoke_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_adapter_install_smoke_observer_bundle(
    options: plugin_cli::PluginAdapterInstallSmokeObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = [
        (
            "macos",
            options.macos_verification,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_verification,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_verification,
            options.windows_sha256.trim().to_string(),
        ),
    ];
    let mut platform_verifications = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} adapter install-smoke verification sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} plugin-adapter-install-smoke-verification.json"),
        )?;
        let verification_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let verification: serde_json::Value = serde_json::from_str(&verification_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_plugin_adapter_install_smoke_verification(&verification, platform)?;

        let bundled_path = platforms_root
            .join(platform)
            .join("plugin-adapter-install-smoke-verification.json");
        atomic_write_text(&bundled_path, &verification_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled plugin-adapter-install-smoke-verification.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} adapter install-smoke verification changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_verifications.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&verification, "schema_version"),
                "status": json_string(&verification, "status"),
                "smoke_path": json_string(&verification, "smoke_path"),
                "smoke_sha256": json_string(&verification, "smoke_sha256"),
                "adapter_install_smoke_schema_version": json_string(&verification, "adapter_install_smoke_schema_version"),
                "targets": verification.get("targets").cloned().unwrap_or_else(|| serde_json::json!([])),
                "provider_auth": verification.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": verification.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": verification.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": verification.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
                "factory_v3_role": json_string(&verification, "factory_v3_role")
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-plugin-adapter-install-smoke-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-adapter-install-smoke-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-plugin-adapter-install-smoke-observer-bundle.json");
    let platform_verifications_value = serde_json::Value::Object(platform_verifications);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": [
            "ao2.plugin-adapter-install-smoke.v1",
            "ao2.plugin-adapter-install-smoke-verification.v1"
        ],
        "platform_verifications": platform_verifications_value,
        "platform_verifications_sha256": canonical_json_sha256(&platform_verifications_value),
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
        "k37-plugin-adapter-install-smoke-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_adapter_observer_bundle(
    options: plugin_cli::PluginAdapterObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    let platforms_root = bundle_root.join("platforms");
    fs::create_dir_all(&platforms_root)
        .with_context(|| format!("create {}", platforms_root.display()))?;

    let inputs = [
        (
            "macos",
            options.macos_verification,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_verification,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_verification,
            options.windows_sha256.trim().to_string(),
        ),
    ];
    let mut platform_verifications = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} adapter verification sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} plugin-adapter-scaffold-verification.json"),
        )?;
        let verification_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let verification: serde_json::Value = serde_json::from_str(&verification_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_plugin_adapter_scaffold_verification(&verification, platform)?;

        let bundled_path = platforms_root
            .join(platform)
            .join("plugin-adapter-scaffold-verification.json");
        atomic_write_text(&bundled_path, &verification_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled plugin-adapter-scaffold-verification.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} adapter verification changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_verifications.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&verification, "schema_version"),
                "status": json_string(&verification, "status"),
                "summary_path": json_string(&verification, "summary_path"),
                "summary_sha256": json_string(&verification, "summary_sha256"),
                "targets": verification.get("targets").cloned().unwrap_or_else(|| serde_json::json!([])),
                "provider_auth": verification.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": verification.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": verification.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": verification.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
                "factory_v3_role": json_string(&verification, "factory_v3_role")
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-plugin-adapter-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-adapter-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-plugin-adapter-observer-bundle.json");
    let platform_verifications_value = serde_json::Value::Object(platform_verifications);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-plugin-adapter-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": [
            "ao2.plugin-adapter-scaffold.v1",
            "ao2.plugin-adapter-scaffold-verification.v1"
        ],
        "platform_verifications": platform_verifications_value,
        "platform_verifications_sha256": canonical_json_sha256(&platform_verifications_value),
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
        "k37-plugin-adapter-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-plugin-adapter-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}
