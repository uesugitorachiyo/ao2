use crate::provider_ops::provider_profiles_json;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
pub(crate) fn init(target: PathBuf) -> Result<()> {
    let state = target.join(".ao2");
    fs::create_dir_all(&state).with_context(|| format!("create {}", state.display()))?;
    let readme = state.join("README.md");
    if !readme.exists() {
        fs::write(
            &readme,
            "# AO2 Local State\n\nRun artifacts are stored under `runs/<run-id>/`.\n",
        )?;
    }
    let profiles = state.join("provider-profiles.json");
    if !profiles.exists() {
        fs::write(&profiles, provider_profiles_json()?)?;
    }
    println!("initialized {}", state.display());
    Ok(())
}

pub(crate) fn status(target: PathBuf, run_id: String) -> Result<()> {
    let path = target
        .join(".ao2")
        .join("runs")
        .join(&run_id)
        .join("run-record.json");
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

pub(crate) fn export(target: PathBuf, run_id: String) -> Result<()> {
    let path = target
        .join(".ao2")
        .join("runs")
        .join(&run_id)
        .join("evidence-pack")
        .join("evidence-pack.json");
    if !path.exists() {
        anyhow::bail!("evidence pack not found: {}", path.display());
    }
    println!("{}", path.display());
    Ok(())
}
