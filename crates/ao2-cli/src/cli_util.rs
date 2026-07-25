use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use ao2_core::sha256_hex;
use flate2::{Compression, GzBuilder};
use serde::de::DeserializeOwned;
use tar::Builder;

pub(crate) fn atomic_write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    // Route through the shared ao2-core durable writer: temp file + write_all +
    // sync_all + atomic rename, with the temp cleaned up on any error. This is
    // the same write discipline the AO2 evidence boundary depends on, so a crash
    // or power loss can never truncate the destination to a zero-length file or
    // strew half-written temporaries beside it.
    ao2_core::atomic_write(path, content)
        .with_context(|| format!("atomic write {}", path.display()))?;
    Ok(())
}

pub(crate) fn now_unix_ms() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    unix_ms_from_duration(duration)
}

pub(crate) fn unix_ms_from_duration(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

pub(crate) fn sanitize_greenfield_id(candidate: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in candidate.trim().chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            Some('-')
        } else {
            None
        };
        if let Some(ch) = normalized {
            if ch == '-' {
                if !last_dash && !out.is_empty() {
                    out.push(ch);
                    last_dash = true;
                }
            } else {
                out.push(ch);
                last_dash = false;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "greenfield-run".to_string()
    } else {
        out
    }
}

pub(crate) fn binary_name_for_target(target: &str) -> &'static str {
    if target.contains("windows") {
        "ao2.exe"
    } else {
        "ao2"
    }
}

pub(crate) fn run_dir(target: &Path, run_id: &str) -> PathBuf {
    target.join(".ao2").join("runs").join(run_id)
}

pub(crate) fn open_report_target(path: &Path) -> Result<()> {
    if std::env::var_os("AO2_TEST_NO_OPEN").is_some() {
        return Ok(());
    }
    let status = if cfg!(windows) {
        ProcessCommand::new("cmd")
            .arg("/C")
            .arg("start")
            .arg(OsString::from(""))
            .arg(path)
            .status()
    } else if cfg!(target_os = "macos") {
        ProcessCommand::new("open").arg(path).status()
    } else {
        ProcessCommand::new("xdg-open").arg(path).status()
    }
    .with_context(|| format!("open report {}", path.display()))?;
    if !status.success() {
        anyhow::bail!("open report failed: {}", path.display());
    }
    Ok(())
}

pub(crate) fn create_tar_gz(stage_dir: &Path, archive_path: &Path) -> Result<()> {
    let archive = fs::File::create(archive_path)
        .with_context(|| format!("create {}", archive_path.display()))?;
    let encoder = GzBuilder::new()
        .mtime(0)
        .write(archive, Compression::default());
    let mut tar = Builder::new(encoder);
    for relative_path in sorted_regular_files(stage_dir)? {
        let source = stage_dir.join(&relative_path);
        let mut file = fs::File::open(&source)
            .with_context(|| format!("open archive input {}", source.display()))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("stat archive input {}", source.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(metadata.len());
        header.set_mode(deterministic_archive_mode(&source, &metadata));
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_username("")?;
        header.set_groupname("")?;
        header.set_cksum();
        tar.append_data(&mut header, &relative_path, &mut file)
            .with_context(|| format!("archive {}", source.display()))?;
    }
    let encoder = tar.into_inner()?;
    encoder.finish()?;
    Ok(())
}

fn sorted_regular_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn collect(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries = fs::read_dir(current)
            .with_context(|| format!("read archive directory {}", current.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                collect(root, &path, files)?;
            } else if file_type.is_file() {
                files.push(path.strip_prefix(root)?.to_path_buf());
            } else {
                anyhow::bail!("unsupported archive entry: {}", path.display());
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn deterministic_archive_mode(path: &Path, _metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if _metadata.permissions().mode() & 0o111 != 0 {
            return 0o755;
        }
    }
    if matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("sh")
    ) {
        0o755
    } else {
        0o644
    }
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("open {}", path.display()))?;
    Ok(sha256_hex(bytes))
}

pub(crate) fn sha256_bytes_hex(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

pub(crate) fn canonical_json_string(value: &serde_json::Value) -> Result<String> {
    let mut canonical = String::new();
    write_canonical_json_value(&mut canonical, value);
    Ok(canonical)
}

pub(crate) fn canonical_json_sha256(value: &serde_json::Value) -> String {
    let mut canonical = String::new();
    write_canonical_json_value(&mut canonical, value);
    sha256_bytes_hex(canonical.as_bytes())
}

pub(crate) fn json_array<'a>(value: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(crate) fn json_string(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

pub(crate) fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let json = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(json).with_context(|| format!("parse {}", path.display()))
}

pub(crate) fn json_bool(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_default()
}

pub(crate) fn json_u64(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

pub(crate) fn json_f64(value: &serde_json::Value, key: &str) -> f64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default()
}

pub(crate) fn string_array_text(values: &[serde_json::Value]) -> String {
    values
        .iter()
        .map(json_value_text)
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn concerns_text(values: &[serde_json::Value]) -> String {
    values
        .iter()
        .map(|value| {
            if value.is_object() {
                let severity = json_string(value, "severity");
                let message = json_string(value, "message");
                if severity.is_empty() {
                    message
                } else {
                    format!("{severity}: {message}")
                }
            } else {
                json_value_text(value)
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn usage_text(value: Option<&serde_json::Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let mut parts = Vec::new();
    for key in ["input_tokens", "output_tokens", "total_tokens", "cost_usd"] {
        if let Some(metric) = value.get(key) {
            if !metric.is_null() {
                parts.push(format!("{key}: {}", json_value_text(metric)));
            }
        }
    }
    parts.join(", ")
}

pub(crate) fn pills(values: &[serde_json::Value]) -> String {
    values
        .iter()
        .map(json_value_text)
        .map(|text| format!("<span class=\"pill\">{}</span>", escape_html(&text)))
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn pills_from_strings(values: &[String]) -> String {
    values
        .iter()
        .map(|text| format!("<span class=\"pill\">{}</span>", escape_html(text)))
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn json_value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub(crate) fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

// AO2 canonical JSON v1 (`ao2-canonical-v1`): sorted object keys, no
// whitespace, serde_json number formatting, and JSON-minimal string escaping.
// It is pinned by shared golden vectors with ao2-control-plane; do not change
// toward RFC 8785/JCS without an explicit content-address migration.
fn write_canonical_json_value(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        serde_json::Value::Number(number) => out.push_str(&number.to_string()),
        serde_json::Value::String(text) => write_canonical_json_string(out, text),
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_json_value(out, item);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_json_string(out, key);
                out.push(':');
                write_canonical_json_value(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn write_canonical_json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => {
                write!(out, "\\u{:04x}", ch as u32).expect("write to string");
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("write to string");
    }
    output
}

/// Standard base64 (RFC 4648 section 4, with padding), the encoding the
/// control plane decodes for `export_b64` / `evidence_pack_b64`.
pub(crate) fn base64_standard(bytes: &[u8]) -> String {
    use base64::prelude::{Engine as _, BASE64_STANDARD};
    BASE64_STANDARD.encode(bytes)
}

#[cfg(test)]
mod canonical_json_contract_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct CanonicalVectorSet {
        algorithm: String,
        vectors: Vec<CanonicalVector>,
    }

    #[derive(Debug, Deserialize)]
    struct CanonicalVector {
        name: String,
        input: serde_json::Value,
        canonical: String,
        sha256: String,
    }

    #[test]
    fn ao2_canonical_v1_matches_shared_golden_vectors() {
        let vectors: CanonicalVectorSet = serde_json::from_str(include_str!(
            "../../../tests/fixtures/canonical-json-vectors.json"
        ))
        .expect("canonical vector fixture parses");
        assert_eq!(vectors.algorithm, "ao2-canonical-v1");
        assert!(!vectors.vectors.is_empty());

        for vector in vectors.vectors {
            let canonical = canonical_json_string(&vector.input).unwrap();
            assert_eq!(canonical, vector.canonical, "{}", vector.name);
            assert_eq!(
                sha256_bytes_hex(canonical.as_bytes()),
                vector.sha256,
                "{}",
                vector.name
            );
            assert_eq!(
                canonical_json_sha256(&vector.input),
                vector.sha256,
                "{}",
                vector.name
            );
        }
    }
}
