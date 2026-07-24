use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use clap::Subcommand;

use crate::cli_util::{json_array, json_string, json_u64, sha256_file};
use crate::{memory_export_json, memory_publish_to_control_plane_json, trimmed_required};
use ao2_core::sha256_hex;

#[derive(Debug, Subcommand)]
pub(crate) enum MemoryCommand {
    Write {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "source-run-id")]
        source_run_id: Option<String>,
        #[arg(long = "source-path")]
        source_path: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Search {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Export {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-memory")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    Publish {
        #[arg(long = "export")]
        export_path: PathBuf,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: String,
        /// Escape valve for legacy operators that have not yet provisioned a
        /// memory-export signing key. As of slice 19, `ao2 memory publish`
        /// defaults to fail-closed when the export does not have sibling
        /// `.json.sig` + `memory-export-signing-public.pem` files, so the
        /// upload path always reaches the signed control-plane ingest
        /// endpoint. Pass this flag to opt out and upload the plain export
        /// via `/api/v1/memory/export` instead. Hidden because the
        /// principled path is to sign the export at `ao2 memory export` time
        /// via `--signing-key`.
        #[arg(long = "allow-unsigned-memory-export", hide = true)]
        allow_unsigned_memory_export: bool,
        #[arg(long)]
        json: bool,
    },
    LinkRun {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "memory-id")]
        memory_id: String,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long, default_value = "related")]
        relationship: String,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn memory(command: MemoryCommand) -> Result<()> {
    match command {
        MemoryCommand::Write {
            target,
            kind,
            title,
            body,
            tags,
            source_run_id,
            source_path,
            json,
        } => {
            let record = memory_write_record_json(
                &target,
                kind,
                title,
                body,
                tags,
                source_run_id,
                source_path,
            )?;
            append_jsonl(&memory_records_path(&target), &record)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&record)?);
            } else {
                println!("memory_id={}", json_string(&record, "id"));
                println!("memory_record={}", memory_records_path(&target).display());
            }
            Ok(())
        }
        MemoryCommand::Search {
            target,
            query,
            limit,
            json,
        } => {
            let result = memory_search_json(&target, &query, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("memory_matches={}", json_array(&result, "matches").len());
                for item in json_array(&result, "matches") {
                    println!(
                        "{}\t{}\t{}",
                        json_string(item, "id"),
                        json_string(item, "kind"),
                        json_string(item, "title")
                    );
                }
            }
            Ok(())
        }
        MemoryCommand::Export {
            target,
            query,
            limit,
            out,
            signing_key,
            signer_id,
            json,
        } => {
            let result = memory_export_json(
                &target,
                query.as_deref(),
                limit,
                &out,
                signing_key,
                signer_id,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("memory_export={}", json_string(&result, "export_path"));
                println!("record_count={}", json_u64(&result, "record_count"));
                println!("link_count={}", json_u64(&result, "link_count"));
            }
            Ok(())
        }
        MemoryCommand::LinkRun {
            target,
            memory_id,
            run_id,
            relationship,
            json,
        } => {
            let link = memory_link_run_json(&target, memory_id, run_id, relationship)?;
            append_jsonl(&memory_run_links_path(&target), &link)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&link)?);
            } else {
                println!("memory_id={}", json_string(&link, "memory_id"));
                println!("run_id={}", json_string(&link, "run_id"));
                println!(
                    "memory_run_link={}",
                    memory_run_links_path(&target).display()
                );
            }
            Ok(())
        }
        MemoryCommand::Publish {
            export_path,
            control_plane_url,
            api_token,
            allow_unsigned_memory_export,
            json,
        } => {
            let require_signed_export = !allow_unsigned_memory_export;
            let result = memory_publish_to_control_plane_json(
                &export_path,
                &control_plane_url,
                &api_token,
                require_signed_export,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("memory_export={}", json_string(&result, "export_path"));
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
            }
            Ok(())
        }
    }
}

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
