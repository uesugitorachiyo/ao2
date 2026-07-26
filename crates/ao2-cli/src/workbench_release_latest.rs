use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli_util::{json_array, json_string, query_value_owned};
use crate::release_comparison::release_comparison_bundle_verification_json;

pub(crate) fn workbench_latest_release_comparison_json(query: &str) -> Result<serde_json::Value> {
    let bundle_root = query_value_owned(query, "bundle_root")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/release-comparison-bundles"));
    if !bundle_root.is_dir() {
        anyhow::bail!(
            "release comparison bundle root does not exist: {}",
            bundle_root.display()
        );
    }
    let mut bundle_dirs = fs::read_dir(&bundle_root)
        .with_context(|| format!("read {}", bundle_root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("release-comparison-"))
        })
        .collect::<Vec<_>>();
    bundle_dirs.sort_by(|left, right| {
        right
            .file_name()
            .cmp(&left.file_name())
            .then_with(|| right.cmp(left))
    });

    let mut candidates_checked = 0_u64;
    let mut failed_candidates = Vec::new();
    for bundle_dir in bundle_dirs {
        candidates_checked += 1;
        match release_comparison_bundle_verification_json(&bundle_dir) {
            Ok(verification) if json_string(&verification, "status") == "verified" => {
                return Ok(serde_json::json!({
                    "schema_version": "ao2.workbench-latest-release-comparison.v1",
                    "bundle_root": bundle_root,
                    "bundle_dir": bundle_dir,
                    "candidates_checked": candidates_checked,
                    "failed_candidates": failed_candidates,
                    "verification": verification
                }));
            }
            Ok(verification) => {
                failed_candidates.push(serde_json::json!({
                    "bundle_dir": bundle_dir,
                    "status": json_string(&verification, "status"),
                    "reasons": json_array(&verification, "reasons")
                }));
            }
            Err(error) => {
                failed_candidates.push(serde_json::json!({
                    "bundle_dir": bundle_dir,
                    "status": "error",
                    "error": error.to_string()
                }));
            }
        }
    }
    anyhow::bail!(
        "no verified release comparison bundle found under {}",
        bundle_root.display()
    )
}
