use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_policy::{redact_secrets, secret_redaction_class_counts};

use crate::cli_util::{json_array, json_string, json_u64, sha256_file};

pub(crate) fn workbench_support_bundle_path(target: &Path, generated_at_ms: u64) -> PathBuf {
    target
        .join(".ao2")
        .join("workbench")
        .join("support-bundles")
        .join(format!("support-bundle-{generated_at_ms}"))
        .join("support-bundle.json")
}

pub(crate) fn workbench_evidence_exports_for_support_bundle(
    target: &Path,
) -> Result<Vec<serde_json::Value>> {
    let exports_dir = target
        .join(".ao2")
        .join("workbench")
        .join("evidence-exports");
    if !exports_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(&exports_dir)
        .with_context(|| format!("read {}", exports_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let content: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
            )
            .with_context(|| format!("parse {}", path.display()))?;
            if json_string(&content, "schema_version") != "ao2.workbench-evidence-export.v1" {
                return Err(anyhow!(
                    "workbench evidence export must use schema ao2.workbench-evidence-export.v1: {}",
                    path.display()
                ));
            }
            Ok(serde_json::json!({
                "path": path,
                "sha256": sha256_file(&path)?,
                "kind": json_string(&content, "export_kind"),
                "generated_at_ms": content
                    .get("generated_at_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
                "content": content
            }))
        })
        .collect()
}

pub(crate) fn empty_workbench_redaction_audit() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.workbench-support-redaction-audit.v1",
        "redaction_count": 0,
        "secret_classes": {},
        "redacted_fields": []
    })
}

pub(crate) fn workbench_support_bundle_redaction_audit(
    bundle: &serde_json::Value,
) -> serde_json::Value {
    let mut redacted_fields = Vec::new();
    let mut redaction_count = 0usize;
    let mut secret_classes = BTreeMap::<String, usize>::new();
    for (index, log) in bundle["job_logs"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let run_id = log["job"]["run_id"].as_str().unwrap_or_default();
        for field in ["stdout", "stderr"] {
            let Some(value) = log[field].as_str() else {
                continue;
            };
            let redacted = redact_secrets(value);
            if redacted != value {
                let field_secret_classes = secret_redaction_class_counts(value);
                redaction_count += field_secret_classes.values().sum::<usize>();
                for (class, count) in &field_secret_classes {
                    *secret_classes.entry(class.clone()).or_default() += count;
                }
                redacted_fields.push(serde_json::json!({
                    "path": format!("job_logs[{index}].{field}"),
                    "run_id": run_id,
                    "field": field,
                    "secret_classes": field_secret_classes,
                    "redacted_excerpt": workbench_redaction_excerpt(&redacted)
                }));
            }
        }
    }
    serde_json::json!({
        "schema_version": "ao2.workbench-support-redaction-audit.v1",
        "redaction_count": redaction_count,
        "secret_classes": secret_classes,
        "redacted_fields": redacted_fields
    })
}

pub(crate) fn workbench_support_bundle_redaction_preview(
    bundle: &serde_json::Value,
) -> serde_json::Value {
    let audit = workbench_support_bundle_redaction_audit(bundle);
    serde_json::json!({
        "schema_version": "ao2.workbench-support-redaction-preview.v1",
        "redaction_count": json_u64(&audit, "redaction_count"),
        "secret_classes": audit["secret_classes"].clone(),
        "redacted_fields": json_array(&audit, "redacted_fields")
    })
}

fn workbench_redaction_excerpt(value: &str) -> String {
    const MAX_CHARS: usize = 240;
    let excerpt = value
        .lines()
        .find(|line| line.contains("[REDACTED]"))
        .unwrap_or(value);
    excerpt.chars().take(MAX_CHARS).collect()
}
