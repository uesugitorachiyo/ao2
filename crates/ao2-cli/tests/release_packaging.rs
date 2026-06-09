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
    assert!(entries.iter().any(|entry| entry == "SHA256SUMS"));
    assert!(entries.iter().any(|entry| entry == "RELEASE-MANIFEST.json"));

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
    assert!(entries.iter().any(|entry| entry == "SHA256SUMS"));
    assert!(entries.iter().any(|entry| entry == "RELEASE-MANIFEST.json"));

    let manifest = archive_text_entry(
        Path::new(json["archive"].as_str().expect("archive")),
        "RELEASE-MANIFEST.json",
    );
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest).expect("release manifest is json");
    assert_eq!(manifest_json["target"], "windows-x86_64");
    assert_eq!(manifest_json["binary"], "ao2.exe");
    assert_eq!(manifest_json["binary_path"], "bin/ao2.exe");
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
    assert_eq!(json["artifact_count"], 3);
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

    let manifest_text = archive_text_entry(archive, "EVIDENCE-BUNDLE-MANIFEST.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("manifest is json");
    assert_eq!(manifest["schema_version"], "ao2.release-evidence-bundle.v1");
    assert_eq!(
        manifest["artifacts"].as_array().expect("artifacts").len(),
        3
    );
    assert_eq!(
        manifest["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    let checksums = archive_text_entry(archive, "SHA256SUMS");
    assert!(checksums.contains("artifacts/readiness/readiness.json"));
    assert!(checksums.contains("EVIDENCE-BUNDLE-MANIFEST.json"));
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
    assert_eq!(json["artifact_count"], 3);
    assert_eq!(json["failure_count"], 0);
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
    assert_eq!(json["artifact_count"], 3);
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

    assert!(script.contains("AO2_MACOS_ARCHIVE"));
    assert!(script.contains("AO2_LINUX_ARCHIVE"));
    assert!(script.contains("AO2_LINUX_X86_64_ARCHIVE"));
    assert!(script.contains("AO2_WINDOWS_ARCHIVE"));
    assert!(script.contains("AO2_UBUNTU_SSH_TARGET"));
    assert!(script.contains("linux_x86_64_remote_smoke=passed"));
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

#[test]
fn cli_signature_helpers_use_native_crypto_without_openssl_shellouts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source =
        fs::read_to_string(root.join("crates/ao2-cli/src/main.rs")).expect("cli source exists");
    let replay_tests = fs::read_to_string(root.join("crates/ao2-cli/tests/cli_approval_replay.rs"))
        .expect("cli approval replay tests exist");
    for function_name in [
        "verify_release_archive_signature",
        "derive_public_key_from_private_key",
        "sign_file_with_private_key",
        "verify_file_signature",
        "verify_release_provenance_signature",
    ] {
        let function_source = function_body_source(&source, function_name);
        assert!(
            !function_source.contains("ProcessCommand::new(\"openssl\")"),
            "{function_name} must not shell out to openssl"
        );
    }
    assert!(source.contains("RsaPrivateKey"));
    assert!(source.contains("RsaPublicKey"));
    assert!(
        !replay_tests.contains("Command::new(\"openssl\")"),
        "integration tests must generate signing keys through native AO2 helpers"
    );
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
    let cli_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/main.rs")).expect("cli source exists");

    assert!(three_os.contains("AO2_WINDOWS_SSH_TARGET"));
    assert!(three_os.contains("AO2_UBUNTU_SSH_TARGET"));
    assert!(three_os.contains("AO2_LINUX_X86_64_ARCHIVE"));
    assert!(three_os.contains("linux_x86_64_remote_smoke"));
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
    assert!(release_archives.contains("macos_install_smoke=passed"));
    assert!(release_archives.contains("ubuntu_install_smoke=passed"));
    assert!(release_archives.contains("windows_static_smoke=passed"));

    assert!(gate.contains("MIT OR Apache-2.0"));
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
    assert!(cli_source.contains("ao2.phase1-replacement-promotion-inputs-verification.v1"));
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
    assert!(download.contains("AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY"));
    assert!(download.contains("AO2_RELEASE_ROLLBACK_VERIFY"));
    assert!(download.contains("release-rollback-summary.json"));
    assert!(download.contains("ao2.release-rollback-summary.v1"));
    assert!(download.contains("macos_download_rollback=passed"));
    assert!(download.contains("scripts/smoke-windows-release.ps1"));
    assert!(download.contains("windows_download_verify=passed"));
    assert!(download.contains("windows_download_rollback=passed"));
    assert!(download.contains("ubuntu_download_rollback=passed"));
    assert!(download.contains("release_download_verify=passed"));

    assert!(release_ship.contains("AO2_RELEASE_COMPARISON_DIR"));
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
    assert!(cli_source
        .contains("Keep Releases<input id=\"release-retention-keep-releases\" value=\"3\">"));
    assert!(cli_source
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
        "ao2-risky-pr-golden-release-support-bundle",
    ] {
        assert!(
            ci.contains(needle),
            "missing risky-pr golden CI marker: {needle}"
        );
    }
    assert!(verification.contains("Risky PR golden release support bundle artifacts"));
    assert!(verification.contains("ao2-risky-pr-golden-release-support-bundle"));
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
fn release_build_all_script_and_manual_workflow_cover_public_release_sequence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/release-build-all.sh"))
        .expect("release build-all script exists");
    let ship_script = fs::read_to_string(root.join("scripts/release-ship.sh"))
        .expect("release ship script exists");
    let workflow = fs::read_to_string(root.join(".github/workflows/public-release-build.yml"))
        .expect("manual public release build workflow exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");

    assert!(script.contains("npm run build:release"));
    assert!(script.contains("npm run package:local"));
    assert!(script.contains("npm run package:linux:aarch64:docker"));
    assert!(script.contains("npm run package:linux:x86_64:docker"));
    assert!(script.contains("npm run package:windows:x86_64:docker"));
    assert!(script.contains("npm run release:sign-provenance"));
    assert!(script.contains("npm run release:verify-provenance"));
    assert!(script.contains("release_build_all=passed"));

    assert!(package_json.contains("\"release:build-all\""));
    assert!(package_json.contains("\"release:ship\""));

    assert!(ship_script.contains("AO2_RELEASE_SHIP_CONFIRM"));
    assert!(ship_script.contains("npm run verify"));
    assert!(ship_script.contains("npm run release:build-all"));
    assert!(ship_script.contains("AO2_REQUIRE_NATIVE_WINDOWS_SMOKE:-1"));
    assert!(ship_script.contains("npm run smoke:three-os"));
    assert!(ship_script.contains("npm run release:gate"));
    assert!(ship_script.contains("git tag -a"));
    assert!(ship_script.contains("gh release create"));
    assert!(ship_script.contains("dist-linux-x86_64/ao2-\"$AO2_VERSION\"-linux-x86_64.tar.gz"));
    assert!(ship_script.contains("dist-provenance/ao2-\"$AO2_VERSION\"-linux-x86_64.tar.gz.sha256"));
    assert!(
        ship_script.contains("dist-provenance/ao2-\"$AO2_VERSION\"-macos-aarch64.tar.gz.sha256")
    );
    assert!(ship_script.contains("dist-provenance/ao2-\"$AO2_VERSION\"-macos-aarch64.tar.gz.sig"));
    assert!(ship_script.contains("AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY=1"));
    assert!(ship_script.contains("npm run release:download-verify"));
    assert!(ship_script.contains("doctor --json --release"));
    assert!(ship_script.contains("release_ship=passed"));

    assert!(workflow.contains("workflow_dispatch:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("\n  push:"));
    assert!(workflow.contains("npm run release:build-all"));
    assert!(workflow.contains("actions/upload-artifact@v7.0.1"));
}

#[test]
fn linux_x86_64_docker_packaging_constrains_emulated_build_parallelism() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("scripts/package-linux-x86_64-docker.sh"))
        .expect("linux x86_64 docker packaging script exists");

    assert!(script.contains("AO2_LINUX_X86_64_CARGO_BUILD_JOBS"));
    assert!(script.contains("CARGO_BUILD_JOBS=\"$AO2_LINUX_X86_64_CARGO_BUILD_JOBS\""));
    assert!(script.contains("CARGO_INCREMENTAL=0"));
}

#[test]
fn w4_release_workflows_include_no_factory_v3_guard_artifacts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let release_gate = fs::read_to_string(root.join(".github/workflows/release-gate.yml"))
        .expect("release gate workflow exists");
    let public_release =
        fs::read_to_string(root.join(".github/workflows/public-release-build.yml"))
            .expect("public release build workflow exists");
    let w4_roadmap = fs::read_to_string(root.join("docs/roadmap/PHASE-2-W4-CI-INTEGRATION.md"))
        .expect("W4 roadmap exists");
    let ready_to_ship = fs::read_to_string(root.join("docs/release/READY-TO-SHIP.md"))
        .expect("ready-to-ship release runbook exists");

    for artifact in [&release_gate, &public_release] {
        assert!(artifact.contains("npm run verify:no-factory-v3"));
        assert!(artifact.contains("AO2_HOSTED_RELEASE_GATE"));
        assert!(artifact.contains("AO2_REQUIRE_NATIVE_WINDOWS_SMOKE"));
        assert!(artifact.contains("AO2_ALLOW_UNSIGNED_OBLIGATION_GATES"));
        assert!(artifact.contains("target/no-factory-v3-green-path/"));
        assert!(artifact.contains("target/release-gate-with-replacement/"));
        assert!(artifact.contains("if-no-files-found: warn"));
    }
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
    assert!(ci.contains("cargo test -p ao2-cli --test cli_approval_replay"));
    assert!(ci.contains("cli_workbench_project_start"));
    assert!(ci.contains("cli_workbench_provider"));
    assert!(ci.contains("cli_workbench_queue"));
    assert!(ci.contains("cli_workbench_queue -- --test-threads=1"));
    assert!(ci.contains("cli_repair"));
    assert!(ci.contains("cli_report"));
    assert!(ci.contains("cli_run"));
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
    assert!(ci.contains("cargo clippy --workspace --all-targets -- -D warnings"));
    assert!(ci.contains("cargo build --release -p ao2-cli"));
    assert!(!ci.contains("npm run verify"));
    assert!(ci.contains("timeout-minutes: 15"));
    assert!(ci.contains("timeout_minutes: 20"));
    assert!(ci.contains("cargo deny check bans licenses sources advisories"));
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
        assert!(release_workflow.contains("npm run gate:full"));
    }
}

#[test]
fn project_declares_dual_mit_or_apache_license_and_third_party_notice() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("cargo toml exists");
    let package_json = fs::read_to_string(root.join("package.json")).expect("package json exists");
    let readme = fs::read_to_string(root.join("README.md")).expect("readme exists");
    let license = fs::read_to_string(root.join("LICENSE")).expect("license exists");
    let mit_license = fs::read_to_string(root.join("LICENSE-MIT")).expect("MIT license exists");
    let apache_license =
        fs::read_to_string(root.join("LICENSE-APACHE")).expect("Apache license exists");
    let third_party = fs::read_to_string(root.join("docs/THIRD-PARTY-LICENSES.md"))
        .expect("third-party license notice exists");

    assert!(cargo_toml.contains("license = \"MIT OR Apache-2.0\""));
    assert!(package_json.contains("\"license\": \"MIT OR Apache-2.0\""));
    assert!(license.contains("Apache License, Version 2.0"));
    assert!(license.contains("MIT License"));
    assert!(mit_license.contains("MIT License"));
    assert!(apache_license.contains("Apache License"));
    assert!(readme.contains("MIT OR Apache-2.0"));
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
    assert!(readme.contains("docs/INSTALL.md"));
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
    assert!(script.contains("windows_install_rollback=passed"));
    assert!(script.contains("command: powershell -NoProfile -Command if"));
    assert!(script.contains("run $WorkflowPath"));
    assert!(script.contains("replay windows-install-smoke-repair"));
    assert!(script.contains("expected ok after repair"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(!workflow.contains("pull_request:"));
    assert!(!workflow.contains("\n  push:"));
    assert!(workflow.contains("windows-latest"));
    assert!(workflow.contains("scripts/smoke-windows-release.ps1"));
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

fn function_body_source<'a>(source: &'a str, function_name: &str) -> &'a str {
    let start = source
        .find(&format!("fn {function_name}"))
        .unwrap_or_else(|| panic!("{function_name} exists"));
    let tail = &source[start + 1..];
    let end = tail.find("\nfn ").unwrap_or(tail.len()) + 1;
    &source[start..start + end]
}
