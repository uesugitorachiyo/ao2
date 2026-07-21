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

#[test]
fn cli_plugin_adapter_scaffold_consumes_package_and_k37_bundle_for_codex_claude() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_dir = temp.path().join("plugin-manifest");

    let manifest = ao2([
        "plugin",
        "manifest",
        "--out-dir",
        manifest_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(manifest.status.success(), "{}", stderr(&manifest));

    let manifest_path = manifest_dir.join("ao2-plugin-manifest.json");
    let manifest_sha256 = sha256_path(&manifest_path);
    let manifest_verify = ao2([
        "plugin",
        "manifest-verify",
        "--manifest-dir",
        manifest_dir.to_str().unwrap(),
        "--manifest-sha256",
        &manifest_sha256,
        "--json",
    ]);
    assert!(
        manifest_verify.status.success(),
        "{}",
        stderr(&manifest_verify)
    );
    let manifest_verification_path = manifest_dir.join("manifest-verification.json");
    fs::write(&manifest_verification_path, stdout(&manifest_verify)).unwrap();
    let manifest_verification_sha256 = sha256_path(&manifest_verification_path);

    let install_smoke_path = manifest_dir.join("install-smoke.json");
    let install_smoke = ao2([
        "plugin",
        "install-smoke",
        "--manifest-dir",
        manifest_dir.to_str().unwrap(),
        "--verification",
        manifest_verification_path.to_str().unwrap(),
        "--verification-sha256",
        &manifest_verification_sha256,
        "--out",
        install_smoke_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(install_smoke.status.success(), "{}", stderr(&install_smoke));
    let install_smoke_sha256 = sha256_path(&install_smoke_path);

    let package_dir = temp.path().join("plugin-package");
    let package = ao2([
        "plugin",
        "package",
        "--manifest-dir",
        manifest_dir.to_str().unwrap(),
        "--manifest-verification",
        manifest_verification_path.to_str().unwrap(),
        "--manifest-verification-sha256",
        &manifest_verification_sha256,
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

    let observer_inputs_dir = temp.path().join("observer-inputs");
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

    let k37_dir = temp.path().join("k37-bundle");
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

    let scaffold_dir = temp.path().join("adapter-scaffold");
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

    let json: serde_json::Value = serde_json::from_str(&stdout(&scaffold)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-adapter-scaffold.v1");
    assert_eq!(json["status"], "ready_for_local_oauth_wrapper_integration");
    assert_eq!(json["targets"], serde_json::json!(["codex", "claude"]));
    assert_eq!(json["package"]["summary_sha256"], package_summary_sha256);
    assert_eq!(json["package"]["archive_sha256"], package_archive_sha256);
    assert_eq!(
        json["k37_observer_bundle"]["summary_sha256"],
        k37_summary_sha256
    );
    assert_eq!(
        json["k37_observer_bundle"]["archive_sha256"],
        k37_archive_sha256
    );
    assert_eq!(
        json["digest_gates"]["package_summary_sha256_verified"],
        true
    );
    assert_eq!(
        json["digest_gates"]["package_archive_sha256_verified"],
        true
    );
    assert_eq!(json["digest_gates"]["k37_bundle_sha256_verified"], true);
    assert_eq!(json["digest_gates"]["k37_archive_sha256_verified"], true);
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
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

    for target in ["codex", "claude"] {
        let adapter_path = Path::new(json["adapter_files"][target]["path"].as_str().unwrap());
        assert!(adapter_path.is_file(), "missing {target} adapter scaffold");
        assert_eq!(
            json["adapter_files"][target]["sha256"],
            sha256_path(adapter_path)
        );
        let adapter_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(adapter_path).unwrap()).unwrap();
        assert_eq!(adapter_json["schema_version"], "ao2.plugin-adapter.v1");
        assert_eq!(adapter_json["target"], target);
        assert_eq!(adapter_json["provider_auth"]["local_oauth_cli_only"], true);
        assert_eq!(
            adapter_json["inputs"]["package_summary_sha256"],
            package_summary_sha256
        );
        assert_eq!(
            adapter_json["inputs"]["k37_bundle_sha256"],
            k37_summary_sha256
        );
        assert_eq!(
            adapter_json["commands"]["readiness"],
            "ao2 plugin readiness --json"
        );
        assert!(adapter_json["commands"]["wrapper_harness"]
            .as_str()
            .unwrap()
            .contains("ao2 plugin wrapper-harness "));
        assert!(adapter_json["commands"]["closer_decision"]
            .as_str()
            .unwrap()
            .contains("ao2 factory closer-decision "));
        assert!(adapter_json["commands"]["closer_decision_verify"]
            .as_str()
            .unwrap()
            .contains("ao2 factory closer-decision-verify "));
    }

    let bad_digest = ao2([
        "plugin",
        "adapter-scaffold",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
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
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin package summary sha256 mismatch"));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&scaffold).contains(forbidden),
            "adapter scaffold exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_adapter_scaffold_verify_accepts_digest_pinned_scaffold() {
    let temp = tempfile::tempdir().unwrap();
    let scaffold_dir = temp.path().join("adapter-scaffold");
    let summary_path = write_plugin_adapter_scaffold_fixture(&scaffold_dir, false);
    let summary_sha256 = sha256_path(&summary_path);

    let verify = ao2([
        "plugin",
        "adapter-scaffold-verify",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));

    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-adapter-scaffold-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["summary_sha256"], summary_sha256);
    assert_eq!(json["targets"], serde_json::json!(["codex", "claude"]));
    assert_eq!(json["adapter_files_verified"], true);
    assert_eq!(json["digest_gates_verified"], true);
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
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
    assert_eq!(json["side_effects"]["provider_execution_started"], false);
    assert_eq!(json["side_effects"]["queue_mutated"], false);
    assert_eq!(json["side_effects"]["memory_written"], false);
    assert_eq!(json["side_effects"]["ao_artifacts_mutated"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["side_effects"]["release_approved"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let bad_digest = ao2([
        "plugin",
        "adapter-scaffold-verify",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin adapter scaffold summary sha256 mismatch"));

    let tampered_summary_path = write_plugin_adapter_scaffold_fixture(&scaffold_dir, true);
    let tampered_summary_sha256 = sha256_path(&tampered_summary_path);
    let tampered = ao2([
        "plugin",
        "adapter-scaffold-verify",
        "--summary",
        tampered_summary_path.to_str().unwrap(),
        "--summary-sha256",
        &tampered_summary_sha256,
        "--json",
    ]);
    assert!(!tampered.status.success());
    assert!(stderr(&tampered).contains("plugin adapter codex trust_boundary is not observer-only"));

    let forbidden_env = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "plugin",
            "adapter-scaffold-verify",
            "--summary",
            summary_path.to_str().unwrap(),
            "--summary-sha256",
            &summary_sha256,
            "--json",
        ])
        .env("OPENAI_API_KEY", "forbidden-test-value")
        .output()
        .unwrap();
    assert!(!forbidden_env.status.success());
    assert!(stderr(&forbidden_env).contains("OPENAI_API_KEY"));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&verify).contains(forbidden),
            "adapter scaffold verification exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_adapter_install_smoke_accepts_digest_pinned_scaffold() {
    let temp = tempfile::tempdir().unwrap();
    let scaffold_dir = temp.path().join("adapter-scaffold");
    let summary_path = write_plugin_adapter_scaffold_fixture(&scaffold_dir, false);
    let summary_sha256 = sha256_path(&summary_path);
    let smoke_path = temp.path().join("adapter-install-smoke.json");

    let smoke = ao2([
        "plugin",
        "adapter-install-smoke",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--out",
        smoke_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(smoke.status.success(), "{}", stderr(&smoke));

    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-adapter-install-smoke.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["summary_path"], summary_path.display().to_string());
    assert_eq!(json["summary_sha256"], summary_sha256);
    assert_eq!(json["targets"], serde_json::json!(["codex", "claude"]));
    assert_eq!(json["adapter_files_verified"], true);
    assert_eq!(json["digest_gates_verified"], true);
    assert_eq!(json["command_surface_verified"], true);
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
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
    assert_eq!(json["side_effects"]["ao_artifacts_mutated"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["side_effects"]["release_approved"], false);
    assert_eq!(json["token_safe_output_verified"], true);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    for target in ["codex", "claude"] {
        assert_eq!(json["target_results"][target]["status"], "passed");
        assert_eq!(
            json["target_results"][target]["adapter_schema_version"],
            "ao2.plugin-adapter.v1"
        );
        assert_eq!(
            json["target_results"][target]["commands_verified"],
            serde_json::json!([
                "readiness",
                "package_verify",
                "distribution_observer_bundle",
                "consumer_lifecycle_observer_bundle",
                "consumer_lifecycle_observer_bundle_verify",
                "control_plane_fixture_handoff",
                "control_plane_fixture_handoff_verify",
                "release_candidate",
                "release_candidate_verify",
                "release_candidate_windows_recovery",
                "release_candidate_windows_recovery_verify",
                "release_candidate_windows_transfer_bundle",
                "release_candidate_observer_bundle",
                "release_candidate_observer_bundle_verify",
                "release_candidate_control_plane_fixture_handoff",
                "release_candidate_control_plane_fixture_handoff_verify",
                "final_install_transcript",
                "final_install_transcript_observer_bundle",
                "closer_decision",
                "closer_decision_verify",
                "shipment_readiness",
                "adapter_install_smoke_verify",
                "adapter_install_smoke_observer_bundle",
                "wrapper_harness",
                "wrapper_harness_verify"
            ])
        );
    }
    assert!(smoke_path.is_file());
    assert_eq!(json["artifact_sha256"], sha256_path(&smoke_path));

    let bad_digest = ao2([
        "plugin",
        "adapter-install-smoke",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin adapter scaffold summary sha256 mismatch"));

    let forbidden_env = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "plugin",
            "adapter-install-smoke",
            "--summary",
            summary_path.to_str().unwrap(),
            "--summary-sha256",
            &summary_sha256,
            "--json",
        ])
        .env("ANTHROPIC_API_KEY", "forbidden-test-value")
        .output()
        .unwrap();
    assert!(!forbidden_env.status.success());
    assert!(stderr(&forbidden_env).contains("ANTHROPIC_API_KEY"));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&smoke).contains(forbidden),
            "adapter install smoke output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(&smoke_path).unwrap().contains(forbidden),
            "adapter install smoke artifact exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_adapter_install_smoke_verify_accepts_digest_pinned_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let scaffold_dir = temp.path().join("adapter-scaffold");
    let summary_path = write_plugin_adapter_scaffold_fixture(&scaffold_dir, false);
    let summary_sha256 = sha256_path(&summary_path);
    let smoke_path = temp.path().join("adapter-install-smoke.json");

    let smoke = ao2([
        "plugin",
        "adapter-install-smoke",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--out",
        smoke_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(smoke.status.success(), "{}", stderr(&smoke));
    let smoke_sha256 = sha256_path(&smoke_path);

    let verify = ao2([
        "plugin",
        "adapter-install-smoke-verify",
        "--smoke",
        smoke_path.to_str().unwrap(),
        "--smoke-sha256",
        &smoke_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));

    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-adapter-install-smoke-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["smoke_path"], smoke_path.display().to_string());
    assert_eq!(json["smoke_sha256"], smoke_sha256);
    assert_eq!(
        json["adapter_install_smoke_schema_version"],
        "ao2.plugin-adapter-install-smoke.v1"
    );
    assert_eq!(json["targets"], serde_json::json!(["codex", "claude"]));
    assert_eq!(json["adapter_files_verified"], true);
    assert_eq!(json["digest_gates_verified"], true);
    assert_eq!(json["command_surface_verified"], true);
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
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
    assert_eq!(json["side_effects"]["provider_execution_started"], false);
    assert_eq!(json["side_effects"]["queue_mutated"], false);
    assert_eq!(json["side_effects"]["memory_written"], false);
    assert_eq!(json["side_effects"]["ao_artifacts_mutated"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["side_effects"]["release_approved"], false);
    assert_eq!(json["token_safe_output_verified"], true);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let bad_digest = ao2([
        "plugin",
        "adapter-install-smoke-verify",
        "--smoke",
        smoke_path.to_str().unwrap(),
        "--smoke-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin adapter install smoke sha256 mismatch"));

    let mut tampered_smoke: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&smoke_path).unwrap()).unwrap();
    tampered_smoke["trust_boundary"]["control_plane_role"] = serde_json::json!("producer");
    let tampered_path = temp.path().join("tampered-adapter-install-smoke.json");
    fs::write(
        &tampered_path,
        serde_json::to_string_pretty(&tampered_smoke).unwrap(),
    )
    .unwrap();
    let tampered_sha256 = sha256_path(&tampered_path);
    let tampered = ao2([
        "plugin",
        "adapter-install-smoke-verify",
        "--smoke",
        tampered_path.to_str().unwrap(),
        "--smoke-sha256",
        &tampered_sha256,
        "--json",
    ]);
    assert!(!tampered.status.success());
    assert!(stderr(&tampered)
        .contains("plugin adapter install smoke trust_boundary is not observer-only"));

    let forbidden_env = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "plugin",
            "adapter-install-smoke-verify",
            "--smoke",
            smoke_path.to_str().unwrap(),
            "--smoke-sha256",
            &smoke_sha256,
            "--json",
        ])
        .env("OPENAI_API_KEY", "forbidden-test-value")
        .output()
        .unwrap();
    assert!(!forbidden_env.status.success());
    assert!(stderr(&forbidden_env).contains("OPENAI_API_KEY"));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&verify).contains(forbidden),
            "adapter install smoke verification output exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_adapter_install_smoke_observer_bundle_packages_three_platform_verifications() {
    let temp = tempfile::tempdir().unwrap();
    let mut verification_paths = Vec::new();
    let mut verification_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let scaffold_dir = temp.path().join(platform).join("adapter-scaffold");
        let summary_path = write_plugin_adapter_scaffold_fixture(&scaffold_dir, false);
        let summary_sha256 = sha256_path(&summary_path);
        let smoke_path = temp
            .path()
            .join(platform)
            .join("plugin-adapter-install-smoke.json");
        let smoke = ao2([
            "plugin",
            "adapter-install-smoke",
            "--summary",
            summary_path.to_str().unwrap(),
            "--summary-sha256",
            &summary_sha256,
            "--out",
            smoke_path.to_str().unwrap(),
            "--json",
        ]);
        assert!(smoke.status.success(), "{}", stderr(&smoke));
        let smoke_sha256 = sha256_path(&smoke_path);
        let verify = ao2([
            "plugin",
            "adapter-install-smoke-verify",
            "--smoke",
            smoke_path.to_str().unwrap(),
            "--smoke-sha256",
            &smoke_sha256,
            "--json",
        ]);
        assert!(verify.status.success(), "{}", stderr(&verify));
        let verification_path = temp
            .path()
            .join(platform)
            .join("plugin-adapter-install-smoke-verification.json");
        fs::write(&verification_path, stdout(&verify)).unwrap();
        verification_shas.push(sha256_path(&verification_path));
        verification_paths.push(verification_path);
    }

    let out_dir = temp.path().join("adapter-install-smoke-observer-bundle");
    let bundle = ao2([
        "plugin",
        "adapter-install-smoke-observer-bundle",
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
        "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1"
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
            "ao2.plugin-adapter-install-smoke.v1",
            "ao2.plugin-adapter-install-smoke-verification.v1"
        ])
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
            json["platform_verifications"][*platform]["sha256"],
            verification_shas[idx]
        );
        assert_eq!(
            json["platform_verifications"][*platform]["schema_version"],
            "ao2.plugin-adapter-install-smoke-verification.v1"
        );
        assert_eq!(
            json["platform_verifications"][*platform]["status"],
            "passed"
        );
        assert!(json["platform_verifications"][*platform]["smoke_sha256"]
            .as_str()
            .unwrap()
            .chars()
            .all(|ch| ch.is_ascii_hexdigit()));
        assert!(Path::new(
            json["platform_verifications"][*platform]["bundled_path"]
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
            "adapter install-smoke observer bundle output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(summary_path)
                .unwrap()
                .contains(forbidden),
            "adapter install-smoke observer bundle summary exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "adapter-install-smoke-observer-bundle",
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
    assert!(
        stderr(&bad_digest).contains("macos adapter install-smoke verification sha256 mismatch")
    );
}

#[test]
fn cli_plugin_adapter_observer_bundle_packages_three_platform_adapter_verifications() {
    let temp = tempfile::tempdir().unwrap();
    let mut verification_paths = Vec::new();
    let mut verification_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let scaffold_dir = temp.path().join(platform).join("adapter-scaffold");
        let summary_path = write_plugin_adapter_scaffold_fixture(&scaffold_dir, false);
        let summary_sha256 = sha256_path(&summary_path);
        let verify = ao2([
            "plugin",
            "adapter-scaffold-verify",
            "--summary",
            summary_path.to_str().unwrap(),
            "--summary-sha256",
            &summary_sha256,
            "--json",
        ]);
        assert!(verify.status.success(), "{}", stderr(&verify));
        let verification_path = temp
            .path()
            .join(platform)
            .join("plugin-adapter-scaffold-verification.json");
        fs::write(&verification_path, stdout(&verify)).unwrap();
        verification_shas.push(sha256_path(&verification_path));
        verification_paths.push(verification_path);
    }

    let out_dir = temp.path().join("adapter-observer-bundle");
    let bundle = ao2([
        "plugin",
        "adapter-observer-bundle",
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
        "ao2.k37-plugin-adapter-observer-bundle.v1"
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
            "ao2.plugin-adapter-scaffold.v1",
            "ao2.plugin-adapter-scaffold-verification.v1"
        ])
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
            json["platform_verifications"][*platform]["sha256"],
            verification_shas[idx]
        );
        assert_eq!(
            json["platform_verifications"][*platform]["schema_version"],
            "ao2.plugin-adapter-scaffold-verification.v1"
        );
        assert_eq!(
            json["platform_verifications"][*platform]["status"],
            "passed"
        );
        assert!(json["platform_verifications"][*platform]["summary_sha256"]
            .as_str()
            .unwrap()
            .chars()
            .all(|ch| ch.is_ascii_hexdigit()));
        assert!(Path::new(
            json["platform_verifications"][*platform]["bundled_path"]
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
            "adapter observer bundle output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(summary_path)
                .unwrap()
                .contains(forbidden),
            "adapter observer bundle summary exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "adapter-observer-bundle",
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
    assert!(stderr(&bad_digest).contains("macos adapter verification sha256 mismatch"));
}

fn write_plugin_adapter_scaffold_fixture(root: &Path, tamper_codex_trust: bool) -> PathBuf {
    fs::create_dir_all(root.join("codex")).unwrap();
    fs::create_dir_all(root.join("claude")).unwrap();
    let package_summary_sha256 = "1111111111111111111111111111111111111111111111111111111111111111";
    let package_archive_sha256 = "2222222222222222222222222222222222222222222222222222222222222222";
    let k37_bundle_sha256 = "3333333333333333333333333333333333333333333333333333333333333333";
    let k37_archive_sha256 = "4444444444444444444444444444444444444444444444444444444444444444";
    let provider_auth = serde_json::json!({
        "local_oauth_cli_only": true,
        "provider_api_key_auth_allowed": false,
        "provider_api_key_env_required": false
    });
    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "factory_v3_role": "parity_auditor",
        "control_plane_role": "read_only_observer",
        "mutates_ao_artifacts": false,
        "control_plane_approves_release": false
    });
    let commands = serde_json::json!({
        "readiness": "ao2 plugin readiness --json",
        "package_verify": "ao2 plugin package-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "distribution_observer_bundle": "ao2 plugin distribution-observer-bundle --macos-observer <path> --macos-sha256 <sha256> --ubuntu-observer <path> --ubuntu-sha256 <sha256> --windows-observer <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "consumer_lifecycle_observer_bundle": "ao2 plugin consumer-lifecycle-observer-bundle --macos-lifecycle <path> --macos-sha256 <sha256> --ubuntu-lifecycle <path> --ubuntu-sha256 <sha256> --windows-lifecycle <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "consumer_lifecycle_observer_bundle_verify": "ao2 plugin consumer-lifecycle-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "control_plane_fixture_handoff": "ao2 plugin control-plane-fixture-handoff --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "control_plane_fixture_handoff_verify": "ao2 plugin control-plane-fixture-handoff-verify --handoff <path> --handoff-sha256 <sha256> --out <path> --json",
        "release_candidate": "ao2 plugin release-candidate --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --distribution-rehearsal <path> --distribution-rehearsal-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-gate-with-replacement-observer-bundle <path> --release-gate-with-replacement-observer-bundle-sha256 <sha256> --release-gate-with-replacement-observer-archive <path> --release-gate-with-replacement-observer-archive-sha256 <sha256> --control-plane-fixture-handoff-verification <path> --control-plane-fixture-handoff-verification-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "release_candidate_verify": "ao2 plugin release-candidate-verify --summary <path> --summary-sha256 <sha256> --json",
        "release_candidate_windows_recovery": "ao2 plugin release-candidate-windows-recovery --package-summary <path> --package-summary-sha256 <sha256> --package-archive <path> --package-archive-sha256 <sha256> --distribution-rehearsal <path> --distribution-rehearsal-sha256 <sha256> --adapter-observer-bundle <path> --adapter-observer-bundle-sha256 <sha256> --adapter-observer-archive <path> --adapter-observer-archive-sha256 <sha256> --adapter-install-smoke-observer-bundle <path> --adapter-install-smoke-observer-bundle-sha256 <sha256> --adapter-install-smoke-observer-archive <path> --adapter-install-smoke-observer-archive-sha256 <sha256> --consumer-lifecycle-observer-bundle <path> --consumer-lifecycle-observer-bundle-sha256 <sha256> --consumer-lifecycle-observer-archive <path> --consumer-lifecycle-observer-archive-sha256 <sha256> --release-gate-with-replacement-observer-bundle <path> --release-gate-with-replacement-observer-bundle-sha256 <sha256> --release-gate-with-replacement-observer-archive <path> --release-gate-with-replacement-observer-archive-sha256 <sha256> --control-plane-fixture-handoff-verification <path> --control-plane-fixture-handoff-verification-sha256 <sha256> --control-plane-readback-commit <sha> --out-dir <dir> --json",
        "release_candidate_windows_recovery_verify": "ao2 plugin release-candidate-windows-recovery-verify --recovery <path> --recovery-sha256 <sha256> --out <path> --json",
        "release_candidate_windows_transfer_bundle": "ao2 plugin release-candidate-windows-transfer-bundle --ao2-source-archive <path> --ao2-source-archive-sha256 <sha256> --recovery-dir <dir> --recovery <path> --recovery-sha256 <sha256> --recovery-verification <path> --recovery-verification-sha256 <sha256> --out-dir <dir> --json",
        "release_candidate_observer_bundle": "ao2 plugin release-candidate-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "release_candidate_observer_bundle_verify": "ao2 plugin release-candidate-observer-bundle-verify --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --json",
        "release_candidate_control_plane_fixture_handoff": "ao2 plugin release-candidate-control-plane-fixture-handoff --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "release_candidate_control_plane_fixture_handoff_verify": "ao2 plugin release-candidate-control-plane-fixture-handoff-verify --handoff <path> --handoff-sha256 <sha256> --out <path> --json",
        "final_install_transcript": "ao2 plugin final-install-transcript --summary <path> --summary-sha256 <sha256> --archive <path> --archive-sha256 <sha256> --out-dir <dir> --json",
        "final_install_transcript_observer_bundle": "ao2 plugin final-install-transcript-observer-bundle --macos-codex-transcript <path> --macos-codex-sha256 <sha256> --macos-claude-transcript <path> --macos-claude-sha256 <sha256> --ubuntu-codex-transcript <path> --ubuntu-codex-sha256 <sha256> --ubuntu-claude-transcript <path> --ubuntu-claude-sha256 <sha256> --windows-codex-transcript <path> --windows-codex-sha256 <sha256> --windows-claude-transcript <path> --windows-claude-sha256 <sha256> --out-dir <dir> --json",
        "closer_decision": "ao2 factory closer-decision --rubric <path> --rubric-sha256 <sha256> --evidence <path> --evidence-sha256 <sha256> --skill-contract-manifest <path> --skill-contract-manifest-sha256 <sha256> --signing-key <path> --signer-id <id> --out <path> --json",
        "closer_decision_verify": "ao2 factory closer-decision-verify --decision <path> --decision-sha256 <sha256> --json",
        "shipment_readiness": "ao2 plugin shipment-readiness --json",
        "adapter_install_smoke_verify": "ao2 plugin adapter-install-smoke-verify --smoke <path> --smoke-sha256 <sha256> --json",
        "adapter_install_smoke_observer_bundle": "ao2 plugin adapter-install-smoke-observer-bundle --macos-verification <path> --macos-sha256 <sha256> --ubuntu-verification <path> --ubuntu-sha256 <sha256> --windows-verification <path> --windows-sha256 <sha256> --out-dir <dir> --json",
        "wrapper_harness": "ao2 plugin wrapper-harness --readiness <path> --readiness-sha256 <sha256> --args-file <path> --args-sha256 <sha256> --run-kind <app-run|project-run> --out-dir <dir> --json",
        "wrapper_harness_verify": "ao2 plugin wrapper-harness-verify --evidence-dir <dir> --summary-sha256 <sha256> --json"
    });
    let digest_gates = serde_json::json!({
        "package_summary_sha256_verified": true,
        "package_archive_sha256_verified": true,
        "k37_bundle_sha256_verified": true,
        "k37_archive_sha256_verified": true,
        "wrapper_inputs_must_be_sha256_pinned": true
    });
    let control_plane_observation = serde_json::json!({
        "role": "read_only_observer",
        "may_observe_evidence_bundle_path": true,
        "may_mutate_evidence": false,
        "may_approve_release": false
    });
    let side_effects = serde_json::json!({
        "provider_execution_started": false,
        "queue_mutated": false,
        "memory_written": false,
        "ao_artifacts_mutated": false,
        "control_plane_mutated": false,
        "release_approved": false
    });
    let mut adapter_files = serde_json::Map::new();
    for target in ["codex", "claude"] {
        let adapter_path = root.join(target).join("ao2-plugin-adapter.json");
        let adapter_trust = if target == "codex" && tamper_codex_trust {
            serde_json::json!({
                "execution_owner": "ao2",
                "factory_v3_role": "parity_auditor",
                "control_plane_role": "producer",
                "mutates_ao_artifacts": false,
                "control_plane_approves_release": false
            })
        } else {
            trust_boundary.clone()
        };
        let adapter = serde_json::json!({
            "schema_version": "ao2.plugin-adapter.v1",
            "status": "ready_for_local_oauth_wrapper_integration",
            "target": target,
            "provider_auth": provider_auth,
            "inputs": {
                "package_summary_path": "target/plugin-package/ao2-plugin-package.json",
                "package_summary_sha256": package_summary_sha256,
                "package_archive_path": "target/plugin-package/ao2-plugin-package.tar.gz",
                "package_archive_sha256": package_archive_sha256,
                "k37_bundle_path": "target/k37/k37-plugin-observer-bundle.json",
                "k37_bundle_sha256": k37_bundle_sha256,
                "k37_archive_path": "target/k37/k37-plugin-observer-bundle.tar.gz",
                "k37_archive_sha256": k37_archive_sha256
            },
            "commands": commands,
            "digest_gates": digest_gates,
            "side_effects": side_effects,
            "trust_boundary": adapter_trust,
            "control_plane_observation": control_plane_observation,
            "factory_v3_role": "parity_auditor",
            "token_safe_output": {
                "redaction_policy": "paths_status_and_digests_only",
                "bearer_tokens_serialized": false,
                "cookies_serialized": false,
                "private_keys_serialized": false
            }
        });
        fs::write(
            &adapter_path,
            serde_json::to_string_pretty(&adapter).unwrap(),
        )
        .unwrap();
        adapter_files.insert(
            target.to_string(),
            serde_json::json!({
                "path": adapter_path.display().to_string(),
                "sha256": sha256_path(&adapter_path),
                "schema_version": "ao2.plugin-adapter.v1",
                "status": "ready_for_local_oauth_wrapper_integration"
            }),
        );
    }
    let summary_path = root.join("plugin-adapter-scaffold.json");
    let summary = serde_json::json!({
        "schema_version": "ao2.plugin-adapter-scaffold.v1",
        "status": "ready_for_local_oauth_wrapper_integration",
        "summary_path": summary_path.display().to_string(),
        "targets": ["codex", "claude"],
        "package": {
            "summary_path": "target/plugin-package/ao2-plugin-package.json",
            "summary_sha256": package_summary_sha256,
            "archive_path": "target/plugin-package/ao2-plugin-package.tar.gz",
            "archive_sha256": package_archive_sha256,
            "schema_version": "ao2.plugin-package.v1"
        },
        "k37_observer_bundle": {
            "summary_path": "target/k37/k37-plugin-observer-bundle.json",
            "summary_sha256": k37_bundle_sha256,
            "archive_path": "target/k37/k37-plugin-observer-bundle.tar.gz",
            "archive_sha256": k37_archive_sha256,
            "schema_version": "ao2.k37-plugin-observer-bundle.v1"
        },
        "adapter_files": serde_json::Value::Object(adapter_files),
        "digest_gates": digest_gates,
        "provider_auth": provider_auth,
        "trust_boundary": trust_boundary,
        "control_plane_observation": control_plane_observation,
        "side_effects": side_effects,
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    summary_path
}
