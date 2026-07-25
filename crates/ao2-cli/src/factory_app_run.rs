use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::artifact_safety::factory_app_run_bundle_reject_secret_markers;
use crate::cli_util::{
    atomic_write_text, create_tar_gz, json_string, now_unix_ms, sanitize_greenfield_id, sha256_file,
};
use crate::factory_compat::read_factory_compat_value;
use crate::factory_evaluator::{factory_evaluator_rubric_json, FactoryEvaluatorRubricOptions};
use crate::greenfield_workflow::{greenfield_governed_run_json, GreenfieldGovernedRunOptions};

pub(crate) struct FactoryAppRunOptions<'a> {
    pub(crate) spec: &'a Path,
    pub(crate) target: &'a Path,
    pub(crate) run_id: String,
    pub(crate) verifier_command: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt: Option<String>,
    pub(crate) provider_prompt_file: Option<PathBuf>,
    pub(crate) provider_max_budget_usd: Option<f64>,
    pub(crate) factory_decision: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) out_dir: &'a Path,
}

pub(crate) fn factory_app_run_json(options: FactoryAppRunOptions<'_>) -> Result<serde_json::Value> {
    fs::create_dir_all(options.out_dir).with_context(|| {
        format!(
            "create factory app run out dir {}",
            options.out_dir.display()
        )
    })?;
    let app_out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize factory app run out dir {}",
            options.out_dir.display()
        )
    })?;
    let rubric_dir = app_out_dir.join("rubric");
    fs::create_dir_all(&rubric_dir).with_context(|| format!("create {}", rubric_dir.display()))?;
    let rubric_path = rubric_dir.join(format!(
        "{}-evaluator-rubric.json",
        sanitize_greenfield_id(&options.run_id)
    ));
    let evaluator_rubric = factory_evaluator_rubric_json(FactoryEvaluatorRubricOptions {
        spec: options.spec,
        run_id: options.run_id.clone(),
        verifier_command: options.verifier_command.clone(),
        signing_key: options.signing_key.clone(),
        signer_id: format!("{}-rubric", options.signer_id),
        out: &rubric_path,
    })?;
    let rubric_sha256 = json_string(&evaluator_rubric, "rubric_sha256");
    let app = greenfield_governed_run_json(GreenfieldGovernedRunOptions {
        spec: options.spec,
        target: options.target,
        run_id: options.run_id.clone(),
        verifier_command: options.verifier_command,
        provider: options.provider,
        provider_prompt: options.provider_prompt,
        provider_prompt_file: options.provider_prompt_file,
        provider_max_budget_usd: options.provider_max_budget_usd,
        factory_decision: options.factory_decision,
        signing_key: options.signing_key,
        signer_id: options.signer_id,
        max_repair_attempts: options.max_repair_attempts,
        out_dir: options.out_dir,
    })?;
    let out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize factory app run out dir {}",
            options.out_dir.display()
        )
    })?;
    let result_path = out_dir.join(format!(
        "{}-factory-app-run.json",
        sanitize_greenfield_id(&options.run_id)
    ));
    let status = json_string(&app, "status");
    let release_review_artifacts_ready = status == "accepted"
        && Path::new(&json_string(&app["artifacts"], "greenfield_governed_run")).is_file()
        && Path::new(&json_string(&app["artifacts"], "governed_run")).is_file()
        && Path::new(&json_string(&app["artifacts"], "packed_evidence")).is_file()
        && Path::new(&json_string(&app["artifacts"], "evaluator_decision")).is_file();
    let result = serde_json::json!({
        "schema_version": "ao2.factory-app-run.v1",
        "status": status,
        "run_id": options.run_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
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
            "factory_app_run": result_path.display().to_string(),
            "evaluator_rubric": json_string(&evaluator_rubric["artifacts"], "rubric"),
            "greenfield_governed_run": json_string(&app["artifacts"], "greenfield_governed_run"),
            "greenfield_ingest": json_string(&app["artifacts"], "greenfield_ingest"),
            "plan": json_string(&app["artifacts"], "plan"),
            "governed_run": json_string(&app["artifacts"], "governed_run"),
            "evidence_pack": json_string(&app["artifacts"], "packed_evidence"),
            "evaluator_decision": json_string(&app["artifacts"], "evaluator_decision")
        },
        "rubric_sha256": rubric_sha256,
        "evaluator_rubric": evaluator_rubric,
        "app": app,
        "release_review": {
            "ready": release_review_artifacts_ready,
            "rubric_sha256": rubric_sha256,
            "evaluator_rubric": json_string(&evaluator_rubric["artifacts"], "rubric"),
            "artifacts": {
                "evaluator_rubric": json_string(&evaluator_rubric["artifacts"], "rubric"),
                "plan": json_string(&app["artifacts"], "plan"),
                "governed_run": json_string(&app["artifacts"], "governed_run"),
                "evidence_pack": json_string(&app["artifacts"], "packed_evidence"),
                "evaluator_decision": json_string(&app["artifacts"], "evaluator_decision")
            },
            "downstream_contract": {
                "verifier_outputs_must_reference": "rubric_sha256",
                "closer_outputs_must_reference": "rubric_sha256",
                "factory_v3_may_compare_or_audit": true,
                "factory_v3_must_not_be_primary_producer": true
            },
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        },
        "app_run_checklist": {
            "ao2_derived_signed_evaluator_rubric": !rubric_sha256.is_empty(),
            "ao2_ingested_plain_spec": app["greenfield_governed_run_checklist"]["ao2_ingested_plain_spec"],
            "ao2_generated_work_request": app["greenfield_governed_run_checklist"]["ao2_generated_work_request"],
            "ao2_generated_runspec": app["greenfield_governed_run_checklist"]["ao2_generated_runspec"],
            "ao2_executed_generated_governed_plan": app["greenfield_governed_run_checklist"]["ao2_executed_generated_governed_plan"],
            "ao2_verified_primary_run_result": app["greenfield_governed_run_checklist"]["ao2_verified_primary_run_result"],
            "ao2_packed_primary_evidence": app["greenfield_governed_run_checklist"]["ao2_packed_primary_evidence"],
            "ao2_signed_evaluator_closure": app["greenfield_governed_run_checklist"]["ao2_signed_evaluator_closure"],
            "verifier_outputs_reference_rubric_sha256": !rubric_sha256.is_empty(),
            "closer_outputs_reference_rubric_sha256": !rubric_sha256.is_empty(),
            "release_review_artifacts_ready": release_review_artifacts_ready,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
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
    atomic_write_text(&result_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn factory_app_run_bundle_json(
    app_run_path: &Path,
    archive_path: &Path,
) -> Result<serde_json::Value> {
    let app_run = read_factory_compat_value(app_run_path)?;
    factory_app_run_bundle_validate(&app_run)?;

    let archive_parent = archive_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(archive_parent)
        .with_context(|| format!("create {}", archive_parent.display()))?;
    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let created_at_ms = now_unix_ms();
    let stage_dir =
        archive_parent.join(format!(".ao2-factory-app-run-bundle-{created_at_ms}.stage"));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove stale {}", stage_dir.display()))?;
    }
    fs::create_dir_all(stage_dir.join("artifacts"))
        .with_context(|| format!("create {}", stage_dir.join("artifacts").display()))?;

    let app_run_base = app_run_path.parent().unwrap_or_else(|| Path::new("."));
    let mut artifacts = Vec::new();
    let mut checksum_entries: Vec<(String, String)> = Vec::new();
    for (key, label, bundle_name) in [
        ("factory_app_run", "factory-app-run", "factory-app-run.json"),
        (
            "evaluator_rubric",
            "evaluator-rubric",
            "evaluator-rubric.json",
        ),
        (
            "greenfield_governed_run",
            "greenfield-governed-run",
            "greenfield-governed-run.json",
        ),
        (
            "greenfield_ingest",
            "greenfield-ingest",
            "greenfield-ingest.json",
        ),
        ("plan", "plan", "plan.json"),
        ("governed_run", "governed-run", "governed-run.json"),
        ("evidence_pack", "evidence-pack", "evidence-pack.json"),
        (
            "evaluator_decision",
            "evaluator-decision",
            "evaluator-decision.json",
        ),
    ] {
        let source = if key == "factory_app_run" {
            app_run_path.to_path_buf()
        } else {
            factory_app_run_bundle_artifact_path(&app_run, app_run_base, key)?
        };
        let relative_path = format!("artifacts/{label}/{bundle_name}");
        let staged_path = stage_dir.join(&relative_path);
        fs::create_dir_all(
            staged_path
                .parent()
                .context("staged artifact has parent directory")?,
        )
        .with_context(|| format!("create parent for {}", staged_path.display()))?;
        fs::copy(&source, &staged_path)
            .with_context(|| format!("copy {} to {}", source.display(), staged_path.display()))?;
        factory_app_run_bundle_reject_secret_markers(&staged_path, &relative_path)?;
        let source_sha256 = sha256_file(&source)?;
        let bundle_sha256 = sha256_file(&staged_path)?;
        if source_sha256 != bundle_sha256 {
            anyhow::bail!("artifact digest changed while staging {label}");
        }
        let size_bytes = fs::metadata(&staged_path)
            .with_context(|| format!("stat {}", staged_path.display()))?
            .len();
        checksum_entries.push((relative_path.clone(), bundle_sha256.clone()));
        artifacts.push(serde_json::json!({
            "label": label,
            "source_path": source,
            "bundle_path": relative_path,
            "sha256": bundle_sha256,
            "size_bytes": size_bytes
        }));
    }

    let release_review_path = stage_dir.join("release-review.json");
    let release_review = serde_json::json!({
        "schema_version": "ao2.factory-app-run-release-review.v1",
        "source_app_run": app_run_path,
        "release_review": app_run["release_review"].clone(),
        "rubric_sha256": app_run["rubric_sha256"].clone(),
        "evaluator_rubric": app_run["artifacts"]["evaluator_rubric"].clone(),
        "app_run_checklist": app_run["app_run_checklist"].clone(),
        "trust_boundary": app_run["trust_boundary"].clone()
    });
    let mut release_review_text = serde_json::to_string_pretty(&release_review)?;
    release_review_text.push('\n');
    atomic_write_text(&release_review_path, &release_review_text)?;
    factory_app_run_bundle_reject_secret_markers(&release_review_path, "release-review.json")?;
    checksum_entries.push((
        "release-review.json".to_string(),
        sha256_file(&release_review_path)?,
    ));

    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let manifest = serde_json::json!({
        "schema_version": "ao2.factory-app-run-bundle.v1",
        "created_at": created_at,
        "created_at_ms": created_at_ms,
        "source_app_run": app_run_path,
        "artifact_count": artifacts.len(),
        "artifacts": artifacts,
        "files": checksum_entries.iter().map(|(path, sha256)| {
            serde_json::json!({
                "path": path,
                "sha256": sha256
            })
        }).collect::<Vec<_>>(),
        "trust_boundary": trust_boundary
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
    let archive_sha256 = sha256_file(archive_path)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-app-run-bundle.v1",
        "status": "bundled",
        "created_at": manifest["created_at"].clone(),
        "created_at_ms": created_at_ms,
        "app_run": app_run_path,
        "archive": archive_path,
        "sha256": archive_sha256,
        "artifact_count": manifest["artifact_count"].clone(),
        "manifest_entry": "manifest.json",
        "checksum_entry": "SHA256SUMS",
        "artifacts": manifest["artifacts"].clone(),
        "trust_boundary": manifest["trust_boundary"].clone()
    }))
}

fn factory_app_run_bundle_artifact_path(
    app_run: &serde_json::Value,
    base: &Path,
    key: &str,
) -> Result<PathBuf> {
    let raw = json_string(&app_run["artifacts"], key);
    if raw.trim().is_empty() {
        anyhow::bail!("factory app-run artifact {key} is missing");
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    if !path.is_file() {
        anyhow::bail!(
            "factory app-run artifact {key} is not a file: {}",
            path.display()
        );
    }
    Ok(path)
}

pub(crate) fn factory_app_run_bundle_validate(app_run: &serde_json::Value) -> Result<()> {
    if json_string(app_run, "schema_version") != "ao2.factory-app-run.v1" {
        anyhow::bail!("--app-run must use schema ao2.factory-app-run.v1");
    }
    if json_string(app_run, "status") != "accepted" {
        anyhow::bail!("factory app-run bundle requires accepted app-run evidence");
    }
    if app_run["release_review"]["ready"].as_bool() != Some(true) {
        anyhow::bail!("factory app-run release_review.ready must be true");
    }
    let rubric_sha256 = json_string(app_run, "rubric_sha256");
    if rubric_sha256.trim().is_empty() {
        anyhow::bail!("factory app-run must include rubric_sha256");
    }
    if json_string(&app_run["release_review"], "rubric_sha256") != rubric_sha256 {
        anyhow::bail!("factory app-run release_review.rubric_sha256 must match rubric_sha256");
    }
    if app_run["app_run_checklist"]["ao2_derived_signed_evaluator_rubric"].as_bool() != Some(true)
        || app_run["app_run_checklist"]["verifier_outputs_reference_rubric_sha256"].as_bool()
            != Some(true)
        || app_run["app_run_checklist"]["closer_outputs_reference_rubric_sha256"].as_bool()
            != Some(true)
    {
        anyhow::bail!(
            "factory app-run must derive a signed rubric and reference rubric_sha256 downstream"
        );
    }
    if json_string(
        &app_run["release_review"]["downstream_contract"],
        "verifier_outputs_must_reference",
    ) != "rubric_sha256"
        || json_string(
            &app_run["release_review"]["downstream_contract"],
            "closer_outputs_must_reference",
        ) != "rubric_sha256"
    {
        anyhow::bail!("factory app-run release review must require rubric_sha256 references");
    }
    if app_run["factory_replacement_boundary"]["factory_v3_drives_workflow"].as_bool()
        != Some(false)
    {
        anyhow::bail!("factory-v3 must not drive bundled AO2 app-run workflow");
    }
    if json_string(&app_run["factory_replacement_boundary"], "factory_v3_role")
        != "parity_oracle_only"
    {
        anyhow::bail!("factory-v3 role must remain parity_oracle_only");
    }
    if json_string(
        &app_run["factory_replacement_boundary"],
        "control_plane_role",
    ) != "read_only_observer_after_signed_evidence"
    {
        anyhow::bail!("control plane role must remain read_only_observer_after_signed_evidence");
    }
    if app_run["factory_replacement_boundary"]["control_plane_approves_release"].as_bool()
        != Some(false)
        || app_run["release_review"]["control_plane_approves_release"].as_bool() != Some(false)
        || app_run["trust_boundary"]["control_plane_approves_release"].as_bool() != Some(false)
    {
        anyhow::bail!("control plane must not approve release");
    }
    if app_run["factory_replacement_boundary"]["mutates_ao_artifacts"].as_bool() != Some(false)
        || app_run["trust_boundary"]["mutates_ao_artifacts"].as_bool() != Some(false)
    {
        anyhow::bail!("control plane must not mutate AO artifacts");
    }
    if json_string(
        &app_run["factory_replacement_boundary"],
        "release_acceptance_owner",
    ) != "factory-v3 evaluator-closer"
        || json_string(&app_run["release_review"], "release_acceptance_owner")
            != "factory-v3 evaluator-closer"
        || json_string(&app_run["trust_boundary"], "release_acceptance_owner")
            != "factory-v3 evaluator-closer"
    {
        anyhow::bail!("release acceptance owner must be factory-v3 evaluator-closer");
    }
    Ok(())
}
