use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::install_cmd::{install_update_result, InstallUpdateOptions};
use crate::release_assets::{
    copy_release_asset, download_github_release_assets, download_release_asset_from_metadata,
    download_release_assets, read_release_metadata, release_asset_name,
    release_metadata_from_asset_dir, required_provenance_asset_names, upgrade_check_report,
};
use crate::runtime_target_label;

#[derive(Debug, Subcommand)]
pub(crate) enum UpgradeCommand {
    Check {
        #[arg(long)]
        release_file: Option<PathBuf>,
        #[arg(long)]
        release_url: Option<String>,
    },
    Apply(Box<UpgradeApplyCommand>),
}

#[derive(Debug, Args)]
pub(crate) struct UpgradeApplyCommand {
    #[arg(long)]
    release_file: Option<PathBuf>,
    #[arg(long)]
    release_url: Option<String>,
    #[arg(long)]
    github_release: Option<String>,
    #[arg(long, default_value = "uesugitorachiyo/ao2")]
    repo: String,
    #[arg(long)]
    asset_dir: Option<PathBuf>,
    #[arg(long)]
    release_base_url: Option<String>,
    #[arg(long, default_value = "target/ao2-upgrade")]
    download_dir: PathBuf,
    #[arg(long)]
    provenance_dir: Option<PathBuf>,
    #[arg(long)]
    install_dir: Option<PathBuf>,
    #[arg(long)]
    target_label: Option<String>,
}

pub(crate) struct UpgradeApplyOptions {
    pub(crate) release_file: Option<PathBuf>,
    pub(crate) release_url: Option<String>,
    pub(crate) github_release: Option<String>,
    pub(crate) repo: String,
    pub(crate) asset_dir: Option<PathBuf>,
    pub(crate) release_base_url: Option<String>,
    pub(crate) download_dir: PathBuf,
    pub(crate) provenance_dir: Option<PathBuf>,
    pub(crate) install_dir: Option<PathBuf>,
    pub(crate) target_label: Option<String>,
}

pub(crate) fn upgrade(command: UpgradeCommand) -> Result<()> {
    match command {
        UpgradeCommand::Check {
            release_file,
            release_url,
        } => upgrade_check(release_file, release_url),
        UpgradeCommand::Apply(options) => {
            let options = *options;
            upgrade_apply(UpgradeApplyOptions {
                release_file: options.release_file,
                release_url: options.release_url,
                github_release: options.github_release,
                repo: options.repo,
                asset_dir: options.asset_dir,
                release_base_url: options.release_base_url,
                download_dir: options.download_dir,
                provenance_dir: options.provenance_dir,
                install_dir: options.install_dir,
                target_label: options.target_label,
            })
        }
    }
}

pub(crate) fn upgrade_apply(options: UpgradeApplyOptions) -> Result<()> {
    let UpgradeApplyOptions {
        release_file,
        release_url,
        github_release,
        repo,
        mut asset_dir,
        release_base_url,
        download_dir,
        provenance_dir,
        install_dir,
        target_label,
    } = options;
    let release = if let Some(tag) = github_release {
        if release_file.is_some()
            || release_url.is_some()
            || asset_dir.is_some()
            || release_base_url.is_some()
        {
            anyhow::bail!(
                "--github-release cannot be combined with release metadata or asset source flags"
            );
        }
        download_github_release_assets(&repo, &tag, &download_dir)?;
        asset_dir = Some(download_dir.clone());
        release_metadata_from_asset_dir(&download_dir, &tag)?
    } else {
        read_release_metadata(release_file, release_url)?
    };
    let check = upgrade_check_report(&release)?;
    let latest_version = check["latest_version"]
        .as_str()
        .context("upgrade check missing latest_version")?
        .to_string();
    let target = target_label.unwrap_or_else(runtime_target_label);
    let archive_name = release_asset_name(&release, &target, &latest_version)
        .with_context(|| format!("release is missing archive asset for target {target}"))?;
    let provenance_dir = provenance_dir.unwrap_or_else(|| download_dir.join("provenance"));
    fs::create_dir_all(&download_dir)
        .with_context(|| format!("create {}", download_dir.display()))?;
    fs::create_dir_all(&provenance_dir)
        .with_context(|| format!("create {}", provenance_dir.display()))?;

    let archive = if let Some(asset_dir) = asset_dir {
        copy_release_asset(&asset_dir, &archive_name, &download_dir)?;
        for name in required_provenance_asset_names(&archive_name) {
            copy_release_asset(&asset_dir, &name, &provenance_dir)?;
        }
        download_dir.join(&archive_name)
    } else if let Some(base_url) = release_base_url {
        download_release_assets(&base_url, &latest_version, &target, &provenance_dir)?
    } else {
        download_release_asset_from_metadata(&release, &archive_name, &download_dir)?;
        for name in required_provenance_asset_names(&archive_name) {
            download_release_asset_from_metadata(&release, &name, &provenance_dir)?;
        }
        download_dir.join(&archive_name)
    };

    let install = install_update_result(InstallUpdateOptions {
        archive: Some(archive),
        release_base_url: None,
        version: latest_version,
        target_label: Some(target),
        provenance_dir,
        public_checksum_manifest: None,
        install_dir,
    })?;
    let result = serde_json::json!({
        "schema_version": "ao2.upgrade-apply.v1",
        "status": "upgraded",
        "check": check,
        "install": install,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

pub(crate) fn upgrade_check(
    release_file: Option<PathBuf>,
    release_url: Option<String>,
) -> Result<()> {
    let release = read_release_metadata(release_file, release_url)?;
    let result = upgrade_check_report(&release)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
