use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::plugin_cli;
use super::plugin_contract::{
    validate_plugin_control_plane_observation, validate_plugin_side_effects_false,
};
use super::plugin_distribution::{
    plugin_package_archive_json, read_plugin_package_archive_files, sha256_archive_file,
    validate_plugin_observer_trust_boundary, validate_plugin_provider_auth,
};
use super::{
    atomic_write_text, canonical_json_sha256, copy_dir_recursive, create_tar_gz,
    factory_app_run_bundle_reject_secret_markers, fail_if_provider_api_key_env_present,
    is_git_sha_prefix, is_sha256_hex, json_bool, json_string, json_u64,
    resolve_cli_artifact_reference, sha256_file,
};

pub(super) fn plugin_release_candidate(
    options: plugin_cli::PluginReleaseCandidateOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    if !is_git_sha_prefix(&options.control_plane_readback_commit) {
        anyhow::bail!(
            "control-plane readback commit must be a 7-40 character lowercase hex git sha"
        );
    }

    let (package_summary_sha256, package_summary) =
        validate_plugin_release_candidate_json_artifact(
            &options.package_summary,
            options.package_summary_sha256.trim(),
            "package summary",
            "ao2.plugin-package.v1",
            &["packaged"],
        )?;
    let package_archive_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.package_archive,
        options.package_archive_sha256.trim(),
        "package archive",
    )?;
    let (distribution_rehearsal_sha256, distribution_rehearsal) =
        validate_plugin_release_candidate_json_artifact(
            &options.distribution_rehearsal,
            options.distribution_rehearsal_sha256.trim(),
            "distribution rehearsal",
            "ao2.plugin-distribution-rehearsal.v1",
            &["passed"],
        )?;
    let (adapter_observer_bundle_sha256, adapter_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.adapter_observer_bundle,
            options.adapter_observer_bundle_sha256.trim(),
            "adapter observer bundle",
            "ao2.k37-plugin-adapter-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let adapter_observer_archive_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.adapter_observer_archive,
        options.adapter_observer_archive_sha256.trim(),
        "adapter observer archive",
    )?;
    let (adapter_install_smoke_observer_bundle_sha256, adapter_install_smoke_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.adapter_install_smoke_observer_bundle,
            options.adapter_install_smoke_observer_bundle_sha256.trim(),
            "adapter install-smoke observer bundle",
            "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let adapter_install_smoke_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.adapter_install_smoke_observer_archive,
            options.adapter_install_smoke_observer_archive_sha256.trim(),
            "adapter install-smoke observer archive",
        )?;
    let (consumer_lifecycle_observer_bundle_sha256, consumer_lifecycle_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.consumer_lifecycle_observer_bundle,
            options.consumer_lifecycle_observer_bundle_sha256.trim(),
            "consumer lifecycle observer bundle",
            "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let consumer_lifecycle_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.consumer_lifecycle_observer_archive,
            options.consumer_lifecycle_observer_archive_sha256.trim(),
            "consumer lifecycle observer archive",
        )?;
    let (
        release_gate_with_replacement_observer_bundle_sha256,
        release_gate_with_replacement_observer_bundle,
    ) = validate_plugin_release_candidate_json_artifact(
        &options.release_gate_with_replacement_observer_bundle,
        options
            .release_gate_with_replacement_observer_bundle_sha256
            .trim(),
        "release-gate-with-replacement observer bundle",
        "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
        &["ready_for_k37_observation"],
    )?;
    let release_gate_with_replacement_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.release_gate_with_replacement_observer_archive,
            options
                .release_gate_with_replacement_observer_archive_sha256
                .trim(),
            "release-gate-with-replacement observer archive",
        )?;
    if json_string(
        &release_gate_with_replacement_observer_bundle,
        "archive_sha256",
    ) != release_gate_with_replacement_observer_archive_sha256
    {
        anyhow::bail!(
            "release-gate-with-replacement observer bundle archive_sha256 does not match archive"
        );
    }
    let (control_plane_fixture_handoff_verification_sha256, control_plane_handoff_verification) =
        validate_plugin_release_candidate_json_artifact(
            &options.control_plane_fixture_handoff_verification,
            options
                .control_plane_fixture_handoff_verification_sha256
                .trim(),
            "control-plane fixture handoff verification",
            "ao2.control-plane-fixture-handoff-verification.v1",
            &["passed"],
        )?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let summary_path = options.out_dir.join("plugin-release-candidate.json");
    let side_effects = serde_json::json!({
        "would_execute_provider": false,
        "would_execute_queue": false,
        "would_write_memory": false,
        "would_mutate_control_plane": false,
        "would_mutate_ao_artifacts": false,
        "would_approve_release": false
    });
    let evidence = serde_json::json!({
        "package": {
            "summary_path": options.package_summary.display().to_string(),
            "summary_sha256": package_summary_sha256,
            "summary_schema_version": json_string(&package_summary, "schema_version"),
            "archive_path": options.package_archive.display().to_string(),
            "archive_sha256": package_archive_sha256
        },
        "distribution_rehearsal": {
            "path": options.distribution_rehearsal.display().to_string(),
            "sha256": distribution_rehearsal_sha256,
            "schema_version": json_string(&distribution_rehearsal, "schema_version")
        },
        "adapter_observer_bundle": {
            "summary_path": options.adapter_observer_bundle.display().to_string(),
            "summary_sha256": adapter_observer_bundle_sha256,
            "schema_version": json_string(&adapter_observer_bundle, "schema_version"),
            "archive_path": options.adapter_observer_archive.display().to_string(),
            "archive_sha256": adapter_observer_archive_sha256
        },
        "adapter_install_smoke_observer_bundle": {
            "summary_path": options.adapter_install_smoke_observer_bundle.display().to_string(),
            "summary_sha256": adapter_install_smoke_observer_bundle_sha256,
            "schema_version": json_string(&adapter_install_smoke_observer_bundle, "schema_version"),
            "archive_path": options.adapter_install_smoke_observer_archive.display().to_string(),
            "archive_sha256": adapter_install_smoke_observer_archive_sha256
        },
        "consumer_lifecycle_observer_bundle": {
            "summary_path": options.consumer_lifecycle_observer_bundle.display().to_string(),
            "summary_sha256": consumer_lifecycle_observer_bundle_sha256,
            "schema_version": json_string(&consumer_lifecycle_observer_bundle, "schema_version"),
            "archive_path": options.consumer_lifecycle_observer_archive.display().to_string(),
            "archive_sha256": consumer_lifecycle_observer_archive_sha256
        },
        "release_gate_with_replacement_observer_bundle": {
            "summary_path": options.release_gate_with_replacement_observer_bundle.display().to_string(),
            "summary_sha256": release_gate_with_replacement_observer_bundle_sha256,
            "schema_version": json_string(&release_gate_with_replacement_observer_bundle, "schema_version"),
            "archive_path": options.release_gate_with_replacement_observer_archive.display().to_string(),
            "archive_sha256": release_gate_with_replacement_observer_archive_sha256
        },
        "control_plane_fixture_handoff_verification": {
            "path": options.control_plane_fixture_handoff_verification.display().to_string(),
            "sha256": control_plane_fixture_handoff_verification_sha256,
            "schema_version": json_string(&control_plane_handoff_verification, "schema_version")
        }
    });
    let evidence_sha256 = canonical_json_sha256(&evidence);
    let control_plane_readback_commit = options.control_plane_readback_commit.clone();
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate.v1",
        "status": "ready_for_local_release_review",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "release_review_inputs": [
            "ao2.plugin-package.v1",
            "ao2.plugin-distribution-rehearsal.v1",
            "ao2.k37-plugin-adapter-observer-bundle.v1",
            "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
            "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
            "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
            "ao2.control-plane-fixture-handoff-verification.v1"
        ],
        "evidence": evidence,
        "evidence_sha256": evidence_sha256,
        "control_plane_readback": {
            "repo": "ao2-control-plane",
            "commit": control_plane_readback_commit,
            "role": "read_only_observer",
            "mutated_by_this_command": false,
            "approves_release": false
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
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    factory_app_run_bundle_reject_secret_markers(&summary_path, "plugin-release-candidate.json")?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_local_release_review");
        println!("schema_version=ao2.plugin-release-candidate.v1");
        println!("summary={}", summary_path.display());
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_verify(
    options: plugin_cli::PluginReleaseCandidateVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "plugin release-candidate summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "plugin-release-candidate.json",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_release_candidate_summary(&summary)?;

    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "source_schema_version": json_string(&summary, "schema_version"),
        "release_review_inputs": summary.get("release_review_inputs").cloned().unwrap_or_else(|| serde_json::json!([])),
        "evidence_sha256": json_string(&summary, "evidence_sha256"),
        "control_plane_readback": summary["control_plane_readback"].clone(),
        "provider_auth": summary["provider_auth"].clone(),
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": summary["side_effects"].clone(),
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    let response_body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-release-candidate-verification.v1");
        println!("summary={}", options.summary.display());
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_windows_recovery(
    options: plugin_cli::PluginReleaseCandidateWindowsRecoveryOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;
    if !is_git_sha_prefix(&options.control_plane_readback_commit) {
        anyhow::bail!("control-plane readback commit must be a git sha prefix");
    }

    let (package_summary_sha256, _) = validate_plugin_release_candidate_json_artifact(
        &options.package_summary,
        options.package_summary_sha256.trim(),
        "package summary",
        "ao2.plugin-package.v1",
        &["packaged"],
    )?;
    let package_archive_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.package_archive,
        options.package_archive_sha256.trim(),
        "package archive",
    )?;
    let (distribution_rehearsal_sha256, _) = validate_plugin_release_candidate_json_artifact(
        &options.distribution_rehearsal,
        options.distribution_rehearsal_sha256.trim(),
        "distribution rehearsal",
        "ao2.plugin-distribution-rehearsal.v1",
        &["passed"],
    )?;
    let (adapter_observer_bundle_sha256, _) = validate_plugin_release_candidate_json_artifact(
        &options.adapter_observer_bundle,
        options.adapter_observer_bundle_sha256.trim(),
        "adapter observer bundle",
        "ao2.k37-plugin-adapter-observer-bundle.v1",
        &["ready_for_k37_observation"],
    )?;
    let adapter_observer_archive_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.adapter_observer_archive,
        options.adapter_observer_archive_sha256.trim(),
        "adapter observer archive",
    )?;
    let (adapter_install_smoke_observer_bundle_sha256, _) =
        validate_plugin_release_candidate_json_artifact(
            &options.adapter_install_smoke_observer_bundle,
            options.adapter_install_smoke_observer_bundle_sha256.trim(),
            "adapter install-smoke observer bundle",
            "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let adapter_install_smoke_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.adapter_install_smoke_observer_archive,
            options.adapter_install_smoke_observer_archive_sha256.trim(),
            "adapter install-smoke observer archive",
        )?;
    let (consumer_lifecycle_observer_bundle_sha256, _) =
        validate_plugin_release_candidate_json_artifact(
            &options.consumer_lifecycle_observer_bundle,
            options.consumer_lifecycle_observer_bundle_sha256.trim(),
            "consumer lifecycle observer bundle",
            "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let consumer_lifecycle_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.consumer_lifecycle_observer_archive,
            options.consumer_lifecycle_observer_archive_sha256.trim(),
            "consumer lifecycle observer archive",
        )?;
    let (
        release_gate_with_replacement_observer_bundle_sha256,
        release_gate_with_replacement_observer_bundle,
    ) = validate_plugin_release_candidate_json_artifact(
        &options.release_gate_with_replacement_observer_bundle,
        options
            .release_gate_with_replacement_observer_bundle_sha256
            .trim(),
        "release-gate-with-replacement observer bundle",
        "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
        &["ready_for_k37_observation"],
    )?;
    let release_gate_with_replacement_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.release_gate_with_replacement_observer_archive,
            options
                .release_gate_with_replacement_observer_archive_sha256
                .trim(),
            "release-gate-with-replacement observer archive",
        )?;
    if json_string(
        &release_gate_with_replacement_observer_bundle,
        "archive_sha256",
    ) != release_gate_with_replacement_observer_archive_sha256
    {
        anyhow::bail!(
            "release-gate-with-replacement observer bundle archive_sha256 does not match archive"
        );
    }
    let (control_plane_fixture_handoff_verification_sha256, _) =
        validate_plugin_release_candidate_json_artifact(
            &options.control_plane_fixture_handoff_verification,
            options
                .control_plane_fixture_handoff_verification_sha256
                .trim(),
            "control-plane fixture handoff verification",
            "ao2.control-plane-fixture-handoff-verification.v1",
            &["passed"],
        )?;

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
                "{label} digest changed while preparing recovery input: expected {expected_sha256}, copied {copied_sha256}"
            );
        }
        factory_app_run_bundle_reject_secret_markers(&target, label)?;
        Ok::<PathBuf, anyhow::Error>(target)
    };
    copy_input(
        &options.package_summary,
        "ao2-plugin-package.json",
        &package_summary_sha256,
        "release-candidate recovery package summary",
    )?;
    copy_input(
        &options.package_archive,
        "ao2-plugin-package.tar.gz",
        &package_archive_sha256,
        "release-candidate recovery package archive",
    )?;
    copy_input(
        &options.distribution_rehearsal,
        "plugin-distribution-rehearsal.json",
        &distribution_rehearsal_sha256,
        "release-candidate recovery distribution rehearsal",
    )?;
    copy_input(
        &options.adapter_observer_bundle,
        "k37-plugin-adapter-observer-bundle.json",
        &adapter_observer_bundle_sha256,
        "release-candidate recovery adapter observer bundle",
    )?;
    copy_input(
        &options.adapter_observer_archive,
        "k37-plugin-adapter-observer-bundle.tar.gz",
        &adapter_observer_archive_sha256,
        "release-candidate recovery adapter observer archive",
    )?;
    copy_input(
        &options.adapter_install_smoke_observer_bundle,
        "k37-plugin-adapter-install-smoke-observer-bundle.json",
        &adapter_install_smoke_observer_bundle_sha256,
        "release-candidate recovery adapter install-smoke observer bundle",
    )?;
    copy_input(
        &options.adapter_install_smoke_observer_archive,
        "k37-plugin-adapter-install-smoke-observer-bundle.tar.gz",
        &adapter_install_smoke_observer_archive_sha256,
        "release-candidate recovery adapter install-smoke observer archive",
    )?;
    copy_input(
        &options.consumer_lifecycle_observer_bundle,
        "k37-plugin-consumer-lifecycle-observer-bundle.json",
        &consumer_lifecycle_observer_bundle_sha256,
        "release-candidate recovery consumer lifecycle observer bundle",
    )?;
    copy_input(
        &options.consumer_lifecycle_observer_archive,
        "k37-plugin-consumer-lifecycle-observer-bundle.tar.gz",
        &consumer_lifecycle_observer_archive_sha256,
        "release-candidate recovery consumer lifecycle observer archive",
    )?;
    copy_input(
        &options.release_gate_with_replacement_observer_bundle,
        "k37-release-gate-with-replacement-observer-bundle.json",
        &release_gate_with_replacement_observer_bundle_sha256,
        "release-candidate recovery release-gate-with-replacement observer bundle",
    )?;
    copy_input(
        &options.release_gate_with_replacement_observer_archive,
        "k37-release-gate-with-replacement-observer-bundle.tar.gz",
        &release_gate_with_replacement_observer_archive_sha256,
        "release-candidate recovery release-gate-with-replacement observer archive",
    )?;
    copy_input(
        &options.control_plane_fixture_handoff_verification,
        "control-plane-fixture-handoff-verification.json",
        &control_plane_fixture_handoff_verification_sha256,
        "release-candidate recovery control-plane fixture handoff verification",
    )?;

    let script_path = options.out_dir.join("run-release-candidate.ps1");
    let script = format!(
        r#"param(
    [string]$Ao2 = "ao2",
    [string]$OutDir = (Join-Path $PSScriptRoot "release-candidate")
)

$ErrorActionPreference = "Stop"
$InputRoot = Join-Path $PSScriptRoot "inputs"
& $Ao2 plugin release-candidate `
    --package-summary (Join-Path $InputRoot "ao2-plugin-package.json") `
    --package-summary-sha256 "{package_summary_sha256}" `
    --package-archive (Join-Path $InputRoot "ao2-plugin-package.tar.gz") `
    --package-archive-sha256 "{package_archive_sha256}" `
    --distribution-rehearsal (Join-Path $InputRoot "plugin-distribution-rehearsal.json") `
    --distribution-rehearsal-sha256 "{distribution_rehearsal_sha256}" `
    --adapter-observer-bundle (Join-Path $InputRoot "k37-plugin-adapter-observer-bundle.json") `
    --adapter-observer-bundle-sha256 "{adapter_observer_bundle_sha256}" `
    --adapter-observer-archive (Join-Path $InputRoot "k37-plugin-adapter-observer-bundle.tar.gz") `
    --adapter-observer-archive-sha256 "{adapter_observer_archive_sha256}" `
    --adapter-install-smoke-observer-bundle (Join-Path $InputRoot "k37-plugin-adapter-install-smoke-observer-bundle.json") `
    --adapter-install-smoke-observer-bundle-sha256 "{adapter_install_smoke_observer_bundle_sha256}" `
    --adapter-install-smoke-observer-archive (Join-Path $InputRoot "k37-plugin-adapter-install-smoke-observer-bundle.tar.gz") `
    --adapter-install-smoke-observer-archive-sha256 "{adapter_install_smoke_observer_archive_sha256}" `
    --consumer-lifecycle-observer-bundle (Join-Path $InputRoot "k37-plugin-consumer-lifecycle-observer-bundle.json") `
    --consumer-lifecycle-observer-bundle-sha256 "{consumer_lifecycle_observer_bundle_sha256}" `
    --consumer-lifecycle-observer-archive (Join-Path $InputRoot "k37-plugin-consumer-lifecycle-observer-bundle.tar.gz") `
    --consumer-lifecycle-observer-archive-sha256 "{consumer_lifecycle_observer_archive_sha256}" `
    --release-gate-with-replacement-observer-bundle (Join-Path $InputRoot "k37-release-gate-with-replacement-observer-bundle.json") `
    --release-gate-with-replacement-observer-bundle-sha256 "{release_gate_with_replacement_observer_bundle_sha256}" `
    --release-gate-with-replacement-observer-archive (Join-Path $InputRoot "k37-release-gate-with-replacement-observer-bundle.tar.gz") `
    --release-gate-with-replacement-observer-archive-sha256 "{release_gate_with_replacement_observer_archive_sha256}" `
    --control-plane-fixture-handoff-verification (Join-Path $InputRoot "control-plane-fixture-handoff-verification.json") `
    --control-plane-fixture-handoff-verification-sha256 "{control_plane_fixture_handoff_verification_sha256}" `
    --control-plane-readback-commit "{control_plane_readback_commit}" `
    --out-dir $OutDir `
    --json
if ($LASTEXITCODE -ne 0) {{
    exit $LASTEXITCODE
}}

$Summary = Join-Path $OutDir "plugin-release-candidate.json"
$SummarySha256 = (Get-FileHash -Algorithm SHA256 $Summary).Hash.ToLowerInvariant()
& $Ao2 plugin release-candidate-verify `
    --summary $Summary `
    --summary-sha256 $SummarySha256 `
    --json
if ($LASTEXITCODE -ne 0) {{
    exit $LASTEXITCODE
}}
"#,
        package_summary_sha256 = package_summary_sha256,
        package_archive_sha256 = package_archive_sha256,
        distribution_rehearsal_sha256 = distribution_rehearsal_sha256,
        adapter_observer_bundle_sha256 = adapter_observer_bundle_sha256,
        adapter_observer_archive_sha256 = adapter_observer_archive_sha256,
        adapter_install_smoke_observer_bundle_sha256 = adapter_install_smoke_observer_bundle_sha256,
        adapter_install_smoke_observer_archive_sha256 =
            adapter_install_smoke_observer_archive_sha256,
        consumer_lifecycle_observer_bundle_sha256 = consumer_lifecycle_observer_bundle_sha256,
        consumer_lifecycle_observer_archive_sha256 = consumer_lifecycle_observer_archive_sha256,
        release_gate_with_replacement_observer_bundle_sha256 =
            release_gate_with_replacement_observer_bundle_sha256,
        release_gate_with_replacement_observer_archive_sha256 =
            release_gate_with_replacement_observer_archive_sha256,
        control_plane_fixture_handoff_verification_sha256 =
            control_plane_fixture_handoff_verification_sha256,
        control_plane_readback_commit = options.control_plane_readback_commit.as_str()
    );
    atomic_write_text(&script_path, &script)?;
    factory_app_run_bundle_reject_secret_markers(&script_path, "run-release-candidate.ps1")?;
    let script_sha256 = sha256_file(&script_path)?;

    let manifest_path = options
        .out_dir
        .join("windows-release-candidate-recovery.json");
    let side_effects = serde_json::json!({
        "provider_execution_started": false,
        "queue_mutated": false,
        "memory_written": false,
        "control_plane_mutated": false,
        "ao_artifacts_mutated": false,
        "release_approved": false
    });
    let manifest = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-windows-recovery.v1",
        "status": "ready_for_windows_execution",
        "platform": "windows",
        "manifest_path": manifest_path.display().to_string(),
        "script_path": script_path.display().to_string(),
        "script_sha256": script_sha256,
        "execution": {
            "runner": "powershell",
            "single_session_command": "powershell -ExecutionPolicy Bypass -File .\\run-release-candidate.ps1",
            "ao2_argument": "-Ao2 <path-to-ao2.exe-or-ao2>",
            "output_argument": "-OutDir <windows-output-dir>",
            "produces": "ao2.plugin-release-candidate-verification.v1"
        },
        "release_review_inputs": {
            "package": {
                "summary_path": "inputs/ao2-plugin-package.json",
                "summary_sha256": package_summary_sha256,
                "archive_path": "inputs/ao2-plugin-package.tar.gz",
                "archive_sha256": package_archive_sha256
            },
            "distribution_rehearsal": {
                "path": "inputs/plugin-distribution-rehearsal.json",
                "sha256": distribution_rehearsal_sha256
            },
            "adapter_observer_bundle": {
                "summary_path": "inputs/k37-plugin-adapter-observer-bundle.json",
                "summary_sha256": adapter_observer_bundle_sha256,
                "archive_path": "inputs/k37-plugin-adapter-observer-bundle.tar.gz",
                "archive_sha256": adapter_observer_archive_sha256
            },
            "adapter_install_smoke_observer_bundle": {
                "summary_path": "inputs/k37-plugin-adapter-install-smoke-observer-bundle.json",
                "summary_sha256": adapter_install_smoke_observer_bundle_sha256,
                "archive_path": "inputs/k37-plugin-adapter-install-smoke-observer-bundle.tar.gz",
                "archive_sha256": adapter_install_smoke_observer_archive_sha256
            },
            "consumer_lifecycle_observer_bundle": {
                "summary_path": "inputs/k37-plugin-consumer-lifecycle-observer-bundle.json",
                "summary_sha256": consumer_lifecycle_observer_bundle_sha256,
                "archive_path": "inputs/k37-plugin-consumer-lifecycle-observer-bundle.tar.gz",
                "archive_sha256": consumer_lifecycle_observer_archive_sha256
            },
            "release_gate_with_replacement_observer_bundle": {
                "summary_path": "inputs/k37-release-gate-with-replacement-observer-bundle.json",
                "summary_sha256": release_gate_with_replacement_observer_bundle_sha256,
                "archive_path": "inputs/k37-release-gate-with-replacement-observer-bundle.tar.gz",
                "archive_sha256": release_gate_with_replacement_observer_archive_sha256
            },
            "control_plane_fixture_handoff_verification": {
                "path": "inputs/control-plane-fixture-handoff-verification.json",
                "sha256": control_plane_fixture_handoff_verification_sha256
            }
        },
        "control_plane_readback": {
            "repo": "ao2-control-plane",
            "commit": options.control_plane_readback_commit,
            "role": "read_only_observer",
            "mutated_by_this_command": false,
            "approves_release": false
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
        "windows-release-candidate-recovery.json",
    )?;
    let manifest_sha256 = sha256_file(&manifest_path)?;

    let mut response = manifest;
    response["manifest_sha256"] = serde_json::json!(manifest_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_windows_execution");
        println!("schema_version=ao2.plugin-release-candidate-windows-recovery.v1");
        println!("manifest={}", manifest_path.display());
        println!("script={}", script_path.display());
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_windows_recovery_verify(
    options: plugin_cli::PluginReleaseCandidateWindowsRecoveryVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_recovery_sha256 = options.recovery_sha256.trim();
    let actual_recovery_sha256 = sha256_file(&options.recovery)?;
    if supplied_recovery_sha256 != actual_recovery_sha256 {
        anyhow::bail!(
            "release-candidate Windows recovery sha256 mismatch for {}: expected {}, actual {}",
            options.recovery.display(),
            supplied_recovery_sha256,
            actual_recovery_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(
        &options.recovery,
        "windows-release-candidate-recovery.json",
    )?;
    let recovery: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.recovery)
            .with_context(|| format!("read {}", options.recovery.display()))?,
    )
    .with_context(|| format!("parse {}", options.recovery.display()))?;
    let script_path =
        validate_plugin_release_candidate_windows_recovery_manifest(&options.recovery, &recovery)?;
    let script_sha256 = sha256_file(&script_path)?;

    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-windows-recovery-verification.v1",
        "status": "passed",
        "recovery_path": options.recovery.display().to_string(),
        "recovery_sha256": actual_recovery_sha256,
        "source_schema_version": json_string(&recovery, "schema_version"),
        "platform": json_string(&recovery, "platform"),
        "script_path": script_path.display().to_string(),
        "script_sha256": script_sha256,
        "execution": recovery["execution"].clone(),
        "release_review_inputs": recovery["release_review_inputs"].clone(),
        "control_plane_readback": recovery["control_plane_readback"].clone(),
        "provider_auth": recovery["provider_auth"].clone(),
        "trust_boundary": recovery["trust_boundary"].clone(),
        "control_plane_observation": recovery["control_plane_observation"].clone(),
        "side_effects": recovery["side_effects"].clone(),
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    atomic_write_text(&options.out, &body)?;
    factory_app_run_bundle_reject_secret_markers(
        &options.out,
        "release-candidate-windows-recovery-verification.json",
    )?;
    let verification_sha256 = sha256_file(&options.out)?;
    let mut response = verification;
    response["verification_sha256"] = serde_json::json!(verification_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-release-candidate-windows-recovery-verification.v1");
        println!("verification={}", options.out.display());
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_windows_transfer_bundle(
    options: plugin_cli::PluginReleaseCandidateWindowsTransferBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let ao2_source_archive_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.ao2_source_archive,
        options.ao2_source_archive_sha256.trim(),
        "AO2 source archive",
    )?;
    let recovery_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.recovery,
        options.recovery_sha256.trim(),
        "release-candidate Windows recovery",
    )?;
    let recovery: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.recovery)
            .with_context(|| format!("read {}", options.recovery.display()))?,
    )
    .with_context(|| format!("parse {}", options.recovery.display()))?;
    validate_plugin_release_candidate_windows_recovery_manifest(&options.recovery, &recovery)?;

    let (recovery_verification_sha256, recovery_verification) =
        validate_plugin_release_candidate_json_artifact(
            &options.recovery_verification,
            options.recovery_verification_sha256.trim(),
            "release-candidate Windows recovery verification",
            "ao2.plugin-release-candidate-windows-recovery-verification.v1",
            &["passed"],
        )?;
    if json_string(&recovery_verification, "recovery_sha256") != recovery_sha256 {
        anyhow::bail!(
            "release-candidate Windows recovery verification recovery_sha256 mismatch: expected {}, got {}",
            recovery_sha256,
            json_string(&recovery_verification, "recovery_sha256")
        );
    }
    if json_string(&recovery_verification, "source_schema_version")
        != "ao2.plugin-release-candidate-windows-recovery.v1"
        || json_string(&recovery_verification, "platform") != "windows"
    {
        anyhow::bail!("release-candidate Windows recovery verification source metadata is invalid");
    }

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let transfer_dir = options.out_dir.join("transfer");
    fs::create_dir_all(&transfer_dir)
        .with_context(|| format!("create {}", transfer_dir.display()))?;

    let source_archive_target = transfer_dir.join("ao2-source.tar.gz");
    fs::copy(&options.ao2_source_archive, &source_archive_target).with_context(|| {
        format!(
            "copy {} to {}",
            options.ao2_source_archive.display(),
            source_archive_target.display()
        )
    })?;
    let copied_source_sha256 = sha256_file(&source_archive_target)?;
    if copied_source_sha256 != ao2_source_archive_sha256 {
        anyhow::bail!(
            "AO2 source archive changed while preparing transfer: expected {}, copied {}",
            ao2_source_archive_sha256,
            copied_source_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(
        &source_archive_target,
        "transfer AO2 source archive",
    )?;

    let recovery_target_dir = transfer_dir.join("recovery");
    copy_dir_recursive(&options.recovery_dir, &recovery_target_dir)?;
    let copied_recovery_path = recovery_target_dir.join("windows-release-candidate-recovery.json");
    if !copied_recovery_path.is_file() {
        anyhow::bail!(
            "release-candidate Windows transfer recovery directory is missing windows-release-candidate-recovery.json"
        );
    }
    let copied_recovery_sha256 = sha256_file(&copied_recovery_path)?;
    if copied_recovery_sha256 != recovery_sha256 {
        anyhow::bail!(
            "release-candidate Windows recovery changed while preparing transfer: expected {}, copied {}",
            recovery_sha256,
            copied_recovery_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(
        &copied_recovery_path,
        "transfer windows-release-candidate-recovery.json",
    )?;
    let copied_runner_path = recovery_target_dir.join("run-release-candidate.ps1");
    if !copied_runner_path.is_file() {
        anyhow::bail!(
            "release-candidate Windows transfer recovery directory is missing run-release-candidate.ps1"
        );
    }
    factory_app_run_bundle_reject_secret_markers(
        &copied_runner_path,
        "transfer run-release-candidate.ps1",
    )?;

    let recovery_verification_target =
        transfer_dir.join("release-candidate-windows-recovery-verification.json");
    fs::copy(
        &options.recovery_verification,
        &recovery_verification_target,
    )
    .with_context(|| {
        format!(
            "copy {} to {}",
            options.recovery_verification.display(),
            recovery_verification_target.display()
        )
    })?;
    let copied_verification_sha256 = sha256_file(&recovery_verification_target)?;
    if copied_verification_sha256 != recovery_verification_sha256 {
        anyhow::bail!(
            "release-candidate Windows recovery verification changed while preparing transfer: expected {}, copied {}",
            recovery_verification_sha256,
            copied_verification_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(
        &recovery_verification_target,
        "transfer release-candidate-windows-recovery-verification.json",
    )?;

    let archive_path = options
        .out_dir
        .join("release-candidate-windows-transfer-bundle.tar.gz");
    create_tar_gz(&transfer_dir, &archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "release-candidate-windows-transfer-bundle.tar.gz",
    )?;
    let archive_sha256 = sha256_file(&archive_path)?;

    let summary_path = options
        .out_dir
        .join("release-candidate-windows-transfer-bundle.json");
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-windows-transfer-bundle.v1",
        "status": "ready_for_windows_transfer",
        "platform": "windows",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "transfer_root": transfer_dir.display().to_string(),
        "transfer_inputs": {
            "ao2_source_archive": {
                "source_path": options.ao2_source_archive.display().to_string(),
                "bundled_path": source_archive_target.display().to_string(),
                "sha256": ao2_source_archive_sha256
            },
            "recovery": {
                "source_dir": options.recovery_dir.display().to_string(),
                "source_path": options.recovery.display().to_string(),
                "bundled_dir": recovery_target_dir.display().to_string(),
                "bundled_path": copied_recovery_path.display().to_string(),
                "sha256": recovery_sha256,
                "schema_version": json_string(&recovery, "schema_version"),
                "status": json_string(&recovery, "status")
            },
            "recovery_verification": {
                "source_path": options.recovery_verification.display().to_string(),
                "bundled_path": recovery_verification_target.display().to_string(),
                "sha256": recovery_verification_sha256,
                "schema_version": json_string(&recovery_verification, "schema_version"),
                "status": json_string(&recovery_verification, "status")
            }
        },
        "execution": {
            "copy_archive_to_windows": "release-candidate-windows-transfer-bundle.tar.gz",
            "extract_on_windows": true,
            "runner": "powershell",
            "single_session_command": "powershell -ExecutionPolicy Bypass -File .\\run-release-candidate.ps1",
            "ao2_argument": "-Ao2 <path-to-ao2.exe-or-ao2>",
            "output_argument": "-OutDir <windows-output-dir>",
            "produces": "ao2.plugin-release-candidate-verification.v1"
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
            "provider_execution_started": false,
            "queue_mutated": false,
            "memory_written": false,
            "control_plane_mutated": false,
            "ao_artifacts_mutated": false,
            "release_approved": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    let summary_body = serde_json::to_string_pretty(&summary)?;
    atomic_write_text(&summary_path, &summary_body)?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "release-candidate-windows-transfer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_windows_transfer");
        println!("schema_version=ao2.plugin-release-candidate-windows-transfer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_observer_bundle(
    options: plugin_cli::PluginReleaseCandidateObserverBundleOptions,
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
    let mut platform_release_candidates = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} release-candidate verification sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} plugin-release-candidate-verification.json"),
        )?;
        let verification_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let verification: serde_json::Value = serde_json::from_str(&verification_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_plugin_release_candidate_verification_artifact(&verification, platform)?;

        let bundled_path = platforms_root
            .join(platform)
            .join("plugin-release-candidate-verification.json");
        atomic_write_text(&bundled_path, &verification_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled plugin-release-candidate-verification.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} release-candidate verification changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_release_candidates.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&verification, "schema_version"),
                "status": json_string(&verification, "status"),
                "source_schema_version": json_string(&verification, "source_schema_version"),
                "summary_sha256": json_string(&verification, "summary_sha256"),
                "evidence_sha256": json_string(&verification, "evidence_sha256"),
                "release_review_inputs": verification.get("release_review_inputs").cloned().unwrap_or_else(|| serde_json::json!([])),
                "control_plane_readback": verification.get("control_plane_readback").cloned().unwrap_or_else(|| serde_json::json!({})),
                "provider_auth": verification.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": verification.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": verification.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": verification.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
                "token_safe_output_verified": json_bool(&verification, "token_safe_output_verified"),
                "factory_v3_role": json_string(&verification, "factory_v3_role")
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-plugin-release-candidate-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-release-candidate-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-plugin-release-candidate-observer-bundle.json");
    let platform_release_candidates_value = serde_json::Value::Object(platform_release_candidates);
    let platform_release_candidates_sha256 =
        canonical_json_sha256(&platform_release_candidates_value);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-plugin-release-candidate-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": [
            "ao2.plugin-release-candidate.v1",
            "ao2.plugin-release-candidate-verification.v1"
        ],
        "platform_release_candidates": platform_release_candidates_value,
        "platform_release_candidates_sha256": platform_release_candidates_sha256,
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
        "k37-plugin-release-candidate-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-plugin-release-candidate-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_observer_bundle_verify(
    options: plugin_cli::PluginReleaseCandidateObserverBundleVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "release-candidate observer bundle summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "release-candidate observer bundle archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-plugin-release-candidate-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-plugin-release-candidate-observer-bundle.tar.gz",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_release_candidate_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_release_candidates = summary
        .get("platform_release_candidates")
        .and_then(serde_json::Value::as_object)
        .context("release-candidate observer bundle missing platform_release_candidates")?;
    for platform in ["macos", "ubuntu", "windows"] {
        let archive_path =
            format!("platforms/{platform}/plugin-release-candidate-verification.json");
        let verification =
            plugin_package_archive_json(&archive_files, &archive_path, "bundled verification")?;
        validate_plugin_release_candidate_verification_artifact(&verification, platform)?;
        let verification_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        let summary_verification = platform_release_candidates
            .get(platform)
            .with_context(|| format!("release-candidate observer bundle missing {platform}"))?;
        if verification_sha256 != json_string(summary_verification, "sha256") {
            anyhow::bail!(
                "{platform} release-candidate observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_verification, "sha256"),
                verification_sha256
            );
        }
        if json_string(summary_verification, "schema_version")
            != "ao2.plugin-release-candidate-verification.v1"
            || json_string(summary_verification, "status") != "passed"
            || json_string(summary_verification, "source_schema_version")
                != "ao2.plugin-release-candidate.v1"
        {
            anyhow::bail!(
                "{platform} release-candidate observer bundle summary metadata is invalid"
            );
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-plugin-release-candidate-observer-bundle-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "platform_release_candidates_sha256": json_string(&summary, "platform_release_candidates_sha256"),
        "archive_contents_verified": true,
        "platform_release_candidates_verified": true,
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
        println!("schema_version=ao2.k37-plugin-release-candidate-observer-bundle-verification.v1");
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_control_plane_fixture_handoff(
    options: plugin_cli::PluginReleaseCandidateControlPlaneFixtureHandoffOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "release-candidate control-plane fixture summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "release-candidate control-plane fixture archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-plugin-release-candidate-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-plugin-release-candidate-observer-bundle.tar.gz",
    )?;
    let summary_text = fs::read_to_string(&options.summary)
        .with_context(|| format!("read {}", options.summary.display()))?;
    let summary: serde_json::Value = serde_json::from_str(&summary_text)
        .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_release_candidate_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_release_candidates = summary
        .get("platform_release_candidates")
        .and_then(serde_json::Value::as_object)
        .context("release-candidate observer bundle missing platform_release_candidates")?;
    for platform in ["macos", "ubuntu", "windows"] {
        let archive_path =
            format!("platforms/{platform}/plugin-release-candidate-verification.json");
        let verification =
            plugin_package_archive_json(&archive_files, &archive_path, "bundled verification")?;
        validate_plugin_release_candidate_verification_artifact(&verification, platform)?;
        let verification_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        let summary_verification = platform_release_candidates
            .get(platform)
            .with_context(|| format!("release-candidate observer bundle missing {platform}"))?;
        if verification_sha256 != json_string(summary_verification, "sha256") {
            anyhow::bail!(
                "{platform} release-candidate observer bundle archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_verification, "sha256"),
                verification_sha256
            );
        }
    }

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let fixture_dir = options.out_dir.join("control-plane-fixture");
    fs::create_dir_all(&fixture_dir)
        .with_context(|| format!("create {}", fixture_dir.display()))?;
    let fixture_path = fixture_dir.join("release-candidate-observer-bundle.json");
    atomic_write_text(&fixture_path, &summary_text)?;
    factory_app_run_bundle_reject_secret_markers(
        &fixture_path,
        "release-candidate-observer-bundle.json",
    )?;
    let fixture_sha256 = sha256_file(&fixture_path)?;
    if fixture_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "release-candidate control-plane fixture digest mismatch: source {}, fixture {}",
            actual_summary_sha256,
            fixture_sha256
        );
    }

    let recommended_fixture_path =
        "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/release-candidate-observer-bundle.json";
    let recommended_test_name =
        "release_candidate_observer_bundle_is_read_only_three_platform_evidence";
    let handoff_path = options
        .out_dir
        .join("ao2-release-candidate-control-plane-fixture-handoff.json");
    let handoff = serde_json::json!({
        "schema_version": "ao2.release-candidate-control-plane-fixture-handoff.v1",
        "status": "ready_for_control_plane_readback",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "source_schema_version": "ao2.k37-plugin-release-candidate-observer-bundle.v1",
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
        "expected_schema_version": "ao2.k37-plugin-release-candidate-observer-bundle.v1",
        "expected_status": "ready_for_k37_observation",
        "expected_platforms": ["macos", "ubuntu", "windows"],
        "expected_platform_count": 3,
        "expected_observed_evidence_scope": [
            "ao2.plugin-release-candidate.v1",
            "ao2.plugin-release-candidate-verification.v1"
        ],
        "control_plane_readback_assertions": {
            "assert_platform_release_candidates": ["macos", "ubuntu", "windows"],
            "assert_release_review_inputs": [
                "ao2.plugin-package.v1",
                "ao2.plugin-distribution-rehearsal.v1",
                "ao2.k37-plugin-adapter-observer-bundle.v1",
                "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
                "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
                "ao2.control-plane-fixture-handoff-verification.v1"
            ],
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
        "ao2-release-candidate-control-plane-fixture-handoff.json",
    )?;
    let handoff_sha256 = sha256_file(&handoff_path)?;

    let mut response = handoff;
    response["handoff_sha256"] = serde_json::json!(handoff_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_control_plane_readback");
        println!("schema_version=ao2.release-candidate-control-plane-fixture-handoff.v1");
        println!("handoff={}", handoff_path.display());
        println!("fixture={}", fixture_path.display());
    }
    Ok(())
}

pub(super) fn plugin_release_candidate_control_plane_fixture_handoff_verify(
    options: plugin_cli::PluginReleaseCandidateControlPlaneFixtureHandoffVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_handoff_sha256 = options.handoff_sha256.trim();
    let actual_handoff_sha256 = sha256_file(&options.handoff)?;
    if supplied_handoff_sha256 != actual_handoff_sha256 {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff sha256 mismatch for {}: expected {}, actual {}",
            options.handoff.display(),
            supplied_handoff_sha256,
            actual_handoff_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.handoff,
        "ao2-release-candidate-control-plane-fixture-handoff.json",
    )?;
    let handoff_text = fs::read_to_string(&options.handoff)
        .with_context(|| format!("read {}", options.handoff.display()))?;
    let handoff: serde_json::Value = serde_json::from_str(&handoff_text)
        .with_context(|| format!("parse {}", options.handoff.display()))?;
    validate_plugin_release_candidate_control_plane_fixture_handoff(&handoff)?;

    let summary_path = PathBuf::from(json_string(&handoff, "summary_path"));
    let archive_path = PathBuf::from(json_string(&handoff, "archive_path"));
    let fixture_path = PathBuf::from(json_string(&handoff["fixture"], "path"));
    let actual_summary_sha256 = sha256_file(&summary_path)?;
    if actual_summary_sha256 != json_string(&handoff, "summary_sha256") {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff summary sha256 mismatch: expected {}, actual {}",
            json_string(&handoff, "summary_sha256"),
            actual_summary_sha256
        );
    }
    let actual_archive_sha256 = sha256_file(&archive_path)?;
    if actual_archive_sha256 != json_string(&handoff, "archive_sha256") {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff archive sha256 mismatch: expected {}, actual {}",
            json_string(&handoff, "archive_sha256"),
            actual_archive_sha256
        );
    }
    let fixture_sha256 = sha256_file(&fixture_path)?;
    if fixture_sha256 != json_string(&handoff["fixture"], "sha256") {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff fixture sha256 mismatch: expected {}, actual {}",
            json_string(&handoff["fixture"], "sha256"),
            fixture_sha256
        );
    }
    if fixture_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff fixture must match source summary digest: fixture {}, summary {}",
            fixture_sha256,
            actual_summary_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "k37-plugin-release-candidate-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-release-candidate-observer-bundle.tar.gz",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &fixture_path,
        "release-candidate-observer-bundle.json",
    )?;
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&summary_path)
            .with_context(|| format!("read {}", summary_path.display()))?,
    )
    .with_context(|| format!("parse {}", summary_path.display()))?;
    validate_plugin_release_candidate_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    if let Some(parent) = options.out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
    }
    let verification = serde_json::json!({
        "schema_version": "ao2.release-candidate-control-plane-fixture-handoff-verification.v1",
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
        "release-candidate-control-plane-fixture-handoff-verification.json",
    )?;
    let verification_sha256 = sha256_file(&options.out)?;

    let mut response = verification;
    response["verification_sha256"] = serde_json::json!(verification_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!(
            "schema_version=ao2.release-candidate-control-plane-fixture-handoff-verification.v1"
        );
        println!("verification={}", options.out.display());
    }
    Ok(())
}

pub(super) fn plugin_final_install_transcript(
    options: plugin_cli::PluginFinalInstallTranscriptOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "final install transcript summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "final install transcript archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    factory_app_run_bundle_reject_secret_markers(
        &options.summary,
        "k37-plugin-release-candidate-observer-bundle.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.archive,
        "k37-plugin-release-candidate-observer-bundle.tar.gz",
    )?;
    let summary_text = fs::read_to_string(&options.summary)
        .with_context(|| format!("read {}", options.summary.display()))?;
    let summary: serde_json::Value = serde_json::from_str(&summary_text)
        .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_release_candidate_observer_bundle_summary(&summary, &actual_archive_sha256)?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let platform_release_candidates = summary
        .get("platform_release_candidates")
        .and_then(serde_json::Value::as_object)
        .context("release-candidate observer bundle missing platform_release_candidates")?;
    for platform in ["macos", "ubuntu", "windows"] {
        let archive_path =
            format!("platforms/{platform}/plugin-release-candidate-verification.json");
        let verification =
            plugin_package_archive_json(&archive_files, &archive_path, "bundled verification")?;
        validate_plugin_release_candidate_verification_artifact(&verification, platform)?;
        let verification_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        let summary_verification = platform_release_candidates
            .get(platform)
            .with_context(|| format!("release-candidate observer bundle missing {platform}"))?;
        if verification_sha256 != json_string(summary_verification, "sha256") {
            anyhow::bail!(
                "{platform} final install transcript archive sha256 mismatch: summary {}, actual {}",
                json_string(summary_verification, "sha256"),
                verification_sha256
            );
        }
    }

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let transcript_path = options.out_dir.join("plugin-final-install-transcript.json");
    let markdown_path = options.out_dir.join("INSTALL-TRANSCRIPT.md");

    let verify_command = format!(
        "ao2 plugin release-candidate-observer-bundle-verify --summary {} --summary-sha256 {} --archive {} --archive-sha256 {} --json",
        options.summary.display(),
        actual_summary_sha256,
        options.archive.display(),
        actual_archive_sha256
    );
    let readiness_command = "ao2 plugin readiness --json".to_string();
    let target_steps = |target: &str| {
        serde_json::json!([
            {
                "target": target,
                "step": "readiness",
                "command": readiness_command,
                "executes_provider": false,
                "mutates_queue": false,
                "writes_memory": false,
                "mutates_control_plane": false,
                "mutates_ao_artifacts": false,
                "approves_release": false
            },
            {
                "target": target,
                "step": "verify_release_candidate_observer_bundle",
                "command": verify_command,
                "executes_provider": false,
                "mutates_queue": false,
                "writes_memory": false,
                "mutates_control_plane": false,
                "mutates_ao_artifacts": false,
                "approves_release": false
            }
        ])
    };
    let mut install_transcript = Vec::new();
    for target in ["codex", "claude"] {
        if let Some(steps) = target_steps(target).as_array() {
            install_transcript.extend(steps.iter().cloned());
        }
    }
    let install_transcript_value = serde_json::Value::Array(install_transcript);
    let install_transcript_sha256 = canonical_json_sha256(&install_transcript_value);

    let provider_auth = serde_json::json!({
        "local_oauth_cli_only": true,
        "provider_api_key_auth_allowed": false,
        "provider_api_key_env_required": false
    });
    let side_effects = serde_json::json!({
        "would_execute_provider": false,
        "would_execute_queue": false,
        "would_write_memory": false,
        "would_mutate_control_plane": false,
        "would_mutate_ao_artifacts": false,
        "would_approve_release": false
    });
    let transcript = serde_json::json!({
        "schema_version": "ao2.plugin-final-install-transcript.v1",
        "status": "ready_for_plugin_consumers",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "consumer_targets": ["codex", "claude"],
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "source_schema_version": "ao2.k37-plugin-release-candidate-observer-bundle.v1",
        "source_status": json_string(&summary, "status"),
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "platform_release_candidates_sha256": json_string(&summary, "platform_release_candidates_sha256"),
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "install_transcript": install_transcript_value,
        "install_transcript_sha256": install_transcript_sha256,
        "provider_auth": provider_auth,
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "side_effects": side_effects,
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor",
        "transcript_path": transcript_path.display().to_string(),
        "markdown_path": markdown_path.display().to_string()
    });
    validate_plugin_provider_auth(
        transcript
            .get("provider_auth")
            .context("final install transcript missing provider_auth")?,
        "final install transcript",
    )?;
    validate_plugin_observer_trust_boundary(
        transcript
            .get("trust_boundary")
            .context("final install transcript missing trust_boundary")?,
        "final install transcript",
    )?;
    validate_plugin_control_plane_observation(
        transcript
            .get("control_plane_observation")
            .context("final install transcript missing control_plane_observation")?,
        "final install transcript",
    )?;
    validate_plugin_side_effects_false(
        transcript
            .get("side_effects")
            .context("final install transcript missing side_effects")?,
        "final install transcript",
    )?;

    let transcript_body = serde_json::to_string_pretty(&transcript)?;
    atomic_write_text(&transcript_path, &transcript_body)?;
    factory_app_run_bundle_reject_secret_markers(
        &transcript_path,
        "plugin-final-install-transcript.json",
    )?;
    let transcript_sha256 = sha256_file(&transcript_path)?;

    let markdown = format!(
        "# AO2 Plugin Final Install Transcript\n\n\
Local OAuth CLI only. Provider API-key auth is forbidden.\n\n\
## Inputs\n\n\
- Release-candidate observer bundle: `{}`\n\
- Summary SHA256: `{}`\n\
- Archive: `{}`\n\
- Archive SHA256: `{}`\n\n\
## Codex\n\n\
1. `{}`\n\
2. `{}`\n\n\
## Claude\n\n\
1. `{}`\n\
2. `{}`\n\n\
Trust boundary: control plane remains read-only observer; no provider execution, queue mutation, memory write, AO artifact mutation, control-plane mutation, or release approval is performed by this transcript.\n",
        options.summary.display(),
        actual_summary_sha256,
        options.archive.display(),
        actual_archive_sha256,
        readiness_command,
        verify_command,
        readiness_command,
        verify_command
    );
    atomic_write_text(&markdown_path, &markdown)?;
    factory_app_run_bundle_reject_secret_markers(&markdown_path, "INSTALL-TRANSCRIPT.md")?;
    let markdown_sha256 = sha256_file(&markdown_path)?;

    let mut response = transcript;
    response["transcript_sha256"] = serde_json::json!(transcript_sha256);
    response["markdown_sha256"] = serde_json::json!(markdown_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_plugin_consumers");
        println!("schema_version=ao2.plugin-final-install-transcript.v1");
        println!("transcript={}", transcript_path.display());
        println!("markdown={}", markdown_path.display());
    }
    Ok(())
}

pub(super) fn plugin_final_install_transcript_observer_bundle(
    options: plugin_cli::PluginFinalInstallTranscriptObserverBundleOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let bundle_root = options.out_dir.join("bundle");
    if bundle_root.exists() {
        fs::remove_dir_all(&bundle_root)
            .with_context(|| format!("remove {}", bundle_root.display()))?;
    }
    fs::create_dir_all(&bundle_root)
        .with_context(|| format!("create {}", bundle_root.display()))?;

    let transcript_inputs = [
        (
            "macos",
            "codex",
            &options.macos_codex_transcript,
            options.macos_codex_sha256.trim(),
        ),
        (
            "macos",
            "claude",
            &options.macos_claude_transcript,
            options.macos_claude_sha256.trim(),
        ),
        (
            "ubuntu",
            "codex",
            &options.ubuntu_codex_transcript,
            options.ubuntu_codex_sha256.trim(),
        ),
        (
            "ubuntu",
            "claude",
            &options.ubuntu_claude_transcript,
            options.ubuntu_claude_sha256.trim(),
        ),
        (
            "windows",
            "codex",
            &options.windows_codex_transcript,
            options.windows_codex_sha256.trim(),
        ),
        (
            "windows",
            "claude",
            &options.windows_claude_transcript,
            options.windows_claude_sha256.trim(),
        ),
    ];

    let mut platform_transcripts = serde_json::Map::new();
    for platform in ["macos", "ubuntu", "windows"] {
        platform_transcripts.insert(platform.to_string(), serde_json::json!({}));
    }

    for (platform, target, source_path, supplied_sha256) in transcript_inputs {
        let (actual_sha256, transcript) = validate_plugin_final_install_transcript_artifact(
            source_path,
            supplied_sha256,
            platform,
            target,
        )?;

        let bundled_path = bundle_root
            .join("platforms")
            .join(platform)
            .join(target)
            .join("plugin-final-install-transcript.json");
        if let Some(parent) = bundled_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let transcript_text = fs::read_to_string(source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        atomic_write_text(&bundled_path, &transcript_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} {target} plugin-final-install-transcript.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} {target} final install transcript changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        let platform_entry = platform_transcripts
            .get_mut(platform)
            .and_then(serde_json::Value::as_object_mut)
            .with_context(|| format!("missing {platform} transcript map"))?;
        platform_entry.insert(
            target.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&transcript, "schema_version"),
                "status": json_string(&transcript, "status"),
                "source_schema_version": json_string(&transcript, "source_schema_version"),
                "summary_sha256": json_string(&transcript, "summary_sha256"),
                "archive_sha256": json_string(&transcript, "archive_sha256"),
                "install_transcript_sha256": json_string(&transcript, "install_transcript_sha256"),
                "consumer_targets": transcript.get("consumer_targets").cloned().unwrap_or_else(|| serde_json::json!([])),
                "observed_evidence_scope": transcript.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
                "provider_auth": transcript.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": transcript.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": transcript.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": transcript.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
                "token_safe_output_verified": json_bool(&transcript, "token_safe_output_verified"),
                "factory_v3_role": json_string(&transcript, "factory_v3_role")
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-plugin-final-install-transcript-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-final-install-transcript-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-plugin-final-install-transcript-observer-bundle.json");
    let platform_transcripts_value = serde_json::Value::Object(platform_transcripts);
    let platform_transcripts_sha256 = canonical_json_sha256(&platform_transcripts_value);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-plugin-final-install-transcript-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "target_count": 2,
        "transcript_count": 6,
        "platforms": ["macos", "ubuntu", "windows"],
        "consumer_targets": ["codex", "claude"],
        "observed_evidence_scope": [
            "ao2.plugin-final-install-transcript.v1"
        ],
        "platform_transcripts": platform_transcripts_value,
        "platform_transcripts_sha256": platform_transcripts_sha256,
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
        "k37-plugin-final-install-transcript-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-plugin-final-install-transcript-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_shipment_readiness(
    options: plugin_cli::PluginShipmentReadinessOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    if !is_git_sha_prefix(&options.control_plane_readback_commit) {
        anyhow::bail!(
            "control-plane readback commit must be a 7-40 character lowercase hex git sha"
        );
    }

    let (package_summary_sha256, package_summary) =
        validate_plugin_release_candidate_json_artifact(
            &options.package_summary,
            options.package_summary_sha256.trim(),
            "shipment package summary",
            "ao2.plugin-package.v1",
            &["packaged"],
        )?;
    let package_archive_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.package_archive,
        options.package_archive_sha256.trim(),
        "shipment package archive",
    )?;
    let (adapter_observer_bundle_sha256, adapter_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.adapter_observer_bundle,
            options.adapter_observer_bundle_sha256.trim(),
            "shipment adapter observer bundle",
            "ao2.k37-plugin-adapter-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let adapter_observer_archive_sha256 = validate_plugin_release_candidate_file_artifact(
        &options.adapter_observer_archive,
        options.adapter_observer_archive_sha256.trim(),
        "shipment adapter observer archive",
    )?;
    let (adapter_install_smoke_observer_bundle_sha256, adapter_install_smoke_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.adapter_install_smoke_observer_bundle,
            options.adapter_install_smoke_observer_bundle_sha256.trim(),
            "shipment adapter install-smoke observer bundle",
            "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let adapter_install_smoke_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.adapter_install_smoke_observer_archive,
            options.adapter_install_smoke_observer_archive_sha256.trim(),
            "shipment adapter install-smoke observer archive",
        )?;
    let (consumer_lifecycle_observer_bundle_sha256, consumer_lifecycle_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.consumer_lifecycle_observer_bundle,
            options.consumer_lifecycle_observer_bundle_sha256.trim(),
            "shipment consumer lifecycle observer bundle",
            "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let consumer_lifecycle_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.consumer_lifecycle_observer_archive,
            options.consumer_lifecycle_observer_archive_sha256.trim(),
            "shipment consumer lifecycle observer archive",
        )?;
    let (release_candidate_observer_bundle_sha256, release_candidate_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.release_candidate_observer_bundle,
            options.release_candidate_observer_bundle_sha256.trim(),
            "shipment release-candidate observer bundle",
            "ao2.k37-plugin-release-candidate-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let release_candidate_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.release_candidate_observer_archive,
            options.release_candidate_observer_archive_sha256.trim(),
            "shipment release-candidate observer archive",
        )?;
    let (final_install_transcript_observer_bundle_sha256, final_install_transcript_observer_bundle) =
        validate_plugin_release_candidate_json_artifact(
            &options.final_install_transcript_observer_bundle,
            options
                .final_install_transcript_observer_bundle_sha256
                .trim(),
            "shipment final install transcript observer bundle",
            "ao2.k37-plugin-final-install-transcript-observer-bundle.v1",
            &["ready_for_k37_observation"],
        )?;
    let final_install_transcript_observer_archive_sha256 =
        validate_plugin_release_candidate_file_artifact(
            &options.final_install_transcript_observer_archive,
            options
                .final_install_transcript_observer_archive_sha256
                .trim(),
            "shipment final install transcript observer archive",
        )?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let summary_path = options.out_dir.join("plugin-shipment-readiness.json");
    let shipment_evidence = serde_json::json!({
        "package": {
            "summary_path": options.package_summary.display().to_string(),
            "summary_sha256": package_summary_sha256,
            "summary_schema_version": json_string(&package_summary, "schema_version"),
            "archive_path": options.package_archive.display().to_string(),
            "archive_sha256": package_archive_sha256
        },
        "adapter_observer_bundle": {
            "summary_path": options.adapter_observer_bundle.display().to_string(),
            "summary_sha256": adapter_observer_bundle_sha256,
            "schema_version": json_string(&adapter_observer_bundle, "schema_version"),
            "archive_path": options.adapter_observer_archive.display().to_string(),
            "archive_sha256": adapter_observer_archive_sha256
        },
        "adapter_install_smoke_observer_bundle": {
            "summary_path": options.adapter_install_smoke_observer_bundle.display().to_string(),
            "summary_sha256": adapter_install_smoke_observer_bundle_sha256,
            "schema_version": json_string(&adapter_install_smoke_observer_bundle, "schema_version"),
            "archive_path": options.adapter_install_smoke_observer_archive.display().to_string(),
            "archive_sha256": adapter_install_smoke_observer_archive_sha256
        },
        "consumer_lifecycle_observer_bundle": {
            "summary_path": options.consumer_lifecycle_observer_bundle.display().to_string(),
            "summary_sha256": consumer_lifecycle_observer_bundle_sha256,
            "schema_version": json_string(&consumer_lifecycle_observer_bundle, "schema_version"),
            "archive_path": options.consumer_lifecycle_observer_archive.display().to_string(),
            "archive_sha256": consumer_lifecycle_observer_archive_sha256
        },
        "release_candidate_observer_bundle": {
            "summary_path": options.release_candidate_observer_bundle.display().to_string(),
            "summary_sha256": release_candidate_observer_bundle_sha256,
            "schema_version": json_string(&release_candidate_observer_bundle, "schema_version"),
            "archive_path": options.release_candidate_observer_archive.display().to_string(),
            "archive_sha256": release_candidate_observer_archive_sha256
        },
        "final_install_transcript_observer_bundle": {
            "summary_path": options.final_install_transcript_observer_bundle.display().to_string(),
            "summary_sha256": final_install_transcript_observer_bundle_sha256,
            "schema_version": json_string(&final_install_transcript_observer_bundle, "schema_version"),
            "archive_path": options.final_install_transcript_observer_archive.display().to_string(),
            "archive_sha256": final_install_transcript_observer_archive_sha256
        }
    });
    let shipment_evidence_sha256 = canonical_json_sha256(&shipment_evidence);
    let side_effects = serde_json::json!({
        "would_execute_provider": false,
        "would_execute_queue": false,
        "would_write_memory": false,
        "would_mutate_control_plane": false,
        "would_mutate_ao_artifacts": false,
        "would_approve_release": false
    });
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-shipment-readiness.v1",
        "status": "ready_for_operator_handoff",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "plugin_targets": ["codex", "claude"],
        "platforms": ["macos", "ubuntu", "windows"],
        "shipment_inputs": [
            "ao2.plugin-package.v1",
            "ao2.k37-plugin-adapter-observer-bundle.v1",
            "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
            "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
            "ao2.k37-plugin-release-candidate-observer-bundle.v1",
            "ao2.k37-plugin-final-install-transcript-observer-bundle.v1"
        ],
        "shipment_evidence": shipment_evidence,
        "shipment_evidence_sha256": shipment_evidence_sha256,
        "control_plane_readback": {
            "repo": "ao2-control-plane",
            "commit": options.control_plane_readback_commit,
            "role": "read_only_observer",
            "mutated_by_this_command": false,
            "approves_release": false
        },
        "required_operator_checks": [
            "verify local OAuth CLI login for Codex and Claude",
            "verify package digest before install",
            "verify final install transcript observer bundle digest",
            "keep ao2-control-plane read-only",
            "verify hosted C85 Release Gate result before operator handoff"
        ],
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
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    factory_app_run_bundle_reject_secret_markers(&summary_path, "plugin-shipment-readiness.json")?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_operator_handoff");
        println!("schema_version=ao2.plugin-shipment-readiness.v1");
        println!("summary={}", summary_path.display());
    }
    Ok(())
}

fn validate_plugin_final_install_transcript_artifact(
    path: &Path,
    supplied_sha256: &str,
    platform: &str,
    target: &str,
) -> Result<(String, serde_json::Value)> {
    let label = format!("{platform} {target} final install transcript");
    let actual_sha256 = sha256_file(path)?;
    if supplied_sha256 != actual_sha256 {
        anyhow::bail!(
            "{label} sha256 mismatch for {}: expected {}, actual {}",
            path.display(),
            supplied_sha256,
            actual_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(path, &label)?;
    let transcript: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;
    if json_string(&transcript, "schema_version") != "ao2.plugin-final-install-transcript.v1" {
        anyhow::bail!(
            "{label} requires ao2.plugin-final-install-transcript.v1, got {}",
            json_string(&transcript, "schema_version")
        );
    }
    if json_string(&transcript, "status") != "ready_for_plugin_consumers" {
        anyhow::bail!("{label} status must be ready_for_plugin_consumers");
    }
    if json_string(&transcript, "producer") != "ao2" {
        anyhow::bail!("{label} producer must be ao2");
    }
    if transcript.get("consumer_targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("{label} consumer_targets must be codex and claude");
    }
    if json_string(&transcript, "source_schema_version")
        != "ao2.k37-plugin-release-candidate-observer-bundle.v1"
        || json_string(&transcript, "source_status") != "ready_for_k37_observation"
    {
        anyhow::bail!("{label} source release-candidate observer bundle contract is invalid");
    }
    if json_u64(&transcript, "platform_count") != 3
        || transcript.get("platforms") != Some(&serde_json::json!(["macos", "ubuntu", "windows"]))
    {
        anyhow::bail!("{label} must cover macos, ubuntu, and windows");
    }
    for field in [
        "summary_sha256",
        "archive_sha256",
        "platform_release_candidates_sha256",
        "install_transcript_sha256",
    ] {
        let digest = json_string(&transcript, field);
        if !is_sha256_hex(&digest) {
            anyhow::bail!("{label} {field} must be a sha256 digest");
        }
    }
    let install_transcript = transcript
        .get("install_transcript")
        .and_then(serde_json::Value::as_array)
        .context("final install transcript missing install_transcript")?;
    for required_target in ["codex", "claude"] {
        if !install_transcript
            .iter()
            .any(|entry| json_string(entry, "target") == required_target)
        {
            anyhow::bail!("{label} missing install transcript target {required_target}");
        }
    }
    for entry in install_transcript {
        for field in [
            "executes_provider",
            "mutates_queue",
            "writes_memory",
            "mutates_control_plane",
            "mutates_ao_artifacts",
            "approves_release",
        ] {
            if json_bool(entry, field) {
                anyhow::bail!("{label} install transcript step {field} must be false");
            }
        }
    }
    validate_plugin_provider_auth(
        transcript
            .get("provider_auth")
            .context("final install transcript missing provider_auth")?,
        &label,
    )?;
    validate_plugin_observer_trust_boundary(
        transcript
            .get("trust_boundary")
            .context("final install transcript missing trust_boundary")?,
        &label,
    )?;
    validate_plugin_control_plane_observation(
        transcript
            .get("control_plane_observation")
            .context("final install transcript missing control_plane_observation")?,
        &label,
    )?;
    validate_plugin_side_effects_false(
        transcript
            .get("side_effects")
            .context("final install transcript missing side_effects")?,
        &label,
    )?;
    if !json_bool(&transcript, "token_safe_output_verified") {
        anyhow::bail!("{label} token_safe_output_verified must be true");
    }
    if json_string(&transcript, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("{label} factory_v3_role must be parity_auditor");
    }
    Ok((actual_sha256, transcript))
}

fn validate_plugin_release_candidate_control_plane_fixture_handoff(
    handoff: &serde_json::Value,
) -> Result<()> {
    if json_string(handoff, "schema_version")
        != "ao2.release-candidate-control-plane-fixture-handoff.v1"
    {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff requires ao2.release-candidate-control-plane-fixture-handoff.v1, got {}",
            json_string(handoff, "schema_version")
        );
    }
    if json_string(handoff, "status") != "ready_for_control_plane_readback" {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff status must be ready_for_control_plane_readback"
        );
    }
    if json_string(handoff, "producer") != "ao2" {
        anyhow::bail!("release-candidate control-plane fixture handoff producer must be ao2");
    }
    if json_string(handoff, "source_schema_version")
        != "ao2.k37-plugin-release-candidate-observer-bundle.v1"
        || json_string(handoff, "expected_schema_version")
            != "ao2.k37-plugin-release-candidate-observer-bundle.v1"
        || json_string(handoff, "expected_status") != "ready_for_k37_observation"
    {
        anyhow::bail!("release-candidate control-plane fixture handoff source contract is invalid");
    }
    if handoff.get("expected_platforms") != Some(&serde_json::json!(["macos", "ubuntu", "windows"]))
        || json_u64(handoff, "expected_platform_count") != 3
    {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff must expect macos, ubuntu, and windows"
        );
    }
    if handoff.get("expected_observed_evidence_scope")
        != Some(&serde_json::json!([
            "ao2.plugin-release-candidate.v1",
            "ao2.plugin-release-candidate-verification.v1"
        ]))
    {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff observed evidence scope is invalid"
        );
    }
    if json_string(handoff, "recommended_control_plane_fixture_path")
        != "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/release-candidate-observer-bundle.json"
        || json_string(handoff, "recommended_control_plane_test_name")
            != "release_candidate_observer_bundle_is_read_only_three_platform_evidence"
    {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff recommendation metadata is invalid"
        );
    }
    validate_plugin_provider_auth(
        handoff
            .get("provider_auth")
            .context("release-candidate control-plane fixture handoff missing provider_auth")?,
        "release-candidate control-plane fixture handoff",
    )?;
    validate_plugin_observer_trust_boundary(
        handoff
            .get("trust_boundary")
            .context("release-candidate control-plane fixture handoff missing trust_boundary")?,
        "release-candidate control-plane fixture handoff",
    )?;
    validate_plugin_control_plane_observation(
        handoff.get("control_plane_observation").context(
            "release-candidate control-plane fixture handoff missing control_plane_observation",
        )?,
        "release-candidate control-plane fixture handoff",
    )?;
    validate_plugin_side_effects_false(
        handoff
            .get("side_effects")
            .context("release-candidate control-plane fixture handoff missing side_effects")?,
        "release-candidate control-plane fixture handoff",
    )?;
    if !json_bool(handoff, "token_safe_output_verified") {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff token_safe_output_verified must be true"
        );
    }
    if json_string(handoff, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!(
            "release-candidate control-plane fixture handoff factory_v3_role must be parity_auditor"
        );
    }
    Ok(())
}

fn validate_plugin_release_candidate_windows_recovery_manifest(
    recovery_path: &Path,
    recovery: &serde_json::Value,
) -> Result<PathBuf> {
    if json_string(recovery, "schema_version") != "ao2.plugin-release-candidate-windows-recovery.v1"
    {
        anyhow::bail!(
            "release-candidate Windows recovery requires ao2.plugin-release-candidate-windows-recovery.v1, got {}",
            json_string(recovery, "schema_version")
        );
    }
    if json_string(recovery, "status") != "ready_for_windows_execution" {
        anyhow::bail!(
            "release-candidate Windows recovery status must be ready_for_windows_execution"
        );
    }
    if json_string(recovery, "platform") != "windows" {
        anyhow::bail!("release-candidate Windows recovery platform must be windows");
    }

    let script_path =
        resolve_cli_artifact_reference(recovery_path, &json_string(recovery, "script_path"));
    if !script_path.is_file() {
        anyhow::bail!(
            "release-candidate Windows recovery script does not exist: {}",
            script_path.display()
        );
    }
    let expected_script_sha256 = json_string(recovery, "script_sha256");
    if !is_sha256_hex(&expected_script_sha256) {
        anyhow::bail!("release-candidate Windows recovery script_sha256 must be a digest");
    }
    let actual_script_sha256 = sha256_file(&script_path)?;
    if expected_script_sha256 != actual_script_sha256 {
        anyhow::bail!(
            "release-candidate Windows recovery script sha256 mismatch: expected {expected_script_sha256}, actual {actual_script_sha256}"
        );
    }
    factory_app_run_bundle_reject_secret_markers(&script_path, "run-release-candidate.ps1")?;
    let script = fs::read_to_string(&script_path)
        .with_context(|| format!("read {}", script_path.display()))?;
    for required in [
        "plugin release-candidate",
        "plugin release-candidate-verify",
        "Join-Path $PSScriptRoot",
    ] {
        if !script.contains(required) {
            anyhow::bail!(
                "release-candidate Windows recovery script missing required text: {required}"
            );
        }
    }

    let execution = recovery
        .get("execution")
        .context("release-candidate Windows recovery missing execution")?;
    if json_string(execution, "runner") != "powershell"
        || !json_string(execution, "single_session_command").contains("run-release-candidate.ps1")
        || json_string(execution, "produces") != "ao2.plugin-release-candidate-verification.v1"
    {
        anyhow::bail!("release-candidate Windows recovery execution contract is invalid");
    }

    let inputs = recovery
        .get("release_review_inputs")
        .context("release-candidate Windows recovery missing release_review_inputs")?;
    for (section, fields) in [
        ("package", vec!["summary_sha256", "archive_sha256"]),
        ("distribution_rehearsal", vec!["sha256"]),
        (
            "adapter_observer_bundle",
            vec!["summary_sha256", "archive_sha256"],
        ),
        (
            "adapter_install_smoke_observer_bundle",
            vec!["summary_sha256", "archive_sha256"],
        ),
        (
            "consumer_lifecycle_observer_bundle",
            vec!["summary_sha256", "archive_sha256"],
        ),
        (
            "release_gate_with_replacement_observer_bundle",
            vec!["summary_sha256", "archive_sha256"],
        ),
        ("control_plane_fixture_handoff_verification", vec!["sha256"]),
    ] {
        let entry = inputs.get(section).with_context(|| {
            format!("release-candidate Windows recovery missing input {section}")
        })?;
        for field in fields {
            let digest = json_string(entry, field);
            if !is_sha256_hex(&digest) {
                anyhow::bail!(
                    "release-candidate Windows recovery input {section}.{field} must be a sha256 digest"
                );
            }
        }
    }

    let readback = recovery
        .get("control_plane_readback")
        .context("release-candidate Windows recovery missing control_plane_readback")?;
    if json_string(readback, "repo") != "ao2-control-plane"
        || json_string(readback, "role") != "read_only_observer"
        || json_bool(readback, "mutated_by_this_command")
        || json_bool(readback, "approves_release")
        || !is_git_sha_prefix(&json_string(readback, "commit"))
    {
        anyhow::bail!("release-candidate Windows recovery control-plane readback is invalid");
    }
    validate_plugin_provider_auth(
        recovery
            .get("provider_auth")
            .context("release-candidate Windows recovery missing provider_auth")?,
        "release-candidate Windows recovery",
    )?;
    validate_plugin_observer_trust_boundary(
        recovery
            .get("trust_boundary")
            .context("release-candidate Windows recovery missing trust_boundary")?,
        "release-candidate Windows recovery",
    )?;
    validate_plugin_control_plane_observation(
        recovery
            .get("control_plane_observation")
            .context("release-candidate Windows recovery missing control_plane_observation")?,
        "release-candidate Windows recovery",
    )?;
    validate_plugin_side_effects_false(
        recovery
            .get("side_effects")
            .context("release-candidate Windows recovery missing side_effects")?,
        "release-candidate Windows recovery",
    )?;
    if !json_bool(recovery, "token_safe_output_verified") {
        anyhow::bail!("release-candidate Windows recovery token_safe_output_verified must be true");
    }
    if json_string(recovery, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("release-candidate Windows recovery factory_v3_role must be parity_auditor");
    }
    Ok(script_path)
}

fn validate_plugin_release_candidate_file_artifact(
    path: &Path,
    supplied_sha256: &str,
    label: &str,
) -> Result<String> {
    let actual_sha256 = sha256_file(path)?;
    if supplied_sha256 != actual_sha256 {
        anyhow::bail!(
            "{label} sha256 mismatch for {}: expected {}, actual {}",
            path.display(),
            supplied_sha256,
            actual_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(path, label)?;
    Ok(actual_sha256)
}

fn validate_plugin_release_candidate_json_artifact(
    path: &Path,
    supplied_sha256: &str,
    label: &str,
    expected_schema_version: &str,
    accepted_statuses: &[&str],
) -> Result<(String, serde_json::Value)> {
    let actual_sha256 =
        validate_plugin_release_candidate_file_artifact(path, supplied_sha256, label)?;
    let value: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;
    if json_string(&value, "schema_version") != expected_schema_version {
        anyhow::bail!(
            "{label} requires {expected_schema_version}, got {}",
            json_string(&value, "schema_version")
        );
    }
    let status = json_string(&value, "status");
    if !accepted_statuses.iter().any(|accepted| *accepted == status) {
        anyhow::bail!("{label} status is not accepted: {status}");
    }
    if let Some(auth) = value.get("provider_auth").filter(|auth| !auth.is_null()) {
        validate_plugin_provider_auth(auth, label)?;
    }
    validate_plugin_observer_trust_boundary(
        value
            .get("trust_boundary")
            .with_context(|| format!("{label} missing trust_boundary"))?,
        label,
    )?;
    validate_plugin_control_plane_observation(
        value
            .get("control_plane_observation")
            .with_context(|| format!("{label} missing control_plane_observation"))?,
        label,
    )?;
    if let Some(side_effects) = value
        .get("side_effects")
        .filter(|side_effects| !side_effects.is_null())
    {
        validate_plugin_side_effects_false(side_effects, label)?;
    }
    if !value
        .get("token_safe_output_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
    {
        anyhow::bail!("{label} token_safe_output_verified must not be false");
    }
    if json_string(&value, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("{label} factory_v3_role must be parity_auditor");
    }
    Ok((actual_sha256, value))
}

fn validate_plugin_release_candidate_observer_bundle_summary(
    summary: &serde_json::Value,
    archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version")
        != "ao2.k37-plugin-release-candidate-observer-bundle.v1"
    {
        anyhow::bail!(
            "release-candidate observer bundle requires ao2.k37-plugin-release-candidate-observer-bundle.v1, got {}",
            json_string(summary, "schema_version")
        );
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!("release-candidate observer bundle status must be ready_for_k37_observation");
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("release-candidate observer bundle producer must be ao2");
    }
    if json_string(summary, "archive_sha256") != archive_sha256 {
        anyhow::bail!("release-candidate observer bundle archive sha256 does not match");
    }
    if summary
        .get("platform_count")
        .and_then(serde_json::Value::as_u64)
        != Some(3)
    {
        anyhow::bail!("release-candidate observer bundle platform_count must be 3");
    }
    if summary.get("platforms") != Some(&serde_json::json!(["macos", "ubuntu", "windows"])) {
        anyhow::bail!("release-candidate observer bundle platforms must be macos/ubuntu/windows");
    }
    let observed_scope = summary
        .get("observed_evidence_scope")
        .and_then(serde_json::Value::as_array)
        .context("release-candidate observer bundle missing observed_evidence_scope")?;
    for required in [
        "ao2.plugin-release-candidate.v1",
        "ao2.plugin-release-candidate-verification.v1",
    ] {
        if !observed_scope
            .iter()
            .any(|entry| entry.as_str() == Some(required))
        {
            anyhow::bail!("release-candidate observer bundle missing scope {required}");
        }
    }
    let platform_release_candidates = summary
        .get("platform_release_candidates")
        .context("release-candidate observer bundle missing platform_release_candidates")?;
    if json_string(summary, "platform_release_candidates_sha256")
        != canonical_json_sha256(platform_release_candidates)
    {
        anyhow::bail!("release-candidate observer bundle platform digest mismatch");
    }
    for platform in ["macos", "ubuntu", "windows"] {
        if platform_release_candidates.get(platform).is_none() {
            anyhow::bail!("release-candidate observer bundle missing {platform}");
        }
    }
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("release-candidate observer bundle missing trust_boundary")?,
        "release-candidate observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("release-candidate observer bundle missing control_plane_observation")?,
        "release-candidate observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("release-candidate observer bundle missing side_effects")?,
        "release-candidate observer bundle",
    )?;
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("release-candidate observer bundle token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("release-candidate observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

fn validate_plugin_release_candidate_verification_artifact(
    verification: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(verification, "schema_version") != "ao2.plugin-release-candidate-verification.v1"
    {
        anyhow::bail!(
            "{platform} release-candidate verification requires ao2.plugin-release-candidate-verification.v1, got {}",
            json_string(verification, "schema_version")
        );
    }
    if json_string(verification, "status") != "passed" {
        anyhow::bail!("{platform} release-candidate verification must be passed");
    }
    if json_string(verification, "source_schema_version") != "ao2.plugin-release-candidate.v1" {
        anyhow::bail!(
            "{platform} release-candidate verification source_schema_version must be ao2.plugin-release-candidate.v1"
        );
    }
    for field in ["summary_sha256", "evidence_sha256"] {
        let digest = json_string(verification, field);
        if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            anyhow::bail!(
                "{platform} release-candidate verification {field} must be a sha256 hex digest"
            );
        }
    }
    let release_review_inputs = verification
        .get("release_review_inputs")
        .and_then(serde_json::Value::as_array)
        .context("release-candidate verification missing release_review_inputs")?;
    for required in [
        "ao2.plugin-package.v1",
        "ao2.plugin-distribution-rehearsal.v1",
        "ao2.k37-plugin-adapter-observer-bundle.v1",
        "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
        "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
        "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
        "ao2.control-plane-fixture-handoff-verification.v1",
    ] {
        if !release_review_inputs
            .iter()
            .any(|entry| entry.as_str() == Some(required))
        {
            anyhow::bail!(
                "{platform} release-candidate verification missing release input {required}"
            );
        }
    }
    let readback = verification
        .get("control_plane_readback")
        .context("release-candidate verification missing control_plane_readback")?;
    if json_string(readback, "repo") != "ao2-control-plane"
        || json_string(readback, "role") != "read_only_observer"
        || json_bool(readback, "mutated_by_this_command")
        || json_bool(readback, "approves_release")
        || !is_git_sha_prefix(&json_string(readback, "commit"))
    {
        anyhow::bail!(
            "{platform} release-candidate verification control-plane readback is invalid"
        );
    }
    validate_plugin_provider_auth(
        verification
            .get("provider_auth")
            .context("release-candidate verification missing provider_auth")?,
        &format!("{platform} release-candidate verification"),
    )?;
    validate_plugin_observer_trust_boundary(
        verification
            .get("trust_boundary")
            .context("release-candidate verification missing trust_boundary")?,
        &format!("{platform} release-candidate verification"),
    )?;
    validate_plugin_control_plane_observation(
        verification
            .get("control_plane_observation")
            .context("release-candidate verification missing control_plane_observation")?,
        &format!("{platform} release-candidate verification"),
    )?;
    validate_plugin_side_effects_false(
        verification
            .get("side_effects")
            .context("release-candidate verification missing side_effects")?,
        &format!("{platform} release-candidate verification"),
    )?;
    if !json_bool(verification, "token_safe_output_verified") {
        anyhow::bail!(
            "{platform} release-candidate verification token_safe_output_verified must be true"
        );
    }
    if json_string(verification, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!(
            "{platform} release-candidate verification factory_v3_role must be parity_auditor"
        );
    }
    Ok(())
}

fn validate_plugin_release_candidate_summary(summary: &serde_json::Value) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.plugin-release-candidate.v1" {
        anyhow::bail!(
            "plugin release-candidate requires ao2.plugin-release-candidate.v1, got {}",
            json_string(summary, "schema_version")
        );
    }
    if json_string(summary, "status") != "ready_for_local_release_review" {
        anyhow::bail!("plugin release-candidate status must be ready_for_local_release_review");
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("plugin release-candidate producer must be ao2");
    }
    let evidence = summary
        .get("evidence")
        .context("plugin release-candidate missing evidence")?;
    if json_string(summary, "evidence_sha256") != canonical_json_sha256(evidence) {
        anyhow::bail!("plugin release-candidate evidence digest mismatch");
    }
    for field in [
        "package",
        "distribution_rehearsal",
        "adapter_observer_bundle",
        "adapter_install_smoke_observer_bundle",
        "consumer_lifecycle_observer_bundle",
        "release_gate_with_replacement_observer_bundle",
        "control_plane_fixture_handoff_verification",
    ] {
        if evidence.get(field).is_none() {
            anyhow::bail!("plugin release-candidate missing evidence field {field}");
        }
    }
    let readback = summary
        .get("control_plane_readback")
        .context("plugin release-candidate missing control_plane_readback")?;
    if json_string(readback, "repo") != "ao2-control-plane"
        || json_string(readback, "role") != "read_only_observer"
        || json_bool(readback, "mutated_by_this_command")
        || json_bool(readback, "approves_release")
        || !is_git_sha_prefix(&json_string(readback, "commit"))
    {
        anyhow::bail!("plugin release-candidate control-plane readback is invalid");
    }
    validate_plugin_provider_auth(
        summary
            .get("provider_auth")
            .context("plugin release-candidate missing provider_auth")?,
        "plugin release-candidate",
    )?;
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("plugin release-candidate missing trust_boundary")?,
        "plugin release-candidate",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("plugin release-candidate missing control_plane_observation")?,
        "plugin release-candidate",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("plugin release-candidate missing side_effects")?,
        "plugin release-candidate",
    )?;
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("plugin release-candidate token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("plugin release-candidate factory_v3_role must be parity_auditor");
    }
    Ok(())
}
