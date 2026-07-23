use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::{Signature as RsaPkcs1v15Signature, SigningKey, VerifyingKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use signature::{SignatureEncoding, Signer, Verifier};

use crate::atomic_write_text;
use crate::cli_util::sha256_file;

pub(crate) fn verify_release_archive_signature(
    archive: &Path,
    provenance_dir: &Path,
) -> Result<()> {
    let archive_name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .context("archive filename is utf8")?;
    let checksum_file = provenance_dir.join(format!("{archive_name}.sha256"));
    let signature_file = provenance_dir.join(format!("{archive_name}.sig"));
    let public_key = provenance_dir.join("ao2-release-signing-public.pem");
    if !checksum_file.is_file() {
        anyhow::bail!("missing release checksum: {}", checksum_file.display());
    }
    if !signature_file.is_file() {
        anyhow::bail!("missing release signature: {}", signature_file.display());
    }
    if !public_key.is_file() {
        anyhow::bail!("missing release public key: {}", public_key.display());
    }
    let checksum_text = fs::read_to_string(&checksum_file)
        .with_context(|| format!("read {}", checksum_file.display()))?;
    let expected = checksum_text
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let digest = parts.next()?;
            let name = parts.next()?;
            (name == archive_name).then(|| digest.to_string())
        })
        .with_context(|| format!("checksum for {archive_name} not found"))?;
    let actual = sha256_file(archive)?;
    if actual != expected {
        anyhow::bail!("archive checksum mismatch");
    }
    let verified = verify_file_signature(archive, &signature_file, &public_key)?;
    if !verified {
        anyhow::bail!("archive signature verification failed");
    }
    Ok(())
}

pub(crate) fn derive_public_key_from_private_key(
    private_key: &Path,
    public_key: &Path,
) -> Result<()> {
    if let Some(parent) = public_key.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let public_pem = public_key_pem_from_private_key(private_key)?;
    atomic_write_text(public_key, &public_pem)?;
    Ok(())
}

pub(crate) fn public_key_pem_from_private_key(private_key: &Path) -> Result<String> {
    let private_key = read_rsa_private_key(private_key)?;
    RsaPublicKey::from(&private_key)
        .to_public_key_pem(LineEnding::LF)
        .context("encode RSA public key pem")
}

pub(crate) fn sign_file_with_private_key(
    private_key: &Path,
    input: &Path,
    signature: &Path,
) -> Result<()> {
    let input_bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let signature_bytes = sign_bytes_with_private_key(private_key, &input_bytes)?;
    if let Some(parent) = signature.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(signature, signature_bytes)
        .with_context(|| format!("write {}", signature.display()))?;
    Ok(())
}

pub(crate) fn sign_bytes_with_private_key(
    private_key: &Path,
    input_bytes: &[u8],
) -> Result<Vec<u8>> {
    let private_key = read_rsa_private_key(private_key)?;
    let signing_key = SigningKey::<Sha256>::new(private_key);
    let signed: RsaPkcs1v15Signature = signing_key.sign(input_bytes);
    Ok(signed.to_bytes().to_vec())
}

pub(crate) fn verify_file_signature(
    input: &Path,
    signature: &Path,
    public_key: &Path,
) -> Result<bool> {
    if !input.is_file() || !signature.is_file() || !public_key.is_file() {
        return Ok(false);
    }
    let input_bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let signature_bytes =
        fs::read(signature).with_context(|| format!("read {}", signature.display()))?;
    let public_key = read_rsa_public_key(public_key)?;
    Ok(verify_rsa_sha256_bytes(&input_bytes, &signature_bytes, public_key).is_ok())
}

pub(crate) fn read_rsa_private_key(path: &Path) -> Result<RsaPrivateKey> {
    let pem = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    RsaPrivateKey::from_pkcs8_pem(&pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(&pem))
        .with_context(|| format!("parse RSA private key {}", path.display()))
}

fn read_rsa_public_key(path: &Path) -> Result<RsaPublicKey> {
    let pem = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    RsaPublicKey::from_public_key_pem(&pem)
        .with_context(|| format!("parse RSA public key {}", path.display()))
}

fn verify_rsa_sha256_bytes(
    input_bytes: &[u8],
    signature_bytes: &[u8],
    public_key: RsaPublicKey,
) -> Result<()> {
    let signature = RsaPkcs1v15Signature::try_from(signature_bytes)
        .context("parse RSA PKCS#1 v1.5 signature")?;
    let verifying_key = VerifyingKey::<Sha256>::new(public_key);
    verifying_key
        .verify(input_bytes, &signature)
        .context("verify RSA/SHA-256 signature")
}

pub(crate) fn extract_tar_gz(archive: &Path, extract_dir: &Path) -> Result<()> {
    let file = fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(extract_dir)
        .with_context(|| format!("extract into {}", extract_dir.display()))?;
    Ok(())
}

pub(crate) fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type for {}", source_path.display()))?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}
