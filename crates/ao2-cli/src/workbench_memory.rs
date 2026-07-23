use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::control_plane_http::{control_plane_endpoint, get_text_http};
use crate::memory_store::{
    append_jsonl, memory_link_run_json, memory_recent_json, memory_run_links_path,
    memory_search_json,
};
use crate::{
    memory_export_json, memory_publish_to_control_plane_json, now_unix_ms, query_value_owned,
    WorkbenchSupportSigning,
};

pub(crate) fn workbench_memory_search_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let memory_query = query_value_owned(query, "query")
        .or_else(|| query_value_owned(query, "q"))
        .context("query is required")?;
    let limit = query_value_owned(query, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10);
    memory_search_json(target, &memory_query, limit)
}

pub(crate) fn workbench_memory_recent_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let limit = query_value_owned(query, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10);
    memory_recent_json(target, limit)
}

pub(crate) fn workbench_memory_export_json(
    target: &Path,
    form: &BTreeMap<String, String>,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let query = form
        .get("query")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = form
        .get("limit")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(50);
    let generated_at_ms = now_unix_ms();
    let out = form
        .get("out")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            target
                .join(".ao2")
                .join("workbench")
                .join("memory-exports")
                .join(format!("memory-export-{generated_at_ms}.json"))
        });
    let signing_key = support_signing.map(|signing| signing.key_path.clone());
    let signer_id = support_signing
        .map(|signing| signing.signer_id.clone())
        .unwrap_or_else(|| "ao2-memory".to_string());
    memory_export_json(
        target,
        query.as_deref(),
        limit,
        &out,
        signing_key,
        signer_id,
    )
}

pub(crate) fn workbench_memory_publish_latest_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let control_plane_url = form
        .get("control_plane_url")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("control_plane_url is required")?;
    let api_token = form
        .get("api_token")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("api_token is required")?;
    // Keep workbench uploads default-on signed, matching `ao2 memory publish`,
    // unless the operator explicitly opts out for local/plain export handling.
    let allow_unsigned_memory_export = form
        .get("allow_unsigned_memory_export")
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    let require_signed_export = !allow_unsigned_memory_export;
    let export_path = latest_workbench_memory_export_path(target)?;
    memory_publish_to_control_plane_json(
        &export_path,
        &control_plane_url,
        &api_token,
        require_signed_export,
    )
}

pub(crate) fn workbench_memory_control_plane_dashboard_json(
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let control_plane_url = form
        .get("control_plane_url")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("control_plane_url is required")?;
    let api_token = form
        .get("api_token")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("api_token is required")?;
    let endpoint = control_plane_endpoint(&control_plane_url, "/api/v1/memory/export/dashboard")?;
    let dashboard_html = get_text_http(&endpoint, &api_token)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.memory-control-plane-dashboard.v1",
        "endpoint": endpoint,
        "dashboard_html": dashboard_html
    }))
}

fn latest_workbench_memory_export_path(target: &Path) -> Result<PathBuf> {
    let export_dir = target.join(".ao2").join("workbench").join("memory-exports");
    let mut exports = fs::read_dir(&export_dir)
        .with_context(|| format!("read {}", export_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext == "json")
        })
        .collect::<Vec<_>>();
    exports.sort();
    exports
        .pop()
        .ok_or_else(|| anyhow!("no memory exports found under {}", export_dir.display()))
}

pub(crate) fn workbench_memory_link_run_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let memory_id = form
        .get("memory_id")
        .cloned()
        .context("memory_id is required")?;
    let run_id = form.get("run_id").cloned().context("run_id is required")?;
    let relationship = form
        .get("relationship")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "related".to_string());
    let link = memory_link_run_json(target, memory_id, run_id, relationship)?;
    append_jsonl(&memory_run_links_path(target), &link)?;
    Ok(link)
}
