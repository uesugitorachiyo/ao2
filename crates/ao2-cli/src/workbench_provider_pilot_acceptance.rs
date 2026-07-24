use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::{json_array, json_string, json_u64};

pub(crate) fn provider_pilot_acceptance_verification_json(
    acceptance_bundle: &Path,
) -> Result<serde_json::Value> {
    let content = fs::read_to_string(acceptance_bundle)
        .with_context(|| format!("read {}", acceptance_bundle.display()))?;
    let acceptance: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse {}", acceptance_bundle.display()))?;
    let schema_version = json_string(&acceptance, "schema_version");
    if schema_version != "ao2.codex-provider-pilot-acceptance.v1"
        && schema_version != "ao2.claude-provider-pilot-acceptance.v1"
        && schema_version != "ao2.antigravity-provider-pilot-acceptance.v1"
    {
        anyhow::bail!(
            "provider pilot acceptance bundle must use ao2.codex-provider-pilot-acceptance.v1, ao2.claude-provider-pilot-acceptance.v1, or ao2.antigravity-provider-pilot-acceptance.v1: {}",
            acceptance_bundle.display()
        );
    }
    if json_string(&acceptance, "status") != "passed" {
        anyhow::bail!(
            "provider pilot acceptance bundle must have status=passed: status={}",
            json_string(&acceptance, "status")
        );
    }
    let provider = json_string(&acceptance, "provider");
    if provider != "codex" && provider != "claude" && provider != "antigravity" {
        anyhow::bail!(
            "provider pilot acceptance provider must be codex, claude, or antigravity: {provider}"
        );
    }
    if json_string(&acceptance, "run_id").is_empty() {
        anyhow::bail!("provider pilot acceptance bundle must include run_id");
    }
    if json_string(&acceptance["replay"], "status") != "accepted" {
        anyhow::bail!(
            "provider pilot acceptance replay must be accepted: status={}",
            json_string(&acceptance["replay"], "status")
        );
    }
    let digest_failures = json_array(&acceptance["replay"], "digest_failures");
    if !digest_failures.is_empty() {
        anyhow::bail!(
            "provider pilot acceptance replay must have zero digest failures: count={}",
            digest_failures.len()
        );
    }
    if json_string(&acceptance["score"], "verdict") != "ready" {
        anyhow::bail!(
            "provider pilot acceptance score verdict must be ready: verdict={}",
            json_string(&acceptance["score"], "verdict")
        );
    }
    let score = json_u64(&acceptance["score"], "score");
    if score < 90 {
        anyhow::bail!("provider pilot acceptance score must be at least 90: score={score}");
    }
    Ok(acceptance)
}

pub(crate) fn collect_provider_pilot_acceptance_bundles(
    root: &Path,
    bundles: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_provider_pilot_acceptance_bundles(&path, bundles)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "provider-pilot-acceptance.json")
        {
            bundles.push(path);
        }
    }
    Ok(())
}

pub(crate) fn provider_cost_ledger_release_tag(root: &Path, bundle: &Path) -> String {
    bundle
        .strip_prefix(root)
        .ok()
        .and_then(|relative| {
            relative.components().find_map(|component| {
                let text = component.as_os_str().to_string_lossy();
                text.starts_with('v').then(|| text.to_string())
            })
        })
        .unwrap_or_default()
}

pub(crate) fn provider_pilot_acceptance_sort_name(root: &Path, bundle: &Path) -> String {
    let release_tag = provider_cost_ledger_release_tag(root, bundle);
    if !release_tag.is_empty() {
        return release_tag;
    }
    bundle
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}
