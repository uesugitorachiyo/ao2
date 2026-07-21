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
fn cli_plugin_final_install_transcript_records_codex_claude_digest_pinned_steps() {
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

    let out_dir = temp.path().join("final-install-transcript");
    let transcript = ao2([
        "plugin",
        "final-install-transcript",
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
    assert!(transcript.status.success(), "{}", stderr(&transcript));
    let json: serde_json::Value = serde_json::from_str(&stdout(&transcript)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-final-install-transcript.v1"
    );
    assert_eq!(json["status"], "ready_for_plugin_consumers");
    assert_eq!(json["summary_sha256"], summary_sha256);
    assert_eq!(json["archive_sha256"], archive_sha256);
    assert_eq!(
        json["consumer_targets"],
        serde_json::json!(["codex", "claude"])
    );
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
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(json["token_safe_output_verified"], true);
    assert_eq!(json["factory_v3_role"], "parity_auditor");
    assert!(json["install_transcript"]
        .as_array()
        .unwrap()
        .iter()
        .any(|step| step["command"]
            .as_str()
            .unwrap()
            .contains("release-candidate-observer-bundle-verify")));
    assert!(json["install_transcript"]
        .as_array()
        .unwrap()
        .iter()
        .all(|step| step["executes_provider"] == false
            && step["mutates_control_plane"] == false
            && step["approves_release"] == false));

    let transcript_path = Path::new(json["transcript_path"].as_str().unwrap());
    assert!(transcript_path.is_file());
    assert_eq!(json["transcript_sha256"], sha256_path(transcript_path));
    let markdown_path = Path::new(json["markdown_path"].as_str().unwrap());
    assert!(markdown_path.is_file());
    assert_eq!(json["markdown_sha256"], sha256_path(markdown_path));
    let markdown = fs::read_to_string(markdown_path).unwrap();
    assert!(markdown.contains("Codex"));
    assert!(markdown.contains("Claude"));
    assert!(markdown.contains("Local OAuth CLI only"));
    assert!(markdown.contains("release-candidate-observer-bundle-verify"));

    let bad_digest = ao2([
        "plugin",
        "final-install-transcript",
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
    assert!(stderr(&bad_digest).contains("final install transcript summary sha256 mismatch"));
}

#[test]
fn cli_plugin_final_install_transcript_observer_bundle_packages_three_platform_targets() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("final-install-transcripts");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut transcript_paths = std::collections::BTreeMap::new();
    let mut transcript_shas = std::collections::BTreeMap::new();
    for platform in ["macos", "ubuntu", "windows"] {
        for target in ["codex", "claude"] {
            let dir = inputs_dir.join(platform).join(target);
            fs::create_dir_all(&dir).unwrap();
            let transcript_path = dir.join("plugin-final-install-transcript.json");
            let transcript = serde_json::json!({
                "schema_version": "ao2.plugin-final-install-transcript.v1",
                "status": "ready_for_plugin_consumers",
                "producer": "ao2",
                "work_source": "codex-cron AO2 production/plugin readiness",
                "consumer_targets": ["codex", "claude"],
                "summary_path": "target/k37-plugin-release-candidate-observer-bundle.json",
                "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "archive_path": "target/k37-plugin-release-candidate-observer-bundle.tar.gz",
                "archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
                "source_schema_version": "ao2.k37-plugin-release-candidate-observer-bundle.v1",
                "source_status": "ready_for_k37_observation",
                "platform_count": 3,
                "platforms": ["macos", "ubuntu", "windows"],
                "platform_release_candidates_sha256": "3333333333333333333333333333333333333333333333333333333333333333",
                "observed_evidence_scope": [
                    "ao2.plugin-release-candidate.v1",
                    "ao2.plugin-release-candidate-verification.v1"
                ],
                "install_transcript": [
                    {
                        "target": "codex",
                        "step": "readiness",
                        "command": "ao2 plugin readiness --json",
                        "executes_provider": false,
                        "mutates_queue": false,
                        "writes_memory": false,
                        "mutates_control_plane": false,
                        "mutates_ao_artifacts": false,
                        "approves_release": false
                    },
                    {
                        "target": "claude",
                        "step": "verify_release_candidate_observer_bundle",
                        "command": "ao2 plugin release-candidate-observer-bundle-verify --json",
                        "executes_provider": false,
                        "mutates_queue": false,
                        "writes_memory": false,
                        "mutates_control_plane": false,
                        "mutates_ao_artifacts": false,
                        "approves_release": false
                    }
                ],
                "install_transcript_sha256": "4444444444444444444444444444444444444444444444444444444444444444",
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
                "factory_v3_role": "parity_auditor",
                "transcript_path": transcript_path.display().to_string(),
                "markdown_path": dir.join("INSTALL-TRANSCRIPT.md").display().to_string()
            });
            fs::write(
                &transcript_path,
                serde_json::to_string_pretty(&transcript).unwrap(),
            )
            .unwrap();
            transcript_paths.insert((platform.to_string(), target.to_string()), transcript_path);
        }
    }
    for (key, path) in &transcript_paths {
        transcript_shas.insert(key.clone(), sha256_path(path));
    }

    let out_dir = temp.path().join("final-install-observer-bundle");
    let bundle = ao2([
        "plugin",
        "final-install-transcript-observer-bundle",
        "--macos-codex-transcript",
        transcript_paths[&("macos".to_string(), "codex".to_string())]
            .to_str()
            .unwrap(),
        "--macos-codex-sha256",
        &transcript_shas[&("macos".to_string(), "codex".to_string())],
        "--macos-claude-transcript",
        transcript_paths[&("macos".to_string(), "claude".to_string())]
            .to_str()
            .unwrap(),
        "--macos-claude-sha256",
        &transcript_shas[&("macos".to_string(), "claude".to_string())],
        "--ubuntu-codex-transcript",
        transcript_paths[&("ubuntu".to_string(), "codex".to_string())]
            .to_str()
            .unwrap(),
        "--ubuntu-codex-sha256",
        &transcript_shas[&("ubuntu".to_string(), "codex".to_string())],
        "--ubuntu-claude-transcript",
        transcript_paths[&("ubuntu".to_string(), "claude".to_string())]
            .to_str()
            .unwrap(),
        "--ubuntu-claude-sha256",
        &transcript_shas[&("ubuntu".to_string(), "claude".to_string())],
        "--windows-codex-transcript",
        transcript_paths[&("windows".to_string(), "codex".to_string())]
            .to_str()
            .unwrap(),
        "--windows-codex-sha256",
        &transcript_shas[&("windows".to_string(), "codex".to_string())],
        "--windows-claude-transcript",
        transcript_paths[&("windows".to_string(), "claude".to_string())]
            .to_str()
            .unwrap(),
        "--windows-claude-sha256",
        &transcript_shas[&("windows".to_string(), "claude".to_string())],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.k37-plugin-final-install-transcript-observer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(json["target_count"], 2);
    assert_eq!(json["transcript_count"], 6);
    assert_eq!(
        json["consumer_targets"],
        serde_json::json!(["codex", "claude"])
    );
    assert_eq!(
        json["observed_evidence_scope"],
        serde_json::json!(["ao2.plugin-final-install-transcript.v1"])
    );
    assert_eq!(
        json["platform_transcripts"]["windows"]["claude"]["sha256"],
        transcript_shas[&("windows".to_string(), "claude".to_string())]
    );
    assert_eq!(
        json["platform_transcripts"]["macos"]["codex"]["schema_version"],
        "ao2.plugin-final-install-transcript.v1"
    );
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
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(json["token_safe_output_verified"], true);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(archive_path.is_file());
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "final-install-transcript-observer-bundle",
        "--macos-codex-transcript",
        transcript_paths[&("macos".to_string(), "codex".to_string())]
            .to_str()
            .unwrap(),
        "--macos-codex-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--macos-claude-transcript",
        transcript_paths[&("macos".to_string(), "claude".to_string())]
            .to_str()
            .unwrap(),
        "--macos-claude-sha256",
        &transcript_shas[&("macos".to_string(), "claude".to_string())],
        "--ubuntu-codex-transcript",
        transcript_paths[&("ubuntu".to_string(), "codex".to_string())]
            .to_str()
            .unwrap(),
        "--ubuntu-codex-sha256",
        &transcript_shas[&("ubuntu".to_string(), "codex".to_string())],
        "--ubuntu-claude-transcript",
        transcript_paths[&("ubuntu".to_string(), "claude".to_string())]
            .to_str()
            .unwrap(),
        "--ubuntu-claude-sha256",
        &transcript_shas[&("ubuntu".to_string(), "claude".to_string())],
        "--windows-codex-transcript",
        transcript_paths[&("windows".to_string(), "codex".to_string())]
            .to_str()
            .unwrap(),
        "--windows-codex-sha256",
        &transcript_shas[&("windows".to_string(), "codex".to_string())],
        "--windows-claude-transcript",
        transcript_paths[&("windows".to_string(), "claude".to_string())]
            .to_str()
            .unwrap(),
        "--windows-claude-sha256",
        &transcript_shas[&("windows".to_string(), "claude".to_string())],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos codex final install transcript sha256 mismatch"));
}

#[test]
fn cli_plugin_shipment_readiness_aggregates_operator_handoff_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("shipment-inputs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "factory_v3_role": "parity_auditor",
        "control_plane_role": "read_only_observer",
        "mutates_ao_artifacts": false,
        "control_plane_approves_release": false
    });
    let control_plane_observation = serde_json::json!({
        "role": "read_only_observer",
        "may_observe_evidence_bundle_path": true,
        "may_mutate_evidence": false,
        "may_approve_release": false
    });
    let side_effects = serde_json::json!({
        "would_execute_provider": false,
        "would_execute_queue": false,
        "would_write_memory": false,
        "would_mutate_control_plane": false,
        "would_mutate_ao_artifacts": false,
        "would_approve_release": false
    });
    let provider_auth = serde_json::json!({
        "local_oauth_cli_only": true,
        "provider_api_key_auth_allowed": false,
        "provider_api_key_env_required": false
    });

    let write_json = |name: &str, value: serde_json::Value| {
        let path = inputs_dir.join(name);
        fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        let sha = sha256_path(&path);
        (path, sha)
    };
    let write_archive = |name: &str| {
        let path = inputs_dir.join(name);
        fs::write(&path, b"token-safe fixture archive\n").unwrap();
        let sha = sha256_path(&path);
        (path, sha)
    };

    let (package_summary, package_summary_sha256) = write_json(
        "ao2-plugin-package.json",
        serde_json::json!({
            "schema_version": "ao2.plugin-package.v1",
            "status": "packaged",
            "producer": "ao2",
            "provider_auth": provider_auth,
            "trust_boundary": trust_boundary,
            "control_plane_observation": control_plane_observation,
            "side_effects": side_effects,
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        }),
    );
    let (package_archive, package_archive_sha256) = write_archive("ao2-plugin-package.tar.gz");
    let (adapter_observer_bundle, adapter_observer_bundle_sha256) = write_json(
        "k37-plugin-adapter-observer-bundle.json",
        serde_json::json!({
            "schema_version": "ao2.k37-plugin-adapter-observer-bundle.v1",
            "status": "ready_for_k37_observation",
            "producer": "ao2",
            "provider_auth": provider_auth,
            "trust_boundary": trust_boundary,
            "control_plane_observation": control_plane_observation,
            "side_effects": side_effects,
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        }),
    );
    let (adapter_observer_archive, adapter_observer_archive_sha256) =
        write_archive("k37-plugin-adapter-observer-bundle.tar.gz");
    let (adapter_install_smoke_observer_bundle, adapter_install_smoke_observer_bundle_sha256) =
        write_json(
            "k37-plugin-adapter-install-smoke-observer-bundle.json",
            serde_json::json!({
                "schema_version": "ao2.k37-plugin-adapter-install-smoke-observer-bundle.v1",
                "status": "ready_for_k37_observation",
                "producer": "ao2",
                "provider_auth": provider_auth,
                "trust_boundary": trust_boundary,
                "control_plane_observation": control_plane_observation,
                "side_effects": side_effects,
                "token_safe_output_verified": true,
                "factory_v3_role": "parity_auditor"
            }),
        );
    let (adapter_install_smoke_observer_archive, adapter_install_smoke_observer_archive_sha256) =
        write_archive("k37-plugin-adapter-install-smoke-observer-bundle.tar.gz");
    let (consumer_lifecycle_observer_bundle, consumer_lifecycle_observer_bundle_sha256) =
        write_json(
            "k37-plugin-consumer-lifecycle-observer-bundle.json",
            serde_json::json!({
                "schema_version": "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1",
                "status": "ready_for_k37_observation",
                "producer": "ao2",
                "platform_count": 3,
                "platforms": ["macos", "ubuntu", "windows"],
                "provider_auth": provider_auth,
                "trust_boundary": trust_boundary,
                "control_plane_observation": control_plane_observation,
                "side_effects": side_effects,
                "token_safe_output_verified": true,
                "factory_v3_role": "parity_auditor"
            }),
        );
    let (consumer_lifecycle_observer_archive, consumer_lifecycle_observer_archive_sha256) =
        write_archive("k37-plugin-consumer-lifecycle-observer-bundle.tar.gz");
    let (release_candidate_observer_bundle, release_candidate_observer_bundle_sha256) = write_json(
        "k37-plugin-release-candidate-observer-bundle.json",
        serde_json::json!({
            "schema_version": "ao2.k37-plugin-release-candidate-observer-bundle.v1",
            "status": "ready_for_k37_observation",
            "producer": "ao2",
            "platform_count": 3,
            "platforms": ["macos", "ubuntu", "windows"],
            "provider_auth": provider_auth,
            "trust_boundary": trust_boundary,
            "control_plane_observation": control_plane_observation,
            "side_effects": side_effects,
            "token_safe_output_verified": true,
            "factory_v3_role": "parity_auditor"
        }),
    );
    let (release_candidate_observer_archive, release_candidate_observer_archive_sha256) =
        write_archive("k37-plugin-release-candidate-observer-bundle.tar.gz");
    let (final_install_transcript_observer_bundle, final_install_transcript_observer_bundle_sha256) =
        write_json(
            "k37-plugin-final-install-transcript-observer-bundle.json",
            serde_json::json!({
                "schema_version": "ao2.k37-plugin-final-install-transcript-observer-bundle.v1",
                "status": "ready_for_k37_observation",
                "producer": "ao2",
                "platform_count": 3,
                "target_count": 2,
                "transcript_count": 6,
                "platforms": ["macos", "ubuntu", "windows"],
                "consumer_targets": ["codex", "claude"],
                "observed_evidence_scope": ["ao2.plugin-final-install-transcript.v1"],
                "provider_auth": provider_auth,
                "trust_boundary": trust_boundary,
                "control_plane_observation": control_plane_observation,
                "side_effects": side_effects,
                "token_safe_output_verified": true,
                "factory_v3_role": "parity_auditor"
            }),
        );
    let (
        final_install_transcript_observer_archive,
        final_install_transcript_observer_archive_sha256,
    ) = write_archive("k37-plugin-final-install-transcript-observer-bundle.tar.gz");

    let out_dir = temp.path().join("shipment-readiness");
    let readiness = ao2([
        "plugin",
        "shipment-readiness",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        &package_summary_sha256,
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &package_archive_sha256,
        "--adapter-observer-bundle",
        adapter_observer_bundle.to_str().unwrap(),
        "--adapter-observer-bundle-sha256",
        &adapter_observer_bundle_sha256,
        "--adapter-observer-archive",
        adapter_observer_archive.to_str().unwrap(),
        "--adapter-observer-archive-sha256",
        &adapter_observer_archive_sha256,
        "--adapter-install-smoke-observer-bundle",
        adapter_install_smoke_observer_bundle.to_str().unwrap(),
        "--adapter-install-smoke-observer-bundle-sha256",
        &adapter_install_smoke_observer_bundle_sha256,
        "--adapter-install-smoke-observer-archive",
        adapter_install_smoke_observer_archive.to_str().unwrap(),
        "--adapter-install-smoke-observer-archive-sha256",
        &adapter_install_smoke_observer_archive_sha256,
        "--consumer-lifecycle-observer-bundle",
        consumer_lifecycle_observer_bundle.to_str().unwrap(),
        "--consumer-lifecycle-observer-bundle-sha256",
        &consumer_lifecycle_observer_bundle_sha256,
        "--consumer-lifecycle-observer-archive",
        consumer_lifecycle_observer_archive.to_str().unwrap(),
        "--consumer-lifecycle-observer-archive-sha256",
        &consumer_lifecycle_observer_archive_sha256,
        "--release-candidate-observer-bundle",
        release_candidate_observer_bundle.to_str().unwrap(),
        "--release-candidate-observer-bundle-sha256",
        &release_candidate_observer_bundle_sha256,
        "--release-candidate-observer-archive",
        release_candidate_observer_archive.to_str().unwrap(),
        "--release-candidate-observer-archive-sha256",
        &release_candidate_observer_archive_sha256,
        "--final-install-transcript-observer-bundle",
        final_install_transcript_observer_bundle.to_str().unwrap(),
        "--final-install-transcript-observer-bundle-sha256",
        &final_install_transcript_observer_bundle_sha256,
        "--final-install-transcript-observer-archive",
        final_install_transcript_observer_archive.to_str().unwrap(),
        "--final-install-transcript-observer-archive-sha256",
        &final_install_transcript_observer_archive_sha256,
        "--control-plane-readback-commit",
        "85a92b6e43d5579f68d01ccceb5c49f8e9268b6f",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(readiness.status.success(), "{}", stderr(&readiness));
    let json: serde_json::Value = serde_json::from_str(&stdout(&readiness)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-shipment-readiness.v1");
    assert_eq!(json["status"], "ready_for_operator_handoff");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(
        json["control_plane_readback"]["commit"],
        "85a92b6e43d5579f68d01ccceb5c49f8e9268b6f"
    );
    assert_eq!(
        json["plugin_targets"],
        serde_json::json!(["codex", "claude"])
    );
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["shipment_evidence"]["package"]["summary_sha256"],
        package_summary_sha256
    );
    assert_eq!(
        json["shipment_evidence"]["final_install_transcript_observer_bundle"]["summary_sha256"],
        final_install_transcript_observer_bundle_sha256
    );
    assert_eq!(
        json["required_operator_checks"],
        serde_json::json!([
            "verify local OAuth CLI login for Codex and Claude",
            "verify package digest before install",
            "verify final install transcript observer bundle digest",
            "keep ao2-control-plane read-only",
            "verify hosted C85 Release Gate result before operator handoff"
        ])
    );
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
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(json["token_safe_output_verified"], true);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));

    let bad_digest = ao2([
        "plugin",
        "shipment-readiness",
        "--package-summary",
        package_summary.to_str().unwrap(),
        "--package-summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--package-archive",
        package_archive.to_str().unwrap(),
        "--package-archive-sha256",
        &package_archive_sha256,
        "--adapter-observer-bundle",
        adapter_observer_bundle.to_str().unwrap(),
        "--adapter-observer-bundle-sha256",
        &adapter_observer_bundle_sha256,
        "--adapter-observer-archive",
        adapter_observer_archive.to_str().unwrap(),
        "--adapter-observer-archive-sha256",
        &adapter_observer_archive_sha256,
        "--adapter-install-smoke-observer-bundle",
        adapter_install_smoke_observer_bundle.to_str().unwrap(),
        "--adapter-install-smoke-observer-bundle-sha256",
        &adapter_install_smoke_observer_bundle_sha256,
        "--adapter-install-smoke-observer-archive",
        adapter_install_smoke_observer_archive.to_str().unwrap(),
        "--adapter-install-smoke-observer-archive-sha256",
        &adapter_install_smoke_observer_archive_sha256,
        "--consumer-lifecycle-observer-bundle",
        consumer_lifecycle_observer_bundle.to_str().unwrap(),
        "--consumer-lifecycle-observer-bundle-sha256",
        &consumer_lifecycle_observer_bundle_sha256,
        "--consumer-lifecycle-observer-archive",
        consumer_lifecycle_observer_archive.to_str().unwrap(),
        "--consumer-lifecycle-observer-archive-sha256",
        &consumer_lifecycle_observer_archive_sha256,
        "--release-candidate-observer-bundle",
        release_candidate_observer_bundle.to_str().unwrap(),
        "--release-candidate-observer-bundle-sha256",
        &release_candidate_observer_bundle_sha256,
        "--release-candidate-observer-archive",
        release_candidate_observer_archive.to_str().unwrap(),
        "--release-candidate-observer-archive-sha256",
        &release_candidate_observer_archive_sha256,
        "--final-install-transcript-observer-bundle",
        final_install_transcript_observer_bundle.to_str().unwrap(),
        "--final-install-transcript-observer-bundle-sha256",
        &final_install_transcript_observer_bundle_sha256,
        "--final-install-transcript-observer-archive",
        final_install_transcript_observer_archive.to_str().unwrap(),
        "--final-install-transcript-observer-archive-sha256",
        &final_install_transcript_observer_archive_sha256,
        "--control-plane-readback-commit",
        "85a92b6e43d5579f68d01ccceb5c49f8e9268b6f",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("shipment package summary sha256 mismatch"));
}

#[test]
fn cli_skill_contract_manifest_generates_inventory_and_fail_closed_guardrail() {
    let temp = tempfile::tempdir().unwrap();
    let factory_root = temp.path().join("factory-v3");
    for dir in ["agents", "scripts", "docs/plans"] {
        fs::create_dir_all(factory_root.join(dir)).unwrap();
    }
    for (path, body) in [
        (
            "agents/intake.toml",
            "name = \"intake\"\noutputs = [\"bounded intake\"]\n",
        ),
        (
            "scripts/verify_closure.py",
            "print('closure verification placeholder')\n",
        ),
        (
            "agents/evaluator-closer.toml",
            "name = \"evaluator-closer\"\noutputs = [\"acceptance decision\"]\n",
        ),
        (
            "scripts/factory_doctor.py",
            "FORBIDDEN_ENV = ['OPENAI_API_KEY', 'ANTHROPIC_API_KEY']\n",
        ),
        (
            "SETUP.md",
            "Use local CLI OAuth only. Do not configure provider API keys.\n",
        ),
        (
            "docs/plans/ao2-factory-v3-replacement-parity-plan.md",
            "Cross-platform behavior on macOS, Ubuntu, and Windows. Plugin bundle readiness.\n",
        ),
    ] {
        fs::write(factory_root.join(path), body).unwrap();
    }

    let out_dir = temp.path().join("skill-contract-manifest");
    let generated = ao2([
        "skill-contract-manifest",
        "generate",
        "--factory-v3-root",
        factory_root.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(generated.status.success(), "{}", stderr(&generated));
    let json: serde_json::Value = serde_json::from_str(&stdout(&generated)).unwrap();
    assert_eq!(json["schema_version"], "ao2.skill-contract-manifest.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["entry_count"], 7);
    assert_eq!(
        json["required_inventory"],
        serde_json::json!([
            "intake",
            "closure_verification",
            "evaluator_closer_acceptance",
            "provider_auth_rules",
            "redaction_token_safety",
            "cross_platform_proof",
            "plugin_shipment_runbook_rules"
        ])
    );
    assert_eq!(json["guardrails"]["runtime_critical_checked"], true);
    assert_eq!(
        json["guardrails"]["runtime_critical_requires_enforcement_or_blocker"],
        true
    );
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
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["token_safe_output_verified"], true);

    let manifest_path = Path::new(json["manifest_path"].as_str().unwrap());
    assert!(manifest_path.is_file());
    assert_eq!(json["manifest_sha256"], sha256_path(manifest_path));
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["entries"].as_array().unwrap().len(), 7);
    let entries = manifest["entries"].as_array().unwrap();
    for required in json["required_inventory"].as_array().unwrap() {
        let name = required.as_str().unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("missing entry {name}"));
        assert!(Path::new(entry["source_path"].as_str().unwrap()).is_file());
        assert_eq!(
            entry["source_sha256"],
            sha256_path(Path::new(entry["source_path"].as_str().unwrap()))
        );
        if entry["category"] == "runtime_critical" {
            let has_enforcement = entry["enforcement"]["ao2_command"].as_str().is_some()
                && entry["enforcement"]["ao2_test"].as_str().is_some()
                && entry["enforcement"]["ao2_artifact"].as_str().is_some();
            let has_blocker = entry["blocker"].as_str().is_some();
            assert!(
                has_enforcement || has_blocker,
                "runtime-critical entry {name} must be enforced or blocked"
            );
        }
    }

    let provider_auth = entries
        .iter()
        .find(|entry| entry["name"] == "provider_auth_rules")
        .unwrap();
    assert_eq!(provider_auth["ao2_disposition"], "enforced");
    assert_eq!(
        provider_auth["enforcement"]["ao2_command"],
        "ao2 plugin readiness"
    );
    let closure = entries
        .iter()
        .find(|entry| entry["name"] == "closure_verification")
        .unwrap();
    assert_eq!(closure["ao2_disposition"], "enforced");
    assert_eq!(
        closure["enforcement"]["ao2_command"],
        "ao2 factory closer-decision"
    );
    assert_eq!(
        closure["enforcement"]["ao2_artifact"],
        "ao2.factory-closer-decision.v1"
    );
    assert!(closure["blocker"].is_null());

    let mut tampered = manifest.clone();
    let tampered_entries = tampered["entries"].as_array_mut().unwrap();
    let tampered_provider_auth = tampered_entries
        .iter_mut()
        .find(|entry| entry["name"] == "provider_auth_rules")
        .unwrap();
    tampered_provider_auth["enforcement"] = serde_json::json!({});
    tampered_provider_auth["blocker"] = serde_json::Value::Null;
    let tampered_path = temp.path().join("tampered-skill-contract-manifest.json");
    fs::write(
        &tampered_path,
        serde_json::to_string_pretty(&tampered).unwrap(),
    )
    .unwrap();
    let tampered_sha256 = sha256_path(&tampered_path);
    let rejected = ao2([
        "skill-contract-manifest",
        "verify",
        "--manifest",
        tampered_path.to_str().unwrap(),
        "--manifest-sha256",
        &tampered_sha256,
        "--json",
    ]);
    assert!(!rejected.status.success());
    assert!(stderr(&rejected).contains(
        "runtime-critical skill-contract entry provider_auth_rules lacks enforcement or blocker"
    ));
}

#[test]
fn cli_plugin_distribution_observer_bundle_packages_three_platform_k37_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("observer-inputs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut input_paths = Vec::new();
    let mut input_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let path = inputs_dir.join(format!("{platform}-k37-plugin-observer-input.json"));
        let input = serde_json::json!({
            "schema_version": "ao2.k37-plugin-observer-input.v1",
            "status": "ready_for_k37_observation",
            "producer": "ao2",
            "work_source": "codex-cron AO2 production/plugin readiness",
            "package_summary_path": format!("target/{platform}/ao2-plugin-package.json"),
            "package_summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
            "package_archive_path": format!("target/{platform}/ao2-plugin-package.tar.gz"),
            "package_archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
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

    let out_dir = temp.path().join("observer-bundle");
    let bundle = ao2([
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
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));

    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(json["schema_version"], "ao2.k37-plugin-observer-bundle.v1");
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
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
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        assert_eq!(
            json["platform_inputs"][*platform]["sha256"],
            input_shas[idx]
        );
        assert_eq!(
            json["platform_inputs"][*platform]["schema_version"],
            "ao2.k37-plugin-observer-input.v1"
        );
        assert_eq!(
            json["platform_inputs"][*platform]["status"],
            "ready_for_k37_observation"
        );
        assert!(Path::new(
            json["platform_inputs"][*platform]["bundled_path"]
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

    let stdout_body = stdout(&bundle);
    let summary_body = fs::read_to_string(summary_path).unwrap();
    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout_body.contains(forbidden),
            "observer bundle output exposed forbidden marker {forbidden}"
        );
        assert!(
            !summary_body.contains(forbidden),
            "observer bundle summary exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "distribution-observer-bundle",
        "--macos-observer",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-observer",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-observer",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos observer input sha256 mismatch"));
}

#[test]
fn cli_plugin_clean_package_operator_index_packages_three_platform_rehearsals() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("clean-package-rehearsals");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut input_paths = Vec::new();
    let mut input_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let observer_input_path =
            inputs_dir.join(format!("{platform}-k37-plugin-observer-input.json"));
        let observer_input = serde_json::json!({
            "schema_version": "ao2.k37-plugin-observer-input.v1",
            "status": "ready_for_k37_observation",
            "producer": "ao2",
            "work_source": "codex-cron AO2 production/plugin readiness",
            "package_summary_path": format!("target/{platform}/ao2-plugin-package.json"),
            "package_summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
            "package_archive_path": format!("target/{platform}/ao2-plugin-package.tar.gz"),
            "package_archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
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
        fs::write(
            &observer_input_path,
            serde_json::to_string_pretty(&observer_input).unwrap(),
        )
        .unwrap();
        let observer_input_sha256 = sha256_path(&observer_input_path);

        let rehearsal_path =
            inputs_dir.join(format!("{platform}-plugin-distribution-rehearsal.json"));
        let rehearsal = serde_json::json!({
            "schema_version": "ao2.plugin-distribution-rehearsal.v1",
            "status": "passed",
            "summary_path": format!("target/{platform}/plugin-distribution-rehearsal.json"),
            "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
            "archive_path": format!("target/{platform}/ao2-plugin-package.tar.gz"),
            "archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
            "package_verified_before_install": true,
            "targets": ["codex", "claude"],
            "target_results": {
                "codex": {"status": "passed"},
                "claude": {"status": "passed"}
            },
            "observer_input": {
                "path": observer_input_path.display().to_string(),
                "sha256": observer_input_sha256
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
            "factory_v3_role": "parity_auditor",
            "token_safe_output_verified": true
        });
        fs::write(
            &rehearsal_path,
            serde_json::to_string_pretty(&rehearsal).unwrap(),
        )
        .unwrap();
        input_shas.push(sha256_path(&rehearsal_path));
        input_paths.push(rehearsal_path);
    }

    let out_dir = temp.path().join("clean-package-operator-index");
    let index = ao2([
        "plugin",
        "clean-package-operator-index",
        "--macos-rehearsal",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &input_shas[0],
        "--ubuntu-rehearsal",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-rehearsal",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(index.status.success(), "{}", stderr(&index));

    let json: serde_json::Value = serde_json::from_str(&stdout(&index)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.k37-clean-package-operator-index.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["plugin_targets"],
        serde_json::json!(["codex", "claude"])
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
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
    );
    assert_eq!(json["token_safe_output_verified"], true);

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        assert_eq!(
            json["platform_rehearsals"][*platform]["sha256"],
            input_shas[idx]
        );
        assert_eq!(
            json["platform_rehearsals"][*platform]["schema_version"],
            "ao2.plugin-distribution-rehearsal.v1"
        );
        assert_eq!(
            json["platform_rehearsals"][*platform]["target_results"]["codex"]["status"],
            "passed"
        );
        assert_eq!(
            json["platform_rehearsals"][*platform]["target_results"]["claude"]["status"],
            "passed"
        );
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "clean-package-operator-index",
        "--macos-rehearsal",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-rehearsal",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-rehearsal",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos clean package rehearsal sha256 mismatch"));
}

#[test]
fn cli_plugin_packaged_replacement_observer_bundle_packages_and_verifies_three_platform_proofs() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("packaged-replacement-proofs");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut input_paths = Vec::new();
    let mut input_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let proof_path = inputs_dir.join(format!("{platform}-packaged-replacement-hardening.json"));
        let proof = serde_json::json!({
            "schema_version": "ao2.packaged-replacement-hardening.v1",
            "status": "passed",
            "platform": platform,
            "package": {
                "summary": format!("target/{platform}/ao2-plugin-package.json"),
                "summary_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "archive": format!("target/{platform}/ao2-plugin-package.tar.gz"),
                "archive_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
                "package_verify": format!("target/{platform}/package-verify.json"),
                "package_verify_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
            },
            "factory_replacement": {
                "app_run": format!("target/{platform}/app-run.json"),
                "app_run_sha256": "4444444444444444444444444444444444444444444444444444444444444444",
                "app_run_bundle": format!("target/{platform}/app-run-bundle.tgz"),
                "app_run_bundle_sha256": "5555555555555555555555555555555555555555555555555555555555555555",
                "project_plan": format!("target/{platform}/project-plan.json"),
                "project_plan_sha256": "6666666666666666666666666666666666666666666666666666666666666666",
                "project_run": format!("target/{platform}/project-run.json"),
                "project_run_sha256": "7777777777777777777777777777777777777777777777777777777777777777",
                "release_review_package": format!("target/{platform}/release-review-package.tgz"),
                "release_review_package_sha256": "8888888888888888888888888888888888888888888888888888888888888888",
                "rubric_sha256": "9999999999999999999999999999999999999999999999999999999999999999",
                "project_acceptance_rubric_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
            "closer_decision": {
                "decision": format!("target/{platform}/closer-decision.json"),
                "decision_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "decision_verification": format!("target/{platform}/closer-decision-verification.json"),
                "decision_verification_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "schema_version": "ao2.factory-closer-decision.v1",
                "verification_schema_version": "ao2.factory-closer-decision-verification.v1",
                "rubric_sha256": "9999999999999999999999999999999999999999999999999999999999999999"
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
                "provider_execution": false,
                "queue_mutation": false,
                "memory_write": false,
                "control_plane_mutation": false,
                "ao_artifact_mutation": false,
                "release_approval": false
            },
            "token_safe_output": {
                "bearer_tokens_serialized": false,
                "cookies_serialized": false,
                "private_keys_serialized": false,
                "redaction_policy": "paths_status_and_digests_only"
            },
            "factory_v3_role": "parity_auditor"
        });
        fs::write(&proof_path, serde_json::to_string_pretty(&proof).unwrap()).unwrap();
        input_shas.push(sha256_path(&proof_path));
        input_paths.push(proof_path);
    }

    let out_dir = temp.path().join("packaged-replacement-observer-bundle");
    let bundle = ao2([
        "plugin",
        "packaged-replacement-observer-bundle",
        "--macos-proof",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &input_shas[0],
        "--ubuntu-proof",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-proof",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));

    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.k37-packaged-replacement-hardening-observer-bundle.v1"
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
            "ao2.packaged-replacement-hardening.v1",
            "ao2.factory-closer-decision.v1",
            "ao2.factory-closer-decision-verification.v1"
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
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
    );
    assert_eq!(json["token_safe_output_verified"], true);

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        assert_eq!(
            json["platform_proofs"][*platform]["sha256"],
            input_shas[idx]
        );
        assert_eq!(
            json["platform_proofs"][*platform]["schema_version"],
            "ao2.packaged-replacement-hardening.v1"
        );
        assert_eq!(json["platform_proofs"][*platform]["status"], "passed");
        assert_eq!(
            json["platform_proofs"][*platform]["factory_replacement"]["rubric_sha256"],
            "9999999999999999999999999999999999999999999999999999999999999999"
        );
        assert_eq!(
            json["platform_proofs"][*platform]["closer_decision"]["decision_sha256"],
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(
            json["platform_proofs"][*platform]["closer_decision"]["rubric_sha256"],
            json["platform_proofs"][*platform]["factory_replacement"]["rubric_sha256"]
        );
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let verify = ao2([
        "plugin",
        "packaged-replacement-observer-bundle-verify",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        json["summary_sha256"].as_str().unwrap(),
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        json["archive_sha256"].as_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verification: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verification["schema_version"],
        "ao2.k37-packaged-replacement-hardening-observer-bundle-verification.v1"
    );
    assert_eq!(verification["status"], "passed");
    assert_eq!(
        verification["source_schema_version"],
        "ao2.k37-packaged-replacement-hardening-observer-bundle.v1"
    );
    assert_eq!(verification["summary_sha256"], json["summary_sha256"]);
    assert_eq!(verification["archive_sha256"], json["archive_sha256"]);

    let bad_digest = ao2([
        "plugin",
        "packaged-replacement-observer-bundle",
        "--macos-proof",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-proof",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-proof",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos packaged replacement proof sha256 mismatch"));

    let mut missing_closer: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&input_paths[0]).unwrap()).unwrap();
    missing_closer
        .as_object_mut()
        .unwrap()
        .remove("closer_decision");
    let missing_closer_path = inputs_dir.join("macos-missing-closer.json");
    fs::write(
        &missing_closer_path,
        serde_json::to_string_pretty(&missing_closer).unwrap(),
    )
    .unwrap();
    let missing_closer_sha256 = sha256_path(&missing_closer_path);
    let rejected_missing_closer = ao2([
        "plugin",
        "packaged-replacement-observer-bundle",
        "--macos-proof",
        missing_closer_path.to_str().unwrap(),
        "--macos-sha256",
        &missing_closer_sha256,
        "--ubuntu-proof",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-proof",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!rejected_missing_closer.status.success());
    assert!(stderr(&rejected_missing_closer)
        .contains("macos packaged replacement proof missing closer_decision"));
}

#[test]
fn cli_plugin_release_gate_with_replacement_observer_bundle_packages_three_platform_rollups() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_dir = temp.path().join("release-gate-rollups");
    fs::create_dir_all(&inputs_dir).unwrap();

    let mut input_paths = Vec::new();
    let mut input_shas = Vec::new();
    for platform in ["macos", "ubuntu", "windows"] {
        let rollup_path = inputs_dir.join(format!("{platform}-rollup.json"));
        let rollup = serde_json::json!({
            "schema_version": "ao2.release-gate-with-replacement-parity.v1",
            "overall_verdict": "PASS",
            "ao2_git_head": "1234567890abcdef1234567890abcdef12345678",
            "counts": {
                "passed": 7,
                "non_passed": 0,
                "total_stages": 7
            },
            "stages": [
                {"name": "no_factory_v3_green_path", "status": "PASS"},
                {"name": "replacement_parity", "status": "PASS", "detail": "passed=4/4"},
                {"name": "release_gate", "status": "PASS"},
                {"name": "provider_readiness", "status": "PASS"},
                {"name": "plugin_readiness", "status": "PASS"},
                {"name": "license_provenance", "status": "PASS"},
                {"name": "sidecar_relocation", "status": "PASS"}
            ],
            "trust_boundary": {
                "role": "ao2_canonical_full_release_gate",
                "ao2_role": "canonical_producer",
                "factory_v3_role": "parity_oracle_only",
                "mutates_ao_artifacts": false,
                "mutates_control_plane": false
            }
        });
        fs::write(&rollup_path, serde_json::to_string_pretty(&rollup).unwrap()).unwrap();
        input_shas.push(sha256_path(&rollup_path));
        input_paths.push(rollup_path);
    }

    let out_dir = temp
        .path()
        .join("release-gate-with-replacement-observer-bundle");
    let bundle = ao2([
        "plugin",
        "release-gate-with-replacement-observer-bundle",
        "--macos-rollup",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &input_shas[0],
        "--ubuntu-rollup",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-rollup",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));

    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.k37-release-gate-with-replacement-observer-bundle.v1"
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
        serde_json::json!(["ao2.release-gate-with-replacement-parity.v1"])
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
    assert_eq!(json["side_effects"]["would_execute_queue"], false);
    assert_eq!(json["side_effects"]["would_write_memory"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["side_effects"]["would_approve_release"], false);
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
    );
    assert_eq!(json["token_safe_output_verified"], true);

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        assert_eq!(
            json["platform_rollups"][*platform]["sha256"],
            input_shas[idx]
        );
        assert_eq!(
            json["platform_rollups"][*platform]["schema_version"],
            "ao2.release-gate-with-replacement-parity.v1"
        );
        assert_eq!(
            json["platform_rollups"][*platform]["overall_verdict"],
            "PASS"
        );
        assert_eq!(
            json["platform_rollups"][*platform]["counts"]["non_passed"],
            0
        );
        assert!(Path::new(
            json["platform_rollups"][*platform]["bundled_path"]
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
        "release-gate-with-replacement-observer-bundle",
        "--macos-rollup",
        input_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-rollup",
        input_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &input_shas[1],
        "--windows-rollup",
        input_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &input_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(
        stderr(&bad_digest).contains("macos release-gate-with-replacement rollup sha256 mismatch")
    );
}
