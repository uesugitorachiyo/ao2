use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ao2_core::sha256_hex;
use flate2::{Compression, GzBuilder};
use tar::Builder;

pub(crate) fn binary_name_for_target(target: &str) -> &'static str {
    if target.contains("windows") {
        "ao2.exe"
    } else {
        "ao2"
    }
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
