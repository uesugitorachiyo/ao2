use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli_util::json_string;
use crate::{
    form_value_owned, provider_cost_ledger_json, provider_cost_trend_json, query_value_owned,
    workbench_evidence_export_json, workbench_latest_provider_pilot_acceptance_for,
    WorkbenchProviderPilotAcceptanceFilter,
};

pub(crate) fn workbench_latest_provider_pilot_acceptance_json(
    query: &str,
) -> Result<serde_json::Value> {
    let acceptance_root = query_value_owned(query, "acceptance_root")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/provider-pilot-acceptance"));
    let filter = WorkbenchProviderPilotAcceptanceFilter::from_query(query);
    workbench_latest_provider_pilot_acceptance_for(acceptance_root, filter)
}

pub(crate) fn workbench_provider_pilot_cost_ledger_json(query: &str) -> Result<serde_json::Value> {
    let acceptance_root = query_value_owned(query, "acceptance_root")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/provider-pilot-acceptance"));
    provider_cost_ledger_json(&acceptance_root)
}

pub(crate) fn workbench_provider_pilot_cost_trend_json(query: &str) -> Result<serde_json::Value> {
    let acceptance_root = query_value_owned(query, "acceptance_root")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/provider-pilot-acceptance"));
    provider_cost_trend_json(&acceptance_root)
}

pub(crate) fn workbench_export_latest_provider_pilot_acceptance_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let acceptance_root = form_value_owned(form, "acceptance_root")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/provider-pilot-acceptance"));
    let filter = WorkbenchProviderPilotAcceptanceFilter::from_form(form);
    let latest = workbench_latest_provider_pilot_acceptance_for(acceptance_root, filter)?;
    let acceptance_bundle = json_string(&latest, "acceptance_bundle");
    if acceptance_bundle.is_empty() {
        anyhow::bail!("latest provider pilot acceptance did not include acceptance_bundle");
    }
    let mut export_form = BTreeMap::new();
    export_form.insert("kind".to_string(), "provider-pilot-acceptance".to_string());
    export_form.insert("acceptance_bundle".to_string(), acceptance_bundle);
    let export = workbench_evidence_export_json(target, &export_form)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-provider-pilot-acceptance-export-latest.v1",
        "latest": latest,
        "export": export
    }))
}
