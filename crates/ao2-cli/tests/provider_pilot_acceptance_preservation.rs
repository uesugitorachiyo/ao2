use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn provider_pilot_acceptance_preservation_script_copies_live_codex_and_claude_bundles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source_root = tempfile::tempdir().expect("source tempdir");
    let acceptance_root = source_root
        .path()
        .join("target")
        .join("provider-pilot-acceptance");
    let preserved_root = tempfile::tempdir().expect("preserved tempdir");
    let json_path = preserved_root.path().join("summary.json");
    let tag = "v9.9.9-test";

    write_provider_pilot_acceptance_bundle(
        &acceptance_root
            .join(tag)
            .join("codex")
            .join("provider-pilot-acceptance.json"),
        "codex",
        "ao2.codex-provider-pilot-acceptance.v1",
    );
    write_provider_pilot_acceptance_bundle(
        &acceptance_root
            .join(tag)
            .join("claude")
            .join("provider-pilot-acceptance.json"),
        "claude",
        "ao2.claude-provider-pilot-acceptance.v1",
    );
    write_provider_pilot_acceptance_bundle(
        &acceptance_root
            .join(tag)
            .join("antigravity")
            .join("provider-pilot-acceptance.json"),
        "antigravity",
        "ao2.antigravity-provider-pilot-acceptance.v1",
    );

    let output = Command::new(sh_command())
        .arg(root.join("scripts/preserve-provider-pilot-acceptance.sh"))
        .env("AO2_PROVIDER_PILOT_ACCEPTANCE_ROOT", &acceptance_root)
        .env("AO2_PROVIDER_PILOT_PRESERVE_TAG", tag)
        .env(
            "AO2_PROVIDER_PILOT_PRESERVE_OUT",
            preserved_root.path().join(tag),
        )
        .env("AO2_PROVIDER_PILOT_PRESERVE_JSON", &json_path)
        .output()
        .expect("run provider pilot acceptance preservation script");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(preserved_root
        .path()
        .join(tag)
        .join("codex")
        .join("provider-pilot-acceptance.json")
        .exists());
    assert!(preserved_root
        .path()
        .join(tag)
        .join("claude")
        .join("provider-pilot-acceptance.json")
        .exists());
    assert!(preserved_root
        .path()
        .join(tag)
        .join("antigravity")
        .join("provider-pilot-acceptance.json")
        .exists());

    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_path).expect("preservation summary exists"))
            .expect("preservation summary is json");
    assert_eq!(
        summary["schema"],
        "ao2.provider-pilot-acceptance-preservation.v1"
    );
    assert_eq!(summary["status"], "passed");
    assert_eq!(summary["tag"], tag);
    assert_eq!(summary["providers"]["codex"]["source_class"], "live");
    assert_eq!(summary["providers"]["claude"]["source_class"], "live");
    assert_eq!(summary["providers"]["antigravity"]["source_class"], "live");

    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    assert!(package_json.contains("\"release:preserve-provider-acceptance\""));
}

fn write_provider_pilot_acceptance_bundle(path: &Path, provider: &str, schema: &str) {
    fs::create_dir_all(path.parent().expect("bundle parent")).expect("create bundle parent");
    fs::write(
        path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": schema,
            "status": "passed",
            "provider": provider,
            "run_id": format!("live-{provider}-provider-pilot-v9.9.9-test"),
            "smoke": {
                "score": 100,
                "minimum_score": 90
            },
            "score": {
                "schema": "ao2.provider-evidence-scorecard.v1",
                "score": 100,
                "verdict": "ready"
            },
            "replay": {
                "status": "accepted",
                "digest_failures": 0
            }
        }))
        .expect("serialize bundle"),
    )
    .expect("write provider pilot acceptance bundle");
}

fn sh_command() -> PathBuf {
    ao2_adapters::posix_shell_command().unwrap_or_else(|| PathBuf::from("sh"))
}
