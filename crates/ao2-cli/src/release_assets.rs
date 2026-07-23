use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};

use crate::cli_util::json_string;
use crate::release_versioning::compare_versions;

pub(crate) fn read_release_metadata(
    release_file: Option<PathBuf>,
    release_url: Option<String>,
) -> Result<serde_json::Value> {
    let release = match (release_file, release_url) {
        (Some(path), None) => fs::read_to_string(&path)
            .with_context(|| format!("read release metadata {}", path.display()))?,
        (None, Some(url)) => read_url(&url)?,
        (Some(_), Some(_)) => anyhow::bail!("use either --release-file or --release-url"),
        (None, None) => anyhow::bail!("--release-file or --release-url is required"),
    };
    serde_json::from_str(&release).context("parse release metadata json")
}

pub(crate) fn upgrade_check_report(release: &serde_json::Value) -> Result<serde_json::Value> {
    let latest_version = release_version(release).context("release metadata missing version")?;
    let current_version = env!("CARGO_PKG_VERSION");
    let update_available =
        compare_versions(&latest_version, current_version) == std::cmp::Ordering::Greater;
    let assets = release
        .get("assets")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let result = serde_json::json!({
        "schema_version": "ao2.upgrade-check.v1",
        "current_version": current_version,
        "latest_version": latest_version,
        "update_available": update_available,
        "assets": assets,
    });
    Ok(result)
}

pub(crate) fn release_asset_name(
    release: &serde_json::Value,
    target: &str,
    version: &str,
) -> Option<String> {
    let expected = format!("ao2-{version}-{target}.tar.gz");
    release
        .get("assets")
        .and_then(|assets| assets.as_array())
        .and_then(|assets| {
            assets
                .iter()
                .filter_map(release_asset_name_value)
                .find(|name| name == &expected)
                .or_else(|| {
                    assets
                        .iter()
                        .filter_map(release_asset_name_value)
                        .find(|name| name.contains(target) && name.ends_with(".tar.gz"))
                })
        })
}

fn release_asset_name_value(asset: &serde_json::Value) -> Option<String> {
    asset
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

pub(crate) fn required_provenance_asset_names(archive_name: &str) -> Vec<String> {
    vec![
        format!("{archive_name}.sha256"),
        format!("{archive_name}.sig"),
        "ao2-release-signing-public.pem".to_string(),
        "ao2-release-provenance.json".to_string(),
        "ao2-release-provenance.json.sig".to_string(),
    ]
}

pub(crate) fn copy_release_asset(asset_dir: &Path, name: &str, dest_dir: &Path) -> Result<()> {
    let source = asset_dir.join(name);
    let dest = dest_dir.join(name);
    if source == dest {
        return Ok(());
    }
    fs::copy(&source, &dest).with_context(|| {
        format!(
            "copy release asset {} to {}",
            source.display(),
            dest.display()
        )
    })?;
    Ok(())
}

pub(crate) fn download_release_asset_from_metadata(
    release: &serde_json::Value,
    name: &str,
    dest_dir: &Path,
) -> Result<()> {
    let url = release
        .get("assets")
        .and_then(|assets| assets.as_array())
        .and_then(|assets| {
            assets.iter().find_map(|asset| {
                let asset_name = asset.get("name")?.as_str()?;
                if asset_name != name {
                    return None;
                }
                asset
                    .get("browser_download_url")
                    .or_else(|| asset.get("url"))
                    .and_then(|url| url.as_str())
                    .map(str::to_string)
            })
        })
        .with_context(|| format!("release metadata missing downloadable asset {name}"))?;
    download_file(&url, &dest_dir.join(name))
}

pub(crate) fn download_github_release_assets(
    repo: &str,
    tag: &str,
    download_dir: &Path,
) -> Result<()> {
    fs::create_dir_all(download_dir)
        .with_context(|| format!("create {}", download_dir.display()))?;
    if let Some(fake_asset_dir) = std::env::var_os("AO2_TEST_FAKE_GH_ASSET_DIR") {
        let fake_asset_dir = PathBuf::from(fake_asset_dir);
        for entry in fs::read_dir(&fake_asset_dir)
            .with_context(|| format!("read {}", fake_asset_dir.display()))?
        {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::copy(entry.path(), download_dir.join(entry.file_name())).with_context(
                    || {
                        format!(
                            "copy fake gh asset {} to {}",
                            entry.path().display(),
                            download_dir.display()
                        )
                    },
                )?;
            }
        }
        return Ok(());
    }
    let status = ProcessCommand::new("gh")
        .args(["release", "download", tag, "--repo", repo, "--dir"])
        .arg(download_dir)
        .arg("--clobber")
        .status()
        .with_context(|| format!("run gh release download {tag} from {repo}"))?;
    if !status.success() {
        anyhow::bail!("gh release download failed for {repo} {tag}");
    }
    Ok(())
}

pub(crate) fn release_metadata_from_asset_dir(
    asset_dir: &Path,
    tag: &str,
) -> Result<serde_json::Value> {
    let mut assets = Vec::new();
    for entry in fs::read_dir(asset_dir).with_context(|| format!("read {}", asset_dir.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            assets.push(serde_json::json!({
                "name": name,
                "browser_download_url": entry.path().display().to_string(),
            }));
        }
    }
    assets.sort_by_key(|left| json_string(left, "name"));
    Ok(serde_json::json!({
        "tagName": tag,
        "assets": assets,
    }))
}

fn release_version(release: &serde_json::Value) -> Option<String> {
    release
        .get("tagName")
        .or_else(|| release.get("tag_name"))
        .or_else(|| release.get("name"))
        .and_then(|value| value.as_str())
        .map(|version| version.trim_start_matches('v').to_string())
}

fn read_url(url: &str) -> Result<String> {
    let output = ProcessCommand::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .arg(url)
        .output()
        .with_context(|| format!("run curl for {url}"))?;
    if !output.status.success() {
        anyhow::bail!("download failed: {url}");
    }
    String::from_utf8(output.stdout).context("release metadata is utf8")
}

pub(crate) fn download_release_assets(
    base_url: &str,
    version: &str,
    target: &str,
    provenance_dir: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(provenance_dir)
        .with_context(|| format!("create {}", provenance_dir.display()))?;
    let archive_name = format!("ao2-{version}-{target}.tar.gz");
    let archive = provenance_dir.join(&archive_name);
    download_file(
        &format!("{}/{}", base_url.trim_end_matches('/'), archive_name),
        &archive,
    )?;
    for asset in required_provenance_asset_names(&archive_name) {
        download_file(
            &format!("{}/{}", base_url.trim_end_matches('/'), asset),
            &provenance_dir.join(asset),
        )?;
    }
    Ok(archive)
}

fn download_file(url: &str, dest: &Path) -> Result<()> {
    let status = ProcessCommand::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(dest)
        .arg(url)
        .status()
        .with_context(|| format!("run curl for {url}"))?;
    if !status.success() {
        anyhow::bail!("download failed: {url}");
    }
    Ok(())
}
