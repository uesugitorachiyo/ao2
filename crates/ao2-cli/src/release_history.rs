use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::json_string;

pub(crate) fn release_retention_plan_dirs<P, F>(
    root: &Path,
    keep: usize,
    include_name: P,
    sort_key: F,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>)>
where
    P: Fn(&str) -> bool,
    F: Fn(&Path) -> Vec<u64>,
{
    if !root.is_dir() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut dirs = fs::read_dir(root)
        .with_context(|| format!("read {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(&include_name)
        })
        .collect::<Vec<_>>();
    dirs.sort_by(|left, right| {
        sort_key(right)
            .cmp(&sort_key(left))
            .then_with(|| right.file_name().cmp(&left.file_name()))
    });
    let kept = dirs.iter().take(keep).cloned().collect::<Vec<_>>();
    let removed = dirs.into_iter().skip(keep).collect::<Vec<_>>();
    Ok((kept, removed))
}

pub(crate) fn release_dir_sort_key(path: &Path) -> Vec<u64> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(release_tag_sort_key)
        .unwrap_or_default()
}

pub(crate) fn release_comparison_dir_sort_key(path: &Path) -> Vec<u64> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            name.trim_start_matches("release-comparison-")
                .split(|char: char| !char.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn workbench_release_history_for_dir(
    release_download_dir: PathBuf,
) -> Result<serde_json::Value> {
    let mut entries = Vec::new();
    if release_download_dir.is_dir() {
        for entry in fs::read_dir(&release_download_dir)
            .with_context(|| format!("read {}", release_download_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with('v') {
                continue;
            }
            entries.push(release_history_entry_json(name, &path));
        }
    }
    entries.sort_by(|left, right| {
        let left_tag = json_string(left, "release_tag");
        let right_tag = json_string(right, "release_tag");
        release_tag_sort_key(&right_tag)
            .cmp(&release_tag_sort_key(&left_tag))
            .then_with(|| right_tag.cmp(&left_tag))
    });
    let trend = annotate_release_history_trends(&mut entries);
    Ok(serde_json::json!({
        "schema_version": "ao2.release-history.v1",
        "release_download_dir": release_download_dir,
        "trend": trend,
        "entries": entries,
    }))
}

fn annotate_release_history_trends(entries: &mut [serde_json::Value]) -> serde_json::Value {
    let scores = entries
        .iter()
        .map(release_history_health_score)
        .collect::<Vec<_>>();
    let max_score = 6_u64;
    let mut regression_count = 0_u64;
    let mut attention_count = 0_u64;
    for index in 0..entries.len() {
        let score = scores[index];
        if score < max_score {
            attention_count += 1;
        }
        let previous = entries.get(index + 1);
        let previous_score = scores.get(index + 1).copied();
        let trend_status = match previous_score {
            None => "baseline",
            Some(previous_score) if score < previous_score => {
                regression_count += 1;
                "regressed"
            }
            Some(previous_score) if score > previous_score => "improved",
            Some(_) => "unchanged",
        };
        let previous_release_tag = previous
            .map(|entry| json_string(entry, "release_tag"))
            .filter(|tag| !tag.is_empty());
        let changed_fields = previous
            .map(|entry| release_history_changed_fields(&entries[index], entry))
            .unwrap_or_default();
        if let Some(object) = entries[index].as_object_mut() {
            object.insert("health_score".to_string(), serde_json::json!(score));
            object.insert("max_health_score".to_string(), serde_json::json!(max_score));
            object.insert(
                "previous_health_score".to_string(),
                previous_score
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
            );
            object.insert(
                "previous_release_tag".to_string(),
                previous_release_tag
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
            );
            object.insert("trend_status".to_string(), serde_json::json!(trend_status));
            object.insert(
                "changed_fields".to_string(),
                serde_json::json!(changed_fields),
            );
        }
    }
    serde_json::json!({
        "entry_count": entries.len(),
        "latest_release_tag": entries
            .first()
            .map(|entry| json_string(entry, "release_tag"))
            .unwrap_or_default(),
        "latest_health_score": scores.first().copied().unwrap_or_default(),
        "max_health_score": max_score,
        "attention_count": attention_count,
        "regression_count": regression_count,
    })
}

fn release_history_health_score(entry: &serde_json::Value) -> u64 {
    let platforms = &entry["platforms"];
    [
        json_string(entry, "status") == "ok",
        entry
            .get("assets_available")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        entry
            .get("provenance_verified")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        entry
            .get("provenance_tag_matches")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        json_string(entry, "rollback_status") == "verified",
        platforms["macos-aarch64"].as_str() == Some("passed")
            && platforms["linux-x86_64"].as_str() == Some("passed")
            && platforms["windows-x86_64"].as_str() == Some("passed"),
    ]
    .into_iter()
    .filter(|passed| *passed)
    .count() as u64
}

fn release_history_changed_fields(
    current: &serde_json::Value,
    previous: &serde_json::Value,
) -> Vec<String> {
    let mut changed = Vec::new();
    for field in [
        "status",
        "assets_available",
        "asset_count",
        "provenance_verified",
        "provenance_tag_matches",
        "rollback_status",
        "platforms",
    ] {
        if current.get(field) != previous.get(field) {
            changed.push(field.to_string());
        }
    }
    changed
}

fn release_history_entry_json(release_tag: &str, path: &Path) -> serde_json::Value {
    let doctor_path = path.join("release-doctor.json");
    let rollback_path = path.join("release-rollback-summary.json");
    let doctor = fs::read_to_string(&doctor_path)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok());
    let rollback = fs::read_to_string(&rollback_path)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok());

    let null = serde_json::Value::Null;
    let release = doctor
        .as_ref()
        .map(|json| json.get("release").unwrap_or(json))
        .unwrap_or(&null);
    let rollback_ref = rollback.as_ref().unwrap_or(&null);
    let release_tag_value = {
        let from_doctor = json_string(release, "release_tag");
        if from_doctor.is_empty() {
            release_tag.to_string()
        } else {
            from_doctor
        }
    };
    let rollback_status = {
        let status = json_string(rollback_ref, "status");
        if status.is_empty() {
            "missing".to_string()
        } else {
            status
        }
    };
    serde_json::json!({
        "release_tag": release_tag_value,
        "path": path,
        "doctor_json": doctor_path,
        "rollback_summary_json": rollback_path,
        "status": doctor
            .as_ref()
            .map(|json| json_string(json, "status"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "missing".to_string()),
        "assets_available": release
            .get("assets_available")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "asset_count": release
            .get("asset_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        "provenance_verified": release
            .get("provenance_verified")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "provenance_tag_matches": release
            .get("provenance_tag_matches")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "rollback_status": rollback_status,
        "platforms": {
            "macos-aarch64": json_string(&rollback_ref["platforms"]["macos-aarch64"], "status"),
            "linux-x86_64": json_string(&rollback_ref["platforms"]["linux-x86_64"], "status"),
            "windows-x86_64": json_string(&rollback_ref["platforms"]["windows-x86_64"], "status"),
        }
    })
}

pub(crate) fn release_tag_sort_key(tag: &str) -> Vec<u64> {
    tag.trim_start_matches('v')
        .split(|char: char| !char.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}
