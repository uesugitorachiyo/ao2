use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};

use crate::artifact_safety::factory_app_run_bundle_reject_secret_markers;
use crate::cli_util::{atomic_write_text, json_string, sanitize_greenfield_id, sha256_file};
use crate::factory_compat::{read_factory_compat_value, reject_factory_provider_api_key_auth};
use crate::factory_evaluator::{factory_project_acceptance_criteria, factory_project_spec_title};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};

pub(crate) struct FactoryProjectPlanOptions<'a> {
    pub(crate) project_spec: &'a Path,
    pub(crate) project_root: &'a Path,
    pub(crate) run_id: String,
    pub(crate) verifier_command: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt_dir: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) out: &'a Path,
}

pub(crate) struct FactoryProjectPlanValidateOptions<'a> {
    pub(crate) project_plan: &'a Path,
    pub(crate) project_root: &'a Path,
    pub(crate) out: &'a Path,
}

struct FactoryProjectPromptScaffold<'a> {
    project_title: &'a str,
    run_id: &'a str,
    step_id: &'a str,
    step_line: &'a str,
    spec_path: &'a Path,
    target_path: &'a Path,
    verifier_command: &'a str,
    provider: &'a str,
}

pub(crate) fn factory_project_plan_init_app_step_repo(target: &Path) -> Result<()> {
    let git_dir = target.join(".git");
    if git_dir.exists() {
        return Ok(());
    }
    for args in [
        &["init", "--quiet"][..],
        &["config", "user.email", "ao2-factory@example.invalid"][..],
        &["config", "user.name", "AO2 Factory"][..],
        &["config", "core.longpaths", "true"][..],
        &["add", "-A"][..],
        &["commit", "--quiet", "-m", "factory project app-step base"][..],
    ] {
        let output = ProcessCommand::new("git")
            .args(args)
            .current_dir(target)
            .output()
            .with_context(|| format!("run git {args:?} in {}", target.display()))?;
        if !output.status.success() {
            anyhow::bail!(
                "initialize factory project app-step git repo {} with git {:?}: {}",
                target.display(),
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    Ok(())
}

pub(crate) fn factory_project_plan_json(
    options: FactoryProjectPlanOptions<'_>,
) -> Result<serde_json::Value> {
    if !options.project_spec.is_file() {
        anyhow::bail!(
            "factory project spec does not exist: {}",
            options.project_spec.display()
        );
    }
    let project_spec_text = fs::read_to_string(options.project_spec)
        .with_context(|| format!("read project spec {}", options.project_spec.display()))?;
    reject_factory_provider_api_key_auth(
        "factory_project_spec",
        &serde_json::json!({ "project_spec": project_spec_text }),
    )?;

    fs::create_dir_all(options.project_root)
        .with_context(|| format!("create project root {}", options.project_root.display()))?;
    let project_root = fs::canonicalize(options.project_root)
        .with_context(|| format!("canonicalize {}", options.project_root.display()))?;
    let specs_dir = project_root.join("specs");
    let apps_dir = project_root.join("apps");
    let rubric_dir = project_root.join("rubrics");
    fs::create_dir_all(&specs_dir).with_context(|| format!("create {}", specs_dir.display()))?;
    fs::create_dir_all(&apps_dir).with_context(|| format!("create {}", apps_dir.display()))?;
    fs::create_dir_all(&rubric_dir).with_context(|| format!("create {}", rubric_dir.display()))?;
    let provider_prompt_dir = if let Some(dir) = options.provider_prompt_dir {
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let dir =
            fs::canonicalize(&dir).with_context(|| format!("canonicalize {}", dir.display()))?;
        if !dir.starts_with(&project_root) {
            anyhow::bail!(
                "provider prompt dir must stay under project root: {}",
                dir.display()
            );
        }
        Some(dir)
    } else {
        None
    };

    let run_id = sanitize_greenfield_id(&options.run_id);
    let project_title = factory_project_spec_title(&project_spec_text);
    let step_lines = factory_project_spec_step_lines(&project_spec_text);
    let mut app_steps = Vec::new();
    for (index, step_line) in step_lines.iter().enumerate() {
        let mut step_id = factory_project_step_id(step_line);
        if step_id.trim().is_empty() {
            step_id = format!("step-{index}");
        }
        if app_steps
            .iter()
            .any(|step: &serde_json::Value| json_string(step, "id") == step_id)
        {
            step_id = format!("{step_id}-{index}");
        }
        let spec_path = specs_dir.join(format!("{step_id}.md"));
        let target_path = apps_dir.join(&step_id);
        fs::create_dir_all(&target_path)
            .with_context(|| format!("create {}", target_path.display()))?;
        let target_readme = target_path.join("README.md");
        atomic_write_text(
            &target_readme,
            &format!("# {project_title} - {step_id}\n\nFactory project app-step target.\n"),
        )?;
        factory_project_plan_init_app_step_repo(&target_path)?;
        let step_spec = format!(
            "# {project_title} - {step_id}\n\nProject run: {run_id}\n\nSource project spec: {}\n\nApp step:\n- {step_line}\n\nAcceptance:\n- Implement this app step without changing AO trust boundaries.\n- Verifier command: `{}`.\n- Release acceptance remains owned by factory-v3 evaluator-closer.\n",
            options.project_spec.display(),
            options.verifier_command
        );
        atomic_write_text(&spec_path, &step_spec)?;
        factory_app_run_bundle_reject_secret_markers(&spec_path, "project-step-spec.md")?;

        let mut step = serde_json::json!({
            "id": step_id,
            "title": step_line,
            "spec": spec_path.display().to_string(),
            "target": target_path.display().to_string(),
            "verifier_command": options.verifier_command.clone(),
            "provider_profile": {
                "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
            }
        });
        if let Some(provider) = &options.provider {
            step["provider"] = serde_json::Value::String(provider.clone());
        }
        if let Some(prompt_dir) = &provider_prompt_dir {
            let prompt_path =
                prompt_dir.join(format!("{}-provider-prompt.sh", json_string(&step, "id")));
            let step_id = json_string(&step, "id");
            let prompt = factory_project_provider_prompt_scaffold(FactoryProjectPromptScaffold {
                project_title: &project_title,
                run_id: &run_id,
                step_id: &step_id,
                step_line,
                spec_path: &spec_path,
                target_path: &target_path,
                verifier_command: &options.verifier_command,
                provider: options.provider.as_deref().unwrap_or("scripted"),
            });
            atomic_write_text(&prompt_path, &prompt)?;
            factory_app_run_bundle_reject_secret_markers(&prompt_path, "project-provider-prompt")?;
            step["provider_prompt_file"] =
                serde_json::Value::String(prompt_path.display().to_string());
            step["provider_prompt_scaffold"] = serde_json::Value::Bool(true);
        }
        app_steps.push(step);
    }
    let rubric_path = rubric_dir.join(format!("{run_id}-acceptance-rubric.json"));
    let project_spec_sha256 = sha256_file(options.project_spec)?;
    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let acceptance_rubric = factory_project_acceptance_rubric_json(
        &project_spec_text,
        options.project_spec,
        &project_spec_sha256,
        &project_title,
        &run_id,
        &options.verifier_command,
        &step_lines,
        &trust_boundary,
        &rubric_path,
        options.signing_key.as_deref(),
        &options.signer_id,
    )?;
    let rubric_sha256 = sha256_file(&rubric_path)?;
    for step in app_steps.iter_mut() {
        step["acceptance_rubric"] = serde_json::Value::String(rubric_path.display().to_string());
        step["acceptance_rubric_sha256"] = serde_json::Value::String(rubric_sha256.clone());
    }
    let result = serde_json::json!({
        "schema_version": "ao2.factory-project-plan.v1",
        "status": "accepted",
        "run_id": options.run_id,
        "project_title": project_title,
        "project_spec": options.project_spec.display().to_string(),
        "project_spec_sha256": project_spec_sha256,
        "project_root": project_root.display().to_string(),
        "acceptance_rubric": acceptance_rubric,
        "acceptance_rubric_sha256": rubric_sha256.clone(),
        "app_steps": app_steps,
        "factory_replacement_boundary": {
            "ao2_execution_owner": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "artifacts": {
            "project_plan": options.out.display().to_string(),
            "project_spec": options.project_spec.display().to_string(),
            "project_root": project_root.display().to_string(),
            "acceptance_rubric": rubric_path.display().to_string(),
            "acceptance_rubric_sha256": rubric_sha256.clone()
        },
        "trust_boundary": trust_boundary
    });
    if let Some(parent) = options
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write_text(options.out, &serde_json::to_string_pretty(&result)?)?;
    factory_app_run_bundle_reject_secret_markers(options.out, "project-plan.json")?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn factory_project_acceptance_rubric_json(
    project_spec_text: &str,
    project_spec: &Path,
    project_spec_sha256: &str,
    project_title: &str,
    run_id: &str,
    verifier_command: &str,
    step_lines: &[String],
    trust_boundary: &serde_json::Value,
    out: &Path,
    signing_key: Option<&Path>,
    signer_id: &str,
) -> Result<serde_json::Value> {
    if let Some(parent) = out.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let signed_payload_path = out.with_extension("signed-payload.json");
    let signature_path = out.with_extension("json.sig");
    let public_key_path = out.with_extension("public.pem");
    let pass_fail_criteria =
        factory_project_acceptance_criteria(project_spec_text, verifier_command);
    let step_criteria = step_lines
        .iter()
        .enumerate()
        .map(|(index, step)| {
            serde_json::json!({
                "index": index,
                "step": step,
                "must_pass": [
                    "implements the requested app step",
                    "preserves AO2/factory trust-boundary fields",
                    format!("verifier command exits 0: {verifier_command}")
                ]
            })
        })
        .collect::<Vec<_>>();
    let mut rubric = serde_json::json!({
        "schema_version": "ao2.factory-acceptance-rubric.v1",
        "status": "accepted",
        "run_id": run_id,
        "project_title": project_title,
        "source_project_spec": project_spec.display().to_string(),
        "source_project_spec_sha256": project_spec_sha256,
        "verifier_grade_pass_fail_criteria": pass_fail_criteria,
        "thresholds": {
            "failed_step_count": 0,
            "verifier_exit_code": 0,
            "required_signature_status": "signed"
        },
        "must_have_artifacts": [
            "project-plan/project-plan.json",
            "project-plan/project-plan-validation.json",
            "project-run/factory-project-run.json",
            "project-run/factory-project-run-state.json",
            "release-review/release-review-package.tgz"
        ],
        "step_criteria": step_criteria,
        "trust_boundary": trust_boundary
    });
    atomic_write_text(
        &signed_payload_path,
        &serde_json::to_string_pretty(&rubric)?,
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &signed_payload_path,
        "acceptance-rubric-signed-payload.json",
    )?;
    let signature = match signing_key {
        Some(key_path) => {
            derive_public_key_from_private_key(key_path, &public_key_path)?;
            sign_file_with_private_key(key_path, &signed_payload_path, &signature_path)?;
            let signature_verified =
                verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?;
            serde_json::json!({
                "schema_version": "ao2.factory-acceptance-rubric-signature.v1",
                "signature_algorithm": "RSA/SHA-256",
                "signer_id": signer_id,
                "signed_payload": "acceptance_rubric_without_signature_field",
                "signed_payload_path": signed_payload_path.display().to_string(),
                "signed_payload_sha256": sha256_file(&signed_payload_path)?,
                "signature_path": signature_path.display().to_string(),
                "signature_sha256": sha256_file(&signature_path)?,
                "public_key_path": public_key_path.display().to_string(),
                "public_key_sha256": sha256_file(&public_key_path)?,
                "signature_status": "signed",
                "signature_verified": signature_verified
            })
        }
        None => serde_json::json!({
            "schema_version": "ao2.factory-acceptance-rubric-signature.v1",
            "signed_payload": "acceptance_rubric_without_signature_field",
            "signed_payload_path": signed_payload_path.display().to_string(),
            "signed_payload_sha256": sha256_file(&signed_payload_path)?,
            "signature_status": "unsigned",
            "signature_verified": false
        }),
    };
    rubric["signature"] = signature;
    atomic_write_text(out, &serde_json::to_string_pretty(&rubric)?)?;
    factory_app_run_bundle_reject_secret_markers(out, "acceptance-rubric.json")?;
    Ok(rubric)
}

fn factory_project_provider_prompt_scaffold(input: FactoryProjectPromptScaffold<'_>) -> String {
    if input.provider == "scripted" {
        return format!(
            "#!/bin/sh\n: <<'AO2_PROVIDER_PROMPT_CONTEXT'\nAO2 provider prompt scaffold\n\nProject: {}\nRun ID: {}\nStep ID: {}\nProvider: {}\n\nImplement this project app step in the target directory.\n\nApp step:\n- {}\n\nInputs:\n- Step spec: {}\n- Target directory: {}\n- Verifier command: `{}`\n\nNon-negotiable boundaries:\n- Use local OAuth CLI only; do not request or embed API keys.\n- Do not print bearer tokens, cookies, credentials, or secrets.\n- Preserve release acceptance ownership: factory-v3 evaluator-closer.\n- Keep ao2-control-plane as a read-only observer after signed evidence exists.\n- Do not make the control plane approve release or mutate AO artifacts.\nAO2_PROVIDER_PROMPT_CONTEXT\nprintf 'Summary: AO2 scripted provider scaffold executed for {}\\n'\nprintf 'Changed files: none\\n'\nprintf 'Verification: delegated to configured verifier command\\n'\nprintf 'Input tokens: 0\\n'\n",
            input.project_title,
            input.run_id,
            input.step_id,
            input.provider,
            input.step_line,
            input.spec_path.display(),
            input.target_path.display(),
            input.verifier_command,
            input.step_id
        );
    }
    format!(
        "# AO2 provider prompt scaffold\n\nProject: {}\nRun ID: {}\nStep ID: {}\nProvider: {}\n\nImplement this project app step in the target directory.\n\nApp step:\n- {}\n\nInputs:\n- Step spec: {}\n- Target directory: {}\n- Verifier command: `{}`\n\nNon-negotiable boundaries:\n- Use local OAuth CLI only; do not request or embed API keys.\n- Do not print bearer tokens, cookies, credentials, or secrets.\n- Preserve release acceptance ownership: factory-v3 evaluator-closer.\n- Keep ao2-control-plane as a read-only observer after signed evidence exists.\n- Do not make the control plane approve release or mutate AO artifacts.\n\nExpected response:\n- Summary of changes.\n- Files changed.\n- Verification run and result.\n- Blockers, if any.\n",
        input.project_title,
        input.run_id,
        input.step_id,
        input.provider,
        input.step_line,
        input.spec_path.display(),
        input.target_path.display(),
        input.verifier_command
    )
}

pub(crate) fn factory_project_plan_validate_json(
    options: FactoryProjectPlanValidateOptions<'_>,
) -> Result<serde_json::Value> {
    if !options.project_plan.is_file() {
        anyhow::bail!(
            "factory project plan does not exist: {}",
            options.project_plan.display()
        );
    }
    let project_plan = read_factory_compat_value(options.project_plan)?;
    reject_factory_provider_api_key_auth("factory_project_plan_validation", &project_plan)?;
    fs::create_dir_all(options.project_root)
        .with_context(|| format!("create project root {}", options.project_root.display()))?;
    let project_root = fs::canonicalize(options.project_root)
        .with_context(|| format!("canonicalize {}", options.project_root.display()))?;
    let plan_base = options
        .project_plan
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut blockers = Vec::<String>::new();
    let mut seen_step_ids = BTreeSet::<String>::new();
    let mut app_step_count = 0usize;
    let schema_ok = json_string(&project_plan, "schema_version") == "ao2.factory-project-plan.v1";
    if !schema_ok {
        blockers
            .push("project plan schema_version must be ao2.factory-project-plan.v1".to_string());
    }
    let steps: &[serde_json::Value] = match project_plan["app_steps"].as_array() {
        Some(steps) if !steps.is_empty() => steps.as_slice(),
        Some(_) => {
            blockers.push("project plan app_steps must not be empty".to_string());
            &[]
        }
        None => {
            blockers.push("project plan must contain app_steps array".to_string());
            &[]
        }
    };
    let mut all_paths_within_project_root = true;
    let mut all_required_files_exist = true;
    let mut no_duplicate_step_ids = true;
    let mut no_secret_markers = true;

    for (index, step) in steps.iter().enumerate() {
        app_step_count += 1;
        let step_id = json_string(step, "id");
        if step_id.trim().is_empty() {
            blockers.push(format!("app_steps[{index}].id must not be empty"));
            no_duplicate_step_ids = false;
        } else if !seen_step_ids.insert(step_id.clone()) {
            blockers.push(format!("duplicate app step id: {step_id}"));
            no_duplicate_step_ids = false;
        }
        for (key, expected_kind) in [("spec", "file"), ("target", "dir")] {
            match factory_project_plan_validate_path(
                step,
                plan_base,
                &project_root,
                key,
                expected_kind,
            ) {
                Ok(path) => {
                    if key == "spec" {
                        if let Err(err) =
                            factory_app_run_bundle_reject_secret_markers(&path, "project-step-spec")
                        {
                            blockers.push(format!(
                                "app_steps[{index}].{key} contains secret marker: {err}"
                            ));
                            no_secret_markers = false;
                        }
                    }
                }
                Err(err) => {
                    let message = err.to_string();
                    if message.contains("escapes project root") {
                        all_paths_within_project_root = false;
                    }
                    if message.contains("does not exist")
                        || message.contains("must be a file")
                        || message.contains("must be a directory")
                    {
                        all_required_files_exist = false;
                    }
                    blockers.push(format!("app_steps[{index}].{key} {message}"));
                }
            }
        }
        if !json_string(step, "provider_prompt_file").trim().is_empty() {
            match factory_project_plan_validate_path(
                step,
                plan_base,
                &project_root,
                "provider_prompt_file",
                "file",
            ) {
                Ok(path) => {
                    if let Err(err) =
                        factory_app_run_bundle_reject_secret_markers(&path, "provider-prompt")
                    {
                        blockers.push(format!(
                            "app_steps[{index}].provider_prompt_file contains secret marker: {err}"
                        ));
                        no_secret_markers = false;
                    }
                }
                Err(err) => {
                    let message = err.to_string();
                    if message.contains("escapes project root") {
                        all_paths_within_project_root = false;
                    }
                    if message.contains("does not exist") || message.contains("must be a file") {
                        all_required_files_exist = false;
                    }
                    blockers.push(format!("app_steps[{index}].provider_prompt_file {message}"));
                }
            }
        }
    }

    let control_plane_remains_observer =
        project_plan["factory_replacement_boundary"]["control_plane_approves_release"].as_bool()
            == Some(false)
            && project_plan["factory_replacement_boundary"]["mutates_ao_artifacts"].as_bool()
                == Some(false)
            && json_string(
                &project_plan["factory_replacement_boundary"],
                "control_plane_role",
            )
            .contains("read_only_observer");
    if !control_plane_remains_observer {
        blockers.push(
            "control plane must remain read-only observer and must not approve release".to_string(),
        );
    }
    let release_acceptance_owner_ok = json_string(
        &project_plan["factory_replacement_boundary"],
        "release_acceptance_owner",
    ) == "factory-v3 evaluator-closer";
    if !release_acceptance_owner_ok {
        blockers.push("release_acceptance_owner must be factory-v3 evaluator-closer".to_string());
    }
    let rubric = match factory_project_acceptance_rubric_validate(&project_plan, plan_base) {
        Ok(rubric) => {
            if !rubric["accepted"].as_bool().unwrap_or(false) {
                for blocker in rubric["blockers"].as_array().into_iter().flatten() {
                    blockers.push(format!(
                        "acceptance rubric {}",
                        blocker.as_str().unwrap_or("is invalid")
                    ));
                }
            }
            rubric
        }
        Err(err) => {
            blockers.push(format!("acceptance rubric {err}"));
            serde_json::json!({
                "accepted": false,
                "blockers": [err.to_string()]
            })
        }
    };
    let signed_acceptance_rubric = rubric["accepted"].as_bool().unwrap_or(false);
    let status = if blockers.is_empty() {
        "accepted"
    } else {
        "rejected"
    };
    let result = serde_json::json!({
        "schema_version": "ao2.factory-project-plan-validation.v1",
        "status": status,
        "project_plan": options.project_plan.display().to_string(),
        "project_plan_sha256": sha256_file(options.project_plan)?,
        "project_root": project_root.display().to_string(),
        "app_step_count": app_step_count,
        "checks": {
            "schema_version": schema_ok,
            "app_steps_non_empty": !steps.is_empty(),
            "no_duplicate_step_ids": no_duplicate_step_ids,
            "all_paths_within_project_root": all_paths_within_project_root,
            "all_required_files_exist": all_required_files_exist,
            "no_secret_markers": no_secret_markers,
            "control_plane_remains_observer": control_plane_remains_observer,
            "release_acceptance_owner": release_acceptance_owner_ok,
            "signed_acceptance_rubric": signed_acceptance_rubric
        },
        "rubric": rubric,
        "blockers": blockers,
        "artifacts": {
            "validation": options.out.display().to_string(),
            "project_plan": options.project_plan.display().to_string(),
            "project_root": project_root.display().to_string()
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    if let Some(parent) = options
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write_text(options.out, &serde_json::to_string_pretty(&result)?)?;
    factory_app_run_bundle_reject_secret_markers(options.out, "project-plan-validation.json")?;
    if status != "accepted" {
        anyhow::bail!(
            "factory project plan validation rejected: {}",
            options.out.display()
        );
    }
    Ok(result)
}

pub(crate) fn factory_project_acceptance_rubric_validate(
    project_plan: &serde_json::Value,
    plan_base: &Path,
) -> Result<serde_json::Value> {
    let rubric_raw = json_string(&project_plan["artifacts"], "acceptance_rubric");
    if rubric_raw.trim().is_empty() {
        anyhow::bail!("path is missing from artifacts.acceptance_rubric");
    }
    let rubric_path = factory_project_plan_resolve_existing_file(plan_base, &rubric_raw)?;
    let actual_sha = sha256_file(&rubric_path)?;
    let mut blockers = Vec::<String>::new();
    for (label, expected) in [
        (
            "artifacts.acceptance_rubric_sha256",
            json_string(&project_plan["artifacts"], "acceptance_rubric_sha256"),
        ),
        (
            "acceptance_rubric_sha256",
            json_string(project_plan, "acceptance_rubric_sha256"),
        ),
    ] {
        if expected.trim().is_empty() {
            blockers.push(format!("{label} is missing"));
        } else if expected != actual_sha {
            blockers.push(format!("{label} does not match rubric bytes"));
        }
    }
    let rubric = read_factory_compat_value(&rubric_path)?;
    reject_factory_provider_api_key_auth("factory_acceptance_rubric", &rubric)?;
    if json_string(&rubric, "schema_version") != "ao2.factory-acceptance-rubric.v1" {
        blockers.push("schema_version must be ao2.factory-acceptance-rubric.v1".to_string());
    }
    if json_string(&rubric, "source_project_spec_sha256")
        != json_string(project_plan, "project_spec_sha256")
    {
        blockers.push("source_project_spec_sha256 must match project plan".to_string());
    }
    if json_string(&rubric["trust_boundary"], "release_acceptance_owner")
        != "factory-v3 evaluator-closer"
    {
        blockers
            .push("release_acceptance_owner must remain factory-v3 evaluator-closer".to_string());
    }
    if rubric["trust_boundary"]["control_plane_approves_release"].as_bool() != Some(false)
        || rubric["trust_boundary"]["mutates_ao_artifacts"].as_bool() != Some(false)
    {
        blockers.push("control plane must remain observer-only".to_string());
    }
    let signature = &rubric["signature"];
    if json_string(signature, "signature_status") != "signed" {
        blockers.push("signature_status must be signed".to_string());
    }
    if signature["signature_verified"].as_bool() != Some(true) {
        blockers.push("signature_verified must be true".to_string());
    }
    let signed_payload_path = factory_project_plan_resolve_existing_file(
        rubric_path.parent().unwrap_or_else(|| Path::new(".")),
        &json_string(signature, "signed_payload_path"),
    );
    let signature_path = factory_project_plan_resolve_existing_file(
        rubric_path.parent().unwrap_or_else(|| Path::new(".")),
        &json_string(signature, "signature_path"),
    );
    let public_key_path = factory_project_plan_resolve_existing_file(
        rubric_path.parent().unwrap_or_else(|| Path::new(".")),
        &json_string(signature, "public_key_path"),
    );
    match (signed_payload_path, signature_path, public_key_path) {
        (Ok(signed_payload_path), Ok(signature_path), Ok(public_key_path)) => {
            let expected_payload_sha = json_string(signature, "signed_payload_sha256");
            if expected_payload_sha.trim().is_empty()
                || sha256_file(&signed_payload_path)? != expected_payload_sha
            {
                blockers.push("signed payload sha256 mismatch".to_string());
            }
            let verified =
                verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?;
            if !verified {
                blockers.push("cryptographic signature verification failed".to_string());
            }
        }
        _ => blockers.push("signature sidecar paths must exist".to_string()),
    }
    let accepted = blockers.is_empty();
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-acceptance-rubric-validation.v1",
        "accepted": accepted,
        "path": rubric_path.display().to_string(),
        "sha256": actual_sha,
        "rubric_schema": rubric["schema_version"].clone(),
        "signature_status": json_string(signature, "signature_status"),
        "signature_verified": accepted,
        "blockers": blockers
    }))
}

pub(crate) fn factory_project_plan_resolve_existing_file(
    base: &Path,
    raw: &str,
) -> Result<PathBuf> {
    if raw.trim().is_empty() {
        anyhow::bail!("path is missing");
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    if !path.is_file() {
        anyhow::bail!("not a file: {}", path.display());
    }
    Ok(path)
}

fn factory_project_plan_validate_path(
    step: &serde_json::Value,
    base: &Path,
    project_root: &Path,
    key: &str,
    expected_kind: &str,
) -> Result<PathBuf> {
    let raw = json_string(step, key);
    if raw.trim().is_empty() {
        anyhow::bail!("is missing");
    }
    let raw_path = PathBuf::from(&raw);
    if raw_path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        anyhow::bail!("escapes project root: {raw}");
    }
    let path = if raw_path.is_absolute() {
        raw_path
    } else {
        base.join(raw_path)
    };
    if path.is_absolute() && !path.starts_with(project_root) {
        anyhow::bail!("escapes project root: {}", path.display());
    }
    if !path.exists() {
        anyhow::bail!("does not exist: {}", path.display());
    }
    let canonical =
        fs::canonicalize(&path).with_context(|| format!("canonicalize {}", path.display()))?;
    if !canonical.starts_with(project_root) {
        anyhow::bail!("escapes project root: {}", canonical.display());
    }
    match expected_kind {
        "file" if !canonical.is_file() => {
            anyhow::bail!("must be a file: {}", canonical.display());
        }
        "dir" if !canonical.is_dir() => {
            anyhow::bail!("must be a directory: {}", canonical.display());
        }
        _ => {}
    }
    Ok(canonical)
}

fn factory_project_spec_step_lines(project_spec_text: &str) -> Vec<String> {
    let mut in_steps = false;
    let mut steps = Vec::new();
    for line in project_spec_text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if trimmed.starts_with('#') {
            in_steps = lower.contains("app steps")
                || lower.contains("application steps")
                || lower.contains("project steps");
            continue;
        }
        if in_steps {
            if let Some(step) = trimmed.strip_prefix("- ") {
                let step = step.trim();
                if !step.is_empty() {
                    steps.push(step.to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('-') {
                in_steps = false;
            }
        }
    }
    if steps.is_empty() {
        steps.push("Application workflow".to_string());
    }
    steps
}

fn factory_project_step_id(step_line: &str) -> String {
    let lower = step_line.to_ascii_lowercase();
    let candidate = lower
        .split(" workflow")
        .next()
        .unwrap_or(&lower)
        .split(" step")
        .next()
        .unwrap_or(&lower)
        .split(':')
        .next()
        .unwrap_or(&lower)
        .trim()
        .to_string();
    sanitize_greenfield_id(&candidate)
}
