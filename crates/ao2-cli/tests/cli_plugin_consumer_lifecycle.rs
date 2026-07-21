use std::fs;
use std::path::{Path, PathBuf};
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

fn prepare_plugin_package_and_adapter_scaffold(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let manifest_dir = root.join("plugin-manifest");
    let package_manifest = ao2([
        "plugin",
        "manifest",
        "--out-dir",
        manifest_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        package_manifest.status.success(),
        "{}",
        stderr(&package_manifest)
    );

    let manifest_path = manifest_dir.join("ao2-plugin-manifest.json");
    let manifest_sha256 = sha256_path(&manifest_path);
    let verify = ao2([
        "plugin",
        "manifest-verify",
        "--manifest-dir",
        manifest_dir.to_str().unwrap(),
        "--manifest-sha256",
        &manifest_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verification_path = manifest_dir.join("manifest-verification.json");
    fs::write(&verification_path, stdout(&verify)).unwrap();
    let verification_sha256 = sha256_path(&verification_path);

    let install_smoke_path = manifest_dir.join("install-smoke.json");
    let install_smoke = ao2([
        "plugin",
        "install-smoke",
        "--manifest-dir",
        manifest_dir.to_str().unwrap(),
        "--verification",
        verification_path.to_str().unwrap(),
        "--verification-sha256",
        &verification_sha256,
        "--out",
        install_smoke_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(install_smoke.status.success(), "{}", stderr(&install_smoke));
    let install_smoke_sha256 = sha256_path(&install_smoke_path);

    let package_dir = root.join("plugin-package");
    let package = ao2([
        "plugin",
        "package",
        "--manifest-dir",
        manifest_dir.to_str().unwrap(),
        "--manifest-verification",
        verification_path.to_str().unwrap(),
        "--manifest-verification-sha256",
        &verification_sha256,
        "--install-smoke",
        install_smoke_path.to_str().unwrap(),
        "--install-smoke-sha256",
        &install_smoke_sha256,
        "--out-dir",
        package_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_summary = package_dir.join("ao2-plugin-package.json");
    let package_archive = package_dir.join("ao2-plugin-package.tar.gz");
    let package_summary_sha256 = sha256_path(&package_summary);
    let package_archive_sha256 = sha256_path(&package_archive);

    let observer_inputs_dir = root.join("observer-inputs");
    fs::create_dir_all(&observer_inputs_dir).unwrap();
    let mut input_paths = Vec::new();
    let mut input_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = observer_inputs_dir.join(format!("{platform}-k37-plugin-observer-input.json"));
        let input = serde_json::json!({
            "schema_version": "ao2.k37-plugin-observer-input.v1",
            "status": "ready_for_k37_observation",
            "producer": "ao2",
            "work_source": "codex-cron AO2 production/plugin readiness",
            "package_summary_path": package_summary.display().to_string(),
            "package_summary_sha256": package_summary_sha256,
            "package_archive_path": package_archive.display().to_string(),
            "package_archive_sha256": package_archive_sha256,
            "target_results": {
                "codex": {"status": "passed"},
                "claude": {"status": "passed"}
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
            "factory_v3_role": "parity_auditor"
        });
        fs::write(&path, serde_json::to_string_pretty(&input).unwrap()).unwrap();
        input_shas.push(sha256_path(&path));
        input_paths.push(path);
    }

    let k37_dir = root.join("k37-bundle");
    let k37_bundle = ao2([
        "plugin",
        "distribution-observer-bundle",
        "--macos-observer",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &input_shas[0],
        "--ubuntu-observer",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-observer",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        k37_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(k37_bundle.status.success(), "{}", stderr(&k37_bundle));
    let k37_summary = k37_dir.join("k37-plugin-observer-bundle.json");
    let k37_archive = k37_dir.join("k37-plugin-observer-bundle.tar.gz");
    let k37_summary_sha256 = sha256_path(&k37_summary);
    let k37_archive_sha256 = sha256_path(&k37_archive);

    let scaffold_dir = root.join("adapter-scaffold");
    let scaffold = ao2([
        "plugin",
        "adapter-scaffold",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        &package_summary_sha256,
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &package_archive_sha256,
        "--k37-bundle",
        k37_summary.to_str().unwrap(),
        "--k37-bundle-sha256",
        &k37_summary_sha256,
        "--k37-archive",
        k37_archive.to_str().unwrap(),
        "--k37-archive-sha256",
        &k37_archive_sha256,
        "--out-dir",
        scaffold_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(scaffold.status.success(), "{}", stderr(&scaffold));

    (
        package_summary,
        package_archive,
        scaffold_dir.join("plugin-adapter-scaffold.json"),
    )
}

#[test]
fn cli_plugin_consumer_lifecycle_verifies_installed_package_and_adapters_without_side_effects() {
    let temp = tempfile::tempdir().unwrap();
    let (package_summary, package_archive, adapter_summary) =
        prepare_plugin_package_and_adapter_scaffold(temp.path());
    let package_summary_sha256 = sha256_path(&package_summary);
    let package_archive_sha256 = sha256_path(&package_archive);
    let adapter_summary_sha256 = sha256_path(&adapter_summary);
    let out_dir = temp.path().join("plugin-consumer-lifecycle");

    let lifecycle = ao2([
        "plugin",
        "consumer-lifecycle",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        &package_summary_sha256,
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &package_archive_sha256,
        "--adapter-scaffold",
        adapter_summary.to_str().unwrap(),
        "--adapter-scaffold-sha256",
        &adapter_summary_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(lifecycle.status.success(), "{}", stderr(&lifecycle));

    let json: serde_json::Value = serde_json::from_str(&stdout(&lifecycle)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-consumer-lifecycle.v1");
    assert_eq!(json["status"], "passed");
    assert_eq!(json["targets"], serde_json::json!(["codex", "claude"]));
    assert_eq!(json["package"]["summary_sha256"], package_summary_sha256);
    assert_eq!(json["package"]["archive_sha256"], package_archive_sha256);
    assert_eq!(
        json["adapter_scaffold"]["summary_sha256"],
        adapter_summary_sha256
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
    assert_eq!(
        json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(json["factory_v3_role"], "parity_auditor");
    assert_eq!(json["side_effects"]["provider_execution_started"], false);
    assert_eq!(json["side_effects"]["queue_mutated"], false);
    assert_eq!(json["side_effects"]["memory_written"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["side_effects"]["ao_artifacts_mutated"], false);
    assert_eq!(json["side_effects"]["release_approved"], false);

    for target in ["codex", "claude"] {
        let target_json = &json["target_results"][target];
        assert_eq!(target_json["status"], "passed");
        assert_eq!(target_json["installed_package_paths_only"], true);
        assert_eq!(target_json["provider_execution_started"], false);
        assert_eq!(target_json["queue_mutated"], false);
        assert_eq!(target_json["memory_written"], false);
        assert_eq!(target_json["control_plane_mutated"], false);
        assert_eq!(target_json["ao_artifacts_mutated"], false);
        assert_eq!(target_json["release_approved"], false);
        for field in [
            "readiness_sha256",
            "manifest_verification_sha256",
            "install_smoke_sha256",
            "package_verification_sha256",
            "adapter_scaffold_verification_sha256",
            "adapter_install_smoke_verification_sha256",
            "wrapper_harness_verification_sha256",
        ] {
            assert_eq!(
                target_json[field].as_str().unwrap().len(),
                64,
                "{target} missing digest field {field}"
            );
        }
        assert!(Path::new(target_json["wrapper_sandbox_dir"].as_str().unwrap()).is_dir());
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&lifecycle).contains(forbidden),
            "consumer lifecycle output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(summary_path)
                .unwrap()
                .contains(forbidden),
            "consumer lifecycle summary exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "consumer-lifecycle",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &package_archive_sha256,
        "--adapter-scaffold",
        adapter_summary.to_str().unwrap(),
        "--adapter-scaffold-sha256",
        &adapter_summary_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin package summary sha256 mismatch"));
}

#[test]
fn cli_plugin_consumer_lifecycle_windows_recovery_writes_digest_pinned_runner() {
    let temp = tempfile::tempdir().unwrap();
    let (package_summary, package_archive, adapter_summary) =
        prepare_plugin_package_and_adapter_scaffold(temp.path());
    let package_summary_sha256 = sha256_path(&package_summary);
    let package_archive_sha256 = sha256_path(&package_archive);
    let adapter_summary_sha256 = sha256_path(&adapter_summary);
    let out_dir = temp.path().join("windows-consumer-lifecycle-recovery");

    let recovery = ao2([
        "plugin",
        "consumer-lifecycle-windows-recovery",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        &package_summary_sha256,
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &package_archive_sha256,
        "--adapter-scaffold",
        adapter_summary.to_str().unwrap(),
        "--adapter-scaffold-sha256",
        &adapter_summary_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(recovery.status.success(), "{}", stderr(&recovery));

    let json: serde_json::Value = serde_json::from_str(&stdout(&recovery)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-consumer-lifecycle-windows-recovery.v1"
    );
    assert_eq!(json["status"], "ready_for_windows_execution");
    assert_eq!(json["platform"], "windows");
    assert_eq!(json["package"]["summary_sha256"], package_summary_sha256);
    assert_eq!(json["package"]["archive_sha256"], package_archive_sha256);
    assert_eq!(
        json["adapter_scaffold"]["source_summary_sha256"],
        adapter_summary_sha256
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
        "plugin-adapter-scaffold.json",
    ] {
        assert!(out_dir.join("inputs").join(input_name).is_file());
    }
    let portable_adapter_summary = out_dir
        .join("inputs")
        .join("adapter-scaffold")
        .join("plugin-adapter-scaffold.json");
    assert!(portable_adapter_summary.is_file());
    let portable_adapter_summary_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&portable_adapter_summary).unwrap()).unwrap();
    let portable_adapter_summary_sha256 = sha256_path(&portable_adapter_summary);
    assert_eq!(
        json["adapter_scaffold"]["portable_summary_sha256"],
        portable_adapter_summary_sha256
    );
    for target in ["codex", "claude"] {
        let adapter_path = out_dir.join(
            portable_adapter_summary_json["adapter_files"][target]["path"]
                .as_str()
                .unwrap(),
        );
        assert!(adapter_path.is_file());
        assert_eq!(
            portable_adapter_summary_json["adapter_files"][target]["sha256"],
            sha256_path(&adapter_path)
        );
    }

    let script = fs::read_to_string(script_path).unwrap();
    assert!(script.contains("param("));
    assert!(script.contains("plugin"));
    assert!(script.contains("consumer-lifecycle"));
    assert!(script.contains(&package_summary_sha256));
    assert!(script.contains(&package_archive_sha256));
    assert!(script.contains(&portable_adapter_summary_sha256));
    assert!(script.contains("Join-Path $PSScriptRoot"));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&recovery).contains(forbidden),
            "windows recovery output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(manifest_path)
                .unwrap()
                .contains(forbidden),
            "windows recovery manifest exposed forbidden marker {forbidden}"
        );
        assert!(
            !script.contains(forbidden),
            "windows recovery script exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "consumer-lifecycle-windows-recovery",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &package_archive_sha256,
        "--adapter-scaffold",
        adapter_summary.to_str().unwrap(),
        "--adapter-scaffold-sha256",
        &adapter_summary_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin package summary sha256 mismatch"));
}

#[test]
fn cli_plugin_consumer_lifecycle_observer_bundle_packages_three_platform_lifecycle_proofs() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("consumer-lifecycle-proofs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut proof_paths = Vec::new();
    let mut proof_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = inputs_dir.join(format!("{platform}-plugin-consumer-lifecycle.json"));
        let proof = serde_json::json!({
            "schema_version": "ao2.plugin-consumer-lifecycle.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-consumer-lifecycle.json"),
            "targets": ["codex", "claude"],
            "package": {
                "summary_path": format!("target/{platform}/ao2-plugin-package.json"),
                "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "archive_path": format!("target/{platform}/ao2-plugin-package.tar.gz"),
                "archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222"
            },
            "adapter_scaffold": {
                "summary_path": format!("target/{platform}/plugin-adapter-scaffold.json"),
                "summary_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
            },
            "target_results": {
                "codex": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                },
                "claude": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                }
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
        fs::write(&path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
        proof_shas.push(sha256_path(&path));
        proof_paths.push(path);
    }

    let out_dir = temp.path().join("consumer-lifecycle-observer-bundle");
    let bundle = ao2([
        "plugin",
        "consumer-lifecycle-observer-bundle",
        "--macos-lifecycle",
        proof_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &proof_shas[0],
        "--ubuntu-lifecycle",
        proof_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &proof_shas[1],
        "--windows-lifecycle",
        proof_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &proof_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));

    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1"
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
        serde_json::json!(["ao2.plugin-consumer-lifecycle.v1"])
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
    assert_eq!(
        json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        json["control_plane_observation"]["may_mutate_evidence"],
        false
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_mutate_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        assert_eq!(
            json["platform_lifecycles"][*platform]["sha256"],
            proof_shas[idx]
        );
        assert_eq!(
            json["platform_lifecycles"][*platform]["schema_version"],
            "ao2.plugin-consumer-lifecycle.v1"
        );
        assert_eq!(json["platform_lifecycles"][*platform]["status"], "passed");
        assert_eq!(
            json["platform_lifecycles"][*platform]["targets"],
            serde_json::json!(["codex", "claude"])
        );
        assert!(Path::new(
            json["platform_lifecycles"][*platform]["bundled_path"]
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

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&bundle).contains(forbidden),
            "consumer lifecycle observer bundle output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(summary_path)
                .unwrap()
                .contains(forbidden),
            "consumer lifecycle observer bundle summary exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "consumer-lifecycle-observer-bundle",
        "--macos-lifecycle",
        proof_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-lifecycle",
        proof_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &proof_shas[1],
        "--windows-lifecycle",
        proof_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &proof_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos consumer lifecycle sha256 mismatch"));
}

#[test]
fn cli_plugin_consumer_lifecycle_observer_bundle_verify_accepts_digest_pinned_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("consumer-lifecycle-proofs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut proof_paths = Vec::new();
    let mut proof_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = inputs_dir.join(format!("{platform}-plugin-consumer-lifecycle.json"));
        let proof = serde_json::json!({
            "schema_version": "ao2.plugin-consumer-lifecycle.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-consumer-lifecycle.json"),
            "targets": ["codex", "claude"],
            "package": {
                "summary_path": format!("target/{platform}/ao2-plugin-package.json"),
                "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "archive_path": format!("target/{platform}/ao2-plugin-package.tar.gz"),
                "archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222"
            },
            "adapter_scaffold": {
                "summary_path": format!("target/{platform}/plugin-adapter-scaffold.json"),
                "summary_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
            },
            "target_results": {
                "codex": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                },
                "claude": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                }
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
        fs::write(&path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
        proof_shas.push(sha256_path(&path));
        proof_paths.push(path);
    }

    let out_dir = temp.path().join("consumer-lifecycle-observer-bundle");
    let bundle = ao2([
        "plugin",
        "consumer-lifecycle-observer-bundle",
        "--macos-lifecycle",
        proof_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &proof_shas[0],
        "--ubuntu-lifecycle",
        proof_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &proof_shas[1],
        "--windows-lifecycle",
        proof_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &proof_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = Path::new(bundle_json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(bundle_json["archive_path"].as_str().unwrap());
    let summary_sha256 = sha256_path(summary_path);
    let archive_sha256 = sha256_path(archive_path);

    let verify = ao2([
        "plugin",
        "consumer-lifecycle-observer-bundle-verify",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        &archive_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.k37-plugin-consumer-lifecycle-observer-bundle-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["summary_sha256"], summary_sha256);
    assert_eq!(json["archive_sha256"], archive_sha256);
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(json["archive_contents_verified"], true);
    assert_eq!(json["bundled_lifecycles_verified"], true);
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let bad_archive_digest = ao2([
        "plugin",
        "consumer-lifecycle-observer-bundle-verify",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(!bad_archive_digest.status.success());
    assert!(stderr(&bad_archive_digest)
        .contains("consumer lifecycle observer bundle archive sha256 mismatch"));
}

#[test]
fn cli_plugin_control_plane_fixture_handoff_accepts_consumer_lifecycle_observer_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("consumer-lifecycle-proofs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut proof_paths = Vec::new();
    let mut proof_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = inputs_dir.join(format!("{platform}-plugin-consumer-lifecycle.json"));
        let proof = serde_json::json!({
            "schema_version": "ao2.plugin-consumer-lifecycle.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-consumer-lifecycle.json"),
            "targets": ["codex", "claude"],
            "package": {
                "summary_path": format!("target/{platform}/ao2-plugin-package.json"),
                "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "archive_path": format!("target/{platform}/ao2-plugin-package.tar.gz"),
                "archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222"
            },
            "adapter_scaffold": {
                "summary_path": format!("target/{platform}/plugin-adapter-scaffold.json"),
                "summary_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
            },
            "target_results": {
                "codex": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                },
                "claude": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                }
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
        fs::write(&path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
        proof_shas.push(sha256_path(&path));
        proof_paths.push(path);
    }

    let bundle_dir = temp.path().join("consumer-lifecycle-observer-bundle");
    let bundle = ao2([
        "plugin",
        "consumer-lifecycle-observer-bundle",
        "--macos-lifecycle",
        proof_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &proof_shas[0],
        "--ubuntu-lifecycle",
        proof_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &proof_shas[1],
        "--windows-lifecycle",
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

    let out_dir = temp.path().join("control-plane-fixture-handoff");
    let handoff = ao2([
        "plugin",
        "control-plane-fixture-handoff",
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
        "ao2.control-plane-fixture-handoff.v1"
    );
    assert_eq!(json["status"], "ready_for_control_plane_readback");
    assert_eq!(
        json["source_schema_version"],
        "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1"
    );
    assert_eq!(json["summary_sha256"], summary_sha256);
    assert_eq!(json["archive_sha256"], archive_sha256);
    assert_eq!(
        json["recommended_control_plane_fixture_path"],
        "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/consumer-lifecycle-observer-bundle.json"
    );
    assert_eq!(
        json["recommended_control_plane_test_name"],
        "consumer_lifecycle_observer_bundle_is_read_only_three_platform_evidence"
    );
    assert_eq!(
        json["expected_platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["expected_observed_evidence_scope"],
        serde_json::json!(["ao2.plugin-consumer-lifecycle.v1"])
    );
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let fixture_path = Path::new(json["fixture"]["path"].as_str().unwrap());
    assert!(fixture_path.is_file());
    assert_eq!(json["fixture"]["sha256"], sha256_path(fixture_path));
    assert_eq!(
        fs::read_to_string(fixture_path).unwrap(),
        fs::read_to_string(summary_path).unwrap()
    );

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&handoff).contains(forbidden),
            "control-plane fixture handoff output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(Path::new(json["handoff_path"].as_str().unwrap()))
                .unwrap()
                .contains(forbidden),
            "control-plane fixture handoff exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "control-plane-fixture-handoff",
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
    assert!(
        stderr(&bad_digest).contains("consumer lifecycle observer bundle summary sha256 mismatch")
    );
}

#[test]
fn cli_plugin_control_plane_fixture_handoff_verify_accepts_digest_pinned_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("consumer-lifecycle-proofs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut proof_paths = Vec::new();
    let mut proof_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = inputs_dir.join(format!("{platform}-plugin-consumer-lifecycle.json"));
        let proof = serde_json::json!({
            "schema_version": "ao2.plugin-consumer-lifecycle.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-consumer-lifecycle.json"),
            "targets": ["codex", "claude"],
            "package": {
                "summary_path": format!("target/{platform}/ao2-plugin-package.json"),
                "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "archive_path": format!("target/{platform}/ao2-plugin-package.tar.gz"),
                "archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222"
            },
            "adapter_scaffold": {
                "summary_path": format!("target/{platform}/plugin-adapter-scaffold.json"),
                "summary_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
            },
            "target_results": {
                "codex": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                },
                "claude": {
                    "status": "passed",
                    "installed_package_paths_only": true,
                    "provider_execution_started": false,
                    "queue_mutated": false,
                    "memory_written": false,
                    "control_plane_mutated": false,
                    "ao_artifacts_mutated": false,
                    "release_approved": false
                }
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
        fs::write(&path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
        proof_shas.push(sha256_path(&path));
        proof_paths.push(path);
    }

    let bundle_dir = temp.path().join("consumer-lifecycle-observer-bundle");
    let bundle = ao2([
        "plugin",
        "consumer-lifecycle-observer-bundle",
        "--macos-lifecycle",
        proof_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &proof_shas[0],
        "--ubuntu-lifecycle",
        proof_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &proof_shas[1],
        "--windows-lifecycle",
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

    let out_dir = temp.path().join("control-plane-fixture-handoff");
    let handoff = ao2([
        "plugin",
        "control-plane-fixture-handoff",
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
    assert_eq!(handoff_json["handoff_sha256"], handoff_sha256);

    let verification_path = temp
        .path()
        .join("control-plane-fixture-handoff-verification.json");
    let verify = ao2([
        "plugin",
        "control-plane-fixture-handoff-verify",
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
        "ao2.control-plane-fixture-handoff-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["handoff_sha256"], handoff_sha256);
    assert_eq!(
        json["source_schema_version"],
        "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1"
    );
    assert_eq!(
        json["recommended_control_plane_fixture_path"],
        "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/consumer-lifecycle-observer-bundle.json"
    );
    assert_eq!(
        json["recommended_control_plane_test_name"],
        "consumer_lifecycle_observer_bundle_is_read_only_three_platform_evidence"
    );
    assert_eq!(
        json["expected_platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
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
        "control-plane-fixture-handoff-verify",
        "--handoff",
        handoff_path.to_str().unwrap(),
        "--handoff-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--out",
        verification_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("control-plane fixture handoff sha256 mismatch"));
}
