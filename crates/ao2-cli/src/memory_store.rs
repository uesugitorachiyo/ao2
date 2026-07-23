use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};

use crate::cli_util::sha256_file;
use crate::trimmed_required;
use ao2_core::sha256_hex;

pub(crate) fn memory_write_record_json(
    target: &Path,
    kind: String,
    title: String,
    body: String,
    tags: Vec<String>,
    source_run_id: Option<String>,
    source_path: Option<String>,
) -> Result<serde_json::Value> {
    let kind = trimmed_required("--kind", &kind)?;
    let title = trimmed_required("--title", &title)?;
    let body = trimmed_required("--body", &body)?;
    let generated_at_ms = now_unix_ms();
    let normalized_tags = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    let source = memory_source_json(target, source_run_id, source_path)?;
    let digest_input = serde_json::json!({
        "generated_at_ms": generated_at_ms,
        "kind": kind,
        "title": title,
        "body": body,
        "tags": normalized_tags,
        "source": source
    });
    let digest = sha256_hex(serde_json::to_string(&digest_input)?.as_bytes());
    Ok(serde_json::json!({
        "schema_version": "ao2.memory-record.v1",
        "id": format!("mem-{generated_at_ms}-{}", &digest[..12]),
        "created_at_ms": generated_at_ms,
        "kind": kind,
        "title": title,
        "body": body,
        "tags": normalized_tags,
        "source": source
    }))
}

fn memory_source_json(
    target: &Path,
    source_run_id: Option<String>,
    source_path: Option<String>,
) -> Result<serde_json::Value> {
    let source_path_sha256 = match source_path.as_deref() {
        Some(path) if !path.trim().is_empty() => {
            let candidate = target.join(path);
            if candidate.is_file() {
                Some(sha256_file(&candidate)?)
            } else {
                None
            }
        }
        _ => None,
    };
    Ok(serde_json::json!({
        "run_id": source_run_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        "path": source_path
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        "path_sha256": source_path_sha256
    }))
}

pub(crate) fn memory_search_json(
    target: &Path,
    query: &str,
    limit: usize,
) -> Result<serde_json::Value> {
    if limit == 0 {
        return Err(anyhow!("--limit must be greater than 0"));
    }
    let query = trimmed_required("--query", query)?;
    let query_lc = query.to_lowercase();
    let mut matches = Vec::new();
    for record in read_jsonl_values(&memory_records_path(target))? {
        if memory_record_matches(&record, &query_lc) {
            matches.push(record);
            if matches.len() >= limit {
                break;
            }
        }
    }
    Ok(serde_json::json!({
        "schema_version": "ao2.memory-search.v1",
        "query": query,
        "limit": limit,
        "matches": matches
    }))
}

pub(crate) fn memory_recent_json(target: &Path, limit: usize) -> Result<serde_json::Value> {
    if limit == 0 {
        return Err(anyhow!("--limit must be greater than 0"));
    }
    let mut records = read_jsonl_values(&memory_records_path(target))?;
    records.reverse();
    records.truncate(limit);
    Ok(serde_json::json!({
        "schema_version": "ao2.memory-recent.v1",
        "limit": limit,
        "records": records
    }))
}

pub(crate) fn memory_link_run_json(
    target: &Path,
    memory_id: String,
    run_id: String,
    relationship: String,
) -> Result<serde_json::Value> {
    let memory_id = trimmed_required("--memory-id", &memory_id)?;
    let run_id = trimmed_required("--run-id", &run_id)?;
    let relationship = trimmed_required("--relationship", &relationship)?;
    let records = read_jsonl_values(&memory_records_path(target))?;
    if !records.iter().any(|record| record["id"] == memory_id) {
        return Err(anyhow!("unknown memory id: {memory_id}"));
    }
    Ok(serde_json::json!({
        "schema_version": "ao2.memory-run-link.v1",
        "created_at_ms": now_unix_ms(),
        "memory_id": memory_id,
        "run_id": run_id,
        "relationship": relationship
    }))
}

pub(crate) fn memory_records_path(target: &Path) -> PathBuf {
    target.join(".ao2").join("memory").join("records.jsonl")
}

pub(crate) fn memory_run_links_path(target: &Path) -> PathBuf {
    target.join(".ao2").join("memory").join("run-links.jsonl")
}

pub(crate) fn read_jsonl_values(path: &Path) -> Result<Vec<serde_json::Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut values = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = serde_json::from_str(line)
            .with_context(|| format!("parse {} line {}", path.display(), index + 1))?;
        values.push(value);
    }
    Ok(values)
}

pub(crate) fn append_jsonl(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(value)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn memory_record_matches(record: &serde_json::Value, query_lc: &str) -> bool {
    let haystack = serde_json::to_string(record)
        .unwrap_or_default()
        .to_lowercase();
    haystack.contains(query_lc)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
