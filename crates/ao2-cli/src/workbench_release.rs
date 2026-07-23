use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::json_string;
use crate::doctor_cmd::doctor_report_json;
use crate::{
    query_value_owned, release_comparison_bundle_json, release_comparison_bundle_verification_json,
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
