use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::cli_util::canonical_json_sha256;
use super::plugin_cli::{
    PluginConsumerLifecycleObserverBundleOptions,
    PluginConsumerLifecycleObserverBundleVerifyOptions, PluginConsumerLifecycleOptions,
    PluginConsumerLifecycleWindowsRecoveryOptions,
};
use super::plugin_contract::{
    validate_plugin_adapter_install_smoke_contract, validate_plugin_adapter_scaffold_summary,
    validate_plugin_consumer_lifecycle_contract,
    validate_plugin_consumer_lifecycle_observer_bundle_summary,
};
use super::plugin_distribution::{
    plugin_package_archive_json, read_plugin_package_archive_files, sha256_archive_file,
    validate_plugin_install_smoke_contract, validate_plugin_package_contract,
    write_plugin_package_installation,
};
use super::plugin_wrapper::validate_plugin_readiness_contract;
use super::{
    atomic_write_text, create_tar_gz, factory_app_run_bundle_reject_secret_markers,
    fail_if_provider_api_key_env_present, is_sha256_hex, json_string,
    resolve_cli_artifact_reference, run_current_ao2_json_command, sha256_file,
};

pub(super) fn plugin_consumer_lifecycle(options: PluginConsumerLifecycleOptions) -> Result<()> {
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
    let archive_files = read_plugin_package_archive_files(&options.package_archive)?;

    let supplied_adapter_scaffold_sha256 = options.adapter_scaffold_sha256.trim();
    let actual_adapter_scaffold_sha256 = sha256_file(&options.adapter_scaffold)?;
    if supplied_adapter_scaffold_sha256 != actual_adapter_scaffold_sha256 {
        anyhow::bail!(
            "plugin adapter scaffold sha256 mismatch for {}: expected {}, actual {}",
            options.adapter_scaffold.display(),
            supplied_adapter_scaffold_sha256,
            actual_adapter_scaffold_sha256
        );
    }
    let adapter_scaffold: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.adapter_scaffold)
            .with_context(|| format!("read {}", options.adapter_scaffold.display()))?,
    )
    .with_context(|| format!("parse {}", options.adapter_scaffold.display()))?;
    validate_plugin_adapter_scaffold_summary(&adapter_scaffold)?;
    if json_string(
        adapter_scaffold
            .get("package")
            .context("plugin adapter scaffold missing package")?,
        "summary_sha256",
    ) != actual_package_summary_sha256
    {
        anyhow::bail!("plugin adapter scaffold package summary digest does not match package");
    }
    if json_string(
        adapter_scaffold
            .get("package")
            .context("plugin adapter scaffold missing package")?,
        "archive_sha256",
    ) != actual_package_archive_sha256
    {
        anyhow::bail!("plugin adapter scaffold package archive digest does not match package");
    }
    factory_app_run_bundle_reject_secret_markers(
        &options.package_summary,
        "ao2-plugin-package.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.package_archive,
        "ao2-plugin-package.tar.gz",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.adapter_scaffold,
        "plugin-adapter-scaffold.json",
    )?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let mut target_results = serde_json::Map::new();
    for target in ["codex", "claude"] {
        let wrapper_sandbox_dir = options.out_dir.join("wrapper-sandboxes").join(target);
        if wrapper_sandbox_dir.exists() {
            fs::remove_dir_all(&wrapper_sandbox_dir)
                .with_context(|| format!("remove {}", wrapper_sandbox_dir.display()))?;
        }
        fs::create_dir_all(&wrapper_sandbox_dir)
            .with_context(|| format!("create {}", wrapper_sandbox_dir.display()))?;
        let installed_package_dir = wrapper_sandbox_dir.join("ao2-governed-execution");
        write_plugin_installation_from_archive(
            &archive_files,
            &installed_package_dir,
            &options.package_summary,
            &options.package_archive,
        )?;
        let installed_summary = installed_package_dir.join("ao2-plugin-package.json");
        let installed_archive = installed_package_dir.join("ao2-plugin-package.tar.gz");
        let installed_adapter_summary = write_plugin_consumer_installed_adapter_scaffold(
            &adapter_scaffold,
            &wrapper_sandbox_dir.join("adapter-scaffold"),
            &installed_summary,
            &installed_archive,
        )?;
        let installed_adapter_summary_sha256 = sha256_file(&installed_adapter_summary)?;

        let target_result = run_plugin_consumer_lifecycle_target(
            target,
            &wrapper_sandbox_dir,
            &installed_package_dir,
            &actual_package_summary_sha256,
            &actual_package_archive_sha256,
            &installed_adapter_summary,
            &installed_adapter_summary_sha256,
        )?;
        target_results.insert(target.to_string(), target_result);
    }

    let summary_path = options.out_dir.join("plugin-consumer-lifecycle.json");
    let side_effects = serde_json::json!({
        "provider_execution_started": false,
        "queue_mutated": false,
        "memory_written": false,
        "control_plane_mutated": false,
        "ao_artifacts_mutated": false,
        "release_approved": false
    });
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-consumer-lifecycle.v1",
        "status": "passed",
        "summary_path": summary_path.display().to_string(),
        "targets": ["codex", "claude"],
        "package": {
            "summary_path": options.package_summary.display().to_string(),
            "summary_sha256": actual_package_summary_sha256,
            "archive_path": options.package_archive.display().to_string(),
            "archive_sha256": actual_package_archive_sha256
        },
        "adapter_scaffold": {
            "summary_path": options.adapter_scaffold.display().to_string(),
            "summary_sha256": actual_adapter_scaffold_sha256
        },
        "target_results": serde_json::Value::Object(target_results),
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
    let summary_body = serde_json::to_string_pretty(&summary)?;
    atomic_write_text(&summary_path, &summary_body)?;
    factory_app_run_bundle_reject_secret_markers(&summary_path, "plugin-consumer-lifecycle.json")?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-consumer-lifecycle.v1");
        println!("summary={}", summary_path.display());
    }
    Ok(())
}

pub(super) fn plugin_consumer_lifecycle_windows_recovery(
    options: PluginConsumerLifecycleWindowsRecoveryOptions,
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

    let supplied_adapter_scaffold_sha256 = options.adapter_scaffold_sha256.trim();
    let actual_adapter_scaffold_sha256 = sha256_file(&options.adapter_scaffold)?;
    if supplied_adapter_scaffold_sha256 != actual_adapter_scaffold_sha256 {
        anyhow::bail!(
            "plugin adapter scaffold sha256 mismatch for {}: expected {}, actual {}",
            options.adapter_scaffold.display(),
            supplied_adapter_scaffold_sha256,
            actual_adapter_scaffold_sha256
        );
    }
    let adapter_scaffold: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.adapter_scaffold)
            .with_context(|| format!("read {}", options.adapter_scaffold.display()))?,
    )
    .with_context(|| format!("parse {}", options.adapter_scaffold.display()))?;
    validate_plugin_adapter_scaffold_summary(&adapter_scaffold)?;

    factory_app_run_bundle_reject_secret_markers(
        &options.package_summary,
        "ao2-plugin-package.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.package_archive,
        "ao2-plugin-package.tar.gz",
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &options.adapter_scaffold,
        "plugin-adapter-scaffold.json",
    )?;

    let inputs_dir = options.out_dir.join("inputs");
    fs::create_dir_all(&inputs_dir).with_context(|| format!("create {}", inputs_dir.display()))?;
    let copied_package_summary = inputs_dir.join("ao2-plugin-package.json");
    let copied_package_archive = inputs_dir.join("ao2-plugin-package.tar.gz");
    let copied_adapter_scaffold = inputs_dir.join("plugin-adapter-scaffold.json");
    fs::copy(&options.package_summary, &copied_package_summary).with_context(|| {
        format!(
            "copy {} to {}",
            options.package_summary.display(),
            copied_package_summary.display()
        )
    })?;
    fs::copy(&options.package_archive, &copied_package_archive).with_context(|| {
        format!(
            "copy {} to {}",
            options.package_archive.display(),
            copied_package_archive.display()
        )
    })?;
    fs::copy(&options.adapter_scaffold, &copied_adapter_scaffold).with_context(|| {
        format!(
            "copy {} to {}",
            options.adapter_scaffold.display(),
            copied_adapter_scaffold.display()
        )
    })?;
    if sha256_file(&copied_package_summary)? != actual_package_summary_sha256 {
        anyhow::bail!("copied plugin package summary digest changed");
    }
    if sha256_file(&copied_package_archive)? != actual_package_archive_sha256 {
        anyhow::bail!("copied plugin package archive digest changed");
    }
    if sha256_file(&copied_adapter_scaffold)? != actual_adapter_scaffold_sha256 {
        anyhow::bail!("copied plugin adapter scaffold digest changed");
    }

    let portable_adapter_dir = inputs_dir.join("adapter-scaffold");
    fs::create_dir_all(&portable_adapter_dir)
        .with_context(|| format!("create {}", portable_adapter_dir.display()))?;
    let mut portable_adapter_scaffold = adapter_scaffold.clone();
    portable_adapter_scaffold["summary_path"] =
        serde_json::json!("inputs/adapter-scaffold/plugin-adapter-scaffold.json");
    portable_adapter_scaffold["package"]["summary_path"] =
        serde_json::json!("inputs/ao2-plugin-package.json");
    portable_adapter_scaffold["package"]["archive_path"] =
        serde_json::json!("inputs/ao2-plugin-package.tar.gz");
    let source_adapter_files = adapter_scaffold
        .get("adapter_files")
        .and_then(serde_json::Value::as_object)
        .context("plugin adapter scaffold missing adapter_files")?;
    let mut portable_adapter_files = serde_json::Map::new();
    for target in ["codex", "claude"] {
        let entry = source_adapter_files
            .get(target)
            .with_context(|| format!("plugin adapter scaffold missing {target} adapter file"))?;
        let source_path =
            resolve_cli_artifact_reference(&options.adapter_scaffold, &json_string(entry, "path"));
        let expected_sha256 = json_string(entry, "sha256");
        if !is_sha256_hex(&expected_sha256) {
            anyhow::bail!("plugin adapter scaffold {target} adapter sha256 must be a digest");
        }
        let actual_sha256 = sha256_file(&source_path)?;
        if expected_sha256 != actual_sha256 {
            anyhow::bail!(
                "plugin adapter scaffold {target} adapter sha256 mismatch: expected {expected_sha256}, actual {actual_sha256}"
            );
        }
        let mut adapter: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&source_path)
                .with_context(|| format!("read {}", source_path.display()))?,
        )
        .with_context(|| format!("parse {}", source_path.display()))?;
        adapter["inputs"]["package_summary_path"] =
            serde_json::json!("inputs/ao2-plugin-package.json");
        adapter["inputs"]["package_archive_path"] =
            serde_json::json!("inputs/ao2-plugin-package.tar.gz");

        let target_dir = portable_adapter_dir.join(target);
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("create {}", target_dir.display()))?;
        let portable_adapter_path = target_dir.join("ao2-plugin-adapter.json");
        atomic_write_text(
            &portable_adapter_path,
            &serde_json::to_string_pretty(&adapter)?,
        )?;
        factory_app_run_bundle_reject_secret_markers(
            &portable_adapter_path,
            &format!("{target} portable ao2-plugin-adapter.json"),
        )?;
        let portable_relative_path =
            format!("inputs/adapter-scaffold/{target}/ao2-plugin-adapter.json");
        portable_adapter_files.insert(
            target.to_string(),
            serde_json::json!({
                "path": portable_relative_path,
                "sha256": sha256_file(&portable_adapter_path)?,
                "schema_version": "ao2.plugin-adapter.v1",
                "status": "ready_for_local_oauth_wrapper_integration"
            }),
        );
    }
    portable_adapter_scaffold["adapter_files"] = serde_json::Value::Object(portable_adapter_files);
    let portable_adapter_summary = portable_adapter_dir.join("plugin-adapter-scaffold.json");
    atomic_write_text(
        &portable_adapter_summary,
        &serde_json::to_string_pretty(&portable_adapter_scaffold)?,
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &portable_adapter_summary,
        "portable plugin-adapter-scaffold.json",
    )?;
    let portable_adapter_summary_sha256 = sha256_file(&portable_adapter_summary)?;

    let script_path = options.out_dir.join("run-consumer-lifecycle.ps1");
    let script = format!(
        r#"param(
    [string]$Ao2 = "ao2",
    [string]$OutDir = (Join-Path $PSScriptRoot "consumer-lifecycle")
)

$ErrorActionPreference = "Stop"
$InputRoot = Join-Path $PSScriptRoot "inputs"
$AdapterScaffold = Join-Path (Join-Path $InputRoot "adapter-scaffold") "plugin-adapter-scaffold.json"
& $Ao2 plugin consumer-lifecycle `
    --package-summary (Join-Path $InputRoot "ao2-plugin-package.json") `
    --package-summary-sha256 "{package_summary_sha256}" `
    --package-archive (Join-Path $InputRoot "ao2-plugin-package.tar.gz") `
    --package-archive-sha256 "{package_archive_sha256}" `
    --adapter-scaffold $AdapterScaffold `
    --adapter-scaffold-sha256 "{adapter_scaffold_sha256}" `
    --out-dir $OutDir `
    --json
if ($LASTEXITCODE -ne 0) {{
    exit $LASTEXITCODE
}}
"#,
        package_summary_sha256 = actual_package_summary_sha256,
        package_archive_sha256 = actual_package_archive_sha256,
        adapter_scaffold_sha256 = portable_adapter_summary_sha256.as_str()
    );
    atomic_write_text(&script_path, &script)?;
    factory_app_run_bundle_reject_secret_markers(&script_path, "run-consumer-lifecycle.ps1")?;
    let script_sha256 = sha256_file(&script_path)?;

    let manifest_path = options
        .out_dir
        .join("windows-consumer-lifecycle-recovery.json");
    let side_effects = serde_json::json!({
        "provider_execution_started": false,
        "queue_mutated": false,
        "memory_written": false,
        "control_plane_mutated": false,
        "ao_artifacts_mutated": false,
        "release_approved": false
    });
    let manifest = serde_json::json!({
        "schema_version": "ao2.plugin-consumer-lifecycle-windows-recovery.v1",
        "status": "ready_for_windows_execution",
        "platform": "windows",
        "manifest_path": manifest_path.display().to_string(),
        "script_path": script_path.display().to_string(),
        "script_sha256": script_sha256,
        "execution": {
            "runner": "powershell",
            "single_session_command": "powershell -ExecutionPolicy Bypass -File .\\run-consumer-lifecycle.ps1",
            "ao2_argument": "-Ao2 <path-to-ao2.exe-or-ao2>",
            "output_argument": "-OutDir <windows-output-dir>"
        },
        "package": {
            "summary_path": copied_package_summary.display().to_string(),
            "summary_sha256": actual_package_summary_sha256,
            "archive_path": copied_package_archive.display().to_string(),
            "archive_sha256": actual_package_archive_sha256
        },
        "adapter_scaffold": {
            "source_summary_path": copied_adapter_scaffold.display().to_string(),
            "source_summary_sha256": actual_adapter_scaffold_sha256,
            "portable_summary_path": portable_adapter_summary.display().to_string(),
            "portable_summary_sha256": portable_adapter_summary_sha256
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
    let manifest_body = serde_json::to_string_pretty(&manifest)?;
    atomic_write_text(&manifest_path, &manifest_body)?;
    factory_app_run_bundle_reject_secret_markers(
        &manifest_path,
        "windows-consumer-lifecycle-recovery.json",
    )?;
    let manifest_sha256 = sha256_file(&manifest_path)?;

    let mut response = manifest;
    response["manifest_sha256"] = serde_json::json!(manifest_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_windows_execution");
        println!("schema_version=ao2.plugin-consumer-lifecycle-windows-recovery.v1");
        println!("manifest={}", manifest_path.display());
        println!("script={}", script_path.display());
    }
    Ok(())
}

pub(super) fn plugin_consumer_lifecycle_observer_bundle(
    options: PluginConsumerLifecycleObserverBundleOptions,
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
            options.macos_lifecycle,
            options.macos_sha256.trim().to_string(),
        ),
        (
            "ubuntu",
            options.ubuntu_lifecycle,
            options.ubuntu_sha256.trim().to_string(),
        ),
        (
            "windows",
            options.windows_lifecycle,
            options.windows_sha256.trim().to_string(),
        ),
    ];
    let mut platform_lifecycles = serde_json::Map::new();
    for (platform, source_path, supplied_sha256) in inputs {
        let actual_sha256 = sha256_file(&source_path)?;
        if actual_sha256 != supplied_sha256 {
            anyhow::bail!(
                "{platform} consumer lifecycle sha256 mismatch for {}: expected {}, actual {}",
                source_path.display(),
                supplied_sha256,
                actual_sha256
            );
        }
        factory_app_run_bundle_reject_secret_markers(
            &source_path,
            &format!("{platform} plugin-consumer-lifecycle.json"),
        )?;
        let lifecycle_text = fs::read_to_string(&source_path)
            .with_context(|| format!("read {}", source_path.display()))?;
        let lifecycle: serde_json::Value = serde_json::from_str(&lifecycle_text)
            .with_context(|| format!("parse {}", source_path.display()))?;
        validate_plugin_consumer_lifecycle_contract(&lifecycle, platform)?;

        let bundled_path = platforms_root
            .join(platform)
            .join("plugin-consumer-lifecycle.json");
        atomic_write_text(&bundled_path, &lifecycle_text)?;
        factory_app_run_bundle_reject_secret_markers(
            &bundled_path,
            &format!("{platform} bundled plugin-consumer-lifecycle.json"),
        )?;
        let bundled_sha256 = sha256_file(&bundled_path)?;
        if bundled_sha256 != actual_sha256 {
            anyhow::bail!(
                "{platform} consumer lifecycle changed while bundling: expected {}, bundled {}",
                actual_sha256,
                bundled_sha256
            );
        }

        platform_lifecycles.insert(
            platform.to_string(),
            serde_json::json!({
                "source_path": source_path.display().to_string(),
                "bundled_path": bundled_path.display().to_string(),
                "sha256": actual_sha256,
                "schema_version": json_string(&lifecycle, "schema_version"),
                "status": json_string(&lifecycle, "status"),
                "summary_path": json_string(&lifecycle, "summary_path"),
                "targets": lifecycle.get("targets").cloned().unwrap_or_else(|| serde_json::json!([])),
                "package": lifecycle.get("package").cloned().unwrap_or_else(|| serde_json::json!({})),
                "adapter_scaffold": lifecycle.get("adapter_scaffold").cloned().unwrap_or_else(|| serde_json::json!({})),
                "target_results": lifecycle.get("target_results").cloned().unwrap_or_else(|| serde_json::json!({})),
                "provider_auth": lifecycle.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
                "trust_boundary": lifecycle.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
                "control_plane_observation": lifecycle.get("control_plane_observation").cloned().unwrap_or_else(|| serde_json::json!({})),
                "side_effects": lifecycle.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
                "factory_v3_role": json_string(&lifecycle, "factory_v3_role")
            }),
        );
    }

    let archive_path = options
        .out_dir
        .join("k37-plugin-consumer-lifecycle-observer-bundle.tar.gz");
    create_tar_gz(&bundle_root, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    factory_app_run_bundle_reject_secret_markers(
        &archive_path,
        "k37-plugin-consumer-lifecycle-observer-bundle.tar.gz",
    )?;

    let summary_path = options
        .out_dir
        .join("k37-plugin-consumer-lifecycle-observer-bundle.json");
    let platform_lifecycles_value = serde_json::Value::Object(platform_lifecycles);
    let summary = serde_json::json!({
        "schema_version": "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "summary_path": summary_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
        "archive_sha256": archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": [
            "ao2.plugin-consumer-lifecycle.v1"
        ],
        "platform_lifecycles": platform_lifecycles_value,
        "platform_lifecycles_sha256": canonical_json_sha256(&platform_lifecycles_value),
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
        "k37-plugin-consumer-lifecycle-observer-bundle.json",
    )?;
    let summary_sha256 = sha256_file(&summary_path)?;

    let mut response = summary;
    response["summary_sha256"] = serde_json::json!(summary_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if options.json_output {
        println!("{response_body}");
    } else {
        println!("status=ready_for_k37_observation");
        println!("schema_version=ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1");
        println!("summary={}", summary_path.display());
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_consumer_lifecycle_observer_bundle_verify(
    options: PluginConsumerLifecycleObserverBundleVerifyOptions,
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
    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
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
        if json_string(summary_lifecycle, "schema_version") != "ao2.plugin-consumer-lifecycle.v1"
            || json_string(summary_lifecycle, "status") != "passed"
        {
            anyhow::bail!(
                "{platform} consumer lifecycle observer bundle summary lifecycle metadata is invalid"
            );
        }
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.k37-plugin-consumer-lifecycle-observer-bundle-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "platform_count": 3,
        "platforms": ["macos", "ubuntu", "windows"],
        "observed_evidence_scope": summary.get("observed_evidence_scope").cloned().unwrap_or_else(|| serde_json::json!([])),
        "archive_contents_verified": true,
        "bundled_lifecycles_verified": true,
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
        println!(
            "schema_version=ao2.k37-plugin-consumer-lifecycle-observer-bundle-verification.v1"
        );
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

fn run_plugin_consumer_lifecycle_target(
    target: &str,
    wrapper_sandbox_dir: &Path,
    installed_package_dir: &Path,
    package_summary_sha256: &str,
    package_archive_sha256: &str,
    installed_adapter_summary: &Path,
    installed_adapter_summary_sha256: &str,
) -> Result<serde_json::Value> {
    let manifest_dir = installed_package_dir.join("manifest");
    let evidence_dir = wrapper_sandbox_dir
        .join("evidence")
        .join("consumer-lifecycle");
    fs::create_dir_all(&evidence_dir)
        .with_context(|| format!("create {}", evidence_dir.display()))?;

    let installed_summary = installed_package_dir.join("ao2-plugin-package.json");
    let installed_archive = installed_package_dir.join("ao2-plugin-package.tar.gz");
    let package_verification = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "package-verify".to_string(),
        "--summary".to_string(),
        installed_summary.display().to_string(),
        "--summary-sha256".to_string(),
        package_summary_sha256.to_string(),
        "--archive".to_string(),
        installed_archive.display().to_string(),
        "--archive-sha256".to_string(),
        package_archive_sha256.to_string(),
        "--json".to_string(),
    ])?;
    let package_verification_path = evidence_dir.join("package-verification.json");
    atomic_write_text(
        &package_verification_path,
        &serde_json::to_string_pretty(&package_verification)?,
    )?;

    let readiness_path = evidence_dir.join("plugin-readiness.json");
    let readiness = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "readiness".to_string(),
        "--out".to_string(),
        readiness_path.display().to_string(),
        "--json".to_string(),
    ])?;
    validate_plugin_readiness_contract(&readiness)?;
    let readiness_sha256 = sha256_file(&readiness_path)?;

    let manifest_path = manifest_dir.join("ao2-plugin-manifest.json");
    let manifest_sha256 = sha256_file(&manifest_path)?;
    let manifest_verification = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "manifest-verify".to_string(),
        "--manifest-dir".to_string(),
        manifest_dir.display().to_string(),
        "--manifest-sha256".to_string(),
        manifest_sha256.clone(),
        "--json".to_string(),
    ])?;
    let manifest_verification_path = evidence_dir.join("manifest-verification.json");
    atomic_write_text(
        &manifest_verification_path,
        &serde_json::to_string_pretty(&manifest_verification)?,
    )?;
    let manifest_verification_sha256 = sha256_file(&manifest_verification_path)?;

    let install_smoke_path = evidence_dir.join("install-smoke.json");
    let install_smoke = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "install-smoke".to_string(),
        "--manifest-dir".to_string(),
        manifest_dir.display().to_string(),
        "--verification".to_string(),
        manifest_verification_path.display().to_string(),
        "--verification-sha256".to_string(),
        manifest_verification_sha256.clone(),
        "--out".to_string(),
        install_smoke_path.display().to_string(),
        "--json".to_string(),
    ])?;
    validate_plugin_install_smoke_contract(&install_smoke)?;
    let install_smoke_sha256 = sha256_file(&install_smoke_path)?;

    let adapter_scaffold_verification = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "adapter-scaffold-verify".to_string(),
        "--summary".to_string(),
        installed_adapter_summary.display().to_string(),
        "--summary-sha256".to_string(),
        installed_adapter_summary_sha256.to_string(),
        "--json".to_string(),
    ])?;
    let adapter_scaffold_verification_path =
        evidence_dir.join("adapter-scaffold-verification.json");
    atomic_write_text(
        &adapter_scaffold_verification_path,
        &serde_json::to_string_pretty(&adapter_scaffold_verification)?,
    )?;
    let adapter_scaffold_verification_sha256 = sha256_file(&adapter_scaffold_verification_path)?;

    let adapter_install_smoke_path = evidence_dir.join("adapter-install-smoke.json");
    let adapter_install_smoke = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "adapter-install-smoke".to_string(),
        "--summary".to_string(),
        installed_adapter_summary.display().to_string(),
        "--summary-sha256".to_string(),
        installed_adapter_summary_sha256.to_string(),
        "--out".to_string(),
        adapter_install_smoke_path.display().to_string(),
        "--json".to_string(),
    ])?;
    validate_plugin_adapter_install_smoke_contract(&adapter_install_smoke)?;
    let adapter_install_smoke_sha256 = sha256_file(&adapter_install_smoke_path)?;

    let adapter_install_smoke_verification = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "adapter-install-smoke-verify".to_string(),
        "--smoke".to_string(),
        adapter_install_smoke_path.display().to_string(),
        "--smoke-sha256".to_string(),
        adapter_install_smoke_sha256.clone(),
        "--json".to_string(),
    ])?;
    let adapter_install_smoke_verification_path =
        evidence_dir.join("adapter-install-smoke-verification.json");
    atomic_write_text(
        &adapter_install_smoke_verification_path,
        &serde_json::to_string_pretty(&adapter_install_smoke_verification)?,
    )?;
    let adapter_install_smoke_verification_sha256 =
        sha256_file(&adapter_install_smoke_verification_path)?;

    let wrapper_harness_dir = write_plugin_consumer_lifecycle_wrapper_fixture(
        target,
        &evidence_dir,
        &readiness_path,
        &readiness_sha256,
    )?;
    let wrapper_harness_sha256 =
        sha256_file(&wrapper_harness_dir.join("plugin-wrapper-harness.json"))?;
    let wrapper_harness_verification = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "wrapper-harness-verify".to_string(),
        "--evidence-dir".to_string(),
        wrapper_harness_dir.display().to_string(),
        "--summary-sha256".to_string(),
        wrapper_harness_sha256,
        "--json".to_string(),
    ])?;
    let wrapper_harness_verification_path = evidence_dir.join("wrapper-harness-verification.json");
    atomic_write_text(
        &wrapper_harness_verification_path,
        &serde_json::to_string_pretty(&wrapper_harness_verification)?,
    )?;
    let wrapper_harness_verification_sha256 = sha256_file(&wrapper_harness_verification_path)?;
    let package_verification_sha256 = sha256_file(&package_verification_path)?;

    Ok(serde_json::json!({
        "status": "passed",
        "target": target,
        "wrapper_sandbox_dir": wrapper_sandbox_dir.display().to_string(),
        "installed_package_dir": installed_package_dir.display().to_string(),
        "installed_package_paths_only": true,
        "manifest_dir": manifest_dir.display().to_string(),
        "manifest_sha256": manifest_sha256,
        "readiness_path": readiness_path.display().to_string(),
        "readiness_sha256": readiness_sha256,
        "manifest_verification_path": manifest_verification_path.display().to_string(),
        "manifest_verification_sha256": manifest_verification_sha256,
        "install_smoke_path": install_smoke_path.display().to_string(),
        "install_smoke_sha256": install_smoke_sha256,
        "package_verification_path": package_verification_path.display().to_string(),
        "package_verification_sha256": package_verification_sha256,
        "adapter_scaffold_summary_path": installed_adapter_summary.display().to_string(),
        "adapter_scaffold_summary_sha256": installed_adapter_summary_sha256,
        "adapter_scaffold_verification_path": adapter_scaffold_verification_path.display().to_string(),
        "adapter_scaffold_verification_sha256": adapter_scaffold_verification_sha256,
        "adapter_install_smoke_path": adapter_install_smoke_path.display().to_string(),
        "adapter_install_smoke_sha256": adapter_install_smoke_sha256,
        "adapter_install_smoke_verification_path": adapter_install_smoke_verification_path.display().to_string(),
        "adapter_install_smoke_verification_sha256": adapter_install_smoke_verification_sha256,
        "wrapper_harness_dir": wrapper_harness_dir.display().to_string(),
        "wrapper_harness_verification_path": wrapper_harness_verification_path.display().to_string(),
        "wrapper_harness_verification_sha256": wrapper_harness_verification_sha256,
        "provider_execution_started": false,
        "queue_mutated": false,
        "memory_written": false,
        "control_plane_mutated": false,
        "ao_artifacts_mutated": false,
        "release_approved": false,
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        }
    }))
}

fn write_plugin_consumer_installed_adapter_scaffold(
    source_summary: &serde_json::Value,
    adapter_root: &Path,
    installed_package_summary: &Path,
    installed_package_archive: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(adapter_root)
        .with_context(|| format!("create {}", adapter_root.display()))?;
    let mut installed_summary = source_summary.clone();
    let summary_path = adapter_root.join("plugin-adapter-scaffold.json");
    installed_summary["summary_path"] = serde_json::json!(summary_path.display().to_string());
    installed_summary["package"]["summary_path"] =
        serde_json::json!(installed_package_summary.display().to_string());
    installed_summary["package"]["archive_path"] =
        serde_json::json!(installed_package_archive.display().to_string());

    let source_adapter_files = source_summary
        .get("adapter_files")
        .and_then(serde_json::Value::as_object)
        .context("plugin adapter scaffold missing adapter_files")?;
    let mut installed_adapter_files = serde_json::Map::new();
    for target in ["codex", "claude"] {
        let entry = source_adapter_files
            .get(target)
            .with_context(|| format!("plugin adapter scaffold missing {target} adapter file"))?;
        let source_path = PathBuf::from(json_string(entry, "path"));
        let expected_sha256 = json_string(entry, "sha256");
        let actual_sha256 = sha256_file(&source_path)?;
        if expected_sha256 != actual_sha256 {
            anyhow::bail!(
                "plugin adapter scaffold {target} adapter sha256 mismatch: expected {expected_sha256}, actual {actual_sha256}"
            );
        }
        let mut adapter: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&source_path)
                .with_context(|| format!("read {}", source_path.display()))?,
        )
        .with_context(|| format!("parse {}", source_path.display()))?;
        adapter["inputs"]["package_summary_path"] =
            serde_json::json!(installed_package_summary.display().to_string());
        adapter["inputs"]["package_archive_path"] =
            serde_json::json!(installed_package_archive.display().to_string());

        let target_dir = adapter_root.join(target);
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("create {}", target_dir.display()))?;
        let installed_adapter_path = target_dir.join("ao2-plugin-adapter.json");
        atomic_write_text(
            &installed_adapter_path,
            &serde_json::to_string_pretty(&adapter)?,
        )?;
        factory_app_run_bundle_reject_secret_markers(
            &installed_adapter_path,
            &format!("{target} installed ao2-plugin-adapter.json"),
        )?;
        installed_adapter_files.insert(
            target.to_string(),
            serde_json::json!({
                "path": installed_adapter_path.display().to_string(),
                "sha256": sha256_file(&installed_adapter_path)?,
                "schema_version": "ao2.plugin-adapter.v1",
                "status": "ready_for_local_oauth_wrapper_integration"
            }),
        );
    }
    installed_summary["adapter_files"] = serde_json::Value::Object(installed_adapter_files);
    atomic_write_text(
        &summary_path,
        &serde_json::to_string_pretty(&installed_summary)?,
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "installed plugin-adapter-scaffold.json",
    )?;
    Ok(summary_path)
}

fn write_plugin_consumer_lifecycle_wrapper_fixture(
    target: &str,
    evidence_dir: &Path,
    readiness_path: &Path,
    readiness_sha256: &str,
) -> Result<PathBuf> {
    let wrapper_harness_dir = evidence_dir.join("wrapper-harness");
    fs::create_dir_all(&wrapper_harness_dir)
        .with_context(|| format!("create {}", wrapper_harness_dir.display()))?;
    let args_file = wrapper_harness_dir.join("wrapper-args.json");
    let args = serde_json::json!({
        "schema_version": "ao2.plugin-wrapper-args.v1",
        "run_kind": "app-run",
        "args": [
            "factory",
            "app-run",
            "--json"
        ]
    });
    atomic_write_text(&args_file, &serde_json::to_string_pretty(&args)?)?;
    let args_sha256 = sha256_file(&args_file)?;

    let stdout_path = wrapper_harness_dir.join("stdout.redacted.txt");
    let stderr_path = wrapper_harness_dir.join("stderr.redacted.txt");
    atomic_write_text(
        &stdout_path,
        &format!("Summary: {target} consumer lifecycle wrapper verification fixture\n"),
    )?;
    atomic_write_text(&stderr_path, "")?;

    let summary_path = wrapper_harness_dir.join("plugin-wrapper-harness.json");
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-wrapper-harness.v1",
        "status": "accepted",
        "run_kind": "app-run",
        "readiness_path": readiness_path.display().to_string(),
        "readiness_sha256": readiness_sha256,
        "args_file": args_file.display().to_string(),
        "args_sha256": args_sha256,
        "child_exit_code": 0,
        "exit_code_contract": {
            "success": 0,
            "runtime_error": 1,
            "cli_usage": 2,
            "enforced": true
        },
        "digest_gates": {
            "readiness_sha256_verified": true,
            "args_sha256_verified": true,
            "factory_command_digest_pinned_before_execution": true
        },
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "forbidden_provider_api_key_env_absent": true
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
        },
        "token_safe_output": {
            "stdout_redacted": true,
            "stderr_redacted": true,
            "redaction_class_counts": {}
        },
        "evidence": {
            "bundle_path": wrapper_harness_dir.display().to_string(),
            "summary": summary_path.display().to_string(),
            "stdout_redacted": stdout_path.display().to_string(),
            "stderr_redacted": stderr_path.display().to_string()
        },
        "ao2_artifacts": {},
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        }
    });
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    factory_app_run_bundle_reject_secret_markers(&summary_path, "plugin-wrapper-harness.json")?;
    Ok(wrapper_harness_dir)
}

fn write_plugin_installation_from_archive(
    archive_files: &BTreeMap<String, Vec<u8>>,
    installed_root: &Path,
    final_summary: &Path,
    archive: &Path,
) -> Result<()> {
    write_plugin_package_installation(archive_files, installed_root, final_summary, archive)
}
