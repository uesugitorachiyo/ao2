use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::{binary_name_for_target, create_tar_gz, sha256_file};
use crate::release_installer_scripts::write_installer_scripts;
use crate::release_verifier_scripts::write_release_verifier_scripts;
use crate::{runtime_git_commit, runtime_target_label};

pub(crate) fn package_release(
    out_dir: PathBuf,
    version: String,
    binary: Option<PathBuf>,
    target_label: Option<String>,
) -> Result<()> {
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let source_binary = match binary {
        Some(path) => path,
        None => std::env::current_exe().context("resolve current executable")?,
    };
    if !source_binary.is_file() {
        anyhow::bail!("release binary is not a file: {}", source_binary.display());
    }
    let target = target_label.unwrap_or_else(runtime_target_label);
    let packaged_git_commit =
        std::env::var("AO2_PACKAGED_GIT_COMMIT").unwrap_or_else(|_| runtime_git_commit());
    let packaged_build_profile = std::env::var("AO2_PACKAGED_BUILD_PROFILE").unwrap_or_else(|_| {
        option_env!("AO2_BUILD_PROFILE")
            .unwrap_or("unknown")
            .to_string()
    });
    if packaged_build_profile == "release" && version != env!("CARGO_PKG_VERSION") {
        anyhow::bail!(
            "requested release version {version} does not match compiled binary {}",
            env!("CARGO_PKG_VERSION")
        );
    }
    let binary_name = binary_name_for_target(&target);
    let package_name = format!("ao2-{version}-{target}");
    let stage_dir = out_dir.join(format!(".{package_name}.stage"));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)?;
    }
    fs::create_dir_all(stage_dir.join("bin"))?;
    let staged_binary = stage_dir.join("bin").join(binary_name);
    fs::copy(&source_binary, &staged_binary).context("copy release binary into stage")?;
    let binary_sha256 = sha256_file(&staged_binary)?;
    write_installer_scripts(&stage_dir, binary_name)?;
    write_release_verifier_scripts(&stage_dir)?;
    fs::copy(release_legal_file("LICENSE")?, stage_dir.join("LICENSE"))
        .context("copy LICENSE into release stage")?;
    fs::copy(release_legal_file("NOTICE")?, stage_dir.join("NOTICE"))
        .context("copy NOTICE into release stage")?;
    if target == "windows-x86_64" {
        let worker = release_legal_file("scripts/ao2_windows_outbound_worker.py")?;
        fs::copy(worker, stage_dir.join("ao2-windows-outbound-worker.py"))?;
        let launcher = release_legal_file("scripts/ao2-windows-worker.cmd")?;
        fs::copy(launcher, stage_dir.join("ao2-windows-worker.cmd"))?;
    }
    fs::write(stage_dir.join("VERSION"), format!("{version}\n"))?;
    fs::write(
        stage_dir.join("BUILD-PROVENANCE.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.build-provenance.v1",
            "package": "ao2",
            "version": env!("CARGO_PKG_VERSION"),
            "git_commit": packaged_git_commit,
            "build_profile": packaged_build_profile,
            "target": target
        }))?,
    )?;
    fs::write(
        stage_dir.join("SBOM.cdx.json"),
        include_str!(concat!(env!("OUT_DIR"), "/ao2.cdx.json")),
    )?;
    fs::write(
        stage_dir.join("UNINSTALL.txt"),
        "AO2 uninstall\n\nRemove the installed ao2 binary, its rollback copy, and its install-verification sidecar from the install directory.\n\nUnix default:\n  rm -f \"$HOME/.local/bin/ao2\" \"$HOME/.local/bin/ao2.rollback\" \"$HOME/.local/bin/ao2.install-verification.json\"\n\nWindows PowerShell default:\n  Remove-Item -Force -ErrorAction SilentlyContinue \"$env:LOCALAPPDATA\\AO2\\bin\\ao2.exe\", \"$env:LOCALAPPDATA\\AO2\\bin\\ao2.exe.rollback\", \"$env:LOCALAPPDATA\\AO2\\bin\\ao2.exe.install-verification.json\"\n\nUse the same custom AO2_INSTALL_DIR supplied during installation when applicable. Runtime state is not removed automatically.\n",
    )?;
    fs::write(
        stage_dir.join("README.txt"),
        format!(
            "AO2 {version}\n\nVerify this archive offline before installing:\n  sh verify-release.sh\n\nAdd this package's bin directory to PATH, then run:\n  ao2 --help\n\nUninstall instructions:\n  See UNINSTALL.txt\n"
        ),
    )?;

    let mut checksum_paths = vec![
        format!("bin/{binary_name}"),
        "BUILD-PROVENANCE.json".to_string(),
        "LICENSE".to_string(),
        "NOTICE".to_string(),
        "README.txt".to_string(),
        "RELEASE-MANIFEST.json".to_string(),
        "RELEASE-VERIFICATION.json".to_string(),
        "SBOM.cdx.json".to_string(),
        "UNINSTALL.txt".to_string(),
        "VERSION".to_string(),
        "Verify-Release.ps1".to_string(),
        "install.ps1".to_string(),
        "install.sh".to_string(),
        "verify-release.sh".to_string(),
    ];
    if target == "windows-x86_64" {
        checksum_paths.extend([
            "ao2-windows-outbound-worker.py".to_string(),
            "ao2-windows-worker.cmd".to_string(),
        ]);
    }
    let mut archive_files = checksum_paths.clone();
    archive_files.push("SHA256SUMS".to_string());
    archive_files.sort();

    let manifest = serde_json::json!({
        "schema_version": "ao2.release-manifest.v1",
        "package": package_name,
        "version": version,
        "target": target,
        "binary": binary_name,
        "binary_path": format!("bin/{binary_name}"),
        "binary_sha256": binary_sha256,
        "installers": ["install.sh", "install.ps1"],
        "verifiers": ["verify-release.sh", "Verify-Release.ps1"],
        "verification_report": "RELEASE-VERIFICATION.json",
        "build_provenance": "BUILD-PROVENANCE.json",
        "sbom": "SBOM.cdx.json",
        "uninstall": "UNINSTALL.txt",
        "checksum_file": "SHA256SUMS",
        "legal_files": ["LICENSE", "NOTICE"],
        "files": archive_files
    });
    fs::write(
        stage_dir.join("RELEASE-MANIFEST.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    let verification_report = serde_json::json!({
        "schema_version": "ao2.release-archive-offline-verification.v1",
        "status": "packaged",
        "package": package_name,
        "version": version,
        "target": target,
        "binary": binary_name,
        "binary_path": format!("bin/{binary_name}"),
        "checksum_file": "SHA256SUMS",
        "checksum_coverage": checksum_paths,
        "verifiers": ["verify-release.sh", "Verify-Release.ps1"],
        "provider_api_keys_required": false,
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "release_acceptance_owner": "factory-v3 evaluator-closer"
    });
    fs::write(
        stage_dir.join("RELEASE-VERIFICATION.json"),
        serde_json::to_string_pretty(&verification_report)?,
    )?;

    let mut checksum_text = String::new();
    for relative_path in checksum_paths {
        let digest = sha256_file(&stage_dir.join(&relative_path))?;
        checksum_text.push_str(&format!("{digest}  {relative_path}\n"));
    }
    fs::write(stage_dir.join("SHA256SUMS"), checksum_text)?;

    let archive_path = out_dir.join(format!("{package_name}.tar.gz"));
    create_tar_gz(&stage_dir, &archive_path)?;
    fs::remove_dir_all(&stage_dir).with_context(|| format!("remove {}", stage_dir.display()))?;

    let sha256 = sha256_file(&archive_path)?;
    let checksum_path = out_dir.join("SHA256SUMS");
    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("archive filename is utf8")?;
    fs::write(&checksum_path, format!("{sha256}  {archive_name}\n"))?;

    let result = serde_json::json!({
        "binary": binary_name,
        "version": version,
        "target": target,
        "archive": archive_path,
        "sha256": sha256,
        "checksum_file": checksum_path,
        "install_hint": "extract the archive and add its bin directory to PATH"
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn release_legal_file(name: &str) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join(name));
        candidates.push(current_dir.join("../..").join(name));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(name),
    );

    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    anyhow::bail!("release legal file {name} not found; run from the repository root")
}
