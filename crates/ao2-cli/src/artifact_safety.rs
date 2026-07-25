use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub(crate) fn factory_app_run_bundle_reject_secret_markers(
    path: &Path,
    relative_path: &str,
) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    for marker in [
        "Authorization: Bearer ",
        "AO2_CP_API_TOKEN=",
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
    ] {
        if text.contains(marker) {
            anyhow::bail!(
                "factory app-run bundle contains forbidden secret marker {marker:?} in {relative_path}"
            );
        }
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
        factory_app_run_bundle_reject_secret_fields(&value, relative_path)?;
    }
    Ok(())
}

pub(crate) fn factory_app_run_bundle_reject_secret_fields(
    value: &serde_json::Value,
    path: &str,
) -> Result<()> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let key_lower = key.to_ascii_lowercase();
                if matches!(
                    key_lower.as_str(),
                    "token" | "access_token" | "refresh_token"
                ) {
                    anyhow::bail!(
                        "factory app-run bundle contains forbidden secret field at {path}.{key}"
                    );
                }
                factory_app_run_bundle_reject_secret_fields(child, &format!("{path}.{key}"))?;
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                factory_app_run_bundle_reject_secret_fields(child, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}
