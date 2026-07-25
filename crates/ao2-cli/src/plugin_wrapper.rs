use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};
use ao2_policy::{redact_secrets, secret_redaction_class_counts};
use serde::Deserialize;

use super::{
    atomic_write_text, factory_app_run_bundle_reject_secret_markers,
    fail_if_provider_api_key_env_present, json_bool, json_string, json_u64, sha256_file,
};

pub(super) struct PluginWrapperHarnessOptions {
    pub(super) readiness: PathBuf,
    pub(super) readiness_sha256: String,
    pub(super) args_file: PathBuf,
    pub(super) args_sha256: String,
    pub(super) run_kind: String,
    pub(super) out_dir: PathBuf,
    pub(super) json_output: bool,
}

pub(super) struct PluginWrapperHarnessVerifyOptions {
    pub(super) evidence_dir: PathBuf,
    pub(super) summary_sha256: String,
    pub(super) json_output: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct PluginWrapperArgsFile {
    pub(super) schema_version: String,
    pub(super) run_kind: String,
    pub(super) args: Vec<String>,
}

pub(super) fn plugin_wrapper_harness(options: PluginWrapperHarnessOptions) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let supplied_readiness_sha256 = options.readiness_sha256.trim();
    let actual_readiness_sha256 = sha256_file(&options.readiness)?;
    if supplied_readiness_sha256 != actual_readiness_sha256 {
        anyhow::bail!(
            "readiness_sha256 mismatch for {}: expected {}, actual {}",
            options.readiness.display(),
            supplied_readiness_sha256,
            actual_readiness_sha256
        );
    }
    let readiness: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&options.readiness)
            .with_context(|| format!("read {}", options.readiness.display()))?,
    )
    .with_context(|| format!("parse {}", options.readiness.display()))?;
    validate_plugin_readiness_contract(&readiness)?;

    let supplied_args_sha256 = options.args_sha256.trim();
    let actual_args_sha256 = sha256_file(&options.args_file)?;
    if supplied_args_sha256 != actual_args_sha256 {
        anyhow::bail!(
            "args_sha256 mismatch for {}: expected {}, actual {}",
            options.args_file.display(),
            supplied_args_sha256,
            actual_args_sha256
        );
    }
    let args_payload: PluginWrapperArgsFile = serde_json::from_str(
        &fs::read_to_string(&options.args_file)
            .with_context(|| format!("read {}", options.args_file.display()))?,
    )
    .with_context(|| format!("parse {}", options.args_file.display()))?;
    validate_plugin_wrapper_args(&args_payload, &options.run_kind)?;
    let resolved_args =
        plugin_wrapper_args_resolve_relative_paths(&options.args_file, &args_payload.args);

    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("create {}", options.out_dir.display()))?;
    let output =
        ProcessCommand::new(std::env::current_exe().context("resolve current ao2 binary")?)
            .args(&resolved_args)
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .context("run digest-pinned ao2 factory command")?;

    let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout_redacted = redact_secrets(&stdout_raw);
    let stderr_redacted = redact_secrets(&stderr_raw);
    let stdout_path = options.out_dir.join("stdout.redacted.txt");
    let stderr_path = options.out_dir.join("stderr.redacted.txt");
    atomic_write_text(&stdout_path, &stdout_redacted)?;
    atomic_write_text(&stderr_path, &stderr_redacted)?;

    let child_json = serde_json::from_str::<serde_json::Value>(&stdout_redacted).ok();
    let child_exit_code = output.status.code().unwrap_or(1);
    let status = if output.status.success() {
        "accepted"
    } else {
        "failed"
    };
    let redaction_counts = secret_redaction_class_counts(&format!("{stdout_raw}\n{stderr_raw}"));
    let ao2_artifacts = child_json
        .as_ref()
        .and_then(|json| json.get("artifacts"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let summary_path = options.out_dir.join("plugin-wrapper-harness.json");
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-wrapper-harness.v1",
        "status": status,
        "run_kind": options.run_kind,
        "readiness_path": options.readiness.display().to_string(),
        "readiness_sha256": actual_readiness_sha256,
        "args_file": options.args_file.display().to_string(),
        "args_sha256": actual_args_sha256,
        "child_exit_code": child_exit_code,
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
            "redaction_class_counts": redaction_counts
        },
        "evidence": {
            "bundle_path": options.out_dir.display().to_string(),
            "summary": summary_path.display().to_string(),
            "stdout_redacted": stdout_path.display().to_string(),
            "stderr_redacted": stderr_path.display().to_string()
        },
        "ao2_artifacts": ao2_artifacts,
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        }
    });
    let summary_body = serde_json::to_string_pretty(&summary)?;
    atomic_write_text(&summary_path, &summary_body)?;

    if options.json_output {
        println!("{summary_body}");
    } else {
        println!("status={status}");
        println!("schema_version=ao2.plugin-wrapper-harness.v1");
        println!("evidence_bundle_path={}", options.out_dir.display());
    }

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!("digest-pinned ao2 factory command failed with exit code {child_exit_code}");
    }
}

pub(super) fn plugin_wrapper_harness_verify(
    options: PluginWrapperHarnessVerifyOptions,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let summary_path = options.evidence_dir.join("plugin-wrapper-harness.json");
    let supplied_summary_sha256 = options.summary_sha256.trim();
    let actual_summary_sha256 = sha256_file(&summary_path)?;
    if supplied_summary_sha256 != actual_summary_sha256 {
        anyhow::bail!(
            "plugin wrapper harness summary_sha256 mismatch for {}: expected {}, actual {}",
            summary_path.display(),
            supplied_summary_sha256,
            actual_summary_sha256
        );
    }

    let summary: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&summary_path)
            .with_context(|| format!("read {}", summary_path.display()))?,
    )
    .with_context(|| format!("parse {}", summary_path.display()))?;
    validate_plugin_wrapper_harness_summary(&summary, &summary_path)?;

    let run_kind = json_string(&summary, "run_kind");
    let evidence = summary
        .get("evidence")
        .context("plugin wrapper harness summary missing evidence")?;
    let stdout_path = PathBuf::from(json_string(evidence, "stdout_redacted"));
    let stderr_path = PathBuf::from(json_string(evidence, "stderr_redacted"));
    factory_app_run_bundle_reject_secret_markers(&summary_path, "plugin-wrapper-harness.json")?;
    factory_app_run_bundle_reject_secret_markers(&stdout_path, "stdout.redacted.txt")?;
    factory_app_run_bundle_reject_secret_markers(&stderr_path, "stderr.redacted.txt")?;

    let artifact_count = summary
        .get("ao2_artifacts")
        .and_then(serde_json::Value::as_object)
        .map(|artifacts| artifacts.len())
        .unwrap_or_default();
    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-wrapper-harness-verification.v1",
        "status": "passed",
        "evidence_dir": options.evidence_dir.display().to_string(),
        "summary_path": summary_path.display().to_string(),
        "summary_sha256": actual_summary_sha256,
        "run_kind": run_kind,
        "child_exit_code": json_u64(&summary, "child_exit_code"),
        "digest_gates_verified": true,
        "trust_boundary_verified": true,
        "token_safe_output_verified": true,
        "ao2_artifact_count": artifact_count,
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
        println!("schema_version=ao2.plugin-wrapper-harness-verification.v1");
        println!("summary_sha256={actual_summary_sha256}");
    }
    Ok(())
}

pub(super) fn validate_plugin_wrapper_harness_summary(
    summary: &serde_json::Value,
    summary_path: &Path,
) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.plugin-wrapper-harness.v1" {
        anyhow::bail!(
            "plugin wrapper harness summary requires ao2.plugin-wrapper-harness.v1, got {}",
            json_string(summary, "schema_version")
        );
    }
    if json_string(summary, "status") != "accepted" {
        anyhow::bail!("plugin wrapper harness summary must be accepted");
    }
    let run_kind = json_string(summary, "run_kind");
    if run_kind != "app-run" && run_kind != "project-run" {
        anyhow::bail!("plugin wrapper harness summary has unsupported run_kind: {run_kind}");
    }
    if json_u64(summary, "child_exit_code") != 0 {
        anyhow::bail!("plugin wrapper harness child_exit_code must be 0");
    }

    let exit_code_contract = summary
        .get("exit_code_contract")
        .context("plugin wrapper harness summary missing exit_code_contract")?;
    if json_u64(exit_code_contract, "success") != 0
        || json_u64(exit_code_contract, "runtime_error") != 1
        || json_u64(exit_code_contract, "cli_usage") != 2
        || !json_bool(exit_code_contract, "enforced")
    {
        anyhow::bail!("plugin wrapper harness exit_code_contract is not enforced");
    }

    let digest_gates = summary
        .get("digest_gates")
        .context("plugin wrapper harness summary missing digest_gates")?;
    if !json_bool(digest_gates, "readiness_sha256_verified")
        || !json_bool(digest_gates, "args_sha256_verified")
        || !json_bool(
            digest_gates,
            "factory_command_digest_pinned_before_execution",
        )
    {
        anyhow::bail!("plugin wrapper harness digest gates are incomplete");
    }

    let provider_auth = summary
        .get("provider_auth")
        .context("plugin wrapper harness summary missing provider_auth")?;
    if !json_bool(provider_auth, "local_oauth_cli_only")
        || json_bool(provider_auth, "provider_api_key_auth_allowed")
        || !json_bool(provider_auth, "forbidden_provider_api_key_env_absent")
    {
        anyhow::bail!("plugin wrapper harness provider_auth is not local OAuth CLI only");
    }

    let trust_boundary = summary
        .get("trust_boundary")
        .context("plugin wrapper harness summary missing trust_boundary")?;
    if json_string(trust_boundary, "execution_owner") != "ao2"
        || json_string(trust_boundary, "factory_v3_role") != "parity_auditor"
        || json_string(trust_boundary, "control_plane_role") != "read_only_observer"
        || json_bool(trust_boundary, "mutates_ao_artifacts")
        || json_bool(trust_boundary, "control_plane_approves_release")
    {
        anyhow::bail!("plugin wrapper harness trust_boundary is not observer-only");
    }

    let token_safe_output = summary
        .get("token_safe_output")
        .context("plugin wrapper harness summary missing token_safe_output")?;
    if !json_bool(token_safe_output, "stdout_redacted")
        || !json_bool(token_safe_output, "stderr_redacted")
    {
        anyhow::bail!("plugin wrapper harness token_safe_output is not redacted");
    }

    let evidence = summary
        .get("evidence")
        .context("plugin wrapper harness summary missing evidence")?;
    for field in ["summary", "stdout_redacted", "stderr_redacted"] {
        let path = PathBuf::from(json_string(evidence, field));
        if path.as_os_str().is_empty() {
            anyhow::bail!("plugin wrapper harness evidence missing {field}");
        }
        if field == "summary" && path != summary_path {
            anyhow::bail!("plugin wrapper harness summary path does not match evidence dir");
        }
        if !path.is_file() {
            anyhow::bail!(
                "plugin wrapper harness evidence file is missing for {field}: {}",
                path.display()
            );
        }
    }

    let control_plane_observation = summary
        .get("control_plane_observation")
        .context("plugin wrapper harness summary missing control_plane_observation")?;
    if json_string(control_plane_observation, "role") != "read_only_observer"
        || json_bool(control_plane_observation, "may_mutate_evidence")
        || json_bool(control_plane_observation, "may_approve_release")
    {
        anyhow::bail!("plugin wrapper harness control_plane_observation is not read-only");
    }

    if let Some(artifacts) = summary
        .get("ao2_artifacts")
        .and_then(serde_json::Value::as_object)
    {
        for (name, value) in artifacts {
            if value.is_null()
                && [
                    "acceptance_rubric",
                    "acceptance_rubric_sha256",
                    "project_plan",
                ]
                .contains(&name.as_str())
            {
                continue;
            }
            let Some(path) = value.as_str() else {
                anyhow::bail!("plugin wrapper harness ao2_artifacts.{name} must be a path string");
            };
            if name.ends_with("_sha256") {
                if path.len() != 64 || !path.chars().all(|ch| ch.is_ascii_hexdigit()) {
                    anyhow::bail!(
                        "plugin wrapper harness ao2_artifacts.{name} must be a sha256 hex digest"
                    );
                }
                continue;
            }
            if !Path::new(path).is_file() {
                anyhow::bail!(
                    "plugin wrapper harness ao2_artifacts.{name} file is missing: {path}"
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_plugin_readiness_contract(readiness: &serde_json::Value) -> Result<()> {
    if json_string(readiness, "schema_version") != "ao2.plugin-readiness.v1" {
        anyhow::bail!(
            "plugin wrapper harness requires ao2.plugin-readiness.v1, got {}",
            json_string(readiness, "schema_version")
        );
    }
    if json_string(readiness, "status") != "accepted" {
        anyhow::bail!("plugin readiness artifact must be accepted");
    }
    let exit_codes = readiness
        .get("exit_codes")
        .context("plugin readiness missing exit_codes")?;
    if exit_codes
        .get("success")
        .and_then(serde_json::Value::as_u64)
        != Some(0)
        || exit_codes
            .get("runtime_error")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
        || exit_codes
            .get("cli_usage")
            .and_then(serde_json::Value::as_u64)
            != Some(2)
    {
        anyhow::bail!("plugin readiness exit_codes must be success=0 runtime_error=1 cli_usage=2");
    }
    let trust_boundary = readiness
        .get("trust_boundary")
        .context("plugin readiness missing trust_boundary")?;
    if json_string(trust_boundary, "control_plane_role") != "read_only_observer"
        || trust_boundary
            .get("mutates_ao_artifacts")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        || trust_boundary
            .get("control_plane_approves_release")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
    {
        anyhow::bail!("plugin readiness trust_boundary is not observer-only");
    }
    Ok(())
}

pub(super) fn validate_plugin_wrapper_args(
    args_payload: &PluginWrapperArgsFile,
    run_kind: &str,
) -> Result<()> {
    if args_payload.schema_version != "ao2.plugin-wrapper-args.v1" {
        anyhow::bail!(
            "plugin wrapper args require ao2.plugin-wrapper-args.v1, got {}",
            args_payload.schema_version
        );
    }
    if args_payload.run_kind != run_kind {
        anyhow::bail!(
            "plugin wrapper run_kind mismatch: args file {}, CLI {}",
            args_payload.run_kind,
            run_kind
        );
    }
    let expected_subcommand = match run_kind {
        "app-run" => "app-run",
        "project-run" => "project-run",
        other => anyhow::bail!("unsupported plugin wrapper run_kind: {other}"),
    };
    if args_payload.args.len() < 2
        || args_payload.args[0] != "factory"
        || args_payload.args[1] != expected_subcommand
    {
        anyhow::bail!("plugin wrapper args must start with `factory {expected_subcommand}`");
    }
    for arg in &args_payload.args {
        let lowered = arg.to_ascii_lowercase();
        if lowered.contains("openai_api_key")
            || lowered.contains("anthropic_api_key")
            || lowered.contains("bearer ")
            || lowered.contains("--api-key")
        {
            anyhow::bail!("plugin wrapper args contain forbidden provider credential material");
        }
    }
    Ok(())
}

fn plugin_wrapper_args_resolve_relative_paths(args_file: &Path, args: &[String]) -> Vec<String> {
    let base = plugin_wrapper_args_path_base(args_file);
    let path_value_flags = [
        "--spec",
        "--target",
        "--provider-prompt-file",
        "--signing-key",
        "--out-dir",
        "--project-spec",
        "--project-plan",
        "--resume-from",
        "--app-run",
        "--factory-decision",
    ];
    let mut resolved = Vec::with_capacity(args.len());
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if path_value_flags.contains(&arg.as_str()) && index + 1 < args.len() {
            resolved.push(arg.clone());
            resolved.push(plugin_wrapper_resolve_path_arg(&base, &args[index + 1]));
            index += 2;
            continue;
        }
        if let Some((flag, value)) = arg.split_once('=') {
            if path_value_flags.contains(&flag) {
                resolved.push(format!(
                    "{flag}={}",
                    plugin_wrapper_resolve_path_arg(&base, value)
                ));
                index += 1;
                continue;
            }
        }
        resolved.push(arg.clone());
        index += 1;
    }
    resolved
}

fn plugin_wrapper_args_path_base(args_file: &Path) -> PathBuf {
    let parent = args_file.parent().unwrap_or_else(|| Path::new("."));
    if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "examples")
    {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

fn plugin_wrapper_resolve_path_arg(base: &Path, value: &str) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        value.to_string()
    } else {
        base.join(path).display().to_string()
    }
}
