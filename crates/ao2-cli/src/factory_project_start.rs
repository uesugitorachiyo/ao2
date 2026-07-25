use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::artifact_safety::factory_app_run_bundle_reject_secret_markers;
use crate::cli_util::{
    atomic_write_text, create_tar_gz, json_array, json_string, now_unix_ms, sanitize_greenfield_id,
    sha256_file,
};
use crate::factory_compat::{read_factory_compat_value, reject_factory_provider_api_key_auth};
use crate::factory_project_contract::factory_project_start_bundle_verify_trust_boundary;
use crate::factory_project_execution::{
    factory_project_acceptance_review_json, factory_project_run_json,
    FactoryProjectAcceptanceReviewOptions, FactoryProjectRunOptions,
};
use crate::factory_project_planning::{
    factory_project_plan_json, factory_project_plan_validate_json, FactoryProjectPlanOptions,
    FactoryProjectPlanValidateOptions,
};
use crate::release_comparison::{
    checksum_manifest_map, release_evidence_bundle_relative_path_allowed,
    release_evidence_bundle_secret_marker_failures,
};
use crate::release_crypto::{extract_tar_gz, verify_file_signature};

pub(crate) struct FactoryProjectStartOptions<'a> {
    pub(crate) project_spec: &'a Path,
    pub(crate) project_root: &'a Path,
    pub(crate) run_id: String,
    pub(crate) verifier_command: String,
    pub(crate) provider: Option<String>,
    pub(crate) provider_prompt_dir: Option<PathBuf>,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) max_repair_attempts: usize,
    pub(crate) handoff_bundle_out: Option<PathBuf>,
    pub(crate) handoff_bundle_report: Option<PathBuf>,
    pub(crate) out_dir: &'a Path,
}

pub(crate) struct FactoryReplacementPacketOptions<'a> {
    pub(crate) queue_status: &'a Path,
    pub(crate) latest_queue_status: &'a Path,
    pub(crate) closure: &'a Path,
    pub(crate) closure_verification: &'a Path,
    pub(crate) cross_os_readbacks: &'a [PathBuf],
    pub(crate) out: &'a Path,
}

pub(crate) fn factory_project_start_json(
    options: FactoryProjectStartOptions<'_>,
) -> Result<serde_json::Value> {
    fs::create_dir_all(options.out_dir).with_context(|| {
        format!(
            "create factory project-start out dir {}",
            options.out_dir.display()
        )
    })?;
    let out_dir = fs::canonicalize(options.out_dir).with_context(|| {
        format!(
            "canonicalize factory project-start out dir {}",
            options.out_dir.display()
        )
    })?;
    if options.handoff_bundle_report.is_some() && options.handoff_bundle_out.is_none() {
        anyhow::bail!("--handoff-bundle-report requires --handoff-bundle-out");
    }
    let run_id = sanitize_greenfield_id(&options.run_id);
    let project_plan_dir = out_dir.join("project-plan");
    let project_plan_path = project_plan_dir.join("project-plan.json");
    let project_plan_validation_path = project_plan_dir.join("project-plan-validation.json");
    let project_run_dir = out_dir.join("project-run");
    let project_acceptance_review_path = out_dir.join("project-acceptance-review.json");
    let project_start_path = out_dir.join(format!("{run_id}-factory-project-start.json"));

    let project_plan = factory_project_plan_json(FactoryProjectPlanOptions {
        project_spec: options.project_spec,
        project_root: options.project_root,
        run_id: options.run_id.clone(),
        verifier_command: options.verifier_command.clone(),
        provider: options.provider.clone(),
        provider_prompt_dir: options.provider_prompt_dir.clone(),
        signing_key: options.signing_key.clone(),
        signer_id: options.signer_id.clone(),
        out: &project_plan_path,
    })?;
    let project_plan_validation =
        factory_project_plan_validate_json(FactoryProjectPlanValidateOptions {
            project_plan: &project_plan_path,
            project_root: options.project_root,
            out: &project_plan_validation_path,
        })?;
    let project_run = factory_project_run_json(FactoryProjectRunOptions {
        project_spec: options.project_spec,
        project_plan: Some(&project_plan_path),
        resume_from: None,
        app_runs: &[],
        run_id: options.run_id.clone(),
        signing_key: options.signing_key.clone(),
        signer_id: options.signer_id.clone(),
        max_repair_attempts: options.max_repair_attempts,
        out_dir: &project_run_dir,
    })?;

    let factory_project_run_path = PathBuf::from(json_string(
        &project_run["artifacts"],
        "factory_project_run",
    ));
    let factory_project_run_state_path = PathBuf::from(json_string(
        &project_run["artifacts"],
        "factory_project_run_state",
    ));
    let release_review_package_path = PathBuf::from(json_string(
        &project_run["artifacts"],
        "release_review_package",
    ));
    let project_acceptance_review =
        factory_project_acceptance_review_json(FactoryProjectAcceptanceReviewOptions {
            project_run: &factory_project_run_path,
            signing_key: options.signing_key.clone(),
            signer_id: format!("{}-project-acceptance-review", options.signer_id),
            out: &project_acceptance_review_path,
        })?;
    let app_run_bundles = project_run["app_runs"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let bundle_path = PathBuf::from(json_string(item, "bundle"));
                    Ok(serde_json::json!({
                        "index": item["index"].clone(),
                        "run_id": item["run_id"].clone(),
                        "bundle": bundle_path.display().to_string(),
                        "bundle_sha256": if bundle_path.is_file() {
                            serde_json::Value::String(sha256_file(&bundle_path)?)
                        } else {
                            serde_json::Value::Null
                        }
                    }))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    let status = json_string(&project_run, "status");
    let mut result = serde_json::json!({
        "schema_version": "ao2.factory-project-start.v1",
        "status": status,
        "run_id": options.run_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "project_spec": options.project_spec.display().to_string(),
        "project_root": options.project_root.display().to_string(),
        "out_dir": out_dir.display().to_string(),
        "app_run_count": project_run["app_run_count"].clone(),
        "step_count": project_run["step_count"].clone(),
        "failed_step_count": project_run["failed_step_count"].clone(),
        "checks": {
            "project_plan_status": json_string(&project_plan, "status"),
            "project_plan_validation_status": json_string(&project_plan_validation, "status"),
            "project_run_status": json_string(&project_run, "status"),
            "release_review_package_ready": project_run["release_review"]["ready"].as_bool().unwrap_or(false),
            "project_acceptance_review_status": json_string(&project_acceptance_review, "status"),
            "project_acceptance_review_recommended_decision": json_string(&project_acceptance_review, "recommended_decision")
        },
        "artifacts": {
            "factory_project_start": project_start_path.display().to_string(),
            "project_plan": project_plan_path.display().to_string(),
            "project_plan_sha256": sha256_file(&project_plan_path)?,
            "acceptance_rubric": project_plan["artifacts"]["acceptance_rubric"].clone(),
            "acceptance_rubric_sha256": project_plan["artifacts"]["acceptance_rubric_sha256"].clone(),
            "project_plan_validation": project_plan_validation_path.display().to_string(),
            "project_plan_validation_sha256": sha256_file(&project_plan_validation_path)?,
            "factory_project_run": factory_project_run_path.display().to_string(),
            "factory_project_run_sha256": sha256_file(&factory_project_run_path)?,
            "factory_project_run_state": factory_project_run_state_path.display().to_string(),
            "factory_project_run_state_sha256": sha256_file(&factory_project_run_state_path)?,
            "project_acceptance_review": project_acceptance_review_path.display().to_string(),
            "project_acceptance_review_sha256": sha256_file(&project_acceptance_review_path)?,
            "release_review_package": release_review_package_path.display().to_string(),
            "release_review_package_sha256": if release_review_package_path.is_file() {
                serde_json::Value::String(sha256_file(&release_review_package_path)?)
            } else {
                serde_json::Value::Null
            },
            "app_run_bundles": app_run_bundles
        },
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
    result["project_acceptance_review"] = project_acceptance_review;
    atomic_write_text(&project_start_path, &serde_json::to_string_pretty(&result)?)?;
    factory_app_run_bundle_reject_secret_markers(
        &project_start_path,
        "factory-project-start.json",
    )?;
    if let Some(handoff_bundle_out) = options.handoff_bundle_out.as_deref() {
        let bundle = factory_project_start_bundle_json(&project_start_path, handoff_bundle_out)?;
        if let Some(report_path) = options.handoff_bundle_report.as_deref() {
            let report_parent = report_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(report_parent)
                .with_context(|| format!("create {}", report_parent.display()))?;
            atomic_write_text(report_path, &serde_json::to_string_pretty(&bundle)?)?;
            factory_app_run_bundle_reject_secret_markers(
                report_path,
                "factory-project-start-bundle.json",
            )?;
        }
        result["artifacts"]["project_start_bundle"] = bundle["archive"].clone();
        result["artifacts"]["project_start_bundle_sha256"] = bundle["sha256"].clone();
        result["project_start_bundle"] = bundle.clone();
        result["hermes_queue_handoff"] = serde_json::json!({
            "schema_version": "ao2.hermes-project-start-handoff.v1",
            "status": "ready",
            "project_start_bundle": bundle["archive"].clone(),
            "project_start_bundle_sha256": bundle["sha256"].clone(),
            "handoff_entry": bundle["handoff_entry"].clone(),
            "manifest_entry": bundle["manifest_entry"].clone(),
            "checksum_entry": bundle["checksum_entry"].clone(),
            "factory_v3_role": bundle["trust_boundary"]["factory_v3_role"].clone(),
            "control_plane_role": bundle["trust_boundary"]["control_plane_role"].clone(),
            "release_acceptance_owner": bundle["trust_boundary"]["release_acceptance_owner"].clone(),
            "hermes_role": "front_end_queue_cron_memory_bookkeeping_only",
            "ao2_role": "canonical_project_start_and_evidence_producer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        });
        atomic_write_text(&project_start_path, &serde_json::to_string_pretty(&result)?)?;
        factory_app_run_bundle_reject_secret_markers(
            &project_start_path,
            "factory-project-start.json",
        )?;
    }
    Ok(result)
}

pub(crate) fn factory_project_start_bundle_json(
    project_start_path: &Path,
    archive_path: &Path,
) -> Result<serde_json::Value> {
    let project_start = read_factory_compat_value(project_start_path)?;
    reject_factory_provider_api_key_auth("factory_project_start_bundle", &project_start)?;
    if project_start["schema_version"] != "ao2.factory-project-start.v1" {
        anyhow::bail!(
            "factory project-start bundle requires ao2.factory-project-start.v1 input: {}",
            project_start_path.display()
        );
    }
    if project_start["factory_replacement_boundary"]["control_plane_approves_release"].as_bool()
        != Some(false)
        || project_start["factory_replacement_boundary"]["mutates_ao_artifacts"].as_bool()
            != Some(false)
    {
        anyhow::bail!("factory project-start bundle refuses control-plane approval or mutation");
    }

    let archive_parent = archive_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(archive_parent)
        .with_context(|| format!("create {}", archive_parent.display()))?;
    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let created_at_ms = now_unix_ms();
    let stage_dir = archive_parent.join(format!(
        ".ao2-factory-project-start-bundle-{created_at_ms}.stage"
    ));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove stale {}", stage_dir.display()))?;
    }
    fs::create_dir_all(&stage_dir).with_context(|| format!("create {}", stage_dir.display()))?;

    let base = project_start_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut artifacts = Vec::<serde_json::Value>::new();
    let mut checksum_entries = Vec::<(String, String)>::new();
    let mut add_file =
        |label: &str, source: &Path, relative_path: &str, scan_text: bool| -> Result<()> {
            if !source.is_file() {
                anyhow::bail!(
                    "factory project-start artifact {label} is not a file: {}",
                    source.display()
                );
            }
            let staged_path = stage_dir.join(relative_path);
            fs::create_dir_all(
                staged_path
                    .parent()
                    .context("staged project-start artifact has parent directory")?,
            )
            .with_context(|| format!("create parent for {}", staged_path.display()))?;
            fs::copy(source, &staged_path).with_context(|| {
                format!("copy {} to {}", source.display(), staged_path.display())
            })?;
            if scan_text {
                factory_app_run_bundle_reject_secret_markers(&staged_path, relative_path)?;
            }
            let source_sha256 = sha256_file(source)?;
            let bundle_sha256 = sha256_file(&staged_path)?;
            if source_sha256 != bundle_sha256 {
                anyhow::bail!("artifact digest changed while staging {label}");
            }
            let size_bytes = fs::metadata(&staged_path)
                .with_context(|| format!("stat {}", staged_path.display()))?
                .len();
            checksum_entries.push((relative_path.to_string(), bundle_sha256.clone()));
            artifacts.push(serde_json::json!({
                "label": label,
                "source_path": source,
                "bundle_path": relative_path,
                "sha256": bundle_sha256,
                "size_bytes": size_bytes
            }));
            Ok(())
        };

    add_file(
        "factory-project-start",
        project_start_path,
        "factory-project-start.json",
        true,
    )?;
    for (key, label, relative_path, scan_text) in [
        (
            "project_plan",
            "project-plan",
            "project-plan/project-plan.json",
            true,
        ),
        (
            "acceptance_rubric",
            "acceptance-rubric",
            "project-plan/acceptance-rubric.json",
            true,
        ),
        (
            "project_plan_validation",
            "project-plan-validation",
            "project-plan/project-plan-validation.json",
            true,
        ),
        (
            "factory_project_run",
            "factory-project-run",
            "project-run/factory-project-run.json",
            true,
        ),
        (
            "factory_project_run_state",
            "factory-project-run-state",
            "project-run/factory-project-run-state.json",
            true,
        ),
        (
            "project_acceptance_review",
            "project-acceptance-review",
            "project-run/project-acceptance-review.json",
            true,
        ),
        (
            "release_review_package",
            "release-review-package",
            "release-review/release-review-package.tgz",
            false,
        ),
    ] {
        let source = factory_project_start_bundle_artifact_path(&project_start, base, key)?;
        add_file(label, &source, relative_path, scan_text)?;
        if matches!(key, "acceptance_rubric" | "project_acceptance_review") {
            let signed_value = read_factory_compat_value(&source)?;
            let sidecar_prefix = if key == "acceptance_rubric" {
                "project-plan/acceptance-rubric"
            } else {
                "project-run/project-acceptance-review"
            };
            let sidecar_label_prefix = if key == "acceptance_rubric" {
                "acceptance-rubric"
            } else {
                "project-acceptance-review"
            };
            let signature = &signed_value["signature"];
            if json_string(signature, "signature_status") == "signed" {
                for (field, label_suffix, path_suffix, scan_sidecar) in [
                    (
                        "signed_payload_path",
                        "signed-payload",
                        ".signed-payload.json",
                        true,
                    ),
                    ("signature_path", "signature", ".json.sig", false),
                    ("public_key_path", "public-key", ".public.pem", true),
                ] {
                    let raw = json_string(signature, field);
                    if raw.is_empty() {
                        anyhow::bail!("{sidecar_label_prefix} signature is missing {field}");
                    }
                    let sidecar_source = factory_project_start_bundle_raw_path(base, &raw)?;
                    add_file(
                        &format!("{sidecar_label_prefix}-{label_suffix}"),
                        &sidecar_source,
                        &format!("{sidecar_prefix}{path_suffix}"),
                        scan_sidecar,
                    )?;
                }
            }
        }
    }
    if let Some(bundles) = project_start["artifacts"]["app_run_bundles"].as_array() {
        for bundle in bundles {
            let index = bundle["index"].as_u64().unwrap_or(0);
            let source =
                factory_project_start_bundle_raw_path(base, &json_string(bundle, "bundle"))?;
            add_file(
                "app-run-bundle",
                &source,
                &format!("app-run-bundles/{index}/app-run-evidence-bundle.tgz"),
                false,
            )?;
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
    let handoff = serde_json::json!({
        "schema_version": "ao2.factory-project-start-handoff.v1",
        "source_project_start": project_start_path,
        "run_id": project_start["run_id"].clone(),
        "status": project_start["status"].clone(),
        "checks": project_start["checks"].clone(),
        "artifact_count": artifacts.len(),
        "trust_boundary": trust_boundary
    });
    let handoff_path = stage_dir.join("handoff.json");
    atomic_write_text(&handoff_path, &serde_json::to_string_pretty(&handoff)?)?;
    factory_app_run_bundle_reject_secret_markers(&handoff_path, "handoff.json")?;
    checksum_entries.push(("handoff.json".to_string(), sha256_file(&handoff_path)?));

    let manifest = serde_json::json!({
        "schema_version": "ao2.factory-project-start-bundle.v1",
        "created_at": created_at,
        "created_at_ms": created_at_ms,
        "source_project_start": project_start_path,
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
    atomic_write_text(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
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
        "schema_version": "ao2.factory-project-start-bundle.v1",
        "status": "bundled",
        "created_at": manifest["created_at"].clone(),
        "created_at_ms": created_at_ms,
        "project_start": project_start_path,
        "archive": archive_path,
        "sha256": archive_sha256,
        "artifact_count": manifest["artifact_count"].clone(),
        "manifest_entry": "manifest.json",
        "checksum_entry": "SHA256SUMS",
        "handoff_entry": "handoff.json",
        "artifacts": manifest["artifacts"].clone(),
        "trust_boundary": manifest["trust_boundary"].clone()
    }))
}

fn factory_project_start_bundle_artifact_path(
    project_start: &serde_json::Value,
    base: &Path,
    key: &str,
) -> Result<PathBuf> {
    factory_project_start_bundle_raw_path(base, &json_string(&project_start["artifacts"], key))
        .with_context(|| format!("resolve factory project-start artifact {key}"))
}

pub(crate) fn factory_project_start_bundle_raw_path(base: &Path, raw: &str) -> Result<PathBuf> {
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

pub(crate) fn factory_project_start_closure_json(
    queue_status_path: &Path,
    latest_queue_status_path: &Path,
    archive_path: &Path,
) -> Result<serde_json::Value> {
    let queue_status = read_factory_compat_value(queue_status_path)?;
    let latest_queue_status = read_factory_compat_value(latest_queue_status_path)?;
    reject_factory_provider_api_key_auth(
        "factory_project_start_closure_queue_status",
        &queue_status,
    )?;
    reject_factory_provider_api_key_auth(
        "factory_project_start_closure_latest_queue_status",
        &latest_queue_status,
    )?;
    if queue_status["schema_version"] != "ao2.factory-queue-status.v1" {
        anyhow::bail!(
            "factory project-start closure requires ao2.factory-queue-status.v1 queue status: {}",
            queue_status_path.display()
        );
    }
    if latest_queue_status["schema_version"] != "ao2.factory-queue-status.v1" {
        anyhow::bail!(
            "factory project-start closure requires ao2.factory-queue-status.v1 latest queue status: {}",
            latest_queue_status_path.display()
        );
    }
    if json_string(&queue_status, "status") != "accepted"
        || json_string(&latest_queue_status, "status") != "accepted"
    {
        anyhow::bail!("factory project-start closure requires accepted queue-status inputs");
    }

    let entry = &queue_status["entry"];
    let latest_entry = &latest_queue_status["entry"];
    let run_id = json_string(&queue_status, "run_id");
    if run_id.trim().is_empty() || json_string(entry, "run_id") != run_id {
        anyhow::bail!("factory project-start closure queue-status run_id is inconsistent");
    }
    if json_string(&latest_queue_status, "run_id") != run_id
        || json_string(latest_entry, "run_id") != run_id
    {
        anyhow::bail!(
            "factory project-start closure latest selector does not point at run_id {run_id}"
        );
    }
    if json_string(entry, "job_kind") != "factory_project_start"
        || json_string(latest_entry, "job_kind") != "factory_project_start"
    {
        anyhow::bail!("factory project-start closure requires factory_project_start queue entries");
    }

    for (label, value) in [
        ("queue-status", &queue_status),
        ("latest-queue-status", &latest_queue_status),
    ] {
        if json_string(&value["trust_boundary"], "release_acceptance_owner")
            != "factory-v3 evaluator-closer"
            || json_string(&value["trust_boundary"], "control_plane_role")
                != "read_only_observer_after_signed_evidence"
            || value["trust_boundary"]["control_plane_approves_release"].as_bool() != Some(false)
            || value["trust_boundary"]["mutates_ao_artifacts"].as_bool() != Some(false)
        {
            anyhow::bail!("factory project-start closure refuses {label} trust boundary drift");
        }
    }

    let selector_pairs = [
        "project_start_bundle_sha256",
        "project_start_bundle_verification_sha256",
        "project_start_operator_summary_sha256",
        "project_acceptance_review_sha256",
    ];
    for key in selector_pairs {
        let left = json_string(entry, key);
        let right = json_string(latest_entry, key);
        if left.trim().is_empty() || left != right {
            anyhow::bail!("factory project-start closure latest selector mismatch for {key}");
        }
    }
    let latest_selector_matches_run_id_selector = true;

    let base = queue_status_path.parent().unwrap_or_else(|| Path::new("."));
    let summary_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(entry, "project_start_operator_summary"),
    )
    .context("resolve project-start operator summary")?;
    let summary_markdown_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(entry, "project_start_operator_summary_markdown"),
    )
    .ok();
    let verification_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(entry, "project_start_bundle_verification"),
    )
    .context("resolve project-start bundle verification")?;
    let acceptance_review_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(entry, "project_acceptance_review"),
    )
    .context("resolve project acceptance review")?;
    let project_start_bundle_path =
        factory_project_start_bundle_raw_path(base, &json_string(entry, "project_start_bundle"))
            .context("resolve project-start handoff bundle")?;

    let summary = read_factory_compat_value(&summary_path)?;
    let verification = read_factory_compat_value(&verification_path)?;
    let acceptance_review = read_factory_compat_value(&acceptance_review_path)?;
    if summary["schema_version"] != "ao2.factory-project-start-operator-summary.v1"
        || json_string(&summary, "status") != "accepted"
    {
        anyhow::bail!("project-start operator summary must be accepted");
    }
    if verification["schema_version"] != "ao2.factory-project-start-bundle-verification.v1"
        || json_string(&verification, "status") != "accepted"
    {
        anyhow::bail!("project-start bundle verification must be accepted");
    }
    if acceptance_review["schema_version"] != "ao2.factory-project-acceptance-review.v1"
        || json_string(&acceptance_review, "status") != "accepted"
        || json_string(&acceptance_review, "recommended_decision") != "accept"
    {
        anyhow::bail!(
            "project acceptance review must be accepted with recommended_decision=accept"
        );
    }
    if json_string(&summary["trust_boundary"], "release_acceptance_owner")
        != "factory-v3 evaluator-closer"
        || summary["trust_boundary"]["control_plane_approves_release"].as_bool() != Some(false)
        || summary["trust_boundary"]["mutates_ao_artifacts"].as_bool() != Some(false)
    {
        anyhow::bail!("project-start operator summary trust boundary drifted");
    }

    let rubric_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(&summary["artifacts"]["acceptance_rubric"], "path"),
    )
    .context("resolve acceptance rubric from project-start operator summary")?;
    let rubric = read_factory_compat_value(&rubric_path)?;
    if rubric["schema_version"] != "ao2.factory-acceptance-rubric.v1"
        || json_string(&rubric, "status") != "accepted"
    {
        anyhow::bail!("acceptance rubric must be accepted");
    }

    for (label, path, expected) in [
        (
            "project-start operator summary",
            summary_path.as_path(),
            json_string(entry, "project_start_operator_summary_sha256"),
        ),
        (
            "project-start bundle verification",
            verification_path.as_path(),
            json_string(entry, "project_start_bundle_verification_sha256"),
        ),
        (
            "project acceptance review",
            acceptance_review_path.as_path(),
            json_string(entry, "project_acceptance_review_sha256"),
        ),
        (
            "project-start handoff bundle",
            project_start_bundle_path.as_path(),
            json_string(entry, "project_start_bundle_sha256"),
        ),
    ] {
        let actual = sha256_file(path)?;
        if expected.trim().is_empty() || actual != expected {
            anyhow::bail!(
                "factory project-start closure digest mismatch for {label}: expected {expected}, got {actual}"
            );
        }
    }

    let archive_parent = archive_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(archive_parent)
        .with_context(|| format!("create {}", archive_parent.display()))?;
    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let created_at_ms = now_unix_ms();
    let stage_dir = archive_parent.join(format!(
        ".ao2-factory-project-start-closure-{created_at_ms}.stage"
    ));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove stale {}", stage_dir.display()))?;
    }
    fs::create_dir_all(&stage_dir).with_context(|| format!("create {}", stage_dir.display()))?;

    let mut artifacts = Vec::<serde_json::Value>::new();
    let mut checksum_entries = Vec::<(String, String)>::new();
    let mut add_file =
        |label: &str, source: &Path, relative_path: &str, scan_text: bool| -> Result<()> {
            if !source.is_file() {
                anyhow::bail!(
                    "factory project-start closure artifact {label} is not a file: {}",
                    source.display()
                );
            }
            let staged_path = stage_dir.join(relative_path);
            fs::create_dir_all(
                staged_path
                    .parent()
                    .context("staged project-start closure artifact has parent directory")?,
            )
            .with_context(|| format!("create parent for {}", staged_path.display()))?;
            fs::copy(source, &staged_path).with_context(|| {
                format!("copy {} to {}", source.display(), staged_path.display())
            })?;
            if scan_text {
                factory_app_run_bundle_reject_secret_markers(&staged_path, relative_path)?;
            }
            let source_sha256 = sha256_file(source)?;
            let bundle_sha256 = sha256_file(&staged_path)?;
            if source_sha256 != bundle_sha256 {
                anyhow::bail!("artifact digest changed while staging {label}");
            }
            let size_bytes = fs::metadata(&staged_path)
                .with_context(|| format!("stat {}", staged_path.display()))?
                .len();
            checksum_entries.push((relative_path.to_string(), bundle_sha256.clone()));
            artifacts.push(serde_json::json!({
                "label": label,
                "source_path": source,
                "bundle_path": relative_path,
                "sha256": bundle_sha256,
                "size_bytes": size_bytes
            }));
            Ok(())
        };

    add_file(
        "queue-status-run-id",
        queue_status_path,
        "queue-status/factory-queue-project-start-status.json",
        true,
    )?;
    add_file(
        "queue-status-latest-completed-project-start",
        latest_queue_status_path,
        "queue-status/factory-queue-project-start-latest-status.json",
        true,
    )?;
    add_file(
        "project-start-operator-summary",
        &summary_path,
        "artifacts/project-start-operator-summary.json",
        true,
    )?;
    if let Some(path) = summary_markdown_path.as_deref() {
        add_file(
            "project-start-operator-summary-markdown",
            path,
            "artifacts/project-start-operator-summary.md",
            true,
        )?;
    }
    add_file(
        "project-start-bundle-verification",
        &verification_path,
        "artifacts/project-start-bundle-verification.json",
        true,
    )?;
    add_file(
        "project-acceptance-review",
        &acceptance_review_path,
        "artifacts/project-acceptance-review.json",
        true,
    )?;
    add_file(
        "acceptance-rubric",
        &rubric_path,
        "artifacts/acceptance-rubric.json",
        true,
    )?;
    add_file(
        "project-start-handoff-bundle",
        &project_start_bundle_path,
        "artifacts/project-start-handoff.tgz",
        false,
    )?;

    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let closure = serde_json::json!({
        "schema_version": "ao2.factory-project-start-closure.v1",
        "status": "packaged",
        "created_at": created_at,
        "created_at_ms": created_at_ms,
        "run_id": run_id,
        "queue_status": queue_status["status"].clone(),
        "latest_queue_status": latest_queue_status["status"].clone(),
        "latest_selector_matches_run_id_selector": latest_selector_matches_run_id_selector,
        "queue_status_sha256": sha256_file(queue_status_path)?,
        "latest_queue_status_sha256": sha256_file(latest_queue_status_path)?,
        "project_start_bundle_sha256": json_string(entry, "project_start_bundle_sha256"),
        "project_start_bundle_verification_status": json_string(entry, "project_start_bundle_verification_status"),
        "project_start_operator_summary_status": json_string(entry, "project_start_operator_summary_status"),
        "project_acceptance_review_status": json_string(entry, "project_acceptance_review_status"),
        "project_acceptance_review_recommended_decision": json_string(entry, "project_acceptance_review_recommended_decision"),
        "trust_boundary": trust_boundary
    });
    let closure_path = stage_dir.join("closure.json");
    atomic_write_text(&closure_path, &serde_json::to_string_pretty(&closure)?)?;
    factory_app_run_bundle_reject_secret_markers(&closure_path, "closure.json")?;
    checksum_entries.push(("closure.json".to_string(), sha256_file(&closure_path)?));

    let manifest = serde_json::json!({
        "schema_version": "ao2.factory-project-start-closure.v1",
        "status": "packaged",
        "created_at": closure["created_at"].clone(),
        "created_at_ms": created_at_ms,
        "run_id": closure["run_id"].clone(),
        "source_queue_status": queue_status_path,
        "source_latest_queue_status": latest_queue_status_path,
        "latest_selector_matches_run_id_selector": latest_selector_matches_run_id_selector,
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
    atomic_write_text(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
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
        "schema_version": "ao2.factory-project-start-closure.v1",
        "status": "packaged",
        "created_at": closure["created_at"].clone(),
        "created_at_ms": created_at_ms,
        "run_id": closure["run_id"].clone(),
        "queue_status": closure["queue_status"].clone(),
        "latest_queue_status": closure["latest_queue_status"].clone(),
        "latest_selector_matches_run_id_selector": latest_selector_matches_run_id_selector,
        "archive": archive_path,
        "sha256": archive_sha256,
        "artifact_count": manifest["artifact_count"].clone(),
        "manifest_entry": "manifest.json",
        "checksum_entry": "SHA256SUMS",
        "closure_entry": "closure.json",
        "artifacts": manifest["artifacts"].clone(),
        "trust_boundary": manifest["trust_boundary"].clone()
    }))
}

pub(crate) fn factory_project_start_closure_verify_json(
    bundle_path: &Path,
) -> Result<serde_json::Value> {
    let mut failures = Vec::<serde_json::Value>::new();
    let bundle_sha256 = match sha256_file(bundle_path) {
        Ok(sha256) => sha256,
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "bundle_unreadable",
                "message": format!("read bundle: {error}")
            }));
            return Ok(factory_project_start_closure_verify_report(
                bundle_path,
                "",
                "",
                0,
                0,
                &factory_project_start_closure_verify_checks(false, false, false, false),
                failures,
            ));
        }
    };

    let extract_dir = std::env::temp_dir().join(format!(
        "ao2-project-start-closure-verify-{}-{}",
        std::process::id(),
        now_unix_ms()
    ));
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir)
            .with_context(|| format!("remove stale {}", extract_dir.display()))?;
    }
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("create {}", extract_dir.display()))?;

    if let Err(error) = extract_tar_gz(bundle_path, &extract_dir) {
        failures.push(serde_json::json!({
            "code": "bundle_extract_failed",
            "message": error.to_string()
        }));
        let _ = fs::remove_dir_all(&extract_dir);
        return Ok(factory_project_start_closure_verify_report(
            bundle_path,
            &bundle_sha256,
            "",
            0,
            0,
            &factory_project_start_closure_verify_checks(false, false, false, false),
            failures,
        ));
    }

    let manifest_path = extract_dir.join("manifest.json");
    let closure_path = extract_dir.join("closure.json");
    let checksum_path = extract_dir.join("SHA256SUMS");
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                failures.push(serde_json::json!({
                    "code": "manifest_json_invalid",
                    "message": format!("parse manifest.json: {error}")
                }));
                serde_json::Value::Null
            }
        },
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "manifest_missing",
                "message": format!("read manifest.json: {error}")
            }));
            serde_json::Value::Null
        }
    };
    let closure = match fs::read_to_string(&closure_path) {
        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                failures.push(serde_json::json!({
                    "code": "closure_json_invalid",
                    "message": format!("parse closure.json: {error}")
                }));
                serde_json::Value::Null
            }
        },
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "closure_missing",
                "message": format!("read closure.json: {error}")
            }));
            serde_json::Value::Null
        }
    };

    let mut manifest_verified = !manifest.is_null()
        && json_string(&manifest, "schema_version") == "ao2.factory-project-start-closure.v1"
        && json_string(&manifest, "status") == "packaged";
    if !manifest_verified {
        failures.push(serde_json::json!({
            "code": "invalid_manifest_schema",
            "message": "manifest.json must use schema ao2.factory-project-start-closure.v1 and status packaged"
        }));
    }
    let mut closure_verified = !closure.is_null()
        && json_string(&closure, "schema_version") == "ao2.factory-project-start-closure.v1"
        && json_string(&closure, "status") == "packaged"
        && json_string(&closure, "queue_status") == "accepted"
        && json_string(&closure, "latest_queue_status") == "accepted";
    if !closure_verified {
        failures.push(serde_json::json!({
            "code": "invalid_closure_schema",
            "message": "closure.json must use schema ao2.factory-project-start-closure.v1, status packaged, and accepted selectors"
        }));
    }

    let mut checksum_reasons = Vec::new();
    let checksum_manifest = match fs::read_to_string(&checksum_path) {
        Ok(body) => checksum_manifest_map(&body, &mut checksum_reasons),
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "sha256sums_missing",
                "message": format!("read SHA256SUMS: {error}")
            }));
            BTreeMap::new()
        }
    };
    failures.extend(checksum_reasons);

    let mut checksums_verified = !checksum_manifest.is_empty();
    let mut files_checked = 0_usize;
    let mut secret_scan_passed = true;
    for (relative_path, expected_sha256) in &checksum_manifest {
        if !release_evidence_bundle_relative_path_allowed(relative_path) {
            checksums_verified = false;
            failures.push(serde_json::json!({
                "code": "unsafe_bundle_path",
                "path": relative_path,
                "message": "SHA256SUMS contains an absolute or parent-directory path"
            }));
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if !file_path.is_file() {
            checksums_verified = false;
            failures.push(serde_json::json!({
                "code": "bundle_file_missing",
                "path": relative_path,
                "message": "SHA256SUMS references a missing file"
            }));
            continue;
        }
        match sha256_file(&file_path) {
            Ok(actual_sha256) if actual_sha256 == *expected_sha256 => files_checked += 1,
            Ok(actual_sha256) => {
                checksums_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_mismatch",
                    "path": relative_path,
                    "expected": expected_sha256,
                    "actual": actual_sha256,
                    "message": "bundle file digest does not match SHA256SUMS"
                }));
            }
            Err(error) => {
                checksums_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_unreadable",
                    "path": relative_path,
                    "message": format!("hash bundle file: {error}")
                }));
            }
        }
        if !relative_path.ends_with(".tgz") {
            if let Err(error) =
                factory_app_run_bundle_reject_secret_markers(&file_path, relative_path)
            {
                secret_scan_passed = false;
                failures.push(serde_json::json!({
                    "code": "secret_marker_detected",
                    "path": relative_path,
                    "message": error.to_string()
                }));
            }
        }
    }

    for required in [
        "manifest.json",
        "closure.json",
        "queue-status/factory-queue-project-start-status.json",
        "queue-status/factory-queue-project-start-latest-status.json",
        "artifacts/project-start-operator-summary.json",
        "artifacts/project-start-bundle-verification.json",
        "artifacts/project-acceptance-review.json",
        "artifacts/acceptance-rubric.json",
        "artifacts/project-start-handoff.tgz",
    ] {
        if !checksum_manifest.contains_key(required) {
            checksums_verified = false;
            failures.push(serde_json::json!({
                "code": "required_file_not_checksummed",
                "path": required,
                "message": "project-start closure archive is missing a required checksum entry"
            }));
        }
    }

    let artifacts = json_array(&manifest, "artifacts");
    let mut artifact_paths = BTreeMap::<String, String>::new();
    let mut artifact_count = artifacts.len();
    for artifact in artifacts {
        let label = json_string(artifact, "label");
        let bundle_path = json_string(artifact, "bundle_path");
        let sha256 = json_string(artifact, "sha256");
        if label.is_empty()
            || !release_evidence_bundle_relative_path_allowed(&bundle_path)
            || !checksum_manifest.contains_key(&bundle_path)
        {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "invalid_artifact_entry",
                "label": label,
                "path": bundle_path,
                "message": "artifact entries must have a safe checksum-covered bundle_path"
            }));
            continue;
        }
        if checksum_manifest
            .get(&bundle_path)
            .is_some_and(|expected| expected != &sha256)
        {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "artifact_sha256_mismatch",
                "label": label,
                "path": bundle_path,
                "message": "artifact sha256 does not match SHA256SUMS"
            }));
        }
        artifact_paths.insert(label, bundle_path);
    }
    if artifact_count == 0 {
        artifact_count = artifact_paths.len();
    }
    for label in [
        "queue-status-run-id",
        "queue-status-latest-completed-project-start",
        "project-start-operator-summary",
        "project-start-bundle-verification",
        "project-acceptance-review",
        "acceptance-rubric",
        "project-start-handoff-bundle",
    ] {
        if !artifact_paths.contains_key(label) {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "required_artifact_missing",
                "label": label,
                "message": "project-start closure is missing a required artifact"
            }));
        }
    }

    let run_id = json_string(&closure, "run_id");
    let latest_selector_matches_run_id_selector = closure
        .get("latest_selector_matches_run_id_selector")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && manifest
            .get("latest_selector_matches_run_id_selector")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    if !latest_selector_matches_run_id_selector {
        closure_verified = false;
        failures.push(serde_json::json!({
            "code": "latest_selector_mismatch",
            "message": "closure and manifest must confirm latest selector matches the run-id selector"
        }));
    }
    let trust_boundary_verified = factory_project_start_bundle_verify_trust_boundary(&manifest)
        && factory_project_start_bundle_verify_trust_boundary(&closure);
    if !trust_boundary_verified {
        failures.push(serde_json::json!({
            "code": "trust_boundary_invalid",
            "message": "closure must preserve AO2 producer, factory-v3 evaluator-closer, and read-only control-plane boundaries"
        }));
    }
    let checks = factory_project_start_closure_verify_checks(
        manifest_verified,
        checksums_verified,
        closure_verified,
        trust_boundary_verified && secret_scan_passed,
    );
    let mut checks = checks;
    checks["latest_selector_matches_run_id_selector"] =
        serde_json::json!(latest_selector_matches_run_id_selector);
    checks["secret_scan_passed"] = serde_json::json!(secret_scan_passed);

    let _ = fs::remove_dir_all(&extract_dir);
    Ok(factory_project_start_closure_verify_report(
        bundle_path,
        &bundle_sha256,
        &run_id,
        artifact_count,
        files_checked,
        &checks,
        failures,
    ))
}

fn factory_project_start_closure_verify_report(
    bundle_path: &Path,
    bundle_sha256: &str,
    run_id: &str,
    artifact_count: usize,
    files_checked: usize,
    checks: &serde_json::Value,
    failures: Vec<serde_json::Value>,
) -> serde_json::Value {
    let status = if failures.is_empty() {
        "accepted"
    } else {
        "failed"
    };
    serde_json::json!({
        "schema_version": "ao2.factory-project-start-closure-verification.v1",
        "status": status,
        "bundle": bundle_path,
        "bundle_sha256": bundle_sha256,
        "run_id": run_id,
        "artifact_count": artifact_count,
        "files_checked": files_checked,
        "checks": checks,
        "failure_count": failures.len(),
        "failures": failures,
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    })
}

fn factory_project_start_closure_verify_checks(
    manifest_verified: bool,
    checksums_verified: bool,
    closure_verified: bool,
    trust_boundary_verified: bool,
) -> serde_json::Value {
    serde_json::json!({
        "manifest_verified": manifest_verified,
        "checksums_verified": checksums_verified,
        "closure_verified": closure_verified,
        "latest_selector_matches_run_id_selector": false,
        "trust_boundary_verified": trust_boundary_verified,
        "secret_scan_passed": false
    })
}

pub(crate) fn factory_replacement_packet_json(
    options: FactoryReplacementPacketOptions<'_>,
) -> Result<serde_json::Value> {
    let queue_status = read_factory_compat_value(options.queue_status)?;
    let latest_queue_status = read_factory_compat_value(options.latest_queue_status)?;
    let closure_verification = read_factory_compat_value(options.closure_verification)?;
    reject_factory_provider_api_key_auth("factory_replacement_packet_queue_status", &queue_status)?;
    reject_factory_provider_api_key_auth(
        "factory_replacement_packet_latest_queue_status",
        &latest_queue_status,
    )?;
    reject_factory_provider_api_key_auth(
        "factory_replacement_packet_closure_verification",
        &closure_verification,
    )?;

    if queue_status["schema_version"] != "ao2.factory-queue-status.v1"
        || latest_queue_status["schema_version"] != "ao2.factory-queue-status.v1"
    {
        anyhow::bail!("factory replacement packet requires ao2.factory-queue-status.v1 inputs");
    }
    if json_string(&queue_status, "status") != "accepted"
        || json_string(&latest_queue_status, "status") != "accepted"
    {
        anyhow::bail!("factory replacement packet requires accepted queue-status inputs");
    }
    if closure_verification["schema_version"] != "ao2.factory-project-start-closure-verification.v1"
        || json_string(&closure_verification, "status") != "accepted"
    {
        anyhow::bail!(
            "factory replacement packet requires accepted project-start closure verification"
        );
    }

    let entry = &queue_status["entry"];
    let latest_entry = &latest_queue_status["entry"];
    let run_id = json_string(&queue_status, "run_id");
    if run_id.trim().is_empty()
        || json_string(entry, "run_id") != run_id
        || json_string(&latest_queue_status, "run_id") != run_id
        || json_string(latest_entry, "run_id") != run_id
        || json_string(&closure_verification, "run_id") != run_id
    {
        anyhow::bail!("factory replacement packet run_id lineage is inconsistent");
    }
    if json_string(entry, "job_kind") != "factory_project_start"
        || json_string(latest_entry, "job_kind") != "factory_project_start"
    {
        anyhow::bail!("factory replacement packet requires factory_project_start queue entries");
    }
    for (label, value) in [
        ("queue-status", &queue_status),
        ("latest-queue-status", &latest_queue_status),
        ("closure-verification", &closure_verification),
    ] {
        if !factory_project_start_bundle_verify_trust_boundary(&value["trust_boundary"]) {
            anyhow::bail!("factory replacement packet refuses {label} trust boundary drift");
        }
    }

    let closure_sha256 = sha256_file(options.closure)?;
    if json_string(&closure_verification, "bundle_sha256") != closure_sha256 {
        anyhow::bail!("factory replacement packet closure verification does not match closure");
    }
    if closure_verification["checks"]["checksums_verified"].as_bool() != Some(true)
        || closure_verification["checks"]["trust_boundary_verified"].as_bool() != Some(true)
        || closure_verification["checks"]["latest_selector_matches_run_id_selector"].as_bool()
            != Some(true)
    {
        anyhow::bail!("factory replacement packet requires accepted closure verifier checks");
    }

    let base = options
        .queue_status
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let summary_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(entry, "project_start_operator_summary"),
    )
    .context("resolve project-start operator summary")?;
    let summary = read_factory_compat_value(&summary_path)?;
    reject_factory_provider_api_key_auth("factory_replacement_packet_summary", &summary)?;
    if summary["schema_version"] != "ao2.factory-project-start-operator-summary.v1"
        || json_string(&summary, "status") != "accepted"
    {
        anyhow::bail!("factory replacement packet requires accepted operator summary");
    }
    if !factory_project_start_bundle_verify_trust_boundary(&summary["trust_boundary"]) {
        anyhow::bail!("factory replacement packet refuses operator summary trust boundary drift");
    }

    let bundle_verification_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(entry, "project_start_bundle_verification"),
    )
    .context("resolve project-start bundle verification")?;
    let acceptance_review_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(entry, "project_acceptance_review"),
    )
    .context("resolve project acceptance review")?;
    let rubric_path = factory_project_start_bundle_raw_path(
        base,
        &json_string(&summary["artifacts"]["acceptance_rubric"], "path"),
    )
    .context("resolve acceptance rubric")?;
    let project_start_path =
        factory_project_start_bundle_raw_path(base, &json_string(entry, "project_start"))
            .context("resolve project-start artifact")?;
    let project_start_bundle_path =
        factory_project_start_bundle_raw_path(base, &json_string(entry, "project_start_bundle"))
            .context("resolve project-start handoff bundle")?;

    let archive_parent = options
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(archive_parent)
        .with_context(|| format!("create {}", archive_parent.display()))?;
    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let created_at_ms = now_unix_ms();
    let stage_dir = archive_parent.join(format!(
        ".ao2-factory-replacement-packet-{created_at_ms}.stage"
    ));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove stale {}", stage_dir.display()))?;
    }
    fs::create_dir_all(&stage_dir).with_context(|| format!("create {}", stage_dir.display()))?;

    let mut artifacts = Vec::<serde_json::Value>::new();
    let mut checksum_entries = Vec::<(String, String)>::new();
    let mut add_file =
        |label: &str, source: &Path, relative_path: &str, scan_text: bool| -> Result<()> {
            if !source.is_file() {
                anyhow::bail!(
                    "factory replacement packet artifact {label} is not a file: {}",
                    source.display()
                );
            }
            let staged_path = stage_dir.join(relative_path);
            fs::create_dir_all(
                staged_path
                    .parent()
                    .context("staged replacement packet artifact has parent directory")?,
            )
            .with_context(|| format!("create parent for {}", staged_path.display()))?;
            fs::copy(source, &staged_path).with_context(|| {
                format!("copy {} to {}", source.display(), staged_path.display())
            })?;
            if scan_text {
                factory_app_run_bundle_reject_secret_markers(&staged_path, relative_path)?;
            }
            let source_sha256 = sha256_file(source)?;
            let bundle_sha256 = sha256_file(&staged_path)?;
            if source_sha256 != bundle_sha256 {
                anyhow::bail!("artifact digest changed while staging {label}");
            }
            let size_bytes = fs::metadata(&staged_path)
                .with_context(|| format!("stat {}", staged_path.display()))?
                .len();
            checksum_entries.push((relative_path.to_string(), bundle_sha256.clone()));
            artifacts.push(serde_json::json!({
                "label": label,
                "source_path": source,
                "bundle_path": relative_path,
                "sha256": bundle_sha256,
                "size_bytes": size_bytes
            }));
            Ok(())
        };

    add_file(
        "queue-status-run-id",
        options.queue_status,
        "queue-status/factory-queue-project-start-status.json",
        true,
    )?;
    add_file(
        "queue-status-latest-completed-project-start",
        options.latest_queue_status,
        "queue-status/factory-queue-project-start-latest-status.json",
        true,
    )?;
    add_file(
        "project-start",
        &project_start_path,
        "artifacts/factory-project-start.json",
        true,
    )?;
    for (label, summary_key, relative_path, scan_text) in [
        (
            "project-plan",
            "project_plan",
            "artifacts/project-plan.json",
            true,
        ),
        (
            "project-run",
            "project_run",
            "artifacts/factory-project-run.json",
            true,
        ),
        (
            "release-review-package",
            "release_review_package",
            "artifacts/release-review-package.tgz",
            false,
        ),
    ] {
        let raw = json_string(&summary["artifacts"][summary_key], "path");
        if !raw.trim().is_empty() {
            let path = factory_project_start_bundle_raw_path(base, &raw)
                .with_context(|| format!("resolve {label} from operator summary"))?;
            add_file(label, &path, relative_path, scan_text)?;
        }
    }
    add_file(
        "acceptance-rubric",
        &rubric_path,
        "artifacts/acceptance-rubric.json",
        true,
    )?;
    add_file(
        "project-start-handoff-bundle",
        &project_start_bundle_path,
        "artifacts/project-start-handoff.tgz",
        false,
    )?;
    add_file(
        "project-start-bundle-verification",
        &bundle_verification_path,
        "artifacts/project-start-bundle-verification.json",
        true,
    )?;
    add_file(
        "project-start-operator-summary",
        &summary_path,
        "artifacts/project-start-operator-summary.json",
        true,
    )?;
    add_file(
        "project-acceptance-review",
        &acceptance_review_path,
        "artifacts/project-acceptance-review.json",
        true,
    )?;
    add_file(
        "project-start-closure",
        options.closure,
        "artifacts/project-start-closure.tgz",
        false,
    )?;
    add_file(
        "project-start-closure-verification",
        options.closure_verification,
        "artifacts/project-start-closure-verification.json",
        true,
    )?;
    for (index, readback) in options.cross_os_readbacks.iter().enumerate() {
        add_file(
            "cross-os-readback-summary",
            readback,
            &format!("cross-os-readback/{index}/summary.json"),
            true,
        )?;
    }

    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "factory_v3_role": "evaluator_closer_and_sampling_auditor",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let replacement_summary = serde_json::json!({
        "ao2_replaces_factory_v3_workflow_driver": true,
        "ao2_packet_role": "single_ao2_owned_review_handoff",
        "factory_v3_role": "evaluator_closer_and_sampling_auditor",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "hermes_role": "front_end_queue_cron_memory_bookkeeping",
        "release_acceptance_owner": "factory-v3 evaluator-closer"
    });
    let checks = serde_json::json!({
        "queue_status_accepted": json_string(&queue_status, "status") == "accepted",
        "latest_queue_status_accepted": json_string(&latest_queue_status, "status") == "accepted",
        "latest_selector_matches_run_id_selector": closure_verification["checks"]["latest_selector_matches_run_id_selector"].as_bool().unwrap_or(false),
        "closure_verification_accepted": json_string(&closure_verification, "status") == "accepted",
        "closure_checksums_verified": closure_verification["checks"]["checksums_verified"].as_bool().unwrap_or(false),
        "closure_trust_boundary_verified": closure_verification["checks"]["trust_boundary_verified"].as_bool().unwrap_or(false),
        "project_start_operator_summary_accepted": json_string(&summary, "status") == "accepted",
        "project_acceptance_review_accepted": json_string(entry, "project_acceptance_review_status") == "accepted",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false
    });
    let packet = serde_json::json!({
        "schema_version": "ao2.factory-replacement-packet.v1",
        "status": "packaged",
        "created_at": created_at,
        "created_at_ms": created_at_ms,
        "run_id": run_id,
        "queue_status": queue_status["status"].clone(),
        "latest_queue_status": latest_queue_status["status"].clone(),
        "closure_verification_status": closure_verification["status"].clone(),
        "closure_sha256": closure_sha256,
        "queue_status_sha256": sha256_file(options.queue_status)?,
        "latest_queue_status_sha256": sha256_file(options.latest_queue_status)?,
        "closure_verification_sha256": sha256_file(options.closure_verification)?,
        "checks": checks,
        "replacement_summary": replacement_summary,
        "artifact_count": artifacts.len(),
        "artifacts": artifacts,
        "trust_boundary": trust_boundary
    });
    let packet_path = stage_dir.join("replacement-packet.json");
    atomic_write_text(&packet_path, &serde_json::to_string_pretty(&packet)?)?;
    factory_app_run_bundle_reject_secret_markers(&packet_path, "replacement-packet.json")?;
    checksum_entries.push((
        "replacement-packet.json".to_string(),
        sha256_file(&packet_path)?,
    ));

    let manifest = serde_json::json!({
        "schema_version": "ao2.factory-replacement-packet.v1",
        "status": "packaged",
        "created_at": packet["created_at"].clone(),
        "created_at_ms": created_at_ms,
        "run_id": packet["run_id"].clone(),
        "source_queue_status": options.queue_status,
        "source_latest_queue_status": options.latest_queue_status,
        "source_closure": options.closure,
        "source_closure_verification": options.closure_verification,
        "artifact_count": packet["artifact_count"].clone(),
        "artifacts": packet["artifacts"].clone(),
        "files": checksum_entries.iter().map(|(path, sha256)| {
            serde_json::json!({
                "path": path,
                "sha256": sha256
            })
        }).collect::<Vec<_>>(),
        "checks": packet["checks"].clone(),
        "replacement_summary": packet["replacement_summary"].clone(),
        "trust_boundary": packet["trust_boundary"].clone()
    });
    let manifest_path = stage_dir.join("manifest.json");
    atomic_write_text(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;
    factory_app_run_bundle_reject_secret_markers(&manifest_path, "manifest.json")?;
    checksum_entries.push(("manifest.json".to_string(), sha256_file(&manifest_path)?));
    checksum_entries.sort_by(|left, right| left.0.cmp(&right.0));
    let checksum_text = checksum_entries
        .iter()
        .map(|(path, sha256)| format!("{sha256}  {path}\n"))
        .collect::<String>();
    atomic_write_text(&stage_dir.join("SHA256SUMS"), &checksum_text)?;

    create_tar_gz(&stage_dir, options.out)?;
    fs::remove_dir_all(&stage_dir).with_context(|| format!("remove {}", stage_dir.display()))?;
    let archive_sha256 = sha256_file(options.out)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-replacement-packet.v1",
        "status": "packaged",
        "created_at": packet["created_at"].clone(),
        "created_at_ms": created_at_ms,
        "run_id": packet["run_id"].clone(),
        "archive": options.out,
        "sha256": archive_sha256,
        "artifact_count": packet["artifact_count"].clone(),
        "manifest_entry": "manifest.json",
        "checksum_entry": "SHA256SUMS",
        "packet_entry": "replacement-packet.json",
        "checks": packet["checks"].clone(),
        "replacement_summary": packet["replacement_summary"].clone(),
        "trust_boundary": packet["trust_boundary"].clone()
    }))
}

pub(crate) fn factory_replacement_packet_verify_json(
    bundle_path: &Path,
) -> Result<serde_json::Value> {
    let mut failures = Vec::<serde_json::Value>::new();
    let bundle_sha256 = match sha256_file(bundle_path) {
        Ok(sha256) => sha256,
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "bundle_unreadable",
                "message": format!("read bundle: {error}")
            }));
            return Ok(factory_replacement_packet_verify_report(
                bundle_path,
                "",
                "",
                0,
                0,
                &factory_replacement_packet_verify_checks(
                    false, false, false, false, false, false, false,
                ),
                failures,
            ));
        }
    };

    let extract_dir = std::env::temp_dir().join(format!(
        "ao2-factory-replacement-packet-verify-{}-{}",
        std::process::id(),
        now_unix_ms()
    ));
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir)
            .with_context(|| format!("remove stale {}", extract_dir.display()))?;
    }
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("create {}", extract_dir.display()))?;

    if let Err(error) = extract_tar_gz(bundle_path, &extract_dir) {
        failures.push(serde_json::json!({
            "code": "bundle_extract_failed",
            "message": error.to_string()
        }));
        let _ = fs::remove_dir_all(&extract_dir);
        return Ok(factory_replacement_packet_verify_report(
            bundle_path,
            &bundle_sha256,
            "",
            0,
            0,
            &factory_replacement_packet_verify_checks(
                false, false, false, false, false, false, false,
            ),
            failures,
        ));
    }

    let manifest_path = extract_dir.join("manifest.json");
    let packet_path = extract_dir.join("replacement-packet.json");
    let checksum_path = extract_dir.join("SHA256SUMS");
    let manifest =
        factory_replacement_packet_read_json_file(&manifest_path, "manifest", &mut failures);
    let packet = factory_replacement_packet_read_json_file(
        &packet_path,
        "replacement_packet",
        &mut failures,
    );

    let mut manifest_verified = !manifest.is_null()
        && json_string(&manifest, "schema_version") == "ao2.factory-replacement-packet.v1"
        && json_string(&manifest, "status") == "packaged";
    if !manifest_verified {
        failures.push(serde_json::json!({
            "code": "invalid_manifest_schema",
            "message": "manifest.json must use schema ao2.factory-replacement-packet.v1 and status packaged"
        }));
    }
    let mut packet_verified = !packet.is_null()
        && json_string(&packet, "schema_version") == "ao2.factory-replacement-packet.v1"
        && json_string(&packet, "status") == "packaged"
        && json_string(&packet, "queue_status") == "accepted"
        && json_string(&packet, "latest_queue_status") == "accepted"
        && json_string(&packet, "closure_verification_status") == "accepted";
    if !packet_verified {
        failures.push(serde_json::json!({
            "code": "invalid_packet_schema",
            "message": "replacement-packet.json must use schema ao2.factory-replacement-packet.v1, status packaged, and accepted queue/closure inputs"
        }));
    }

    let mut checksum_reasons = Vec::new();
    let checksum_manifest = match fs::read_to_string(&checksum_path) {
        Ok(body) => checksum_manifest_map(&body, &mut checksum_reasons),
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "sha256sums_missing",
                "message": format!("read SHA256SUMS: {error}")
            }));
            BTreeMap::new()
        }
    };
    failures.extend(checksum_reasons);

    let mut checksums_verified = !checksum_manifest.is_empty();
    let mut files_checked = 0_usize;
    let mut secret_scan_passed = true;
    for (relative_path, expected_sha256) in &checksum_manifest {
        if !release_evidence_bundle_relative_path_allowed(relative_path) {
            checksums_verified = false;
            failures.push(serde_json::json!({
                "code": "unsafe_bundle_path",
                "path": relative_path,
                "message": "SHA256SUMS contains an absolute or parent-directory path"
            }));
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if !file_path.is_file() {
            checksums_verified = false;
            failures.push(serde_json::json!({
                "code": "bundle_file_missing",
                "path": relative_path,
                "message": "SHA256SUMS references a missing file"
            }));
            continue;
        }
        match sha256_file(&file_path) {
            Ok(actual_sha256) if actual_sha256 == *expected_sha256 => files_checked += 1,
            Ok(actual_sha256) => {
                checksums_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_mismatch",
                    "path": relative_path,
                    "expected": expected_sha256,
                    "actual": actual_sha256,
                    "message": "bundle file digest does not match SHA256SUMS"
                }));
            }
            Err(error) => {
                checksums_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_unreadable",
                    "path": relative_path,
                    "message": format!("hash bundle file: {error}")
                }));
            }
        }
        if !relative_path.ends_with(".tgz") {
            if let Err(error) =
                factory_app_run_bundle_reject_secret_markers(&file_path, relative_path)
            {
                secret_scan_passed = false;
                failures.push(serde_json::json!({
                    "code": "secret_marker_detected",
                    "path": relative_path,
                    "message": error.to_string()
                }));
            }
        }
    }

    for required in [
        "manifest.json",
        "SHA256SUMS",
        "replacement-packet.json",
        "queue-status/factory-queue-project-start-status.json",
        "queue-status/factory-queue-project-start-latest-status.json",
        "artifacts/factory-project-start.json",
        "artifacts/project-plan.json",
        "artifacts/factory-project-run.json",
        "artifacts/release-review-package.tgz",
        "artifacts/acceptance-rubric.json",
        "artifacts/project-start-handoff.tgz",
        "artifacts/project-start-bundle-verification.json",
        "artifacts/project-start-operator-summary.json",
        "artifacts/project-acceptance-review.json",
        "artifacts/project-start-closure.tgz",
        "artifacts/project-start-closure-verification.json",
    ] {
        if required != "SHA256SUMS" && !checksum_manifest.contains_key(required) {
            checksums_verified = false;
            failures.push(serde_json::json!({
                "code": "required_file_not_checksummed",
                "path": required,
                "message": "factory replacement packet is missing a required checksum entry"
            }));
        }
        if !extract_dir.join(required).is_file() {
            checksums_verified = false;
            failures.push(serde_json::json!({
                "code": "required_file_missing",
                "path": required,
                "message": "factory replacement packet is missing a required file"
            }));
        }
    }

    let artifacts = json_array(&manifest, "artifacts");
    let mut artifact_paths = BTreeMap::<String, String>::new();
    let mut artifact_count = artifacts.len();
    for artifact in artifacts {
        let label = json_string(artifact, "label");
        let bundle_path = json_string(artifact, "bundle_path");
        let sha256 = json_string(artifact, "sha256");
        if label.is_empty()
            || !release_evidence_bundle_relative_path_allowed(&bundle_path)
            || !checksum_manifest.contains_key(&bundle_path)
        {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "invalid_artifact_entry",
                "label": label,
                "path": bundle_path,
                "message": "artifact entries must have a safe checksum-covered bundle_path"
            }));
            continue;
        }
        if checksum_manifest
            .get(&bundle_path)
            .is_some_and(|expected| expected != &sha256)
        {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "artifact_sha256_mismatch",
                "label": label,
                "path": bundle_path,
                "message": "artifact sha256 does not match SHA256SUMS"
            }));
        }
        artifact_paths.insert(label, bundle_path);
    }
    if artifact_count == 0 {
        artifact_count = artifact_paths.len();
    }
    for label in [
        "queue-status-run-id",
        "queue-status-latest-completed-project-start",
        "project-start",
        "project-plan",
        "project-run",
        "release-review-package",
        "acceptance-rubric",
        "project-start-handoff-bundle",
        "project-start-bundle-verification",
        "project-start-operator-summary",
        "project-acceptance-review",
        "project-start-closure",
        "project-start-closure-verification",
    ] {
        if !artifact_paths.contains_key(label) {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "required_artifact_missing",
                "label": label,
                "message": "factory replacement packet is missing a required artifact"
            }));
        }
    }

    let run_id = json_string(&packet, "run_id");
    if run_id.trim().is_empty() || json_string(&manifest, "run_id") != run_id {
        packet_verified = false;
        failures.push(serde_json::json!({
            "code": "run_id_lineage_mismatch",
            "message": "manifest.json and replacement-packet.json must carry the same non-empty run_id"
        }));
    }

    let closure_path = extract_dir.join("artifacts/project-start-closure.tgz");
    let closure_verification = factory_replacement_packet_read_json_file(
        &extract_dir.join("artifacts/project-start-closure-verification.json"),
        "closure_verification",
        &mut failures,
    );
    if !closure_verification.is_null() {
        let actual_closure_sha256 = sha256_file(&closure_path).unwrap_or_default();
        if json_string(&closure_verification, "schema_version")
            != "ao2.factory-project-start-closure-verification.v1"
            || json_string(&closure_verification, "status") != "accepted"
            || json_string(&closure_verification, "run_id") != run_id
            || json_string(&closure_verification, "bundle_sha256") != actual_closure_sha256
            || json_string(&packet, "closure_sha256") != actual_closure_sha256
        {
            packet_verified = false;
            failures.push(serde_json::json!({
                "code": "closure_verification_lineage_mismatch",
                "message": "closure verification must be accepted and match the packaged closure archive"
            }));
        }
    }

    let trust_boundary_verified = factory_replacement_packet_trust_boundary_verified(&manifest)
        && factory_replacement_packet_trust_boundary_verified(&packet);
    if !trust_boundary_verified {
        failures.push(serde_json::json!({
            "code": "trust_boundary_invalid",
            "message": "replacement packet must preserve AO2 workflow-driver, factory-v3 evaluator-closer, and read-only control-plane boundaries"
        }));
    }
    let ao2_replacement_driver_verified =
        factory_replacement_summary_verified(&manifest["replacement_summary"])
            && factory_replacement_summary_verified(&packet["replacement_summary"]);
    if !ao2_replacement_driver_verified {
        failures.push(serde_json::json!({
            "code": "replacement_summary_invalid",
            "message": "replacement summary must declare AO2 as workflow driver and Hermes/control-plane/factory-v3 boundary roles"
        }));
    }
    let factory_v3_evaluator_closer_verified =
        json_string(&manifest["replacement_summary"], "factory_v3_role")
            == "evaluator_closer_and_sampling_auditor"
            && json_string(&packet["replacement_summary"], "factory_v3_role")
                == "evaluator_closer_and_sampling_auditor";

    let checks = factory_replacement_packet_verify_checks(
        manifest_verified,
        checksums_verified,
        packet_verified,
        trust_boundary_verified,
        secret_scan_passed,
        ao2_replacement_driver_verified,
        factory_v3_evaluator_closer_verified,
    );

    let _ = fs::remove_dir_all(&extract_dir);
    Ok(factory_replacement_packet_verify_report(
        bundle_path,
        &bundle_sha256,
        &run_id,
        artifact_count,
        files_checked,
        &checks,
        failures,
    ))
}

fn factory_replacement_packet_read_json_file(
    path: &Path,
    label: &str,
    failures: &mut Vec<serde_json::Value>,
) -> serde_json::Value {
    match fs::read_to_string(path) {
        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                failures.push(serde_json::json!({
                    "code": format!("{label}_json_invalid"),
                    "message": format!("parse {}: {error}", path.display())
                }));
                serde_json::Value::Null
            }
        },
        Err(error) => {
            failures.push(serde_json::json!({
                "code": format!("{label}_missing"),
                "message": format!("read {}: {error}", path.display())
            }));
            serde_json::Value::Null
        }
    }
}

fn factory_replacement_packet_trust_boundary_verified(value: &serde_json::Value) -> bool {
    let trust = if value.get("trust_boundary").is_some() {
        &value["trust_boundary"]
    } else {
        value
    };
    json_string(trust, "execution_owner") == "ao2"
        && json_string(trust, "release_acceptance_owner") == "factory-v3 evaluator-closer"
        && json_string(trust, "factory_v3_role") == "evaluator_closer_and_sampling_auditor"
        && json_string(trust, "control_plane_role") == "read_only_observer_after_signed_evidence"
        && trust
            .get("control_plane_approves_release")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && trust
            .get("mutates_ao_artifacts")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
}

fn factory_replacement_summary_verified(summary: &serde_json::Value) -> bool {
    summary
        .get("ao2_replaces_factory_v3_workflow_driver")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && json_string(summary, "ao2_packet_role") == "single_ao2_owned_review_handoff"
        && json_string(summary, "factory_v3_role") == "evaluator_closer_and_sampling_auditor"
        && json_string(summary, "control_plane_role") == "read_only_observer_after_signed_evidence"
        && json_string(summary, "hermes_role") == "front_end_queue_cron_memory_bookkeeping"
        && json_string(summary, "release_acceptance_owner") == "factory-v3 evaluator-closer"
}

fn factory_replacement_packet_verify_report(
    bundle_path: &Path,
    bundle_sha256: &str,
    run_id: &str,
    artifact_count: usize,
    files_checked: usize,
    checks: &serde_json::Value,
    failures: Vec<serde_json::Value>,
) -> serde_json::Value {
    let status = if failures.is_empty() {
        "accepted"
    } else {
        "failed"
    };
    serde_json::json!({
        "schema_version": "ao2.factory-replacement-packet-verification.v1",
        "status": status,
        "bundle": bundle_path,
        "bundle_sha256": bundle_sha256,
        "run_id": run_id,
        "artifact_count": artifact_count,
        "files_checked": files_checked,
        "checks": checks,
        "failure_count": failures.len(),
        "failures": failures,
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "evaluator_closer_and_sampling_auditor",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    })
}

fn factory_replacement_packet_verify_checks(
    manifest_verified: bool,
    checksums_verified: bool,
    packet_verified: bool,
    trust_boundary_verified: bool,
    secret_scan_passed: bool,
    ao2_replacement_driver_verified: bool,
    factory_v3_evaluator_closer_verified: bool,
) -> serde_json::Value {
    serde_json::json!({
        "manifest_verified": manifest_verified,
        "checksums_verified": checksums_verified,
        "packet_verified": packet_verified,
        "trust_boundary_verified": trust_boundary_verified,
        "secret_scan_passed": secret_scan_passed,
        "ao2_replacement_driver_verified": ao2_replacement_driver_verified,
        "factory_v3_evaluator_closer_verified": factory_v3_evaluator_closer_verified
    })
}

pub(crate) fn factory_project_start_bundle_verify_json(
    bundle_path: &Path,
) -> Result<serde_json::Value> {
    let mut failures = Vec::<serde_json::Value>::new();
    let bundle_sha256 = match sha256_file(bundle_path) {
        Ok(sha256) => sha256,
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "bundle_unreadable",
                "message": format!("read bundle: {error}")
            }));
            return Ok(factory_project_start_bundle_verify_report(
                bundle_path,
                "",
                0,
                0,
                &factory_project_start_bundle_verify_checks(false, false),
                failures,
            ));
        }
    };

    let extract_dir = std::env::temp_dir().join(format!(
        "ao2-project-start-bundle-verify-{}-{}",
        std::process::id(),
        now_unix_ms()
    ));
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir)
            .with_context(|| format!("remove stale {}", extract_dir.display()))?;
    }
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("create {}", extract_dir.display()))?;

    if let Err(error) = extract_tar_gz(bundle_path, &extract_dir) {
        failures.push(serde_json::json!({
            "code": "bundle_extract_failed",
            "message": error.to_string()
        }));
        let _ = fs::remove_dir_all(&extract_dir);
        return Ok(factory_project_start_bundle_verify_report(
            bundle_path,
            &bundle_sha256,
            0,
            0,
            &factory_project_start_bundle_verify_checks(false, false),
            failures,
        ));
    }

    let manifest_path = extract_dir.join("manifest.json");
    let checksum_path = extract_dir.join("SHA256SUMS");
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                failures.push(serde_json::json!({
                    "code": "manifest_json_invalid",
                    "message": format!("parse manifest.json: {error}")
                }));
                serde_json::Value::Null
            }
        },
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "manifest_missing",
                "message": format!("read manifest.json: {error}")
            }));
            serde_json::Value::Null
        }
    };

    let mut manifest_verified = !manifest.is_null()
        && json_string(&manifest, "schema_version") == "ao2.factory-project-start-bundle.v1";
    if !manifest_verified {
        failures.push(serde_json::json!({
            "code": "invalid_manifest_schema",
            "message": "manifest.json must use schema ao2.factory-project-start-bundle.v1"
        }));
    }

    let mut checksum_reasons = Vec::new();
    let checksum_manifest = match fs::read_to_string(&checksum_path) {
        Ok(body) => checksum_manifest_map(&body, &mut checksum_reasons),
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "sha256sums_missing",
                "message": format!("read SHA256SUMS: {error}")
            }));
            BTreeMap::new()
        }
    };
    failures.extend(checksum_reasons);

    let mut sha256sums_verified = !checksum_manifest.is_empty();
    let mut files_checked = 0_usize;
    for (relative_path, expected_sha256) in &checksum_manifest {
        if !release_evidence_bundle_relative_path_allowed(relative_path) {
            sha256sums_verified = false;
            failures.push(serde_json::json!({
                "code": "unsafe_bundle_path",
                "path": relative_path,
                "message": "SHA256SUMS contains an absolute or parent-directory path"
            }));
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if !file_path.is_file() {
            sha256sums_verified = false;
            failures.push(serde_json::json!({
                "code": "bundle_file_missing",
                "path": relative_path,
                "message": "SHA256SUMS references a missing file"
            }));
            continue;
        }
        match sha256_file(&file_path) {
            Ok(actual_sha256) if actual_sha256 == *expected_sha256 => files_checked += 1,
            Ok(actual_sha256) => {
                sha256sums_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_mismatch",
                    "path": relative_path,
                    "expected": expected_sha256,
                    "actual": actual_sha256,
                    "message": "bundle file digest does not match SHA256SUMS"
                }));
            }
            Err(error) => {
                sha256sums_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_unreadable",
                    "path": relative_path,
                    "message": format!("hash bundle file: {error}")
                }));
            }
        }
    }

    if !checksum_manifest.contains_key("manifest.json") {
        manifest_verified = false;
        failures.push(serde_json::json!({
            "code": "manifest_not_checksummed",
            "message": "SHA256SUMS must include manifest.json"
        }));
    }
    if !checksum_manifest.contains_key("handoff.json") {
        manifest_verified = false;
        failures.push(serde_json::json!({
            "code": "handoff_not_checksummed",
            "message": "SHA256SUMS must include handoff.json"
        }));
    }

    let artifacts = json_array(&manifest, "artifacts");
    let mut artifact_paths = BTreeMap::<String, String>::new();
    let mut artifact_sha256s = BTreeMap::<String, String>::new();
    for artifact in artifacts {
        let label = json_string(artifact, "label");
        let bundle_path = json_string(artifact, "bundle_path");
        let sha256 = json_string(artifact, "sha256");
        if label.is_empty()
            || !release_evidence_bundle_relative_path_allowed(&bundle_path)
            || !checksum_manifest.contains_key(&bundle_path)
        {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "invalid_artifact_entry",
                "label": label,
                "path": bundle_path,
                "message": "artifact entries must have a safe checksum-covered bundle_path"
            }));
            continue;
        }
        if checksum_manifest
            .get(&bundle_path)
            .is_some_and(|expected| expected != &sha256)
        {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "artifact_sha256_mismatch",
                "label": label,
                "path": bundle_path,
                "message": "artifact sha256 does not match SHA256SUMS"
            }));
        }
        artifact_paths.insert(label.clone(), bundle_path);
        artifact_sha256s.insert(label, sha256);
    }

    for label in [
        "factory-project-start",
        "project-plan",
        "acceptance-rubric",
        "acceptance-rubric-signed-payload",
        "acceptance-rubric-signature",
        "acceptance-rubric-public-key",
        "project-plan-validation",
        "factory-project-run",
        "factory-project-run-state",
        "project-acceptance-review",
        "project-acceptance-review-signed-payload",
        "project-acceptance-review-signature",
        "project-acceptance-review-public-key",
        "release-review-package",
    ] {
        if !artifact_paths.contains_key(label) {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "required_artifact_missing",
                "label": label,
                "message": "project-start bundle is missing a required artifact"
            }));
        }
    }

    let project_start = factory_project_start_bundle_read_json(
        &extract_dir,
        "factory-project-start",
        &artifact_paths,
        &mut failures,
    );
    let project_run = factory_project_start_bundle_read_json(
        &extract_dir,
        "factory-project-run",
        &artifact_paths,
        &mut failures,
    );
    let acceptance_rubric = factory_project_start_bundle_read_json(
        &extract_dir,
        "acceptance-rubric",
        &artifact_paths,
        &mut failures,
    );
    let project_acceptance_review = factory_project_start_bundle_read_json(
        &extract_dir,
        "project-acceptance-review",
        &artifact_paths,
        &mut failures,
    );

    let trust_boundary_verified = factory_project_start_bundle_verify_trust_boundary(&manifest)
        && project_start.as_ref().is_some_and(|value| {
            factory_project_start_bundle_verify_trust_boundary(&value["trust_boundary"])
                || factory_project_start_bundle_verify_trust_boundary(
                    &value["factory_replacement_boundary"],
                )
        })
        && project_run
            .as_ref()
            .is_some_and(factory_project_start_bundle_verify_trust_boundary)
        && acceptance_rubric
            .as_ref()
            .is_some_and(factory_project_start_bundle_verify_trust_boundary)
        && project_acceptance_review
            .as_ref()
            .is_some_and(factory_project_start_bundle_verify_trust_boundary);
    if !trust_boundary_verified {
        failures.push(serde_json::json!({
            "code": "trust_boundary_invalid",
            "message": "bundle must preserve AO2 producer, factory-v3 evaluator-closer, and read-only control-plane boundaries"
        }));
    }

    let project_start_verified = project_start.as_ref().is_some_and(|value| {
        json_string(value, "schema_version") == "ao2.factory-project-start.v1"
            && json_string(value, "status") == "accepted"
    });
    let mut project_run_verified = project_run.as_ref().is_some_and(|value| {
        json_string(value, "schema_version") == "ao2.factory-project-run.v1"
            && json_string(value, "status") == "accepted"
    });
    let mut acceptance_rubric_verified = acceptance_rubric.as_ref().is_some_and(|value| {
        json_string(value, "schema_version") == "ao2.factory-acceptance-rubric.v1"
            && json_string(value, "status") == "accepted"
    });
    let mut project_acceptance_review_verified =
        project_acceptance_review.as_ref().is_some_and(|value| {
            json_string(value, "schema_version") == "ao2.factory-project-acceptance-review.v1"
                && json_string(value, "status") == "accepted"
                && json_string(value, "recommended_decision") == "accept"
        });

    let acceptance_rubric_signature_verified = acceptance_rubric.as_ref().is_some_and(|value| {
        factory_project_start_bundle_verify_signature(
            &extract_dir,
            value,
            "project-plan/acceptance-rubric",
            &mut failures,
        )
    });
    let project_acceptance_review_signature_verified =
        project_acceptance_review.as_ref().is_some_and(|value| {
            factory_project_start_bundle_verify_signature(
                &extract_dir,
                value,
                "project-run/project-acceptance-review",
                &mut failures,
            )
        });
    acceptance_rubric_verified &= acceptance_rubric_signature_verified;
    project_acceptance_review_verified &= project_acceptance_review_signature_verified;

    let review_rubric_digest_matches = acceptance_rubric
        .as_ref()
        .and_then(|_| artifact_sha256s.get("acceptance-rubric"))
        .zip(
            project_acceptance_review
                .as_ref()
                .map(|value| json_string(value, "rubric_sha256")),
        )
        .is_some_and(|(actual, expected)| *actual == expected);
    let review_project_run_digest_matches = project_run
        .as_ref()
        .and_then(|_| artifact_sha256s.get("factory-project-run"))
        .zip(
            project_acceptance_review
                .as_ref()
                .map(|value| json_string(value, "project_run_sha256")),
        )
        .is_some_and(|(actual, expected)| *actual == expected);
    if !review_rubric_digest_matches {
        project_acceptance_review_verified = false;
        failures.push(serde_json::json!({
            "code": "review_rubric_digest_mismatch",
            "message": "project acceptance review rubric digest must match bundled rubric bytes"
        }));
    }
    if !review_project_run_digest_matches {
        project_acceptance_review_verified = false;
        project_run_verified = false;
        failures.push(serde_json::json!({
            "code": "review_project_run_digest_mismatch",
            "message": "project acceptance review project-run digest must match bundled project-run bytes"
        }));
    }
    if !project_start_verified {
        failures.push(serde_json::json!({
            "code": "project_start_invalid",
            "message": "bundled project-start must be accepted and schema-pinned"
        }));
    }
    if !project_run_verified {
        failures.push(serde_json::json!({
            "code": "project_run_invalid",
            "message": "bundled project-run must be accepted and digest-bound to the review"
        }));
    }
    if !acceptance_rubric_verified {
        failures.push(serde_json::json!({
            "code": "acceptance_rubric_invalid",
            "message": "bundled acceptance rubric must be accepted, signed, and verified"
        }));
    }
    if !project_acceptance_review_verified {
        failures.push(serde_json::json!({
            "code": "project_acceptance_review_invalid",
            "message": "bundled project acceptance review must be accepted, signed, and digest-bound"
        }));
    }

    let mut secret_scan_passed = true;
    for relative_path in checksum_manifest.keys() {
        if !release_evidence_bundle_relative_path_allowed(relative_path) {
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if file_path.is_file()
            && !relative_path.ends_with(".tgz")
            && !relative_path.ends_with(".sig")
        {
            release_evidence_bundle_secret_marker_failures(
                relative_path,
                &file_path,
                &mut failures,
                &mut secret_scan_passed,
            );
        }
    }

    let checks = serde_json::json!({
        "manifest_verified": manifest_verified,
        "sha256sums_verified": sha256sums_verified,
        "project_start_verified": project_start_verified,
        "project_run_verified": project_run_verified,
        "acceptance_rubric_verified": acceptance_rubric_verified,
        "project_acceptance_review_verified": project_acceptance_review_verified,
        "acceptance_rubric_signature_verified": acceptance_rubric_signature_verified,
        "project_acceptance_review_signature_verified": project_acceptance_review_signature_verified,
        "review_rubric_digest_matches": review_rubric_digest_matches,
        "review_project_run_digest_matches": review_project_run_digest_matches,
        "trust_boundary_verified": trust_boundary_verified,
        "secret_scan_passed": secret_scan_passed
    });

    let _ = fs::remove_dir_all(&extract_dir);
    Ok(factory_project_start_bundle_verify_report(
        bundle_path,
        &bundle_sha256,
        artifacts.len(),
        files_checked,
        &checks,
        failures,
    ))
}

fn factory_project_start_bundle_verify_report(
    bundle_path: &Path,
    bundle_sha256: &str,
    artifact_count: usize,
    files_checked: usize,
    checks: &serde_json::Value,
    failures: Vec<serde_json::Value>,
) -> serde_json::Value {
    let status = if failures.is_empty() {
        "accepted"
    } else {
        "failed"
    };
    serde_json::json!({
        "schema_version": "ao2.factory-project-start-bundle-verification.v1",
        "status": status,
        "bundle": bundle_path,
        "bundle_sha256": bundle_sha256,
        "artifact_count": artifact_count,
        "files_checked": files_checked,
        "checks": checks,
        "failure_count": failures.len(),
        "failures": failures,
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    })
}

fn factory_project_start_bundle_verify_checks(
    manifest_verified: bool,
    sha256sums_verified: bool,
) -> serde_json::Value {
    serde_json::json!({
        "manifest_verified": manifest_verified,
        "sha256sums_verified": sha256sums_verified,
        "project_start_verified": false,
        "project_run_verified": false,
        "acceptance_rubric_verified": false,
        "project_acceptance_review_verified": false,
        "acceptance_rubric_signature_verified": false,
        "project_acceptance_review_signature_verified": false,
        "review_rubric_digest_matches": false,
        "review_project_run_digest_matches": false,
        "trust_boundary_verified": false,
        "secret_scan_passed": false
    })
}

fn factory_project_start_bundle_read_json(
    extract_dir: &Path,
    label: &str,
    artifact_paths: &BTreeMap<String, String>,
    failures: &mut Vec<serde_json::Value>,
) -> Option<serde_json::Value> {
    let relative_path = artifact_paths.get(label)?;
    let file_path = extract_dir.join(relative_path);
    match fs::read_to_string(&file_path) {
        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => Some(value),
            Err(error) => {
                failures.push(serde_json::json!({
                    "code": "artifact_json_invalid",
                    "label": label,
                    "path": relative_path,
                    "message": format!("parse json: {error}")
                }));
                None
            }
        },
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "artifact_unreadable",
                "label": label,
                "path": relative_path,
                "message": format!("read artifact: {error}")
            }));
            None
        }
    }
}

fn factory_project_start_bundle_verify_signature(
    extract_dir: &Path,
    signed_value: &serde_json::Value,
    sidecar_prefix: &str,
    failures: &mut Vec<serde_json::Value>,
) -> bool {
    let signature = &signed_value["signature"];
    let mut verified = json_string(signature, "signature_status") == "signed"
        && signature
            .get("signature_verified")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let signed_payload = extract_dir.join(format!("{sidecar_prefix}.signed-payload.json"));
    let signature_path = extract_dir.join(format!("{sidecar_prefix}.json.sig"));
    let public_key = extract_dir.join(format!("{sidecar_prefix}.public.pem"));
    for (field, path) in [
        ("signed_payload_sha256", &signed_payload),
        ("signature_sha256", &signature_path),
        ("public_key_sha256", &public_key),
    ] {
        match sha256_file(path) {
            Ok(actual) if actual == json_string(signature, field) => {}
            Ok(actual) => {
                verified = false;
                failures.push(serde_json::json!({
                    "code": "signature_sidecar_digest_mismatch",
                    "field": field,
                    "path": path,
                    "expected": json_string(signature, field),
                    "actual": actual
                }));
            }
            Err(error) => {
                verified = false;
                failures.push(serde_json::json!({
                    "code": "signature_sidecar_unreadable",
                    "field": field,
                    "path": path,
                    "message": error.to_string()
                }));
            }
        }
    }
    match verify_file_signature(&signed_payload, &signature_path, &public_key) {
        Ok(true) => {}
        Ok(false) => {
            verified = false;
            failures.push(serde_json::json!({
                "code": "signature_verification_failed",
                "path": signed_payload
            }));
        }
        Err(error) => {
            verified = false;
            failures.push(serde_json::json!({
                "code": "signature_verification_error",
                "path": signed_payload,
                "message": error.to_string()
            }));
        }
    }
    let payload_value = match fs::read_to_string(&signed_payload)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
    {
        Some(value) => value,
        None => {
            verified = false;
            failures.push(serde_json::json!({
                "code": "signed_payload_json_invalid",
                "path": signed_payload
            }));
            serde_json::Value::Null
        }
    };
    let mut without_signature = signed_value.clone();
    if let Some(object) = without_signature.as_object_mut() {
        object.remove("signature");
    }
    if !payload_value.is_null() && payload_value != without_signature {
        verified = false;
        failures.push(serde_json::json!({
            "code": "signed_payload_body_mismatch",
            "path": signed_payload,
            "message": "signed payload must equal the JSON artifact without its signature field"
        }));
    }
    verified
}
