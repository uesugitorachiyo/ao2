use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_runtime::{replay_run, ReplayOptions};

use crate::cli_util::{json_array, json_string, json_u64, query_value_owned, run_dir};
use crate::evidence_publish::obligation_gate_history_json;
use crate::provider_ops::provider_score_json;
use crate::run_reporting::{run_summary_json, runs_list_json};

pub(crate) fn workbench_run_evidence_summary_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let run_id = query_value_owned(query, "run_id").context("run_id is required")?;
    let run = run_summary_json(target, &run_id)?;
    let run_dir = run_dir(target, &run_id);
    let evidence_pack_path = run_dir.join("evidence-pack").join("evidence-pack.json");
    let evidence_pack_text = fs::read_to_string(&evidence_pack_path)
        .with_context(|| format!("read {}", evidence_pack_path.display()))?;
    let evidence_pack: serde_json::Value = serde_json::from_str(&evidence_pack_text)
        .with_context(|| format!("parse {}", evidence_pack_path.display()))?;
    let replay = replay_run(ReplayOptions {
        target_repo: target.to_path_buf(),
        run_id: run_id.clone(),
    })?;
    let replay_status_value = serde_json::to_value(replay.status)?;
    let replay_status = replay_status_value
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let provider_summaries = json_array(&evidence_pack, "provider_summaries").to_vec();
    let closures = json_array(&evidence_pack, "closures").to_vec();
    let (scorecard, scorecard_error) = match provider_score_json(target, &run_id) {
        Ok(value) => (
            serde_json::json!({
                "present": true,
                "schema": json_string(&value, "schema"),
                "score": json_u64(&value, "score"),
                "verdict": json_string(&value, "verdict"),
                "provider_summary_count": json_u64(&value, "provider_summary_count"),
                "details": value
            }),
            String::new(),
        ),
        Err(error) => (
            serde_json::json!({
                "present": false
            }),
            error.to_string(),
        ),
    };
    let report_path = PathBuf::from(json_string(&run, "report"));
    let report_sections = report_sections_from_html(&report_path);

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-run-evidence-summary.v1",
        "run_id": run_id,
        "workflow_id": json_string(&run, "workflow_id"),
        "objective": json_string(&run, "objective"),
        "status": json_string(&run, "status"),
        "verdict": json_string(&run, "verdict"),
        "replay": {
            "status": replay_status,
            "event_count": replay.event_count,
            "artifact_count": replay.artifact_count,
            "digest_failures": replay.digest_failures.len()
        },
        "scorecard": scorecard,
        "scorecard_error": scorecard_error,
        "provider_summaries": provider_summaries,
        "closures": closures,
        "obligation_ledger": run
            .get("obligation_ledger")
            .cloned()
            .unwrap_or(serde_json::json!({"present": false})),
        "obligation_gates": obligation_gate_history_json(&run_dir.join("evidence-pack")),
        "run_record": json_string(&run, "run_record"),
        "evidence_pack": json_string(&run, "evidence_pack"),
        "static_report": json_string(&run, "report"),
        "report_sections": report_sections,
        "cockpit": json_string(&run, "cockpit")
    }))
}

fn report_sections_from_html(report_path: &Path) -> Vec<String> {
    let Ok(html) = fs::read_to_string(report_path) else {
        return Vec::new();
    };
    [
        "Local Run Record",
        "Static Export Evidence",
        "Evaluator Closure Evidence",
        "Replay Evidence",
    ]
    .into_iter()
    .filter(|section| html.contains(section))
    .map(str::to_string)
    .collect()
}

fn workbench_run_evidence_diff_member(summary: &serde_json::Value) -> serde_json::Value {
    let scorecard = summary.get("scorecard").unwrap_or(&serde_json::Value::Null);
    let provider_score_present = scorecard
        .get("present")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let provider_score = if provider_score_present {
        scorecard
            .get("score")
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    let closure_verdicts = json_array(summary, "closures")
        .iter()
        .map(|closure| {
            serde_json::json!({
                "role": json_string(closure, "role"),
                "verdict": json_string(closure, "verdict")
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "run_id": json_string(summary, "run_id"),
        "status": json_string(summary, "status"),
        "verdict": json_string(summary, "verdict"),
        "replay_status": summary
            .get("replay")
            .and_then(|replay| replay.get("status"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown"),
        "event_count": summary
            .get("replay")
            .and_then(|replay| replay.get("event_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
        "artifact_count": summary
            .get("replay")
            .and_then(|replay| replay.get("artifact_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
        "digest_failures": summary
            .get("replay")
            .and_then(|replay| replay.get("digest_failures"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
        "provider_score_present": provider_score_present,
        "provider_score": provider_score,
        "provider_score_verdict": json_string(scorecard, "verdict"),
        "provider_summary_count": json_array(summary, "provider_summaries").len(),
        "closure_verdicts": closure_verdicts,
        "evidence_pack": json_string(summary, "evidence_pack"),
        "cockpit": json_string(summary, "cockpit")
    })
}

fn json_i64_from_u64(value: &serde_json::Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|number| i64::try_from(number).ok())
        .unwrap_or_default()
}

fn json_optional_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|number| i64::try_from(number).ok())
}

fn closure_verdict_keys(value: &serde_json::Value) -> Vec<String> {
    json_array(value, "closure_verdicts")
        .iter()
        .map(|closure| {
            format!(
                "{}:{}",
                json_string(closure, "role"),
                json_string(closure, "verdict")
            )
        })
        .collect::<Vec<_>>()
}

pub(crate) fn workbench_run_evidence_diff_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let left_run_id = query_value_owned(query, "left_run_id").context("left_run_id is required")?;
    let right_run_id =
        query_value_owned(query, "right_run_id").context("right_run_id is required")?;
    let left_summary =
        workbench_run_evidence_summary_json(target, &format!("run_id={}", left_run_id))?;
    let right_summary =
        workbench_run_evidence_summary_json(target, &format!("run_id={}", right_run_id))?;
    let left = workbench_run_evidence_diff_member(&left_summary);
    let right = workbench_run_evidence_diff_member(&right_summary);
    let score_delta = match (
        json_optional_i64(&left, "provider_score"),
        json_optional_i64(&right, "provider_score"),
    ) {
        (Some(left_score), Some(right_score)) => serde_json::json!(right_score - left_score),
        _ => serde_json::Value::Null,
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-run-evidence-diff.v1",
        "left": left,
        "right": right,
        "comparison": {
            "status_changed": json_string(&left_summary, "status") != json_string(&right_summary, "status"),
            "verdict_changed": json_string(&left_summary, "verdict") != json_string(&right_summary, "verdict"),
            "digest_failure_delta": json_i64_from_u64(&right, "digest_failures") - json_i64_from_u64(&left, "digest_failures"),
            "provider_summary_delta": json_i64_from_u64(&right, "provider_summary_count") - json_i64_from_u64(&left, "provider_summary_count"),
            "score_delta": score_delta,
            "closure_verdicts_changed": closure_verdict_keys(&left) != closure_verdict_keys(&right)
        }
    }))
}

fn workbench_previous_run_id(target: &Path, run_id: &str) -> Result<String> {
    let runs = runs_list_json(target)?;
    let runs = json_array(&runs, "runs");
    let Some(index) = runs
        .iter()
        .position(|run| json_string(run, "run_id") == run_id)
    else {
        return Err(anyhow!("run {run_id} not found"));
    };
    let Some(previous) = runs.get(index + 1) else {
        return Err(anyhow!("no previous run found for {run_id}"));
    };
    Ok(json_string(previous, "run_id"))
}

pub(crate) fn workbench_run_evidence_changes_json(
    target: &Path,
    query: &str,
) -> Result<serde_json::Value> {
    let run_id = query_value_owned(query, "run_id").context("run_id is required")?;
    let previous_run_id = workbench_previous_run_id(target, &run_id)?;
    let diff = workbench_run_evidence_diff_json(
        target,
        &format!("left_run_id={previous_run_id}&right_run_id={run_id}"),
    )?;
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-run-evidence-changes.v1",
        "selected": {
            "run_id": run_id
        },
        "baseline": {
            "run_id": previous_run_id
        },
        "diff": diff
    }))
}
