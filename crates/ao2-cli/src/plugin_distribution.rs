use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;

use super::{
    atomic_write_text, copy_dir_recursive, create_tar_gz, ensure_rsa_private_key,
    factory_app_run_bundle_reject_secret_fields, factory_app_run_bundle_reject_secret_markers,
    factory_project_plan_init_app_step_repo, fail_if_provider_api_key_env_present, json_bool,
    json_string, read_json_file, run_current_ao2_json_command, sha256_bytes_hex, sha256_file,
    validate_plugin_readiness_contract, validate_plugin_wrapper_args,
    validate_plugin_wrapper_harness_summary, write_plugin_installation_from_archive,
    PluginWrapperArgsFile,
};

pub(super) struct PluginManifestVerifyOptions {
    pub(super) manifest_dir: PathBuf,
    pub(super) manifest_sha256: String,
    pub(super) json_output: bool,
}

pub(super) struct PluginInstallSmokeOptions {
    pub(super) manifest_dir: PathBuf,
    pub(super) verification: PathBuf,
    pub(super) verification_sha256: String,
    pub(super) out: Option<PathBuf>,
    pub(super) json_output: bool,
}

pub(super) struct PluginPackageOptions {
    pub(super) manifest_dir: PathBuf,
    pub(super) manifest_verification: PathBuf,
    pub(super) manifest_verification_sha256: String,
    pub(super) install_smoke: PathBuf,
    pub(super) install_smoke_sha256: String,
    pub(super) out_dir: PathBuf,
    pub(super) json_output: bool,
}

pub(super) struct PluginPackageVerifyOptions {
    pub(super) summary: PathBuf,
    pub(super) summary_sha256: String,
    pub(super) archive: PathBuf,
    pub(super) archive_sha256: String,
    pub(super) json_output: bool,
}

pub(super) struct PluginDistributionRehearsalOptions {
    pub(super) summary: PathBuf,
    pub(super) summary_sha256: String,
    pub(super) archive: PathBuf,
    pub(super) archive_sha256: String,
    pub(super) out_dir: PathBuf,
    pub(super) json_output: bool,
}

pub(super) fn plugin_readiness(out: Option<PathBuf>, json_output: bool) -> Result<()> {
    let readiness = plugin_readiness_value();

    let body = serde_json::to_string_pretty(&readiness)?;
    if let Some(out) = out {
        atomic_write_text(&out, &body)?;
    }

    if json_output {
        println!("{body}");
    } else {
        println!("status={}", json_string(&readiness, "status"));
        println!(
            "schema_version={}",
            json_string(&readiness, "schema_version")
        );
        println!("plugin_targets=codex,claude");
        println!("provider_auth=local-oauth-cli-only");
    }
    Ok(())
}

fn plugin_readiness_value() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.plugin-readiness.v1",
        "status": "accepted",
        "plugin_targets": ["codex", "claude"],
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false,
            "provider_auth": "local OAuth CLI only; provider key auth forbidden"
        },
        "stable_json": {
            "stdout_json_flag": true,
            "schema_version_required": true,
            "canonical_schema_field": "schema_version",
            "human_output_is_suppressed_when_json_flag_is_set": true
        },
        "exit_codes": {
            "success": 0,
            "runtime_error": 1,
            "cli_usage": 2
        },
        "digest_gated_inputs": {
            "required": true,
            "accepted_digest": "sha256",
            "surfaces": [
                "ao2 factory verify-handoff",
                "ao2 factory verify-run-result",
                "ao2 factory verify-planning-evidence",
                "ao2 factory verify-evaluator-decision",
                "ao2 factory replacement-packet-verify",
                "ao2 release evidence-bundle-verify",
                "ao2 contract check",
                "ao2 contract gate"
            ]
        },
        "token_safe_output": {
            "provider_api_key_auth_allowed": false,
            "bearer_tokens_serialized": false,
            "cookies_serialized": false,
            "private_keys_serialized": false,
            "redaction_policy": "paths_status_and_digests_only"
        },
        "durable_evidence_paths": {
            "required": true,
            "patterns": [
                ".ao2/runs/<run-id>/evidence-pack/",
                "target/factory-app-run-smoke/<timestamp>/app-run-evidence-bundle.tgz",
                "target/factory-greenfield-run/<timestamp>/",
                "target/factory-project-run/<timestamp>/",
                "target/morning-cross-os-readback/<timestamp>/"
            ]
        },
        "wrapper_contract": {
            "no_provider_process_started_by_readiness": true,
            "no_queue_mutation": true,
            "no_memory_write": true,
            "no_ao_artifact_mutation": true,
            "readiness_artifact_is_optional": true
        }
    })
}

pub(super) fn plugin_manifest(out_dir: PathBuf, json_output: bool) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let readiness_path = out_dir.join("examples/plugin-readiness.example.json");
    let app_args_path = out_dir.join("examples/plugin-wrapper-args.app-run.example.json");
    let project_args_path = out_dir.join("examples/plugin-wrapper-args.project-run.example.json");
    let app_spec_path = out_dir.join("examples/app-spec.md");
    let project_spec_path = out_dir.join("examples/project-spec.md");
    let provider_script_path = out_dir.join("smoke/provider-script.sh");
    let signing_key_generator_path = out_dir.join("smoke/generate-signing-key.sh");
    let signing_key_generator_ps1_path = out_dir.join("smoke/generate-signing-key.ps1");
    let app_target_placeholder_path = out_dir.join("target/ao2-plugin-app/.keep");
    let codex_smoke_path = out_dir.join("smoke/codex-local-oauth-smoke.json");
    let claude_smoke_path = out_dir.join("smoke/claude-local-oauth-smoke.json");
    let install_path = out_dir.join("plugin.json");
    let readme_path = out_dir.join("README.md");
    let manifest_path = out_dir.join("ao2-plugin-manifest.json");

    let readiness = plugin_readiness_value();
    atomic_write_text(&readiness_path, &serde_json::to_string_pretty(&readiness)?)?;
    atomic_write_text(&app_spec_path, &plugin_manifest_app_spec())?;
    atomic_write_text(&project_spec_path, &plugin_manifest_project_spec())?;
    atomic_write_text(&provider_script_path, &plugin_manifest_provider_script())?;
    atomic_write_text(
        &signing_key_generator_path,
        &plugin_manifest_signing_key_generator_sh(),
    )?;
    atomic_write_text(
        &signing_key_generator_ps1_path,
        &plugin_manifest_signing_key_generator_ps1(),
    )?;
    atomic_write_text(&app_target_placeholder_path, "packaged sample app target\n")?;
    factory_project_plan_init_app_step_repo(
        app_target_placeholder_path
            .parent()
            .context("resolve plugin sample app target")?,
    )?;

    let app_args = serde_json::json!({
        "schema_version": "ao2.plugin-wrapper-args.v1",
        "run_kind": "app-run",
        "args": [
            "factory",
            "app-run",
            "--spec",
            "examples/app-spec.md",
            "--target",
            "target/ao2-plugin-app",
            "--run-id",
            "ao2-plugin-app-run",
            "--provider",
            "scripted",
            "--provider-prompt-file",
            "smoke/provider-script.sh",
            "--verifier-command",
            "python -m pytest -q",
            "--signing-key",
            "smoke/signing-key.pem",
            "--signer-id",
            "ao2-plugin-smoke",
            "--out-dir",
            "target/ao2-plugin-app-run",
            "--json"
        ]
    });
    atomic_write_text(&app_args_path, &serde_json::to_string_pretty(&app_args)?)?;

    let project_args = serde_json::json!({
        "schema_version": "ao2.plugin-wrapper-args.v1",
        "run_kind": "project-run",
        "args": [
            "factory",
            "project-run",
            "--project-spec",
            "examples/project-spec.md",
            "--app-run",
            "target/ao2-plugin-app-run/ao2-plugin-app-run-factory-app-run.json",
            "--run-id",
            "ao2-plugin-project-run",
            "--signing-key",
            "smoke/signing-key.pem",
            "--signer-id",
            "ao2-plugin-smoke",
            "--out-dir",
            "target/ao2-plugin-project-run",
            "--json"
        ]
    });
    atomic_write_text(
        &project_args_path,
        &serde_json::to_string_pretty(&project_args)?,
    )?;

    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "factory_v3_role": "parity_auditor",
        "control_plane_role": "read_only_observer",
        "mutates_ao_artifacts": false,
        "control_plane_approves_release": false
    });
    let smoke_fixture = |provider: &str| {
        serde_json::json!({
            "schema_version": "ao2.plugin-local-oauth-smoke.v1",
            "provider": provider,
            "auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "forbidden_provider_api_key_env_absent": true
            },
            "commands": {
                "readiness": "ao2 plugin readiness --json",
                "wrapper_harness_verify": "ao2 plugin wrapper-harness-verify --evidence-dir <dir> --summary-sha256 <sha256> --json"
            },
            "trust_boundary": trust_boundary
        })
    };
    atomic_write_text(
        &codex_smoke_path,
        &serde_json::to_string_pretty(&smoke_fixture("codex"))?,
    )?;
    atomic_write_text(
        &claude_smoke_path,
        &serde_json::to_string_pretty(&smoke_fixture("claude"))?,
    )?;

    let relative_file = |path: &Path| -> String {
        path.strip_prefix(&out_dir)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    let file_entries = vec![
        ("readiness_example", readiness_path.clone()),
        ("app_run_args_example", app_args_path.clone()),
        ("project_run_args_example", project_args_path.clone()),
        ("app_spec_example", app_spec_path.clone()),
        ("project_spec_example", project_spec_path.clone()),
        ("provider_script", provider_script_path.clone()),
        ("signing_key_generator", signing_key_generator_path.clone()),
        (
            "signing_key_generator_ps1",
            signing_key_generator_ps1_path.clone(),
        ),
        (
            "app_target_placeholder",
            app_target_placeholder_path.clone(),
        ),
        ("codex_local_oauth_smoke", codex_smoke_path.clone()),
        ("claude_local_oauth_smoke", claude_smoke_path.clone()),
    ];
    let mut files = serde_json::Map::new();
    for (name, path) in &file_entries {
        files.insert(
            (*name).to_string(),
            serde_json::json!({
                "path": relative_file(path),
                "sha256": sha256_file(path)?
            }),
        );
    }

    let install = serde_json::json!({
        "schema_version": "ao2.codex-claude-plugin-install.v1",
        "name": "ao2-governed-execution",
        "targets": ["codex", "claude"],
        "commands": {
            "readiness": "ao2 plugin readiness --json",
            "manifest_verify": "ao2 plugin manifest-verify --manifest-dir <dir> --manifest-sha256 <sha256> --json",
            "install_smoke": "ao2 plugin install-smoke --manifest-dir <dir> --verification <path> --verification-sha256 <sha256> --json",
            "package": "ao2 plugin package --manifest-dir <dir> --manifest-verification <path> --manifest-verification-sha256 <sha256> --install-smoke <path> --install-smoke-sha256 <sha256> --out-dir <dir> --json",
            "package_verify": "ao2 plugin package-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
            "distribution_rehearsal": "ao2 plugin distribution-rehearsal --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
            "consumer_lifecycle": "ao2 plugin consumer-lifecycle --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --adapter-scaffold <path> --adapter-scaffold-sha256 <sha256> --out-dir <dir> --json",
            "consumer_lifecycle_windows_recovery": "ao2 plugin consumer-lifecycle-windows-recovery --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --adapter-scaffold <path> --adapter-scaffold-sha256 <sha256> --out-dir <dir> --json",
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
            "distribution_observer_bundle": "ao2 plugin distribution-observer-bundle --macos-observer <path> --macos-sha256 <sha256> --ubuntu-observer <path> --ubuntu-sha256 <sha256> --windows-observer <path> --windows-sha256 <sha256> --out-dir <dir> --json",
            "clean_package_operator_index": "ao2 plugin clean-package-operator-index --macos-rehearsal <path> --macos-sha256 <sha256> --ubuntu-rehearsal <path> --ubuntu-sha256 <sha256> --windows-rehearsal <path> --windows-sha256 <sha256> --out-dir <dir> --json",
            "packaged_replacement_observer_bundle": "ao2 plugin packaged-replacement-observer-bundle --macos-proof <path> --macos-sha256 <sha256> --ubuntu-proof <path> --ubuntu-sha256 <sha256> --windows-proof <path> --windows-sha256 <sha256> --out-dir <dir> --json",
            "packaged_replacement_observer_bundle_verify": "ao2 plugin packaged-replacement-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
            "adapter_scaffold": "ao2 plugin adapter-scaffold --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --k37-bundle <path> --k37-bundle-sha256 <sha256> --k37-archive <path> --k37-archive-sha256 <sha256> --out-dir <dir> --json",
            "adapter_scaffold_verify": "ao2 plugin adapter-scaffold-verify --summary <path> --summary-sha256 <sha256> --json",
            "adapter_install_smoke": "ao2 plugin adapter-install-smoke --summary <path> --summary-sha256 <sha256> --out <path> --json",
            "adapter_install_smoke_verify": "ao2 plugin adapter-install-smoke-verify --smoke <path> --smoke-sha256 <sha256> --json",
            "adapter_install_smoke_observer_bundle": "ao2 plugin adapter-install-smoke-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
            "adapter_observer_bundle": "ao2 plugin adapter-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
            "wrapper_harness": "ao2 plugin wrapper-harness --readiness <path> --readiness-sha256 <sha256> --args-file <path> --args-sha256 <sha256> --run-kind <app-run|project-run> --out-dir <dir> --json",
            "wrapper_harness_verify": "ao2 plugin wrapper-harness-verify --evidence-dir <dir> --summary-sha256 <sha256> --json"
        },
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false
        },
        "trust_boundary": trust_boundary
    });
    atomic_write_text(&install_path, &serde_json::to_string_pretty(&install)?)?;
    files.insert(
        "install_manifest".to_string(),
        serde_json::json!({
            "path": relative_file(&install_path),
            "sha256": sha256_file(&install_path)?
        }),
    );

    let readme = plugin_manifest_consumer_readme();
    atomic_write_text(&readme_path, &readme)?;
    files.insert(
        "consumer_readme".to_string(),
        serde_json::json!({
            "path": relative_file(&readme_path),
            "sha256": sha256_file(&readme_path)?
        }),
    );

    let manifest = serde_json::json!({
        "schema_version": "ao2.plugin-manifest.v1",
        "status": "packaged",
        "plugin_targets": ["codex", "claude"],
        "out_dir": out_dir.display().to_string(),
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "provider_api_key_env_required": false
        },
        "entrypoints": {
            "readiness": "ao2 plugin readiness --json",
            "manifest_verify": "ao2 plugin manifest-verify",
            "install_smoke": "ao2 plugin install-smoke",
            "package": "ao2 plugin package",
            "package_verify": "ao2 plugin package-verify",
            "distribution_rehearsal": "ao2 plugin distribution-rehearsal",
            "consumer_lifecycle": "ao2 plugin consumer-lifecycle",
            "consumer_lifecycle_windows_recovery": "ao2 plugin consumer-lifecycle-windows-recovery",
            "consumer_lifecycle_observer_bundle": "ao2 plugin consumer-lifecycle-observer-bundle",
            "consumer_lifecycle_observer_bundle_verify": "ao2 plugin consumer-lifecycle-observer-bundle-verify",
            "control_plane_fixture_handoff": "ao2 plugin control-plane-fixture-handoff",
            "control_plane_fixture_handoff_verify": "ao2 plugin control-plane-fixture-handoff-verify",
            "release_candidate": "ao2 plugin release-candidate",
            "release_candidate_verify": "ao2 plugin release-candidate-verify",
            "release_candidate_windows_recovery": "ao2 plugin release-candidate-windows-recovery",
            "release_candidate_windows_recovery_verify": "ao2 plugin release-candidate-windows-recovery-verify",
            "release_candidate_windows_transfer_bundle": "ao2 plugin release-candidate-windows-transfer-bundle",
            "release_candidate_observer_bundle": "ao2 plugin release-candidate-observer-bundle",
            "release_candidate_observer_bundle_verify": "ao2 plugin release-candidate-observer-bundle-verify",
            "release_candidate_control_plane_fixture_handoff": "ao2 plugin release-candidate-control-plane-fixture-handoff",
            "release_candidate_control_plane_fixture_handoff_verify": "ao2 plugin release-candidate-control-plane-fixture-handoff-verify",
            "final_install_transcript": "ao2 plugin final-install-transcript",
            "final_install_transcript_observer_bundle": "ao2 plugin final-install-transcript-observer-bundle",
            "closer_decision": "ao2 factory closer-decision",
            "closer_decision_verify": "ao2 factory closer-decision-verify",
            "shipment_readiness": "ao2 plugin shipment-readiness",
            "distribution_observer_bundle": "ao2 plugin distribution-observer-bundle",
            "clean_package_operator_index": "ao2 plugin clean-package-operator-index",
            "packaged_replacement_observer_bundle": "ao2 plugin packaged-replacement-observer-bundle",
            "packaged_replacement_observer_bundle_verify": "ao2 plugin packaged-replacement-observer-bundle-verify",
            "adapter_scaffold": "ao2 plugin adapter-scaffold",
            "adapter_scaffold_verify": "ao2 plugin adapter-scaffold-verify",
            "adapter_install_smoke": "ao2 plugin adapter-install-smoke",
            "adapter_install_smoke_verify": "ao2 plugin adapter-install-smoke-verify",
            "adapter_install_smoke_observer_bundle": "ao2 plugin adapter-install-smoke-observer-bundle",
            "adapter_observer_bundle": "ao2 plugin adapter-observer-bundle",
            "wrapper_harness": "ao2 plugin wrapper-harness",
            "wrapper_harness_verify": "ao2 plugin wrapper-harness-verify"
        },
        "schema_examples": {
            "readiness": relative_file(&readiness_path),
            "app_run_args": relative_file(&app_args_path),
            "project_run_args": relative_file(&project_args_path),
            "consumer_readme": relative_file(&readme_path)
        },
        "smoke_fixtures": {
            "codex": relative_file(&codex_smoke_path),
            "claude": relative_file(&claude_smoke_path)
        },
        "files": files,
        "digest_gates": {
            "manifest_files_sha256_verified": true,
            "wrapper_inputs_must_be_sha256_pinned": true
        },
        "trust_boundary": trust_boundary,
        "token_safe_output": {
            "redaction_policy": "paths_status_and_digests_only",
            "bearer_tokens_serialized": false,
            "cookies_serialized": false,
            "private_keys_serialized": false
        }
    });
    let manifest_body = serde_json::to_string_pretty(&manifest)?;
    atomic_write_text(&manifest_path, &manifest_body)?;

    if json_output {
        println!("{manifest_body}");
    } else {
        println!("status=packaged");
        println!("schema_version=ao2.plugin-manifest.v1");
        println!("manifest={}", manifest_path.display());
    }
    Ok(())
}

pub(super) fn plugin_manifest_verify(options: PluginManifestVerifyOptions) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let manifest_path = options.manifest_dir.join("ao2-plugin-manifest.json");
    let supplied_manifest_sha256 = options.manifest_sha256.trim();
    let actual_manifest_sha256 = sha256_file(&manifest_path)?;
    if supplied_manifest_sha256 != actual_manifest_sha256 {
        anyhow::bail!(
            "plugin manifest sha256 mismatch for {}: expected {}, actual {}",
            manifest_path.display(),
            supplied_manifest_sha256,
            actual_manifest_sha256
        );
    }

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("parse {}", manifest_path.display()))?;
    validate_plugin_manifest_contract(&manifest)?;

    let files = manifest
        .get("files")
        .and_then(serde_json::Value::as_object)
        .context("plugin manifest missing files")?;
    for (name, entry) in files {
        let path = plugin_manifest_package_file_path(&options.manifest_dir, name, entry)?;
        let expected_sha256 = json_string(entry, "sha256");
        let actual_sha256 = sha256_file(&path)?;
        if expected_sha256 != actual_sha256 {
            anyhow::bail!(
                "plugin manifest file sha256 mismatch for {name}: expected {expected_sha256}, actual {actual_sha256}"
            );
        }
        factory_app_run_bundle_reject_secret_markers(&path, &json_string(entry, "path"))?;
    }

    let readiness_path =
        plugin_manifest_named_file_path(&options.manifest_dir, files, "readiness_example")?;
    let readiness: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&readiness_path)
            .with_context(|| format!("read {}", readiness_path.display()))?,
    )
    .with_context(|| format!("parse {}", readiness_path.display()))?;
    validate_plugin_readiness_contract(&readiness)?;

    let app_args_path =
        plugin_manifest_named_file_path(&options.manifest_dir, files, "app_run_args_example")?;
    let app_args: PluginWrapperArgsFile = serde_json::from_str(
        &fs::read_to_string(&app_args_path)
            .with_context(|| format!("read {}", app_args_path.display()))?,
    )
    .with_context(|| format!("parse {}", app_args_path.display()))?;
    validate_plugin_wrapper_args(&app_args, "app-run")?;

    let project_args_path =
        plugin_manifest_named_file_path(&options.manifest_dir, files, "project_run_args_example")?;
    let project_args: PluginWrapperArgsFile = serde_json::from_str(
        &fs::read_to_string(&project_args_path)
            .with_context(|| format!("read {}", project_args_path.display()))?,
    )
    .with_context(|| format!("parse {}", project_args_path.display()))?;
    validate_plugin_wrapper_args(&project_args, "project-run")?;

    for (name, provider) in [
        ("codex_local_oauth_smoke", "codex"),
        ("claude_local_oauth_smoke", "claude"),
    ] {
        let path = plugin_manifest_named_file_path(&options.manifest_dir, files, name)?;
        let smoke: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;
        validate_plugin_local_oauth_smoke(&smoke, provider)?;
    }

    let install_path =
        plugin_manifest_named_file_path(&options.manifest_dir, files, "install_manifest")?;
    let install: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&install_path)
            .with_context(|| format!("read {}", install_path.display()))?,
    )
    .with_context(|| format!("parse {}", install_path.display()))?;
    validate_plugin_install_manifest(&install)?;

    let readme_path =
        plugin_manifest_named_file_path(&options.manifest_dir, files, "consumer_readme")?;
    let readme = fs::read_to_string(&readme_path)
        .with_context(|| format!("read {}", readme_path.display()))?;
    validate_plugin_manifest_consumer_readme(&readme)?;
    factory_app_run_bundle_reject_secret_markers(&readme_path, "README.md")?;

    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-manifest-verification.v1",
        "status": "passed",
        "manifest_dir": options.manifest_dir.display().to_string(),
        "manifest_path": manifest_path.display().to_string(),
        "manifest_sha256": actual_manifest_sha256,
        "file_count": files.len(),
        "file_digests_verified": true,
        "readiness_contract_verified": true,
        "wrapper_args_verified": true,
        "local_oauth_smokes_verified": true,
        "install_manifest_verified": true,
        "consumer_readme_verified": true,
        "provider_auth": manifest["provider_auth"].clone(),
        "trust_boundary": manifest["trust_boundary"].clone(),
        "token_safe_output_verified": true,
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-manifest-verification.v1");
        println!("manifest_sha256={actual_manifest_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_install_smoke(options: PluginInstallSmokeOptions) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_verification_sha256 = options.verification_sha256.trim();
    let actual_verification_sha256 = sha256_file(&options.verification)?;
    if supplied_verification_sha256 != actual_verification_sha256 {
        anyhow::bail!(
            "plugin manifest verification sha256 mismatch for {}: expected {}, actual {}",
            options.verification.display(),
            supplied_verification_sha256,
            actual_verification_sha256
        );
    }

    let verification: serde_json::Value = read_json_file(&options.verification)?;
    validate_plugin_manifest_verification_contract(&verification)?;

    let manifest_path = options.manifest_dir.join("ao2-plugin-manifest.json");
    let manifest_sha256 = sha256_file(&manifest_path)?;
    if manifest_sha256 != json_string(&verification, "manifest_sha256") {
        anyhow::bail!(
            "plugin install smoke manifest sha256 mismatch for {}: verification {}, actual {}",
            manifest_path.display(),
            json_string(&verification, "manifest_sha256"),
            manifest_sha256
        );
    }

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("parse {}", manifest_path.display()))?;
    validate_plugin_manifest_contract(&manifest)?;
    let files = manifest
        .get("files")
        .and_then(serde_json::Value::as_object)
        .context("plugin manifest missing files")?;
    let install_path =
        plugin_manifest_named_file_path(&options.manifest_dir, files, "install_manifest")?;
    let install: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&install_path)
            .with_context(|| format!("read {}", install_path.display()))?,
    )
    .with_context(|| format!("parse {}", install_path.display()))?;
    validate_plugin_install_manifest(&install)?;
    factory_app_run_bundle_reject_secret_markers(&manifest_path, "ao2-plugin-manifest.json")?;
    factory_app_run_bundle_reject_secret_markers(&install_path, "plugin.json")?;

    let commands = install
        .get("commands")
        .context("plugin install manifest missing commands")?;
    let smoke = serde_json::json!({
        "schema_version": "ao2.plugin-install-smoke.v1",
        "status": "passed",
        "manifest_dir": options.manifest_dir.display().to_string(),
        "manifest_sha256": manifest_sha256,
        "manifest_verification_path": options.verification.display().to_string(),
        "manifest_verification_sha256": actual_verification_sha256,
        "plugin_targets": ["codex", "claude"],
        "provider_auth": install["provider_auth"].clone(),
        "trust_boundary": install["trust_boundary"].clone(),
        "digest_gates": {
            "manifest_verification_sha256_verified": true,
            "manifest_sha256_verified": true,
            "manifest_files_sha256_verified": true,
            "wrapper_inputs_must_be_sha256_pinned": true
        },
        "install_commands_verified": {
            "readiness": !json_string(commands, "readiness").is_empty(),
            "manifest_verify": !json_string(commands, "manifest_verify").is_empty(),
            "install_smoke": !json_string(commands, "install_smoke").is_empty(),
            "package": !json_string(commands, "package").is_empty(),
            "package_verify": !json_string(commands, "package_verify").is_empty(),
            "distribution_rehearsal": !json_string(commands, "distribution_rehearsal").is_empty(),
            "consumer_lifecycle": !json_string(commands, "consumer_lifecycle").is_empty(),
            "consumer_lifecycle_windows_recovery": !json_string(commands, "consumer_lifecycle_windows_recovery").is_empty(),
            "consumer_lifecycle_observer_bundle": !json_string(commands, "consumer_lifecycle_observer_bundle").is_empty(),
            "consumer_lifecycle_observer_bundle_verify": !json_string(commands, "consumer_lifecycle_observer_bundle_verify").is_empty(),
            "control_plane_fixture_handoff": !json_string(commands, "control_plane_fixture_handoff").is_empty(),
            "control_plane_fixture_handoff_verify": !json_string(commands, "control_plane_fixture_handoff_verify").is_empty(),
            "release_candidate": !json_string(commands, "release_candidate").is_empty(),
            "release_candidate_verify": !json_string(commands, "release_candidate_verify").is_empty(),
            "release_candidate_windows_recovery": !json_string(commands, "release_candidate_windows_recovery").is_empty(),
            "release_candidate_windows_recovery_verify": !json_string(commands, "release_candidate_windows_recovery_verify").is_empty(),
            "release_candidate_windows_transfer_bundle": !json_string(commands, "release_candidate_windows_transfer_bundle").is_empty(),
            "release_candidate_observer_bundle": !json_string(commands, "release_candidate_observer_bundle").is_empty(),
            "release_candidate_observer_bundle_verify": !json_string(commands, "release_candidate_observer_bundle_verify").is_empty(),
            "release_candidate_control_plane_fixture_handoff": !json_string(commands, "release_candidate_control_plane_fixture_handoff").is_empty(),
            "release_candidate_control_plane_fixture_handoff_verify": !json_string(commands, "release_candidate_control_plane_fixture_handoff_verify").is_empty(),
            "final_install_transcript": !json_string(commands, "final_install_transcript").is_empty(),
            "final_install_transcript_observer_bundle": !json_string(commands, "final_install_transcript_observer_bundle").is_empty(),
            "closer_decision": !json_string(commands, "closer_decision").is_empty(),
            "closer_decision_verify": !json_string(commands, "closer_decision_verify").is_empty(),
            "shipment_readiness": !json_string(commands, "shipment_readiness").is_empty(),
            "distribution_observer_bundle": !json_string(commands, "distribution_observer_bundle").is_empty(),
            "adapter_scaffold": !json_string(commands, "adapter_scaffold").is_empty(),
            "adapter_scaffold_verify": !json_string(commands, "adapter_scaffold_verify").is_empty(),
            "adapter_install_smoke": !json_string(commands, "adapter_install_smoke").is_empty(),
            "adapter_install_smoke_verify": !json_string(commands, "adapter_install_smoke_verify").is_empty(),
            "adapter_install_smoke_observer_bundle": !json_string(commands, "adapter_install_smoke_observer_bundle").is_empty(),
            "adapter_observer_bundle": !json_string(commands, "adapter_observer_bundle").is_empty(),
            "wrapper_harness": !json_string(commands, "wrapper_harness").is_empty(),
            "wrapper_harness_verify": !json_string(commands, "wrapper_harness_verify").is_empty()
        },
        "dry_run": {
            "provider_execution_started": false,
            "queue_mutated": false,
            "memory_written": false,
            "ao_artifacts_mutated": false,
            "release_approved": false,
            "control_plane_mutated": false
        },
        "token_safe_output": {
            "redaction_policy": "paths_status_and_digests_only",
            "bearer_tokens_serialized": false,
            "cookies_serialized": false,
            "private_keys_serialized": false
        },
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&smoke)?;
    if let Some(out) = options.out {
        atomic_write_text(&out, &body)?;
    }
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-install-smoke.v1");
        println!("manifest_sha256={manifest_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_package(options: PluginPackageOptions) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let manifest_path = options.manifest_dir.join("ao2-plugin-manifest.json");
    let manifest_sha256 = sha256_file(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("parse {}", manifest_path.display()))?;
    validate_plugin_manifest_contract(&manifest)?;

    let supplied_verification_sha256 = options.manifest_verification_sha256.trim();
    let actual_verification_sha256 = sha256_file(&options.manifest_verification)?;
    if supplied_verification_sha256 != actual_verification_sha256 {
        anyhow::bail!(
            "plugin manifest verification sha256 mismatch for {}: expected {}, actual {}",
            options.manifest_verification.display(),
            supplied_verification_sha256,
            actual_verification_sha256
        );
    }
    let verification: serde_json::Value = read_json_file(&options.manifest_verification)?;
    validate_plugin_manifest_verification_contract(&verification)?;
    if json_string(&verification, "manifest_sha256") != manifest_sha256 {
        anyhow::bail!(
            "plugin package manifest sha256 mismatch: verification {}, actual {}",
            json_string(&verification, "manifest_sha256"),
            manifest_sha256
        );
    }

    let supplied_install_smoke_sha256 = options.install_smoke_sha256.trim();
    let actual_install_smoke_sha256 = sha256_file(&options.install_smoke)?;
    if supplied_install_smoke_sha256 != actual_install_smoke_sha256 {
        anyhow::bail!(
            "plugin install smoke sha256 mismatch for {}: expected {}, actual {}",
            options.install_smoke.display(),
            supplied_install_smoke_sha256,
            actual_install_smoke_sha256
        );
    }
    let install_smoke: serde_json::Value = read_json_file(&options.install_smoke)?;
    validate_plugin_install_smoke_contract(&install_smoke)?;
    if json_string(&install_smoke, "manifest_sha256") != manifest_sha256 {
        anyhow::bail!(
            "plugin package install smoke manifest sha256 mismatch: smoke {}, actual {}",
            json_string(&install_smoke, "manifest_sha256"),
            manifest_sha256
        );
    }
    if json_string(&install_smoke, "manifest_verification_sha256") != actual_verification_sha256 {
        anyhow::bail!(
            "plugin package install smoke verification sha256 mismatch: smoke {}, actual {}",
            json_string(&install_smoke, "manifest_verification_sha256"),
            actual_verification_sha256
        );
    }

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let stage_dir = options.out_dir.join(".ao2-plugin-package-stage");
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove {}", stage_dir.display()))?;
    }
    fs::create_dir_all(&stage_dir).with_context(|| format!("create {}", stage_dir.display()))?;

    let manifest_stage = stage_dir.join("manifest");
    copy_dir_recursive(&options.manifest_dir, &manifest_stage)?;
    let evidence_stage = stage_dir.join("evidence");
    fs::create_dir_all(&evidence_stage)
        .with_context(|| format!("create {}", evidence_stage.display()))?;
    fs::copy(
        &options.manifest_verification,
        evidence_stage.join("manifest-verification.json"),
    )
    .with_context(|| {
        format!(
            "copy {} to {}",
            options.manifest_verification.display(),
            evidence_stage.join("manifest-verification.json").display()
        )
    })?;
    fs::copy(
        &options.install_smoke,
        evidence_stage.join("install-smoke.json"),
    )
    .with_context(|| {
        format!(
            "copy {} to {}",
            options.install_smoke.display(),
            evidence_stage.join("install-smoke.json").display()
        )
    })?;

    factory_app_run_bundle_reject_secret_markers(&manifest_path, "ao2-plugin-manifest.json")?;
    factory_app_run_bundle_reject_secret_markers(
        &options.manifest_verification,
        "manifest-verification.json",
    )?;
    factory_app_run_bundle_reject_secret_markers(&options.install_smoke, "install-smoke.json")?;

    let archive_path = options.out_dir.join("ao2-plugin-package.tar.gz");
    let summary_stage_path = stage_dir.join("ao2-plugin-package.json");
    let summary_path = options.out_dir.join("ao2-plugin-package.json");
    let archive_placeholder = serde_json::json!({
        "path": archive_path.display().to_string(),
        "sha256": null
    });
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-package.v1",
        "status": "packaged",
        "plugin_targets": ["codex", "claude"],
        "manifest_dir": options.manifest_dir.display().to_string(),
        "manifest_sha256": manifest_sha256,
        "manifest_verification_path": options.manifest_verification.display().to_string(),
        "manifest_verification_sha256": actual_verification_sha256,
        "install_smoke_path": options.install_smoke.display().to_string(),
        "install_smoke_sha256": actual_install_smoke_sha256,
        "summary_path": summary_path.display().to_string(),
        "archive": archive_placeholder,
        "provider_auth": manifest["provider_auth"].clone(),
        "trust_boundary": manifest["trust_boundary"].clone(),
        "digest_gates": {
            "manifest_sha256_verified": true,
            "manifest_verification_sha256_verified": true,
            "install_smoke_sha256_verified": true,
            "manifest_files_sha256_verified": true,
            "wrapper_inputs_must_be_sha256_pinned": true
        },
        "package_contents": {
            "manifest_dir": "manifest/",
            "manifest_verification": "evidence/manifest-verification.json",
            "install_smoke": "evidence/install-smoke.json",
            "summary": "ao2-plugin-package.json"
        },
        "token_safe_output": {
            "redaction_policy": "paths_status_and_digests_only",
            "bearer_tokens_serialized": false,
            "cookies_serialized": false,
            "private_keys_serialized": false
        },
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "factory_v3_role": "parity_auditor"
    });
    atomic_write_text(
        &summary_stage_path,
        &serde_json::to_string_pretty(&summary)?,
    )?;
    create_tar_gz(&stage_dir, &archive_path)?;
    let archive_sha256 = sha256_file(&archive_path)?;
    let mut final_summary = summary;
    final_summary["archive"]["sha256"] = serde_json::json!(archive_sha256);
    let final_body = serde_json::to_string_pretty(&final_summary)?;
    atomic_write_text(&summary_path, &final_body)?;

    if options.json_output {
        println!("{final_body}");
    } else {
        println!("status=packaged");
        println!("schema_version=ao2.plugin-package.v1");
        println!("archive={}", archive_path.display());
    }
    Ok(())
}

pub(super) fn plugin_package_verify(options: PluginPackageVerifyOptions) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "plugin package summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "plugin package archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_package_contract(&summary)?;
    if json_string(&summary["archive"], "sha256") != actual_archive_sha256 {
        anyhow::bail!(
            "plugin package summary archive sha256 mismatch: summary {}, actual {}",
            json_string(&summary["archive"], "sha256"),
            actual_archive_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(&options.summary, "ao2-plugin-package.json")?;

    let archive_files = read_plugin_package_archive_files(&options.archive)?;
    let embedded_summary = plugin_package_archive_json(
        &archive_files,
        "ao2-plugin-package.json",
        "embedded plugin package summary",
    )?;
    let mut expected_embedded_summary = summary.clone();
    expected_embedded_summary["archive"]["sha256"] = serde_json::Value::Null;
    if embedded_summary != expected_embedded_summary {
        anyhow::bail!("plugin package embedded summary does not match external summary");
    }
    validate_plugin_package_contract_allowing_pending_archive_sha(&embedded_summary)?;

    let manifest = plugin_package_archive_json(
        &archive_files,
        "manifest/ao2-plugin-manifest.json",
        "embedded plugin manifest",
    )?;
    validate_plugin_manifest_contract(&manifest)?;
    let manifest_sha256 = sha256_archive_file(&archive_files, "manifest/ao2-plugin-manifest.json")?;
    if manifest_sha256 != json_string(&summary, "manifest_sha256") {
        anyhow::bail!(
            "plugin package embedded manifest sha256 mismatch: summary {}, actual {}",
            json_string(&summary, "manifest_sha256"),
            manifest_sha256
        );
    }

    let files = manifest
        .get("files")
        .and_then(serde_json::Value::as_object)
        .context("plugin package embedded manifest missing files")?;
    for (name, entry) in files {
        let relative_path = json_string(entry, "path");
        if relative_path.is_empty() {
            anyhow::bail!("plugin package embedded manifest file {name} missing path");
        }
        if Path::new(&relative_path)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            anyhow::bail!("plugin package embedded manifest file {name} path is unsafe");
        }
        let archive_path = format!("manifest/{relative_path}");
        let actual_sha256 = sha256_archive_file(&archive_files, &archive_path)?;
        let expected_sha256 = json_string(entry, "sha256");
        if expected_sha256 != actual_sha256 {
            anyhow::bail!(
                "plugin package embedded manifest file sha256 mismatch for {name}: expected {expected_sha256}, actual {actual_sha256}"
            );
        }
    }

    let verification = plugin_package_archive_json(
        &archive_files,
        "evidence/manifest-verification.json",
        "embedded plugin manifest verification",
    )?;
    validate_plugin_manifest_verification_contract(&verification)?;
    let verification_sha256 =
        sha256_archive_file(&archive_files, "evidence/manifest-verification.json")?;
    if verification_sha256 != json_string(&summary, "manifest_verification_sha256") {
        anyhow::bail!(
            "plugin package embedded manifest verification sha256 mismatch: summary {}, actual {}",
            json_string(&summary, "manifest_verification_sha256"),
            verification_sha256
        );
    }

    let install_smoke = plugin_package_archive_json(
        &archive_files,
        "evidence/install-smoke.json",
        "embedded plugin install smoke",
    )?;
    validate_plugin_install_smoke_contract(&install_smoke)?;
    let install_smoke_sha256 = sha256_archive_file(&archive_files, "evidence/install-smoke.json")?;
    if install_smoke_sha256 != json_string(&summary, "install_smoke_sha256") {
        anyhow::bail!(
            "plugin package embedded install smoke sha256 mismatch: summary {}, actual {}",
            json_string(&summary, "install_smoke_sha256"),
            install_smoke_sha256
        );
    }

    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-package-verification.v1",
        "status": "passed",
        "summary_path": options.summary.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": actual_archive_sha256,
        "manifest_sha256": manifest_sha256,
        "manifest_verification_sha256": verification_sha256,
        "install_smoke_sha256": install_smoke_sha256,
        "archive_contents_verified": true,
        "embedded_summary_verified": true,
        "embedded_manifest_verified": true,
        "embedded_evidence_verified": true,
        "token_safe_output_verified": true,
        "provider_auth": summary["provider_auth"].clone(),
        "trust_boundary": summary["trust_boundary"].clone(),
        "control_plane_observation": summary["control_plane_observation"].clone(),
        "factory_v3_role": "parity_auditor"
    });
    let body = serde_json::to_string_pretty(&verification)?;
    if options.json_output {
        println!("{body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-package-verification.v1");
        println!("archive_sha256={actual_archive_sha256}");
    }
    Ok(())
}

pub(super) fn plugin_distribution_rehearsal(
    options: PluginDistributionRehearsalOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&options.summary)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "plugin package summary sha256 mismatch for {}: expected {}, actual {}",
            options.summary.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }
    let supplied_archive_sha256 = options.archive_sha256.trim();
    let actual_archive_sha256 = sha256_file(&options.archive)?;
    if supplied_archive_sha256 != actual_archive_sha256 {
        anyhow::bail!(
            "plugin package archive sha256 mismatch for {}: expected {}, actual {}",
            options.archive.display(),
            supplied_archive_sha256,
            actual_archive_sha256
        );
    }

    let source_summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.summary)
            .with_context(|| format!("read {}", options.summary.display()))?,
    )
    .with_context(|| format!("parse {}", options.summary.display()))?;
    validate_plugin_package_contract(&source_summary)?;
    let archive_files = read_plugin_package_archive_files(&options.archive)?;

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let package_preflight = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "package-verify".to_string(),
        "--summary".to_string(),
        options.summary.display().to_string(),
        "--summary-sha256".to_string(),
        actual_summary_sha256.clone(),
        "--archive".to_string(),
        options.archive.display().to_string(),
        "--archive-sha256".to_string(),
        actual_archive_sha256.clone(),
        "--json".to_string(),
    ])?;

    let mut target_results = serde_json::Map::new();
    for target in ["codex", "claude"] {
        let installed_root = options
            .out_dir
            .join("installations")
            .join(target)
            .join("ao2-governed-execution");
        write_plugin_installation_from_archive(
            &archive_files,
            &installed_root,
            &options.summary,
            &options.archive,
        )?;
        let result = run_plugin_distribution_target_rehearsal(
            target,
            &installed_root,
            &actual_summary_sha256,
            &actual_archive_sha256,
        )?;
        target_results.insert(target.to_string(), result);
    }

    let observer_input_path = options.out_dir.join("k37-plugin-observer-input.json");
    let observer_input = serde_json::json!({
        "schema_version": "ao2.k37-plugin-observer-input.v1",
        "status": "ready_for_k37_observation",
        "producer": "ao2",
        "work_source": "codex-cron AO2 production/plugin readiness",
        "package_summary_path": options.summary.display().to_string(),
        "package_summary_sha256": actual_summary_sha256,
        "package_archive_path": options.archive.display().to_string(),
        "package_archive_sha256": actual_archive_sha256,
        "observed_evidence_scope": [
            "ao2.plugin-readiness.v1",
            "ao2.plugin-manifest-verification.v1",
            "ao2.plugin-install-smoke.v1",
            "ao2.plugin-package-verification.v1",
            "ao2.plugin-wrapper-harness.v1",
            "ao2.plugin-wrapper-harness-verification.v1"
        ],
        "target_results": target_results.clone(),
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
        "factory_v3_role": "parity_auditor"
    });
    atomic_write_text(
        &observer_input_path,
        &serde_json::to_string_pretty(&observer_input)?,
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &observer_input_path,
        "k37-plugin-observer-input.json",
    )?;
    let observer_input_sha256 = sha256_file(&observer_input_path)?;

    let summary_path = options.out_dir.join("plugin-distribution-rehearsal.json");
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-distribution-rehearsal.v1",
        "status": "passed",
        "summary_path": summary_path.display().to_string(),
        "summary_sha256": supplied_summary_sha256,
        "archive_path": options.archive.display().to_string(),
        "archive_sha256": supplied_archive_sha256,
        "package_verified_before_install": json_string(&package_preflight, "status") == "passed",
        "targets": ["codex", "claude"],
        "target_results": target_results,
        "observer_input": {
            "path": observer_input_path.display().to_string(),
            "sha256": observer_input_sha256
        },
        "provider_auth": source_summary["provider_auth"].clone(),
        "trust_boundary": source_summary["trust_boundary"].clone(),
        "control_plane_observation": source_summary["control_plane_observation"].clone(),
        "factory_v3_role": "parity_auditor",
        "token_safe_output_verified": true
    });
    let summary_body = serde_json::to_string_pretty(&summary)?;
    atomic_write_text(&summary_path, &summary_body)?;
    factory_app_run_bundle_reject_secret_markers(
        &summary_path,
        "plugin-distribution-rehearsal.json",
    )?;

    if options.json_output {
        println!("{summary_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.plugin-distribution-rehearsal.v1");
        println!("observer_input={}", observer_input_path.display());
    }
    Ok(())
}

fn run_plugin_distribution_target_rehearsal(
    target: &str,
    installed_root: &Path,
    summary_sha256: &str,
    archive_sha256: &str,
) -> Result<serde_json::Value> {
    let manifest_dir = installed_root.join("manifest");
    let evidence_dir = installed_root
        .join("evidence")
        .join("distribution-rehearsal");
    fs::create_dir_all(&evidence_dir)
        .with_context(|| format!("create {}", evidence_dir.display()))?;

    let installed_summary = installed_root.join("ao2-plugin-package.json");
    let installed_archive = installed_root.join("ao2-plugin-package.tar.gz");
    let package_verification = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "package-verify".to_string(),
        "--summary".to_string(),
        installed_summary.display().to_string(),
        "--summary-sha256".to_string(),
        summary_sha256.to_string(),
        "--archive".to_string(),
        installed_archive.display().to_string(),
        "--archive-sha256".to_string(),
        archive_sha256.to_string(),
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
        evidence_dir
            .join("install-smoke.json")
            .display()
            .to_string(),
        "--json".to_string(),
    ])?;
    let install_smoke_path = evidence_dir.join("install-smoke.json");
    let install_smoke_sha256 = sha256_file(&install_smoke_path)?;
    validate_plugin_install_smoke_contract(&install_smoke)?;

    let args_file = manifest_dir.join("examples/plugin-wrapper-args.app-run.example.json");
    let signing_key = manifest_dir.join("smoke/signing-key.pem");
    ensure_rsa_private_key(&signing_key, 2048)?;
    let args_sha256 = sha256_file(&args_file)?;
    let wrapper_harness_dir = evidence_dir.join("wrapper-harness");
    let wrapper_harness = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "wrapper-harness".to_string(),
        "--readiness".to_string(),
        readiness_path.display().to_string(),
        "--readiness-sha256".to_string(),
        readiness_sha256.clone(),
        "--args-file".to_string(),
        args_file.display().to_string(),
        "--args-sha256".to_string(),
        args_sha256,
        "--run-kind".to_string(),
        "app-run".to_string(),
        "--out-dir".to_string(),
        wrapper_harness_dir.display().to_string(),
        "--json".to_string(),
    ])?;
    if signing_key.exists() {
        fs::remove_file(&signing_key)
            .with_context(|| format!("remove temporary signing key {}", signing_key.display()))?;
    }
    let public_key = signing_key.with_file_name("signing-key.public.pem");
    if public_key.exists() {
        fs::remove_file(&public_key)
            .with_context(|| format!("remove temporary public key {}", public_key.display()))?;
    }
    validate_plugin_wrapper_harness_summary(
        &wrapper_harness,
        &wrapper_harness_dir.join("plugin-wrapper-harness.json"),
    )?;
    let wrapper_harness_sha256 =
        sha256_file(&wrapper_harness_dir.join("plugin-wrapper-harness.json"))?;

    let wrapper_verification = run_current_ao2_json_command(&[
        "plugin".to_string(),
        "wrapper-harness-verify".to_string(),
        "--evidence-dir".to_string(),
        wrapper_harness_dir.display().to_string(),
        "--summary-sha256".to_string(),
        wrapper_harness_sha256.clone(),
        "--json".to_string(),
    ])?;
    let wrapper_verification_path = evidence_dir.join("wrapper-harness-verification.json");
    atomic_write_text(
        &wrapper_verification_path,
        &serde_json::to_string_pretty(&wrapper_verification)?,
    )?;

    let package_verification_sha256 = sha256_file(&package_verification_path)?;
    let wrapper_harness_verification_sha256 = sha256_file(&wrapper_verification_path)?;
    let result = serde_json::json!({
        "status": "passed",
        "target": target,
        "installed_package_dir": installed_root.display().to_string(),
        "commands_from_installed_package_paths": true,
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
        "wrapper_harness_dir": wrapper_harness_dir.display().to_string(),
        "wrapper_harness_sha256": wrapper_harness_sha256,
        "wrapper_harness_verification_path": wrapper_verification_path.display().to_string(),
        "wrapper_harness_verification_sha256": wrapper_harness_verification_sha256,
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        }
    });
    Ok(result)
}

pub(super) fn read_plugin_package_archive_files(
    archive_path: &Path,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let archive_file =
        fs::File::open(archive_path).with_context(|| format!("open {}", archive_path.display()))?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);
    let mut files = BTreeMap::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.to_path_buf();
        let normalized = plugin_package_normalized_archive_path(&path)?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        reject_secret_markers_in_bytes(&bytes, &normalized)?;
        files.insert(normalized, bytes);
    }
    Ok(files)
}

pub(super) fn plugin_package_normalized_archive_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => {
                let part = part.to_string_lossy();
                for segment in part.split(['/', '\\']) {
                    if segment.is_empty() || segment == "." || segment == ".." {
                        anyhow::bail!(
                            "plugin package archive contains unsafe path: {}",
                            path.display()
                        );
                    }
                    parts.push(segment.to_string());
                }
            }
            _ => anyhow::bail!(
                "plugin package archive contains unsafe path: {}",
                path.display()
            ),
        }
    }
    if parts.is_empty() {
        anyhow::bail!("plugin package archive contains empty path");
    }
    Ok(parts.join("/"))
}

pub(super) fn plugin_package_archive_json(
    files: &BTreeMap<String, Vec<u8>>,
    path: &str,
    label: &str,
) -> Result<serde_json::Value> {
    let bytes = files
        .get(path)
        .with_context(|| format!("plugin package archive missing {path}"))?;
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    serde_json::from_slice(bytes).with_context(|| format!("parse {label} at {path}"))
}

pub(super) fn sha256_archive_file(files: &BTreeMap<String, Vec<u8>>, path: &str) -> Result<String> {
    let bytes = files
        .get(path)
        .with_context(|| format!("plugin package archive missing {path}"))?;
    Ok(sha256_bytes_hex(bytes))
}

pub(super) fn reject_secret_markers_in_bytes(bytes: &[u8], relative_path: &str) -> Result<()> {
    let text = String::from_utf8_lossy(bytes);
    for marker in [
        "Authorization: Bearer ",
        "AO2_CP_API_TOKEN=",
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
    ] {
        if text.contains(marker) {
            anyhow::bail!(
                "plugin package archive contains forbidden secret marker {marker:?} in {relative_path}"
            );
        }
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
        factory_app_run_bundle_reject_secret_fields(&value, relative_path)?;
    }
    Ok(())
}

pub(super) fn validate_plugin_package_contract(package: &serde_json::Value) -> Result<()> {
    validate_plugin_package_contract_inner(package, true)
}

fn validate_plugin_package_contract_allowing_pending_archive_sha(
    package: &serde_json::Value,
) -> Result<()> {
    validate_plugin_package_contract_inner(package, false)
}

fn validate_plugin_package_contract_inner(
    package: &serde_json::Value,
    require_archive_sha: bool,
) -> Result<()> {
    if json_string(package, "schema_version") != "ao2.plugin-package.v1" {
        anyhow::bail!(
            "plugin package requires ao2.plugin-package.v1, got {}",
            json_string(package, "schema_version")
        );
    }
    if json_string(package, "status") != "packaged" {
        anyhow::bail!("plugin package must be packaged");
    }
    if package.get("plugin_targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("plugin package must target codex and claude");
    }
    validate_plugin_provider_auth(
        package
            .get("provider_auth")
            .context("plugin package missing provider_auth")?,
        "plugin package",
    )?;
    validate_plugin_observer_trust_boundary(
        package
            .get("trust_boundary")
            .context("plugin package missing trust_boundary")?,
        "plugin package",
    )?;
    let digest_gates = package
        .get("digest_gates")
        .context("plugin package missing digest_gates")?;
    for field in [
        "manifest_sha256_verified",
        "manifest_verification_sha256_verified",
        "install_smoke_sha256_verified",
        "manifest_files_sha256_verified",
        "wrapper_inputs_must_be_sha256_pinned",
    ] {
        if !json_bool(digest_gates, field) {
            anyhow::bail!("plugin package digest gate is incomplete: {field}");
        }
    }
    let contents = package
        .get("package_contents")
        .context("plugin package missing package_contents")?;
    for (field, expected) in [
        ("manifest_dir", "manifest/"),
        (
            "manifest_verification",
            "evidence/manifest-verification.json",
        ),
        ("install_smoke", "evidence/install-smoke.json"),
        ("summary", "ao2-plugin-package.json"),
    ] {
        if json_string(contents, field) != expected {
            anyhow::bail!("plugin package package_contents.{field} must be {expected}");
        }
    }
    let token_safe_output = package
        .get("token_safe_output")
        .context("plugin package missing token_safe_output")?;
    if json_string(token_safe_output, "redaction_policy") != "paths_status_and_digests_only"
        || json_bool(token_safe_output, "bearer_tokens_serialized")
        || json_bool(token_safe_output, "cookies_serialized")
        || json_bool(token_safe_output, "private_keys_serialized")
    {
        anyhow::bail!("plugin package token_safe_output is not safe");
    }
    let observation = package
        .get("control_plane_observation")
        .context("plugin package missing control_plane_observation")?;
    if json_string(observation, "role") != "read_only_observer"
        || json_bool(observation, "may_mutate_evidence")
        || json_bool(observation, "may_approve_release")
    {
        anyhow::bail!("plugin package control-plane observation is not read-only");
    }
    if json_string(package, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("plugin package factory_v3_role must be parity_auditor");
    }
    for field in [
        "manifest_sha256",
        "manifest_verification_sha256",
        "install_smoke_sha256",
    ] {
        let digest = json_string(package, field);
        if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            anyhow::bail!("plugin package {field} must be a sha256 hex digest");
        }
    }
    let archive = package
        .get("archive")
        .context("plugin package missing archive")?;
    if json_string(archive, "path").is_empty() {
        anyhow::bail!("plugin package archive.path is required");
    }
    if require_archive_sha {
        let archive_sha = json_string(archive, "sha256");
        if archive_sha.len() != 64 || !archive_sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
            anyhow::bail!("plugin package archive.sha256 must be a sha256 hex digest");
        }
    } else if !archive
        .get("sha256")
        .is_some_and(serde_json::Value::is_null)
    {
        anyhow::bail!(
            "embedded plugin package archive.sha256 must be null before archive digesting"
        );
    }
    Ok(())
}

pub(super) fn validate_plugin_install_smoke_contract(smoke: &serde_json::Value) -> Result<()> {
    if json_string(smoke, "schema_version") != "ao2.plugin-install-smoke.v1" {
        anyhow::bail!(
            "plugin install smoke requires ao2.plugin-install-smoke.v1, got {}",
            json_string(smoke, "schema_version")
        );
    }
    if json_string(smoke, "status") != "passed" {
        anyhow::bail!("plugin install smoke must be passed");
    }
    validate_plugin_provider_auth(
        smoke
            .get("provider_auth")
            .context("plugin install smoke missing provider_auth")?,
        "plugin install smoke",
    )?;
    validate_plugin_observer_trust_boundary(
        smoke
            .get("trust_boundary")
            .context("plugin install smoke missing trust_boundary")?,
        "plugin install smoke",
    )?;
    let digest_gates = smoke
        .get("digest_gates")
        .context("plugin install smoke missing digest_gates")?;
    if !json_bool(digest_gates, "manifest_verification_sha256_verified")
        || !json_bool(digest_gates, "manifest_sha256_verified")
        || !json_bool(digest_gates, "manifest_files_sha256_verified")
        || !json_bool(digest_gates, "wrapper_inputs_must_be_sha256_pinned")
    {
        anyhow::bail!("plugin install smoke digest gates are incomplete");
    }
    let dry_run = smoke
        .get("dry_run")
        .context("plugin install smoke missing dry_run")?;
    for field in [
        "provider_execution_started",
        "queue_mutated",
        "memory_written",
        "ao_artifacts_mutated",
        "release_approved",
        "control_plane_mutated",
    ] {
        if json_bool(dry_run, field) {
            anyhow::bail!("plugin install smoke dry_run field must be false: {field}");
        }
    }
    let observation = smoke
        .get("control_plane_observation")
        .context("plugin install smoke missing control_plane_observation")?;
    if json_string(observation, "role") != "read_only_observer"
        || json_bool(observation, "may_mutate_evidence")
        || json_bool(observation, "may_approve_release")
    {
        anyhow::bail!("plugin install smoke control-plane observation is not read-only");
    }
    Ok(())
}

fn validate_plugin_manifest_verification_contract(verification: &serde_json::Value) -> Result<()> {
    if json_string(verification, "schema_version") != "ao2.plugin-manifest-verification.v1" {
        anyhow::bail!(
            "plugin manifest verification requires ao2.plugin-manifest-verification.v1, got {}",
            json_string(verification, "schema_version")
        );
    }
    if json_string(verification, "status") != "passed" {
        anyhow::bail!("plugin manifest verification must be passed");
    }
    if !json_bool(verification, "file_digests_verified")
        || !json_bool(verification, "readiness_contract_verified")
        || !json_bool(verification, "wrapper_args_verified")
        || !json_bool(verification, "local_oauth_smokes_verified")
        || !json_bool(verification, "install_manifest_verified")
        || !json_bool(verification, "consumer_readme_verified")
        || !json_bool(verification, "token_safe_output_verified")
    {
        anyhow::bail!("plugin manifest verification is incomplete");
    }
    validate_plugin_provider_auth(
        verification
            .get("provider_auth")
            .context("plugin manifest verification missing provider_auth")?,
        "plugin manifest verification",
    )?;
    validate_plugin_observer_trust_boundary(
        verification
            .get("trust_boundary")
            .context("plugin manifest verification missing trust_boundary")?,
        "plugin manifest verification",
    )?;
    let observation = verification
        .get("control_plane_observation")
        .context("plugin manifest verification missing control_plane_observation")?;
    if json_string(observation, "role") != "read_only_observer"
        || json_bool(observation, "may_mutate_evidence")
        || json_bool(observation, "may_approve_release")
    {
        anyhow::bail!("plugin manifest verification control-plane observation is not read-only");
    }
    Ok(())
}

fn validate_plugin_manifest_contract(manifest: &serde_json::Value) -> Result<()> {
    if json_string(manifest, "schema_version") != "ao2.plugin-manifest.v1" {
        anyhow::bail!(
            "plugin manifest requires ao2.plugin-manifest.v1, got {}",
            json_string(manifest, "schema_version")
        );
    }
    if json_string(manifest, "status") != "packaged" {
        anyhow::bail!("plugin manifest must be packaged");
    }
    if manifest.get("plugin_targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("plugin manifest must target codex and claude");
    }
    validate_plugin_provider_auth(
        manifest
            .get("provider_auth")
            .context("plugin manifest missing provider_auth")?,
        "plugin manifest",
    )?;
    validate_plugin_observer_trust_boundary(
        manifest
            .get("trust_boundary")
            .context("plugin manifest missing trust_boundary")?,
        "plugin manifest",
    )?;
    let digest_gates = manifest
        .get("digest_gates")
        .context("plugin manifest missing digest_gates")?;
    if !json_bool(digest_gates, "manifest_files_sha256_verified")
        || !json_bool(digest_gates, "wrapper_inputs_must_be_sha256_pinned")
    {
        anyhow::bail!("plugin manifest digest gates are incomplete");
    }
    let token_safe_output = manifest
        .get("token_safe_output")
        .context("plugin manifest missing token_safe_output")?;
    if json_string(token_safe_output, "redaction_policy") != "paths_status_and_digests_only"
        || json_bool(token_safe_output, "bearer_tokens_serialized")
        || json_bool(token_safe_output, "cookies_serialized")
        || json_bool(token_safe_output, "private_keys_serialized")
    {
        anyhow::bail!("plugin manifest token_safe_output is not safe");
    }
    Ok(())
}

fn plugin_manifest_named_file_path(
    manifest_dir: &Path,
    files: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<PathBuf> {
    let entry = files
        .get(name)
        .with_context(|| format!("plugin manifest files missing {name}"))?;
    plugin_manifest_package_file_path(manifest_dir, name, entry)
}

fn plugin_manifest_package_file_path(
    manifest_dir: &Path,
    name: &str,
    entry: &serde_json::Value,
) -> Result<PathBuf> {
    let relative_path = json_string(entry, "path");
    if relative_path.is_empty() {
        anyhow::bail!("plugin manifest file {name} missing path");
    }
    let path = PathBuf::from(&relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        anyhow::bail!("plugin manifest file {name} path must stay inside manifest dir");
    }
    let sha256 = json_string(entry, "sha256");
    if sha256.len() != 64 || !sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("plugin manifest file {name} sha256 must be a hex digest");
    }
    Ok(manifest_dir.join(path))
}

fn validate_plugin_local_oauth_smoke(smoke: &serde_json::Value, provider: &str) -> Result<()> {
    if json_string(smoke, "schema_version") != "ao2.plugin-local-oauth-smoke.v1" {
        anyhow::bail!(
            "plugin local OAuth smoke requires ao2.plugin-local-oauth-smoke.v1, got {}",
            json_string(smoke, "schema_version")
        );
    }
    if json_string(smoke, "provider") != provider {
        anyhow::bail!("plugin local OAuth smoke provider mismatch for {provider}");
    }
    let auth = smoke
        .get("auth")
        .context("plugin local OAuth smoke missing auth")?;
    if !json_bool(auth, "local_oauth_cli_only")
        || json_bool(auth, "provider_api_key_auth_allowed")
        || !json_bool(auth, "forbidden_provider_api_key_env_absent")
    {
        anyhow::bail!("plugin local OAuth smoke auth is not local OAuth CLI only");
    }
    validate_plugin_observer_trust_boundary(
        smoke
            .get("trust_boundary")
            .context("plugin local OAuth smoke missing trust_boundary")?,
        "plugin local OAuth smoke",
    )?;
    Ok(())
}

fn validate_plugin_install_manifest(install: &serde_json::Value) -> Result<()> {
    if json_string(install, "schema_version") != "ao2.codex-claude-plugin-install.v1" {
        anyhow::bail!(
            "plugin install manifest requires ao2.codex-claude-plugin-install.v1, got {}",
            json_string(install, "schema_version")
        );
    }
    if install.get("targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("plugin install manifest must target codex and claude");
    }
    let commands = install
        .get("commands")
        .context("plugin install manifest missing commands")?;
    for command in [
        "readiness",
        "manifest_verify",
        "install_smoke",
        "package",
        "package_verify",
        "distribution_rehearsal",
        "consumer_lifecycle",
        "consumer_lifecycle_windows_recovery",
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
        "distribution_observer_bundle",
        "clean_package_operator_index",
        "packaged_replacement_observer_bundle",
        "packaged_replacement_observer_bundle_verify",
        "adapter_scaffold",
        "adapter_scaffold_verify",
        "adapter_install_smoke",
        "adapter_install_smoke_verify",
        "adapter_install_smoke_observer_bundle",
        "adapter_observer_bundle",
        "wrapper_harness",
        "wrapper_harness_verify",
    ] {
        if json_string(commands, command).is_empty() {
            anyhow::bail!("plugin install manifest missing command {command}");
        }
    }
    validate_plugin_provider_auth(
        install
            .get("provider_auth")
            .context("plugin install manifest missing provider_auth")?,
        "plugin install manifest",
    )?;
    validate_plugin_observer_trust_boundary(
        install
            .get("trust_boundary")
            .context("plugin install manifest missing trust_boundary")?,
        "plugin install manifest",
    )?;
    Ok(())
}

fn plugin_manifest_consumer_readme() -> String {
    [
        "# AO2 Governed Execution Plugin",
        "",
        "This package is the deterministic Codex/Claude wrapper surface for AO2 governed execution.",
        "",
        "## Trust Boundary",
        "",
        "- execution_owner: ao2",
        "- factory_v3_role: parity_auditor",
        "- control_plane_role: read_only_observer",
        "- mutates_ao_artifacts: false",
        "- control_plane_approves_release: false",
        "",
        "## Authentication",
        "",
        "Use local OAuth CLI only for Codex and Claude provider authentication. Provider API-key authentication is not part of this package.",
        "",
        "## Smoke Commands",
        "",
        "Generate the local sample signing key before running the packaged wrapper examples:",
        "",
        "```bash",
        "sh smoke/generate-signing-key.sh",
        "If ao2 is not on PATH yet, run AO2_BIN=/path/to/ao2 sh smoke/generate-signing-key.sh.",
        "```",
        "",
        "```bash",
        "ao2 plugin readiness --json",
        "ao2 plugin manifest-verify --manifest-dir <dir> --manifest-sha256 <sha256> --json",
        "ao2 plugin install-smoke --manifest-dir <dir> --verification <path> --verification-sha256 <sha256> --json",
        "ao2 plugin package --manifest-dir <dir> --manifest-verification <path> --manifest-verification-sha256 <sha256> --install-smoke <path> --install-smoke-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin package-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "ao2 plugin distribution-rehearsal --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin consumer-lifecycle --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --adapter-scaffold <path> --adapter-scaffold-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin consumer-lifecycle-windows-recovery --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --adapter-scaffold <path> --adapter-scaffold-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin consumer-lifecycle-observer-bundle --macos-lifecycle <path> --macos-sha256 <sha256> --ubuntu-lifecycle <path> --ubuntu-sha256 <sha256> --windows-lifecycle <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin consumer-lifecycle-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "ao2 plugin control-plane-fixture-handoff --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin control-plane-fixture-handoff-verify --handoff <path> --handoff-sha256 <sha256> --out <path> --json",
        "ao2 plugin release-candidate --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --distribution-rehearsal <path> --distribution-rehearsal-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-gate-with-replacement-observer-bundle <path> --release-gate-with-replacement-observer-bundle-sha256 <sha256> --release-gate-with-replacement-observer-archive <path> --release-gate-with-replacement-observer-archive-sha256 <sha256> --control-plane-fixture-handoff-verification <path> --control-plane-fixture-handoff-verification-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "ao2 plugin release-candidate-verify --summary <path> --summary-sha256 <sha256> --json",
        "ao2 plugin release-candidate-windows-recovery --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --distribution-rehearsal <path> --distribution-rehearsal-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-gate-with-replacement-observer-bundle <path> --release-gate-with-replacement-observer-bundle-sha256 <sha256> --release-gate-with-replacement-observer-archive <path> --release-gate-with-replacement-observer-archive-sha256 <sha256> --control-plane-fixture-handoff-verification <path> --control-plane-fixture-handoff-verification-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "ao2 plugin release-candidate-windows-recovery-verify --recovery <path> --recovery-sha256 <sha256> --out <path> --json",
        "ao2 plugin release-candidate-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin release-candidate-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "ao2 plugin release-candidate-control-plane-fixture-handoff --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin release-candidate-control-plane-fixture-handoff-verify --handoff <path> --handoff-sha256 <sha256> --out <path> --json",
        "ao2 plugin final-install-transcript --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin final-install-transcript-observer-bundle --macos-codex-transcript <path> --macos-codex-sha256 <sha256> --macos-claude-transcript <path> --macos-claude-sha256 <sha256> --ubuntu-codex-transcript <path> --ubuntu-codex-sha256 <sha256> --ubuntu-claude-transcript <path> --ubuntu-claude-sha256 <sha256> --windows-codex-transcript <path> --windows-codex-sha256 <sha256> --windows-claude-transcript <path> --windows-claude-sha256 <sha256> --out-dir <dir> --json",
        "ao2 factory closer-decision --rubric <path> --rubric-sha256 <sha256> --evidence <path> --evidence-sha256 <sha256> --skill-contract-manifest <path> --skill-contract-manifest-sha256 <sha256> --signing-key <path> --signer-id <id> --out <path> --json",
        "ao2 factory closer-decision-verify --decision <path> --decision-sha256 <sha256> --json",
        "ao2 plugin shipment-readiness --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-candidate-observer-bundle <path> --release-candidate-observer-bundle-sha256 <sha256> --release-candidate-observer-archive <path> --release-candidate-observer-archive-sha256 <sha256> --final-install-transcript-observer-bundle <path> --final-install-transcript-observer-bundle-sha256 <sha256> --final-install-transcript-observer-archive <path> --final-install-transcript-observer-archive-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "ao2 plugin distribution-observer-bundle --macos-observer <path> --macos-sha256 <sha256> --ubuntu-observer <path> --ubuntu-sha256 <sha256> --windows-observer <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin clean-package-operator-index --macos-rehearsal <path> --macos-sha256 <sha256> --ubuntu-rehearsal <path> --ubuntu-sha256 <sha256> --windows-rehearsal <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin packaged-replacement-observer-bundle --macos-proof <path> --macos-sha256 <sha256> --ubuntu-proof <path> --ubuntu-sha256 <sha256> --windows-proof <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin packaged-replacement-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "ao2 plugin adapter-scaffold --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --k37-bundle <path> --k37-bundle-sha256 <sha256> --k37-archive <path> --k37-archive-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin adapter-scaffold-verify --summary <path> --summary-sha256 <sha256> --json",
        "ao2 plugin adapter-install-smoke --summary <path> --summary-sha256 <sha256> --out <path> --json",
        "ao2 plugin adapter-install-smoke-verify --smoke <path> --smoke-sha256 <sha256> --json",
        "ao2 plugin adapter-install-smoke-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin adapter-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "ao2 plugin wrapper-harness --readiness <path> --readiness-sha256 <sha256> --args-file <path> --args-sha256 <sha256> --run-kind <app-run|project-run> --out-dir <dir> --json",
        "ao2 plugin wrapper-harness-verify --evidence-dir <dir> --summary-sha256 <sha256> --json",
        "```",
        "",
        "Wrapper inputs are digest-pinned before execution. Persisted wrapper stdout and stderr are redacted; durable evidence paths and SHA256 digests are the review surface.",
        "",
    ]
    .join("\n")
}

fn plugin_manifest_app_spec() -> String {
    [
        "# AO2 Plugin Sample App",
        "",
        "Acceptance:",
        "- Implement a health check helper returning `ok`.",
        "- Preserve local OAuth CLI-only provider posture.",
        "- Keep ao2-control-plane as read-only observer only.",
        "",
    ]
    .join("\n")
}

fn plugin_manifest_project_spec() -> String {
    [
        "# AO2 Plugin Sample Project",
        "",
        "Acceptance:",
        "- Reuse the packaged app-run output as project-run evidence.",
        "- Package release-review evidence without factory-v3 driving execution.",
        "- Preserve factory-v3 as parity auditor and evaluator-closer reference.",
        "",
    ]
    .join("\n")
}

fn plugin_manifest_provider_script() -> String {
    [
        "mkdir -p plugin_sample tests",
        "cat > plugin_sample/__init__.py <<'PY'",
        "from .service import health",
        "",
        "__all__ = [\"health\"]",
        "PY",
        "cat > plugin_sample/service.py <<'PY'",
        "def health() -> str:",
        "    return \"ok\"",
        "PY",
        "cat > tests/test_service.py <<'PY'",
        "from plugin_sample import health",
        "",
        "",
        "def test_health_returns_ok():",
        "    assert health() == \"ok\"",
        "PY",
        "printf 'Summary: packaged AO2 plugin sample app implemented\\n'",
        "printf 'Changed files: plugin_sample/service.py tests/test_service.py\\n'",
        "",
    ]
    .join("\n")
}

fn plugin_manifest_signing_key_generator_sh() -> String {
    [
        "#!/bin/sh",
        "set -eu",
        "AO2_BIN=${AO2_BIN:-ao2}",
        "\"$AO2_BIN\" workbench support-keygen --out smoke/signing-key.pem --bits 2048 >/dev/null",
        "printf 'generated smoke/signing-key.pem\\n'",
        "",
    ]
    .join("\n")
}

fn plugin_manifest_signing_key_generator_ps1() -> String {
    [
        "$ErrorActionPreference = 'Stop'",
        "$Ao2Bin = if ($env:AO2_BIN) { $env:AO2_BIN } else { 'ao2' }",
        "& $Ao2Bin workbench support-keygen --out smoke/signing-key.pem --bits 2048 | Out-Null",
        "Write-Output 'generated smoke/signing-key.pem'",
        "",
    ]
    .join("\n")
}

fn validate_plugin_manifest_consumer_readme(readme: &str) -> Result<()> {
    for required in [
        "ao2 plugin readiness --json",
        "ao2 plugin manifest-verify ",
        "ao2 plugin install-smoke ",
        "ao2 plugin package ",
        "ao2 plugin package-verify ",
        "ao2 plugin distribution-rehearsal ",
        "ao2 plugin consumer-lifecycle ",
        "ao2 plugin consumer-lifecycle-windows-recovery ",
        "ao2 plugin consumer-lifecycle-observer-bundle ",
        "ao2 plugin consumer-lifecycle-observer-bundle-verify ",
        "ao2 plugin control-plane-fixture-handoff ",
        "ao2 plugin control-plane-fixture-handoff-verify ",
        "ao2 plugin release-candidate ",
        "ao2 plugin release-candidate-verify ",
        "ao2 plugin release-candidate-windows-recovery ",
        "ao2 plugin release-candidate-windows-recovery-verify ",
        "ao2 plugin release-candidate-observer-bundle ",
        "ao2 plugin release-candidate-observer-bundle-verify ",
        "ao2 plugin release-candidate-control-plane-fixture-handoff ",
        "ao2 plugin release-candidate-control-plane-fixture-handoff-verify ",
        "ao2 plugin final-install-transcript ",
        "ao2 plugin final-install-transcript-observer-bundle ",
        "ao2 factory closer-decision ",
        "ao2 factory closer-decision-verify ",
        "ao2 plugin shipment-readiness ",
        "ao2 plugin distribution-observer-bundle ",
        "ao2 plugin clean-package-operator-index ",
        "ao2 plugin packaged-replacement-observer-bundle ",
        "ao2 plugin packaged-replacement-observer-bundle-verify ",
        "ao2 plugin adapter-scaffold ",
        "ao2 plugin adapter-scaffold-verify ",
        "ao2 plugin adapter-install-smoke ",
        "ao2 plugin adapter-install-smoke-verify ",
        "ao2 plugin adapter-install-smoke-observer-bundle ",
        "ao2 plugin adapter-observer-bundle ",
        "ao2 plugin wrapper-harness ",
        "ao2 plugin wrapper-harness-verify ",
        "local OAuth CLI only",
        "control_plane_role: read_only_observer",
        "mutates_ao_artifacts: false",
        "control_plane_approves_release: false",
    ] {
        if !readme.contains(required) {
            anyhow::bail!("plugin consumer README missing required text: {required}");
        }
    }
    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        if readme.contains(forbidden) {
            anyhow::bail!("plugin consumer README contains forbidden marker: {forbidden}");
        }
    }
    Ok(())
}

pub(super) fn validate_plugin_provider_auth(auth: &serde_json::Value, context: &str) -> Result<()> {
    if !json_bool(auth, "local_oauth_cli_only") || json_bool(auth, "provider_api_key_auth_allowed")
    {
        anyhow::bail!("{context} provider_auth is not local OAuth CLI only");
    }
    Ok(())
}

pub(super) fn validate_plugin_observer_trust_boundary(
    trust_boundary: &serde_json::Value,
    context: &str,
) -> Result<()> {
    if json_string(trust_boundary, "execution_owner") != "ao2"
        || json_string(trust_boundary, "factory_v3_role") != "parity_auditor"
        || json_string(trust_boundary, "control_plane_role") != "read_only_observer"
        || json_bool(trust_boundary, "mutates_ao_artifacts")
        || json_bool(trust_boundary, "control_plane_approves_release")
    {
        anyhow::bail!("{context} trust_boundary is not observer-only");
    }
    Ok(())
}
