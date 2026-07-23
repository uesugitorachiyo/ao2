use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli_util::sha256_file;

pub(crate) fn verify_release_archive_offline_contract(
    extract_dir: &Path,
    manifest: &serde_json::Value,
    target: &str,
    binary_name: &str,
) -> Result<serde_json::Value> {
    let verification_report = manifest["verification_report"]
        .as_str()
        .context("release manifest missing release verification report")?;
    if verification_report != "RELEASE-VERIFICATION.json" {
        anyhow::bail!("release verification report must be RELEASE-VERIFICATION.json");
    }
    ensure_safe_release_archive_path(verification_report, "release verification report")?;
    let report_path = extract_dir.join(verification_report);
    let report_text = fs::read_to_string(&report_path)
        .with_context(|| format!("read release verification report {}", report_path.display()))?;
    let report: serde_json::Value = serde_json::from_str(&report_text).with_context(|| {
        format!(
            "parse release verification report {}",
            report_path.display()
        )
    })?;
    if report["schema_version"] != "ao2.release-archive-offline-verification.v1" {
        anyhow::bail!("unexpected release verification report schema");
    }
    if report["status"] != "packaged" {
        anyhow::bail!("release verification report status must be packaged");
    }
    if report["target"] != target {
        anyhow::bail!("release verification report target does not match install target");
    }
    if report["binary"] != binary_name {
        anyhow::bail!("release verification report binary does not match install target");
    }
    let binary_path = manifest["binary_path"]
        .as_str()
        .context("release manifest missing binary_path")?;
    if report["binary_path"] != binary_path {
        anyhow::bail!("release verification report binary_path does not match release manifest");
    }
    let manifest_checksum_file = manifest["checksum_file"]
        .as_str()
        .context("release manifest missing checksum_file")?;
    let report_checksum_file = report["checksum_file"]
        .as_str()
        .context("release verification report missing checksum_file")?;
    if manifest_checksum_file != "SHA256SUMS" || report_checksum_file != "SHA256SUMS" {
        anyhow::bail!("release archive checksum file must be SHA256SUMS");
    }
    let checksum_path = extract_dir.join("SHA256SUMS");
    let checksum_text = fs::read_to_string(&checksum_path).with_context(|| {
        format!(
            "read release archive checksum file {}",
            checksum_path.display()
        )
    })?;
    let checksum_manifest = parse_release_archive_sha256sums(&checksum_text)?;
    if checksum_manifest.is_empty() {
        anyhow::bail!("release archive SHA256SUMS must not be empty");
    }

    for (relative_path, expected_sha) in &checksum_manifest {
        ensure_safe_release_archive_path(relative_path, "release archive SHA256SUMS")?;
        let file_path = extract_dir.join(relative_path);
        if !file_path.is_file() {
            anyhow::bail!("SHA256SUMS references missing release archive file {relative_path}");
        }
        let actual_sha = sha256_file(&file_path)?;
        if actual_sha != *expected_sha {
            anyhow::bail!("release archive checksum mismatch for {relative_path}");
        }
    }

    let expected_binary_sha = manifest["binary_sha256"]
        .as_str()
        .context("release manifest missing binary_sha256")?;
    match checksum_manifest.get(binary_path) {
        Some(checksum_sha) if checksum_sha == expected_binary_sha => {}
        Some(_) => anyhow::bail!("release manifest binary checksum does not match SHA256SUMS"),
        None => anyhow::bail!("SHA256SUMS must include packaged binary {binary_path}"),
    }

    let coverage = report["checksum_coverage"]
        .as_array()
        .context("release verification report missing checksum_coverage")?;
    let mut coverage_paths = BTreeSet::new();
    for value in coverage {
        let relative_path = value
            .as_str()
            .context("release verification checksum_coverage entries must be strings")?;
        ensure_safe_release_archive_path(relative_path, "release verification checksum_coverage")?;
        coverage_paths.insert(relative_path.to_string());
        if !checksum_manifest.contains_key(relative_path) {
            anyhow::bail!("SHA256SUMS must include checksum coverage entry {relative_path}");
        }
    }

    for required in [
        binary_path,
        "RELEASE-MANIFEST.json",
        "RELEASE-VERIFICATION.json",
        "install.sh",
        "install.ps1",
        "verify-release.sh",
        "Verify-Release.ps1",
        "README.txt",
        "VERSION",
    ] {
        if !coverage_paths.contains(required) {
            anyhow::bail!("release verification checksum_coverage must include {required}");
        }
        if !checksum_manifest.contains_key(required) {
            anyhow::bail!("SHA256SUMS must include {required}");
        }
    }

    require_release_report_false(&report, "provider_api_keys_required")?;
    require_release_report_false(&report, "control_plane_approves_release")?;
    require_release_report_false(&report, "mutates_ao_artifacts")?;
    if report["release_acceptance_owner"] != "factory-v3 evaluator-closer" {
        anyhow::bail!("release acceptance owner must be factory-v3 evaluator-closer");
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.release-archive-offline-verification.v1",
        "status": "verified",
        "checksum_file": "SHA256SUMS",
        "verification_report": verification_report,
        "checksum_coverage_verified": true,
        "provider_api_keys_required": false,
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "release_acceptance_owner": "factory-v3 evaluator-closer"
    }))
}

fn parse_release_archive_sha256sums(text: &str) -> Result<BTreeMap<String, String>> {
    let mut manifest = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let digest = parts
            .next()
            .with_context(|| format!("missing digest on SHA256SUMS line {}", index + 1))?;
        let relative_path = parts
            .next()
            .with_context(|| format!("missing path on SHA256SUMS line {}", index + 1))?;
        if parts.next().is_some() {
            anyhow::bail!("invalid SHA256SUMS line {}", index + 1);
        }
        if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            anyhow::bail!("invalid digest on SHA256SUMS line {}", index + 1);
        }
        ensure_safe_release_archive_path(relative_path, "release archive SHA256SUMS")?;
        if manifest
            .insert(relative_path.to_string(), digest.to_ascii_lowercase())
            .is_some()
        {
            anyhow::bail!("duplicate SHA256SUMS entry {relative_path}");
        }
    }
    Ok(manifest)
}

pub(crate) fn ensure_safe_release_archive_path(relative_path: &str, label: &str) -> Result<()> {
    if relative_path.trim().is_empty() {
        anyhow::bail!("{label} path must not be empty");
    }
    let path = Path::new(relative_path);
    if path.is_absolute() {
        anyhow::bail!("{label} contains an absolute or parent-directory path");
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                anyhow::bail!("{label} contains an absolute or parent-directory path");
            }
        }
    }
    Ok(())
}

fn require_release_report_false(report: &serde_json::Value, key: &str) -> Result<()> {
    match report.get(key).and_then(serde_json::Value::as_bool) {
        Some(false) => Ok(()),
        Some(true) => anyhow::bail!("release verification report {key} must be false"),
        None => anyhow::bail!("release verification report missing boolean {key}"),
    }
}
