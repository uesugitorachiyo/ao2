use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::json_string;
use crate::doctor_cmd::doctor_report_json;
use crate::release_summary_enrich::release_summary_enrich_report_json;
use crate::{
    atomic_write_text, form_value_owned, query_value_owned, release_comparison_bundle_json,
    release_comparison_bundle_verification_json, release_comparison_dir_sort_key,
    release_dir_sort_key, release_gate_report_json, release_retention_plan_dirs,
    workbench_release_history_for_dir, WorkbenchSupportSigning,
};

pub(crate) fn workbench_release_health_json(
    query: &str,
    default_provenance_dir: &Path,
) -> Result<serde_json::Value> {
    let release = query_value_owned(query, "release")
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
    let provenance_dir = query_value_owned(query, "provenance_dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_provenance_dir.to_path_buf());
    let release_asset_dir = query_value_owned(query, "release_asset_dir").map(PathBuf::from);
    let release_repo = query_value_owned(query, "release_repo")
        .unwrap_or_else(|| "uesugitorachiyo/ao2".to_string());
    doctor_report_json(
        None,
        provenance_dir,
        Some(release),
        release_asset_dir,
        release_repo,
    )
}

pub(crate) fn workbench_release_history_json(query: &str) -> Result<serde_json::Value> {
    let release_download_dir = query_value_owned(query, "release_download_dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release-download"));
    workbench_release_history_for_dir(release_download_dir)
}

pub(crate) fn workbench_release_gate_artifact_json(query: &str) -> Result<serde_json::Value> {
    let path = query_value_owned(query, "path")
        .map(PathBuf::from)
        .context("path is required")?;
    let body = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let artifact: serde_json::Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))?;
    Ok(serde_json::json!({
        "schema": "ao2.workbench-release-gate-artifact.v1",
        "path": path,
        "artifact": artifact
    }))
}

pub(crate) fn workbench_release_comparison_json(
    form: &BTreeMap<String, String>,
    signing: &WorkbenchSupportSigning,
) -> Result<serde_json::Value> {
    let release_download_dir = form
        .get("release_download_dir")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release-download"));
    let out_dir = form
        .get("out_dir")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release-comparison-bundles"));
    let release_comparison = release_comparison_bundle_json(
        release_download_dir,
        out_dir,
        Some(signing.key_path.as_path()),
        &signing.signer_id,
    )?;
    let bundle_dir = PathBuf::from(json_string(&release_comparison, "bundle_dir"));
    let verification = release_comparison_bundle_verification_json(&bundle_dir)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-release-comparison.v1",
        "release_comparison": release_comparison,
        "verification": verification
    }))
}

pub(crate) fn workbench_release_comparison_verification_json(
    query: &str,
) -> Result<serde_json::Value> {
    let bundle_dir = query_value_owned(query, "bundle_dir")
        .map(PathBuf::from)
        .context("bundle_dir is required")?;
    let verification = release_comparison_bundle_verification_json(&bundle_dir)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-release-comparison-verification.v1",
        "bundle_dir": bundle_dir,
        "verification": verification
    }))
}

pub(crate) fn workbench_release_retention_prune_json(
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let release_download_dir = form
        .get("release_download_dir")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release-download"));
    let bundle_root = form
        .get("bundle_root")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release-comparison-bundles"));
    let keep_releases = form
        .get("keep_releases")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(5);
    let keep_bundles = form
        .get("keep_bundles")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(5);
    if keep_releases == 0 || keep_bundles == 0 {
        anyhow::bail!("release retention keep counts must be greater than 0");
    }
    let dry_run = form
        .get("dry_run")
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(true);

    let (kept_release_dirs, removed_release_dirs) = release_retention_plan_dirs(
        &release_download_dir,
        keep_releases,
        |name| name.starts_with('v'),
        release_dir_sort_key,
    )?;
    let (kept_bundle_dirs, removed_bundle_dirs) = release_retention_plan_dirs(
        &bundle_root,
        keep_bundles,
        |name| name.starts_with("release-comparison-"),
        release_comparison_dir_sort_key,
    )?;
    if !dry_run {
        for path in removed_release_dirs
            .iter()
            .chain(removed_bundle_dirs.iter())
        {
            if path.exists() {
                fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
            }
        }
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-release-retention-prune.v1",
        "dry_run": dry_run,
        "release_download_dir": release_download_dir,
        "bundle_root": bundle_root,
        "keep_releases": keep_releases,
        "keep_bundles": keep_bundles,
        "kept_release_count": kept_release_dirs.len(),
        "removed_release_count": removed_release_dirs.len(),
        "kept_bundle_count": kept_bundle_dirs.len(),
        "removed_bundle_count": removed_bundle_dirs.len(),
        "total_removed_count": removed_release_dirs.len() + removed_bundle_dirs.len(),
        "kept_release_dirs": kept_release_dirs,
        "removed_release_dirs": removed_release_dirs,
        "kept_bundle_dirs": kept_bundle_dirs,
        "removed_bundle_dirs": removed_bundle_dirs
    }))
}

pub(crate) fn workbench_release_summary_enrich_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let summary = PathBuf::from(form_value_owned(form, "summary").context("summary is required")?);
    let out = PathBuf::from(form_value_owned(form, "out").context("out is required")?);
    let run_id = form_value_owned(form, "run_id");
    release_summary_enrich_report_json(summary, target.to_path_buf(), run_id, Vec::new(), out)
}

pub(crate) fn workbench_release_gate_json(
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let summary = PathBuf::from(form_value_owned(form, "summary").context("summary is required")?);
    let provenance_dir = PathBuf::from(
        form_value_owned(form, "provenance_dir").context("provenance_dir is required")?,
    );
    let macos_archive = PathBuf::from(
        form_value_owned(form, "macos_archive").context("macos_archive is required")?,
    );
    let linux_archive = PathBuf::from(
        form_value_owned(form, "linux_archive").context("linux_archive is required")?,
    );
    let linux_x86_64_archive = PathBuf::from(
        form_value_owned(form, "linux_x86_64_archive")
            .context("linux_x86_64_archive is required")?,
    );
    let windows_archive = PathBuf::from(
        form_value_owned(form, "windows_archive").context("windows_archive is required")?,
    );
    let require_native_windows = form
        .get("require_native_windows")
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    // Mirror the CLI default-on flip from slice 11: obligation-gate signing is
    // required by default; operators must explicitly opt out with the
    // `allow_unsigned_obligation_gates` form param. The legacy
    // `require_obligation_gate_signing` form param is accepted but ignored so
    // pre-slice-17 callers that explicitly opted in continue to parse cleanly.
    let allow_unsigned_obligation_gates = form
        .get("allow_unsigned_obligation_gates")
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    let _legacy_require_obligation_gate_signing = form
        .get("require_obligation_gate_signing")
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    let require_obligation_gate_signing = !allow_unsigned_obligation_gates;
    let replacement_smoke_gate =
        form_value_owned(form, "replacement_smoke_gate").map(PathBuf::from);
    let mut report = release_gate_report_json(
        summary,
        provenance_dir,
        Some(macos_archive),
        linux_archive,
        linux_x86_64_archive,
        windows_archive,
        require_native_windows,
        replacement_smoke_gate,
        None,
        Vec::new(),
        Vec::new(),
        require_obligation_gate_signing,
    )?;
    if let Some(artifact_out) = form_value_owned(form, "artifact_out").map(PathBuf::from) {
        if let Some(object) = report.as_object_mut() {
            object.insert(
                "artifact_path".to_string(),
                serde_json::json!(artifact_out.clone()),
            );
        }
        atomic_write_text(&artifact_out, &serde_json::to_string_pretty(&report)?)?;
    }
    Ok(report)
}
