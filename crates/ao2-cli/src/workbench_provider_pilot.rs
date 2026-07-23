use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli_util::{json_string, json_u64};
use crate::{
    form_value_owned, provider_cost_ledger_json, provider_cost_trend_json, query_value_owned,
    workbench_evidence_export_json, workbench_latest_provider_pilot_acceptance_for,
};

#[derive(Clone, Debug)]
pub(crate) struct WorkbenchProviderPilotAcceptanceFilter {
    pub(crate) provider: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) replay_status: Option<String>,
    pub(crate) verdict: Option<String>,
    pub(crate) min_score: Option<u64>,
    pub(crate) limit: Option<usize>,
    pub(crate) sort: String,
}

impl WorkbenchProviderPilotAcceptanceFilter {
    pub(crate) fn from_query(query: &str) -> Self {
        Self {
            provider: query_value_owned(query, "provider"),
            status: query_value_owned(query, "history_status"),
            replay_status: query_value_owned(query, "history_replay_status"),
            verdict: query_value_owned(query, "history_verdict"),
            min_score: query_value_owned(query, "history_min_score")
                .and_then(|value| value.parse::<u64>().ok()),
            limit: query_value_owned(query, "history_limit")
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|limit| *limit > 0),
            sort: query_value_owned(query, "history_sort").unwrap_or_else(|| "newest".to_string()),
        }
    }

    pub(crate) fn from_form(form: &BTreeMap<String, String>) -> Self {
        Self {
            provider: form_value_owned(form, "provider"),
            status: form_value_owned(form, "history_status"),
            replay_status: form_value_owned(form, "history_replay_status"),
            verdict: form_value_owned(form, "history_verdict"),
            min_score: form_value_owned(form, "history_min_score")
                .and_then(|value| value.parse::<u64>().ok()),
            limit: form_value_owned(form, "history_limit")
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|limit| *limit > 0),
            sort: form_value_owned(form, "history_sort").unwrap_or_else(|| "newest".to_string()),
        }
    }

    pub(crate) fn matches(&self, acceptance: &serde_json::Value) -> bool {
        if self
            .provider
            .as_deref()
            .is_some_and(|provider| json_string(acceptance, "provider") != provider)
        {
            return false;
        }
        if self
            .status
            .as_deref()
            .is_some_and(|status| json_string(acceptance, "status") != status)
        {
            return false;
        }
        if self
            .replay_status
            .as_deref()
            .is_some_and(|status| json_string(&acceptance["replay"], "status") != status)
        {
            return false;
        }
        if self
            .verdict
            .as_deref()
            .is_some_and(|verdict| json_string(&acceptance["score"], "verdict") != verdict)
        {
            return false;
        }
        if self
            .min_score
            .is_some_and(|min_score| json_u64(&acceptance["score"], "score") < min_score)
        {
            return false;
        }
        true
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "provider": self.provider,
            "status": self.status,
            "replay_status": self.replay_status,
            "verdict": self.verdict,
            "min_score": self.min_score,
            "limit": self.limit,
            "sort": self.sort
        })
    }
}

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
