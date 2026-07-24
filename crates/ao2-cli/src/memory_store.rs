use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use clap::Subcommand;

use crate::cli_util::{base64_standard, hex_lower, json_array, json_string, json_u64, sha256_file};
use crate::control_plane_http::{control_plane_endpoint, post_json_http};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};
use crate::{atomic_write_text, trimmed_required};
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

pub(crate) fn memory_export_json(
    target: &Path,
    query: Option<&str>,
    limit: usize,
    out: &Path,
    signing_key: Option<PathBuf>,
    signer_id: String,
) -> Result<serde_json::Value> {
    if limit == 0 {
        return Err(anyhow!("--limit must be greater than 0"));
    }
    let records = match query {
        Some(query) if !query.trim().is_empty() => {
            json_array(&memory_search_json(target, query, limit)?, "matches").to_vec()
        }
        _ => read_jsonl_values(&memory_records_path(target))?
            .into_iter()
            .take(limit)
            .collect(),
    };
    let record_ids = records
        .iter()
        .filter_map(|record| record.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let links = read_jsonl_values(&memory_run_links_path(target))?
        .into_iter()
        .filter(|link| {
            link.get("memory_id")
                .and_then(serde_json::Value::as_str)
                .map(|id| record_ids.contains(id))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let generated_at_ms = crate::now_unix_ms();
    let export = serde_json::json!({
        "schema_version": "ao2.memory-export.v1",
        "generated_at_ms": generated_at_ms,
        "target": target,
        "query": query.unwrap_or(""),
        "limit": limit,
        "record_count": records.len(),
        "link_count": links.len(),
        "records": records,
        "links": links
    });
    atomic_write_text(out, &serde_json::to_string_pretty(&export)?)?;
    let export_sha256 = sha256_file(out)?;
    let signing = match signing_key {
        Some(key_path) => {
            let signer_id = trimmed_required("--signer-id", &signer_id)?;
            let signature_path = out.with_extension("json.sig");
            let public_key_path = out
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("memory-export-signing-public.pem");
            derive_public_key_from_private_key(&key_path, &public_key_path)?;
            sign_file_with_private_key(&key_path, out, &signature_path)?;
            let signature_verified = verify_file_signature(out, &signature_path, &public_key_path)?;
            serde_json::json!({
                "present": true,
                "signature_verified": signature_verified,
                "signer_id": signer_id,
                "signature_algorithm": "RSA/SHA-256",
                "signature_path": signature_path,
                "public_key_path": public_key_path,
                "public_key_sha256": sha256_file(&public_key_path)?
            })
        }
        None => serde_json::json!({
            "present": false,
            "signature_verified": false
        }),
    };
    let mut result = export;
    result["export_path"] = serde_json::json!(out);
    result["sha256"] = serde_json::json!(export_sha256);
    result["signing"] = signing.clone();
    result["signature_path"] = signing
        .get("signature_path")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    result["public_key_path"] = signing
        .get("public_key_path")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    Ok(result)
}

pub(crate) fn memory_publish_to_control_plane_json(
    export_path: &Path,
    control_plane_url: &str,
    api_token: &str,
    require_signed_export: bool,
) -> Result<serde_json::Value> {
    let api_token = trimmed_required("--api-token", api_token)?;
    let content = fs::read_to_string(export_path)
        .with_context(|| format!("read {}", export_path.display()))?;
    let export: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse {}", export_path.display()))?;
    let schema_version = json_string(&export, "schema_version");
    if schema_version != "ao2.memory-export.v1" {
        return Err(anyhow!(
            "memory publish requires ao2.memory-export.v1, got {schema_version}"
        ));
    }
    let signed_metadata = memory_export_signature_metadata(export_path)?;
    if require_signed_export && signed_metadata.is_none() {
        return Err(anyhow!(
            "memory publish requires a signed export by default (slice 19 producer-side \
             default-on, mirroring slice 11/17/18 obligation-gate signing flips); export \
             at {} has no sibling `.json.sig` + `memory-export-signing-public.pem` so the \
             signed control-plane endpoint cannot be reached. Sign the export at \
             `ao2 memory export` time via `--signing-key`, or pass \
             `--allow-unsigned-memory-export` to opt out and upload the plain export via \
             `/api/v1/memory/export`",
            export_path.display()
        ));
    }
    let (endpoint, post_body, signed) = match signed_metadata {
        Some(signature) => (
            control_plane_endpoint(control_plane_url, "/api/v1/memory/export/signed")?,
            serde_json::to_string(&serde_json::json!({
                "schema_version": "ao2.cp-memory-export-signed-upload.v1",
                "export": export,
                // Exact bytes the sibling `.json.sig` signs: the verbatim export-file
                // content. Lets the control plane verify over these, not a lossy
                // re-serialization of `export`.
                "export_b64": base64_standard(content.as_bytes()),
                "signature": signature
            }))?,
            true,
        ),
        None => (
            control_plane_endpoint(control_plane_url, "/api/v1/memory/export")?,
            content,
            false,
        ),
    };
    let receipt = post_json_http(&endpoint, &api_token, &post_body)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.memory-control-plane-publish.v1",
        "export_path": export_path,
        "endpoint": endpoint,
        "signed": signed,
        "receipt": receipt
    }))
}

fn memory_export_signature_metadata(export_path: &Path) -> Result<Option<serde_json::Value>> {
    let signature_path = export_path.with_extension("json.sig");
    let public_key_path = export_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("memory-export-signing-public.pem");
    if !signature_path.is_file() || !public_key_path.is_file() {
        return Ok(None);
    }
    Ok(Some(serde_json::json!({
        "present": true,
        "signature_algorithm": "RSA/SHA-256",
        "signature_path": signature_path,
        "signature_sha256": sha256_file(&signature_path)?,
        "signature_hex": hex_lower(&fs::read(&signature_path)
            .with_context(|| format!("read {}", signature_path.display()))?),
        "public_key_path": public_key_path,
        "public_key_sha256": sha256_file(&public_key_path)?,
        "public_key_pem": fs::read_to_string(&public_key_path)
            .with_context(|| format!("read {}", public_key_path.display()))?
    })))
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
