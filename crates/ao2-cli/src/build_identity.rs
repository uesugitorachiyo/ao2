use anyhow::Result;

#[used]
static AO2_RUST_BUILD_PROVENANCE_MARKER: &str = concat!(
    "AO_RUST_BUILD_PROVENANCE_V1\0",
    "{\"build_profile\":\"",
    env!("AO2_BUILD_PROFILE"),
    "\",\"cargo_lock_sha256\":\"",
    env!("AO2_CARGO_LOCK_SHA256"),
    "\",\"repository\":\"ao2\",\"source_sha\":\"",
    env!("AO2_GIT_COMMIT"),
    "\",\"source_modified\":",
    env!("AO2_SOURCE_MODIFIED"),
    ",\"target\":\"",
    env!("AO2_BUILD_TARGET"),
    "\",\"version\":\"",
    env!("CARGO_PKG_VERSION"),
    "\"}\0"
);

pub(crate) fn rust_build_provenance_marker() -> &'static str {
    AO2_RUST_BUILD_PROVENANCE_MARKER
}

pub(crate) fn version(json: bool) -> Result<()> {
    std::hint::black_box(rust_build_provenance_marker());
    let target = runtime_target_label();
    let git_commit = runtime_git_commit();
    if json {
        let version = serde_json::json!({
            "package": "ao2",
            "version": env!("CARGO_PKG_VERSION"),
            "target": target,
            "git_commit": git_commit,
            "build_profile": option_env!("AO2_BUILD_PROFILE").unwrap_or("unknown"),
            "release_manifest_schema": "ao2.release-manifest.v1",
            "release_provenance_schema": "ao2.release-provenance.v1"
        });
        println!("{}", serde_json::to_string_pretty(&version)?);
    } else {
        println!("ao2 {}", env!("CARGO_PKG_VERSION"));
        println!("target={target}");
        println!("git_commit={git_commit}");
    }
    Ok(())
}

pub(crate) fn runtime_git_commit() -> String {
    option_env!("AO2_GIT_COMMIT")
        .unwrap_or("unknown")
        .to_string()
}

pub(crate) fn runtime_target_label() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::rust_build_provenance_marker;

    #[test]
    fn embedded_rust_provenance_is_strictly_bound() {
        let marker = rust_build_provenance_marker();
        let payload = marker
            .strip_prefix("AO_RUST_BUILD_PROVENANCE_V1\0")
            .and_then(|value| value.strip_suffix('\0'))
            .expect("bounded marker");
        let value: serde_json::Value = serde_json::from_str(payload).expect("marker JSON");
        assert_eq!(value["repository"], "ao2");
        assert_eq!(value["source_sha"].as_str().map(str::len), Some(40));
        assert_eq!(value["cargo_lock_sha256"].as_str().map(str::len), Some(64));
        assert!(value["source_modified"].is_boolean());
        assert!(!value["target"].as_str().unwrap_or_default().is_empty());
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    }
}
