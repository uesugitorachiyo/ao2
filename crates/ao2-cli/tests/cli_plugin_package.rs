use std::fs;
use std::path::Path;
use std::process::Command;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;

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
fn generate_native_signing_key(path: &Path, bits: usize) {
    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["workbench", "support-keygen", "--out"])
        .arg(path)
        .args(["--bits", &bits.to_string()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        path.is_file(),
        "native signing key exists: {}",
        path.display()
    );
}
fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}
fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_plugin_readiness_emits_codex_claude_wrapper_contract() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("plugin-readiness.json");

    let result = ao2([
        "plugin",
        "readiness",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(result.status.success(), "{}", stderr(&result));

    let stdout_body = stdout(&result);
    let stdout_json: serde_json::Value = serde_json::from_str(&stdout_body).unwrap();
    let file_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();

    assert_eq!(stdout_json, file_json);
    assert_eq!(stdout_json["schema_version"], "ao2.plugin-readiness.v1");
    assert_eq!(stdout_json["status"], "accepted");
    assert_eq!(
        stdout_json["plugin_targets"],
        serde_json::json!(["codex", "claude"])
    );
    assert_eq!(stdout_json["stable_json"]["stdout_json_flag"], true);
    assert_eq!(stdout_json["stable_json"]["schema_version_required"], true);
    assert_eq!(stdout_json["exit_codes"]["success"], 0);
    assert_eq!(stdout_json["exit_codes"]["runtime_error"], 1);
    assert_eq!(stdout_json["exit_codes"]["cli_usage"], 2);
    assert_eq!(stdout_json["digest_gated_inputs"]["required"], true);
    assert_eq!(
        stdout_json["token_safe_output"]["provider_api_key_auth_allowed"],
        false
    );
    assert_eq!(
        stdout_json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(stdout_json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        stdout_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        stdout_json["trust_boundary"]["factory_v3_role"],
        "parity_auditor"
    );
    assert!(stdout_json["durable_evidence_paths"]["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path
            .as_str()
            .unwrap()
            .contains("app-run-evidence-bundle.tgz")));

    for forbidden in [
        "Bearer ",
        "sk-",
        "BEGIN PRIVATE KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
    ] {
        assert!(
            !stdout_body.contains(forbidden),
            "plugin readiness output exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_manifest_packages_codex_claude_installable_contract() {
    let temp = tempfile::tempdir().unwrap();
    let out_dir = temp.path().join("plugin-manifest");

    let result = ao2([
        "plugin",
        "manifest",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(result.status.success(), "{}", stderr(&result));

    let stdout_body = stdout(&result);
    let manifest_json: serde_json::Value = serde_json::from_str(&stdout_body).unwrap();
    assert_eq!(manifest_json["schema_version"], "ao2.plugin-manifest.v1");
    assert_eq!(manifest_json["status"], "packaged");
    assert_eq!(
        manifest_json["plugin_targets"],
        serde_json::json!(["codex", "claude"])
    );
    assert_eq!(manifest_json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        manifest_json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(
        manifest_json["trust_boundary"]["mutates_ao_artifacts"],
        false
    );
    assert_eq!(
        manifest_json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        manifest_json["trust_boundary"]["factory_v3_role"],
        "parity_auditor"
    );
    assert_eq!(
        manifest_json["digest_gates"]["manifest_files_sha256_verified"],
        true
    );

    let manifest_path = out_dir.join("ao2-plugin-manifest.json");
    let install_path = out_dir.join("plugin.json");
    let readiness_example = out_dir.join("examples/plugin-readiness.example.json");
    let app_args_example = out_dir.join("examples/plugin-wrapper-args.app-run.example.json");
    let project_args_example =
        out_dir.join("examples/plugin-wrapper-args.project-run.example.json");
    let app_spec_example = out_dir.join("examples/app-spec.md");
    let project_spec_example = out_dir.join("examples/project-spec.md");
    let provider_script = out_dir.join("smoke/provider-script.sh");
    let signing_key_generator = out_dir.join("smoke/generate-signing-key.sh");
    let signing_key_generator_ps1 = out_dir.join("smoke/generate-signing-key.ps1");
    let app_target_placeholder = out_dir.join("target/ao2-plugin-app/.keep");
    let codex_smoke = out_dir.join("smoke/codex-local-oauth-smoke.json");
    let claude_smoke = out_dir.join("smoke/claude-local-oauth-smoke.json");
    let consumer_readme = out_dir.join("README.md");
    for path in [
        &manifest_path,
        &install_path,
        &readiness_example,
        &app_args_example,
        &project_args_example,
        &app_spec_example,
        &project_spec_example,
        &provider_script,
        &signing_key_generator,
        &signing_key_generator_ps1,
        &app_target_placeholder,
        &codex_smoke,
        &claude_smoke,
        &consumer_readme,
    ] {
        assert!(path.is_file(), "missing manifest file {}", path.display());
    }

    let file_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest_json, file_manifest);

    let readiness_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&readiness_example).unwrap()).unwrap();
    assert_eq!(readiness_json["schema_version"], "ao2.plugin-readiness.v1");
    assert_eq!(readiness_json["status"], "accepted");

    let app_args_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&app_args_example).unwrap()).unwrap();
    assert_eq!(
        app_args_json["schema_version"],
        "ao2.plugin-wrapper-args.v1"
    );
    assert_eq!(app_args_json["run_kind"], "app-run");
    assert_eq!(app_args_json["args"][0], "factory");
    assert_eq!(app_args_json["args"][1], "app-run");

    let project_args_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&project_args_example).unwrap()).unwrap();
    assert_eq!(project_args_json["run_kind"], "project-run");
    assert_eq!(project_args_json["args"][0], "factory");
    assert_eq!(project_args_json["args"][1], "project-run");

    let codex_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&codex_smoke).unwrap()).unwrap();
    let claude_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&claude_smoke).unwrap()).unwrap();
    assert_eq!(codex_json["provider"], "codex");
    assert_eq!(claude_json["provider"], "claude");
    assert_eq!(codex_json["auth"]["local_oauth_cli_only"], true);
    assert_eq!(claude_json["auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        codex_json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(
        claude_json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );

    assert_eq!(
        manifest_json["files"]["consumer_readme"]["path"],
        "README.md"
    );
    assert_eq!(
        manifest_json["files"]["consumer_readme"]["sha256"],
        sha256_path(&consumer_readme)
    );
    assert_eq!(
        manifest_json["files"]["app_spec_example"]["path"],
        "examples/app-spec.md"
    );
    assert_eq!(
        manifest_json["files"]["provider_script"]["path"],
        "smoke/provider-script.sh"
    );
    assert_eq!(
        manifest_json["files"]["signing_key_generator"]["path"],
        "smoke/generate-signing-key.sh"
    );
    assert_eq!(
        manifest_json["files"]["signing_key_generator_ps1"]["path"],
        "smoke/generate-signing-key.ps1"
    );
    assert_eq!(
        manifest_json["files"]["app_target_placeholder"]["path"],
        "target/ao2-plugin-app/.keep"
    );
    let readme_body = fs::read_to_string(&consumer_readme).unwrap();
    let signing_key_generator_body = fs::read_to_string(&signing_key_generator).unwrap();
    let signing_key_generator_ps1_body = fs::read_to_string(&signing_key_generator_ps1).unwrap();
    assert!(readme_body.contains("ao2 plugin readiness --json"));
    assert!(readme_body.contains("AO2_BIN=/path/to/ao2 sh smoke/generate-signing-key.sh"));
    assert!(signing_key_generator_body.contains("AO2_BIN=${AO2_BIN:-ao2}"));
    assert!(signing_key_generator_body.contains("\"$AO2_BIN\" workbench support-keygen"));
    assert!(signing_key_generator_ps1_body.contains("$env:AO2_BIN"));
    assert!(signing_key_generator_ps1_body.contains("& $Ao2Bin workbench support-keygen"));
    assert!(readme_body.contains("ao2 plugin wrapper-harness "));
    assert!(readme_body.contains("ao2 plugin manifest-verify "));
    assert!(readme_body.contains("ao2 plugin package "));
    assert!(readme_body.contains("ao2 plugin package-verify "));
    assert!(readme_body.contains("ao2 plugin adapter-install-smoke-verify "));
    assert!(readme_body.contains("ao2 plugin adapter-install-smoke-observer-bundle "));
    assert!(readme_body.contains("ao2 factory closer-decision "));
    assert!(readme_body.contains("ao2 factory closer-decision-verify "));
    assert!(readme_body.contains("local OAuth CLI only"));
    assert!(readme_body.contains("control_plane_role: read_only_observer"));
    assert!(readme_body.contains("mutates_ao_artifacts: false"));
    assert!(readme_body.contains("control_plane_approves_release: false"));
    let install_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&install_path).unwrap()).unwrap();
    assert!(install_json["commands"]["closer_decision"]
        .as_str()
        .unwrap()
        .contains("ao2 factory closer-decision "));
    assert!(install_json["commands"]["closer_decision_verify"]
        .as_str()
        .unwrap()
        .contains("ao2 factory closer-decision-verify "));
    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !readme_body.contains(forbidden),
            "plugin README exposed forbidden marker {forbidden}"
        );
    }

    for forbidden in ["Bearer ", "sk-", "BEGIN PRIVATE KEY"] {
        assert!(
            !stdout_body.contains(forbidden),
            "plugin manifest output exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_manifest_verify_accepts_digest_pinned_package() {
    let temp = tempfile::tempdir().unwrap();
    let out_dir = temp.path().join("plugin-manifest");

    let package = ao2([
        "plugin",
        "manifest",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));

    let manifest_path = out_dir.join("ao2-plugin-manifest.json");
    let manifest_sha256 = sha256_path(&manifest_path);
    let verify = ao2([
        "plugin",
        "manifest-verify",
        "--manifest-dir",
        out_dir.to_str().unwrap(),
        "--manifest-sha256",
        &manifest_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));

    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-manifest-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["manifest_sha256"], manifest_sha256);
    assert_eq!(json["file_digests_verified"], true);
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
    assert_eq!(json["consumer_readme_verified"], true);

    let bad_digest = ao2([
        "plugin",
        "manifest-verify",
        "--manifest-dir",
        out_dir.to_str().unwrap(),
        "--manifest-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin manifest sha256 mismatch"));
}

#[test]
fn cli_plugin_install_smoke_consumes_verified_manifest_without_provider_execution() {
    let temp = tempfile::tempdir().unwrap();
    let out_dir = temp.path().join("plugin-manifest");

    let package = ao2([
        "plugin",
        "manifest",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));

    let manifest_path = out_dir.join("ao2-plugin-manifest.json");
    let manifest_sha256 = sha256_path(&manifest_path);
    let verify = ao2([
        "plugin",
        "manifest-verify",
        "--manifest-dir",
        out_dir.to_str().unwrap(),
        "--manifest-sha256",
        &manifest_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));

    let verification_path = out_dir.join("manifest-verification.json");
    fs::write(&verification_path, format!("\u{feff}{}", stdout(&verify))).unwrap();
    let verification_sha256 = sha256_path(&verification_path);
    let smoke_path = out_dir.join("install-smoke.json");
    let install_smoke = ao2([
        "plugin",
        "install-smoke",
        "--manifest-dir",
        out_dir.to_str().unwrap(),
        "--verification",
        verification_path.to_str().unwrap(),
        "--verification-sha256",
        &verification_sha256,
        "--out",
        smoke_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(install_smoke.status.success(), "{}", stderr(&install_smoke));

    let json: serde_json::Value = serde_json::from_str(&stdout(&install_smoke)).unwrap();
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&smoke_path).unwrap()).unwrap();
    assert_eq!(json, persisted);
    assert_eq!(json["schema_version"], "ao2.plugin-install-smoke.v1");
    assert_eq!(json["status"], "passed");
    assert_eq!(json["manifest_sha256"], manifest_sha256);
    assert_eq!(json["manifest_verification_sha256"], verification_sha256);
    assert_eq!(json["dry_run"]["provider_execution_started"], false);
    assert_eq!(json["dry_run"]["queue_mutated"], false);
    assert_eq!(json["dry_run"]["memory_written"], false);
    assert_eq!(json["dry_run"]["ao_artifacts_mutated"], false);
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
    );
    assert_eq!(
        json["digest_gates"]["manifest_verification_sha256_verified"],
        true
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
    assert_eq!(json["install_commands_verified"]["readiness"], true);
    assert_eq!(json["install_commands_verified"]["wrapper_harness"], true);
    assert_eq!(
        json["install_commands_verified"]["wrapper_harness_verify"],
        true
    );
    assert_eq!(json["install_commands_verified"]["package"], true);
    assert_eq!(json["install_commands_verified"]["package_verify"], true);
    assert_eq!(json["install_commands_verified"]["closer_decision"], true);
    assert_eq!(
        json["install_commands_verified"]["closer_decision_verify"],
        true
    );

    let stdout_body = stdout(&install_smoke);
    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout_body.contains(forbidden),
            "install smoke output exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "install-smoke",
        "--manifest-dir",
        out_dir.to_str().unwrap(),
        "--verification",
        verification_path.to_str().unwrap(),
        "--verification-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin manifest verification sha256 mismatch"));
}

#[test]
fn cli_plugin_package_bundles_verified_manifest_and_install_smoke_for_wrappers() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_dir = temp.path().join("plugin-manifest");

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
    fs::write(&verification_path, format!("\u{feff}{}", stdout(&verify))).unwrap();
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

    let out_dir = temp.path().join("plugin-package");
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
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));

    let json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-package.v1");
    assert_eq!(json["status"], "packaged");
    assert_eq!(json["manifest_sha256"], manifest_sha256);
    assert_eq!(json["manifest_verification_sha256"], verification_sha256);
    assert_eq!(json["install_smoke_sha256"], install_smoke_sha256);
    assert_eq!(
        json["digest_gates"]["manifest_verification_sha256_verified"],
        true
    );
    assert_eq!(json["digest_gates"]["install_smoke_sha256_verified"], true);
    assert_eq!(
        json["provider_auth"]["provider_api_key_auth_allowed"],
        false
    );
    assert_eq!(json["provider_auth"]["local_oauth_cli_only"], true);
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

    let archive_path = Path::new(json["archive"]["path"].as_str().unwrap());
    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    assert!(archive_path.is_file());
    assert!(summary_path.is_file());
    assert_eq!(json["archive"]["sha256"], sha256_path(archive_path));

    let extract_dir = temp.path().join("plugin-package-extract");
    fs::create_dir_all(&extract_dir).unwrap();
    let archive_file = fs::File::open(archive_path).unwrap();
    let decoder = GzDecoder::new(archive_file);
    let mut archive = Archive::new(decoder);
    archive.unpack(&extract_dir).unwrap();

    assert!(extract_dir
        .join("manifest")
        .join("ao2-plugin-manifest.json")
        .is_file());
    assert!(extract_dir
        .join("evidence")
        .join("manifest-verification.json")
        .is_file());
    assert!(extract_dir
        .join("evidence")
        .join("install-smoke.json")
        .is_file());
    assert!(extract_dir.join("ao2-plugin-package.json").is_file());
    assert!(extract_dir
        .join("manifest")
        .join("examples")
        .join("app-spec.md")
        .is_file());
    assert!(extract_dir
        .join("manifest")
        .join("examples")
        .join("project-spec.md")
        .is_file());
    assert!(extract_dir
        .join("manifest")
        .join("smoke")
        .join("provider-script.sh")
        .is_file());
    assert!(extract_dir
        .join("manifest")
        .join("smoke")
        .join("generate-signing-key.sh")
        .is_file());
    assert!(extract_dir
        .join("manifest")
        .join("target")
        .join("ao2-plugin-app")
        .join(".keep")
        .is_file());
    let extracted_manifest_text = fs::read_to_string(
        extract_dir
            .join("manifest")
            .join("ao2-plugin-manifest.json"),
    )
    .unwrap();
    assert!(!extracted_manifest_text.contains("BEGIN PRIVATE KEY"));

    let stdout_body = stdout(&package);
    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout_body.contains(forbidden),
            "plugin package output exposed forbidden marker {forbidden}"
        );
    }

    let bad_install_smoke_digest = ao2([
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
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_install_smoke_digest.status.success());
    assert!(stderr(&bad_install_smoke_digest).contains("plugin install smoke sha256 mismatch"));
}

#[test]
fn cli_plugin_wrapper_harness_runs_packaged_app_example_with_relative_paths() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_dir = temp.path().join("plugin-manifest");

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

    let signing_key = manifest_dir.join("smoke/signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let readiness_path = manifest_dir.join("examples/plugin-readiness.example.json");
    let readiness_sha256 = sha256_path(&readiness_path);
    let args_path = manifest_dir.join("examples/plugin-wrapper-args.app-run.example.json");
    let args_sha256 = sha256_path(&args_path);
    let out_dir = temp.path().join("wrapper-harness");
    let wrapper = ao2([
        "plugin",
        "wrapper-harness",
        "--readiness",
        readiness_path.to_str().unwrap(),
        "--readiness-sha256",
        &readiness_sha256,
        "--args-file",
        args_path.to_str().unwrap(),
        "--args-sha256",
        &args_sha256,
        "--run-kind",
        "app-run",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(wrapper.status.success(), "{}", stderr(&wrapper));

    let json: serde_json::Value = serde_json::from_str(&stdout(&wrapper)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-wrapper-harness.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["run_kind"], "app-run");
    assert_eq!(json["digest_gates"]["args_sha256_verified"], true);
    assert_eq!(json["token_safe_output"]["stdout_redacted"], true);
    assert_eq!(json["token_safe_output"]["stderr_redacted"], true);

    let summary_path = out_dir.join("plugin-wrapper-harness.json");
    let summary_sha256 = sha256_path(&summary_path);
    let verify = ao2([
        "plugin",
        "wrapper-harness-verify",
        "--evidence-dir",
        out_dir.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
}

#[test]
fn cli_plugin_package_verify_accepts_digest_pinned_distributable() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_dir = temp.path().join("plugin-manifest");

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
    fs::write(&verification_path, format!("\u{feff}{}", stdout(&verify))).unwrap();
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

    let out_dir = temp.path().join("plugin-package");
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
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let summary_path = out_dir.join("ao2-plugin-package.json");
    let archive_path = out_dir.join("ao2-plugin-package.tar.gz");
    let summary_sha256 = sha256_path(&summary_path);
    let archive_sha256 = sha256_path(&archive_path);

    let verify_package = ao2([
        "plugin",
        "package-verify",
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
    assert!(
        verify_package.status.success(),
        "{}",
        stderr(&verify_package)
    );

    let json: serde_json::Value = serde_json::from_str(&stdout(&verify_package)).unwrap();
    assert_eq!(json["schema_version"], "ao2.plugin-package-verification.v1");
    assert_eq!(json["status"], "passed");
    assert_eq!(json["summary_sha256"], summary_sha256);
    assert_eq!(json["archive_sha256"], archive_sha256);
    assert_eq!(json["manifest_sha256"], package_json["manifest_sha256"]);
    assert_eq!(json["archive_contents_verified"], true);
    assert_eq!(json["embedded_summary_verified"], true);
    assert_eq!(json["embedded_evidence_verified"], true);
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

    let bad_archive_digest = ao2([
        "plugin",
        "package-verify",
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
    assert!(stderr(&bad_archive_digest).contains("plugin package archive sha256 mismatch"));

    let stdout_body = stdout(&verify_package);
    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout_body.contains(forbidden),
            "plugin package verification output exposed forbidden marker {forbidden}"
        );
    }
}

#[test]
fn cli_plugin_distribution_rehearsal_installs_package_for_codex_and_claude() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_dir = temp.path().join("plugin-manifest");

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

    let package_dir = temp.path().join("plugin-package");
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

    let summary_path = package_dir.join("ao2-plugin-package.json");
    let archive_path = package_dir.join("ao2-plugin-package.tar.gz");
    let summary_sha256 = sha256_path(&summary_path);
    let archive_sha256 = sha256_path(&archive_path);
    let rehearsal_dir = temp.path().join("plugin-distribution-rehearsal");
    let rehearsal = ao2([
        "plugin",
        "distribution-rehearsal",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        &summary_sha256,
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        &archive_sha256,
        "--out-dir",
        rehearsal_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(rehearsal.status.success(), "{}", stderr(&rehearsal));

    let json: serde_json::Value = serde_json::from_str(&stdout(&rehearsal)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.plugin-distribution-rehearsal.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["summary_sha256"], summary_sha256);
    assert_eq!(json["archive_sha256"], archive_sha256);
    assert_eq!(json["package_verified_before_install"], true);
    assert_eq!(json["targets"], serde_json::json!(["codex", "claude"]));
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

    for target in ["codex", "claude"] {
        let target_json = &json["target_results"][target];
        assert_eq!(target_json["status"], "passed");
        assert_eq!(target_json["commands_from_installed_package_paths"], true);
        assert_eq!(target_json["manifest_sha256"], manifest_sha256);
        assert!(
            Path::new(target_json["installed_package_dir"].as_str().unwrap())
                .join("manifest")
                .join("ao2-plugin-manifest.json")
                .is_file()
        );
        for field in [
            "readiness_sha256",
            "manifest_verification_sha256",
            "install_smoke_sha256",
            "package_verification_sha256",
            "wrapper_harness_sha256",
            "wrapper_harness_verification_sha256",
        ] {
            assert_eq!(
                target_json[field].as_str().unwrap().len(),
                64,
                "{target} missing digest field {field}"
            );
        }
    }

    let observer_input_path = Path::new(json["observer_input"]["path"].as_str().unwrap());
    assert!(observer_input_path.is_file());
    assert_eq!(
        json["observer_input"]["sha256"],
        sha256_path(observer_input_path)
    );
    let observer_input: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(observer_input_path).unwrap()).unwrap();
    assert_eq!(
        observer_input["schema_version"],
        "ao2.k37-plugin-observer-input.v1"
    );
    assert_eq!(observer_input["status"], "ready_for_k37_observation");
    assert_eq!(
        observer_input["control_plane_observation"]["role"],
        "read_only_observer"
    );

    let stdout_body = stdout(&rehearsal);
    let observer_body = fs::read_to_string(observer_input_path).unwrap();
    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout_body.contains(forbidden),
            "distribution rehearsal output exposed forbidden marker {forbidden}"
        );
        assert!(
            !observer_body.contains(forbidden),
            "observer input exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "distribution-rehearsal",
        "--summary",
        summary_path.to_str().unwrap(),
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path.to_str().unwrap(),
        "--archive-sha256",
        &archive_sha256,
        "--out-dir",
        rehearsal_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("plugin package summary sha256 mismatch"));
}
