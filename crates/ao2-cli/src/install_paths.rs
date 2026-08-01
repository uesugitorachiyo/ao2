use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

pub(crate) fn default_install_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
            })
            .join("AO2")
            .join("bin")
    } else {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("bin")
    }
}

pub(crate) fn install_verification_evidence_path(installed_binary: &Path) -> PathBuf {
    let file_name = installed_binary
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ao2");
    installed_binary.with_file_name(format!("{file_name}.install-verification.json"))
}

pub(crate) fn command_exists(command: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return true;
        }
        cfg!(windows) && dir.join(format!("{command}.exe")).is_file()
    })
}

pub(crate) fn binary_on_path(binary_name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(binary_name))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn is_binary_on_path(binary_name: &str, installed_binary: &Path) -> bool {
    let Ok(expected) = fs::canonicalize(installed_binary) else {
        return false;
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(binary_name);
        fs::canonicalize(candidate)
            .map(|candidate| candidate == expected)
            .unwrap_or(false)
    })
}

pub(crate) fn rollback_path_for_binary(installed_binary: &Path) -> PathBuf {
    let filename = installed_binary
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ao2");
    installed_binary.with_file_name(format!("{filename}.rollback"))
}

#[cfg(unix)]
pub(crate) fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}
