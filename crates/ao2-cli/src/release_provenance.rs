use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::RsaPrivateKey;

use crate::cli_util::{json_string, sha256_file};
use crate::release_crypto::{
    derive_public_key_from_private_key, read_rsa_private_key, sign_file_with_private_key,
    verify_file_signature, verify_release_archive_signature,
};
use crate::{atomic_write_text, runtime_git_commit};

#[allow(clippy::too_many_arguments)]
pub(crate) fn release_sign_provenance(
    version: String,
    macos_archive: Option<PathBuf>,
    linux_archive: PathBuf,
    linux_x86_64_archive: PathBuf,
    windows_archive: PathBuf,
    provenance_dir: PathBuf,
    private_key: PathBuf,
    release_tag: Option<String>,
    json: bool,
) -> Result<()> {
    let mut archives = Vec::new();
    if let Some(macos_archive) = macos_archive {
        archives.push(macos_archive);
    }
    archives.push(linux_archive);
    archives.push(linux_x86_64_archive);
    archives.push(windows_archive);
    for archive in &archives {
        if !archive.is_file() {
            anyhow::bail!("missing release archive: {}", archive.display());
        }
    }
    fs::create_dir_all(&provenance_dir)
        .with_context(|| format!("create {}", provenance_dir.display()))?;
    ensure_rsa_private_key(&private_key, 3072)?;
    let public_key = provenance_dir.join("ao2-release-signing-public.pem");
    derive_public_key_from_private_key(&private_key, &public_key)?;

    let mut archive_entries = Vec::new();
    for archive in &archives {
        let name = archive
            .file_name()
            .and_then(|name| name.to_str())
            .context("archive filename is utf8")?
            .to_string();
        let digest = sha256_file(archive)?;
        atomic_write_text(
            &provenance_dir.join(format!("{name}.sha256")),
            &format!("{digest}  {name}\n"),
        )?;
        sign_file_with_private_key(
            &private_key,
            archive,
            &provenance_dir.join(format!("{name}.sig")),
        )?;
        let signature = format!("{name}.sig");
        let checksum = format!("{name}.sha256");
        archive_entries.push(serde_json::json!({
            "name": name,
            "path": archive,
            "sha256": digest,
            "signature": signature,
            "checksum": checksum
        }));
    }

    let tag = release_tag.unwrap_or_else(|| format!("v{version}"));
    let provenance = serde_json::json!({
        "schema_version": "ao2.release-provenance.v1",
        "package": "ao2",
        "version": version,
        "git_commit": runtime_git_commit(),
        "release_tag": tag,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "signature_algorithm": "RSA-3072/SHA-256",
        "archives": archive_entries
    });
    let provenance_json = provenance_dir.join("ao2-release-provenance.json");
    atomic_write_text(
        &provenance_json,
        &serde_json::to_string_pretty(&provenance)?,
    )?;
    let provenance_signature = provenance_dir.join("ao2-release-provenance.json.sig");
    sign_file_with_private_key(&private_key, &provenance_json, &provenance_signature)?;

    let report = serde_json::json!({
        "schema": "ao2.release-provenance-sign.v1",
        "release_provenance_dir": provenance_dir,
        "release_public_key": public_key,
        "release_provenance": provenance_json,
        "release_provenance_signature": provenance_signature,
        "archive_count": archives.len(),
        "status": "passed"
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "release_provenance_dir={}",
            json_string(&report, "release_provenance_dir")
        );
        println!(
            "release_public_key={}",
            json_string(&report, "release_public_key")
        );
        println!(
            "release_provenance={}",
            json_string(&report, "release_provenance")
        );
        println!("release_provenance_sign=passed");
    }
    Ok(())
}

pub(crate) fn release_verify_provenance(
    macos_archive: Option<PathBuf>,
    linux_archive: PathBuf,
    linux_x86_64_archive: PathBuf,
    windows_archive: PathBuf,
    provenance_dir: PathBuf,
    public_key: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let public_key =
        public_key.unwrap_or_else(|| provenance_dir.join("ao2-release-signing-public.pem"));
    if !public_key.is_file() {
        anyhow::bail!("missing release public key: {}", public_key.display());
    }
    let mut archives = Vec::new();
    if let Some(macos_archive) = macos_archive {
        archives.push(macos_archive);
    }
    archives.push(linux_archive);
    archives.push(linux_x86_64_archive);
    archives.push(windows_archive);
    let mut results = Vec::new();
    for archive in &archives {
        verify_release_archive_signature(archive, &provenance_dir)?;
        results.push(serde_json::json!({
            "archive": archive,
            "verified": true
        }));
    }
    let provenance = provenance_dir.join("ao2-release-provenance.json");
    let provenance_signature = provenance_dir.join("ao2-release-provenance.json.sig");
    let body = fs::read_to_string(&provenance)
        .with_context(|| format!("read {}", provenance.display()))?;
    let provenance_json: serde_json::Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", provenance.display()))?;
    if json_string(&provenance_json, "schema_version") != "ao2.release-provenance.v1" {
        anyhow::bail!("invalid release provenance schema");
    }
    if !verify_release_provenance_signature(&provenance, &provenance_signature, &public_key) {
        anyhow::bail!("release provenance signature verification failed");
    }
    let report = serde_json::json!({
        "schema": "ao2.release-provenance-verify.v1",
        "release_provenance_dir": provenance_dir,
        "public_key": public_key,
        "provenance": provenance,
        "provenance_verified": true,
        "archive_count": archives.len(),
        "archives": results,
        "status": "passed"
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "release_provenance_dir={}",
            json_string(&report, "release_provenance_dir")
        );
        println!("release_provenance_verify=passed");
    }
    Ok(())
}

pub(crate) fn ensure_rsa_private_key(private_key: &Path, bits: usize) -> Result<()> {
    if private_key.is_file() {
        read_rsa_private_key(private_key)?;
        return Ok(());
    }
    if let Some(parent) = private_key.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let pem = generate_rsa_private_key_pem(bits)?;
    atomic_write_text(private_key, pem.as_str())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(private_key)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(private_key, permissions)?;
    }
    Ok(())
}

fn generate_rsa_private_key_pem(bits: usize) -> Result<String> {
    std::thread::Builder::new()
        .name("ao2-rsa-keygen".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let mut rng = rand::rngs::OsRng;
            let key =
                RsaPrivateKey::new(&mut rng, bits).context("generate RSA release signing key")?;
            key.to_pkcs8_pem(LineEnding::LF)
                .map(|pem| pem.to_string())
                .context("encode RSA private key pem")
        })
        .context("spawn RSA release signing key generator")?
        .join()
        .map_err(|_| anyhow::anyhow!("RSA release signing key generator panicked"))?
}

pub(crate) fn verify_release_provenance_signature(
    provenance_json: &Path,
    provenance_signature: &Path,
    public_key: &Path,
) -> bool {
    if !provenance_json.is_file() || !provenance_signature.is_file() || !public_key.is_file() {
        return false;
    }
    verify_file_signature(provenance_json, provenance_signature, public_key).unwrap_or(false)
}
