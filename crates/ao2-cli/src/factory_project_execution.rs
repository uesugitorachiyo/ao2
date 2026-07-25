use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::artifact_safety::factory_app_run_bundle_reject_secret_markers;
use crate::cli_util::{
    atomic_write_text, create_tar_gz, json_string, now_unix_ms, sanitize_greenfield_id, sha256_file,
};
use crate::factory_app_run::{
    factory_app_run_bundle_json, factory_app_run_bundle_validate, factory_app_run_json,
    FactoryAppRunOptions,
};
use crate::factory_compat::{read_factory_compat_value, reject_factory_provider_api_key_auth};
use crate::factory_project_planning::{
    factory_project_acceptance_rubric_validate, factory_project_plan_resolve_existing_file,
};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};

pub(crate) struct FactoryProjectRunOptions<'a> {
    pub(crate) project_spec: &'a Path,
    pub(crate) project_plan: Option<&'a Path>,
    pub(crate) resume_from: Option<&'a Path>,
    pub(crate) app_runs: &'a [PathBuf],
    pub(crate) run_id: String,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) out_dir: &'a Path,
}

pub(crate) struct FactoryProjectAcceptanceReviewOptions<'a> {
    pub(crate) project_run: &'a Path,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) out: &'a Path,
}

pub(crate) fn factory_project_run_json(
    options: FactoryProjectRunOptions<'_>,
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

    fs::create_dir_all(options.out_dir).with_context(|| {
        format!(
            "create factory project-run out dir {}",
            options.out_dir.display()
        )
    })?;
    let out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize factory project-run out dir {}",
            options.out_dir.display()
        )
    })?;
    let run_id = sanitize_greenfield_id(&options.run_id);
    let project_run_path = out_dir.join(format!("{run_id}-factory-project-run.json"));
    let project_state_path = out_dir.join(format!("{run_id}-factory-project-run-state.json"));
    let release_review_package = out_dir.join(format!("{run_id}-release-review-package.tgz"));
    let bundles_dir = out_dir.join("app-run-bundles");
    fs::create_dir_all(&bundles_dir)
        .with_context(|| format!("create {}", bundles_dir.display()))?;

    let mut app_run_paths = options.app_runs.to_vec();
    let resume_state = if let Some(resume_from) = options.resume_from {
        let state = read_factory_compat_value(resume_from)?;
        reject_factory_provider_api_key_auth("factory_project_run_resume_state", &state)?;
        Some(state)
    } else {
        None
    };
    let mut project_plan = serde_json::Value::Null;
    let mut acceptance_rubric = serde_json::Value::Null;
    let mut acceptance_rubric_sha256 = String::new();
    let mut project_steps = Vec::<serde_json::Value>::new();
    let mut dispatched_project_plan = false;
    let mut reused_resume_state = false;
    let mut failed_step_count = 0usize;
    if let Some(project_plan_path) = options.project_plan {
        project_plan = read_factory_compat_value(project_plan_path)?;
        reject_factory_provider_api_key_auth("factory_project_plan", &project_plan)?;
        let plan_base = project_plan_path.parent().unwrap_or_else(|| Path::new("."));
        acceptance_rubric = factory_project_acceptance_rubric_validate(&project_plan, plan_base)
            .with_context(|| {
                format!(
                    "factory project-run requires a signed AO2 acceptance rubric in {}",
                    project_plan_path.display()
                )
            })?;
        if !acceptance_rubric["accepted"].as_bool().unwrap_or(false) {
            anyhow::bail!(
                "factory project-run requires accepted signed AO2 acceptance rubric: {}",
                acceptance_rubric["blockers"]
            );
        }
        acceptance_rubric_sha256 = json_string(&acceptance_rubric, "sha256");
        let steps = project_plan["app_steps"].as_array().ok_or_else(|| {
            anyhow!(
                "factory project plan must contain app_steps array: {}",
                project_plan_path.display()
            )
        })?;
        if steps.is_empty() {
            anyhow::bail!("factory project plan app_steps must not be empty");
        }
        let step_root = out_dir.join("app-run-steps");
        fs::create_dir_all(&step_root)
            .with_context(|| format!("create {}", step_root.display()))?;
        for (index, step) in steps.iter().enumerate() {
            let step_id_raw = json_string(step, "id");
            let step_id = if step_id_raw.trim().is_empty() {
                format!("step-{index}")
            } else {
                sanitize_greenfield_id(&step_id_raw)
            };
            if let Some(resumed_step) =
                factory_project_resume_step(resume_state.as_ref(), &step_id)?
            {
                app_run_paths.push(PathBuf::from(json_string(&resumed_step, "app_run")));
                let mut resumed_step = resumed_step;
                resumed_step["reused_from_resume"] = serde_json::Value::Bool(true);
                project_steps.push(resumed_step);
                reused_resume_state = true;
                continue;
            }
            let spec = factory_project_plan_path(step, plan_base, "spec")?;
            let target = factory_project_plan_path(step, plan_base, "target")?;
            let provider_prompt_file =
                factory_project_plan_optional_path(step, plan_base, "provider_prompt_file")?;
            let verifier_command = json_string(step, "verifier_command");
            let verifier_command = if verifier_command.trim().is_empty() {
                "npm run verify".to_string()
            } else {
                verifier_command
            };
            let provider = factory_project_plan_optional_string(step, "provider");
            let provider_prompt = factory_project_plan_optional_string(step, "provider_prompt");
            let provider_max_budget_usd = step
                .get("provider_max_budget_usd")
                .and_then(|value| value.as_f64());
            let step_run_id = if resume_state.is_some() {
                format!("{run_id}-{step_id}-resume")
            } else {
                format!("{run_id}-{step_id}")
            };
            let step_out_dir = step_root.join(&step_id);
            let step_signer_id = factory_project_plan_optional_string(step, "signer_id")
                .unwrap_or_else(|| format!("{}-{step_id}", options.signer_id));
            let app_run_result = factory_app_run_json(FactoryAppRunOptions {
                spec: &spec,
                target: &target,
                run_id: step_run_id,
                verifier_command,
                provider,
                provider_prompt,
                provider_prompt_file,
                provider_max_budget_usd,
                factory_decision: None,
                signing_key: options.signing_key.clone(),
                signer_id: step_signer_id,
                max_repair_attempts: options.max_repair_attempts,
                out_dir: &step_out_dir,
            });
            match app_run_result {
                Ok(app_run) => {
                    let app_run_path =
                        PathBuf::from(json_string(&app_run["artifacts"], "factory_app_run"));
                    let app_run_sha256 = if app_run_path.is_file() {
                        Some(sha256_file(&app_run_path)?)
                    } else {
                        None
                    };
                    let status = json_string(&app_run, "status");
                    if status == "accepted" {
                        app_run_paths.push(app_run_path.clone());
                    } else {
                        failed_step_count += 1;
                    }
                    project_steps.push(serde_json::json!({
                        "index": index,
                        "id": step_id,
                        "status": status,
                        "app_run": app_run_path.display().to_string(),
                        "app_run_sha256": app_run_sha256,
                        "bundle": serde_json::Value::Null,
                        "bundle_sha256": serde_json::Value::Null,
                        "acceptance_rubric_sha256": acceptance_rubric_sha256.clone(),
                        "reused_from_resume": false
                    }));
                }
                Err(error) => {
                    failed_step_count += 1;
                    project_steps.push(serde_json::json!({
                        "index": index,
                        "id": step_id,
                        "status": "rejected",
                        "error": error.to_string(),
                        "app_run": serde_json::Value::Null,
                        "app_run_sha256": serde_json::Value::Null,
                        "bundle": serde_json::Value::Null,
                        "bundle_sha256": serde_json::Value::Null,
                        "acceptance_rubric_sha256": acceptance_rubric_sha256.clone(),
                        "reused_from_resume": false
                    }));
                }
            }
        }
        dispatched_project_plan = true;
    }
    if app_run_paths.is_empty() {
        anyhow::bail!(
            "factory project-run requires at least one --app-run or project-plan app step"
        );
    }

    let mut app_run_items = Vec::new();
    for (index, app_run_path) in app_run_paths.iter().enumerate() {
        let app_run = read_factory_compat_value(app_run_path)?;
        factory_app_run_bundle_validate(&app_run)?;
        let bundle_path = bundles_dir.join(format!("{index}-app-run-evidence-bundle.tgz"));
        let bundle = factory_app_run_bundle_json(app_run_path, &bundle_path)?;
        app_run_items.push(serde_json::json!({
            "index": index,
            "run_id": json_string(&app_run, "run_id"),
            "app_run": app_run_path,
            "bundle": bundle["archive"].clone(),
            "bundle_sha256": bundle["sha256"].clone(),
            "acceptance_rubric_sha256": if acceptance_rubric_sha256.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(acceptance_rubric_sha256.clone())
            },
            "release_review_ready": app_run["release_review"]["ready"].as_bool().unwrap_or(false),
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        }));
    }
    for step in project_steps.iter_mut() {
        if json_string(step, "status") != "accepted" {
            continue;
        }
        let step_app_run = json_string(step, "app_run");
        if let Some(item) = app_run_items
            .iter()
            .find(|item| json_string(item, "app_run") == step_app_run)
        {
            step["bundle"] = item["bundle"].clone();
            step["bundle_sha256"] = item["bundle_sha256"].clone();
        }
    }

    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    if failed_step_count > 0 {
        let state = serde_json::json!({
            "schema_version": "ao2.factory-project-run-state.v1",
            "status": "rejected",
            "run_id": options.run_id.clone(),
            "created_at": created_at,
            "project_spec": options.project_spec.display().to_string(),
            "project_plan": options.project_plan.map(|path| path.display().to_string()),
            "acceptance_rubric": acceptance_rubric.clone(),
            "step_count": project_steps.len(),
            "accepted_step_count": app_run_items.len(),
            "failed_step_count": failed_step_count,
            "steps": project_steps,
            "app_runs": app_run_items,
            "trust_boundary": trust_boundary
        });
        atomic_write_text(&project_state_path, &serde_json::to_string_pretty(&state)?)?;
        factory_app_run_bundle_reject_secret_markers(
            &project_state_path,
            "factory-project-run-state.json",
        )?;
        let result = serde_json::json!({
            "schema_version": "ao2.factory-project-run.v1",
            "status": "rejected",
            "run_id": options.run_id.clone(),
            "created_at": created_at,
            "app_run_count": app_run_items.len(),
            "step_count": state["step_count"].clone(),
            "failed_step_count": failed_step_count,
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
                "factory_project_run": project_run_path.display().to_string(),
                "factory_project_run_state": project_state_path.display().to_string(),
                "project_spec": options.project_spec.display().to_string(),
                "project_plan": options.project_plan.map(|path| path.display().to_string()),
                "acceptance_rubric": if acceptance_rubric["path"].is_string() {
                    acceptance_rubric["path"].clone()
                } else {
                    serde_json::Value::Null
                },
                "acceptance_rubric_sha256": if acceptance_rubric_sha256.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(acceptance_rubric_sha256.clone())
                },
                "release_review_package": serde_json::Value::Null
            },
            "app_runs": state["app_runs"].clone(),
            "project_steps": state["steps"].clone(),
            "project_plan": project_plan,
            "acceptance_rubric": acceptance_rubric,
            "release_review": {
                "ready": false,
                "package": serde_json::Value::Null,
                "app_run_count": app_run_items.len(),
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false
            },
            "project_run_checklist": {
                "ao2_ingested_project_spec": true,
                "ao2_dispatched_project_plan": dispatched_project_plan,
                "ao2_reused_resume_state": reused_resume_state,
                "ao2_collected_app_run_bundles": !app_run_items.is_empty(),
                "ao2_preserved_partial_evidence": true,
                "release_review_package_ready": false,
                "factory_v3_drives_workflow": false,
                "factory_v3_role": "parity_oracle_only",
                "control_plane_role": "read_only_observer_after_signed_evidence",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false,
                "mutates_ao_artifacts": false
            },
            "trust_boundary": trust_boundary
        });
        atomic_write_text(&project_run_path, &serde_json::to_string_pretty(&result)?)?;
        factory_app_run_bundle_reject_secret_markers(&project_run_path, "project-run.json")?;
        return Ok(result);
    }
    let result = serde_json::json!({
        "schema_version": "ao2.factory-project-run.v1",
        "status": "accepted",
        "run_id": options.run_id.clone(),
        "created_at": created_at,
        "app_run_count": app_run_items.len(),
        "step_count": project_steps.len(),
        "failed_step_count": 0,
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
            "factory_project_run": project_run_path.display().to_string(),
            "factory_project_run_state": project_state_path.display().to_string(),
            "project_spec": options.project_spec.display().to_string(),
            "project_plan": options.project_plan.map(|path| path.display().to_string()),
            "acceptance_rubric": if acceptance_rubric["path"].is_string() {
                acceptance_rubric["path"].clone()
            } else {
                serde_json::Value::Null
            },
            "acceptance_rubric_sha256": if acceptance_rubric_sha256.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(acceptance_rubric_sha256.clone())
            },
            "release_review_package": release_review_package.display().to_string()
        },
        "app_runs": app_run_items,
        "project_steps": project_steps,
        "project_plan": project_plan,
        "acceptance_rubric": acceptance_rubric,
        "release_review": {
            "ready": true,
            "package": release_review_package.display().to_string(),
            "app_run_count": app_run_paths.len(),
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        },
        "project_run_checklist": {
            "ao2_ingested_project_spec": true,
            "ao2_dispatched_project_plan": dispatched_project_plan,
            "ao2_reused_resume_state": reused_resume_state,
            "ao2_collected_app_run_bundles": true,
            "ao2_preserved_partial_evidence": false,
            "release_review_package_ready": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        },
        "trust_boundary": trust_boundary
    });
    let state = serde_json::json!({
        "schema_version": "ao2.factory-project-run-state.v1",
        "status": "accepted",
        "run_id": result["run_id"].clone(),
        "created_at": created_at,
        "project_spec": options.project_spec.display().to_string(),
        "project_plan": options.project_plan.map(|path| path.display().to_string()),
        "acceptance_rubric": acceptance_rubric.clone(),
        "step_count": result["step_count"].clone(),
        "accepted_step_count": result["app_run_count"].clone(),
        "failed_step_count": 0,
        "steps": result["project_steps"].clone(),
        "app_runs": result["app_runs"].clone(),
        "trust_boundary": result["trust_boundary"].clone()
    });
    atomic_write_text(&project_state_path, &serde_json::to_string_pretty(&state)?)?;
    factory_app_run_bundle_reject_secret_markers(
        &project_state_path,
        "factory-project-run-state.json",
    )?;
    atomic_write_text(&project_run_path, &serde_json::to_string_pretty(&result)?)?;
    factory_app_run_bundle_reject_secret_markers(&project_run_path, "project-run.json")?;

    let package = factory_project_run_package_json(
        &result,
        options.project_spec,
        options.project_plan,
        &app_run_paths,
        &release_review_package,
    )?;
    let mut result = result;
    result["release_review_package"] = package;
    atomic_write_text(&project_run_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn factory_project_acceptance_review_json(
    options: FactoryProjectAcceptanceReviewOptions<'_>,
) -> Result<serde_json::Value> {
    if !options.project_run.is_file() {
        anyhow::bail!(
            "factory project acceptance review requires --project-run file: {}",
            options.project_run.display()
        );
    }
    if let Some(parent) = options
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let project_run = read_factory_compat_value(options.project_run)?;
    reject_factory_provider_api_key_auth("factory_project_acceptance_review", &project_run)?;
    let mut blockers = Vec::<String>::new();
    if json_string(&project_run, "schema_version") != "ao2.factory-project-run.v1" {
        blockers.push("project_run schema_version must be ao2.factory-project-run.v1".to_string());
    }
    if json_string(&project_run, "status") != "accepted" {
        blockers.push("project_run status must be accepted".to_string());
    }
    if project_run["factory_replacement_boundary"]["control_plane_approves_release"].as_bool()
        != Some(false)
        || project_run["factory_replacement_boundary"]["mutates_ao_artifacts"].as_bool()
            != Some(false)
    {
        blockers.push("control plane must not approve releases or mutate AO artifacts".to_string());
    }
    if json_string(
        &project_run["factory_replacement_boundary"],
        "release_acceptance_owner",
    ) != "factory-v3 evaluator-closer"
    {
        blockers.push("release_acceptance_owner must be factory-v3 evaluator-closer".to_string());
    }

    let project_base = options
        .project_run
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let project_plan_base = project_run["artifacts"]["project_plan"]
        .as_str()
        .and_then(|raw| Path::new(raw).parent())
        .unwrap_or(project_base);
    let mut rubric = match factory_project_acceptance_rubric_validate(
        &project_run["project_plan"],
        project_plan_base,
    ) {
        Ok(rubric) => {
            if !rubric["accepted"].as_bool().unwrap_or(false) {
                for blocker in rubric["blockers"].as_array().into_iter().flatten() {
                    blockers.push(format!(
                        "rubric {}",
                        blocker.as_str().unwrap_or("validation failed")
                    ));
                }
            }
            rubric
        }
        Err(err) => {
            blockers.push(format!("rubric validation failed: {err}"));
            serde_json::json!({
                "schema_version": "ao2.factory-acceptance-rubric-validation.v1",
                "accepted": false,
                "blockers": [err.to_string()]
            })
        }
    };
    let rubric_sha256 = json_string(&rubric, "sha256");
    let expected_run_rubric_sha =
        json_string(&project_run["artifacts"], "acceptance_rubric_sha256");
    if expected_run_rubric_sha.trim().is_empty() {
        blockers.push("artifacts.acceptance_rubric_sha256 is missing".to_string());
    } else if expected_run_rubric_sha != rubric_sha256 {
        let blocker = "artifacts.acceptance_rubric_sha256 does not match signed rubric".to_string();
        blockers.push(blocker.clone());
        rubric["accepted"] = serde_json::Value::Bool(false);
        if let Some(rubric_blockers) = rubric["blockers"].as_array_mut() {
            rubric_blockers.push(serde_json::Value::String(blocker));
        }
    }
    if json_string(&project_run["acceptance_rubric"], "sha256") != rubric_sha256 {
        let blocker = "embedded acceptance_rubric sha256 does not match signed rubric".to_string();
        blockers.push(blocker.clone());
        rubric["accepted"] = serde_json::Value::Bool(false);
        if let Some(rubric_blockers) = rubric["blockers"].as_array_mut() {
            rubric_blockers.push(serde_json::Value::String(blocker));
        }
    }

    let mut missing_artifacts = Vec::<String>::new();
    for (label, key) in [
        ("factory_project_run", "factory_project_run"),
        ("factory_project_run_state", "factory_project_run_state"),
        ("acceptance_rubric", "acceptance_rubric"),
        ("release_review_package", "release_review_package"),
    ] {
        let raw = json_string(&project_run["artifacts"], key);
        if raw.trim().is_empty() {
            missing_artifacts.push(label.to_string());
            continue;
        }
        let path = factory_project_plan_resolve_existing_file(project_base, &raw);
        if path.is_err() {
            missing_artifacts.push(label.to_string());
        }
    }
    for (index, app_run) in project_run["app_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        for (label, key) in [("app_run", "app_run"), ("bundle", "bundle")] {
            let raw = json_string(app_run, key);
            if raw.trim().is_empty()
                || factory_project_plan_resolve_existing_file(project_base, &raw).is_err()
            {
                missing_artifacts.push(format!("app_runs[{index}].{label}"));
            }
        }
        if app_run["control_plane_approves_release"].as_bool() != Some(false) {
            blockers.push(format!(
                "app_runs[{index}] control_plane_approves_release must be false"
            ));
        }
        if json_string(app_run, "release_acceptance_owner") != "factory-v3 evaluator-closer" {
            blockers.push(format!(
                "app_runs[{index}] release_acceptance_owner must be factory-v3 evaluator-closer"
            ));
        }
    }
    let must_have_artifacts_present = missing_artifacts.is_empty();
    if !must_have_artifacts_present {
        blockers.push(format!(
            "missing must-have artifacts: {}",
            missing_artifacts.join(", ")
        ));
    }

    let failed_step_count = project_run["failed_step_count"].as_u64().unwrap_or(1);
    let release_review_ready = project_run["release_review"]["ready"].as_bool() == Some(true);
    let release_package_ready =
        project_run["project_run_checklist"]["release_review_package_ready"].as_bool()
            == Some(true);
    let thresholds_satisfied =
        failed_step_count == 0 && release_review_ready && release_package_ready;
    if !thresholds_satisfied {
        blockers.push("rubric thresholds are not satisfied".to_string());
    }
    let status = if blockers.is_empty() {
        "accepted"
    } else {
        "rejected"
    };
    let recommended_decision = if status == "accepted" {
        "accept"
    } else {
        "reject"
    };
    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let mut review = serde_json::json!({
        "schema_version": "ao2.factory-project-acceptance-review.v1",
        "status": status,
        "recommended_decision": recommended_decision,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "project_run": options.project_run.display().to_string(),
        "project_run_sha256": sha256_file(options.project_run)?,
        "rubric_sha256": rubric_sha256,
        "rubric": rubric,
        "must_have_artifacts_present": must_have_artifacts_present,
        "missing_artifacts": missing_artifacts,
        "thresholds_satisfied": thresholds_satisfied,
        "thresholds": {
            "failed_step_count": failed_step_count,
            "release_review_ready": release_review_ready,
            "release_review_package_ready": release_package_ready
        },
        "blockers": blockers,
        "artifacts": {
            "review": options.out.display().to_string(),
            "project_run": options.project_run.display().to_string(),
            "acceptance_rubric": project_run["artifacts"]["acceptance_rubric"].clone(),
            "release_review_package": project_run["artifacts"]["release_review_package"].clone()
        },
        "trust_boundary": trust_boundary
    });
    let signed_payload_path = options.out.with_extension("signed-payload.json");
    atomic_write_text(
        &signed_payload_path,
        &serde_json::to_string_pretty(&review)?,
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &signed_payload_path,
        "project-acceptance-review-signed-payload.json",
    )?;
    let signature = match options.signing_key.as_deref() {
        Some(key_path) => {
            let signature_path = options.out.with_extension("json.sig");
            let public_key_path = options.out.with_extension("public.pem");
            derive_public_key_from_private_key(key_path, &public_key_path)?;
            sign_file_with_private_key(key_path, &signed_payload_path, &signature_path)?;
            let signature_verified =
                verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?;
            serde_json::json!({
                "schema_version": "ao2.factory-project-acceptance-review-signature.v1",
                "signature_algorithm": "RSA/SHA-256",
                "signer_id": options.signer_id,
                "signed_payload": "project_acceptance_review_without_signature_field",
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
            "schema_version": "ao2.factory-project-acceptance-review-signature.v1",
            "signed_payload": "project_acceptance_review_without_signature_field",
            "signed_payload_path": signed_payload_path.display().to_string(),
            "signed_payload_sha256": sha256_file(&signed_payload_path)?,
            "signature_status": "unsigned",
            "signature_verified": false
        }),
    };
    review["signature"] = signature;
    atomic_write_text(options.out, &serde_json::to_string_pretty(&review)?)?;
    factory_app_run_bundle_reject_secret_markers(options.out, "project-acceptance-review.json")?;
    if status != "accepted" {
        anyhow::bail!(
            "factory project acceptance review rejected: {}",
            options.out.display()
        );
    }
    Ok(review)
}

fn factory_project_run_package_json(
    project_run: &serde_json::Value,
    project_spec: &Path,
    project_plan: Option<&Path>,
    app_runs: &[PathBuf],
    archive_path: &Path,
) -> Result<serde_json::Value> {
    let archive_parent = archive_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(archive_parent)
        .with_context(|| format!("create {}", archive_parent.display()))?;
    let created_at_ms = now_unix_ms();
    let stage_dir = archive_parent.join(format!(".ao2-factory-project-run-{created_at_ms}.stage"));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove stale {}", stage_dir.display()))?;
    }
    fs::create_dir_all(&stage_dir).with_context(|| format!("create {}", stage_dir.display()))?;

    let mut checksum_entries = Vec::<(String, String)>::new();
    let mut add_file = |source: &Path, relative_path: &str, scan_text: bool| -> Result<()> {
        let staged_path = stage_dir.join(relative_path);
        fs::create_dir_all(
            staged_path
                .parent()
                .context("staged project artifact has parent directory")?,
        )
        .with_context(|| format!("create parent for {}", staged_path.display()))?;
        fs::copy(source, &staged_path)
            .with_context(|| format!("copy {} to {}", source.display(), staged_path.display()))?;
        if scan_text {
            factory_app_run_bundle_reject_secret_markers(&staged_path, relative_path)?;
        }
        let source_sha256 = sha256_file(source)?;
        let bundle_sha256 = sha256_file(&staged_path)?;
        if source_sha256 != bundle_sha256 {
            anyhow::bail!("artifact digest changed while staging {relative_path}");
        }
        checksum_entries.push((relative_path.to_string(), bundle_sha256));
        Ok(())
    };

    let project_spec_name = project_spec
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("project-spec.md");
    add_file(
        project_spec,
        &format!("project-spec/{project_spec_name}"),
        true,
    )?;
    if let Some(project_plan) = project_plan {
        add_file(project_plan, "project-plan/project-plan.json", true)?;
    }
    let acceptance_rubric_path =
        PathBuf::from(json_string(&project_run["artifacts"], "acceptance_rubric"));
    if acceptance_rubric_path.is_file() {
        add_file(
            &acceptance_rubric_path,
            "project-plan/acceptance-rubric.json",
            true,
        )?;
    }
    add_file(
        &PathBuf::from(json_string(
            &project_run["artifacts"],
            "factory_project_run",
        )),
        "project-run.json",
        true,
    )?;
    let project_state_path = PathBuf::from(json_string(
        &project_run["artifacts"],
        "factory_project_run_state",
    ));
    if project_state_path.is_file() {
        add_file(
            &project_state_path,
            "project-state/factory-project-run-state.json",
            true,
        )?;
    }
    for (index, app_run) in app_runs.iter().enumerate() {
        add_file(
            app_run,
            &format!("app-runs/{index}/factory-app-run.json"),
            true,
        )?;
        let bundle_path = PathBuf::from(json_string(&project_run["app_runs"][index], "bundle"));
        add_file(
            &bundle_path,
            &format!("app-run-bundles/{index}/app-run-evidence-bundle.tgz"),
            false,
        )?;
    }

    let manifest = serde_json::json!({
        "schema_version": "ao2.factory-project-run.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "created_at_ms": created_at_ms,
        "run_id": project_run["run_id"].clone(),
        "app_run_count": project_run["app_run_count"].clone(),
        "app_runs": project_run["app_runs"].clone(),
        "files": checksum_entries.iter().map(|(path, sha256)| {
            serde_json::json!({
                "path": path,
                "sha256": sha256
            })
        }).collect::<Vec<_>>(),
        "trust_boundary": project_run["trust_boundary"].clone()
    });
    let manifest_path = stage_dir.join("manifest.json");
    let mut manifest_text = serde_json::to_string_pretty(&manifest)?;
    manifest_text.push('\n');
    atomic_write_text(&manifest_path, &manifest_text)?;
    factory_app_run_bundle_reject_secret_markers(&manifest_path, "manifest.json")?;
    checksum_entries.push(("manifest.json".to_string(), sha256_file(&manifest_path)?));
    checksum_entries.sort_by(|left, right| left.0.cmp(&right.0));
    let checksum_text = checksum_entries
        .iter()
        .map(|(path, sha256)| format!("{sha256}  {path}\n"))
        .collect::<String>();
    atomic_write_text(&stage_dir.join("SHA256SUMS"), &checksum_text)?;

    create_tar_gz(&stage_dir, archive_path)?;
    fs::remove_dir_all(&stage_dir).with_context(|| format!("remove {}", stage_dir.display()))?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-run-package.v1",
        "status": "packaged",
        "archive": archive_path,
        "sha256": sha256_file(archive_path)?,
        "manifest_entry": "manifest.json",
        "checksum_entry": "SHA256SUMS",
        "trust_boundary": project_run["trust_boundary"].clone()
    }))
}

fn factory_project_resume_step(
    resume_state: Option<&serde_json::Value>,
    step_id: &str,
) -> Result<Option<serde_json::Value>> {
    let Some(resume_state) = resume_state else {
        return Ok(None);
    };
    if json_string(resume_state, "schema_version") != "ao2.factory-project-run-state.v1" {
        anyhow::bail!(
            "factory project-run resume state must use schema ao2.factory-project-run-state.v1"
        );
    }
    let Some(steps) = resume_state["steps"].as_array() else {
        anyhow::bail!("factory project-run resume state must contain steps array");
    };
    for step in steps {
        if json_string(step, "id") != step_id || json_string(step, "status") != "accepted" {
            continue;
        }
        let app_run = PathBuf::from(json_string(step, "app_run"));
        if !app_run.is_file() {
            return Ok(None);
        }
        let expected_app_run_sha256 = json_string(step, "app_run_sha256");
        if expected_app_run_sha256.trim().is_empty()
            || sha256_file(&app_run)? != expected_app_run_sha256
        {
            return Ok(None);
        }
        let bundle = PathBuf::from(json_string(step, "bundle"));
        if !bundle.is_file() {
            return Ok(None);
        }
        let expected_bundle_sha256 = json_string(step, "bundle_sha256");
        if expected_bundle_sha256.trim().is_empty()
            || sha256_file(&bundle)? != expected_bundle_sha256
        {
            return Ok(None);
        }
        return Ok(Some(step.clone()));
    }
    Ok(None)
}

fn factory_project_plan_path(step: &serde_json::Value, base: &Path, key: &str) -> Result<PathBuf> {
    let raw = json_string(step, key);
    if raw.trim().is_empty() {
        anyhow::bail!("factory project plan app step is missing {key}");
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    Ok(path)
}

fn factory_project_plan_optional_path(
    step: &serde_json::Value,
    base: &Path,
    key: &str,
) -> Result<Option<PathBuf>> {
    let raw = json_string(step, key);
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    Ok(Some(path))
}

fn factory_project_plan_optional_string(step: &serde_json::Value, key: &str) -> Option<String> {
    let raw = json_string(step, key);
    if raw.trim().is_empty() {
        None
    } else {
        Some(raw)
    }
}
