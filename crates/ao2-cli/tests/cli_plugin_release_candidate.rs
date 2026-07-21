use std::fs;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
}

fn ao2<const N: usize>(args: [&str; N]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env(
        "AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER",
        "human:test-auto-approve",
    );
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_plugin_release_candidate_aggregates_digest_pinned_plugin_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("release-candidate-inputs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let artifact = |name: &str, schema_version: &str, status: &str| {
        let path = inputs_dir.join(format!("{name}.json"));
        let body = serde_json::json!({
            "schema_version": schema_version,
            "status": status,
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            },
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            },
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "side_effects": {
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_write_memory": false,
                "would_mutate_control_plane": false,
                "would_mutate_ao_artifacts": false,
                "would_approve_release": false
            },
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        });
        fs::write(&path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
        path
    };
    let archive = |name: &str| {
        let path = inputs_dir.join(format!("{name}.tar.gz"));
        fs::write(&path, format!("{name} archive fixture")).unwrap();
        path
    };

    let package_summary = artifact("ao2-plugin-package", "ao2.plugin-package.v1", "packaged");
    let package_archive = archive("ao2-plugin-package");
    let distribution_rehearsal = artifact(
        "plugin-distribution-rehearsal",
        "ao2.plugin-distribution-rehearsal.v1",
        "passed",
    );
    let adapter_bundle = artifact(
        "k37-plugin-adapter-observer-bundle",
        "ao2.k37-plugin-adapter-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let adapter_archive = archive("k37-plugin-adapter-observer-bundle");
    let adapter_install_smoke_bundle = artifact(
        "k37-plugin-adapter-install-smoke-observer-bundle",
        "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let adapter_install_smoke_archive = archive("k37-plugin-adapter-install-smoke-observer-bundle");
    let consumer_lifecycle_bundle = artifact(
        "k37-plugin-consumer-lifecycle-observer-bundle",
        "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let consumer_lifecycle_archive = archive("k37-plugin-consumer-lifecycle-observer-bundle");
    let release_gate_observer_bundle = artifact(
        "k37-release-gate-with-replacement-observer-bundle",
        "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let release_gate_observer_archive =
        archive("k37-release-gate-with-replacement-observer-bundle");
    let mut release_gate_summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&release_gate_observer_bundle).unwrap()).unwrap();
    release_gate_summary["archive_sha256"] =
        serde_json::json!(sha256_path(&release_gate_observer_archive));
    fs::write(
        &release_gate_observer_bundle,
        serde_json::to_string_pretty(&release_gate_summary).unwrap(),
    )
    .unwrap();
    let control_plane_handoff_verification = artifact(
        "control-plane-fixture-handoff-verification",
        "ao2.control-plane-fixture-handoff-verification.v1",
        "passed",
    );

    let out_dir = temp.path().join("plugin-release-candidate");
    let release = ao2([
        "plugin",
        "release-candidate",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        &sha256_path(&package_summary),
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &sha256_path(&package_archive),
        "--distribution-rehearsal",
        distribution_rehearsal.to_str().unwrap(),
        "--distribution-rehearsal-sha256",
        &sha256_path(&distribution_rehearsal),
        "--adapter-observer-bundle",
        adapter_bundle.to_str().unwrap(),
        "--adapter-observer-bundle-sha256",
        &sha256_path(&adapter_bundle),
        "--adapter-observer-archive",
        adapter_archive.to_str().unwrap(),
        "--adapter-observer-archive-sha256",
        &sha256_path(&adapter_archive),
        "--adapter-install-smoke-observer-bundle",
        adapter_install_smoke_bundle.to_str().unwrap(),
        "--adapter-install-smoke-observer-bundle-sha256",
        &sha256_path(&adapter_install_smoke_bundle),
        "--adapter-install-smoke-observer-archive",
        adapter_install_smoke_archive.to_str().unwrap(),
        "--adapter-install-smoke-observer-archive-sha256",
        &sha256_path(&adapter_install_smoke_archive),
        "--consumer-lifecycle-observer-bundle",
        consumer_lifecycle_bundle.to_str().unwrap(),
        "--consumer-lifecycle-observer-bundle-sha256",
        &sha256_path(&consumer_lifecycle_bundle),
        "--consumer-lifecycle-observer-archive",
        consumer_lifecycle_archive.to_str().unwrap(),
        "--consumer-lifecycle-observer-archive-sha256",
        &sha256_path(&consumer_lifecycle_archive),
        "--release-gate-with-replacement-observer-bundle",
        release_gate_observer_bundle.to_str().unwrap(),
        "--release-gate-with-replacement-observer-bundle-sha256",
        &sha256_path(&release_gate_observer_bundle),
        "--release-gate-with-replacement-observer-archive",
        release_gate_observer_archive.to_str().unwrap(),
        "--release-gate-with-replacement-observer-archive-sha256",
        &sha256_path(&release_gate_observer_archive),
        "--control-plane-fixture-handoff-verification",
        control_plane_handoff_verification.to_str().unwrap(),
        "--control-plane-fixture-handoff-verification-sha256",
        &sha256_path(&control_plane_handoff_verification),
        "--control-plane-readback-commit",
        "a54a60f",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(release.status.success(), "{}", stderr(&release));
    let json: serde_json::Value = serde_json::from_str(&stdout(&release)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-release-candidate.v1");
    assert_eq!(json["status"], "ready_for_local_release_review");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["control_plane_readback"]["commit"], "a54a60f");
    assert_eq!(json["control_plane_readback"]["role"], "read_only_observer");
    assert_eq!(
        json["evidence"]["package"]["summary_sha256"],
        sha256_path(&package_summary)
    );
    assert_eq!(
        json["evidence"]["consumer_lifecycle_observer_bundle"]["summary_sha256"],
        sha256_path(&consumer_lifecycle_bundle)
    );
    assert_eq!(
        json["evidence"]["release_gate_with_replacement_observer_bundle"]["summary_sha256"],
        sha256_path(&release_gate_observer_bundle)
    );
    assert_eq!(
        json["evidence"]["release_gate_with_replacement_observer_bundle"]["archive_sha256"],
        sha256_path(&release_gate_observer_archive)
    );
    assert!(json["release_review_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|input| input.as_str()
            == Some("ao2.k37-release-gate-with-replacement-observer-bundle.v1")));
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");
    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    let summary_sha256 = sha256_path(summary_path);
    assert_eq!(json["summary_sha256"], summary_sha256);

    let verify = ao2([
        "plugin",
        "release-candidate-verify",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.plugin-release-candidate-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["control_plane_readback"]["commit"], "a54a60f");

    let bad_digest = ao2([
        "plugin",
        "release-candidate",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &sha256_path(&package_archive),
        "--distribution-rehearsal",
        distribution_rehearsal.to_str().unwrap(),
        "--distribution-rehearsal-sha256",
        &sha256_path(&distribution_rehearsal),
        "--adapter-observer-bundle",
        adapter_bundle.to_str().unwrap(),
        "--adapter-observer-bundle-sha256",
        &sha256_path(&adapter_bundle),
        "--adapter-observer-archive",
        adapter_archive.to_str().unwrap(),
        "--adapter-observer-archive-sha256",
        &sha256_path(&adapter_archive),
        "--adapter-install-smoke-observer-bundle",
        adapter_install_smoke_bundle.to_str().unwrap(),
        "--adapter-install-smoke-observer-bundle-sha256",
        &sha256_path(&adapter_install_smoke_bundle),
        "--adapter-install-smoke-observer-archive",
        adapter_install_smoke_archive.to_str().unwrap(),
        "--adapter-install-smoke-observer-archive-sha256",
        &sha256_path(&adapter_install_smoke_archive),
        "--consumer-lifecycle-observer-bundle",
        consumer_lifecycle_bundle.to_str().unwrap(),
        "--consumer-lifecycle-observer-bundle-sha256",
        &sha256_path(&consumer_lifecycle_bundle),
        "--consumer-lifecycle-observer-archive",
        consumer_lifecycle_archive.to_str().unwrap(),
        "--consumer-lifecycle-observer-archive-sha256",
        &sha256_path(&consumer_lifecycle_archive),
        "--release-gate-with-replacement-observer-bundle",
        release_gate_observer_bundle.to_str().unwrap(),
        "--release-gate-with-replacement-observer-bundle-sha256",
        &sha256_path(&release_gate_observer_bundle),
        "--release-gate-with-replacement-observer-archive",
        release_gate_observer_archive.to_str().unwrap(),
        "--release-gate-with-replacement-observer-archive-sha256",
        &sha256_path(&release_gate_observer_archive),
        "--control-plane-fixture-handoff-verification",
        control_plane_handoff_verification.to_str().unwrap(),
        "--control-plane-fixture-handoff-verification-sha256",
        &sha256_path(&control_plane_handoff_verification),
        "--control-plane-readback-commit",
        "a54a60f",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("package summary sha256 mismatch"));
}

#[test]
fn cli_plugin_release_candidate_windows_recovery_writes_digest_pinned_runner() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("release-candidate-recovery-inputs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let artifact = |name: &str, schema_version: &str, status: &str| {
        let path = inputs_dir.join(format!("{name}.json"));
        let body = serde_json::json!({
            "schema_version": schema_version,
            "status": status,
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            },
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            },
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "side_effects": {
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_write_memory": false,
                "would_mutate_control_plane": false,
                "would_mutate_ao_artifacts": false,
                "would_approve_release": false
            },
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        });
        fs::write(&path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
        path
    };
    let archive = |name: &str| {
        let path = inputs_dir.join(format!("{name}.tar.gz"));
        fs::write(&path, format!("{name} archive fixture")).unwrap();
        path
    };

    let package_summary = artifact("ao2-plugin-package", "ao2.plugin-package.v1", "packaged");
    let package_archive = archive("ao2-plugin-package");
    let distribution_rehearsal = artifact(
        "plugin-distribution-rehearsal",
        "ao2.plugin-distribution-rehearsal.v1",
        "passed",
    );
    let adapter_bundle = artifact(
        "k37-plugin-adapter-observer-bundle",
        "ao2.k37-plugin-adapter-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let adapter_archive = archive("k37-plugin-adapter-observer-bundle");
    let adapter_install_smoke_bundle = artifact(
        "k37-plugin-adapter-install-smoke-observer-bundle",
        "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let adapter_install_smoke_archive = archive("k37-plugin-adapter-install-smoke-observer-bundle");
    let consumer_lifecycle_bundle = artifact(
        "k37-plugin-consumer-lifecycle-observer-bundle",
        "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let consumer_lifecycle_archive = archive("k37-plugin-consumer-lifecycle-observer-bundle");
    let release_gate_observer_bundle = artifact(
        "k37-release-gate-with-replacement-observer-bundle",
        "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
        "ready_for_k37_observation",
    );
    let release_gate_observer_archive =
        archive("k37-release-gate-with-replacement-observer-bundle");
    let mut release_gate_summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&release_gate_observer_bundle).unwrap()).unwrap();
    release_gate_summary["archive_sha256"] =
        serde_json::json!(sha256_path(&release_gate_observer_archive));
    fs::write(
        &release_gate_observer_bundle,
        serde_json::to_string_pretty(&release_gate_summary).unwrap(),
    )
    .unwrap();
    let control_plane_handoff_verification = artifact(
        "control-plane-fixture-handoff-verification",
        "ao2.control-plane-fixture-handoff-verification.v1",
        "passed",
    );

    let out_dir = temp
        .path()
        .join("plugin-release-candidate-windows-recovery");
    let recovery = ao2([
        "plugin",
        "release-candidate-windows-recovery",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        &sha256_path(&package_summary),
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &sha256_path(&package_archive),
        "--distribution-rehearsal",
        distribution_rehearsal.to_str().unwrap(),
        "--distribution-rehearsal-sha256",
        &sha256_path(&distribution_rehearsal),
        "--adapter-observer-bundle",
        adapter_bundle.to_str().unwrap(),
        "--adapter-observer-bundle-sha256",
        &sha256_path(&adapter_bundle),
        "--adapter-observer-archive",
        adapter_archive.to_str().unwrap(),
        "--adapter-observer-archive-sha256",
        &sha256_path(&adapter_archive),
        "--adapter-install-smoke-observer-bundle",
        adapter_install_smoke_bundle.to_str().unwrap(),
        "--adapter-install-smoke-observer-bundle-sha256",
        &sha256_path(&adapter_install_smoke_bundle),
        "--adapter-install-smoke-observer-archive",
        adapter_install_smoke_archive.to_str().unwrap(),
        "--adapter-install-smoke-observer-archive-sha256",
        &sha256_path(&adapter_install_smoke_archive),
        "--consumer-lifecycle-observer-bundle",
        consumer_lifecycle_bundle.to_str().unwrap(),
        "--consumer-lifecycle-observer-bundle-sha256",
        &sha256_path(&consumer_lifecycle_bundle),
        "--consumer-lifecycle-observer-archive",
        consumer_lifecycle_archive.to_str().unwrap(),
        "--consumer-lifecycle-observer-archive-sha256",
        &sha256_path(&consumer_lifecycle_archive),
        "--release-gate-with-replacement-observer-bundle",
        release_gate_observer_bundle.to_str().unwrap(),
        "--release-gate-with-replacement-observer-bundle-sha256",
        &sha256_path(&release_gate_observer_bundle),
        "--release-gate-with-replacement-observer-archive",
        release_gate_observer_archive.to_str().unwrap(),
        "--release-gate-with-replacement-observer-archive-sha256",
        &sha256_path(&release_gate_observer_archive),
        "--control-plane-fixture-handoff-verification",
        control_plane_handoff_verification.to_str().unwrap(),
        "--control-plane-fixture-handoff-verification-sha256",
        &sha256_path(&control_plane_handoff_verification),
        "--control-plane-readback-commit",
        "a54a60f",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(recovery.status.success(), "{}", stderr(&recovery));

    let json: serde_json::Value = serde_json::from_str(&stdout(&recovery)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-release-candidate-windows-recovery.v1"
    );
    assert_eq!(json["status"], "ready_for_windows_execution");
    assert_eq!(json["platform"], "windows");
    assert_eq!(json["control_plane_readback"]["commit"], "a54a60f");
    assert_eq!(
        json["release_review_inputs"]["package"]["summary_sha256"],
        sha256_path(&package_summary)
    );
    assert_eq!(
        json["release_review_inputs"]["adapter_observer_bundle"]["archive_sha256"],
        sha256_path(&adapter_archive)
    );
    assert_eq!(
        json["release_review_inputs"]["release_gate_with_replacement_observer_bundle"]
            ["summary_sha256"],
        sha256_path(&release_gate_observer_bundle)
    );
    assert_eq!(
        json["release_review_inputs"]["release_gate_with_replacement_observer_bundle"]
            ["archive_sha256"],
        sha256_path(&release_gate_observer_archive)
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["side_effects"]["provider_execution_started"], false);
    assert_eq!(json["side_effects"]["queue_mutated"], false);
    assert_eq!(json["side_effects"]["memory_written"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["side_effects"]["ao_artifacts_mutated"], false);
    assert_eq!(json["side_effects"]["release_approved"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let manifest_path = Path::new(json["manifest_path"].as_str().unwrap());
    let script_path = Path::new(json["script_path"].as_str().unwrap());
    assert!(manifest_path.is_file());
    assert!(script_path.is_file());
    assert_eq!(json["manifest_sha256"], sha256_path(manifest_path));
    assert_eq!(json["script_sha256"], sha256_path(script_path));

    for input_name in [
        "ao2-plugin-package.json",
        "ao2-plugin-package.tar.gz",
        "plugin-distribution-rehearsal.json",
        "k37-plugin-adapter-observer-bundle.json",
        "k37-plugin-adapter-observer-bundle.tar.gz",
        "k37-plugin-adapter-install-smoke-observer-bundle.json",
        "k37-plugin-adapter-install-smoke-observer-bundle.tar.gz",
        "k37-plugin-consumer-lifecycle-observer-bundle.json",
        "k37-plugin-consumer-lifecycle-observer-bundle.tar.gz",
        "k37-release-gate-with-replacement-observer-bundle.json",
        "k37-release-gate-with-replacement-observer-bundle.tar.gz",
        "control-plane-fixture-handoff-verification.json",
    ] {
        assert!(out_dir.join("inputs").join(input_name).is_file());
    }

    let script = fs::read_to_string(script_path).unwrap();
    assert!(script.contains("param("));
    assert!(script.contains("plugin release-candidate"));
    assert!(script.contains("plugin release-candidate-verify"));
    assert!(script.contains("Join-Path $PSScriptRoot"));
    assert!(script.contains(&sha256_path(&package_summary)));
    assert!(script.contains(&sha256_path(&adapter_archive)));
    assert!(script.contains("k37-release-gate-with-replacement-observer-bundle.json"));
    assert!(script.contains(&sha256_path(&release_gate_observer_bundle)));
    assert!(script.contains("a54a60f"));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&recovery).contains(forbidden),
            "release-candidate recovery output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(manifest_path)
                .unwrap()
                .contains(forbidden),
            "release-candidate recovery manifest exposed forbidden marker {forbidden}"
        );
        assert!(
            !script.contains(forbidden),
            "release-candidate recovery script exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "release-candidate-windows-recovery",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &sha256_path(&package_archive),
        "--distribution-rehearsal",
        distribution_rehearsal.to_str().unwrap(),
        "--distribution-rehearsal-sha256",
        &sha256_path(&distribution_rehearsal),
        "--adapter-observer-bundle",
        adapter_bundle.to_str().unwrap(),
        "--adapter-observer-bundle-sha256",
        &sha256_path(&adapter_bundle),
        "--adapter-observer-archive",
        adapter_archive.to_str().unwrap(),
        "--adapter-observer-archive-sha256",
        &sha256_path(&adapter_archive),
        "--adapter-install-smoke-observer-bundle",
        adapter_install_smoke_bundle.to_str().unwrap(),
        "--adapter-install-smoke-observer-bundle-sha256",
        &sha256_path(&adapter_install_smoke_bundle),
        "--adapter-install-smoke-observer-archive",
        adapter_install_smoke_archive.to_str().unwrap(),
        "--adapter-install-smoke-observer-archive-sha256",
        &sha256_path(&adapter_install_smoke_archive),
        "--consumer-lifecycle-observer-bundle",
        consumer_lifecycle_bundle.to_str().unwrap(),
        "--consumer-lifecycle-observer-bundle-sha256",
        &sha256_path(&consumer_lifecycle_bundle),
        "--consumer-lifecycle-observer-archive",
        consumer_lifecycle_archive.to_str().unwrap(),
        "--consumer-lifecycle-observer-archive-sha256",
        &sha256_path(&consumer_lifecycle_archive),
        "--release-gate-with-replacement-observer-bundle",
        release_gate_observer_bundle.to_str().unwrap(),
        "--release-gate-with-replacement-observer-bundle-sha256",
        &sha256_path(&release_gate_observer_bundle),
        "--release-gate-with-replacement-observer-archive",
        release_gate_observer_archive.to_str().unwrap(),
        "--release-gate-with-replacement-observer-archive-sha256",
        &sha256_path(&release_gate_observer_archive),
        "--control-plane-fixture-handoff-verification",
        control_plane_handoff_verification.to_str().unwrap(),
        "--control-plane-fixture-handoff-verification-sha256",
        &sha256_path(&control_plane_handoff_verification),
        "--control-plane-readback-commit",
        "a54a60f",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("package summary sha256 mismatch"));
}

#[test]
fn cli_plugin_release_candidate_windows_recovery_verify_accepts_digest_pinned_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let recovery_dir = temp.path().join("release-candidate-windows-recovery");
    fs::create_dir_all(&recovery_dir).unwrap();

    let script_path = recovery_dir.join("run-release-candidate.ps1");
    fs::write(
        &script_path,
        r#"param(
    [string]$Ao2 = "ao2",
    [string]$OutDir = (Join-Path $PSScriptRoot "release-candidate")
)

$ErrorActionPreference = "Stop"
$InputRoot = Join-Path $PSScriptRoot "inputs"
& $Ao2 plugin release-candidate `
    --out-dir $OutDir `
    --json
& $Ao2 plugin release-candidate-verify `
    --summary (Join-Path $OutDir "plugin-release-candidate.json") `
    --summary-sha256 "1111111111111111111111111111111111111111111111111111111111111111" `
    --json
"#,
    )
    .unwrap();

    let recovery_path = recovery_dir.join("windows-release-candidate-recovery.json");
    let recovery = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-windows-recovery.v1",
        "status": "ready_for_windows_execution",
        "platform": "windows",
        "manifest_path": recovery_path.display().to_string(),
        "script_path": script_path.display().to_string(),
        "script_sha256": sha256_path(&script_path),
        "execution": {
            "runner": "powershell",
            "single_session_command": "powershell -ExecutionPolicy Bypass -File .\\run-release-candidate.ps1",
            "ao2_argument": "-Ao2 <path-to-ao2.exe-or-ao2>",
            "output_argument": "-OutDir <windows-output-dir>",
            "produces": "ao2.plugin-release-candidate-verification.v1"
        },
        "release_review_inputs": {
            "package": {
                "summary_path": "inputs/ao2-plugin-package.json",
                "summary_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
                "archive_path": "inputs/ao2-plugin-package.tar.gz",
                "archive_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
            },
            "distribution_rehearsal": {
                "path": "inputs/plugin-distribution-rehearsal.json",
                "sha256": "4444444444444444444444444444444444444444444444444444444444444444"
            },
            "adapter_observer_bundle": {
                "summary_path": "inputs/k37-plugin-adapter-observer-bundle.json",
                "summary_sha256": "5555555555555555555555555555555555555555555555555555555555555555",
                "archive_path": "inputs/k37-plugin-adapter-observer-bundle.tar.gz",
                "archive_sha256": "6666666666666666666666666666666666666666666666666666666666666666"
            },
            "adapter_install_smoke_observer_bundle": {
                "summary_path": "inputs/k37-plugin-adapter-install-smoke-observer-bundle.json",
                "summary_sha256": "7777777777777777777777777777777777777777777777777777777777777777",
                "archive_path": "inputs/k37-plugin-adapter-install-smoke-observer-bundle.tar.gz",
                "archive_sha256": "8888888888888888888888888888888888888888888888888888888888888888"
            },
            "consumer_lifecycle_observer_bundle": {
                "summary_path": "inputs/k37-plugin-consumer-lifecycle-observer-bundle.json",
                "summary_sha256": "9999999999999999999999999999999999999999999999999999999999999999",
                "archive_path": "inputs/k37-plugin-consumer-lifecycle-observer-bundle.tar.gz",
                "archive_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
            "release_gate_with_replacement_observer_bundle": {
                "summary_path": "inputs/k37-release-gate-with-replacement-observer-bundle.json",
                "summary_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "archive_path": "inputs/k37-release-gate-with-replacement-observer-bundle.tar.gz",
                "archive_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
            },
            "control_plane_fixture_handoff_verification": {
                "path": "inputs/control-plane-fixture-handoff-verification.json",
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }
        },
        "control_plane_readback": {
            "repo": "ao2-control-plane",
            "commit": "a54a60f",
            "role": "read_only_observer",
            "mutated_by_this_command": false,
            "approves_release": false
        },
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "provider_api_key_env_required": false
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
        },
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "side_effects": {
            "provider_execution_started": false,
            "queue_mutated": false,
            "memory_written": false,
            "control_plane_mutated": false,
            "ao_artifacts_mutated": false,
            "release_approved": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    fs::write(
        &recovery_path,
        serde_json::to_string_pretty(&recovery).unwrap(),
    )
    .unwrap();
    let recovery_sha256 = sha256_path(&recovery_path);
    let out_path = temp
        .path()
        .join("release-candidate-windows-recovery-verification.json");

    let verification = ao2([
        "plugin",
        "release-candidate-windows-recovery-verify",
        "--recovery",
        recovery_path.to_str().unwrap(),
        "--recovery-sha256",
        &recovery_sha256,
        "--out",
        out_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(verification.status.success(), "{}", stderr(&verification));

    let json: serde_json::Value = serde_json::from_str(&stdout(&verification)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-release-candidate-windows-recovery-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["recovery_sha256"], recovery_sha256);
    assert_eq!(json["script_sha256"], sha256_path(&script_path));
    assert_eq!(json["platform"], "windows");
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["provider_execution_started"], false);
    assert_eq!(json["side_effects"]["queue_mutated"], false);
    assert_eq!(json["side_effects"]["memory_written"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["side_effects"]["ao_artifacts_mutated"], false);
    assert_eq!(json["side_effects"]["release_approved"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");
    assert!(out_path.is_file());

    let bad_digest = ao2([
        "plugin",
        "release-candidate-windows-recovery-verify",
        "--recovery",
        recovery_path.to_str().unwrap(),
        "--recovery-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--out",
        out_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("release-candidate Windows recovery sha256 mismatch"));
}

#[test]
fn cli_plugin_release_candidate_windows_transfer_bundle_packages_verified_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let source_archive = temp.path().join("ao2-source.tar.gz");
    fs::write(&source_archive, "token-safe ao2 source archive").unwrap();

    let recovery_dir = temp.path().join("release-candidate-windows-recovery");
    fs::create_dir_all(&recovery_dir).unwrap();
    let script_path = recovery_dir.join("run-release-candidate.ps1");
    fs::write(
        &script_path,
        r#"param(
    [string]$Ao2 = "ao2",
    [string]$OutDir = (Join-Path $PSScriptRoot "release-candidate")
)

$ErrorActionPreference = "Stop"
$InputRoot = Join-Path $PSScriptRoot "inputs"
& $Ao2 plugin release-candidate `
    --out-dir $OutDir `
    --json
& $Ao2 plugin release-candidate-verify `
    --summary (Join-Path $OutDir "plugin-release-candidate.json") `
    --summary-sha256 "1111111111111111111111111111111111111111111111111111111111111111" `
    --json
"#,
    )
    .unwrap();
    let recovery_path = recovery_dir.join("windows-release-candidate-recovery.json");
    let recovery = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-windows-recovery.v1",
        "status": "ready_for_windows_execution",
        "platform": "windows",
        "manifest_path": recovery_path.display().to_string(),
        "script_path": script_path.display().to_string(),
        "script_sha256": sha256_path(&script_path),
        "execution": {
            "runner": "powershell",
            "single_session_command": "powershell -ExecutionPolicy Bypass -File .\\run-release-candidate.ps1",
            "ao2_argument": "-Ao2 <path-to-ao2.exe-or-ao2>",
            "output_argument": "-OutDir <windows-output-dir>",
            "produces": "ao2.plugin-release-candidate-verification.v1"
        },
        "release_review_inputs": {
            "package": {
                "summary_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
                "archive_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
            },
            "distribution_rehearsal": {
                "sha256": "4444444444444444444444444444444444444444444444444444444444444444"
            },
            "adapter_observer_bundle": {
                "summary_sha256": "5555555555555555555555555555555555555555555555555555555555555555",
                "archive_sha256": "6666666666666666666666666666666666666666666666666666666666666666"
            },
            "adapter_install_smoke_observer_bundle": {
                "summary_sha256": "7777777777777777777777777777777777777777777777777777777777777777",
                "archive_sha256": "8888888888888888888888888888888888888888888888888888888888888888"
            },
            "consumer_lifecycle_observer_bundle": {
                "summary_sha256": "9999999999999999999999999999999999999999999999999999999999999999",
                "archive_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
            "release_gate_with_replacement_observer_bundle": {
                "summary_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "archive_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
            },
            "control_plane_fixture_handoff_verification": {
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }
        },
        "control_plane_readback": {
            "repo": "ao2-control-plane",
            "commit": "a54a60f",
            "role": "read_only_observer",
            "mutated_by_this_command": false,
            "approves_release": false
        },
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "provider_api_key_env_required": false
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false
        },
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "side_effects": {
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_write_memory": false,
            "would_mutate_control_plane": false,
            "would_mutate_ao_artifacts": false,
            "would_approve_release": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    fs::write(
        &recovery_path,
        serde_json::to_string_pretty(&recovery).unwrap(),
    )
    .unwrap();

    let verification_path = temp
        .path()
        .join("release-candidate-windows-recovery-verification.json");
    let recovery_sha256 = sha256_path(&recovery_path);
    let verification = serde_json::json!({
        "schema_version": "ao2.plugin-release-candidate-windows-recovery-verification.v1",
        "status": "passed",
        "recovery_path": recovery_path.display().to_string(),
        "recovery_sha256": recovery_sha256,
        "source_schema_version": "ao2.plugin-release-candidate-windows-recovery.v1",
        "platform": "windows",
        "script_path": script_path.display().to_string(),
        "script_sha256": sha256_path(&script_path),
        "execution": recovery["execution"].clone(),
        "release_review_inputs": recovery["release_review_inputs"].clone(),
        "control_plane_readback": recovery["control_plane_readback"].clone(),
        "provider_auth": recovery["provider_auth"].clone(),
        "trust_boundary": recovery["trust_boundary"].clone(),
        "control_plane_observation": recovery["control_plane_observation"].clone(),
        "side_effects": recovery["side_effects"].clone(),
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    fs::write(
        &verification_path,
        serde_json::to_string_pretty(&verification).unwrap(),
    )
    .unwrap();

    let out_dir = temp
        .path()
        .join("release-candidate-windows-transfer-bundle");
    let output = ao2([
        "plugin",
        "release-candidate-windows-transfer-bundle",
        "--ao2-source-archive",
        source_archive.to_str().unwrap(),
        "--ao2-source-archive-sha256",
        &sha256_path(&source_archive),
        "--recovery-dir",
        recovery_dir.to_str().unwrap(),
        "--recovery",
        recovery_path.to_str().unwrap(),
        "--recovery-sha256",
        &sha256_path(&recovery_path),
        "--recovery-verification",
        verification_path.to_str().unwrap(),
        "--recovery-verification-sha256",
        &sha256_path(&verification_path),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-release-candidate-windows-transfer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_windows_transfer");
    assert_eq!(json["platform"], "windows");
    assert_eq!(
        json["transfer_inputs"]["ao2_source_archive"]["sha256"],
        sha256_path(&source_archive)
    );
    assert_eq!(
        json["transfer_inputs"]["recovery"]["sha256"],
        sha256_path(&recovery_path)
    );
    assert_eq!(
        json["transfer_inputs"]["recovery_verification"]["sha256"],
        sha256_path(&verification_path)
    );
    assert_eq!(
        json["execution"]["single_session_command"],
        "powershell -ExecutionPolicy Bypass -File .\\run-release-candidate.ps1"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["provider_execution_started"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));
    assert!(out_dir.join("transfer").join("ao2-source.tar.gz").is_file());
    assert!(out_dir
        .join("transfer")
        .join("recovery")
        .join("run-release-candidate.ps1")
        .is_file());
    assert!(out_dir
        .join("transfer")
        .join("release-candidate-windows-recovery-verification.json")
        .is_file());

    let bad_digest = ao2([
        "plugin",
        "release-candidate-windows-transfer-bundle",
        "--ao2-source-archive",
        source_archive.to_str().unwrap(),
        "--ao2-source-archive-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--recovery-dir",
        recovery_dir.to_str().unwrap(),
        "--recovery",
        recovery_path.to_str().unwrap(),
        "--recovery-sha256",
        &sha256_path(&recovery_path),
        "--recovery-verification",
        verification_path.to_str().unwrap(),
        "--recovery-verification-sha256",
        &sha256_path(&verification_path),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("AO2 source archive sha256 mismatch"));
}

#[test]
fn cli_plugin_release_candidate_observer_bundle_packages_three_platform_release_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let mut verification_paths = Vec::new();
    let mut verification_shas = Vec::new();

    for (platform, commit) in [
        ("macos", "a54a60f"),
        ("ubuntu", "a54a60f"),
        ("windows", "a54a60f"),
    ] {
        let verification_path = temp
            .path()
            .join(platform)
            .join("plugin-release-candidate-verification.json");
        fs::create_dir_all(verification_path.parent().unwrap()).unwrap();
        let verification = serde_json::json!({
            "schema_version": "ao2.plugin-release-candidate-verification.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-release-candidate.json"),
            "summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "source_schema_version": "ao2.plugin-release-candidate.v1",
            "release_review_inputs": [
                "ao2.plugin-package.v1",
                "ao2.plugin-distribution-rehearsal.v1",
                "ao2.k37-plugin-adapter-observer-bundle.v1",
                "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
                "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
                "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
                "ao2.control-plane-fixture-handoff-verification.v1"
            ],
            "evidence_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "control_plane_readback": {
                "repo": "ao2-control-plane",
                "commit": commit,
                "role": "read_only_observer",
                "mutated_by_this_command": false,
                "approves_release": false
            },
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            },
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            },
            "side_effects": {
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_write_memory": false,
                "would_mutate_control_plane": false,
                "would_mutate_ao_artifacts": false,
                "would_approve_release": false
            },
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        });
        fs::write(
            &verification_path,
            serde_json::to_string_pretty(&verification).unwrap(),
        )
        .unwrap();
        verification_shas.push(sha256_path(&verification_path));
        verification_paths.push(verification_path);
    }

    let out_dir = temp.path().join("release-candidate-observer-bundle");
    let bundle = ao2([
        "plugin",
        "release-candidate-observer-bundle",
        "--macos-verification",
        verification_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &verification_shas[0],
        "--ubuntu-verification",
        verification_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &verification_shas[1],
        "--windows-verification",
        verification_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &verification_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));

    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.k37-plugin-release-candidate-observer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["observed_evidence_scope"],
        serde_json::json!([
            "ao2.plugin-release-candidate.v1",
            "ao2.plugin-release-candidate-verification.v1"
        ])
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        assert_eq!(
            json["platform_release_candidates"][*platform]["sha256"],
            verification_shas[idx]
        );
        assert_eq!(
            json["platform_release_candidates"][*platform]["schema_version"],
            "ao2.plugin-release-candidate-verification.v1"
        );
        assert_eq!(
            json["platform_release_candidates"][*platform]["status"],
            "passed"
        );
        assert_eq!(
            json["platform_release_candidates"][*platform]["provider_auth"]["local_oauth_cli_only"],
            true
        );
        assert_eq!(
            json["platform_release_candidates"][*platform]["provider_auth"]
                ["provider_api_key_auth_allowed"],
            false
        );
        assert_eq!(
            json["platform_release_candidates"][*platform]["side_effects"]
                ["would_execute_provider"],
            false
        );
        assert_eq!(
            json["platform_release_candidates"][*platform]["token_safe_output_verified"],
            true
        );
        assert!(Path::new(
            json["platform_release_candidates"][*platform]["bundled_path"]
                .as_str()
                .unwrap()
        )
        .is_file());
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "release-candidate-observer-bundle",
        "--macos-verification",
        verification_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-verification",
        verification_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &verification_shas[1],
        "--windows-verification",
        verification_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &verification_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos release-candidate verification sha256 mismatch"));
}

#[test]
fn cli_plugin_release_candidate_observer_bundle_verify_validates_distributed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let mut verification_paths = Vec::new();
    let mut verification_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let verification_path = temp
            .path()
            .join(platform)
            .join("plugin-release-candidate-verification.json");
        fs::create_dir_all(verification_path.parent().unwrap()).unwrap();
        let verification = serde_json::json!({
            "schema_version": "ao2.plugin-release-candidate-verification.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-release-candidate.json"),
            "summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "source_schema_version": "ao2.plugin-release-candidate.v1",
            "release_review_inputs": [
                "ao2.plugin-package.v1",
                "ao2.plugin-distribution-rehearsal.v1",
                "ao2.k37-plugin-adapter-observer-bundle.v1",
                "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
                "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
                "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
                "ao2.control-plane-fixture-handoff-verification.v1"
            ],
            "evidence_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "control_plane_readback": {
                "repo": "ao2-control-plane",
                "commit": "a54a60f",
                "role": "read_only_observer",
                "mutated_by_this_command": false,
                "approves_release": false
            },
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            },
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            },
            "side_effects": {
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_write_memory": false,
                "would_mutate_control_plane": false,
                "would_mutate_ao_artifacts": false,
                "would_approve_release": false
            },
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        });
        fs::write(
            &verification_path,
            serde_json::to_string_pretty(&verification).unwrap(),
        )
        .unwrap();
        verification_shas.push(sha256_path(&verification_path));
        verification_paths.push(verification_path);
    }

    let bundle_dir = temp.path().join("release-candidate-observer-bundle");
    let bundle = ao2([
        "plugin",
        "release-candidate-observer-bundle",
        "--macos-verification",
        verification_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &verification_shas[0],
        "--ubuntu-verification",
        verification_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &verification_shas[1],
        "--windows-verification",
        verification_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &verification_shas[2],
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = bundle_json["summary_path"].as_str().unwrap();
    let archive_path = bundle_json["archive_path"].as_str().unwrap();
    let summary_sha256 = bundle_json["summary_sha256"].as_str().unwrap();
    let archive_sha256 = bundle_json["archive_sha256"].as_str().unwrap();

    let verify = ao2([
        "plugin",
        "release-candidate-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        summary_sha256,
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.k37-plugin-release-candidate-observer-bundle-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["archive_sha256"], archive_sha256);
    assert_eq!(verify_json["platform_count"], 3);
    assert_eq!(
        verify_json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        verify_json["observed_evidence_scope"],
        serde_json::json!([
            "ao2.plugin-release-candidate.v1",
            "ao2.plugin-release-candidate-verification.v1"
        ])
    );
    assert_eq!(verify_json["archive_contents_verified"], true);
    assert_eq!(verify_json["platform_release_candidates_verified"], true);
    assert_eq!(
        verify_json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(verify_json["side_effects"]["would_approve_release"], false);
    assert_eq!(verify_json["factory_v3_role"], "parity_auditor");

    let bad_digest = ao2([
        "plugin",
        "release-candidate-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(
        stderr(&bad_digest).contains("release-candidate observer bundle summary sha256 mismatch")
    );
}

#[test]
fn cli_plugin_release_candidate_control_plane_fixture_handoff_prepares_read_only_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("release-candidate-proofs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut proof_paths = Vec::new();
    let mut proof_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = inputs_dir.join(format!(
            "{platform}-plugin-release-candidate-verification.json"
        ));
        let proof = serde_json::json!({
            "schema_version": "ao2.plugin-release-candidate-verification.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-release-candidate.json"),
            "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
            "source_schema_version": "ao2.plugin-release-candidate.v1",
            "evidence_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
            "release_review_inputs": [
                "ao2.plugin-package.v1",
                "ao2.plugin-distribution-rehearsal.v1",
                "ao2.k37-plugin-adapter-observer-bundle.v1",
                "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
                "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
                "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
                "ao2.control-plane-fixture-handoff-verification.v1"
            ],
            "control_plane_readback": {
                "repo": "ao2-control-plane",
                "commit": "a54a60f",
                "role": "read_only_observer",
                "mutated_by_this_command": false,
                "approves_release": false
            },
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            },
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            },
            "side_effects": {
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_write_memory": false,
                "would_mutate_control_plane": false,
                "would_mutate_ao_artifacts": false,
                "would_approve_release": false
            },
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        });
        fs::write(&path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
        proof_shas.push(sha256_path(&path));
        proof_paths.push(path);
    }

    let bundle_dir = temp.path().join("release-candidate-observer-bundle");
    let bundle = ao2([
        "plugin",
        "release-candidate-observer-bundle",
        "--macos-verification",
        proof_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &proof_shas[0],
        "--ubuntu-verification",
        proof_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &proof_shas[1],
        "--windows-verification",
        proof_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &proof_shas[2],
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = Path::new(bundle_json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(bundle_json["archive_path"].as_str().unwrap());
    let summary_sha256 = sha256_path(summary_path);
    let archive_sha256 = sha256_path(archive_path);

    let out_dir = temp
        .path()
        .join("release-candidate-control-plane-fixture-handoff");
    let handoff = ao2([
        "plugin",
        "release-candidate-control-plane-fixture-handoff",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        &archive_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(handoff.status.success(), "{}", stderr(&handoff));
    let json: serde_json::Value = serde_json::from_str(&stdout(&handoff)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.release-candidate-control-plane-fixture-handoff.v1"
    );
    assert_eq!(json["status"], "ready_for_control_plane_readback");
    assert_eq!(
        json["source_schema_version"],
        "ao2.k37-plugin-release-candidate-observer-bundle.v1"
    );
    assert_eq!(
        json["recommended_control_plane_fixture_path"],
        "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/release-candidate-observer-bundle.json"
    );
    assert_eq!(
        json["recommended_control_plane_test_name"],
        "release_candidate_observer_bundle_is_read_only_three_platform_evidence"
    );
    assert_eq!(
        json["expected_observed_evidence_scope"],
        serde_json::json!([
            "ao2.plugin-release-candidate.v1",
            "ao2.plugin-release-candidate-verification.v1"
        ])
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let fixture_path = Path::new(json["fixture"]["path"].as_str().unwrap());
    assert!(fixture_path.is_file());
    assert_eq!(json["fixture"]["sha256"], sha256_path(fixture_path));
    assert_eq!(
        fs::read_to_string(fixture_path).unwrap(),
        fs::read_to_string(summary_path).unwrap()
    );

    let bad_digest = ao2([
        "plugin",
        "release-candidate-control-plane-fixture-handoff",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        &archive_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest)
        .contains("release-candidate control-plane fixture summary sha256 mismatch"));
}

#[test]
fn cli_plugin_release_candidate_control_plane_fixture_handoff_verify_accepts_digest_pinned_handoff()
{
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("release-candidate-proofs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut proof_paths = Vec::new();
    let mut proof_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = inputs_dir.join(format!(
            "{platform}-plugin-release-candidate-verification.json"
        ));
        let proof = serde_json::json!({
            "schema_version": "ao2.plugin-release-candidate-verification.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-release-candidate.json"),
            "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
            "source_schema_version": "ao2.plugin-release-candidate.v1",
            "evidence_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
            "release_review_inputs": [
                "ao2.plugin-package.v1",
                "ao2.plugin-distribution-rehearsal.v1",
                "ao2.k37-plugin-adapter-observer-bundle.v1",
                "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
                "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
                "ao2.k37-release-gate-with-replacement-observer-bundle.v1",
                "ao2.control-plane-fixture-handoff-verification.v1"
            ],
            "control_plane_readback": {
                "repo": "ao2-control-plane",
                "commit": "a54a60f",
                "role": "read_only_observer",
                "mutated_by_this_command": false,
                "approves_release": false
            },
            "provider_auth": {
                "local_oauth_cli_only": true,
                "provider_api_key_auth_allowed": false,
                "provider_api_key_env_required": false
            },
            "trust_boundary": {
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            },
            "control_plane_observation": {
                "role": "read_only_observer",
                "may_observe_evidence_bundle_path": true,
                "may_mutate_evidence": false,
                "may_approve_release": false
            },
            "side_effects": {
                "would_execute_provider": false,
                "would_execute_queue": false,
                "would_write_memory": false,
                "would_mutate_control_plane": false,
                "would_mutate_ao_artifacts": false,
                "would_approve_release": false
            },
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        });
        fs::write(&path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
        proof_shas.push(sha256_path(&path));
        proof_paths.push(path);
    }

    let bundle_dir = temp.path().join("release-candidate-observer-bundle");
    let bundle = ao2([
        "plugin",
        "release-candidate-observer-bundle",
        "--macos-verification",
        proof_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &proof_shas[0],
        "--ubuntu-verification",
        proof_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &proof_shas[1],
        "--windows-verification",
        proof_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &proof_shas[2],
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = Path::new(bundle_json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(bundle_json["archive_path"].as_str().unwrap());
    let summary_sha256 = sha256_path(summary_path);
    let archive_sha256 = sha256_path(archive_path);

    let out_dir = temp
        .path()
        .join("release-candidate-control-plane-fixture-handoff");
    let handoff = ao2([
        "plugin",
        "release-candidate-control-plane-fixture-handoff",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        &archive_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(handoff.status.success(), "{}", stderr(&handoff));
    let handoff_json: serde_json::Value = serde_json::from_str(&stdout(&handoff)).unwrap();
    let handoff_path = Path::new(handoff_json["handoff_path"].as_str().unwrap());
    let handoff_sha256 = sha256_path(handoff_path);

    let verification_path = temp
        .path()
        .join("release-candidate-control-plane-fixture-handoff-verification.json");
    let verify = ao2([
        "plugin",
        "release-candidate-control-plane-fixture-handoff-verify",
        "--handoff",
        handoff_path.to_str().unwrap(),
        "--handoff-sha256",
        &handoff_sha256,
        "--out",
        verification_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.release-candidate-control-plane-fixture-handoff-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["handoff_sha256"], handoff_sha256);
    assert_eq!(
        json["source_schema_version"],
        "ao2.k37-plugin-release-candidate-observer-bundle.v1"
    );
    assert_eq!(
        json["recommended_control_plane_fixture_path"],
        "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/release-candidate-observer-bundle.json"
    );
    assert_eq!(
        json["recommended_control_plane_test_name"],
        "release_candidate_observer_bundle_is_read_only_three_platform_evidence"
    );
    assert_eq!(
        json["expected_observed_evidence_scope"],
        serde_json::json!([
            "ao2.plugin-release-candidate.v1",
            "ao2.plugin-release-candidate-verification.v1"
        ])
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");
    assert_eq!(
        json["fixture"]["sha256"],
        sha256_path(Path::new(handoff_json["fixture"]["path"].as_str().unwrap()))
    );
    assert!(verification_path.is_file());
    assert_eq!(
        json["verification_path"],
        verification_path.display().to_string()
    );
    assert_eq!(json["verification_sha256"], sha256_path(&verification_path));

    let bad_digest = ao2([
        "plugin",
        "release-candidate-control-plane-fixture-handoff-verify",
        "--handoff",
        handoff_path.to_str().unwrap(),
        "--handoff-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--out",
        verification_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest)
        .contains("release-candidate control-plane fixture handoff sha256 mismatch"));
}
