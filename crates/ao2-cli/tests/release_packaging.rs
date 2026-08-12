use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn cli_packages_current_binary_for_local_distribution() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let out_dir = tempfile::tempdir().expect("tempdir");

    let output = Command::new(ao2)
        .args([
            "release",
            "package",
            "--out-dir",
            out_dir.path().to_str().expect("utf8 out dir"),
            "--version",
            "9.9.9-test",
        ])
        .output()
        .expect("run ao2 release package");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("package command prints json");
    assert_eq!(json["version"], "9.9.9-test");
    let expected_binary = if cfg!(windows) { "ao2.exe" } else { "ao2" };
    assert_eq!(json["binary"], expected_binary);
    assert!(json["target"]
        .as_str()
        .expect("target")
        .contains(std::env::consts::OS));

    let archive_path = json["archive"].as_str().expect("archive path");
    let checksum_path = json["checksum_file"].as_str().expect("checksum path");
    assert!(fs::metadata(archive_path).expect("archive exists").len() > 0);

    let checksum = fs::read_to_string(checksum_path).expect("checksum file");
    assert!(checksum.contains(json["sha256"].as_str().expect("sha256")));
    assert!(checksum.contains(
        std::path::Path::new(archive_path)
            .file_name()
            .expect("archive filename")
            .to_str()
            .expect("utf8 archive filename")
    ));

    let entries = archive_entries(Path::new(archive_path));
    assert!(entries
        .iter()
        .any(|entry| entry == &format!("bin/{expected_binary}")));
    assert!(entries.iter().any(|entry| entry == "install.sh"));
    assert!(entries.iter().any(|entry| entry == "install.ps1"));
    assert!(entries.iter().any(|entry| entry == "verify-release.sh"));
    assert!(entries.iter().any(|entry| entry == "Verify-Release.ps1"));
    assert!(entries.iter().any(|entry| entry == "SHA256SUMS"));
    assert!(entries.iter().any(|entry| entry == "RELEASE-MANIFEST.json"));
    assert!(entries
        .iter()
        .any(|entry| entry == "RELEASE-VERIFICATION.json"));
    assert!(entries.iter().any(|entry| entry == "BUILD-PROVENANCE.json"));
    assert!(entries.iter().any(|entry| entry == "SBOM.cdx.json"));
    assert!(entries.iter().any(|entry| entry == "UNINSTALL.txt"));
    assert!(entries.iter().any(|entry| entry == "LICENSE"));
    assert!(entries.iter().any(|entry| entry == "NOTICE"));

    let packaged_checksum = archive_text_entry(Path::new(archive_path), "SHA256SUMS");
    let manifest = archive_text_entry(Path::new(archive_path), "RELEASE-MANIFEST.json");
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest).expect("release manifest is json");
    assert_eq!(manifest_json["schema_version"], "ao2.release-manifest.v1");
    assert_eq!(manifest_json["version"], "9.9.9-test");
    assert_eq!(manifest_json["binary"], expected_binary);
    assert_eq!(
        manifest_json["binary_path"],
        format!("bin/{}", manifest_json["binary"].as_str().unwrap())
    );
    assert_eq!(
        manifest_json["binary_sha256"],
        packaged_checksum
            .split_whitespace()
            .next()
            .expect("binary checksum present")
    );
    assert_eq!(
        manifest_json["verification_report"],
        "RELEASE-VERIFICATION.json"
    );
    assert_eq!(
        manifest_json["verifiers"],
        serde_json::json!(["verify-release.sh", "Verify-Release.ps1"])
    );
    let archive_files: BTreeSet<String> = entries.iter().cloned().collect();
    let manifest_files: BTreeSet<String> = manifest_json["files"]
        .as_array()
        .expect("manifest files")
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    assert_eq!(manifest_files, archive_files);

    for required_checksum_entry in [
        format!("bin/{expected_binary}"),
        "RELEASE-MANIFEST.json".to_string(),
        "RELEASE-VERIFICATION.json".to_string(),
        "BUILD-PROVENANCE.json".to_string(),
        "SBOM.cdx.json".to_string(),
        "UNINSTALL.txt".to_string(),
        "install.sh".to_string(),
        "install.ps1".to_string(),
        "verify-release.sh".to_string(),
        "Verify-Release.ps1".to_string(),
        "README.txt".to_string(),
        "LICENSE".to_string(),
        "NOTICE".to_string(),
        "VERSION".to_string(),
    ] {
        assert!(
            packaged_checksum.contains(&required_checksum_entry),
            "SHA256SUMS must cover {required_checksum_entry}:\n{packaged_checksum}"
        );
    }

    let verification = archive_text_entry(Path::new(archive_path), "RELEASE-VERIFICATION.json");
    let verification_json: serde_json::Value =
        serde_json::from_str(&verification).expect("release verification report is json");
    assert_eq!(
        verification_json["schema_version"],
        "ao2.release-archive-offline-verification.v1"
    );
    assert_eq!(verification_json["status"], "packaged");
    assert_eq!(verification_json["target"], manifest_json["target"]);
    assert_eq!(
        verification_json["binary_path"],
        manifest_json["binary_path"]
    );
    assert_eq!(verification_json["provider_api_keys_required"], false);
    assert_eq!(verification_json["control_plane_approves_release"], false);
    assert_eq!(verification_json["mutates_ao_artifacts"], false);
    assert_eq!(
        verification_json["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert!(verification_json["checksum_coverage"]
        .as_array()
        .expect("checksum coverage")
        .iter()
        .any(|path| path == "RELEASE-MANIFEST.json"));
    let checksum_coverage: BTreeSet<String> = verification_json["checksum_coverage"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    let expected_coverage: BTreeSet<String> = archive_files
        .iter()
        .filter(|path| path.as_str() != "SHA256SUMS")
        .cloned()
        .collect();
    assert_eq!(checksum_coverage, expected_coverage);

    let provenance = archive_text_entry(Path::new(archive_path), "BUILD-PROVENANCE.json");
    let provenance_json: serde_json::Value =
        serde_json::from_str(&provenance).expect("build provenance is json");
    assert_eq!(provenance_json["schema_version"], "ao2.build-provenance.v1");
    assert_eq!(provenance_json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(provenance_json["git_commit"].as_str().unwrap().len(), 40);
    assert_ne!(provenance_json["build_profile"], "unknown");

    let sbom = archive_text_entry(Path::new(archive_path), "SBOM.cdx.json");
    let sbom_json: serde_json::Value = serde_json::from_str(&sbom).expect("SBOM is json");
    assert_eq!(sbom_json["bomFormat"], "CycloneDX");
    assert_eq!(sbom_json["specVersion"], "1.5");
    assert!(sbom_json["components"].as_array().unwrap().len() > 10);
}

#[test]
fn cli_release_archives_are_byte_reproducible() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let first = tempfile::tempdir().expect("first tempdir");
    let second = tempfile::tempdir().expect("second tempdir");

    let package = |out: &Path| {
        let output = Command::new(ao2)
            .args([
                "release",
                "package",
                "--out-dir",
                out.to_str().expect("utf8 out dir"),
                "--version",
                env!("CARGO_PKG_VERSION"),
            ])
            .output()
            .expect("run ao2 release package");
        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        fs::read(json["archive"].as_str().unwrap()).expect("read archive")
    };

    assert_eq!(package(first.path()), package(second.path()));
}

#[test]
fn cli_release_profile_packaging_rejects_version_substitution() {
    let out = tempfile::tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .env("AO2_PACKAGED_BUILD_PROFILE", "release")
        .args([
            "release",
            "package",
            "--out-dir",
            out.path().to_str().unwrap(),
            "--version",
            "9.9.9-substituted",
        ])
        .output()
        .expect("run ao2 release package");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not match compiled binary"));
}

#[test]
fn cli_packages_explicit_binary_for_cross_target_distribution() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let out_dir = tempfile::tempdir().expect("tempdir");

    let output = Command::new(ao2)
        .args([
            "release",
            "package",
            "--out-dir",
            out_dir.path().to_str().expect("utf8 out dir"),
            "--version",
            "9.9.9-test",
            "--binary",
            ao2,
            "--target-label",
            "windows-x86_64",
        ])
        .output()
        .expect("run ao2 release package");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("package command prints json");
    assert_eq!(json["target"], "windows-x86_64");
    assert_eq!(json["binary"], "ao2.exe");
    assert!(json["archive"]
        .as_str()
        .expect("archive")
        .ends_with("ao2-9.9.9-test-windows-x86_64.tar.gz"));

    let entries = archive_entries(Path::new(json["archive"].as_str().expect("archive")));
    assert!(entries.iter().any(|entry| entry == "bin/ao2.exe"));
    assert!(entries.iter().any(|entry| entry == "install.ps1"));
    assert!(entries.iter().any(|entry| entry == "Verify-Release.ps1"));
    assert!(entries.iter().any(|entry| entry == "verify-release.sh"));
    assert!(entries.iter().any(|entry| entry == "SHA256SUMS"));
    assert!(entries.iter().any(|entry| entry == "RELEASE-MANIFEST.json"));
    assert!(entries
        .iter()
        .any(|entry| entry == "RELEASE-VERIFICATION.json"));
    assert!(entries.iter().any(|entry| entry == "LICENSE"));
    assert!(entries.iter().any(|entry| entry == "NOTICE"));

    let manifest = archive_text_entry(
        Path::new(json["archive"].as_str().expect("archive")),
        "RELEASE-MANIFEST.json",
    );
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest).expect("release manifest is json");
    assert_eq!(manifest_json["target"], "windows-x86_64");
    assert_eq!(manifest_json["binary"], "ao2.exe");
    assert_eq!(manifest_json["binary_path"], "bin/ao2.exe");
    assert_eq!(
        manifest_json["verifiers"],
        serde_json::json!(["verify-release.sh", "Verify-Release.ps1"])
    );
}

#[test]
fn hermes_project_start_poll_act_contract_covers_fail_closed_status_codes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let contract_path = root
        .join("docs")
        .join("contracts")
        .join("hermes-project-start-poll-act-contract.v1.json");
    let contract_text = fs::read_to_string(&contract_path).expect("read Hermes poll-act contract");
    let contract: serde_json::Value =
        serde_json::from_str(&contract_text).expect("Hermes poll-act contract is JSON");
    assert_eq!(
        contract["schema_version"],
        "ao2.hermes-project-start-poll-act-contract.v1"
    );
    assert_eq!(
        contract["trust_boundary"]["hermes_role"],
        "front_end_queue_cron_memory_bookkeeping"
    );
    assert_eq!(
        contract["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        contract["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        contract["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(contract["trust_boundary"]["mutates_ao_artifacts"], false);

    let covered_codes: BTreeSet<String> = contract["decision_table"]
        .as_array()
        .expect("decision table")
        .iter()
        .filter_map(|row| row["blocker_code"].as_str())
        .map(ToOwned::to_owned)
        .collect();
    for code in [
        "missing_queue_file",
        "missing_queue_entry",
        "queue_entry_status_queued",
        "queue_entry_status_running",
        "queue_entry_status_rejected",
        "queue_entry_status_missing",
        "wrong_job_kind",
        "missing_compact_artifact_queue_submit",
        "missing_compact_artifact_queue_run_next",
        "missing_compact_artifact_completion_contract",
        "missing_compact_artifact_completion_contract_consumer",
        "artifact_run_id_mismatch_queue_submit",
        "artifact_run_id_mismatch_queue_run_next",
        "artifact_run_id_mismatch_completion_contract",
        "artifact_status_mismatch_completion_contract",
        "artifact_status_mismatch_completion_contract_consumer",
        "trust_boundary_mismatch_completion_contract_consumer",
    ] {
        assert!(covered_codes.contains(code), "missing blocker code {code}");
    }

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "Authorization: Bearer",
    ] {
        assert!(
            !contract_text.contains(forbidden),
            "contract must not include forbidden surface: {forbidden}"
        );
    }
    assert!(contract_text.contains("edit raw queue JSON"));
    assert!(contract_text.contains("write control-plane release approval"));
    assert!(contract_text.contains("ao2 factory queue-project-start-complete-status"));
    assert!(contract_text.contains("ao2 factory queue-project-start-complete"));
}

#[test]
fn cli_builds_release_evidence_bundle_archive_with_manifest() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let out_dir = tmp.path().join("bundle-out");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("handoff-checklist.json");
    let decision = tmp.path().join("evaluator-decision.json");
    let install_verification = tmp.path().join("install-verification.json");
    fs::write(
        &readiness,
        r#"{"schema_version":"ao2.cp-release-readiness.v1","status":"ready"}"#,
    )
    .expect("write readiness");
    fs::write(
        &checklist,
        r#"{"schema":"factory-v3/ao2-release-handoff-checklist/v1","status":"ready_for_evaluator_closer"}"#,
    )
    .expect("write checklist");
    fs::write(
        &decision,
        r#"{"schema":"factory-v3/ao2-release-evaluator-decision/v1","status":"accepted"}"#,
    )
    .expect("write decision");
    fs::write(
        &install_verification,
        r#"{"schema_version":"ao2.install-verification-evidence.v1","status":"verified","offline_verification":{"status":"verified"},"provider_api_keys_required":false,"control_plane_approves_release":false,"mutates_ao_artifacts":false}"#,
    )
    .expect("write install verification");

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            out_dir.to_str().expect("utf8 out dir"),
            "--artifact",
            &format!("readiness={}", readiness.display()),
            "--artifact",
            &format!("handoff-checklist={}", checklist.display()),
            "--artifact",
            &format!("evaluator-decision={}", decision.display()),
            "--artifact",
            &format!("install-verification={}", install_verification.display()),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("evidence-bundle prints json");
    assert_eq!(json["schema_version"], "ao2.release-evidence-bundle.v1");
    assert_eq!(json["artifact_count"], 4);
    assert_eq!(json["install_verification_evidence"]["included"], true);
    assert_eq!(
        json["install_verification_evidence"]["artifact_labels"][0],
        "install-verification"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );

    let archive = Path::new(json["archive"].as_str().expect("archive"));
    let entries = archive_entries(archive);
    assert!(entries
        .iter()
        .any(|entry| entry == "EVIDENCE-BUNDLE-MANIFEST.json"));
    assert!(entries.iter().any(|entry| entry == "SHA256SUMS"));
    assert!(entries
        .iter()
        .any(|entry| entry == "artifacts/readiness/readiness.json"));
    assert!(entries
        .iter()
        .any(|entry| entry == "artifacts/handoff-checklist/handoff-checklist.json"));
    assert!(entries
        .iter()
        .any(|entry| entry == "artifacts/evaluator-decision/evaluator-decision.json"));
    assert!(entries
        .iter()
        .any(|entry| entry == "artifacts/install-verification/install-verification.json"));

    let manifest_text = archive_text_entry(archive, "EVIDENCE-BUNDLE-MANIFEST.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("manifest is json");
    assert_eq!(manifest["schema_version"], "ao2.release-evidence-bundle.v1");
    assert_eq!(
        manifest["artifacts"].as_array().expect("artifacts").len(),
        4
    );
    assert_eq!(manifest["install_verification_evidence"]["included"], true);
    assert_eq!(
        manifest["install_verification_evidence"]["artifact_labels"][0],
        "install-verification"
    );
    assert_eq!(
        manifest["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    let checksums = archive_text_entry(archive, "SHA256SUMS");
    assert!(checksums.contains("artifacts/readiness/readiness.json"));
    assert!(checksums.contains("artifacts/install-verification/install-verification.json"));
    assert!(checksums.contains("EVIDENCE-BUNDLE-MANIFEST.json"));
}

#[test]
fn cli_release_evidence_bundle_rejects_missing_install_verification() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    fs::write(
        &readiness,
        r#"{"schema_version":"ao2.cp-release-readiness.v1","status":"ready"}"#,
    )
    .expect("write readiness");

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            tmp.path()
                .join("bundle-out")
                .to_str()
                .expect("utf8 out dir"),
            "--artifact",
            &format!("readiness={}", readiness.display()),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("install verification evidence is required"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn cli_release_evidence_bundle_rejects_schema_invalid_install_verification() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let install_verification = tmp.path().join("install-verification.json");
    fs::write(
        &readiness,
        r#"{"schema_version":"ao2.cp-release-readiness.v1","status":"ready"}"#,
    )
    .expect("write readiness");
    fs::write(
        &install_verification,
        r#"{"schema_version":"ao2.install-verification-evidence.v0","status":"verified","offline_verification":{"status":"verified"},"provider_api_keys_required":false,"control_plane_approves_release":false,"mutates_ao_artifacts":false}"#,
    )
    .expect("write install verification");

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            tmp.path()
                .join("bundle-out")
                .to_str()
                .expect("utf8 out dir"),
            "--artifact",
            &format!("readiness={}", readiness.display()),
            "--artifact",
            &format!("install-verification={}", install_verification.display()),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("install verification evidence schema_version must be ao2.install-verification-evidence.v1"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn cli_release_evidence_bundle_rejects_trust_unsafe_install_verification() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let install_verification = tmp.path().join("install-verification.json");
    fs::write(
        &readiness,
        r#"{"schema_version":"ao2.cp-release-readiness.v1","status":"ready"}"#,
    )
    .expect("write readiness");
    fs::write(
        &install_verification,
        r#"{"schema_version":"ao2.install-verification-evidence.v1","status":"verified","offline_verification":{"status":"verified"},"provider_api_keys_required":false,"control_plane_approves_release":true,"mutates_ao_artifacts":false}"#,
    )
    .expect("write install verification");

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            tmp.path()
                .join("bundle-out")
                .to_str()
                .expect("utf8 out dir"),
            "--artifact",
            &format!("readiness={}", readiness.display()),
            "--artifact",
            &format!("install-verification={}", install_verification.display()),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "install verification evidence must not approve releases through the control plane"
        ),
        "stderr:\n{stderr}"
    );
}

#[test]
fn cli_factory_app_run_bundle_packages_release_review_evidence() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture = write_factory_app_run_fixture(tmp.path(), false);
    let archive = tmp.path().join("app-run-evidence-bundle.tgz");

    let output = Command::new(ao2)
        .args([
            "factory",
            "app-run-bundle",
            "--app-run",
            fixture.to_str().expect("utf8 fixture"),
            "--out",
            archive.to_str().expect("utf8 archive"),
            "--json",
        ])
        .output()
        .expect("run ao2 factory app-run-bundle");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("app-run-bundle prints json");
    assert_eq!(json["schema_version"], "ao2.factory-app-run-bundle.v1");
    assert_eq!(json["status"], "bundled");
    assert_eq!(json["artifact_count"], 8);
    assert_eq!(json["manifest_entry"], "manifest.json");
    assert_eq!(json["checksum_entry"], "SHA256SUMS");
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );

    let archive = Path::new(json["archive"].as_str().expect("archive"));
    let entries = archive_entries(archive);
    for expected in [
        "manifest.json",
        "SHA256SUMS",
        "artifacts/factory-app-run/factory-app-run.json",
        "artifacts/evaluator-rubric/evaluator-rubric.json",
        "artifacts/greenfield-governed-run/greenfield-governed-run.json",
        "artifacts/greenfield-ingest/greenfield-ingest.json",
        "artifacts/plan/plan.json",
        "artifacts/governed-run/governed-run.json",
        "artifacts/evidence-pack/evidence-pack.json",
        "artifacts/evaluator-decision/evaluator-decision.json",
        "release-review.json",
    ] {
        assert!(entries.iter().any(|entry| entry == expected), "{expected}");
    }

    let manifest_text = archive_text_entry(archive, "manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("manifest is json");
    assert_eq!(manifest["schema_version"], "ao2.factory-app-run-bundle.v1");
    assert_eq!(
        manifest["artifacts"].as_array().expect("artifacts").len(),
        8
    );
    assert_eq!(
        manifest["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        manifest["trust_boundary"]["control_plane_approves_release"],
        false
    );

    let checksums = archive_text_entry(archive, "SHA256SUMS");
    assert!(checksums.contains("manifest.json"));
    assert!(checksums.contains("release-review.json"));
    assert!(checksums.contains("artifacts/factory-app-run/factory-app-run.json"));
    assert!(checksums.contains("artifacts/evaluator-rubric/evaluator-rubric.json"));
    assert!(checksums.contains("artifacts/evidence-pack/evidence-pack.json"));
}

#[test]
fn cli_factory_app_run_bundle_rejects_control_plane_release_approval() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture = write_factory_app_run_fixture(tmp.path(), true);

    let output = Command::new(ao2)
        .args([
            "factory",
            "app-run-bundle",
            "--app-run",
            fixture.to_str().expect("utf8 fixture"),
            "--out",
            tmp.path()
                .join("app-run-evidence-bundle.tgz")
                .to_str()
                .expect("utf8 archive"),
            "--json",
        ])
        .output()
        .expect("run ao2 factory app-run-bundle");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("control plane must not approve release"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_factory_project_run_collects_app_run_bundles_for_evaluator_closer() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_spec = tmp.path().join("project.md");
    fs::write(
        &project_spec,
        "# Missed Call Recovery Project\n\nAcceptance:\n- Bundle app workflow evidence.\n- Preserve evaluator-closer ownership.\n",
    )
    .expect("write project spec");
    let intake = write_factory_app_run_fixture(&tmp.path().join("intake"), false);
    let messaging = write_factory_app_run_fixture(&tmp.path().join("messaging"), false);
    let out_dir = tmp.path().join("project-run");

    let output = Command::new(ao2)
        .args([
            "factory",
            "project-run",
            "--project-spec",
            project_spec.to_str().expect("utf8 project spec"),
            "--run-id",
            "missed-call-project",
            "--app-run",
            intake.to_str().expect("utf8 intake app run"),
            "--app-run",
            messaging.to_str().expect("utf8 messaging app run"),
            "--out-dir",
            out_dir.to_str().expect("utf8 out dir"),
            "--json",
        ])
        .output()
        .expect("run ao2 factory project-run");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("project-run prints json");
    assert_eq!(json["schema_version"], "ao2.factory-project-run.v1");
    assert_eq!(json["status"], "accepted");
    assert_eq!(json["app_run_count"], 2);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["project_run_checklist"]["ao2_collected_app_run_bundles"],
        true
    );
    assert_eq!(
        json["project_run_checklist"]["release_review_package_ready"],
        true
    );

    let project_run_path = Path::new(
        json["artifacts"]["factory_project_run"]
            .as_str()
            .expect("project run artifact"),
    );
    assert!(project_run_path.is_file());
    let archive = Path::new(
        json["artifacts"]["release_review_package"]
            .as_str()
            .expect("release package"),
    );
    let entries = archive_entries(archive);
    for expected in [
        "manifest.json",
        "SHA256SUMS",
        "project-run.json",
        "project-state/factory-project-run-state.json",
        "project-spec/project.md",
        "app-runs/0/factory-app-run.json",
        "app-runs/1/factory-app-run.json",
        "app-run-bundles/0/app-run-evidence-bundle.tgz",
        "app-run-bundles/1/app-run-evidence-bundle.tgz",
    ] {
        assert!(entries.iter().any(|entry| entry == expected), "{expected}");
    }
    let manifest_text = archive_text_entry(archive, "manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("manifest is json");
    assert_eq!(manifest["schema_version"], "ao2.factory-project-run.v1");
    assert_eq!(manifest["app_runs"].as_array().expect("app runs").len(), 2);
    assert_eq!(
        manifest["trust_boundary"]["control_plane_approves_release"],
        false
    );
    let checksums = archive_text_entry(archive, "SHA256SUMS");
    assert!(checksums.contains("app-runs/0/factory-app-run.json"));
    assert!(checksums.contains("app-run-bundles/1/app-run-evidence-bundle.tgz"));
}

#[test]
fn cli_factory_project_run_rejects_control_plane_release_approval() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_spec = tmp.path().join("project.md");
    fs::write(&project_spec, "# Project\n").expect("write project spec");
    let app_run = write_factory_app_run_fixture(&tmp.path().join("bad-app"), true);

    let output = Command::new(ao2)
        .args([
            "factory",
            "project-run",
            "--project-spec",
            project_spec.to_str().expect("utf8 project spec"),
            "--run-id",
            "bad-project",
            "--app-run",
            app_run.to_str().expect("utf8 app run"),
            "--out-dir",
            tmp.path()
                .join("project-run")
                .to_str()
                .expect("utf8 out dir"),
            "--json",
        ])
        .output()
        .expect("run ao2 factory project-run");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("control plane must not approve release"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_release_evidence_bundle_rejects_duplicate_labels() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let artifact = tmp.path().join("readiness.json");
    fs::write(&artifact, "{}").expect("write artifact");

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            tmp.path()
                .join("bundle-out")
                .to_str()
                .expect("utf8 out dir"),
            "--artifact",
            &format!("readiness={}", artifact.display()),
            "--artifact",
            &format!("readiness={}", artifact.display()),
        ])
        .output()
        .expect("run ao2 release evidence-bundle");

    assert!(!output.status.success(), "duplicate labels must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("duplicate artifact label"),
        "stderr should identify duplicate label, got:\n{stderr}"
    );
}

#[test]
fn cli_verifies_release_evidence_bundle_archive() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let archive = build_test_evidence_bundle(ao2, tmp.path(), false);

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle-verify",
            "--bundle",
            archive.to_str().expect("utf8 archive"),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle-verify");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("verification prints json");
    assert_eq!(
        json["schema_version"],
        "ao2.release-evidence-bundle-verification.v1"
    );
    assert_eq!(json["status"], "verified");
    assert_eq!(json["manifest_verified"], true);
    assert_eq!(json["trust_boundary_verified"], true);
    assert_eq!(json["secret_scan_passed"], true);
    assert_eq!(json["artifact_count"], 4);
    assert_eq!(json["failure_count"], 0);
}

#[test]
fn cli_release_evidence_bundle_verify_rejects_checksum_uncovered_install_verification() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let archive = build_test_evidence_bundle(ao2, tmp.path(), false);
    let tampered = tamper_evidence_bundle_without_install_checksum(&archive, tmp.path());

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle-verify",
            "--bundle",
            tampered.to_str().expect("utf8 archive"),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle-verify");

    assert!(
        !output.status.success(),
        "checksum-uncovered install evidence must fail verification"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("failed verification still prints json");
    assert_eq!(json["status"], "failed");
    assert!(json["failures"]
        .as_array()
        .expect("failures array")
        .iter()
        .any(|failure| failure["code"] == "install_verification_not_checksummed"));
}

#[test]
fn cli_verifies_phase1_promotion_inputs_manifest() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let temp = tempfile::tempdir().expect("tempdir");
    let manifest = temp.path().join("promotion-inputs.json");
    let verification_out = temp.path().join("promotion-inputs-verification.json");
    let release_gate = temp.path().join("release-gate.json");
    let replacement_smoke_gate = temp.path().join("replacement-smoke-gate.json");
    let phase1_decision = temp.path().join("phase1-decision.json");
    let phase1_checklist = temp.path().join("phase1-checklist.json");
    let phase1_evidence_bundle = temp.path().join("phase1-evidence-bundle.json");
    let provider_acceptance = temp.path().join("provider-acceptance.json");
    let macos_smoke = temp.path().join("macos-smoke.json");
    let ubuntu_smoke = temp.path().join("ubuntu-smoke.json");
    let windows_smoke = temp.path().join("windows-smoke.json");
    let macos_governed = temp.path().join("macos-governed-run.json");
    let ubuntu_governed = temp.path().join("ubuntu-governed-run.json");
    let windows_governed = temp.path().join("windows-governed-run.json");
    let macos_project = temp.path().join("macos-project-run-summary.json");
    let ubuntu_project = temp.path().join("ubuntu-project-run-summary.json");
    let windows_project = temp.path().join("windows-project-run-summary.json");
    for path in [
        &macos_smoke,
        &ubuntu_smoke,
        &windows_smoke,
        &macos_governed,
        &ubuntu_governed,
        &windows_governed,
        &macos_project,
        &ubuntu_project,
        &windows_project,
        &provider_acceptance,
        &release_gate,
        &replacement_smoke_gate,
    ] {
        fs::write(path, "{}").expect("write input fixture");
    }
    fs::write(
        &manifest,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.phase1-replacement-promotion-inputs.v1",
            "release_version": "9.9.9-test",
            "replacement_smoke_mode": "legacy_replacement_smoke_bound",
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false
            },
            "inputs": {
                "replacement_smoke": {
                    "macos": macos_smoke,
                    "ubuntu": ubuntu_smoke,
                    "windows": windows_smoke
                },
                "governed_run_evidence": {
                    "macos": macos_governed,
                    "ubuntu": ubuntu_governed,
                    "windows": windows_governed
                },
                "factory_project_run_summary": {
                    "macos": macos_project,
                    "ubuntu": ubuntu_project,
                    "windows": windows_project
                },
                "provider_acceptance_preservation": provider_acceptance
            },
            "outputs": {
                "replacement_smoke_gate": replacement_smoke_gate,
                "release_gate": release_gate,
                "phase1_decision": phase1_decision,
                "phase1_checklist": phase1_checklist,
                "phase1_evidence_bundle": phase1_evidence_bundle
            }
        }))
        .expect("serialize manifest"),
    )
    .expect("write manifest");

    let output = Command::new(ao2)
        .args([
            "release",
            "phase1-promotion-inputs-verify",
            "--manifest",
            manifest.to_str().expect("manifest utf8"),
            "--out",
            verification_out.to_str().expect("verification out utf8"),
            "--mode",
            "decision-gate",
            "--json",
        ])
        .output()
        .expect("verify phase1 promotion inputs");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is verification json");
    assert_eq!(
        stdout["schema_version"],
        "ao2.phase1-replacement-promotion-inputs-verification.v1"
    );
    assert_eq!(stdout["status"], "accepted");
    assert_eq!(stdout["mode"], "decision_gate");
    assert_eq!(stdout["manifest_path"], manifest.to_string_lossy().as_ref());
    assert_eq!(stdout["missing_required_inputs"], serde_json::json!([]));
    assert_eq!(
        stdout["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        stdout["trust_boundary"]["control_plane_approves_release"],
        false
    );
    let written: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&verification_out).expect("verification report written"),
    )
    .expect("verification report is json");
    assert_eq!(written, stdout);
}

#[test]
fn phase1_evidence_bundle_verify_script_verifies_explicit_archive() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = tempfile::tempdir().expect("tempdir");
    let archive = build_test_evidence_bundle(ao2, tmp.path(), false);
    let verification_out = tmp.path().join("phase1-evidence-bundle-verification.json");

    let output = Command::new(sh_command())
        .arg(root.join("scripts/verify-phase1-evidence-bundle.sh"))
        .current_dir(&root)
        .env("AO2_BIN", ao2)
        .env("AO2_PHASE1_EVIDENCE_BUNDLE", &archive)
        .env("AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT", &verification_out)
        .output()
        .expect("run Phase 1 evidence bundle verifier script");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("phase1_evidence_bundle_verify=passed"));
    assert!(stdout.contains("phase1_evidence_bundle_archive="));
    assert!(stdout.contains("phase1_evidence_bundle_verification="));
    let json: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&verification_out).expect("verification file exists"),
    )
    .expect("verification output is json");
    assert_eq!(json["status"], "verified");
    assert_eq!(json["artifact_count"], 4);
}

#[test]
fn morning_cross_os_readback_dispatch_uploads_archives_and_uses_windows_safe_probe() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script =
        fs::read_to_string(root.join("scripts/morning-dispatch-cross-os-readback.sh")).unwrap();

    assert!(script.contains("create_worktree_archive"));
    assert!(script.contains("git ls-files -z"));
    assert!(script.contains("git ls-files --others --exclude-standard -z"));
    assert!(script.contains("tar --null -czf"));
    assert!(script.contains("ao2-source.tgz"));
    assert!(script.contains("ao2-control-plane-source.tgz"));
    assert!(script.contains("AO2_CONTROL_PLANE_ROOT"));
    assert!(script.contains("AO2_BIN=target/release/ao2"));
    assert!(script.contains("powershell -NoProfile"));
    assert!(script.contains("C:\\Program Files\\Git\\bin\\bash.exe"));
    assert!(!script.contains("git pull --ff-only"));
    assert!(script.contains("dispatch_windows_host \"$WINDOWS_HOST\""));
    assert!(!script.contains("dispatch_unix_host \"$WINDOWS_HOST\""));
}

#[test]
fn cli_reports_phase1_promotion_status_from_root_and_bundle() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("promotion");
    fs::create_dir_all(&root).expect("create promotion root");
    let release_gate = root.join("release-gate.json");
    let decision = root.join("phase1-promotion-decision.json");
    let checklist = root.join("phase1-promotion-checklist.json");
    let inputs_verification = root.join("promotion-inputs-verification.json");
    let dashboard_snapshot_root = tmp.path().join("control-plane-dashboard-snapshot");
    let dashboard_snapshot_index = dashboard_snapshot_root.join("index.html");
    let dashboard_snapshot_manifest = dashboard_snapshot_root.join("manifest.json");
    fs::write(
        &release_gate,
        r#"{"schema":"ao2.release-gate.v1","status":"verified"}"#,
    )
    .expect("write release gate");
    fs::write(
        &decision,
        r#"{"schema":"factory-v3/ao2-phase1-promotion-decision/v1","status":"passed","decision":"promote_phase1_candidate","phase1_state":"phase1_candidate_ready"}"#,
    )
    .expect("write decision");
    fs::write(
        &checklist,
        r#"{"schema":"factory-v3/ao2-phase1-promotion-checklist/v1","schema_version":"ao2.phase1-promotion-checklist.v1","status":"passed","phase1_state":"phase1_candidate_ready"}"#,
    )
    .expect("write checklist");
    fs::write(
        &inputs_verification,
        r#"{"schema_version":"ao2.phase1-replacement-promotion-inputs-verification.v1","status":"accepted","mode":"decision_gate","failure_count":0,"missing_required_inputs":[],"failures":[],"trust_boundary":{"control_plane_role":"read_only_observer","mutates_ao_artifacts":false,"release_acceptance_owner":"factory-v3 evaluator-closer","control_plane_approves_release":false}}"#,
    )
    .expect("write promotion inputs verification");
    fs::create_dir_all(&dashboard_snapshot_root).expect("create dashboard snapshot root");
    fs::write(
        &dashboard_snapshot_index,
        "<!doctype html><title>AO2 Control Plane Dashboard Snapshots</title>",
    )
    .expect("write dashboard snapshot index");
    fs::write(
        &dashboard_snapshot_manifest,
        r#"{
  "schema_version": "ao2.cp-dashboard-snapshot.v1",
  "status": "passed",
  "base_url": "http://127.0.0.1:18745",
  "token_in_output": false,
  "surfaces": [
    {"name": "Phase 1 Promotion", "filename": "index.html", "endpoint": "/api/v1/phase1/promotion/dashboard", "sha256": "abc"}
  ],
  "trust_boundary": {
    "control_plane_role": "read_only_observer",
    "mutates_ao_artifacts": false,
    "control_plane_approves_release": false,
    "release_acceptance_owner": "factory-v3 evaluator-closer"
  }
}"#,
    )
    .expect("write dashboard snapshot manifest");

    let bundle_out = root.join("evidence-bundle");
    let install_verification = root.join("install-verification.json");
    fs::write(
        &install_verification,
        r#"{"schema_version":"ao2.install-verification-evidence.v1","status":"verified","offline_verification":{"status":"verified"},"provider_api_keys_required":false,"control_plane_approves_release":false,"mutates_ao_artifacts":false}"#,
    )
    .expect("write install verification");
    let bundle = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            bundle_out.to_str().expect("utf8 bundle out"),
            "--artifact",
            &format!("release-gate={}", release_gate.display()),
            "--artifact",
            &format!("phase1-decision={}", decision.display()),
            "--artifact",
            &format!("phase1-checklist={}", checklist.display()),
            "--artifact",
            &format!("install-verification={}", install_verification.display()),
            "--json",
        ])
        .output()
        .expect("build phase1 evidence bundle");
    assert!(
        bundle.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&bundle.stdout),
        String::from_utf8_lossy(&bundle.stderr)
    );
    let bundle_json: serde_json::Value =
        serde_json::from_slice(&bundle.stdout).expect("bundle json");
    let archive = bundle_json["archive"].as_str().expect("archive");

    let output = Command::new(ao2)
        .args([
            "release",
            "phase1-promotion-status",
            "--root",
            root.to_str().expect("utf8 root"),
            "--evidence-bundle",
            archive,
            "--json",
        ])
        .output()
        .expect("run phase1 promotion status");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status command prints json");
    assert_eq!(json["schema_version"], "ao2.phase1-promotion-status.v1");
    assert_eq!(json["status"], "ready");
    assert_eq!(json["checks"]["release_gate"], "verified");
    assert_eq!(json["checks"]["decision"], "promote_phase1_candidate");
    assert_eq!(json["checks"]["checklist"], "passed");
    assert_eq!(json["checks"]["promotion_inputs"], "accepted");
    assert_eq!(json["checks"]["evidence_bundle"], "verified");
    assert_eq!(
        json["artifacts"]["promotion_inputs_verification"],
        inputs_verification.display().to_string()
    );
    assert_eq!(
        json["artifacts"]["dashboard_snapshot_manifest"],
        dashboard_snapshot_manifest.display().to_string()
    );
    assert_eq!(
        json["artifacts"]["dashboard_snapshot_index"],
        dashboard_snapshot_index.display().to_string()
    );
    assert_eq!(json["checks"]["dashboard_snapshot"], "available");
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["status"],
        "available"
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["manifest"],
        dashboard_snapshot_manifest.display().to_string()
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["index"],
        dashboard_snapshot_index.display().to_string()
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["schema_version"],
        "ao2.cp-dashboard-snapshot.v1"
    );
    assert_eq!(json["control_plane_dashboard_snapshot"]["surface_count"], 1);
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["token_in_output"],
        false
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["trust_boundary"]
            ["control_plane_approves_release"],
        false
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["manifest_sha256"]
            .as_str()
            .expect("manifest sha"),
        sha256_file_hex(&dashboard_snapshot_manifest)
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["index_sha256"]
            .as_str()
            .expect("index sha"),
        sha256_file_hex(&dashboard_snapshot_index)
    );
    assert_eq!(
        json["control_plane_dashboard_snapshot"]["manifest_sha256"]
            .as_str()
            .expect("manifest sha")
            .len(),
        64
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
}

fn sha256_file_hex(path: &Path) -> String {
    use sha2::{Digest, Sha256};

    let bytes = fs::read(path).expect("read file for sha256");
    Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn cli_blocks_phase1_promotion_status_without_inputs_verification() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("promotion");
    fs::create_dir_all(&root).expect("create promotion root");
    let release_gate = root.join("release-gate.json");
    let decision = root.join("phase1-promotion-decision.json");
    let checklist = root.join("phase1-promotion-checklist.json");
    fs::write(
        &release_gate,
        r#"{"schema":"ao2.release-gate.v1","status":"verified"}"#,
    )
    .expect("write release gate");
    fs::write(
        &decision,
        r#"{"schema":"factory-v3/ao2-phase1-promotion-decision/v1","status":"passed","decision":"promote_phase1_candidate","phase1_state":"phase1_candidate_ready"}"#,
    )
    .expect("write decision");
    fs::write(
        &checklist,
        r#"{"schema":"factory-v3/ao2-phase1-promotion-checklist/v1","schema_version":"ao2.phase1-promotion-checklist.v1","status":"passed","phase1_state":"phase1_candidate_ready"}"#,
    )
    .expect("write checklist");

    let bundle_out = root.join("evidence-bundle");
    let install_verification = root.join("install-verification.json");
    fs::write(
        &install_verification,
        r#"{"schema_version":"ao2.install-verification-evidence.v1","status":"verified","offline_verification":{"status":"verified"},"provider_api_keys_required":false,"control_plane_approves_release":false,"mutates_ao_artifacts":false}"#,
    )
    .expect("write install verification");
    let bundle = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            bundle_out.to_str().expect("utf8 bundle out"),
            "--artifact",
            &format!("release-gate={}", release_gate.display()),
            "--artifact",
            &format!("phase1-decision={}", decision.display()),
            "--artifact",
            &format!("phase1-checklist={}", checklist.display()),
            "--artifact",
            &format!("install-verification={}", install_verification.display()),
            "--json",
        ])
        .output()
        .expect("build phase1 evidence bundle");
    assert!(
        bundle.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&bundle.stdout),
        String::from_utf8_lossy(&bundle.stderr)
    );
    let bundle_json: serde_json::Value =
        serde_json::from_slice(&bundle.stdout).expect("bundle json");
    let archive = bundle_json["archive"].as_str().expect("archive");

    let output = Command::new(ao2)
        .args([
            "release",
            "phase1-promotion-status",
            "--root",
            root.to_str().expect("utf8 root"),
            "--evidence-bundle",
            archive,
            "--json",
        ])
        .output()
        .expect("run phase1 promotion status");
    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status command prints json");
    assert_eq!(json["status"], "blocked");
    assert_eq!(json["checks"]["promotion_inputs"], "missing");
    assert!(json["failures"]
        .as_array()
        .expect("failures array")
        .iter()
        .any(|failure| failure["code"] == "phase1_promotion_inputs_not_verified"));
}

#[test]
fn cli_release_evidence_bundle_verify_rejects_secret_markers() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let archive = build_test_evidence_bundle(ao2, tmp.path(), true);

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle-verify",
            "--bundle",
            archive.to_str().expect("utf8 archive"),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle-verify");

    assert!(
        !output.status.success(),
        "secret markers must fail verification"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("failed verification still prints json");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["secret_scan_passed"], false);
    let failures = json["failures"].as_array().expect("failures array");
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "forbidden_secret_marker"),
        "expected forbidden_secret_marker failure, got {failures:?}"
    );
}

#[test]
fn release_archive_smoke_script_covers_ubuntu_and_windows_paths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script_path = root.join("scripts/smoke-release-archives.sh");
    let script = fs::read_to_string(script_path).expect("release smoke script exists");
    let linux_remote = fs::read_to_string(root.join("scripts/smoke-linux-release-remote.sh"))
        .expect("native Linux x86_64 smoke script exists");
    let linux_docker = fs::read_to_string(root.join("scripts/smoke-linux-release-docker.sh"))
        .expect("Docker Linux x86_64 smoke script exists");

    assert!(script.contains("AO2_MACOS_ARCHIVE"));
    assert!(script.contains("AO2_LINUX_ARCHIVE"));
    assert!(script.contains("AO2_LINUX_X86_64_ARCHIVE"));
    assert!(script.contains("AO2_LINUX_X86_64_SMOKE_MODE"));
    assert!(script.contains("AO2_LINUX_X86_64_DOCKER_LOG"));
    assert!(script.contains("AO2_WINDOWS_ARCHIVE"));
    assert!(script.contains("AO2_UBUNTU_SSH_TARGET"));
    assert!(script.contains("linux_x86_64_remote_smoke=passed"));
    assert!(script.contains("linux_x86_64_docker_smoke=passed"));
    assert!(script.contains("linux_x86_64_install_rollback=passed"));
    assert!(linux_remote.contains("rollback_runner"));
    assert!(linux_remote.contains("linux_x86_64_install_rollback=passed"));
    assert!(script.contains("install.sh"));
    assert!(script.contains("install.ps1"));
    assert!(script.contains("RELEASE-MANIFEST.json"));
    assert!(script.contains("version --json"));
    assert!(script.contains("adapter doctor --provider scripted"));
    assert!(script.contains("provider matrix --json"));
    assert!(script.contains("provider contract --verify --require codex --json"));
    assert!(script.contains("provider_contract_verify=passed"));
    assert!(script.contains("git init -q \"$repo\""));
    assert!(script.contains("git -C \"$repo\" commit -q -m fixture"));
    assert!(script.contains("apt-get install -y --no-install-recommends ca-certificates git jq"));
    assert!(
        linux_docker.contains("apt-get install -y --no-install-recommends ca-certificates git jq")
    );
    assert!(linux_docker.contains("docker run --rm --platform linux/amd64"));
    assert!(linux_docker.contains("git init -q \"$repo\""));
    assert!(linux_docker.contains("git -C \"$repo\" commit -q -m fixture"));
    assert!(linux_docker.contains("approve \"$ticket_id\""));
    assert!(script.contains("requested_action == \"sandbox:apply\""));
    assert!(script.contains("approve \"$ticket_id\""));
    assert!(script.contains("run --resume"));
    assert!(script.contains("test \"$approval_count\" -eq 2"));
    assert!(script.contains(" run \"$work/workflow.yaml\""));
    assert!(script.contains(" replay ubuntu-install-smoke-repair"));
    assert!(script.contains("docker run"));
}

#[test]
fn release_provenance_scripts_sign_and_verify_archive_assets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sign_script = fs::read_to_string(root.join("scripts/release-sign-provenance.sh"))
        .expect("release signing script exists");
    let verify_script = fs::read_to_string(root.join("scripts/release-verify-provenance.sh"))
        .expect("release verification script exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let gitignore = fs::read_to_string(root.join(".gitignore")).expect("gitignore exists");

    assert!(sign_script.contains("ao2.release-provenance.v1"));
    assert!(sign_script.contains(".release-signing/ao2-release-signing-key.pem"));
    assert!(!sign_script.contains("openssl"));
    assert!(sign_script.contains("release sign-provenance"));
    assert!(sign_script.contains("AO2_VERSION=\"${AO2_VERSION:-$(scripts/current-version.sh)}\""));
    assert!(sign_script.contains("ao2-$AO2_VERSION-macos-aarch64.tar.gz"));
    assert!(sign_script.contains("ao2-$AO2_VERSION-linux-aarch64.tar.gz"));
    assert!(sign_script.contains("ao2-$AO2_VERSION-linux-x86_64.tar.gz"));
    assert!(sign_script.contains("ao2-$AO2_VERSION-windows-x86_64.tar.gz"));
    assert!(sign_script.contains("AO2_LINUX_X86_64_ARCHIVE"));

    assert!(!verify_script.contains("openssl"));
    assert!(verify_script.contains("release verify-provenance"));
    assert!(verify_script.contains("AO2_LINUX_X86_64_ARCHIVE"));
    assert!(verify_script.contains("ao2-release-provenance.json.sig"));
    assert!(verify_script.contains("release_provenance_verify=passed"));

    assert!(package_json.contains("\"release:sign-provenance\""));
    assert!(package_json.contains("\"release:verify-provenance\""));
    assert!(gitignore.contains("/.release-signing/"));
    assert!(gitignore.contains("/.ao2/control-plane/"));
    assert!(gitignore.contains("**/.ao2/control-plane/"));
}

#[test]
fn factory_v3_parity_oracle_has_hosted_snapshot_fallback() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/factory-v3-parity-oracle.sh"))
        .expect("factory-v3 parity oracle script exists");
    let snapshot_root = root.join("scripts/parity-oracle-snapshots/factory-v3-20260604");
    let manifest_path = snapshot_root.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("snapshot manifest exists");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("snapshot manifest is json");

    assert!(script.contains("ORACLE_MODE=\"snapshot\""));
    assert!(script.contains("AO2_PARITY_ORACLE_SNAPSHOT_DIR"));
    assert!(script.contains("snapshot_file bridge-mapping.canonical.json"));
    assert!(script.contains("snapshot_file resolved-roles.canonical.json"));
    assert!(script.contains("snapshot_file release-evaluator-decision.canonical.json"));
    assert!(script.contains("snapshot_file release-handoff-checklist.canonical.json"));
    assert!(script.contains("snapshot_file watchdog-no-active-runs-attestation.canonical.json"));
    assert_eq!(
        manifest["schema_version"],
        "ao2.factory-v3-parity-oracle-snapshot.v1"
    );
    assert_eq!(
        manifest["source_git_head"],
        "405e2739648366ad6299518352c1a51e6487783f"
    );
    for name in [
        "bridge-mapping.canonical.json",
        "resolved-roles.canonical.json",
        "release-evaluator-decision.canonical.json",
        "release-handoff-checklist.canonical.json",
        "watchdog-no-active-runs-attestation.canonical.json",
    ] {
        assert!(
            snapshot_root.join(name).is_file(),
            "missing snapshot file: {name}"
        );
        assert!(
            manifest["snapshots_sha256"][name].as_str().is_some(),
            "missing manifest digest for {name}"
        );
    }
}

#[test]
fn phase1_replacement_promotion_script_supports_preflight_without_running_gate() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let macos = temp.path().join("macos-smoke.json");
    let ubuntu = temp.path().join("ubuntu-smoke.json");
    let windows = temp.path().join("windows-smoke.json");
    let macos_governed = temp.path().join("macos-governed-run.json");
    let ubuntu_governed = temp.path().join("ubuntu-governed-run.json");
    let windows_governed = temp.path().join("windows-governed-run.json");
    let macos_project = temp.path().join("macos-factory-project-run-summary.json");
    let ubuntu_project = temp.path().join("ubuntu-factory-project-run-summary.json");
    let windows_project = temp.path().join("windows-factory-project-run-summary.json");
    let provider_acceptance = temp.path().join("provider-acceptance-preservation.json");
    fs::write(&macos, "{}").unwrap();
    fs::write(&ubuntu, "{}").unwrap();
    fs::write(&windows, "{}").unwrap();
    fs::write(&macos_governed, "{}").unwrap();
    fs::write(&ubuntu_governed, "{}").unwrap();
    fs::write(&windows_governed, "{}").unwrap();
    fs::write(&macos_project, "{}").unwrap();
    fs::write(&ubuntu_project, "{}").unwrap();
    fs::write(&windows_project, "{}").unwrap();
    fs::write(&provider_acceptance, "{}").unwrap();

    let output = Command::new(sh_command())
        .arg(root.join("scripts/phase1-replacement-promotion.sh"))
        .current_dir(&root)
        .env("AO2_BIN", ao2)
        .env("AO2_PHASE1_PROMOTION_PREFLIGHT", "1")
        .env("AO2_PHASE1_PROMOTION_ROOT", temp.path().join("promotion"))
        .env("AO2_MACOS_REPLACEMENT_SMOKE", &macos)
        .env("AO2_UBUNTU_REPLACEMENT_SMOKE", &ubuntu)
        .env("AO2_WINDOWS_REPLACEMENT_SMOKE", &windows)
        .env("AO2_MACOS_GOVERNED_RUN_EVIDENCE", &macos_governed)
        .env("AO2_UBUNTU_GOVERNED_RUN_EVIDENCE", &ubuntu_governed)
        .env("AO2_WINDOWS_GOVERNED_RUN_EVIDENCE", &windows_governed)
        .env("AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY", &macos_project)
        .env("AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY", &ubuntu_project)
        .env("AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY", &windows_project)
        .env("AO2_PROVIDER_ACCEPTANCE_PRESERVATION", &provider_acceptance)
        .output()
        .expect("run phase1 replacement promotion preflight");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("phase1_replacement_promotion_preflight=passed"));
    assert!(stdout.contains("macos_replacement_smoke="));
    assert!(stdout.contains("ubuntu_replacement_smoke="));
    assert!(stdout.contains("windows_replacement_smoke="));
    assert!(stdout.contains("macos_governed_run_evidence="));
    assert!(stdout.contains("ubuntu_governed_run_evidence="));
    assert!(stdout.contains("windows_governed_run_evidence="));
    assert!(stdout.contains("macos_factory_project_run_summary="));
    assert!(stdout.contains("ubuntu_factory_project_run_summary="));
    assert!(stdout.contains("windows_factory_project_run_summary="));
    assert!(stdout.contains("provider_acceptance_preservation="));
    assert!(stdout.contains("phase1_decision="));
    assert!(stdout.contains("phase1_evidence_bundle_archive="));
    assert!(stdout.contains("phase1_evidence_bundle_verification="));
}

#[test]
fn phase1_replacement_promotion_script_supports_governed_run_only_preflight() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let macos_governed = temp.path().join("macos-governed-run.json");
    let ubuntu_governed = temp.path().join("ubuntu-governed-run.json");
    let windows_governed = temp.path().join("windows-governed-run.json");
    let macos_project = temp.path().join("macos-factory-project-run-summary.json");
    let ubuntu_project = temp.path().join("ubuntu-factory-project-run-summary.json");
    let windows_project = temp.path().join("windows-factory-project-run-summary.json");
    let provider_acceptance = temp.path().join("provider-acceptance-preservation.json");
    fs::write(&macos_governed, "{}").unwrap();
    fs::write(&ubuntu_governed, "{}").unwrap();
    fs::write(&windows_governed, "{}").unwrap();
    fs::write(&macos_project, "{}").unwrap();
    fs::write(&ubuntu_project, "{}").unwrap();
    fs::write(&windows_project, "{}").unwrap();
    fs::write(&provider_acceptance, "{}").unwrap();

    let output = Command::new(sh_command())
        .arg(root.join("scripts/phase1-replacement-promotion.sh"))
        .current_dir(&root)
        .env("AO2_BIN", ao2)
        .env("AO2_PHASE1_PROMOTION_PREFLIGHT", "1")
        .env("AO2_PHASE1_PROMOTION_ROOT", temp.path().join("promotion"))
        .env("AO2_MACOS_GOVERNED_RUN_EVIDENCE", &macos_governed)
        .env("AO2_UBUNTU_GOVERNED_RUN_EVIDENCE", &ubuntu_governed)
        .env("AO2_WINDOWS_GOVERNED_RUN_EVIDENCE", &windows_governed)
        .env("AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY", &macos_project)
        .env("AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY", &ubuntu_project)
        .env("AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY", &windows_project)
        .env("AO2_PROVIDER_ACCEPTANCE_PRESERVATION", &provider_acceptance)
        .output()
        .expect("run phase1 governed-run-only promotion preflight");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("phase1_replacement_promotion_preflight=passed"));
    assert!(stdout.contains("replacement_smoke_mode=governed_run_primary"));
    assert!(stdout.contains("macos_governed_run_evidence="));
    assert!(stdout.contains("ubuntu_governed_run_evidence="));
    assert!(stdout.contains("windows_governed_run_evidence="));
    assert!(stdout.contains("macos_factory_project_run_summary="));
    assert!(stdout.contains("ubuntu_factory_project_run_summary="));
    assert!(stdout.contains("windows_factory_project_run_summary="));
    assert!(stdout.contains("phase1_evidence_bundle_archive="));
    assert!(stdout.contains("phase1_evidence_bundle_verification="));
}

#[test]
fn phase1_replacement_promotion_preflight_discovers_latest_project_run_summaries() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let macos_governed = temp.path().join("macos-governed-run.json");
    let ubuntu_governed = temp.path().join("ubuntu-governed-run.json");
    let windows_governed = temp.path().join("windows-governed-run.json");
    let provider_acceptance = temp.path().join("provider-acceptance-preservation.json");
    fs::write(&macos_governed, "{}").unwrap();
    fs::write(&ubuntu_governed, "{}").unwrap();
    fs::write(&windows_governed, "{}").unwrap();
    fs::write(&provider_acceptance, "{}").unwrap();

    let summary_root = temp.path().join("target");
    let macos_summary = summary_root
        .join("factory-project-run-smoke/20260529T010000Z/factory-project-run-summary.json");
    let ubuntu_old = summary_root.join(
        "morning-cross-os-readback/20260529T010000Z/ao2-ubuntu-nucx/factory-project-run-summary.json",
    );
    let ubuntu_summary = summary_root.join(
        "morning-cross-os-readback/20260529T020000Z/ao2-ubuntu-nucx/factory-project-run-summary.json",
    );
    let windows_summary = summary_root.join(
        "morning-cross-os-readback/20260529T020000Z/win-hp255-via-ubuntu/factory-project-run-summary.json",
    );
    for (path, host_os) in [
        (&macos_summary, "macos"),
        (&ubuntu_old, "ubuntu"),
        (&ubuntu_summary, "ubuntu"),
        (&windows_summary, "windows"),
    ] {
        fs::create_dir_all(path.parent().expect("summary parent")).unwrap();
        fs::write(
            path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "ao2.factory-project-run.v1",
                "host_os": host_os,
                "status": "accepted"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let output = Command::new(sh_command())
        .arg(root.join("scripts/phase1-replacement-promotion.sh"))
        .current_dir(&root)
        .env("AO2_BIN", ao2)
        .env("AO2_PHASE1_PROMOTION_PREFLIGHT", "1")
        .env("AO2_PHASE1_PROMOTION_ROOT", temp.path().join("promotion"))
        .env("AO2_PROJECT_RUN_SUMMARY_ROOT", &summary_root)
        .env("AO2_MACOS_GOVERNED_RUN_EVIDENCE", &macos_governed)
        .env("AO2_UBUNTU_GOVERNED_RUN_EVIDENCE", &ubuntu_governed)
        .env("AO2_WINDOWS_GOVERNED_RUN_EVIDENCE", &windows_governed)
        .env("AO2_PROVIDER_ACCEPTANCE_PRESERVATION", &provider_acceptance)
        .output()
        .expect("run phase1 promotion preflight with discovered summaries");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("phase1_replacement_promotion_preflight=passed"));
    assert!(output_contains_path_line(
        &stdout,
        "macos_factory_project_run_summary",
        &macos_summary
    ));
    assert!(output_contains_path_line(
        &stdout,
        "ubuntu_factory_project_run_summary",
        &ubuntu_summary
    ));
    assert!(output_contains_path_line(
        &stdout,
        "windows_factory_project_run_summary",
        &windows_summary
    ));
}

#[test]
fn phase1_replacement_promotion_preflight_materializes_verified_input_manifest() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let current_version = Command::new(sh_command())
        .arg(root.join("scripts/current-version.sh"))
        .current_dir(&root)
        .output()
        .expect("read current version");
    assert!(
        current_version.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&current_version.stdout),
        String::from_utf8_lossy(&current_version.stderr)
    );
    let current_version = String::from_utf8(current_version.stdout)
        .expect("current version is utf8")
        .trim()
        .to_string();
    let promotion_root = temp.path().join("promotion");
    let inputs_manifest = promotion_root.join("promotion-inputs.json");
    let inputs_verification = promotion_root.join("promotion-inputs-verification.json");
    let macos_governed = temp.path().join("macos-governed-run.json");
    let ubuntu_governed = temp.path().join("ubuntu-governed-run.json");
    let windows_governed = temp.path().join("windows-governed-run.json");
    let macos_project = temp.path().join("macos-factory-project-run-summary.json");
    let ubuntu_project = temp.path().join("ubuntu-factory-project-run-summary.json");
    let windows_project = temp.path().join("windows-factory-project-run-summary.json");
    let provider_acceptance = temp.path().join("provider-acceptance-preservation.json");
    for path in [
        &macos_governed,
        &ubuntu_governed,
        &windows_governed,
        &macos_project,
        &ubuntu_project,
        &windows_project,
        &provider_acceptance,
    ] {
        fs::write(path, "{}").unwrap();
    }

    let output = Command::new(sh_command())
        .arg(root.join("scripts/phase1-replacement-promotion.sh"))
        .current_dir(&root)
        .env("AO2_BIN", ao2)
        .env("AO2_PHASE1_PROMOTION_PREFLIGHT", "1")
        .env("AO2_PHASE1_PROMOTION_ROOT", &promotion_root)
        .env("AO2_MACOS_GOVERNED_RUN_EVIDENCE", &macos_governed)
        .env("AO2_UBUNTU_GOVERNED_RUN_EVIDENCE", &ubuntu_governed)
        .env("AO2_WINDOWS_GOVERNED_RUN_EVIDENCE", &windows_governed)
        .env("AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY", &macos_project)
        .env("AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY", &ubuntu_project)
        .env("AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY", &windows_project)
        .env("AO2_PROVIDER_ACCEPTANCE_PRESERVATION", &provider_acceptance)
        .output()
        .expect("run phase1 promotion preflight with verified input manifest");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!(
        "phase1_promotion_inputs={}",
        inputs_manifest.display()
    )));
    assert!(stdout.contains(&format!(
        "phase1_promotion_inputs_verification={}",
        inputs_verification.display()
    )));

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&inputs_manifest).expect("promotion inputs manifest exists"),
    )
    .expect("promotion inputs manifest is json");
    assert_eq!(
        manifest["schema_version"],
        "ao2.phase1-replacement-promotion-inputs.v1"
    );
    assert_eq!(manifest["release_version"], current_version);
    assert_eq!(manifest["replacement_smoke_mode"], "governed_run_primary");
    assert_eq!(
        manifest["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        manifest["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        manifest["inputs"]["governed_run_evidence"]["macos"]
            .as_str()
            .expect("macos governed path"),
        macos_governed.to_string_lossy().as_ref()
    );
    assert_eq!(
        manifest["inputs"]["factory_project_run_summary"]["windows"]
            .as_str()
            .expect("windows project summary path"),
        windows_project.to_string_lossy().as_ref()
    );
    assert_eq!(
        manifest["inputs"]["provider_acceptance_preservation"]
            .as_str()
            .expect("provider acceptance path"),
        provider_acceptance.to_string_lossy().as_ref()
    );

    let verification: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&inputs_verification).expect("promotion inputs verification exists"),
    )
    .expect("promotion inputs verification is json");
    assert_eq!(
        verification["schema_version"],
        "ao2.phase1-replacement-promotion-inputs-verification.v1"
    );
    assert_eq!(verification["status"], "accepted");
    assert_eq!(
        verification["manifest_path"]
            .as_str()
            .expect("manifest path"),
        inputs_manifest.to_string_lossy().as_ref()
    );
    assert_eq!(
        verification["missing_required_inputs"],
        serde_json::json!([])
    );
}

#[test]
fn release_operational_scripts_cover_three_os_ci_and_download_verification() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let three_os = fs::read_to_string(root.join("scripts/smoke-three-os-release.sh"))
        .expect("three-OS smoke script exists");
    let release_archives = fs::read_to_string(root.join("scripts/smoke-release-archives.sh"))
        .expect("release archive smoke script exists");
    let release_gate = fs::read_to_string(root.join("scripts/release-gate.sh"))
        .expect("release gate script exists");
    let release_ship = fs::read_to_string(root.join("scripts/release-ship.sh"))
        .expect("release ship script exists");
    let phase1_replacement_promotion =
        fs::read_to_string(root.join("scripts/phase1-replacement-promotion.sh"))
            .expect("Phase 1 replacement promotion script exists");
    let phase1_evidence_bundle_verify =
        fs::read_to_string(root.join("scripts/verify-phase1-evidence-bundle.sh"))
            .expect("Phase 1 evidence bundle verifier script exists");
    let phase1_control_plane_readback =
        fs::read_to_string(root.join("scripts/smoke-phase1-control-plane-readback.sh"))
            .expect("Phase 1 control-plane readback smoke script exists");
    let provider_registry_control_plane_readback =
        fs::read_to_string(root.join("scripts/smoke-provider-registry-control-plane.py"))
            .expect("provider-registry control-plane readback smoke script exists");
    let factory_greenfield_run =
        fs::read_to_string(root.join("scripts/smoke-factory-greenfield-run.sh"))
            .expect("factory greenfield run smoke script exists");
    let factory_app_run = fs::read_to_string(root.join("scripts/smoke-factory-app-run.sh"))
        .expect("factory app run smoke script exists");
    let factory_project_run = fs::read_to_string(root.join("scripts/smoke-factory-project-run.sh"))
        .expect("factory project run smoke script exists");
    let morning_cross_os_readback =
        fs::read_to_string(root.join("scripts/morning-dispatch-cross-os-readback.sh"))
            .expect("morning cross-os readback dispatch exists");
    let workbench_release_comparison_smoke =
        fs::read_to_string(root.join("scripts/smoke-workbench-release-comparison-export.sh"))
            .expect("Workbench release comparison export smoke script exists");
    let workbench_provider_pilot_acceptance_smoke = fs::read_to_string(
        root.join("scripts/smoke-workbench-provider-pilot-acceptance-export.sh"),
    )
    .expect("Workbench provider pilot acceptance export smoke script exists");
    let release_retention_preflight =
        fs::read_to_string(root.join("scripts/release-retention-preflight.sh"))
            .expect("release retention preflight script exists");
    let gate = fs::read_to_string(root.join("scripts/license-provenance-gate.sh"))
        .expect("license/provenance gate exists");
    let download = fs::read_to_string(root.join("scripts/release-download-verify.sh"))
        .expect("download verifier exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let phase1_promotion_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/phase1_promotion.rs"))
            .expect("Phase 1 promotion source exists");
    let workbench_render_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/workbench_render.rs"))
            .expect("Workbench render source exists");

    assert!(three_os.contains("AO2_WINDOWS_SSH_TARGET"));
    assert!(three_os.contains("AO2_UBUNTU_SSH_TARGET"));
    assert!(three_os.contains("AO2_LINUX_X86_64_ARCHIVE"));
    assert!(three_os.contains("AO2_LINUX_X86_64_SMOKE_MODE"));
    assert!(three_os.contains("linux_x86_64_remote_smoke"));
    assert!(three_os.contains("linux_x86_64_docker_smoke"));
    assert!(three_os.contains("macos_smoke_log"));
    assert!(three_os.contains("ubuntu_smoke_log"));
    assert!(three_os.contains("windows_static_smoke_log"));
    assert!(three_os.contains("AO2_RELEASE_SMOKE_LEG=macos"));
    assert!(three_os.contains("AO2_RELEASE_SMOKE_LEG=ubuntu"));
    assert!(three_os.contains("AO2_RELEASE_SMOKE_LEG=linux_x86_64"));
    assert!(three_os.contains("AO2_RELEASE_SMOKE_LEG=windows_static"));
    assert!(three_os.contains("\"macos_smoke\": macos_status"));
    assert!(three_os.contains("\"ubuntu_smoke\": ubuntu_status"));
    assert!(three_os.contains("\"windows_static_smoke\": windows_static_status"));
    assert!(three_os.contains("AO2_REQUIRE_NATIVE_WINDOWS_SMOKE"));
    assert!(three_os.contains("AO2_WINDOWS_SSH_ATTEMPTS"));
    assert!(three_os.contains("AO2_WINDOWS_SSH_CONNECT_TIMEOUT"));
    assert!(three_os.contains("AO2_LOCAL_SMOKE_TIMEOUT_SECONDS"));
    assert!(three_os.contains("AO2_WINDOWS_STEP_TIMEOUT_SECONDS"));
    assert!(three_os.contains("AO2_RELEASE_STEP_TIMEOUT_SECONDS"));
    assert!(three_os.contains("AO2_STEP_HEARTBEAT_SECONDS"));
    assert!(three_os.contains("AO2_WINDOWS_WAKE_MAC"));
    assert!(three_os.contains("AO2_WINDOWS_SSH_TARGET:-win-hp255-via-ubuntu"));
    assert!(!three_os.contains("AO2_WINDOWS_SSH_TARGET:-antho@10.0.0.96"));
    assert!(three_os.contains("AO2_WINDOWS_WAKE_BROADCAST"));
    assert!(three_os.contains("AO2_WINDOWS_WAKE_WAIT_SECONDS"));
    assert!(three_os.contains("windows_wake=sent"));
    assert!(three_os.contains(r"windows_wake=sent host=([^\s]+)"));
    assert!(!three_os.contains(r"windows_wake=sent host=([^\\s]+)"));
    assert!(three_os.contains("windows_ssh_probe=reachable"));
    assert!(three_os.contains("windows_ssh_probe=not_ready"));
    assert!(three_os.contains("run_logged_step()"));
    assert!(three_os.contains("status=running"));
    assert!(three_os.contains("status=timed_out"));
    assert!(three_os.contains("exit_code=124"));
    assert!(three_os.contains("local_smoke=timed_out"));
    assert!(three_os.contains("windows_execute=timed_out"));
    assert!(three_os.contains("release_gate=timed_out"));
    assert!(three_os.contains("subprocess.TimeoutExpired"));
    assert!(three_os.contains("ao2.three-os-smoke-summary.v1"));
    assert!(three_os.contains("summary.json"));
    assert!(three_os.contains("three_os_summary="));
    assert!(three_os.contains("summary-verification.err"));
    assert!(three_os.contains("release smoke-summary"));
    assert!(three_os.contains("release summary-enrich"));
    assert!(three_os.contains("release-obligation-gate.json"));
    assert!(three_os.contains("release-obligation-gate-signing-key.pem"));
    assert!(three_os.contains("workbench support-keygen"));
    assert!(three_os.contains("contract sign-obligation-gate"));
    assert!(three_os.contains("--support-signing-key"));
    assert!(three_os.contains("--support-signer-id"));
    assert!(three_os.contains("three_os_obligation_gate_signing="));
    assert!(three_os.contains("--obligation-gate"));
    assert!(three_os.contains("three_os_enriched_summary="));
    assert!(three_os.contains("release gate"));
    assert!(three_os.contains("release-gate.json"));
    assert!(three_os.contains("three_os_release_gate="));
    assert!(three_os.contains("native_windows_required"));
    assert!(three_os.contains("windows_skip_reason"));
    assert!(three_os.contains("scripts/smoke-release-archives.sh"));
    assert!(three_os.contains("scripts/smoke-windows-release.ps1"));
    assert!(three_os.contains("windows_native_smoke=skipped"));
    assert!(three_os.contains("windows_native_smoke=passed"));
    assert!(three_os.contains("native_windows_required"));
    assert!(three_os.contains("windows_required_failure=0"));
    assert!(three_os.contains("exit \"$windows_required_failure\""));
    assert!(three_os.contains("three_os_smoke=passed"));

    assert!(release_archives.contains("AO2_RELEASE_SMOKE_LEG"));
    assert!(release_archives.contains("should_run_release_smoke_leg()"));
    assert!(release_archives.contains("macos|ubuntu|linux_x86_64|windows_static|all"));
    assert!(release_archives.contains("AO2_RELEASE_SMOKE_JSON"));
    assert!(release_archives.contains("ao2.release-archive-smoke.v1"));
    assert!(release_archives.contains("install_verification_evidence"));
    assert!(release_archives.contains("ao2.install-verification-evidence.v1"));
    assert!(release_archives.contains("macos_install_smoke=passed"));
    assert!(release_archives.contains("ubuntu_install_smoke=passed"));
    assert!(release_archives.contains("windows_static_smoke=passed"));

    assert!(gate.contains("Apache-2.0"));
    assert!(gate.contains("THIRD-PARTY-LICENSES.md"));
    assert!(gate.contains("release:verify-provenance"));
    assert!(gate.contains("GPL"));
    assert!(gate.contains("license_provenance_gate=passed"));

    assert!(release_gate.contains("ao2 release gate"));
    assert!(release_gate.contains("AO2_SMOKE_SUMMARY"));
    assert!(release_gate.contains("summary.enriched.json"));
    assert!(release_gate.contains("--linux-x86-64-archive"));
    assert!(release_gate.contains("AO2_REPLACEMENT_SMOKE_GATE"));
    assert!(release_gate.contains("--replacement-smoke-gate"));
    assert!(release_gate.contains("AO2_GREENFIELD_THREE_OS_SMOKE_GATE"));
    assert!(release_gate.contains("--greenfield-three-os-smoke-gate"));
    assert!(release_gate.contains("AO2_MACOS_GOVERNED_RUN_EVIDENCE"));
    assert!(release_gate.contains("--governed-run-evidence"));
    assert!(release_gate.contains("AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY"));
    assert!(release_gate.contains("AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY"));
    assert!(release_gate.contains("AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY"));
    assert!(release_gate.contains("--factory-project-run-summary"));
    assert!(release_gate.contains("release-gate.json"));
    assert!(release_gate.contains("release_gate=passed"));

    assert!(phase1_replacement_promotion.contains("ao2 factory replacement-smoke-gate"));
    assert!(phase1_replacement_promotion.contains("AO2_MACOS_REPLACEMENT_SMOKE"));
    assert!(phase1_replacement_promotion.contains("AO2_UBUNTU_REPLACEMENT_SMOKE"));
    assert!(phase1_replacement_promotion.contains("AO2_WINDOWS_REPLACEMENT_SMOKE"));
    assert!(phase1_replacement_promotion.contains("AO2_MACOS_GOVERNED_RUN_EVIDENCE"));
    assert!(phase1_replacement_promotion.contains("AO2_UBUNTU_GOVERNED_RUN_EVIDENCE"));
    assert!(phase1_replacement_promotion.contains("AO2_WINDOWS_GOVERNED_RUN_EVIDENCE"));
    assert!(phase1_replacement_promotion.contains("--governed-run-evidence"));
    assert!(phase1_replacement_promotion.contains("AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY"));
    assert!(phase1_replacement_promotion.contains("AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY"));
    assert!(phase1_replacement_promotion.contains("AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY"));
    assert!(phase1_replacement_promotion.contains("AO2_PROJECT_RUN_SUMMARY_ROOT"));
    assert!(phase1_replacement_promotion.contains("factory-project-run-smoke"));
    assert!(phase1_replacement_promotion.contains("morning-cross-os-readback"));
    assert!(phase1_replacement_promotion.contains("AO2_PHASE1_PROMOTION_INPUTS"));
    assert!(phase1_replacement_promotion.contains("ao2.phase1-replacement-promotion-inputs.v1"));
    assert!(
        phase1_promotion_source.contains("ao2.phase1-replacement-promotion-inputs-verification.v1")
    );
    assert!(phase1_replacement_promotion.contains("release phase1-promotion-inputs-verify"));
    assert!(phase1_replacement_promotion.contains("verify_phase1_promotion_inputs decision-gate"));
    assert!(phase1_replacement_promotion.contains("--factory-project-run-summary"));
    assert!(phase1_replacement_promotion.contains("macos-factory-project-run-summary"));
    assert!(phase1_replacement_promotion.contains("ubuntu-factory-project-run-summary"));
    assert!(phase1_replacement_promotion.contains("windows-factory-project-run-summary"));
    assert!(phase1_replacement_promotion.contains("phase1-promotion-inputs"));
    assert!(phase1_replacement_promotion.contains("phase1-promotion-inputs-verification"));
    assert!(phase1_replacement_promotion.contains("AO2_PROVIDER_ACCEPTANCE_PRESERVATION"));
    assert!(phase1_replacement_promotion.contains("--provider-acceptance-preservation"));
    assert!(phase1_replacement_promotion.contains("AO2_REPLACEMENT_SMOKE_GATE"));
    assert!(phase1_replacement_promotion.contains("scripts/release-gate.sh"));
    assert!(phase1_replacement_promotion.contains("ao2 release phase1-decision-build"));
    assert!(phase1_replacement_promotion.contains("ao2 release phase1-promotion-inputs-publish"));
    assert!(phase1_replacement_promotion.contains("AO2_PHASE1_INPUTS_PUBLISH_OUT"));
    assert!(phase1_replacement_promotion.contains("ao2 release phase1-decision-publish"));
    assert!(phase1_replacement_promotion.contains("ao2 release evidence-bundle"));
    assert!(phase1_replacement_promotion.contains("ao2 release evidence-bundle-verify"));
    assert!(phase1_replacement_promotion.contains("AO2_PHASE1_PROMOTION_PUBLISH"));
    assert!(phase1_replacement_promotion.contains("AO2_PHASE1_PROMOTION_PREFLIGHT"));
    assert!(phase1_replacement_promotion.contains("AO2_BIN"));
    assert!(phase1_replacement_promotion.contains("phase1_replacement_promotion=passed"));
    assert!(phase1_replacement_promotion.contains("phase1_replacement_promotion_preflight=passed"));
    assert!(phase1_evidence_bundle_verify.contains("release evidence-bundle-verify"));
    assert!(phase1_evidence_bundle_verify.contains("AO2_PHASE1_EVIDENCE_BUNDLE"));
    assert!(phase1_evidence_bundle_verify.contains("phase1_evidence_bundle_verify=passed"));
    assert!(package_json.contains("\"phase1:replacement-promotion\""));
    assert!(package_json.contains("\"phase1:evidence-bundle-verify\""));
    assert!(package_json.contains("\"phase1:promotion-status\""));
    assert!(phase1_control_plane_readback.contains("release phase1-decision-publish"));
    assert!(phase1_control_plane_readback.contains("release phase1-history-fetch"));
    assert!(phase1_control_plane_readback.contains("/api/v1/phase1/promotion/decision/latest"));
    assert!(phase1_control_plane_readback.contains("/api/v1/phase1/promotion/dashboard.json"));
    assert!(phase1_control_plane_readback.contains("/api/v1/phase1/promotion/operator-panel.json"));
    assert!(phase1_control_plane_readback.contains("materialize_phase1_decision_fixture"));
    assert!(phase1_control_plane_readback.contains("release phase1-decision-build"));
    assert!(phase1_control_plane_readback.contains("ao2.factory-v3-compat-governed-run.v1"));
    assert!(phase1_control_plane_readback.contains("ao2.provider-pilot-acceptance-preservation.v1"));
    assert!(phase1_control_plane_readback.contains("signature_verified"));
    assert!(phase1_control_plane_readback.contains("governed_run_primary"));
    assert!(phase1_control_plane_readback.contains("phase1_control_plane_readback=passed"));
    assert!(provider_registry_control_plane_readback.contains("AO2_CP_API_TOKEN"));
    assert!(provider_registry_control_plane_readback.contains("ao2-cp-server"));
    assert!(
        provider_registry_control_plane_readback.contains("provider registry --control-plane-url")
    );
    assert!(provider_registry_control_plane_readback
        .contains("/api/v1/provider/registry/dashboard.json"));
    assert!(provider_registry_control_plane_readback.contains("/api/v1/provider/registry/latest"));
    assert!(provider_registry_control_plane_readback
        .contains("/api/v1/provider/registry/{sha}/detail.json"));
    assert!(provider_registry_control_plane_readback.contains("metadata_source"));
    assert!(provider_registry_control_plane_readback.contains("doctor_metadata_source"));
    assert!(provider_registry_control_plane_readback.contains("read_only_observer"));
    assert!(provider_registry_control_plane_readback.contains("provider_api_key_auth"));
    assert!(provider_registry_control_plane_readback.contains("TOKEN_REDACTED"));
    assert!(provider_registry_control_plane_readback.contains("timeout="));
    assert!(package_json.contains("\"smoke:phase1-control-plane-readback\""));
    assert!(factory_greenfield_run.contains("ao2 factory greenfield-run"));
    assert!(factory_greenfield_run.contains("fixtures/discount-service"));
    assert!(factory_greenfield_run.contains("factory_greenfield_run=passed"));
    assert!(factory_greenfield_run.contains("ao2.factory-greenfield-run.v1"));
    assert!(factory_greenfield_run.contains("factory_v3_drives_workflow"));
    assert!(factory_greenfield_run.contains("read_only_observer_after_signed_evidence"));
    assert!(factory_greenfield_run.contains("release_acceptance_owner"));
    assert!(package_json.contains("\"smoke:factory-greenfield-run\""));
    assert!(morning_cross_os_readback.contains("scripts/smoke-factory-greenfield-run.sh"));
    assert!(morning_cross_os_readback.contains("factory-greenfield-run-summary.json"));
    assert!(factory_app_run.contains("ao2 factory app-run"));
    assert!(factory_app_run.contains("ao2_cmd factory app-run-bundle"));
    assert!(factory_app_run.contains("fixtures/missed-call-recovery"));
    assert!(factory_app_run.contains("missed_call_recovery"));
    assert!(factory_app_run.contains("product_fixture: 'missed-call-recovery'"));
    assert!(factory_app_run.contains("product_domain: 'missed-call revenue recovery'"));
    assert!(factory_app_run.contains("LeadCapture"));
    assert!(factory_app_run.contains("test_recovery_message_mentions_customer_and_business"));
    assert!(factory_app_run.contains("test_hot_lead_score_prioritizes_recent_repeat_callers"));
    assert!(factory_app_run.contains("factory_app_run=passed"));
    assert!(factory_app_run.contains("app_run_bundle=passed"));
    assert!(factory_app_run.contains("ao2.factory-app-run.v1"));
    assert!(factory_app_run.contains("ao2.factory-app-run-bundle.v1"));
    assert!(factory_app_run.contains("release_review_artifacts_ready"));
    assert!(factory_app_run.contains("read_only_observer_after_signed_evidence"));
    assert!(factory_app_run.contains("release_acceptance_owner"));
    assert!(package_json.contains("\"smoke:factory-app-run\""));
    assert!(morning_cross_os_readback.contains("scripts/smoke-factory-app-run.sh"));
    assert!(morning_cross_os_readback.contains("factory-app-run-summary.json"));
    assert!(morning_cross_os_readback.contains("product_fixture"));
    assert!(morning_cross_os_readback.contains("app_run_bundle_status"));
    assert!(factory_project_run.contains("ao2 factory project-run"));
    assert!(factory_project_run.contains("ao2 factory project-acceptance-review"));
    assert!(factory_project_run.contains("factory queue-submit-project-start"));
    assert!(factory_project_run.contains("factory queue-run-next"));
    assert!(factory_project_run.contains("factory queue-project-start-complete"));
    assert!(factory_project_run.contains("factory queue-project-start-complete-status"));
    assert!(factory_project_run.contains("factory queue-project-start-next-action"));
    assert!(factory_project_run
        .contains("docs/contracts/hermes-project-start-poll-act-contract.v1.json"));
    assert!(factory_project_run.contains("factory queue-status"));
    assert!(factory_project_run.contains("factory queue-completion-contract"));
    assert!(factory_project_run.contains("factory queue-completion-contract-consume"));
    assert!(factory_project_run.contains("--latest-completed-project-start"));
    assert!(factory_project_run.contains("ao2.factory-queue-status.v1"));
    assert!(factory_project_run.contains("ao2.factory-project-start-queue-completion-contract.v1"));
    assert!(factory_project_run
        .contains("ao2.factory-project-start-queue-completion-contract-consumption.v1"));
    assert!(factory_project_run
        .contains("ao2 factory queue-run-next auto project-start closure packaging"));
    assert!(factory_project_run
        .contains("ao2 factory queue-run-next auto project-start closure verification"));
    assert!(factory_project_run.contains("ao2 factory replacement-packet"));
    assert!(factory_project_run.contains("ao2 factory replacement-packet-verify"));
    assert!(factory_project_run
        .contains("factory replacement packet must package replacement evidence"));
    assert!(factory_project_run.contains("factory replacement packet verification must accept"));
    assert!(factory_project_run.contains("ao2.factory-replacement-packet.v1"));
    assert!(factory_project_run.contains("ao2.factory-replacement-packet-verification.v1"));
    assert!(factory_project_run.contains("queued_replacement_packet_status"));
    assert!(factory_project_run.contains("queued_replacement_packet_verification_status"));
    assert!(factory_project_run
        .contains("queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver"));
    assert!(factory_project_run
        .contains("queued_replacement_packet_verification_ao2_replacement_driver_verified"));
    assert!(factory_project_run.contains("evaluator_closer_and_sampling_auditor"));
    assert!(factory_project_run.contains("ao2.factory-project-start-closure.v1"));
    assert!(factory_project_run.contains("ao2.factory-project-start-closure-verification.v1"));
    assert!(factory_project_run.contains("project_start_closure"));
    assert!(factory_project_run.contains("ao2 factory project-plan"));
    assert!(factory_project_run.contains("--signing-key \"$signing_key\""));
    assert!(factory_project_run.contains("factory-project-plan-smoke"));
    assert!(factory_project_run.contains("ao2 factory project-start"));
    assert!(factory_project_run.contains("factory project-start-bundle-verify"));
    assert!(factory_project_run.contains("--handoff-bundle-out"));
    assert!(factory_project_run.contains("--handoff-bundle-report"));
    assert!(factory_project_run.contains("factory project-plan-validate"));
    assert!(factory_project_run.contains("--project-plan"));
    assert!(factory_project_run.contains("--resume-from"));
    assert!(factory_project_run.contains("ao2.factory-project-plan.v1"));
    assert!(factory_project_run.contains("ao2.factory-acceptance-rubric.v1"));
    assert!(factory_project_run.contains("signed_acceptance_rubric"));
    assert!(factory_project_run.contains("acceptance_rubric_sha256"));
    assert!(factory_project_run.contains("ao2.factory-project-plan-validation.v1"));
    assert!(factory_project_run.contains("project_plan_generated_by_ao2"));
    assert!(factory_project_run.contains("factory-project-plan.json"));
    assert!(factory_project_run.contains("project_plan_validation_status"));
    assert!(factory_project_run.contains("project_start_status"));
    assert!(factory_project_run.contains("project_start_bundle_status"));
    assert!(factory_project_run.contains("project_start_bundle_verification_status"));
    assert!(factory_project_run.contains("project_start_bundle_review_signature_verified"));
    assert!(factory_project_run.contains("project-start-summary"));
    assert!(factory_project_run.contains("ao2.factory-project-start-operator-summary.v1"));
    assert!(factory_project_run.contains("project_start_operator_summary_status"));
    assert!(factory_project_run.contains("queued_project_start_bundle_verification_status"));
    assert!(factory_project_run.contains("queued_project_start_operator_summary_status"));
    assert!(factory_project_run.contains("queued_project_start_queue_status"));
    assert!(factory_project_run.contains("queued_project_start_queue_status_schema"));
    assert!(factory_project_run.contains("queued_project_start_queue_status_read_only"));
    assert!(factory_project_run.contains("queued_project_start_latest_queue_status"));
    assert!(factory_project_run.contains("queued_project_start_latest_queue_status_schema"));
    assert!(factory_project_run
        .contains("queued_project_start_latest_queue_status_matches_run_id_selector"));
    assert!(factory_project_run.contains("queued_project_start_completion_contract_status"));
    assert!(factory_project_run.contains("queued_project_start_completion_contract_one_read"));
    assert!(factory_project_run
        .contains("queued_project_start_completion_contract_requires_manual_closure_commands"));
    assert!(
        factory_project_run.contains("queued_project_start_completion_contract_consumer_status")
    );
    assert!(factory_project_run.contains("queued_project_start_completion_contract_consumer_ready"));
    assert!(factory_project_run
        .contains("queued_project_start_completion_contract_consumer_contract_only"));
    assert!(factory_project_run.contains("one_shot_project_start_status"));
    assert!(factory_project_run.contains("one_shot_project_start_ready_for_operator_review"));
    assert!(factory_project_run.contains("one_shot_project_start_contract_consumer_status"));
    assert!(factory_project_run.contains("ao2.factory-project-start-queue-complete.v1"));
    assert!(factory_project_run.contains("one_shot_project_start_probe_status"));
    assert!(factory_project_run.contains("one_shot_project_start_probe_record_state"));
    assert!(factory_project_run.contains("one_shot_project_start_probe_read_only"));
    assert!(factory_project_run.contains("one_shot_project_start_probe_would_execute_queue"));
    assert!(factory_project_run.contains("ao2.factory-project-start-queue-complete-status.v1"));
    assert!(factory_project_run.contains("ao2.factory-project-start-next-action.v1"));
    assert!(factory_project_run.contains("one_shot_project_start_next_action"));
    assert!(factory_project_run.contains("one_shot_project_start_next_action_value"));
    assert!(factory_project_run.contains("one_shot_project_start_next_action_read_only"));
    assert!(factory_project_run.contains("one_shot_project_start_next_action_probe_state"));
    assert!(factory_project_run.contains("queued_project_start_closure_status"));
    assert!(factory_project_run.contains("queued_project_start_closure_schema"));
    assert!(factory_project_run.contains("queued_project_start_closure_bundle"));
    assert!(factory_project_run.contains("project_start_closure_json_sha256"));
    assert!(factory_project_run
        .contains("queued_project_start_closure_latest_selector_matches_run_id_selector"));
    assert!(factory_project_run.contains("queued_project_start_closure_verification_status"));
    assert!(factory_project_run.contains("queued_project_start_closure_verification_schema"));
    assert!(factory_project_run
        .contains("queued_project_start_closure_verification_checksums_verified"));
    assert!(factory_project_run
        .contains("queued_project_start_closure_verification_trust_boundary_verified"));
    assert!(factory_project_run.contains("ao2_queue_packages_project_start_closure"));
    assert!(factory_project_run.contains("ao2_queue_verifies_project_start_closure"));
    assert!(factory_project_run.contains("ao2_queue_verifies_project_start_handoff_bundle"));
    assert!(factory_project_run.contains("queued_project_start_status"));
    assert!(factory_project_run.contains("ao2.factory-project-start-workbench-queue-run-next.v1"));
    assert!(factory_project_run.contains("ao2.hermes-project-start-handoff.v1"));
    assert!(factory_project_run.contains("ao2.factory-project-start-bundle.v1"));
    assert!(factory_project_run.contains("ao2.factory-project-start.v1"));
    assert!(factory_project_run.contains("project_plan_dispatched"));
    assert!(factory_project_run.contains("project_resume_state_reused"));
    assert!(factory_project_run.contains("ao2_preserved_partial_evidence"));
    assert!(factory_project_run.contains("missed-call-recovery-project"));
    assert!(factory_project_run.contains("ao2.factory-project-run.v1"));
    assert!(factory_project_run.contains("ao2.factory-project-acceptance-review.v1"));
    assert!(factory_project_run.contains("project_acceptance_review_status"));
    assert!(factory_project_run.contains("project_acceptance_review_recommended_decision"));
    assert!(factory_project_run.contains("project_acceptance_review_signature_status"));
    assert!(factory_project_run.contains("project_start_acceptance_review_status"));
    assert!(factory_project_run.contains("project_start_acceptance_review_recommended_decision"));
    assert!(factory_project_run.contains("project_start_acceptance_review_signature_status"));
    assert!(factory_project_run.contains("queued_project_acceptance_review_status"));
    assert!(factory_project_run.contains("queued_project_acceptance_review_recommended_decision"));
    assert!(factory_project_run.contains("project-acceptance-review"));
    assert!(factory_project_run.contains("release_review_package_ready"));
    assert!(factory_project_run.contains("read_only_observer_after_signed_evidence"));
    assert!(factory_project_run.contains("release_acceptance_owner"));
    assert!(factory_project_run.contains("app_run_count === 2"));
    assert!(factory_project_run.contains("factory_project_run=passed"));
    assert!(package_json.contains("\"smoke:factory-project-run\""));
    assert!(morning_cross_os_readback.contains("scripts/smoke-factory-project-run.sh"));
    assert!(morning_cross_os_readback.contains("factory-project-run-summary.json"));
    assert!(morning_cross_os_readback.contains("queued_project_start_latest_queue_status"));
    assert!(morning_cross_os_readback
        .contains("queued_project_start_latest_queue_status_matches_run_id_selector"));
    assert!(morning_cross_os_readback.contains("queued_project_start_closure_status"));
    assert!(morning_cross_os_readback.contains("queued_project_start_closure_schema"));
    assert!(morning_cross_os_readback
        .contains("queued_project_start_closure_latest_selector_matches_run_id_selector"));
    assert!(morning_cross_os_readback.contains("queued_project_start_closure_verification_status"));
    assert!(morning_cross_os_readback.contains("queued_project_start_closure_verification_schema"));
    assert!(morning_cross_os_readback
        .contains("queued_project_start_closure_verification_checksums_verified"));
    assert!(morning_cross_os_readback.contains("release_review_package_status"));
    assert!(morning_cross_os_readback.contains("project_acceptance_review_status"));
    assert!(morning_cross_os_readback.contains("project_acceptance_review_signature_status"));
    assert!(morning_cross_os_readback.contains("project_start_acceptance_review_status"));
    assert!(morning_cross_os_readback.contains("queued_project_acceptance_review_status"));
    assert!(morning_cross_os_readback.contains("project_start_bundle_verification_status"));
    assert!(morning_cross_os_readback.contains("queued_project_start_bundle_verification_status"));
    assert!(morning_cross_os_readback.contains("project_start_operator_summary_status"));
    assert!(morning_cross_os_readback.contains("queued_project_start_operator_summary_status"));
    assert!(morning_cross_os_readback.contains("queued_project_start_queue_status"));
    assert!(morning_cross_os_readback.contains("queued_project_start_queue_status_schema"));
    assert!(morning_cross_os_readback.contains("queued_project_start_queue_status_read_only"));
    assert!(morning_cross_os_readback.contains("queued_auto_replacement_packet_status"));
    assert!(
        morning_cross_os_readback.contains("queued_auto_replacement_packet_verification_status")
    );
    assert!(morning_cross_os_readback.contains("queued_replacement_packet_status"));
    assert!(morning_cross_os_readback.contains("queued_replacement_packet_verification_status"));
    assert!(morning_cross_os_readback
        .contains("queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver"));
    assert!(morning_cross_os_readback
        .contains("queued_replacement_packet_verification_ao2_replacement_driver_verified"));
    assert!(morning_cross_os_readback
        .contains("queued_replacement_packet_verification_factory_v3_evaluator_closer_verified"));

    assert!(download.contains("gh release download"));
    assert!(download.contains("release-verify-provenance.sh"));
    assert!(download.contains("AO2_LINUX_X86_64_ARCHIVE"));
    assert!(download.contains("AO2_LINUX_X86_64_SMOKE_MODE"));
    assert!(download.contains("scripts/smoke-release-archives.sh"));
    assert!(download.contains("linux_x86_64_docker_smoke=passed"));
    assert!(download.contains("AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY"));
    assert!(download.contains("AO2_RELEASE_ROLLBACK_VERIFY"));
    assert!(download.contains("release-rollback-summary.json"));
    assert!(download.contains("ao2.release-rollback-summary.v1"));
    assert!(download.contains("\"$AO2_ROLLBACK_SEED_BIN\" install update"));
    assert!(download.contains(
        "\"$AO2_ROLLBACK_SEED_BIN\" install rollback --install-dir \"$macos_install_dir\""
    ));
    assert!(download.contains("macos_download_rollback_runner="));
    assert!(download.contains("macos_download_rollback=passed"));
    assert!(download.contains("scripts/smoke-windows-release.ps1"));
    assert!(download.contains("windows_download_verify=passed"));
    assert!(download.contains("windows_download_rollback=passed"));
    assert!(download.contains("ubuntu_download_rollback=passed"));
    assert!(download.contains("release_download_verify=passed"));

    assert!(release_ship.contains("AO2_RELEASE_COMPARISON_DIR"));
    assert!(release_ship.contains("AO2_LINUX_X86_64_SMOKE_MODE"));
    assert!(release_ship.contains(
        "env -u AO2_UBUNTU_SSH_TARGET -u AO2_LINUX_X86_64_SSH_TARGET npm run release:build-all"
    ));
    assert!(release_ship.contains("npm run release:retention-preflight"));
    assert!(release_ship.contains("AO2_RELEASE_RETENTION_KEEP_RELEASES"));
    assert!(release_ship.contains("AO2_RELEASE_RETENTION_KEEP_BUNDLES"));
    assert!(release_ship.contains(
        r#"AO2_RELEASE_RETENTION_KEEP_RELEASES="${AO2_RELEASE_RETENTION_KEEP_RELEASES:-3}""#
    ));
    assert!(release_ship.contains(
        r#"AO2_RELEASE_RETENTION_KEEP_BUNDLES="${AO2_RELEASE_RETENTION_KEEP_BUNDLES:-3}""#
    ));
    assert!(release_ship.contains("AO2_RELEASE_COMPARISON_RESULT"));
    assert!(release_ship.contains("AO2_RELEASE_COMPARISON_VERIFICATION"));
    assert!(release_ship.contains("release compare"));
    assert!(release_ship.contains("release compare-verify"));
    assert!(release_ship.contains("npm run smoke:workbench-release-comparison-export"));
    assert!(release_ship.contains("npm run smoke:workbench-provider-pilot-acceptance-export"));
    assert!(release_ship.contains("AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE"));
    assert!(release_ship.contains("AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR"));
    assert!(release_ship.contains("AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON"));
    assert!(release_ship.contains("workbench_release_comparison_export=passed"));
    assert!(release_ship.contains("workbench_provider_pilot_acceptance_export=passed"));
    assert!(release_ship.contains("release_comparison_verify=passed"));
    assert!(release_ship.contains(".release-signing/ao2-release-signing-key.pem"));
    assert!(!workbench_release_comparison_smoke.contains("openssl"));
    assert!(!workbench_provider_pilot_acceptance_smoke.contains("openssl"));
    assert!(workbench_release_comparison_smoke.contains("workbench support-keygen"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("workbench support-keygen"));

    assert!(workbench_release_comparison_smoke.contains("kind=release-comparison-verification"));
    assert!(workbench_release_comparison_smoke.contains("/api/runs/evidence/export"));
    assert!(workbench_release_comparison_smoke.contains("/api/queue/export-preview"));
    assert!(workbench_release_comparison_smoke.contains("/api/queue/start"));
    assert!(workbench_release_comparison_smoke.contains("release-redaction-canary"));
    assert!(workbench_release_comparison_smoke.contains("ao2.workbench-support-bundle-preview.v1"));
    assert!(
        workbench_release_comparison_smoke.contains("ao2.workbench-support-redaction-preview.v1")
    );
    assert!(workbench_release_comparison_smoke.contains(".redaction_audit.redaction_count > 0"));
    assert!(workbench_release_comparison_smoke.contains("provider_api_key == 1"));
    assert!(workbench_release_comparison_smoke.contains("/api/queue/export"));
    assert!(workbench_release_comparison_smoke.contains("workbench support-inspect"));
    assert!(
        workbench_release_comparison_smoke.contains("AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR")
    );
    assert!(workbench_release_comparison_smoke.contains("AO2_WORKBENCH_RELEASE_COMPARISON_ROOT"));
    assert!(workbench_release_comparison_smoke.contains("rm -f \"$signing_key\""));
    assert!(
        workbench_release_comparison_smoke.contains("workbench_release_comparison_export=passed")
    );
    assert!(workbench_provider_pilot_acceptance_smoke.contains("kind=provider-pilot-acceptance"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("/api/runs/evidence/export"));
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("/api/provider-pilot/acceptance/export-latest"));
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("ao2.workbench-provider-pilot-acceptance-export-latest.v1"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("history_replay_status=accepted"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("history_min_score=90"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("history_sort=score_desc"));
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("ao2.workbench-provider-pilot-acceptance-trend.v1"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("/api/provider-pilot/cost-ledger"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("ao2.provider-cost-ledger.v1"));
    assert!(
        workbench_provider_pilot_acceptance_smoke.contains("workbench_provider_pilot_cost_ledger=")
    );
    assert!(workbench_provider_pilot_acceptance_smoke.contains("/api/provider-pilot/cost-trend"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("ao2.provider-cost-trend.v1"));
    assert!(
        workbench_provider_pilot_acceptance_smoke.contains("workbench_provider_pilot_cost_trend=")
    );
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("workbench-provider-pilot-dashboard.html"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("provider-pilot-cost-trend-chart"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("Provider pilot cost trend chart"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains(r#""evidence_export_count": 2"#));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("/api/queue/export-preview"));
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("ao2.workbench-support-bundle-preview.v1"));
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("ao2.workbench-support-redaction-preview.v1"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("/api/queue/export"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("workbench support-inspect"));
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE"));
    assert!(workbench_provider_pilot_acceptance_smoke
        .contains("workbench_provider_pilot_acceptance_export=passed"));
    assert!(workbench_provider_pilot_acceptance_smoke.contains("rm -f \"$signing_key\""));

    assert!(release_retention_preflight.contains("target/release-download"));
    assert!(release_retention_preflight.contains("target/release-comparison-bundles"));
    assert!(release_retention_preflight.contains("AO2_RELEASE_RETENTION_PRUNE"));
    assert!(release_retention_preflight.contains("AO2_RELEASE_RETENTION_KEEP_RELEASES"));
    assert!(release_retention_preflight.contains("AO2_RELEASE_RETENTION_KEEP_BUNDLES"));
    assert!(release_retention_preflight.contains(
        r#"AO2_RELEASE_RETENTION_KEEP_RELEASES="${AO2_RELEASE_RETENTION_KEEP_RELEASES:-3}""#
    ));
    assert!(release_retention_preflight.contains(
        r#"AO2_RELEASE_RETENTION_KEEP_BUNDLES="${AO2_RELEASE_RETENTION_KEEP_BUNDLES:-3}""#
    ));
    assert!(release_retention_preflight.contains("release_retention_preflight=passed"));
    assert!(release_retention_preflight.contains("release_retention_removed_total="));
    assert!(release_retention_preflight.contains("-mindepth 1"));
    assert!(release_retention_preflight.contains("rm -rf -- \"$path\""));
    assert!(release_retention_preflight.contains("case \"$path\" in"));
    assert!(workbench_render_source
        .contains("Keep Releases<input id=\"release-retention-keep-releases\" value=\"3\">"));
    assert!(workbench_render_source
        .contains("Keep Bundles<input id=\"release-retention-keep-bundles\" value=\"3\">"));

    assert!(package_json.contains("\"smoke:three-os\""));
    assert!(package_json.contains("\"smoke:workbench-release-comparison-export\""));
    assert!(package_json.contains("\"smoke:workbench-provider-pilot-acceptance-export\""));
    assert!(package_json.contains("\"release:retention-preflight\""));
    assert!(package_json.contains("\"release:gate\""));
    assert!(package_json.contains("\"package:linux:x86_64:docker\""));
    assert!(package_json.contains("\"ci:license-provenance\""));
    assert!(package_json.contains("\"release:download-verify\""));
}

#[test]
fn install_guide_documents_release_comparison_bundles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let install = fs::read_to_string(root.join("docs/INSTALL.md")).expect("install guide exists");

    assert!(install.contains("ao2 release compare"));
    assert!(install.contains("ao2.release-comparison-bundle.v1"));
    assert!(install.contains("ao2.release-comparison-metadata.v1"));
    assert!(install.contains("ao2 release compare-verify"));
    assert!(install.contains("ao2.release-comparison-verification.v1"));
    assert!(install.contains("POST /api/release-comparison"));
    assert!(install.contains("GET /api/release-comparison/verify"));
    assert!(install.contains("GET  /api/release-comparison/latest"));
    assert!(install.contains("ao2.workbench-latest-release-comparison.v1"));
    assert!(install.contains("POST /api/release-retention/prune"));
    assert!(install.contains("ao2.workbench-release-retention-prune.v1"));
    assert!(install.contains("--release-download-dir target/release-download"));
    assert!(install.contains("--signing-key .release-signing/ao2-release-signing-key.pem"));
    assert!(install.contains("--support-signing-key"));
    assert!(install.contains("--bundle-dir target/release-comparison-bundles"));
    assert!(install.contains("release:retention-preflight"));
    assert!(install.contains("AO2_RELEASE_RETENTION_KEEP_RELEASES"));
    assert!(install.contains("keeps the newest three"));
    assert!(install.contains("kind=provider-pilot-acceptance"));
    assert!(install.contains("AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE"));
    assert!(install.contains("smoke:workbench-provider-pilot-acceptance-export"));
    assert!(install.contains("release:preserve-provider-acceptance"));
    assert!(install.contains("ao2.provider-pilot-acceptance-preservation.v1"));
}

#[test]
fn workbench_operator_packet_control_plane_smoke_is_release_wired() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let smoke_script =
        fs::read_to_string(root.join("scripts/smoke-workbench-operator-packet-control-plane.sh"))
            .expect("workbench operator packet smoke script exists");
    let observer_gate =
        fs::read_to_string(root.join("scripts/control-plane-observer-hardening.sh"))
            .expect("observer hardening script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");
    let install = fs::read_to_string(root.join("docs/INSTALL.md")).expect("install docs exist");

    assert!(package_json.contains("\"smoke:workbench-operator-packet-control-plane\""));
    assert!(package_json.contains("scripts/smoke-workbench-operator-packet-control-plane.sh"));
    assert!(smoke_script.contains("ao2.workbench-operator-packet-control-plane-smoke.v1"));
    assert!(smoke_script.contains("workbench serve"));
    assert!(smoke_script.contains("kind=operator-packet"));
    assert!(smoke_script.contains("/api/runs/evidence/publish"));
    assert!(smoke_script.contains("/api/v1/operator-packet/signed"));
    assert!(smoke_script.contains("/api/v1/operator-packet/dashboard.json"));
    assert!(smoke_script.contains("/api/v1/operator-packet/run/$RUN_ID/latest"));
    assert!(smoke_script.contains("ao2.operator-evidence-packet.v1"));
    assert!(smoke_script.contains("ao2.evidence-pack.v1"));
    assert!(smoke_script.contains("token_leak_detected"));
    assert!(smoke_script.contains("env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY"));
    assert!(smoke_script.contains("python_command()"));
    assert!(smoke_script.contains("command -v python3"));
    assert!(smoke_script.contains("command -v python"));
    assert!(smoke_script.contains("exe_suffix()"));
    assert!(smoke_script.contains("MINGW*"));
    assert!(smoke_script.contains("MSYS*"));
    assert!(smoke_script.contains("CYGWIN*"));
    assert!(smoke_script.contains("binary_path()"));
    assert!(smoke_script.contains("AO2_BIN=\"$(binary_path"));
    assert!(smoke_script.contains("CP_SERVER_BIN=\"$(binary_path"));
    assert!(observer_gate.contains("workbench_operator_packet_control_plane_smoke"));
    assert!(observer_gate.contains("AO2_WORKBENCH_OPERATOR_PACKET_CP_SMOKE_ROOT"));
    assert!(observer_gate.contains("smoke:workbench-operator-packet-control-plane"));
    assert!(verification.contains("workbench operator-packet control-plane smoke"));
    assert!(verification.contains("ao2.workbench-operator-packet-control-plane-smoke.v1"));
    assert!(install.contains("smoke:workbench-operator-packet-control-plane"));
}

#[test]
fn risky_pr_golden_path_requires_static_report_index() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let script = fs::read_to_string(root.join("scripts/risky-pr-golden-path.sh"))
        .expect("risky pr golden path script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    assert!(package_json.contains("\"risky-pr:golden\""));
    assert!(script.contains("REPORT_INDEX=\"$OUT_ROOT/cockpit/index.report.json\""));
    assert!(script.contains("ao2.risky-pr-static-report-index.v1"));
    assert!(script.contains("operator_answers"));
    assert!(script.contains("approval_boundary"));
    assert!(script.contains("denied_request_digests"));
    assert!(script.contains("approved_action_digests"));
    assert!(script.contains("required_report_sections"));
    assert!(script.contains("present_report_sections"));
    assert!(script.contains("report_contract_complete"));
    assert!(script.contains("report verify"));
    assert!(script.contains("ao2.report-contract-verification.v1"));
    assert!(script.contains("Request Digest"));
    assert!(script.contains("Action Digest"));
    assert!(script.contains("\"denied_actions\""));
    assert!(script.contains("\"approved_actions\""));
    assert!(script.contains("\"test_evidence\""));
    assert!(script.contains("\"closure_verdict\""));
    assert!(script.contains("\"replay_status\""));
    assert!(script.contains("\"report_contract\""));
    assert!(script.contains("report index missing required section"));
    assert!(verification.contains("ao2.risky-pr-static-report-index.v1"));
    assert!(verification.contains("without filesystem archaeology"));
    assert!(verification.contains("required report sections"));
}

#[test]
fn risky_pr_golden_path_builds_release_support_bundle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/risky-pr-golden-path.sh"))
        .expect("risky pr golden path script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    for needle in [
        "RELEASE_SUPPORT_INPUTS=\"$OUT_ROOT/release-support-inputs\"",
        "RELEASE_SUPPORT_BUNDLE_DIR=\"$OUT_ROOT/release-support-bundle\"",
        "release support-bundle-build",
        "--report-target \"$TARGET\"",
        "--report-run-id \"$RUN_ID\"",
        "--report \"$REPORT\"",
        "--report-index \"$REPORT_INDEX\"",
        "--install-verification \"$RELEASE_SUPPORT_INPUTS/install-verification.json\"",
        "--hosted-release-smoke \"$RELEASE_SUPPORT_INPUTS/hosted-release-smoke.json\"",
        "\"install_verification\"",
        "\"hosted_release_smoke\"",
        "ao2.install-verification-evidence.v1",
        "ao2.release-archive-hosted-smoke.v1",
        "release-support-bundle.json",
        "SHA256SUMS",
        "\"release_support_bundle\"",
        "\"release_support_checksums\"",
        "\"release_support_bundle_sha256\"",
        "\"release_support_bundle_verification_status\"",
    ] {
        assert!(
            script.contains(needle),
            "missing golden-path support bundle marker: {needle}"
        );
    }
    assert!(verification.contains("release support bundle"));
    assert!(verification.contains("ao2.cp-release-support-bundle.v1"));
    assert!(verification.contains("ao2.release-support-bundle-build.v1"));
}

#[test]
fn cli_release_support_bundle_build_embeds_hosted_release_smoke_evidence() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let out_dir = root.join("support-bundle");

    let candidate_correlation = serde_json::json!({
        "status": "matched",
        "blockers": []
    });
    let trust_boundary = serde_json::json!({
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "control_plane_role": "read_only_observer",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false
    });
    let surfaces = [
        (
            "release-assembly.json",
            serde_json::json!({
                "schema_version": "ao2.cp-release-assembly.v1",
                "status": "assembled",
                "candidate_correlation": "matched",
                "candidate_correlation_detail": candidate_correlation,
                "control_plane_approves_release": false
            }),
        ),
        (
            "readiness.json",
            serde_json::json!({
                "schema_version": "ao2.cp-release-readiness.v1",
                "status": "ready",
                "candidate_correlation": candidate_correlation,
                "operator_decision": {
                    "factory_v3_evaluator_closer_required": true,
                    "control_plane_approves_release": false,
                    "release_acceptance_owner": "factory-v3 evaluator-closer"
                }
            }),
        ),
        (
            "handoff.json",
            serde_json::json!({
                "schema_version": "factory-v3/ao2-release-handoff-checklist/v1",
                "status": "ready_for_evaluator_closer",
                "candidate_correlation": candidate_correlation,
                "trust_boundary": trust_boundary
            }),
        ),
        (
            "cockpit.json",
            serde_json::json!({
                "schema_version": "ao2.cp-release-cockpit.v1",
                "status": "ready",
                "candidate_correlation": candidate_correlation
            }),
        ),
        (
            "evaluator-decision.json",
            serde_json::json!({
                "schema_version": "factory-v3/ao2-release-evaluator-decision/v1",
                "status": "accepted",
                "trust_boundary": trust_boundary
            }),
        ),
        (
            "storage-support.json",
            serde_json::json!({
                "schema_version": "ao2.cp-storage-support.v1",
                "status": "ready"
            }),
        ),
        (
            "replay.json",
            serde_json::json!({
                "schema_version": "ao2.replay.v1",
                "status": "accepted",
                "digest_failures": []
            }),
        ),
        (
            "report-contract-verification.json",
            serde_json::json!({
                "schema_version": "ao2.report-contract-verification.v1",
                "contract_schema_version": "ao2.report-contract.v1",
                "status": "passed",
                "complete": true,
                "missing_sections": [],
                "failures": []
            }),
        ),
        (
            "install-verification.json",
            serde_json::json!({
                "schema_version": "ao2.install-verification-evidence.v1",
                "status": "verified",
                "offline_verification": {"status": "verified"},
                "provider_api_keys_required": false,
                "control_plane_approves_release": false,
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }),
        ),
        (
            "hosted-release-smoke.json",
            serde_json::json!({
                "schema_version": "ao2.release-archive-hosted-smoke.v1",
                "status": "passed",
                "target": "linux-x86_64",
                "install_verification_schema": "ao2.install-verification-evidence.v1",
                "install_verification_evidence": "target/release-archive-hosted-smoke/ubuntu-latest/bin/ao2.install-verification.json",
                "provider_api_keys_required": false,
                "control_plane_approves_release": false,
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }),
        ),
        (
            "operator-evidence.json",
            serde_json::json!({
                "factory_v3_evaluator_closer_required": true,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false
            }),
        ),
    ];
    for (name, json) in surfaces {
        fs::write(
            root.join(name),
            serde_json::to_string_pretty(&json).expect("surface json"),
        )
        .expect("write support bundle surface");
    }

    let output = Command::new(ao2)
        .args([
            "release",
            "support-bundle-build",
            "--release-assembly",
            root.join("release-assembly.json").to_str().unwrap(),
            "--readiness",
            root.join("readiness.json").to_str().unwrap(),
            "--handoff",
            root.join("handoff.json").to_str().unwrap(),
            "--cockpit",
            root.join("cockpit.json").to_str().unwrap(),
            "--evaluator-decision",
            root.join("evaluator-decision.json").to_str().unwrap(),
            "--storage-support",
            root.join("storage-support.json").to_str().unwrap(),
            "--replay",
            root.join("replay.json").to_str().unwrap(),
            "--report-contract-verification",
            root.join("report-contract-verification.json")
                .to_str()
                .unwrap(),
            "--install-verification",
            root.join("install-verification.json").to_str().unwrap(),
            "--hosted-release-smoke",
            root.join("hosted-release-smoke.json").to_str().unwrap(),
            "--operator-evidence",
            root.join("operator-evidence.json").to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run ao2 release support-bundle-build");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let build: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("support bundle build prints json");
    assert_eq!(build["status"], "built");
    assert_eq!(build["verification"]["status"], "passed");
    assert_eq!(
        build["verification"]["hosted_release_smoke"]["schema_version"],
        "ao2.release-archive-hosted-smoke.v1"
    );

    let bundle: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(out_dir.join("release-support-bundle.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        bundle["hosted_release_smoke"]["schema_version"],
        "ao2.release-archive-hosted-smoke.v1"
    );
    assert_eq!(bundle["hosted_release_smoke"]["status"], "passed");
    assert_eq!(
        bundle["hosted_release_smoke"]["install_verification_schema"],
        "ao2.install-verification-evidence.v1"
    );
    assert_eq!(
        bundle["hosted_release_smoke"]["provider_api_keys_required"],
        false
    );
    assert_eq!(
        bundle["hosted_release_smoke"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        bundle["hosted_release_smoke"]["mutates_ao_artifacts"],
        false
    );
    let surfaces = bundle["portable_bundle_manifest"]["included_surfaces"]
        .as_array()
        .expect("included surfaces");
    assert!(
        surfaces
            .iter()
            .any(|surface| surface["id"] == "hosted_release_smoke"
                && surface["path"] == "$.hosted_release_smoke"
                && surface["schema_version"] == "ao2.release-archive-hosted-smoke.v1"),
        "portable bundle manifest must include hosted_release_smoke surface"
    );
}

#[test]
fn risky_pr_golden_ci_uploads_release_support_bundle_artifacts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("ci workflow exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    for needle in [
        "risky-pr-golden-artifacts:",
        "name: Risky PR golden release support bundle artifacts",
        "AO2_RISKY_PR_GOLDEN_ROOT=target/risky-pr-golden-ci",
        "npm run risky-pr:golden",
        "target/risky-pr-golden-ci/summary.json",
        "target/risky-pr-golden-ci/release-support-bundle-build.json",
        "target/risky-pr-golden-ci/release-support-bundle/release-support-bundle.json",
        "target/risky-pr-golden-ci/release-support-bundle/SHA256SUMS",
        "repository: uesugitorachiyo/ao2-control-plane",
        "path: ao2-control-plane",
        "python3 ao2-control-plane/scripts/verify_release_support_bundle.py",
        "--checksums target/risky-pr-golden-ci/release-support-bundle/SHA256SUMS",
        "target/risky-pr-golden-ci/release-support-bundle/release-support-bundle.json",
        "> target/risky-pr-golden-ci/release-support-bundle-control-plane-verify.json",
        "target/risky-pr-golden-ci/release-support-bundle-control-plane-verify.json",
        "ao2-risky-pr-golden-release-support-bundle",
    ] {
        assert!(
            ci.contains(needle),
            "missing risky-pr golden CI marker: {needle}"
        );
    }
    assert!(verification.contains("Risky PR golden release support bundle artifacts"));
    assert!(verification.contains("ao2-risky-pr-golden-release-support-bundle"));
    assert!(verification.contains("ao2-control-plane offline verifier"));
}

#[test]
fn ci_compares_shared_release_support_fixture_with_control_plane() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("ci workflow exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    for needle in [
        "release-support-fixture-parity:",
        "name: Release support fixture parity with ao2-control-plane",
        "repository: uesugitorachiyo/ao2-control-plane",
        "path: ao2-control-plane",
        "Use matching ao2-control-plane branch when present",
        "refs/heads/${head_ref}",
        "checkout --detach FETCH_HEAD",
        "cmp -s ao2/tests/fixtures/release-support-bundle-contract-v1.json ao2-control-plane/tests/fixtures/release-support-bundle-contract-v1.json",
        "shasum -a 256 ao2/tests/fixtures/release-support-bundle-contract-v1.json ao2-control-plane/tests/fixtures/release-support-bundle-contract-v1.json",
        "target/release-support-fixture-parity/summary.json",
        "ao2-release-support-fixture-parity",
    ] {
        assert!(
            ci.contains(needle),
            "missing release-support fixture parity CI marker: {needle}"
        );
    }
    assert!(verification.contains("Release support fixture parity with ao2-control-plane"));
    assert!(verification.contains("ao2-release-support-fixture-parity"));
    assert!(verification.contains("same branch name in both repositories"));
    assert!(verification.contains("strict CI evidence family"));
}

#[test]
fn risky_pr_golden_path_indexes_uploaded_release_support_bundle_artifacts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/risky-pr-golden-path.sh"))
        .expect("risky pr golden path script exists");
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("ci workflow exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    for needle in [
        "ARTIFACT_MANIFEST=\"$OUT_ROOT/artifact-manifest.json\"",
        "ao2.risky-pr-golden-artifact-manifest.v1",
        "\"artifact_manifest\"",
        "\"artifact_count\"",
        "\"sha256\"",
        "\"summary.json\"",
        "\"report-verify.json\"",
        "\"release-support-bundle-build.json\"",
        "\"release-support-bundle/release-support-bundle.json\"",
        "\"release-support-bundle/SHA256SUMS\"",
        "\"cockpit/index.report.json\"",
    ] {
        assert!(
            script.contains(needle),
            "missing risky-pr golden artifact manifest marker: {needle}"
        );
    }
    assert!(ci.contains("target/risky-pr-golden-ci/artifact-manifest.json"));
    assert!(verification.contains("ao2.risky-pr-golden-artifact-manifest.v1"));
    assert!(verification.contains("artifact-manifest.json"));
}

#[test]
fn evaluator_closure_corpus_is_release_wired() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let script = fs::read_to_string(root.join("scripts/evaluator-closure-corpus.sh"))
        .expect("evaluator closure corpus script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    assert!(package_json.contains("\"evaluator:closure-corpus\""));
    assert!(package_json.contains("scripts/evaluator-closure-corpus.sh"));
    assert!(script.contains("ao2.evaluator-closure-corpus.v1"));
    assert!(script.contains("missing_test_evidence"));
    assert!(script.contains("unresolved_high_concern"));
    assert!(script.contains("invalid_artifact_digest"));
    assert!(script.contains("unapproved_risky_action"));
    assert!(script.contains("accepted_after_correction"));
    assert!(script.contains("env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY"));
    assert!(verification.contains("evaluator:closure-corpus"));
    assert!(verification.contains("ao2.evaluator-closure-corpus.v1"));
}

#[test]
fn exact_digest_approval_gate_is_release_wired() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let script = fs::read_to_string(root.join("scripts/exact-digest-approval-gate.sh"))
        .expect("exact digest approval gate script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    assert!(package_json.contains("\"approval:exact-digest-gate\""));
    assert!(package_json.contains("scripts/exact-digest-approval-gate.sh"));
    assert!(script.contains("ao2.exact-digest-approval-gate.v1"));
    assert!(script.contains("broad_action_denied"));
    assert!(script.contains("exact_action_approved"));
    assert!(script.contains("modified_digest_rejected"));
    assert!(script.contains("report_exposes_digest_boundary"));
    assert!(script.contains("env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY"));
    assert!(verification.contains("approval:exact-digest-gate"));
    assert!(verification.contains("ao2.exact-digest-approval-gate.v1"));
}

#[test]
fn release_evidence_closure_requires_digest_boundary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let script = fs::read_to_string(root.join("scripts/release-evidence-closure.sh"))
        .expect("release evidence closure script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    assert!(package_json.contains("\"release:evidence-closure\""));
    assert!(script.contains("AO2_RISKY_PR_GOLDEN_ROOT"));
    assert!(script.contains("risky-pr:golden"));
    assert!(script.contains("approval_boundary"));
    assert!(script.contains("denied_request_digests"));
    assert!(script.contains("approved_action_digests"));
    assert!(script.contains("digest_failure_count"));
    assert!(script.contains("evidence_before_closure"));
    assert!(script.contains("ao2.release-evidence-closure.v1"));
    assert!(verification.contains("release:evidence-closure"));
    assert!(verification.contains("approval_boundary"));
    assert!(verification.contains("digest-boundary evidence"));
}

#[test]
fn release_evidence_closure_rejects_missing_digest_boundary_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/release-evidence-closure.sh"))
        .expect("release evidence closure script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exist");

    assert!(script.contains("AO2_RELEASE_EVIDENCE_CLOSURE_FIXTURE"));
    assert!(script.contains("missing_digest_boundary"));
    assert!(script.contains("pop(\"approval_boundary\", None)"));
    assert!(script.contains("risky-pr report index missing approval_boundary"));
    assert!(script.contains("release_evidence_closure_fixture=missing_digest_boundary"));
    assert!(script.contains("payload[\"status\"] != \"accepted\""));
    assert!(verification.contains("missing_digest_boundary"));
    assert!(verification.contains("fail-closed"));
}

#[test]
fn workbench_operator_packet_control_plane_smoke_index_is_release_wired() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let index_script = fs::read_to_string(
        root.join("scripts/workbench-operator-packet-control-plane-smoke-index.sh"),
    )
    .expect("workbench operator packet smoke index script exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification docs exists");

    assert!(package_json.contains("\"smoke:workbench-operator-packet-control-plane:index\""));
    assert!(package_json.contains("scripts/workbench-operator-packet-control-plane-smoke-index.sh"));
    assert!(index_script.contains("ao2.workbench-operator-packet-control-plane-smoke-index.v1"));
    assert!(index_script.contains("AO2_WORKBENCH_OPERATOR_PACKET_CP_INDEX_REQUIRED_OS"));
    assert!(index_script.contains("ubuntu-latest,macos-latest,windows-latest"));
    assert!(index_script.contains("ao2.workbench-operator-packet-control-plane-smoke.v1"));
    assert!(index_script.contains("token_leak_detected"));
    assert!(index_script.contains("evaluator_closure_verdict"));
    assert!(index_script.contains("replay_status"));
    assert!(index_script.contains("provider_score_present"));
    assert!(index_script.contains("missing_os"));
    assert!(index_script.contains("operator_packet_validation_failed"));
    assert!(index_script.contains("python_command()"));
    assert!(verification.contains("smoke:workbench-operator-packet-control-plane:index"));
    assert!(verification.contains("ao2.workbench-operator-packet-control-plane-smoke-index.v1"));
}

#[test]
fn codex_provider_smoke_script_is_guarded_and_evidence_driven() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/smoke-codex-provider.sh"))
        .expect("Codex provider smoke script exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");

    assert!(script.contains("AO2_LIVE_CODEX_SMOKE"));
    assert!(script.contains("explicit_flag_required"));
    assert!(script.contains("adapter doctor --provider codex"));
    assert!(script.contains("OPENAI_API_KEY"));
    assert!(script.contains("ANTHROPIC_API_KEY"));
    assert!(script.contains("provider smoke-all"));
    assert!(script.contains("--live-provider codex"));
    assert!(script.contains("ao2.provider-smoke-all.v1"));
    assert!(script.contains("codex_provider_smoke_history"));
    assert!(script.contains("codex_provider_smoke=passed"));
    assert!(package_json.contains("\"smoke:provider:codex\""));
}

#[test]
fn codex_provider_pilot_acceptance_script_is_guarded_and_evidence_driven() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/smoke-codex-provider-pilot.sh"))
        .expect("Codex provider pilot acceptance script exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let install = fs::read_to_string(root.join("docs/INSTALL.md")).expect("install guide exists");
    let release_ship =
        fs::read_to_string(root.join("scripts/release-ship.sh")).expect("release ship exists");

    assert!(script.contains("AO2_LIVE_CODEX_PILOT"));
    assert!(script.contains("AO2_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(script.contains("explicit_flag_required"));
    assert!(script.contains("adapter doctor --provider codex"));
    assert!(script.contains("OPENAI_API_KEY"));
    assert!(script.contains("ANTHROPIC_API_KEY"));
    assert!(script.contains("provider smoke-all"));
    assert!(script.contains("provider pilot"));
    assert!(script.contains("run examples/risky-pr-run/risky-pr.yaml"));
    assert!(script.contains("provider score"));
    assert!(script.contains("replay"));
    assert!(script.contains("python3 -m pytest"));
    assert!(script.contains("ao2.codex-provider-pilot-acceptance.v1"));
    assert!(script.contains("source_class: \"live\""));
    assert!(script.contains("AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION"));
    assert!(script.contains("release_candidate_version: $release_candidate_version"));
    assert!(script.contains("--argjson max_budget_usd"));
    assert!(script.contains("budget:"));
    assert!(script.contains("codex_provider_pilot_acceptance_bundle"));
    assert!(script.contains("codex_provider_pilot_acceptance=passed"));
    assert!(package_json.contains("\"smoke:provider:codex-pilot\""));
    assert!(install.contains("AO2_LIVE_CODEX_PILOT=1"));
    assert!(install.contains("AO2_RELEASE_CODEX_PILOT_ACCEPTANCE=1"));
    assert!(install.contains("AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(install.contains("ao2.codex-provider-pilot-acceptance.v1"));
    assert!(release_ship.contains("AO2_RELEASE_CODEX_PILOT_ACCEPTANCE"));
    assert!(release_ship.contains("AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(release_ship.contains("AO2_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(release_ship.contains("AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION"));
    assert!(release_ship.contains("AO2_CODEX_PROVIDER_PILOT_ROOT"));
    assert!(release_ship.contains("target/provider-pilot-acceptance/$AO2_RELEASE_TAG"));
    assert!(!release_ship.contains(
        "AO2_RELEASE_CODEX_PILOT_ROOT:-$AO2_RELEASE_DOWNLOAD_DIR/codex-provider-pilot-acceptance"
    ));
    assert!(release_ship.contains("npm run smoke:provider:codex-pilot"));
    assert!(release_ship.contains("release_codex_provider_pilot_acceptance=passed"));
}

#[test]
fn claude_provider_smoke_script_is_guarded_and_evidence_driven() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/smoke-claude-provider.sh"))
        .expect("Claude provider smoke script exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");

    assert!(script.contains("AO2_LIVE_CLAUDE_SMOKE"));
    assert!(script.contains("explicit_flag_required"));
    assert!(script.contains("adapter doctor --provider claude"));
    assert!(script.contains("OPENAI_API_KEY"));
    assert!(script.contains("ANTHROPIC_API_KEY"));
    assert!(script.contains("provider smoke-all"));
    assert!(script.contains("--live-provider claude"));
    assert!(script.contains("ao2.provider-smoke-all.v1"));
    assert!(script.contains("claude_provider_smoke_history"));
    assert!(script.contains("claude_provider_smoke=passed"));
    assert!(package_json.contains("\"smoke:provider:claude\""));
}

#[test]
fn claude_provider_pilot_acceptance_script_is_guarded_and_evidence_driven() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/smoke-claude-provider-pilot.sh"))
        .expect("Claude provider pilot acceptance script exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let install = fs::read_to_string(root.join("docs/INSTALL.md")).expect("install guide exists");
    let release_ship =
        fs::read_to_string(root.join("scripts/release-ship.sh")).expect("release ship exists");

    assert!(script.contains("AO2_LIVE_CLAUDE_PILOT"));
    assert!(script.contains("AO2_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(script.contains("explicit_flag_required"));
    assert!(script.contains("adapter doctor --provider claude"));
    assert!(script.contains("OPENAI_API_KEY"));
    assert!(script.contains("ANTHROPIC_API_KEY"));
    assert!(script.contains("provider smoke-all"));
    assert!(script.contains("provider pilot"));
    assert!(script.contains("run examples/risky-pr-run/risky-pr.yaml"));
    assert!(script.contains("provider score"));
    assert!(script.contains("replay"));
    assert!(script.contains("python3 -m pytest"));
    assert!(script.contains("ao2.claude-provider-pilot-acceptance.v1"));
    assert!(script.contains("source_class: \"live\""));
    assert!(script.contains("AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION"));
    assert!(script.contains("release_candidate_version: $release_candidate_version"));
    assert!(script.contains("--argjson max_budget_usd"));
    assert!(script.contains("budget:"));
    assert!(script.contains("claude_provider_pilot_acceptance_bundle"));
    assert!(script.contains("claude_provider_pilot_acceptance=passed"));
    assert!(package_json.contains("\"smoke:provider:claude-pilot\""));
    assert!(install.contains("AO2_LIVE_CLAUDE_PILOT=1"));
    assert!(install.contains("AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE=1"));
    assert!(install.contains("AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(install.contains("ao2.claude-provider-pilot-acceptance.v1"));
    assert!(release_ship.contains("AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE"));
    assert!(release_ship.contains("AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(release_ship.contains("AO2_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(release_ship.contains("AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION"));
    assert!(release_ship.contains("AO2_CLAUDE_PROVIDER_PILOT_ROOT"));
    assert!(release_ship.contains("target/provider-pilot-acceptance/$AO2_RELEASE_TAG/claude"));
    assert!(release_ship.contains("npm run smoke:provider:claude-pilot"));
    assert!(release_ship.contains("release_claude_provider_pilot_acceptance=passed"));
    assert!(release_ship.contains("workbench_claude_provider_pilot_acceptance_export=passed"));
}

#[test]
fn antigravity_provider_pilot_acceptance_script_is_guarded_and_evidence_driven() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/smoke-antigravity-provider-pilot.sh"))
        .expect("Antigravity provider pilot acceptance script exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let install = fs::read_to_string(root.join("docs/INSTALL.md")).expect("install guide exists");
    let release_ship =
        fs::read_to_string(root.join("scripts/release-ship.sh")).expect("release ship exists");

    assert!(script.contains("AO2_LIVE_ANTIGRAVITY_PILOT"));
    assert!(script.contains("AO2_PROVIDER_PILOT_MAX_BUDGET_USD"));
    assert!(script.contains("explicit_flag_required"));
    assert!(script.contains("adapter doctor --provider antigravity"));
    assert!(script.contains("OPENAI_API_KEY"));
    assert!(script.contains("ANTHROPIC_API_KEY"));
    assert!(script.contains("provider smoke-all"));
    assert!(script.contains("provider pilot"));
    assert!(script.contains("run examples/risky-pr-run/risky-pr.yaml"));
    assert!(script.contains("provider score"));
    assert!(script.contains("replay"));
    assert!(script.contains("python3 -m pytest"));
    assert!(script.contains("ao2.antigravity-provider-pilot-acceptance.v1"));
    assert!(script.contains("source_class: \"live\""));
    assert!(script.contains("AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION"));
    assert!(script.contains("release_candidate_version: $release_candidate_version"));
    assert!(script.contains("--argjson max_budget_usd"));
    assert!(script.contains("budget:"));
    assert!(script.contains("antigravity_provider_pilot_acceptance_bundle"));
    assert!(script.contains("antigravity_provider_pilot_acceptance=passed"));
    assert!(package_json.contains("\"smoke:provider:antigravity-pilot\""));
    assert!(install.contains("AO2_LIVE_ANTIGRAVITY_PILOT=1"));
    assert!(install.contains("AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE=1"));
    assert!(install.contains("ao2.antigravity-provider-pilot-acceptance.v1"));
    assert!(release_ship.contains("AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE"));
    assert!(release_ship.contains("AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT"));
    assert!(release_ship.contains("target/provider-pilot-acceptance/$AO2_RELEASE_TAG/antigravity"));
    assert!(release_ship.contains("npm run smoke:provider:antigravity-pilot"));
    assert!(release_ship.contains("release_antigravity_provider_pilot_acceptance=passed"));
    assert!(release_ship.contains("workbench_antigravity_provider_pilot_acceptance_export=passed"));
}

#[test]
#[cfg(not(windows))]
fn provider_pilot_scripts_initialize_copied_fixture_as_isolated_git_repo() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir outside checkout");
    assert!(
        !temp.path().starts_with(&root),
        "regression fixture must live outside the AO2 checkout"
    );

    for script_name in [
        "smoke-codex-provider-pilot.sh",
        "smoke-claude-provider-pilot.sh",
        "smoke-antigravity-provider-pilot.sh",
    ] {
        let script_path = root.join("scripts").join(script_name);
        let script = fs::read_to_string(&script_path).expect("provider pilot script exists");
        let copy_start = script
            .find("rm -rf -- \"$repo\"")
            .unwrap_or_else(|| panic!("{script_name} missing fixture reset"));
        let prompt_start = script[copy_start..]
            .find("cat > \"$prompt\" <<'PROMPT'")
            .map(|offset| copy_start + offset)
            .unwrap_or_else(|| panic!("{script_name} missing prompt boundary"));
        let smoke_start = script
            .find("provider smoke-all")
            .unwrap_or_else(|| panic!("{script_name} missing provider smoke-all"));
        let fixture_block = &script[copy_start..prompt_start];

        assert!(
            prompt_start < smoke_start,
            "{script_name} must prepare the fixture before provider smoke-all"
        );
        assert!(
            fixture_block.contains("git init -q \"$repo\""),
            "{script_name} must initialize the copied fixture as a Git repository"
        );
        assert!(
            fixture_block.contains("git -C \"$repo\" commit -q -m fixture"),
            "{script_name} must commit the isolated fixture baseline"
        );
        assert!(
            fixture_block.contains("git -C \"$repo\" rev-parse --git-common-dir"),
            "{script_name} must verify the isolated Git repository before provider execution"
        );

        let provider_root = temp.path().join(script_name.replace(".sh", ""));
        fs::create_dir_all(&provider_root).expect("create provider root");
        let repo = provider_root.join("discount-service");
        let harness = temp.path().join(format!("{script_name}.fixture-init.sh"));
        fs::write(
            &harness,
            format!(
                "set -eu\nrepo=\"$1\"\n{fixture_block}\ngit -C \"$repo\" rev-parse --git-common-dir\n",
            ),
        )
        .expect("write fixture init harness");

        let output = Command::new(sh_command())
            .arg(&harness)
            .arg(&repo)
            .current_dir(&root)
            .output()
            .expect("run provider pilot fixture init harness");
        assert!(
            output.status.success(),
            "{script_name} failed to initialize copied fixture as Git repo\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            ".git",
            "{script_name} must create a local .git common dir"
        );
    }
}

#[test]
#[cfg(not(windows))]
fn workbench_release_comparison_smoke_initializes_copied_fixture_as_git_repo() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir outside checkout");
    assert!(
        !temp.path().starts_with(&root),
        "regression fixture must live outside the AO2 checkout"
    );

    let script_name = "smoke-workbench-release-comparison-export.sh";
    let script_path = root.join("scripts").join(script_name);
    let script = fs::read_to_string(&script_path).expect("workbench comparison smoke exists");
    let copy_start = script
        .find("cp -R fixtures/discount-service \"$repo\"")
        .unwrap_or_else(|| panic!("{script_name} missing fixture copy"));
    let support_keygen_start = script[copy_start..]
        .find("ao2_cmd workbench support-keygen")
        .map(|offset| copy_start + offset)
        .unwrap_or_else(|| panic!("{script_name} missing support keygen boundary"));
    let serve_start = script
        .find("ao2_cmd workbench serve")
        .unwrap_or_else(|| panic!("{script_name} missing workbench serve"));
    let fixture_block = &script[copy_start..support_keygen_start];

    assert!(
        support_keygen_start < serve_start,
        "{script_name} must prepare the fixture before workbench serve"
    );
    assert!(
        fixture_block.contains("git init -q \"$repo\""),
        "{script_name} must initialize the copied fixture as a Git repository"
    );
    assert!(
        fixture_block.contains("git -C \"$repo\" commit -q -m fixture"),
        "{script_name} must commit the isolated fixture baseline"
    );
    assert!(
        fixture_block.contains("git -C \"$repo\" rev-parse --git-common-dir"),
        "{script_name} must verify the isolated Git repository before queue execution"
    );

    let smoke_root = temp.path().join("workbench-release-comparison");
    fs::create_dir_all(&smoke_root).expect("create workbench comparison root");
    let repo = smoke_root.join("repo");
    let harness = temp.path().join(format!("{script_name}.fixture-init.sh"));
    fs::write(
        &harness,
        format!(
            "set -eu\nrepo=\"$1\"\n{fixture_block}\ngit -C \"$repo\" rev-parse --git-common-dir\n",
        ),
    )
    .expect("write fixture init harness");

    let output = Command::new(sh_command())
        .arg(&harness)
        .arg(&repo)
        .current_dir(&root)
        .output()
        .expect("run workbench comparison fixture init harness");
    assert!(
        output.status.success(),
        "{script_name} failed to initialize copied fixture as Git repo\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        ".git",
        "{script_name} must create a local .git common dir"
    );
}

#[test]
fn hosted_release_archive_smoke_ci_uploads_three_os_install_sidecar_artifacts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("normal CI workflow exists");
    let unix_smoke = fs::read_to_string(root.join("scripts/release-archive-hosted-smoke.sh"))
        .expect("hosted Unix release archive smoke script exists");
    let windows_smoke = fs::read_to_string(root.join("scripts/release-archive-hosted-smoke.ps1"))
        .expect("hosted Windows release archive smoke script exists");

    assert!(ci.contains("release-archive-hosted-smoke:"));
    assert!(ci.contains("name: Release archive hosted smoke ${{ matrix.os }}"));
    assert!(ci.contains("os: [ubuntu-latest, macos-latest, windows-latest]"));
    assert!(ci.contains("cargo build --release -p ao2-cli"));
    assert!(ci.contains("scripts/release-archive-hosted-smoke.sh"));
    assert!(ci.contains("./scripts/release-archive-hosted-smoke.ps1"));
    assert!(ci.contains("AO2_RELEASE_HOSTED_SMOKE_JSON"));
    assert!(ci.contains("ao2-release-archive-hosted-smoke-${{ matrix.os }}"));
    assert!(ci.contains("target/release-archive-hosted-smoke"));

    for script in [&unix_smoke, &windows_smoke] {
        assert!(script.contains("ao2.release-archive-hosted-smoke.v1"));
        assert!(script.contains("ao2.install-verification-evidence.v1"));
        assert!(script.contains("install_verification_evidence"));
        assert!(script.contains("provider_api_keys_required"));
        assert!(script.contains("control_plane_approves_release"));
        assert!(script.contains("mutates_ao_artifacts"));
        assert!(script.contains("factory-v3 evaluator-closer"));
        assert!(script.contains("status"));
        assert!(script.contains("passed"));
    }

    assert!(unix_smoke.contains("linux-x86_64"));
    assert!(unix_smoke.contains("macos-aarch64"));
    assert!(unix_smoke.contains("install.sh"));
    assert!(unix_smoke.contains("provider matrix --json"));
    assert!(windows_smoke.contains("windows-x86_64"));
    assert!(windows_smoke.contains("install.ps1"));
    assert!(windows_smoke.contains("provider matrix --json"));
}

#[test]
fn release_build_all_script_and_manual_workflow_cover_public_release_sequence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/release-build-all.sh"))
        .expect("release build-all script exists");
    let ship_script = fs::read_to_string(root.join("scripts/release-ship.sh"))
        .expect("release ship script exists");
    let stage_script = fs::read_to_string(root.join("scripts/release-stage-publication-assets.sh"))
        .expect("release publication staging script exists");
    let workflow = fs::read_to_string(root.join(".github/workflows/public-release-build.yml"))
        .expect("manual public release build workflow exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");

    assert!(script.contains("npm run build:release"));
    assert!(script.contains("npm run package:local"));
    assert!(script.contains("npm run package:linux:aarch64:docker"));
    assert!(script.contains("npm run package:linux:x86_64:docker"));
    assert!(script.contains("npm run cross-package:windows:gnu:from-linux"));
    assert!(script.contains("npm run release:sign-provenance"));
    assert!(script.contains("npm run release:verify-provenance"));
    assert!(script.contains("release_build_all=passed"));

    assert!(package_json.contains("\"release:build-all\""));
    assert!(package_json.contains("\"cross-package:windows:gnu:from-linux\""));
    assert!(package_json.contains("\"release:ship\""));

    assert!(ship_script.contains("AO2_RELEASE_SHIP_CONFIRM"));
    assert!(ship_script.contains("npm run verify"));
    assert!(ship_script.contains("npm run release:build-all"));
    assert!(ship_script.contains("AO2_REQUIRE_NATIVE_WINDOWS_SMOKE:-1"));
    assert!(ship_script.contains("npm run smoke:three-os"));
    assert!(ship_script.contains("npm run release:gate"));
    assert!(ship_script.contains("git tag -a"));
    assert!(ship_script.contains("gh release create"));
    assert!(!ship_script.contains("AO2_SYNC_AO_RUNTIME_SOURCE"));
    assert!(!ship_script.contains("AO2_SYNC_AO_CONTROL_PLANE_SOURCE"));
    assert!(!ship_script.contains("AO2_SYNC_AO_OPERATOR_SOURCE"));
    assert!(!ship_script.contains("mirror_run_pair ao-runtime"));
    assert!(!ship_script.contains("mirror_run_pair ao-control-plane"));
    assert!(!ship_script.contains("mirror_run_pair ao-operator"));
    assert!(stage_script.contains("dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz"));
    assert!(stage_script.contains("dist-provenance/$base.tar.gz.sha256"));
    assert!(stage_script.contains("dist/ao2-$AO2_VERSION-macos-aarch64.tar.gz"));
    assert!(stage_script.contains("dist-provenance/$base.tar.gz.sig"));
    assert!(ship_script.contains("AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY=1"));
    assert!(ship_script.contains("npm run release:download-verify"));
    assert!(ship_script.contains("doctor --json --release"));
    assert!(ship_script.contains("release_ship=passed"));

    assert!(workflow.contains("workflow_dispatch:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("\n  push:"));
    assert!(workflow.contains("bind-release-plan:"));
    assert!(workflow.contains("verify-physical-windows-qualification:"));
    assert!(workflow.contains(
        "physical_evidence_sha256: ${{ steps.verify.outputs.physical_evidence_sha256 }}"
    ));
    assert!(workflow.contains("Validate canonical physical Windows qualification bundle"));
    assert!(workflow.contains("Authenticate producer workflow run and artifact"));
    assert!(workflow.contains("validate_physical_windows_workflow_run.py validate-run-id"));
    assert!(workflow.contains("validate_physical_windows_workflow_run.py validate-metadata"));
    assert!(workflow.contains("actions/runs/$RUN_ID/artifacts?per_page=100"));
    assert!(workflow.contains("python3 scripts/physical_windows_qualification.py validate"));
    assert!(workflow.contains("native-build:"));
    assert!(workflow.contains("assemble-promotion-plan:"));
    assert!(workflow.contains("physical_windows_evidence_sha256"));
    assert!(workflow.contains("physical_windows_evidence_mismatch"));
    assert!(workflow.contains("physical_windows_mode = \"physical_bounded\""));
    assert!(!workflow.contains("physical_windows_mode = \"physical_unique\""));
    assert!(workflow.contains("x86_64-pc-windows-msvc"));
    assert!(workflow.contains("cross-package:windows:gnu:from-linux"));
    assert!(workflow.contains("actions/upload-artifact@v7.0.1"));
}

#[test]
fn public_release_publisher_enforces_prerelease_channel_and_immutable_asset_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let contract = fs::read_to_string(root.join("scripts/release-publication-contract.sh"))
        .expect("release publication contract exists");
    let stage = fs::read_to_string(root.join("scripts/release-stage-publication-assets.sh"))
        .expect("release publication staging script exists");
    let ship = fs::read_to_string(root.join("scripts/release-ship.sh"))
        .expect("release ship script exists");
    let approved = fs::read_to_string(root.join("scripts/release-publish-approved-assets.sh"))
        .expect("approved asset publisher exists");

    for needle in [
        "AO2_RELEASE_CHANNEL",
        "AO2_RELEASE_NOTES_FILE",
        "AO2_RELEASE_TITLE",
        "external beta",
        "AO2_RELEASE_CODEX_PILOT_ACCEPTANCE",
        "AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE",
        "AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE",
        "AO2_RELEASE_PRIVATE_KEY",
        "prerelease version requires AO2_RELEASE_CHANNEL=prerelease",
        "stable version requires AO2_RELEASE_CHANNEL=stable",
    ] {
        assert!(
            contract.contains(needle),
            "missing contract guard: {needle}"
        );
    }

    for target in [
        "macos-aarch64",
        "linux-aarch64",
        "linux-x86_64",
        "windows-x86_64",
    ] {
        assert!(stage.contains(target), "missing staged target: {target}");
    }
    for asset in [
        "SHA256SUMS",
        ".tar.gz.sha256",
        ".tar.gz.sig",
        ".sbom.cdx.json",
        "ao2-release-provenance.json",
        "ao2-release-provenance.json.sig",
        "ao2-release-signing-public.pem",
        "ao2-release-artifact-closure-index.json",
        "ao2-release-readiness-summary.json",
        "ao2-release-train-control-plane-bridge-summary.json",
    ] {
        assert!(stage.contains(asset), "missing staged asset: {asset}");
    }

    assert!(!ship.contains("Private AO2 release"));
    assert!(!ship.contains("gh release upload"));
    assert!(!ship.contains("--clobber"));
    assert!(ship.contains("scripts/release-publication-contract.sh"));
    assert!(ship.contains("scripts/release-stage-publication-assets.sh"));
    assert!(ship.contains("AO2_RELEASE_SHIP_DRY_RUN"));
    assert!(ship.contains("AO2_RELEASE_EXPECTED_ASSET_MANIFEST"));
    assert!(ship.contains("AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256"));
    assert!(ship.contains("--prerelease"));
    assert!(ship.contains("--latest=false"));
    assert!(ship.contains("release_approval_bound=false"));
    assert!(ship.contains("release_approval_bound=true"));
    assert!(ship.contains("release_approved_asset_manifest_sha256=not_supplied"));
    assert!(ship.contains("refusing to overwrite existing release"));
    assert!(ship.contains("refusing to reuse existing release tag"));

    assert!(approved.contains("prerelease version requires AO2_RELEASE_CHANNEL=prerelease"));
    assert!(approved.contains("stable version requires AO2_RELEASE_CHANNEL=stable"));
    assert!(!approved.contains("approved asset promotion is restricted to an AO2 prerelease"));
    assert!(approved.contains("release_create_flags=(--latest)"));
    assert!(approved.contains("release_create_flags=(--prerelease --latest=false)"));
    assert!(approved.contains("latest_stable_after=\"$AO2_RELEASE_TAG\""));
}

#[test]
#[cfg(unix)]
fn release_ship_rejects_missing_live_or_partial_dry_run_binding_before_external_commands() {
    use std::os::unix::fs::PermissionsExt;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    let sentinel = temp.path().join("external-command-called");
    fs::create_dir_all(&bin).expect("create stub bin");
    for command in ["git", "npm", "gh"] {
        let stub = bin.join(command);
        fs::write(
            &stub,
            "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$AO2_TEST_EXTERNAL_SENTINEL\"\nexit 99\n",
        )
        .expect("write command stub");
        let mut permissions = fs::metadata(&stub).expect("stub metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&stub, permissions).expect("make stub executable");
    }
    let inherited_path = std::env::var("PATH").expect("PATH");
    let stub_path = format!("{}:{inherited_path}", bin.display());

    let run_ship = |dry_run: &str, manifest: Option<&Path>, digest: Option<&str>| {
        let mut command = Command::new("sh");
        command
            .arg(root.join("scripts/release-ship.sh"))
            .current_dir(&root)
            .env("PATH", &stub_path)
            .env("AO2_TEST_EXTERNAL_SENTINEL", &sentinel)
            .env("AO2_VERSION", "0.5.0-beta.1")
            .env("AO2_RELEASE_TAG", "v0.5.0-beta.1")
            .env("AO2_RELEASE_TARGET_COMMIT", "test-target")
            .env("AO2_RELEASE_SHIP_DRY_RUN", dry_run)
            .env("AO2_RELEASE_SHIP_CONFIRM", "ship-v0.5.0-beta.1")
            .env_remove("AO2_RELEASE_EXPECTED_ASSET_MANIFEST")
            .env_remove("AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256");
        if let Some(value) = manifest {
            command.env("AO2_RELEASE_EXPECTED_ASSET_MANIFEST", value);
        }
        if let Some(value) = digest {
            command.env("AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256", value);
        }
        command.output().expect("run guarded publisher")
    };

    let missing_live = run_ship("0", None, None);
    assert!(!missing_live.status.success());
    assert!(String::from_utf8_lossy(&missing_live.stderr)
        .contains("live publication requires AO2_RELEASE_EXPECTED_ASSET_MANIFEST"));
    assert!(
        !sentinel.exists(),
        "live failure reached an external command"
    );

    let partial_manifest = temp.path().join("manifest.sha256");
    fs::write(&partial_manifest, "fixture\n").expect("write partial manifest");
    let partial_dry_run = run_ship("1", Some(&partial_manifest), None);
    assert!(!partial_dry_run.status.success());
    assert!(String::from_utf8_lossy(&partial_dry_run.stderr)
        .contains("dry run requires both expected asset manifest variables or neither"));
    assert!(
        !sentinel.exists(),
        "partial dry-run failure reached an external command"
    );
}

#[test]
fn release_ship_places_manifest_verification_before_every_mutation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ship = fs::read_to_string(root.join("scripts/release-ship.sh"))
        .expect("release ship script exists");

    let prebuild_guard = ship
        .find("live publication requires AO2_RELEASE_EXPECTED_ASSET_MANIFEST")
        .expect("live manifest guard");
    let moving_head = ship
        .find("refusing to publish unreviewed moving head")
        .expect("moving-head guard");
    let build = ship.find("npm run verify").expect("build verification");
    assert!(prebuild_guard < moving_head);
    assert!(prebuild_guard < build);

    let stage = ship
        .find("scripts/release-stage-publication-assets.sh")
        .expect("publication staging");
    let verify = ship
        .find("scripts/release-verify-approved-assets.py")
        .expect("approved asset verification");
    let local_tag = ship.find("git tag -a").expect("local tag mutation");
    let tag_push = ship
        .find("git push origin \"$AO2_RELEASE_TAG\"")
        .expect("tag push mutation");
    let release_create = ship
        .find("gh release create")
        .expect("GitHub release mutation");
    let dry_run_exit = ship
        .find("release_ship_dry_run=passed")
        .expect("dry-run success output");
    assert!(stage < verify);
    assert!(verify < local_tag);
    assert!(verify < tag_push);
    assert!(verify < release_create);
    assert!(dry_run_exit < local_tag);
    assert!(!ship.contains("gh release upload"));
    assert!(!ship.contains("--clobber"));
}

#[test]
#[cfg(not(windows))]
fn release_ship_prebuild_binding_logic_accepts_neither_or_both_only_for_dry_run() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ship = fs::read_to_string(root.join("scripts/release-ship.sh"))
        .expect("release ship script exists");
    let boundary = ship
        .find("if [ \"$AO2_RELEASE_TARGET_COMMIT\" !=")
        .expect("moving-head boundary");
    let mut prebuild = ship[..boundary].to_owned();
    prebuild.push_str("printf 'test_approval_bound=%s\\n' \"$AO2_RELEASE_APPROVAL_BOUND\"\n");
    let prebuild_script = tempfile::NamedTempFile::new().expect("prebuild script fixture");
    fs::write(prebuild_script.path(), prebuild).expect("write prebuild script");

    let run = |manifest: Option<&str>, digest: Option<&str>| {
        let mut command = Command::new("sh");
        command
            .arg(prebuild_script.path())
            .current_dir(&root)
            .env("AO2_VERSION", "0.5.0-beta.1")
            .env("AO2_RELEASE_TAG", "v0.5.0-beta.1")
            .env("AO2_RELEASE_TARGET_COMMIT", "test-target")
            .env("AO2_RELEASE_SHIP_DRY_RUN", "1")
            .env_remove("AO2_RELEASE_EXPECTED_ASSET_MANIFEST")
            .env_remove("AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256");
        if let Some(value) = manifest {
            command.env("AO2_RELEASE_EXPECTED_ASSET_MANIFEST", value);
        }
        if let Some(value) = digest {
            command.env("AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256", value);
        }
        command.output().expect("run prebuild binding logic")
    };

    let neither = run(None, None);
    assert!(neither.status.success());
    assert!(String::from_utf8_lossy(&neither.stdout).contains("test_approval_bound=0"));

    let digest64 = "a".repeat(64);
    let both = run(Some("approved.sha256"), Some(&digest64));
    assert!(both.status.success());
    assert!(String::from_utf8_lossy(&both.stdout).contains("test_approval_bound=1"));

    for (manifest, digest) in [
        (Some("approved.sha256"), None),
        (None, Some(digest64.as_str())),
    ] {
        let output = run(manifest, digest);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("dry run requires both expected asset manifest variables or neither"));
    }
}

#[test]
#[cfg(not(windows))]
fn one_byte_drift_stops_before_mutation_sentinel() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let (publication_dir, publication_list, manifest, digest) = approved_asset_fixture(temp.path());
    let changed = publication_dir.join("asset-11.bin");
    let mut bytes = fs::read(&changed).expect("read staged asset");
    bytes[0] ^= 1;
    fs::write(&changed, bytes).expect("change exactly one byte");
    let sentinel = temp.path().join("mutation-sentinel");

    let output = Command::new("sh")
        .arg("-c")
        .arg(
            "python3 \"$1\" --manifest \"$2\" --manifest-sha256 \"$3\" \
             --publication-dir \"$4\" --publication-list \"$5\" && : > \"$6\"",
        )
        .arg("sh")
        .arg(root.join("scripts/release-verify-approved-assets.py"))
        .arg(&manifest)
        .arg(&digest)
        .arg(&publication_dir)
        .arg(&publication_list)
        .arg(&sentinel)
        .output()
        .expect("run drift-before-mutation harness");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("approved asset hash mismatch: asset-11.bin"));
    assert!(
        !sentinel.exists(),
        "one-byte drift reached mutation sentinel"
    );
}

#[test]
#[cfg(not(windows))]
fn publication_contract_accepts_stable_channel_and_rejects_provider_pilots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let notes = temp.path().join("notes.md");
    let signing_key = temp.path().join("test-only-signing-key.pem");
    fs::write(&notes, "# AO2 v0.5.0 Stable\n\nStable release notes.\n")
        .expect("write notes fixture");
    fs::write(&signing_key, "test-only fixture\n").expect("write key-presence fixture");

    let run_contract = |channel: &str, pilot: &str| {
        Command::new("bash")
            .arg(root.join("scripts/release-publication-contract.sh"))
            .current_dir(&root)
            .env("AO2_RELEASE_CONTRACT_REQUIRE_ASSETS", "0")
            .env("AO2_RELEASE_CHANNEL", channel)
            .env("AO2_RELEASE_TITLE", "AO2 v0.5.0 stable")
            .env("AO2_RELEASE_NOTES_FILE", &notes)
            .env("AO2_RELEASE_PRIVATE_KEY", &signing_key)
            .env("AO2_RELEASE_CODEX_PILOT_ACCEPTANCE", pilot)
            .output()
            .expect("run release publication contract")
    };

    let accepted = run_contract("stable", "0");
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );

    let prerelease_channel = run_contract("prerelease", "0");
    assert!(!prerelease_channel.status.success());
    assert!(String::from_utf8_lossy(&prerelease_channel.stderr)
        .contains("stable version requires AO2_RELEASE_CHANNEL=stable"));

    let provider_pilot = run_contract("stable", "1");
    assert!(!provider_pilot.status.success());
    assert!(String::from_utf8_lossy(&provider_pilot.stderr)
        .contains("AO2_RELEASE_CODEX_PILOT_ACCEPTANCE must remain disabled"));
}

#[test]
fn beta_release_notes_include_explicit_uninstall_commands() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let notes = fs::read_to_string(root.join("docs/release/v0.5.0-beta.1.md"))
        .expect("beta release notes exist");

    assert!(notes.contains("rm -f \"$HOME/.local/bin/ao2\""));
    assert!(notes.contains("Remove-Item -Force -ErrorAction SilentlyContinue"));
}

#[test]
fn v052_stable_release_notes_include_rollback_command() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let notes = fs::read_to_string(root.join("docs/release/v0.5.2-stable.md"))
        .expect("stable release notes exist");

    assert!(notes.contains("ao2 install rollback"));
}

#[cfg(not(windows))]
fn approved_asset_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf, String) {
    let publication_dir = root.join("publication");
    let publication_list = root.join("publication.assets.txt");
    let manifest = root.join("approved-assets.sha256");
    fs::create_dir_all(&publication_dir).expect("create publication fixture");

    let mut names = Vec::new();
    let mut manifest_lines = Vec::new();
    for index in 0..23 {
        let name = format!("asset-{index:02}.bin");
        let path = publication_dir.join(&name);
        fs::write(&path, format!("approved asset {index}\n")).expect("write staged asset");
        names.push(name.clone());
        manifest_lines.push(format!("{}  {name}", sha256_file_hex(&path)));
    }
    fs::write(&publication_list, names.join("\n") + "\n").expect("write publication list");
    fs::write(&manifest, manifest_lines.join("\n") + "\n").expect("write approved manifest");
    let digest = sha256_file_hex(&manifest);
    (publication_dir, publication_list, manifest, digest)
}

#[cfg(not(windows))]
fn run_approved_asset_verifier(
    root: &Path,
    publication_dir: &Path,
    publication_list: &Path,
    manifest: &Path,
    digest: &str,
) -> std::process::Output {
    Command::new("python3")
        .arg(root.join("scripts/release-verify-approved-assets.py"))
        .arg("--manifest")
        .arg(manifest)
        .arg("--manifest-sha256")
        .arg(digest)
        .arg("--publication-dir")
        .arg(publication_dir)
        .arg("--publication-list")
        .arg(publication_list)
        .output()
        .expect("run approved asset verifier")
}

#[test]
#[cfg(not(windows))]
fn approved_asset_verifier_accepts_exact_23_asset_set() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let (publication_dir, publication_list, manifest, digest) = approved_asset_fixture(temp.path());

    let output = run_approved_asset_verifier(
        &root,
        &publication_dir,
        &publication_list,
        &manifest,
        &digest,
    );
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("release_approved_asset_manifest_sha256={digest}")));
    assert!(stdout.contains("release_approved_asset_count=23"));
    assert!(stdout.contains("release_approved_assets=passed"));
}

#[test]
#[cfg(not(windows))]
fn approved_asset_verifier_rejects_one_changed_byte() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let (publication_dir, publication_list, manifest, digest) = approved_asset_fixture(temp.path());
    let changed = publication_dir.join("asset-07.bin");
    let mut bytes = fs::read(&changed).expect("read asset");
    bytes[0] ^= 1;
    fs::write(&changed, bytes).expect("change one byte");

    let output = run_approved_asset_verifier(
        &root,
        &publication_dir,
        &publication_list,
        &manifest,
        &digest,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("approved asset hash mismatch: asset-07.bin"));
}

#[test]
#[cfg(not(windows))]
fn approved_asset_verifier_rejects_missing_and_extra_assets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let missing_temp = tempfile::tempdir().expect("tempdir");
    let (missing_dir, missing_list, missing_manifest, missing_digest) =
        approved_asset_fixture(missing_temp.path());
    fs::remove_file(missing_dir.join("asset-03.bin")).expect("remove staged asset");
    let missing = run_approved_asset_verifier(
        &root,
        &missing_dir,
        &missing_list,
        &missing_manifest,
        &missing_digest,
    );
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr)
        .contains("approved staged asset is missing: asset-03.bin"));

    let extra_temp = tempfile::tempdir().expect("tempdir");
    let (extra_dir, extra_list, extra_manifest, extra_digest) =
        approved_asset_fixture(extra_temp.path());
    fs::write(extra_dir.join("unapproved.bin"), b"extra\n").expect("write extra asset");
    let mut list = fs::read_to_string(&extra_list).expect("read publication list");
    list.push_str("unapproved.bin\n");
    fs::write(&extra_list, list).expect("append publication list");
    let extra = run_approved_asset_verifier(
        &root,
        &extra_dir,
        &extra_list,
        &extra_manifest,
        &extra_digest,
    );
    assert!(!extra.status.success());
    assert!(String::from_utf8_lossy(&extra.stderr)
        .contains("staged publication set has extra asset: unapproved.bin"));

    let unlisted_temp = tempfile::tempdir().expect("tempdir");
    let (unlisted_dir, unlisted_list, unlisted_manifest, unlisted_digest) =
        approved_asset_fixture(unlisted_temp.path());
    fs::write(unlisted_dir.join("unlisted.bin"), b"unlisted\n")
        .expect("write unlisted directory asset");
    let unlisted = run_approved_asset_verifier(
        &root,
        &unlisted_dir,
        &unlisted_list,
        &unlisted_manifest,
        &unlisted_digest,
    );
    assert!(!unlisted.status.success());
    assert!(String::from_utf8_lossy(&unlisted.stderr)
        .contains("publication directory has unlisted asset: unlisted.bin"));
}

#[test]
#[cfg(not(windows))]
fn approved_asset_verifier_rejects_changed_manifest_digest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().expect("tempdir");
    let (publication_dir, publication_list, manifest, digest) = approved_asset_fixture(temp.path());
    let mut text = fs::read_to_string(&manifest).expect("read manifest");
    text.push('\n');
    fs::write(&manifest, text).expect("change manifest bytes");

    let output = run_approved_asset_verifier(
        &root,
        &publication_dir,
        &publication_list,
        &manifest,
        &digest,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("approved manifest SHA-256 mismatch"));
}

#[test]
#[cfg(not(windows))]
fn approved_asset_verifier_rejects_duplicate_and_unsafe_manifest_names() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let duplicate_temp = tempfile::tempdir().expect("tempdir");
    let (duplicate_dir, duplicate_list, duplicate_manifest, _) =
        approved_asset_fixture(duplicate_temp.path());
    let mut duplicate_text =
        fs::read_to_string(&duplicate_manifest).expect("read duplicate manifest");
    let first = duplicate_text
        .lines()
        .next()
        .expect("first manifest line")
        .to_owned();
    duplicate_text.push_str(&first);
    duplicate_text.push('\n');
    fs::write(&duplicate_manifest, duplicate_text).expect("write duplicate manifest");
    let duplicate_digest = sha256_file_hex(&duplicate_manifest);
    let duplicate = run_approved_asset_verifier(
        &root,
        &duplicate_dir,
        &duplicate_list,
        &duplicate_manifest,
        &duplicate_digest,
    );
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr)
        .contains("duplicate approved asset name: asset-00.bin"));

    let malformed_temp = tempfile::tempdir().expect("tempdir");
    let (malformed_dir, malformed_list, malformed_manifest, _) =
        approved_asset_fixture(malformed_temp.path());
    let text = fs::read_to_string(&malformed_manifest).expect("read malformed manifest");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    lines[0].replace_range(..64, &"A".repeat(64));
    fs::write(&malformed_manifest, lines.join("\n") + "\n").expect("write malformed manifest hash");
    let malformed_digest = sha256_file_hex(&malformed_manifest);
    let malformed = run_approved_asset_verifier(
        &root,
        &malformed_dir,
        &malformed_list,
        &malformed_manifest,
        &malformed_digest,
    );
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr)
        .contains("approved manifest line 1 is malformed"));

    for unsafe_name in [
        "/tmp/asset.bin",
        "../asset.bin",
        "nested/asset.bin",
        "nested\\asset.bin",
        ".",
        "..",
    ] {
        let unsafe_temp = tempfile::tempdir().expect("tempdir");
        let (unsafe_dir, unsafe_list, unsafe_manifest, _) =
            approved_asset_fixture(unsafe_temp.path());
        let text = fs::read_to_string(&unsafe_manifest).expect("read unsafe manifest");
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let hash = lines[0].split_once("  ").expect("manifest separator").0;
        lines[0] = format!("{hash}  {unsafe_name}");
        fs::write(&unsafe_manifest, lines.join("\n") + "\n").expect("write unsafe manifest");
        let unsafe_digest = sha256_file_hex(&unsafe_manifest);
        let output = run_approved_asset_verifier(
            &root,
            &unsafe_dir,
            &unsafe_list,
            &unsafe_manifest,
            &unsafe_digest,
        );
        assert!(
            !output.status.success(),
            "unsafe manifest name unexpectedly accepted: {unsafe_name}"
        );
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("approved manifest contains unsafe asset name"));
    }
}

#[test]
#[cfg(unix)]
fn approved_asset_verifier_rejects_manifest_and_asset_symlinks() {
    use std::os::unix::fs::symlink;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let manifest_temp = tempfile::tempdir().expect("tempdir");
    let (manifest_dir, manifest_list, manifest, digest) =
        approved_asset_fixture(manifest_temp.path());
    let manifest_link = manifest_temp.path().join("manifest-link.sha256");
    symlink(&manifest, &manifest_link).expect("symlink manifest");
    let manifest_output = run_approved_asset_verifier(
        &root,
        &manifest_dir,
        &manifest_list,
        &manifest_link,
        &digest,
    );
    assert!(!manifest_output.status.success());
    assert!(String::from_utf8_lossy(&manifest_output.stderr)
        .contains("approved manifest must be a regular non-symlink file"));

    let asset_temp = tempfile::tempdir().expect("tempdir");
    let (asset_dir, asset_list, asset_manifest, asset_digest) =
        approved_asset_fixture(asset_temp.path());
    let asset = asset_dir.join("asset-05.bin");
    let real_asset = asset_dir.join("asset-05-real.bin");
    fs::rename(&asset, &real_asset).expect("move real asset");
    symlink(&real_asset, &asset).expect("symlink asset");
    let asset_output = run_approved_asset_verifier(
        &root,
        &asset_dir,
        &asset_list,
        &asset_manifest,
        &asset_digest,
    );
    assert!(!asset_output.status.success());
    assert!(String::from_utf8_lossy(&asset_output.stderr)
        .contains("approved staged asset must be a regular non-symlink file: asset-05.bin"));
}

#[test]
fn linux_x86_64_docker_packaging_constrains_emulated_build_parallelism() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/package-linux-x86_64-docker.sh"))
        .expect("linux x86_64 docker packaging script exists");

    assert!(script.contains("AO2_LINUX_X86_64_CARGO_BUILD_JOBS"));
    assert!(script.contains("CARGO_BUILD_JOBS=\"$AO2_LINUX_X86_64_CARGO_BUILD_JOBS\""));
    assert!(script.contains("CARGO_INCREMENTAL=0"));
    assert!(script.contains("AO2_LINUX_X86_64_BUILD_STRATEGY"));
    assert!(script.contains("AO2_LINUX_X86_64_RUN_DOCKER_PLATFORM"));
    assert!(script.contains("AO2_LINUX_X86_64_BUILD_DOCKER_PLATFORM"));
    assert!(script.contains("x86_64-unknown-linux-gnu"));
    assert!(script.contains("gcc-x86-64-linux-gnu"));
    assert!(script.contains("libc6-dev-amd64-cross"));
    assert!(script.contains("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER"));
}

#[test]
fn linux_aarch64_docker_packaging_constrains_emulated_build_parallelism() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/package-linux-aarch64-docker.sh"))
        .expect("linux aarch64 docker packaging script exists");

    assert!(script.contains("AO2_LINUX_AARCH64_CARGO_BUILD_JOBS"));
    assert!(script.contains("CARGO_BUILD_JOBS=\"$AO2_LINUX_AARCH64_CARGO_BUILD_JOBS\""));
    assert!(script.contains("CARGO_INCREMENTAL=0"));
}

#[test]
fn archive_heavy_test_resource_guard_is_wired_for_ci_and_local_use() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let guard = fs::read_to_string(root.join("scripts/ci/archive-heavy-resource-guard.py"))
        .expect("archive-heavy resource guard exists");
    let guard_launcher =
        fs::read_to_string(root.join("scripts/ci/run-archive-heavy-resource-guard.mjs"))
            .expect("archive-heavy resource guard launcher exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("normal CI workflow exists");
    let ci = ci.replace("\r\n", "\n");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification doc exists");

    assert!(guard.contains("ao2.archive-heavy-test-resource-guard.v1"));
    assert!(guard.contains("AO2_ARCHIVE_TEST_MIN_FREE_GB"));
    assert!(guard.contains("AO2_ARCHIVE_TEST_EXPECT_SINGLE_THREAD"));
    assert!(guard.contains("shutil.disk_usage"));
    assert!(guard.contains("tempfile.gettempdir"));
    assert!(guard.contains("archive_heavy_test_resource_guard=passed"));

    assert!(package_json.contains("\"test:archive-resources\""));
    assert!(package_json.contains("node scripts/ci/run-archive-heavy-resource-guard.mjs"));
    assert!(!package_json.contains("\"test:archive-resources\": \"python3 "));
    assert!(guard_launcher.contains("archive-heavy-resource-guard.py"));
    assert!(guard_launcher.contains("python3"));
    assert!(guard_launcher.contains("python"));
    assert!(guard_launcher.contains("py"));

    assert!(ci.contains("npm run test:archive-resources"));
    for test_name in [
        "cli_architecture_ownership",
        "provider_pilot_acceptance_preservation",
        "release_packaging",
    ] {
        assert!(ci.contains(&format!("--test {test_name}")));
    }
    assert!(ci.contains(
        "--test cli_architecture_ownership\n              --test provider_pilot_acceptance_preservation\n              --test release_packaging\n              --test sdd_subcommand\n              -- --test-threads=1"
    ));
    assert!(verification.contains("test:archive-resources"));
    assert!(verification.contains("--test-threads=1"));
}

#[test]
fn w4_release_workflows_include_no_factory_v3_guard_artifacts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let release_gate = fs::read_to_string(root.join(".github/workflows/release-gate.yml"))
        .expect("release gate workflow exists");
    let public_release =
        fs::read_to_string(root.join(".github/workflows/public-release-build.yml"))
            .expect("public release build workflow exists");
    let hosted_validator =
        fs::read_to_string(root.join("scripts/validate_hosted_release_candidates.py"))
            .expect("hosted native candidate validator exists");
    let w4_roadmap = fs::read_to_string(root.join("docs/roadmap/PHASE-2-W4-CI-INTEGRATION.md"))
        .expect("W4 roadmap exists");
    let ready_to_ship = fs::read_to_string(root.join("docs/release/READY-TO-SHIP.md"))
        .expect("ready-to-ship release runbook exists");

    assert!(release_gate.contains("npm run verify:no-factory-v3"));
    assert!(release_gate.contains("AO2_HOSTED_RELEASE_GATE"));
    assert!(release_gate.contains("AO2_REQUIRE_NATIVE_WINDOWS_SMOKE"));
    assert!(release_gate.contains("AO2_ALLOW_UNSIGNED_OBLIGATION_GATES"));
    assert!(release_gate.contains("target/no-factory-v3-green-path/"));
    assert!(release_gate.contains("target/release-gate-with-replacement/"));
    assert!(release_gate.contains("if-no-files-found: warn"));

    assert!(public_release.contains("npm run verify:no-factory-v3"));
    assert!(public_release.contains("validate_hosted_release_candidates.py"));
    assert!(public_release.contains("--source-sha \"$SOURCE_SHA\""));
    assert!(public_release.contains("npm run verify:replacement"));
    assert!(public_release.contains("target/no-factory-v3-green-path/"));
    assert!(public_release.contains("target/hosted-release/native-gate/summary.json"));
    assert!(hosted_validator.contains("signed_four_archive_release_gate"));
    assert!(!public_release.contains("npm run gate:full"));
    assert!(w4_roadmap.contains("Stage 0"));
    assert!(w4_roadmap.contains("gate_with_replacement_passed=3/3"));
    assert!(ready_to_ship.contains("release-gate.yml"));
    assert!(ready_to_ship.contains("npm run verify:no-factory-v3"));
    assert!(ready_to_ship.contains("npm run gate:full"));

    let guard = fs::read_to_string(root.join("scripts/verify-no-factory-v3-green-path.sh"))
        .expect("no factory-v3 green path guard exists");
    assert!(guard.contains("ao2_replaces_factory_v3_workflow_driver"));
    assert!(guard.contains("queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver"));
    assert!(guard.contains("queued_replacement_packet_factory_v3_role"));
    assert!(guard.contains("ao2_replacement_driver_contract"));
    assert!(guard.contains("sampling_auditor"));
    assert!(guard.contains("scripts/parity-oracle-snapshots/"));
    assert!(guard.contains("parity_oracle_snapshot"));
    assert!(!guard.contains("public_mirror_source_only"));

    let release_gate_script =
        fs::read_to_string(root.join("scripts/release-gate.sh")).expect("release gate exists");
    assert!(release_gate_script.contains("AO2_HOSTED_RELEASE_GATE"));
    assert!(release_gate_script.contains("hosted_release_gate_archive_only"));
    assert!(release_gate_script.contains("AO2_ALLOW_UNSIGNED_OBLIGATION_GATES"));
}

#[test]
fn ci_workflow_runs_on_public_changes_while_release_gates_stay_manual() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("normal CI workflow exists");
    let release_gate = fs::read_to_string(root.join(".github/workflows/release-gate.yml"))
        .expect("release gate workflow exists");
    let public_release =
        fs::read_to_string(root.join(".github/workflows/public-release-build.yml"))
            .expect("public release build workflow exists");

    assert!(ci.contains("workflow_dispatch:"));
    assert!(ci.contains("pull_request:"));
    assert!(ci.contains("\n  push:"));
    assert!(ci.contains("branches: [main]"));
    assert!(ci.contains("concurrency:"));
    assert!(ci.contains("cancel-in-progress: true"));
    for approval_phase in [
        "phase: test-cli-approval-core",
        "phase: test-cli-approval-control-plane",
        "phase: test-cli-approval-factory-plan",
        "phase: test-cli-approval-factory-queue",
        "phase: test-cli-approval-factory-project",
        "phase: test-cli-approval-factory-other",
        "phase: test-cli-approval-plugin",
        "phase: test-cli-approval-pulse-provider-release",
        "phase: test-cli-approval-workbench-core",
        "phase: test-cli-approval-workbench-project",
        "phase: test-cli-approval-workbench-provider",
        "phase: test-cli-approval-workbench-queue",
        "phase: test-cli-approval-workbench-release-run-support",
    ] {
        assert!(ci.contains(approval_phase), "missing {approval_phase}");
    }
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_plan"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_plan"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_queue_core"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_queue"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_queue"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_project"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_project"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_pack"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_pack"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_verify"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_verify"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_run"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_run"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_app"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_evaluator_closer"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_evaluator"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_closer"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_greenfield_spec_ingest"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_greenfield"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_greenfield_three_os"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_greenfield"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_governed"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_governed"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_factory_replacement"));
    assert!(
        !ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_factory_replacement")
    );
    assert!(ci.contains("cargo test -p ao2-cli --test cli_memory"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_memory"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_evidence"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_evidence"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_core"));
    for old_core_filter in ["cli_can_pause", "cli_report", "cli_template", "cli_version"] {
        assert!(
            !ci.contains(&format!(
                "cargo test -p ao2-cli --test cli_approval_replay {old_core_filter}"
            )),
            "core route should not use cli_approval_replay filter {old_core_filter}"
        );
    }
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_memory"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_memory"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_lists"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_lists"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_api"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_api"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_serve"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_serve"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_operator"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_operator"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_launch"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_launch"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_greenfield"));
    assert!(
        !ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_greenfield")
    );
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_obligation"));
    assert!(
        !ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_obligation")
    );
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_evidence"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_evidence"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_export"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_export"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_factory"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_factory"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_release"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_release"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_run_evidence"));
    assert!(
        !ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_run_evidence")
    );
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_support"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_support"));
    for non_approval_phase in [
        "phase: test-cli-contract-support",
        "phase: test-cli-release-gate-signing-sidecars",
        "phase: test-cli-release-gate-signing-verified",
        "phase: test-cli-release-gate-signing-rejections",
        "phase: test-cli-release-support",
        "phase: test-cli-release-packaging",
        "phase: test-cli-factory-bridge",
        "phase: test-cli-factory-cancel",
        "phase: test-cli-sdd",
    ] {
        assert!(
            ci.contains(non_approval_phase),
            "missing {non_approval_phase}"
        );
    }
    for split_non_approval_phase in [
        "phase: test-cli-contract-gate-signing",
        "phase: test-cli-factory-control",
        "phase: test-cli-release-readiness",
        "phase: test-cli-release-packaging-sdd",
    ] {
        assert!(
            ci.contains(split_non_approval_phase),
            "missing {split_non_approval_phase}"
        );
    }
    assert!(ci.contains("phase: test-workspace-non-cli"));
    assert!(ci.contains("cargo fmt --all -- --check"));
    assert!(ci.contains("cargo test --workspace --exclude ao2-cli"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_provider_run_repair"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_adapter"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_contract"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_control_plane"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_git"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_plugin_pulse"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_plugin_package"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_plugin_consumer_lifecycle"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_plugin_release_candidate"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_plugin_distribution"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_plugin_adapter"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_plugin_wrapper_harness"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_provider"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_pulse"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_release_install"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_release_phase1"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_release"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_report_cockpit"));
    for project_start_suite in [
        "cli_workbench_project_start",
        "cli_workbench_project_start_recovery",
        "cli_workbench_project_start_claim",
        "cli_workbench_project_start_post_continuation",
        "cli_workbench_project_start_release",
    ] {
        assert!(
            ci.contains(&format!(
                "cargo test -p ao2-cli --test {project_start_suite}"
            )),
            "missing direct project-start suite {project_start_suite}"
        );
    }
    assert!(!ci
        .contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_project_start"));
    assert!(ci.contains("cli_workbench_provider"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_provider"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_provider"));
    assert!(ci.contains("cli_workbench_queue"));
    assert!(ci.contains("cargo test -p ao2-cli --test cli_workbench_queue -- --test-threads=1"));
    assert!(!ci.contains("cargo test -p ao2-cli --test cli_approval_replay cli_workbench_queue"));
    assert!(ci.contains("cli_workbench_queue -- --test-threads=1"));
    assert!(ci.contains("cli_provider_run_repair"));
    assert!(!ci.contains("phase: test-cli-approval-replay"));
    assert!(!ci.contains("phase: test-cli-other"));
    assert!(!ci.contains("command: cargo test -p ao2-cli --test cli_approval_replay\n"));
    assert!(ci.contains("cargo test -p ao2-cli"));
    assert!(ci.contains("--test contract_gate_support_signing"));
    assert!(ci.contains("release_gate_accepts_verified"));
    assert!(ci.contains("release_gate_fails"));
    assert!(ci.contains("release_provenance"));
    assert!(ci.contains("--test factory_cancel_transition"));
    assert!(ci.contains("--test release_handoff_checklist"));
    assert!(ci.contains("--test sdd_subcommand"));
    assert!(ci.contains("actions/setup-go"));
    assert!(ci.contains("contains(matrix.phase, 'sdd')"));
    assert!(ci.contains("go-version: '1.22.x'"));
    assert!(ci
        .contains("cargo clippy --locked --workspace --all-targets --all-features -- -D warnings"));
    assert!(ci.contains("cargo build --release -p ao2-cli"));
    assert!(!ci.contains("npm run verify"));
    assert!(ci.contains("timeout-minutes: 15"));
    assert!(ci.contains("timeout_minutes: 20"));
    assert!(ci.contains("cargo deny check -D warnings bans licenses sources advisories"));
    assert!(ci.contains("name: Rust 1.83 MSRV"));
    assert!(ci.contains("dtolnay/rust-toolchain@1.83.0"));
    assert!(ci.contains("cargo +1.83.0 check --locked --workspace --all-targets"));
    assert!(ci.contains("workbench-operator-packet-control-plane-smoke:"));
    assert!(ci.contains("name: Workbench operator packet control-plane smoke ${{ matrix.os }}"));
    assert!(ci.contains("fail-fast: false"));
    assert!(ci.contains("os: [ubuntu-latest, macos-latest, windows-latest]"));
    assert!(ci.contains("runs-on: ${{ matrix.os }}"));
    assert!(ci.contains("Checkout AO2"));
    assert!(ci.contains("path: ao2"));
    assert!(ci.contains("repository: uesugitorachiyo/ao2-control-plane"));
    assert!(ci.contains("path: ao2-control-plane"));
    assert!(ci.contains("working-directory: ao2"));
    assert!(ci.contains("AO2_CONTROL_PLANE_ROOT: ../ao2-control-plane"));
    assert!(ci.contains("AO2_WORKBENCH_OPERATOR_PACKET_CP_PROFILE: debug"));
    assert!(ci.contains("npm run smoke:workbench-operator-packet-control-plane"));
    assert!(ci.contains("ao2-workbench-operator-packet-control-plane-smoke-${{ matrix.os }}"));
    assert!(ci.contains("target/workbench-operator-packet-control-plane-smoke"));
    assert!(ci.contains("workbench-operator-packet-control-plane-smoke-index:"));
    assert!(ci.contains("name: Workbench operator packet control-plane smoke index"));
    assert!(ci.contains("needs: workbench-operator-packet-control-plane-smoke"));
    assert!(ci.contains("actions/download-artifact@v8.0.1"));
    assert!(!ci.contains("actions/download-artifact@v7.0.1"));
    assert!(ci.contains("pattern: ao2-workbench-operator-packet-control-plane-smoke-*"));
    assert!(ci.contains("AO2_WORKBENCH_OPERATOR_PACKET_CP_INDEX_REQUIRED_OS: ubuntu-latest,macos-latest,windows-latest"));
    assert!(ci.contains("npm run smoke:workbench-operator-packet-control-plane:index"));
    assert!(ci.contains("ao2-workbench-operator-packet-control-plane-smoke-index"));

    for release_workflow in [&release_gate, &public_release] {
        assert!(release_workflow.contains("workflow_dispatch:"));
        assert!(!release_workflow.contains("pull_request:"));
        assert!(!release_workflow.contains("\n  push:"));
    }
    assert!(release_gate.contains("npm run gate:full"));
    assert!(public_release.contains("validate_hosted_release_candidates.py"));
    assert!(public_release.contains("npm run verify:replacement"));
    assert!(!public_release.contains("npm run gate:full"));
}

#[test]
fn hosted_release_builds_emit_bound_rust_supply_chain_evidence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("normal CI workflow exists");
    let build =
        fs::read_to_string(root.join("crates/ao2-cli/build.rs")).expect("AO2 build script exists");
    let identity = fs::read_to_string(root.join("crates/ao2-cli/src/build_identity.rs"))
        .expect("AO2 build identity exists");
    let manifest = fs::read_to_string(root.join("crates/ao2-cli/Cargo.toml"))
        .expect("AO2 CLI manifest exists");
    let verification =
        fs::read_to_string(root.join("docs/VERIFICATION.md")).expect("verification guide exists");

    assert!(manifest.contains("[build-dependencies]"));
    assert!(build.contains("AO2_CARGO_LOCK_SHA256"));
    assert!(build.contains("AO2_SOURCE_MODIFIED"));
    assert!(build.contains("--untracked-files=no"));
    assert!(identity.contains("AO_RUST_BUILD_PROVENANCE_V1\\0"));
    assert!(identity.contains("AO2_CARGO_LOCK_SHA256"));
    assert!(identity.contains("AO2_SOURCE_MODIFIED"));
    assert!(identity.contains("#[used]"));

    assert!(workflow.contains("3bb918466ffec789c7cc0a73cd186b57f7958754"));
    assert!(workflow.contains("persist-credentials: false"));
    assert!(workflow.contains("read_rust_binary_metadata.py"));
    assert!(workflow.contains("build_rust_supply_chain_candidate.py"));
    assert!(workflow.contains("verify_supply_chain_policy.py"));
    assert!(workflow.contains("--dependency-lock Cargo.lock"));
    assert!(workflow.contains("metadata_dir=\"target/supply-chain-inputs/${{ matrix.target }}\""));
    assert!(workflow.contains("> \"$metadata_dir/rust-binary-metadata.json\""));
    assert!(workflow.contains("--metadata-json \"$metadata_dir/rust-binary-metadata.json\""));
    assert!(!workflow.contains("> \"$out/rust-binary-metadata.json\""));
    assert!(workflow.contains("name: ao2-supply-chain-${{ matrix.target }}"));
    assert!(workflow.contains("if-no-files-found: error"));
    assert!(verification.contains("ao2-supply-chain-<target>"));
    assert!(verification.contains("does not authorize release or publication"));
}

#[test]
fn build_identity_watches_resolved_symbolic_head_ref() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let build =
        fs::read_to_string(root.join("crates/ao2-cli/build.rs")).expect("AO2 build script exists");

    assert!(build.contains("\"symbolic-ref\", \"-q\", \"HEAD\""));
    assert!(build.contains("\"rev-parse\", \"--git-path\""));
    assert!(build.contains("cargo:rerun-if-changed={}"));
}

#[test]
fn project_declares_apache_license_and_third_party_notice() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("cargo toml exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let readme = fs::read_to_string(root.join("README.md")).expect("readme exists");
    let license = fs::read_to_string(root.join("LICENSE")).expect("license exists");
    let third_party = fs::read_to_string(root.join("docs/THIRD-PARTY-LICENSES.md"))
        .expect("third-party license notice exists");

    assert!(cargo_toml.contains("license = \"Apache-2.0\""));
    assert!(package_json.contains("\"license\": \"Apache-2.0\""));
    assert!(license.contains("Apache License"));
    assert!(license.contains("Version 2.0"));
    assert!(readme.contains("Apache-2.0"));
    assert!(readme.contains("docs/THIRD-PARTY-LICENSES.md"));
    assert!(third_party.contains("Unicode-3.0"));
    assert!(third_party.contains("Zlib"));
    assert!(third_party.contains("Unlicense"));
    assert!(third_party.contains("BSL-1.0"));
}

#[test]
fn install_guide_documents_update_verify_and_provider_fast_start() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let install = fs::read_to_string(root.join("docs/INSTALL.md")).expect("install guide exists");
    let readme = fs::read_to_string(root.join("README.md")).expect("readme exists");

    assert!(install.contains("ao2 install update"));
    assert!(install.contains("ao2 version --json"));
    assert!(install.contains("npm run release:download-verify"));
    assert!(install.contains("npm run smoke:three-os"));
    assert!(install.contains("macOS"));
    assert!(install.contains("Ubuntu"));
    assert!(install.contains("Windows"));
    assert!(install.contains("ao2 provider doctor --provider scripted"));
    assert!(install.contains("ao2 provider matrix --json"));
    assert!(install.contains("ao2 run --template bug-fix"));
    assert!(install.contains("## Uninstall"));
    assert!(install.contains("ao2.install-verification.json"));
    assert!(install.contains(".ao2/"));
    assert!(install.contains("AO2_INSTALL_DIR"));
    assert!(readme.contains("docs/INSTALL.md"));
}

#[test]
fn install_guide_documents_windows_safe_rollback_runner() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let install = fs::read_to_string(root.join("docs/INSTALL.md")).expect("install guide exists");
    let troubleshooting =
        fs::read_to_string(root.join("docs/TROUBLESHOOTING.md")).expect("troubleshooting exists");
    let support = fs::read_to_string(root.join("docs/SUPPORT-REPRODUCTION.md"))
        .expect("support reproduction exists");

    for text in [&install, &troubleshooting] {
        assert!(text.contains("Windows-safe rollback"));
        assert!(text.contains("Use an extracted or alternate"));
        assert!(text.contains("ao2.exe install rollback --install-dir"));
        assert!(text.contains("rollback_status=blocked_active_executable"));
    }

    assert!(support.contains("Windows rollback runner"));
    assert!(support.contains("rollback_status=blocked_active_executable"));
    assert!(support.contains("Do not paste private Windows user paths"));
}

#[test]
fn native_windows_smoke_assets_are_manual_and_exercise_installed_binary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/smoke-windows-release.ps1"))
        .expect("native Windows smoke script exists");
    let workflow = fs::read_to_string(root.join(".github/workflows/windows-release-smoke.yml"))
        .expect("manual Windows smoke workflow exists");

    assert!(script.contains("param("));
    assert!(script.contains("install.ps1"));
    assert!(script.contains("ao2.exe"));
    assert!(script.contains("RELEASE-MANIFEST.json"));
    assert!(script.contains("version --json"));
    assert!(script.contains("Write-Utf8NoBom"));
    assert!(script.contains("UTF8Encoding($false)"));
    assert!(script.contains("adapter doctor --provider scripted"));
    assert!(script.contains("provider matrix --json"));
    assert!(script.contains("install rollback"));
    assert!(script.contains("$RollbackRunner"));
    assert!(script.contains("$RollbackRunner install rollback"));
    assert!(!script.contains("$Ao2 install rollback"));
    assert!(script.contains("rollback_status=blocked_active_executable"));
    assert!(script.contains("Windows-safe rollback runner"));
    assert!(script.contains("windows_install_rollback=passed"));
    assert!(script.contains("rollback_runner=$RollbackRunner"));
    assert!(script.contains("git -C $RepoDir init"));
    assert!(script.contains("git -C $RepoDir config user.email"));
    assert!(script.contains("git -C $RepoDir config user.name"));
    assert!(script.contains("git -C $RepoDir commit"));
    assert!(script.contains("command: powershell -NoProfile -Command if"));
    assert!(script.contains("run $WorkflowPath"));
    assert!(script.contains("replay windows-install-smoke-repair"));
    assert!(script.contains("expected ok after repair"));
    assert!(script.contains("status=WaitingForApproval"));
    assert!(script.contains("approve $PendingApproval.ticket_id"));
    assert!(script.contains("status=approved"));
    assert!(script.contains("run --resume windows-install-smoke-repair"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("\n  push:"));
    assert!(workflow.contains("windows-latest"));
    assert!(workflow.contains("scripts/smoke-windows-release.ps1"));
    assert!(workflow.contains("$manifestLine = Select-String"));
    assert!(workflow.contains("$manifestLine.Line -split"));
}

#[cfg(windows)]
#[test]
fn windows_installed_binary_self_rollback_reports_safe_runner_diagnostic() {
    let ao2 = Path::new(env!("CARGO_BIN_EXE_ao2"));
    let install_dir = tempfile::tempdir().expect("install dir");
    let installed = install_dir.path().join("ao2.exe");
    let rollback = install_dir.path().join("ao2.exe.rollback");
    fs::copy(ao2, &installed).expect("seed installed ao2.exe");
    fs::copy(ao2, &rollback).expect("seed rollback ao2.exe");

    let output = Command::new(&installed)
        .args([
            "install",
            "rollback",
            "--install-dir",
            install_dir.path().to_str().expect("utf8 install dir"),
            "--target-label",
            "windows-x86_64",
        ])
        .output()
        .expect("run installed rollback");

    assert!(
        !output.status.success(),
        "self rollback should fail with a diagnostic on Windows"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("rollback_status=blocked_active_executable"));
    assert!(stderr.contains("Windows cannot replace the running ao2.exe"));
    assert!(stderr.contains("Use an extracted or alternate ao2.exe runner"));
    assert!(stderr.contains("ao2.exe install rollback --install-dir"));
    assert!(stderr.contains(installed.to_str().expect("utf8 installed path")));
    assert!(stderr.contains(rollback.to_str().expect("utf8 rollback path")));
}

#[test]
fn cross_target_release_builds_pin_source_commit_and_hosted_smoke_checks_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for script in [
        "scripts/package-linux-aarch64-docker.sh",
        "scripts/package-linux-x86_64-docker.sh",
        "scripts/package-windows-x86_64-docker.sh",
    ] {
        let source = fs::read_to_string(root.join(script)).expect("package script exists");
        assert!(
            source.contains("AO2_BUILD_GIT_COMMIT"),
            "{script} must pin build commit"
        );
    }
    let hosted = fs::read_to_string(root.join("scripts/release-archive-hosted-smoke.sh"))
        .expect("hosted smoke exists");
    let hosted_windows = fs::read_to_string(root.join("scripts/release-archive-hosted-smoke.ps1"))
        .expect("Windows hosted smoke exists");
    assert!(hosted.contains("BUILD-PROVENANCE.json"));
    assert!(hosted.contains("SBOM.cdx.json"));
    assert!(hosted.contains("UNINSTALL.txt"));
    assert!(hosted.contains("build_profile"));
    assert!(hosted.contains("git_commit"));
    assert!(hosted_windows.contains("$ExpectedCommit"));
    assert!(hosted_windows.contains("AO2_PACKAGED_GIT_COMMIT"));
    assert!(hosted_windows.contains("AO2_PACKAGED_BUILD_PROFILE"));
    assert!(hosted_windows.contains("BUILD-PROVENANCE.json"));
    assert!(hosted_windows.contains("build_profile"));
    assert!(hosted_windows.contains("git_commit"));
    assert!(hosted_windows.contains("release"));
}

#[test]
fn evidence_migration_contract_has_an_executable_consumer_gate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let migration = fs::read_to_string(root.join("docs/release/0.5-evidence-migration.md"))
        .expect("migration guide exists");
    let gate = fs::read_to_string(root.join("scripts/evidence-compatibility-gate.sh"))
        .expect("compatibility gate exists");

    assert!(migration.contains("ao2.event.payload.v1"));
    assert!(migration.contains("ao2.event.policy-integrity.v2"));
    assert!(migration.contains("legacy"));
    assert!(migration.contains("AO2_CONTROL_PLANE_ROOT"));
    assert!(
        gate.contains("event_hash_vectors_preserve_legacy_and_policy_bound_migration_contracts")
    );
    assert!(gate.contains("ao2_canonical_v1_matches_shared_golden_vectors"));
    assert!(
        gate.contains("post_signed_evidence_pack_verifies_over_exact_bytes_not_reserialization")
    );
    assert!(gate.contains(
        "post_signed_evidence_pack_stores_exact_signed_bytes_ignoring_evidence_pack_field"
    ));
}

#[cfg(unix)]
#[test]
fn unix_installer_installs_packaged_binary_without_admin_access() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let out_dir = tempfile::tempdir().expect("tempdir");
    let extract_dir = tempfile::tempdir().expect("extract tempdir");
    let install_dir = tempfile::tempdir().expect("install tempdir");

    let output = Command::new(ao2)
        .args([
            "release",
            "package",
            "--out-dir",
            out_dir.path().to_str().expect("utf8 out dir"),
            "--version",
            "9.9.9-test",
        ])
        .output()
        .expect("run ao2 release package");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("package command prints json");

    let unpack = Command::new("tar")
        .args([
            "-xzf",
            json["archive"].as_str().expect("archive"),
            "-C",
            extract_dir.path().to_str().expect("utf8 extract dir"),
        ])
        .output()
        .expect("extract archive");
    assert!(
        unpack.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&unpack.stdout),
        String::from_utf8_lossy(&unpack.stderr)
    );

    let install = Command::new(sh_command())
        .arg("install.sh")
        .current_dir(extract_dir.path())
        .env("AO2_INSTALL_DIR", install_dir.path())
        .output()
        .expect("run install.sh");
    assert!(
        install.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let installed = install_dir.path().join("ao2");
    assert!(installed.is_file());
    let install_verification = install_dir.path().join("ao2.install-verification.json");
    assert!(
        install_verification.is_file(),
        "archive install.sh must write install verification sidecar"
    );
    let install_verification_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&install_verification).unwrap()).unwrap();
    assert_eq!(
        install_verification_json["schema_version"],
        "ao2.install-verification-evidence.v1"
    );
    assert_eq!(install_verification_json["status"], "verified");
    assert_eq!(install_verification_json["install_status"], "installed");
    assert_eq!(
        Path::new(
            install_verification_json["installed_binary"]
                .as_str()
                .expect("installed binary path")
        )
        .canonicalize()
        .unwrap(),
        installed.canonicalize().unwrap()
    );
    assert_eq!(
        install_verification_json["offline_verification"]["status"],
        "verified"
    );
    assert_eq!(
        install_verification_json["provider_api_keys_required"],
        false
    );
    assert_eq!(
        install_verification_json["control_plane_approves_release"],
        false
    );
    assert_eq!(install_verification_json["mutates_ao_artifacts"], false);
    assert_eq!(
        install_verification_json["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    let help = Command::new(&installed)
        .arg("--help")
        .output()
        .expect("installed ao2 runs");
    assert!(
        help.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );

    let verify = Command::new(sh_command())
        .arg("verify-release.sh")
        .current_dir(extract_dir.path())
        .output()
        .expect("run verify-release.sh");
    assert!(
        verify.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&verify.stdout),
        String::from_utf8_lossy(&verify.stderr)
    );
    let verification: serde_json::Value =
        serde_json::from_slice(&verify.stdout).expect("verify-release.sh prints json");
    assert_eq!(
        verification["schema_version"],
        "ao2.release-archive-offline-verification.v1"
    );
    assert_eq!(verification["status"], "verified");
    assert_eq!(verification["checksum_file"], "SHA256SUMS");
}

#[cfg(windows)]
#[test]
fn windows_installer_writes_install_verification_sidecar_without_admin_access() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let out_dir = tempfile::tempdir().expect("tempdir");
    let extract_dir = tempfile::tempdir().expect("extract tempdir");
    let install_dir = tempfile::tempdir().expect("install tempdir");

    let output = Command::new(ao2)
        .args([
            "release",
            "package",
            "--out-dir",
            out_dir.path().to_str().expect("utf8 out dir"),
            "--version",
            "9.9.9-test",
            "--target-label",
            "windows-x86_64",
        ])
        .output()
        .expect("run ao2 release package");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("package command prints json");

    let archive = fs::File::open(json["archive"].as_str().expect("archive")).expect("open archive");
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(extract_dir.path()).expect("extract archive");

    let powershell = if Command::new("pwsh")
        .arg("-NoProfile")
        .arg("-Command")
        .arg("$PSVersionTable.PSVersion | Out-Null")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        "pwsh"
    } else {
        "powershell"
    };
    let install = Command::new(powershell)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            "install.ps1",
        ])
        .current_dir(extract_dir.path())
        .env("AO2_INSTALL_DIR", install_dir.path())
        .output()
        .expect("run install.ps1");
    assert!(
        install.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let installed = install_dir.path().join("ao2.exe");
    assert!(installed.is_file());
    let install_verification = install_dir.path().join("ao2.exe.install-verification.json");
    assert!(
        install_verification.is_file(),
        "archive install.ps1 must write install verification sidecar"
    );
    let install_verification_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&install_verification).unwrap()).unwrap();
    assert_eq!(
        install_verification_json["schema_version"],
        "ao2.install-verification-evidence.v1"
    );
    assert_eq!(install_verification_json["status"], "verified");
    assert_eq!(install_verification_json["install_status"], "installed");
    assert_eq!(
        Path::new(
            install_verification_json["installed_binary"]
                .as_str()
                .expect("installed binary path")
        )
        .canonicalize()
        .unwrap(),
        installed.canonicalize().unwrap()
    );
    assert_eq!(
        install_verification_json["offline_verification"]["status"],
        "verified"
    );
    assert_eq!(
        install_verification_json["provider_api_keys_required"],
        false
    );
    assert_eq!(
        install_verification_json["control_plane_approves_release"],
        false
    );
    assert_eq!(install_verification_json["mutates_ao_artifacts"], false);
    assert_eq!(
        install_verification_json["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
}

#[cfg(windows)]
#[test]
fn windows_verifier_validates_packaged_archive_checksums() {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let out_dir = tempfile::tempdir().expect("tempdir");
    let extract_dir = tempfile::tempdir().expect("extract tempdir");

    let output = Command::new(ao2)
        .args([
            "release",
            "package",
            "--out-dir",
            out_dir.path().to_str().expect("utf8 out dir"),
            "--version",
            "9.9.9-test",
            "--target-label",
            "windows-x86_64",
        ])
        .output()
        .expect("run ao2 release package");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("package command prints json");

    let archive = fs::File::open(json["archive"].as_str().expect("archive")).expect("open archive");
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(extract_dir.path()).expect("extract archive");

    let powershell = if Command::new("pwsh")
        .arg("-NoProfile")
        .arg("-Command")
        .arg("$PSVersionTable.PSVersion | Out-Null")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        "pwsh"
    } else {
        "powershell"
    };
    let verify = Command::new(powershell)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            "Verify-Release.ps1",
        ])
        .current_dir(extract_dir.path())
        .output()
        .expect("run Verify-Release.ps1");
    assert!(
        verify.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&verify.stdout),
        String::from_utf8_lossy(&verify.stderr)
    );
    let verification: serde_json::Value =
        serde_json::from_slice(&verify.stdout).expect("Verify-Release.ps1 prints json");
    assert_eq!(
        verification["schema_version"],
        "ao2.release-archive-offline-verification.v1"
    );
    assert_eq!(verification["status"], "verified");
    assert_eq!(verification["checksum_file"], "SHA256SUMS");
}

fn archive_entries(path: &Path) -> Vec<String> {
    let archive = fs::File::open(path).expect("open archive");
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    archive
        .entries()
        .expect("read archive entries")
        .map(|entry| {
            let entry = entry.expect("archive entry");
            entry
                .path()
                .expect("entry path")
                .to_string_lossy()
                .trim_start_matches("./")
                .to_string()
        })
        .collect()
}

fn write_factory_app_run_fixture(root: &Path, control_plane_approves_release: bool) -> PathBuf {
    let run_dir = root.join("run");
    fs::create_dir_all(&run_dir).expect("create run dir");
    let app_run = run_dir.join("factory-app-run.json");
    let evaluator_rubric = run_dir.join("evaluator-rubric.json");
    let greenfield_governed_run = run_dir.join("greenfield-governed-run.json");
    let greenfield_ingest = run_dir.join("greenfield-ingest.json");
    let plan = run_dir.join("plan.json");
    let governed_run = run_dir.join("governed-run.json");
    let evidence_pack = run_dir.join("evidence-pack.json");
    let evaluator_decision = run_dir.join("evaluator-decision.json");

    for (path, body) in [
        (
            &evaluator_rubric,
            r#"{"schema_version":"ao2.factory-evaluator-rubric.v1","status":"accepted"}"#,
        ),
        (
            &greenfield_governed_run,
            r#"{"schema_version":"ao2.factory-greenfield-governed-run.v1","status":"accepted"}"#,
        ),
        (
            &greenfield_ingest,
            r#"{"schema_version":"ao2.greenfield-ingest.v1","status":"ready"}"#,
        ),
        (
            &plan,
            r#"{"schema_version":"ao2.factory-plan.v1","status":"ready"}"#,
        ),
        (
            &governed_run,
            r#"{"schema_version":"ao2.factory-run.v1","status":"accepted"}"#,
        ),
        (
            &evidence_pack,
            r#"{"schema_version":"ao2.evidence-pack.v1","status":"packed"}"#,
        ),
        (
            &evaluator_decision,
            r#"{"schema":"factory-v3/ao2-release-evaluator-decision/v1","status":"accepted"}"#,
        ),
    ] {
        fs::write(path, body).expect("write fixture artifact");
    }

    let app_run_json = serde_json::json!({
        "schema_version": "ao2.factory-app-run.v1",
        "status": "accepted",
        "run_id": "bundle-fixture",
        "rubric_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "factory_replacement_boundary": {
            "ao2_execution_owner": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": control_plane_approves_release,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "artifacts": {
            "factory_app_run": app_run,
            "evaluator_rubric": evaluator_rubric,
            "greenfield_governed_run": greenfield_governed_run,
            "greenfield_ingest": greenfield_ingest,
            "plan": plan,
            "governed_run": governed_run,
            "evidence_pack": evidence_pack,
            "evaluator_decision": evaluator_decision
        },
        "release_review": {
            "ready": true,
            "rubric_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "evaluator_rubric": evaluator_rubric,
            "artifacts": {
                "evaluator_rubric": evaluator_rubric,
                "plan": plan,
                "governed_run": governed_run,
                "evidence_pack": evidence_pack,
                "evaluator_decision": evaluator_decision
            },
            "downstream_contract": {
                "verifier_outputs_must_reference": "rubric_sha256",
                "closer_outputs_must_reference": "rubric_sha256",
                "factory_v3_may_compare_or_audit": true,
                "factory_v3_must_not_be_primary_producer": true
            },
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": control_plane_approves_release
        },
        "app_run_checklist": {
            "ao2_derived_signed_evaluator_rubric": true,
            "release_review_artifacts_ready": true,
            "verifier_outputs_reference_rubric_sha256": true,
            "closer_outputs_reference_rubric_sha256": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence"
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": control_plane_approves_release,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    fs::write(
        &app_run,
        serde_json::to_string_pretty(&app_run_json).expect("fixture json"),
    )
    .expect("write app-run fixture");
    app_run
}

fn build_test_evidence_bundle(ao2: &str, root: &Path, include_secret: bool) -> PathBuf {
    let out_dir = root.join("bundle-out");
    let readiness = root.join("readiness.json");
    let checklist = root.join("handoff-checklist.json");
    let decision = root.join("evaluator-decision.json");
    let install_verification = root.join("install-verification.json");
    fs::write(
        &readiness,
        if include_secret {
            r#"{"schema_version":"ao2.cp-release-readiness.v1","token":"Authorization: Bearer should-redact"}"#
        } else {
            r#"{"schema_version":"ao2.cp-release-readiness.v1","status":"ready"}"#
        },
    )
    .expect("write readiness");
    fs::write(
        &checklist,
        r#"{"schema":"factory-v3/ao2-release-handoff-checklist/v1","status":"ready_for_evaluator_closer"}"#,
    )
    .expect("write checklist");
    fs::write(
        &decision,
        r#"{"schema":"factory-v3/ao2-release-evaluator-decision/v1","status":"accepted"}"#,
    )
    .expect("write decision");
    fs::write(
        &install_verification,
        r#"{"schema_version":"ao2.install-verification-evidence.v1","status":"verified","offline_verification":{"status":"verified"},"provider_api_keys_required":false,"control_plane_approves_release":false,"mutates_ao_artifacts":false}"#,
    )
    .expect("write install verification");

    let output = Command::new(ao2)
        .args([
            "release",
            "evidence-bundle",
            "--out-dir",
            out_dir.to_str().expect("utf8 out dir"),
            "--artifact",
            &format!("readiness={}", readiness.display()),
            "--artifact",
            &format!("handoff-checklist={}", checklist.display()),
            "--artifact",
            &format!("evaluator-decision={}", decision.display()),
            "--artifact",
            &format!("install-verification={}", install_verification.display()),
            "--json",
        ])
        .output()
        .expect("run ao2 release evidence-bundle");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("evidence-bundle prints json");
    PathBuf::from(json["archive"].as_str().expect("archive path"))
}

fn tamper_evidence_bundle_without_install_checksum(archive: &Path, root: &Path) -> PathBuf {
    let extract_dir = root.join("tampered-evidence-bundle");
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir).expect("remove stale tamper dir");
    }
    fs::create_dir_all(&extract_dir).expect("create tamper dir");

    let input = fs::File::open(archive).expect("open archive");
    let decoder = flate2::read::GzDecoder::new(input);
    let mut tar = tar::Archive::new(decoder);
    tar.unpack(&extract_dir).expect("extract archive");

    let checksums = extract_dir.join("SHA256SUMS");
    let filtered = fs::read_to_string(&checksums)
        .expect("read checksums")
        .lines()
        .filter(|line| !line.contains("artifacts/install-verification/"))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    fs::write(&checksums, filtered).expect("write tampered checksums");

    let tampered = root.join("tampered-evidence-bundle.tar.gz");
    let output = fs::File::create(&tampered).expect("create tampered archive");
    let encoder = flate2::write::GzEncoder::new(output, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder
        .append_dir_all(".", &extract_dir)
        .expect("append tampered archive");
    let encoder = builder.into_inner().expect("finish tar");
    encoder.finish().expect("finish gzip");
    tampered
}

fn sh_command() -> PathBuf {
    ao2_adapters::posix_shell_command().unwrap_or_else(|| PathBuf::from("sh"))
}

fn output_contains_path_line(output: &str, key: &str, path: &Path) -> bool {
    let expected = format!("{key}={}", path.display());
    output.contains(&expected)
        || output
            .replace('\\', "/")
            .contains(&expected.replace('\\', "/"))
}

fn archive_text_entry(path: &Path, wanted: &str) -> String {
    let archive = fs::File::open(path).expect("open archive");
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().expect("read archive entries") {
        let mut entry = entry.expect("archive entry");
        let path = entry
            .path()
            .expect("entry path")
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        if path == wanted {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut entry, &mut content)
                .expect("read text archive entry");
            return content;
        }
    }
    panic!("missing archive entry {wanted}");
}
