//! Repository surface scanner — phase P1.
//!
//! `scan(root, head_sha)` walks the working tree and produces a
//! [`SurfaceMap`] that is byte-deterministic across runs and operating
//! systems. The map is the only thing planners are allowed to peek at
//! when deciding which paths a plan may touch (README §7, V2).
//!
//! ## Rules (README §10 P1 acceptance)
//!
//! 1. Files sorted by `path` in ascending UTF-8 order.
//! 2. Excludes `target/`, `.git/`, `node_modules/`, `.env*`, `*.pem`,
//!    `*.key`, and any regular file larger than [`MAX_FILE_SIZE_BYTES`].
//! 3. Allow-listed extensions only — see [`kind_for_extension`].
//! 4. Public Rust symbols (`pub fn|struct|enum|trait NAME`) are extracted
//!    into `SurfaceFile.public_symbols`, sorted + deduped.
//! 5. Two scans of the same tree produce byte-equal canonical JSON.
//!
//! Path separators are normalized to `/` so a scan on Windows matches a
//! scan on macOS/Linux for the same content.

use crate::schema::{SurfaceFile, SurfaceMap};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use walkdir::WalkDir;

/// 1 MiB — files at or below this size are eligible; strictly larger are
/// dropped (V2 surface should not carry binary blobs or vendored bundles).
pub const MAX_FILE_SIZE_BYTES: u64 = 1024 * 1024;

/// Directory basenames that are never descended into.
pub const EXCLUDED_DIRS: &[&str] = &["target", ".git", "node_modules"];

/// Resolve an extension → `SurfaceFile.kind` label. Returns `None` for
/// any extension outside the allow-list, in which case the file is
/// dropped from the surface map.
pub fn kind_for_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "toml" => Some("toml"),
        "md" => Some("markdown"),
        "json" => Some("json"),
        "yml" | "yaml" => Some("yaml"),
        "js" | "mjs" | "cjs" | "jsx" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "py" => Some("python"),
        "sh" | "bash" => Some("shell"),
        "html" => Some("html"),
        "css" => Some("css"),
        "txt" => Some("text"),
        _ => None,
    }
}

/// True if `name` matches a secrets/credentials pattern that must never
/// appear in the surface map regardless of extension.
fn is_secret_basename(name: &str) -> bool {
    name.starts_with(".env") || name.ends_with(".pem") || name.ends_with(".key")
}

/// True if any path component along `rel` is an excluded directory.
/// Used as a second-pass guard in addition to `filter_entry`, so a
/// caller passing in a pre-walked path still gets the same exclusion
/// semantics.
fn under_excluded_dir(rel: &str) -> bool {
    rel.split('/').any(|comp| EXCLUDED_DIRS.contains(&comp))
}

/// Extract public top-level symbols from Rust source. Matches simple
/// `pub fn|struct|enum|trait NAME` declarations at the start of a line
/// (after trimming whitespace) — enough for P1's context shrinker
/// (README §10 P2) without pulling in a syn dependency.
pub fn extract_public_symbols(src: &str) -> Vec<String> {
    let mut syms: Vec<String> = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim_start();
        for kw in ["fn", "struct", "enum", "trait"] {
            let prefix = format!("pub {kw} ");
            if let Some(rest) = trimmed.strip_prefix(&prefix) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    syms.push(name);
                    break;
                }
            }
        }
    }
    syms.sort();
    syms.dedup();
    syms
}

/// Walk `root`, returning a deterministic [`SurfaceMap`].
///
/// `head_sha` is recorded as-is — the scanner does not shell out to git,
/// so the caller (CLI / orchestrator) is responsible for resolving the
/// repo HEAD. Tests may pass any 40-char string.
pub fn scan(root: &Path, head_sha: String) -> io::Result<SurfaceMap> {
    let mut files: Vec<SurfaceFile> = Vec::new();
    let root_canon = root.canonicalize()?;

    let walker = WalkDir::new(&root_canon)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            // Don't skip the root itself even if its basename collides
            // with an excluded name (e.g. a repo literally named "target").
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !EXCLUDED_DIRS.iter().any(|d| name == *d)
        });

    for entry in walker {
        let entry = entry.map_err(io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let basename = entry.file_name().to_string_lossy().into_owned();
        if is_secret_basename(&basename) {
            continue;
        }
        let meta = entry.metadata().map_err(io::Error::other)?;
        if meta.len() > MAX_FILE_SIZE_BYTES {
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let kind = match kind_for_extension(ext) {
            Some(k) => k.to_string(),
            None => continue,
        };
        let rel = path
            .strip_prefix(&root_canon)
            .map_err(|e| io::Error::other(format!("strip_prefix: {e}")))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if under_excluded_dir(&rel_str) {
            continue;
        }
        let bytes = fs::read(path)?;
        let normalized_bytes = normalize_crlf(&bytes);
        let sha256 = hex::encode(Sha256::digest(&normalized_bytes));
        let public_symbols = if kind == "rust" {
            extract_public_symbols(&String::from_utf8_lossy(&normalized_bytes))
        } else {
            Vec::new()
        };
        files.push(SurfaceFile {
            kind,
            path: rel_str,
            public_symbols,
            sha256,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(SurfaceMap { files, head_sha })
}

fn normalize_crlf(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            normalized.push(b'\n');
            i += 2;
        } else {
            normalized.push(bytes[i]);
            i += 1;
        }
    }
    normalized
}

/// Recursive canonical JSON encoder per README §5.3:
///
/// * object keys sorted by UTF-8 codepoint order
/// * no whitespace
/// * no trailing newline
///
/// Arrays are not reordered — element order is part of the data.
pub fn canonical_json(value: &Value) -> String {
    serde_json::to_string(&sort_value(value)).expect("canonical serialization")
}

fn sort_value(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (k, vv) in m {
                sorted.insert(k.clone(), sort_value(vv));
            }
            let mut out = serde_json::Map::new();
            for (k, vv) in sorted {
                out.insert(k, vv);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_value).collect()),
        _ => v.clone(),
    }
}
