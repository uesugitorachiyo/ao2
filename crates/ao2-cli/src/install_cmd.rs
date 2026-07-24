use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::{binary_name_for_target, sha256_file};
use crate::install_paths::{
    default_install_dir, install_verification_evidence_path, make_executable,
    rollback_path_for_binary,
};
use crate::release_archive_contract::{
    ensure_safe_release_archive_path, verify_release_archive_offline_contract,
};
use crate::release_assets::download_release_assets;
use crate::release_crypto::{extract_tar_gz, verify_release_archive_signature};
use crate::{atomic_write_text, runtime_target_label};

pub(crate) struct InstallUpdateOptions {
    pub(crate) archive: Option<PathBuf>,
    pub(crate) release_base_url: Option<String>,
    pub(crate) version: String,
    pub(crate) target_label: Option<String>,
    pub(crate) provenance_dir: PathBuf,
    pub(crate) install_dir: Option<PathBuf>,
}

pub(crate) fn install_update(options: InstallUpdateOptions) -> Result<()> {
    let result = install_update_result(options)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

pub(crate) fn install_update_result(options: InstallUpdateOptions) -> Result<serde_json::Value> {
    let target = options.target_label.unwrap_or_else(runtime_target_label);
    let binary_name = binary_name_for_target(&target);
    let archive = match options.archive {
        Some(archive) => archive,
        None => {
            let base_url = options
                .release_base_url
                .context("--archive or --release-base-url is required")?;
            download_release_assets(
                &base_url,
                &options.version,
                &target,
                &options.provenance_dir,
            )?
        }
    };
    verify_release_archive_signature(&archive, &options.provenance_dir)?;

    let work_dir = std::env::temp_dir().join(format!(
        "ao2-install-update-{}-{}",
        std::process::id(),
        chrono_like_timestamp()
    ));
    let extract_dir = work_dir.join("extract");
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("create {}", extract_dir.display()))?;
    extract_tar_gz(&archive, &extract_dir)?;

    let manifest_path = extract_dir.join("RELEASE-MANIFEST.json");
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    if manifest["schema_version"] != "ao2.release-manifest.v1" {
        anyhow::bail!("unexpected release manifest schema");
    }
    if manifest["binary"] != binary_name {
        anyhow::bail!("archive binary does not match target {target}");
    }
    let binary_path = manifest["binary_path"]
        .as_str()
        .context("release manifest missing binary_path")?;
    ensure_safe_release_archive_path(binary_path, "release manifest binary_path")?;
    let source_binary = extract_dir.join(binary_path);
    let expected_binary_sha = manifest["binary_sha256"]
        .as_str()
        .context("release manifest missing binary_sha256")?;
    let actual_binary_sha = sha256_file(&source_binary)?;
    if actual_binary_sha != expected_binary_sha {
        anyhow::bail!("packaged binary checksum mismatch");
    }
    let offline_verification =
        verify_release_archive_offline_contract(&extract_dir, &manifest, &target, binary_name)?;

    let install_dir = options.install_dir.unwrap_or_else(default_install_dir);
    fs::create_dir_all(&install_dir)
        .with_context(|| format!("create {}", install_dir.display()))?;
    let installed_binary = install_dir.join(binary_name);
    let rollback_binary = rollback_path_for_binary(&installed_binary);
    let rollback_created = if installed_binary.exists() {
        fs::copy(&installed_binary, &rollback_binary).with_context(|| {
            format!(
                "copy rollback {} to {}",
                installed_binary.display(),
                rollback_binary.display()
            )
        })?;
        true
    } else {
        false
    };
    fs::copy(&source_binary, &installed_binary).with_context(|| {
        format!(
            "copy {} to {}",
            source_binary.display(),
            installed_binary.display()
        )
    })?;
    make_executable(&installed_binary)?;

    let evidence_path = install_verification_evidence_path(&installed_binary);
    let evidence = serde_json::json!({
        "schema_version": "ao2.install-verification-evidence.v1",
        "status": "verified",
        "install_status": "installed",
        "version": manifest["version"],
        "target": target,
        "installed_binary": installed_binary,
        "rollback_binary": rollback_created.then_some(rollback_binary),
        "signature_verified": true,
        "offline_verification": offline_verification,
        "archive": archive
    });
    let mut evidence_text = serde_json::to_string_pretty(&evidence)?;
    evidence_text.push('\n');
    atomic_write_text(&evidence_path, &evidence_text)?;

    let _ = fs::remove_dir_all(&work_dir);
    let mut result = evidence;
    result["status"] = serde_json::json!("installed");
    result["install_verification_evidence"] = serde_json::json!(evidence_path);
    Ok(result)
}

pub(crate) fn rollback_install(
    install_dir: Option<PathBuf>,
    target_label: Option<String>,
) -> Result<()> {
    let target = target_label.unwrap_or_else(runtime_target_label);
    let binary_name = binary_name_for_target(&target);
    let install_dir = install_dir.unwrap_or_else(default_install_dir);
    let installed_binary = install_dir.join(binary_name);
    let rollback_binary = rollback_path_for_binary(&installed_binary);
    if !rollback_binary.is_file() {
        anyhow::bail!("rollback binary not found: {}", rollback_binary.display());
    }
    block_windows_active_executable_rollback(
        &installed_binary,
        &rollback_binary,
        &install_dir,
        &target,
    )?;
    fs::copy(&rollback_binary, &installed_binary).with_context(|| {
        format!(
            "copy rollback {} to {}",
            rollback_binary.display(),
            installed_binary.display()
        )
    })?;
    make_executable(&installed_binary)?;
    let result = serde_json::json!({
        "status": "rolled_back",
        "target": target,
        "installed_binary": installed_binary,
        "rollback_binary": rollback_binary,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

#[cfg(windows)]
fn block_windows_active_executable_rollback(
    installed_binary: &Path,
    rollback_binary: &Path,
    install_dir: &Path,
    target: &str,
) -> Result<()> {
    let current_exe = std::env::current_exe().context("resolve current ao2 executable")?;
    let current_exe = canonicalize_for_comparison(&current_exe);
    let installed_binary = canonicalize_for_comparison(installed_binary);
    if current_exe != installed_binary {
        return Ok(());
    }

    eprintln!("Windows-safe rollback runner required");
    eprintln!("rollback_status=blocked_active_executable");
    eprintln!("installed_binary={}", installed_binary.display());
    eprintln!("rollback_binary={}", rollback_binary.display());
    eprintln!(
        "safe_command=Use an extracted or alternate ao2.exe runner: <extracted-or-alternate>\\bin\\ao2.exe install rollback --install-dir \"{}\" --target-label {}",
        install_dir.display(),
        target
    );
    eprintln!(
        "recovery=Windows cannot replace the running ao2.exe. Use an extracted or alternate ao2.exe runner from the verified archive."
    );
    anyhow::bail!(
        "Windows cannot replace the running ao2.exe; rollback_status=blocked_active_executable"
    );
}

#[cfg(not(windows))]
fn block_windows_active_executable_rollback(
    _installed_binary: &Path,
    _rollback_binary: &Path,
    _install_dir: &Path,
    _target: &str,
) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn canonicalize_for_comparison(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn chrono_like_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
