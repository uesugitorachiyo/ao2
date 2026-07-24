use std::path::Path;

use crate::cli_util::{json_array, json_string, json_u64};

pub(crate) fn workbench_provider_pilot_acceptance_trend_json(
    acceptance_history: &[serde_json::Value],
) -> serde_json::Value {
    let current = acceptance_history.first();
    let previous = acceptance_history.get(1);
    let current_score = current.map(|entry| json_u64(entry, "score")).unwrap_or(0);
    let previous_score = previous.map(|entry| json_u64(entry, "score"));
    let score_delta = previous_score.map(|score| current_score as i64 - score as i64);
    let accepted_count = acceptance_history
        .iter()
        .filter(|entry| json_string(entry, "replay_status") == "accepted")
        .count();
    let ready_count = acceptance_history
        .iter()
        .filter(|entry| json_string(entry, "verdict") == "ready")
        .count();
    let best_score = acceptance_history
        .iter()
        .map(|entry| json_u64(entry, "score"))
        .max()
        .unwrap_or(0);
    let worst_score = acceptance_history
        .iter()
        .map(|entry| json_u64(entry, "score"))
        .min()
        .unwrap_or(0);

    serde_json::json!({
        "schema_version": "ao2.workbench-provider-pilot-acceptance-trend.v1",
        "total_count": acceptance_history.len(),
        "accepted_count": accepted_count,
        "ready_count": ready_count,
        "current_release_tag": current.map(|entry| json_string(entry, "release_tag")).unwrap_or_default(),
        "current_provider": current.map(|entry| json_string(entry, "provider")).unwrap_or_default(),
        "current_run_id": current.map(|entry| json_string(entry, "run_id")).unwrap_or_default(),
        "current_score": current_score,
        "previous_release_tag": previous.map(|entry| json_string(entry, "release_tag")),
        "previous_run_id": previous.map(|entry| json_string(entry, "run_id")),
        "previous_score": previous_score,
        "score_delta": score_delta,
        "regression": score_delta.is_some_and(|delta| delta < 0),
        "best_score": best_score,
        "worst_score": worst_score
    })
}

pub(crate) fn sort_workbench_provider_pilot_acceptance_history(
    acceptance_history: &mut [serde_json::Value],
    sort: &str,
) {
    match sort {
        "score_asc" => acceptance_history.sort_by(|left, right| {
            json_u64(left, "score")
                .cmp(&json_u64(right, "score"))
                .then_with(|| {
                    json_string(right, "release_tag").cmp(&json_string(left, "release_tag"))
                })
        }),
        "score_desc" => acceptance_history.sort_by(|left, right| {
            json_u64(right, "score")
                .cmp(&json_u64(left, "score"))
                .then_with(|| {
                    json_string(right, "release_tag").cmp(&json_string(left, "release_tag"))
                })
        }),
        "provider_asc" => acceptance_history.sort_by(|left, right| {
            json_string(left, "provider")
                .cmp(&json_string(right, "provider"))
                .then_with(|| {
                    json_string(right, "release_tag").cmp(&json_string(left, "release_tag"))
                })
        }),
        "run_id_asc" => acceptance_history.sort_by(|left, right| {
            json_string(left, "run_id")
                .cmp(&json_string(right, "run_id"))
                .then_with(|| {
                    json_string(right, "release_tag").cmp(&json_string(left, "release_tag"))
                })
        }),
        _ => {}
    }
}

pub(crate) fn workbench_provider_pilot_acceptance_history_entry(
    bundle: &Path,
    acceptance: &serde_json::Value,
) -> serde_json::Value {
    let release_tag = bundle
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    serde_json::json!({
        "acceptance_bundle": bundle,
        "release_tag": release_tag,
        "provider": json_string(acceptance, "provider"),
        "run_id": json_string(acceptance, "run_id"),
        "status": json_string(acceptance, "status"),
        "score": json_u64(&acceptance["score"], "score"),
        "verdict": json_string(&acceptance["score"], "verdict"),
        "replay_status": json_string(&acceptance["replay"], "status"),
        "digest_failure_count": json_array(&acceptance["replay"], "digest_failures").len(),
        "evidence_pack": json_string(acceptance, "evidence_pack"),
        "cockpit": json_string(acceptance, "cockpit")
    })
}
