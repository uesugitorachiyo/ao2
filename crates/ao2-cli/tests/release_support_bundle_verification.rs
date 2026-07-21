use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn ao2<const N: usize>(args: [&str; N]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
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

fn write_bundle(path: &Path, mut overlay: serde_json::Value) {
    let mut bundle = serde_json::json!({
        "schema_version": "ao2.cp-release-support-bundle.v1",
        "release_assembly": {
            "schema_version": "ao2.cp-release-assembly.v1",
            "status": "assembled",
            "candidate_correlation": "matched",
            "candidate_correlation_detail": candidate_correlation(),
            "control_plane_approves_release": false
        },
        "readiness": {
            "schema_version": "ao2.cp-release-readiness.v1",
            "status": "ready",
            "candidate_correlation": candidate_correlation(),
            "operator_decision": {
                "control_plane_approves_release": false,
                "factory_v3_evaluator_closer_required": true
            }
        },
        "handoff": {
            "schema_version": "factory-v3/ao2-release-handoff-checklist/v1",
            "status": "ready_for_evaluator_closer",
            "candidate_correlation": candidate_correlation(),
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        },
        "cockpit": {
            "schema_version": "ao2.cp-release-cockpit.v1",
            "status": "ready",
            "candidate_correlation": candidate_correlation()
        },
        "evaluator_decision": {
            "schema_version": "factory-v3/ao2-release-evaluator-decision/v1",
            "status": "accepted",
            "decision": "accept_phase1_release_candidate",
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        },
        "storage_support": {"schema_version": "ao2.cp-storage-support.v1", "status": "ready"},
        "replay": {"status": "accepted", "digest_failures": []},
        "report_contract_verification": passed_report_contract_verification(),
        "install_verification": passed_install_verification(),
        "hosted_release_smoke": passed_hosted_release_smoke(),
        "operator_evidence": {
            "factory_v3_evaluator_closer_required": true,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer",
            "control_plane_approves_release": false
        },
        "ci_evidence_index": test_ci_evidence_index(),
        "trust_boundary": {
            "role": "read_only_observer",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        }
    });
    add_portable_manifest(&mut bundle);
    if !overlay.is_null() {
        merge_json(&mut bundle, &mut overlay);
    }
    fs::write(path, serde_json::to_string_pretty(&bundle).unwrap()).unwrap();
}

fn shared_release_support_fixture() -> serde_json::Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = root
        .join("tests")
        .join("fixtures")
        .join("release-support-bundle-contract-v1.json");
    serde_json::from_str(&fs::read_to_string(&fixture).expect("read shared fixture"))
        .expect("fixture is JSON")
}

#[test]
fn shared_release_support_contract_fixture_is_accepted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = root
        .join("tests")
        .join("fixtures")
        .join("release-support-bundle-contract-v1.json");
    let bundle_json = shared_release_support_fixture();
    let bundle_sha256 = canonical_sha256(&bundle_json);
    let temp = tempfile::tempdir().unwrap();
    let checksums_path = temp.path().join("SHA256SUMS");
    let fixture_name = fixture.file_name().unwrap().to_str().unwrap();
    fs::write(
        &checksums_path,
        format!("{bundle_sha256}  {fixture_name}\n"),
    )
    .unwrap();

    let verify = ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        fixture.to_str().unwrap(),
        "--checksums",
        checksums_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        verify.status.success(),
        "shared release-support fixture should verify\nstdout:\n{}\nstderr:\n{}",
        stdout(&verify),
        stderr(&verify)
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "passed");
    assert_eq!(json["surface_count"], 9);
    assert_eq!(json["checksum_verified"], true);
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    let family_ids: Vec<&str> = bundle_json["ci_evidence_index"]["evidence_families"]
        .as_array()
        .expect("fixture has CI evidence families")
        .iter()
        .map(|family| {
            family["id"]
                .as_str()
                .expect("CI evidence family id is a string")
        })
        .collect();
    assert!(
        family_ids.contains(&"stable-promotion-evidence-readback"),
        "shared release-support fixture must include stable-promotion evidence readback"
    );
}

fn passed_report_contract_verification() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.report-contract-verification.v1",
        "contract_schema_version": "ao2.report-contract.v1",
        "status": "passed",
        "complete": true,
        "missing_sections": [],
        "failures": [],
    })
}

fn passed_install_verification() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.install-verification-evidence.v1",
        "status": "verified",
        "offline_verification": {
            "status": "verified"
        },
        "provider_api_keys_required": false,
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false
    })
}

fn passed_hosted_release_smoke() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.release-archive-hosted-smoke.v1",
        "status": "passed",
        "target": "test-fixture",
        "install_verification_schema": "ao2.install-verification-evidence.v1",
        "install_verification_evidence": "install-verification.json",
        "provider_api_keys_required": false,
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "release_acceptance_owner": "factory-v3 evaluator-closer"
    })
}

fn candidate_correlation() -> serde_json::Value {
    serde_json::json!({
        "status": "matched",
        "blockers": [],
        "release_version": "0.4.80",
        "three_os_version": "0.4.80",
        "evaluator_decision": "accepted",
        "codex_acceptance": "accepted",
        "claude_acceptance": "accepted",
    })
}

fn merge_json(base: &mut serde_json::Value, overlay: &mut serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base), serde_json::Value::Object(overlay)) => {
            for (key, value) in overlay {
                merge_json(base.entry(key).or_insert(serde_json::Value::Null), value);
            }
        }
        (base, overlay) => *base = overlay.take(),
    }
}

fn canonical_sha256(value: &serde_json::Value) -> String {
    let mut canonical = String::new();
    write_canonical_value(&mut canonical, value);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn write_canonical_value(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        serde_json::Value::Number(number) => out.push_str(&number.to_string()),
        serde_json::Value::String(text) => write_canonical_string(out, text),
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_value(out, item);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_string(out, key);
                out.push(':');
                write_canonical_value(out, &map[*key]);
            }
            out.push('}');
        }
    }
}

fn write_canonical_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 => {
                use std::fmt::Write as _;
                write!(out, "\\u{:04x}", ch as u32).unwrap();
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn add_portable_manifest(bundle: &mut serde_json::Value) {
    let surfaces = [
        (
            "ci_evidence_index",
            "ci_evidence_index",
            "$.ci_evidence_index",
        ),
        (
            "install_verification",
            "install_verification",
            "$.install_verification",
        ),
        (
            "hosted_release_smoke",
            "hosted_release_smoke",
            "$.hosted_release_smoke",
        ),
        ("release_assembly", "release_assembly", "$.release_assembly"),
        ("release_readiness", "readiness", "$.readiness"),
        ("release_candidate_handoff", "handoff", "$.handoff"),
        ("release_cockpit", "cockpit", "$.cockpit"),
        (
            "release_evaluator_decision",
            "evaluator_decision",
            "$.evaluator_decision",
        ),
        (
            "storage_support_bundle",
            "storage_support",
            "$.storage_support",
        ),
    ];
    let mut included = Vec::new();
    let mut sha_map = serde_json::Map::new();
    for (id, key, path) in surfaces {
        let surface = &bundle[key];
        let sha = canonical_sha256(surface);
        sha_map.insert(id.to_string(), serde_json::Value::String(sha.clone()));
        included.push(serde_json::json!({
            "id": id,
            "schema_version": surface["schema_version"].as_str().unwrap_or(""),
            "path": path,
            "sha256": sha,
        }));
    }
    bundle["portable_bundle_manifest"] = serde_json::json!({
        "schema_version": "ao2.cp-release-support-bundle-manifest.v1",
        "included_surfaces": included,
        "integrity": {
            "algorithm": "sha256-ao2-cp-canonical-json-v1",
            "scope": "embedded_support_bundle_surfaces",
            "surface_sha256": sha_map,
            "verification_plan": {
                "surface_count": surfaces.len(),
                "expected_fail_closed": true
            }
        }
    });
}

fn test_ci_evidence_index() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.cp-ci-evidence-index.v1",
        "status": "indexed",
        "control_plane_role": "read-only-observer",
        "mutates_ao_artifacts": false,
        "control_plane_approves_release": false,
        "auth": {
            "required": true,
            "scheme": "bearer",
            "credential_material_included": false,
            "credential_material_in_urls": false
        },
        "evidence_families": [
            {"id": "risky-pr-golden-bridge-smoke", "artifact_name_pattern": "ao2-control-plane-risky-pr-golden-bridge-<target>", "schema_versions": ["ao2.cp-risky-pr-golden-bridge-smoke.v1"], "operator_action": "download-ci-artifact"},
            {"id": "release-train-bridge-smoke", "artifact_name_pattern": "ao2-control-plane-release-train-bridge-<target>", "schema_versions": ["ao2.cp-release-train-bridge-smoke.v1", "ao2.cp-release-train-readback.v1", "ao2.public-release-train-drill.v1"], "operator_action": "download-ci-artifact"},
            {"id": "stable-promotion-evidence-readback", "artifact_name_pattern": "ao2-control-plane-ao2-stable-promotion-evidence-index-readback", "schema_versions": ["ao2.cp-ao2-stable-promotion-evidence-index-readback.v1", "ao2.cp-stable-promotion-evidence-readback.v1", "ao2.stable-promotion-evidence-index.v1"], "operator_action": "download-ci-artifact"},
            {"id": "ingest-smoke", "artifact_name_pattern": "ao2-control-plane-ingest-smoke-<target>", "schema_versions": ["ao2.cp-ingest-smoke.v1"], "operator_action": "download-ci-artifact"},
            {"id": "release-archive-smoke", "artifact_name_pattern": "ao2-control-plane-smoke-<target>", "schema_versions": ["ao2.cp-release-archive-smoke.v1"], "operator_action": "download-ci-artifact"},
            {"id": "backup-restore-drill", "artifact_name_pattern": "ao2-control-plane-dr-restore", "schema_versions": ["ao2.cp-dr-restore-drill.v1"], "operator_action": "download-ci-artifact"}
        ]
    })
}

fn verify_bundle(bundle: &Path) -> std::process::Output {
    ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        bundle.to_str().unwrap(),
        "--json",
    ])
}

fn write_json(path: &Path, value: serde_json::Value) {
    fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

struct SupportBundleEvidencePaths {
    release_assembly: PathBuf,
    readiness: PathBuf,
    handoff: PathBuf,
    cockpit: PathBuf,
    evaluator_decision: PathBuf,
    storage_support: PathBuf,
    replay: PathBuf,
    report_contract_verification: PathBuf,
    install_verification: PathBuf,
    hosted_release_smoke: PathBuf,
    operator_evidence: PathBuf,
}

fn write_support_bundle_evidence(evidence_dir: &Path) -> SupportBundleEvidencePaths {
    fs::create_dir_all(evidence_dir).unwrap();
    let paths = SupportBundleEvidencePaths {
        release_assembly: evidence_dir.join("release-assembly.json"),
        readiness: evidence_dir.join("readiness.json"),
        handoff: evidence_dir.join("handoff.json"),
        cockpit: evidence_dir.join("cockpit.json"),
        evaluator_decision: evidence_dir.join("evaluator-decision.json"),
        storage_support: evidence_dir.join("storage-support.json"),
        replay: evidence_dir.join("replay.json"),
        report_contract_verification: evidence_dir.join("report-contract-verification.json"),
        install_verification: evidence_dir.join("install-verification.json"),
        hosted_release_smoke: evidence_dir.join("hosted-release-smoke.json"),
        operator_evidence: evidence_dir.join("operator-evidence.json"),
    };

    write_json(
        &paths.release_assembly,
        serde_json::json!({
            "schema_version": "ao2.cp-release-assembly.v1",
            "status": "assembled",
            "candidate_correlation": "matched",
            "candidate_correlation_detail": candidate_correlation(),
            "control_plane_approves_release": false
        }),
    );
    write_json(
        &paths.readiness,
        serde_json::json!({
            "schema_version": "ao2.cp-release-readiness.v1",
            "status": "ready",
            "candidate_correlation": candidate_correlation(),
            "operator_decision": {
                "control_plane_approves_release": false,
                "factory_v3_evaluator_closer_required": true
            }
        }),
    );
    write_json(
        &paths.handoff,
        serde_json::json!({
            "schema_version": "factory-v3/ao2-release-handoff-checklist/v1",
            "status": "ready_for_evaluator_closer",
            "candidate_correlation": candidate_correlation(),
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        }),
    );
    write_json(
        &paths.cockpit,
        serde_json::json!({
            "schema_version": "ao2.cp-release-cockpit.v1",
            "status": "ready",
            "candidate_correlation": candidate_correlation()
        }),
    );
    write_json(
        &paths.evaluator_decision,
        serde_json::json!({
            "schema_version": "factory-v3/ao2-release-evaluator-decision/v1",
            "status": "accepted",
            "decision": "accept_phase1_release_candidate",
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        }),
    );
    write_json(
        &paths.storage_support,
        serde_json::json!({"schema_version": "ao2.cp-storage-support.v1", "status": "ready"}),
    );
    write_json(
        &paths.replay,
        serde_json::json!({"status": "accepted", "digest_failures": []}),
    );
    write_json(
        &paths.report_contract_verification,
        passed_report_contract_verification(),
    );
    write_json(&paths.install_verification, passed_install_verification());
    write_json(&paths.hosted_release_smoke, passed_hosted_release_smoke());
    write_json(
        &paths.operator_evidence,
        serde_json::json!({
            "factory_v3_evaluator_closer_required": true,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer",
            "control_plane_approves_release": false
        }),
    );

    paths
}

fn write_minimal_static_report(report_path: &Path, index_path: &Path, run_id: &str) {
    fs::write(
        report_path,
        r#"<!doctype html>
<html>
<body>
<h2>Objective</h2>
<h2>Run Health</h2>
<h2>Policy Decisions</h2>
<h2>Approvals</h2>
<h2>Artifacts</h2>
<h2>Evaluator Closure Evidence</h2>
<h2>Replay Evidence</h2>
<h2>Static Export Evidence</h2>
<h2>Local Run Record</h2>
</body>
</html>
"#,
    )
    .unwrap();
    write_json(
        index_path,
        serde_json::json!({
            "schema_version": "ao2.risky-pr-static-report-index.v1",
            "run_id": run_id
        }),
    );
}

#[test]
fn release_support_bundle_verify_rejects_missing_or_failed_report_contract_verification() {
    let temp = tempfile::tempdir().unwrap();
    let missing_bundle_path = temp.path().join("missing-report-verification.json");
    write_bundle(
        &missing_bundle_path,
        serde_json::json!({
            "report_contract_verification": null
        }),
    );

    let missing = verify_bundle(&missing_bundle_path);
    assert!(
        !missing.status.success(),
        "support verifier should fail when report contract verification evidence is missing"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&missing)).unwrap();
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|failure| { failure["code"] == "missing_report_contract_verification" }),
        "expected missing_report_contract_verification, got {failures:?}"
    );

    let failed_bundle_path = temp.path().join("failed-report-verification.json");
    write_bundle(
        &failed_bundle_path,
        serde_json::json!({
            "report_contract_verification": {
                "schema_version": "ao2.report-contract-verification.v1",
                "contract_schema_version": "ao2.report-contract.v1",
                "status": "failed",
                "complete": false,
                "missing_sections": ["Replay Evidence"],
                "failures": ["missing required report section: Replay Evidence"]
            }
        }),
    );

    let failed = verify_bundle(&failed_bundle_path);
    assert!(
        !failed.status.success(),
        "support verifier should fail when report contract verification failed"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&failed)).unwrap();
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "report_contract_verification_failed"),
        "expected report_contract_verification_failed, got {failures:?}"
    );
}

#[test]
fn release_support_bundle_verify_rejects_missing_required_evidence_surface() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(
        &bundle_path,
        serde_json::json!({
            "evaluator_decision": null
        }),
    );

    let verify = verify_bundle(&bundle_path);
    assert!(
        !verify.status.success(),
        "support verifier should fail when required evidence is missing"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures.iter().any(|failure| {
            failure["code"] == "missing_surface" && failure["surface"] == "evaluator_decision"
        }),
        "expected missing evaluator_decision surface, got {failures:?}"
    );
}

#[test]
fn release_support_bundle_verify_rejects_missing_or_unsafe_install_verification() {
    let temp = tempfile::tempdir().unwrap();
    let missing_bundle_path = temp.path().join("missing-install-verification.json");
    write_bundle(
        &missing_bundle_path,
        serde_json::json!({
            "install_verification": null
        }),
    );

    let missing = verify_bundle(&missing_bundle_path);
    assert!(
        !missing.status.success(),
        "support verifier should fail when install verification is missing"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&missing)).unwrap();
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures.iter().any(|failure| {
            failure["code"] == "missing_surface" && failure["surface"] == "install_verification"
        }),
        "expected missing install_verification surface, got {failures:?}"
    );

    let unsafe_bundle_path = temp.path().join("unsafe-install-verification.json");
    write_bundle(
        &unsafe_bundle_path,
        serde_json::json!({
            "install_verification": {
                "schema_version": "ao2.install-verification-evidence.v1",
                "status": "verified",
                "offline_verification": {
                    "status": "verified"
                },
                "provider_api_keys_required": true,
                "control_plane_approves_release": true,
                "mutates_ao_artifacts": true
            }
        }),
    );

    let unsafe_verify = verify_bundle(&unsafe_bundle_path);
    assert!(
        !unsafe_verify.status.success(),
        "support verifier should fail on trust-unsafe install verification"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&unsafe_verify)).unwrap();
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|failure| { failure["code"] == "install_verification_invalid" }),
        "expected install_verification_invalid, got {failures:?}"
    );
}

#[test]
fn release_support_bundle_verify_rejects_missing_candidate_correlation() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(
        &bundle_path,
        serde_json::json!({
            "cockpit": {
                "candidate_correlation": null
            }
        }),
    );

    let verify = verify_bundle(&bundle_path);
    assert!(
        !verify.status.success(),
        "support verifier should fail missing candidate correlation"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures.iter().any(|failure| {
            failure["code"] == "candidate_correlation_invalid"
                && failure["surface"] == "release_cockpit"
        }),
        "expected candidate_correlation_invalid, got {failures:?}"
    );
    assert!(stderr(&verify).contains("release support bundle verification failed"));
}

#[test]
fn release_support_bundle_verify_rejects_checksum_digest_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(&bundle_path, serde_json::Value::Null);
    let checksums_path = temp.path().join("SHA256SUMS");
    fs::write(
        &checksums_path,
        "0000000000000000000000000000000000000000000000000000000000000000  release-support-bundle.json\n",
    )
    .unwrap();

    let verify = ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        bundle_path.to_str().unwrap(),
        "--checksums",
        checksums_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !verify.status.success(),
        "support verifier should fail on checksum digest mismatch"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "checksum_mismatch"),
        "expected checksum_mismatch, got {failures:?}"
    );
}

#[test]
fn release_support_bundle_verify_rejects_operator_evidence_gaps_and_control_plane_approval() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(
        &bundle_path,
        serde_json::json!({
            "release_assembly": {"control_plane_approves_release": true},
            "readiness": {
                "operator_decision": {
                    "control_plane_approves_release": true,
                    "factory_v3_evaluator_closer_required": false
                }
            },
            "handoff": {
                "trust_boundary": {
                    "control_plane_role": "release_approver",
                    "release_acceptance_owner": "control-plane"
                }
            },
            "operator_evidence": null
        }),
    );

    let verify = verify_bundle(&bundle_path);
    assert!(
        !verify.status.success(),
        "support verifier should fail on operator/control-plane evidence gaps"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "failed");
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "control_plane_approved_release"),
        "expected control_plane_approved_release, got {failures:?}"
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "missing_operator_evidence"),
        "expected missing_operator_evidence, got {failures:?}"
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "operator_evaluator_closer_not_required"),
        "expected operator_evaluator_closer_not_required, got {failures:?}"
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "release_acceptance_owner_mismatch"),
        "expected release_acceptance_owner_mismatch, got {failures:?}"
    );
}

#[test]
fn release_support_bundle_verify_rejects_replay_not_accepted_or_digest_failures() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(
        &bundle_path,
        serde_json::json!({
            "replay": {
                "status": "rejected",
                "digest_failures": [{"path": "evidence-pack.json", "expected": "old", "actual": "new"}]
            }
        }),
    );

    let verify = verify_bundle(&bundle_path);
    assert!(
        !verify.status.success(),
        "support verifier should fail on replay corruption"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "failed");
    let failures = json["failures"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "replay_not_accepted"),
        "expected replay_not_accepted, got {failures:?}"
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure["code"] == "replay_digest_failures"),
        "expected replay_digest_failures, got {failures:?}"
    );
    assert!(stderr(&verify).contains("release support bundle verification failed"));
}

#[test]
fn release_support_bundle_verify_accepts_complete_evidence_bundle_with_checksum() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(&bundle_path, serde_json::Value::Null);
    let bundle_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&bundle_path).unwrap()).unwrap();
    let bundle_sha256 = canonical_sha256(&bundle_json);
    let checksums_path = temp.path().join("SHA256SUMS");
    fs::write(
        &checksums_path,
        format!("{bundle_sha256}  release-support-bundle.json\n"),
    )
    .unwrap();

    let verify = ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        bundle_path.to_str().unwrap(),
        "--checksums",
        checksums_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "passed");
    assert_eq!(json["checksum_verified"], true);
    assert_eq!(json["failure_count"], 0);
}

#[test]
fn release_support_bundle_build_writes_verifiable_bundle_and_checksums() {
    let temp = tempfile::tempdir().unwrap();
    let evidence_dir = temp.path().join("evidence");
    fs::create_dir_all(&evidence_dir).unwrap();
    let release_assembly = evidence_dir.join("release-assembly.json");
    let readiness = evidence_dir.join("readiness.json");
    let handoff = evidence_dir.join("handoff.json");
    let cockpit = evidence_dir.join("cockpit.json");
    let evaluator_decision = evidence_dir.join("evaluator-decision.json");
    let storage_support = evidence_dir.join("storage-support.json");
    let replay = evidence_dir.join("replay.json");
    let report_contract_verification = evidence_dir.join("report-contract-verification.json");
    let install_verification = evidence_dir.join("install-verification.json");
    let hosted_release_smoke = evidence_dir.join("hosted-release-smoke.json");
    let operator_evidence = evidence_dir.join("operator-evidence.json");

    write_json(
        &release_assembly,
        serde_json::json!({
            "schema_version": "ao2.cp-release-assembly.v1",
            "status": "assembled",
            "candidate_correlation": "matched",
            "candidate_correlation_detail": candidate_correlation(),
            "control_plane_approves_release": false
        }),
    );
    write_json(
        &readiness,
        serde_json::json!({
            "schema_version": "ao2.cp-release-readiness.v1",
            "status": "ready",
            "candidate_correlation": candidate_correlation(),
            "operator_decision": {
                "control_plane_approves_release": false,
                "factory_v3_evaluator_closer_required": true
            }
        }),
    );
    write_json(
        &handoff,
        serde_json::json!({
            "schema_version": "factory-v3/ao2-release-handoff-checklist/v1",
            "status": "ready_for_evaluator_closer",
            "candidate_correlation": candidate_correlation(),
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        }),
    );
    write_json(
        &cockpit,
        serde_json::json!({
            "schema_version": "ao2.cp-release-cockpit.v1",
            "status": "ready",
            "candidate_correlation": candidate_correlation()
        }),
    );
    write_json(
        &evaluator_decision,
        serde_json::json!({
            "schema_version": "factory-v3/ao2-release-evaluator-decision/v1",
            "status": "accepted",
            "decision": "accept_phase1_release_candidate",
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        }),
    );
    write_json(
        &storage_support,
        serde_json::json!({"schema_version": "ao2.cp-storage-support.v1", "status": "ready"}),
    );
    write_json(
        &replay,
        serde_json::json!({"status": "accepted", "digest_failures": []}),
    );
    write_json(
        &report_contract_verification,
        passed_report_contract_verification(),
    );
    write_json(&install_verification, passed_install_verification());
    write_json(&hosted_release_smoke, passed_hosted_release_smoke());
    write_json(
        &operator_evidence,
        serde_json::json!({
            "factory_v3_evaluator_closer_required": true,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer",
            "control_plane_approves_release": false
        }),
    );

    let out_dir = temp.path().join("support-bundle");
    let build = ao2([
        "release",
        "support-bundle-build",
        "--release-assembly",
        release_assembly.to_str().unwrap(),
        "--readiness",
        readiness.to_str().unwrap(),
        "--handoff",
        handoff.to_str().unwrap(),
        "--cockpit",
        cockpit.to_str().unwrap(),
        "--evaluator-decision",
        evaluator_decision.to_str().unwrap(),
        "--storage-support",
        storage_support.to_str().unwrap(),
        "--replay",
        replay.to_str().unwrap(),
        "--report-contract-verification",
        report_contract_verification.to_str().unwrap(),
        "--install-verification",
        install_verification.to_str().unwrap(),
        "--hosted-release-smoke",
        hosted_release_smoke.to_str().unwrap(),
        "--operator-evidence",
        operator_evidence.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));

    let build_json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    let bundle_path = out_dir.join("release-support-bundle.json");
    let checksums_path = out_dir.join("SHA256SUMS");
    assert_eq!(
        build_json["schema_version"],
        "ao2.release-support-bundle-build.v1"
    );
    assert_eq!(build_json["bundle"], bundle_path.display().to_string());
    assert_eq!(
        build_json["checksums"],
        checksums_path.display().to_string()
    );
    assert!(bundle_path.is_file());
    assert!(checksums_path.is_file());
    let bundle_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&bundle_path).unwrap()).unwrap();
    assert_eq!(
        bundle_json["report_contract_verification"]["schema_version"],
        "ao2.report-contract-verification.v1"
    );
    assert_eq!(
        bundle_json["install_verification"]["schema_version"],
        "ao2.install-verification-evidence.v1"
    );
    assert_eq!(bundle_json["install_verification"]["status"], "verified");
    assert_eq!(
        bundle_json["ci_evidence_index"]["schema_version"],
        "ao2.cp-ci-evidence-index.v1"
    );
    let ci_family_ids = bundle_json["ci_evidence_index"]["evidence_families"]
        .as_array()
        .unwrap()
        .iter()
        .map(|family| family["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let fixture_ci_family_ids = shared_release_support_fixture()["ci_evidence_index"]
        ["evidence_families"]
        .as_array()
        .unwrap()
        .iter()
        .map(|family| family["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(
        ci_family_ids.contains(&"release-train-bridge-smoke"),
        "release support bundles must include the release train bridge smoke family; got {ci_family_ids:?}"
    );
    assert!(
        ci_family_ids.contains(&"stable-promotion-evidence-readback"),
        "release support bundles must include stable-promotion evidence readback for control-plane verifier parity; got {ci_family_ids:?}"
    );
    assert_eq!(
        ci_family_ids, fixture_ci_family_ids,
        "generated release support bundles must keep CI evidence families in shared fixture order"
    );
    assert_eq!(
        bundle_json["ci_evidence_index"],
        shared_release_support_fixture()["ci_evidence_index"],
        "generated release support bundles must embed the shared CI evidence index contract"
    );
    let release_train_family = bundle_json["ci_evidence_index"]["evidence_families"]
        .as_array()
        .unwrap()
        .iter()
        .find(|family| family["id"] == "release-train-bridge-smoke")
        .expect("release-train-bridge-smoke family");
    let release_train_schemas = release_train_family["schema_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|schema| schema.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(release_train_schemas.contains(&"ao2.cp-release-train-bridge-smoke.v1"));
    assert!(release_train_schemas.contains(&"ao2.cp-release-train-readback.v1"));
    assert!(release_train_schemas.contains(&"ao2.public-release-train-drill.v1"));
    assert_eq!(
        bundle_json["portable_bundle_manifest"]["schema_version"],
        "ao2.cp-release-support-bundle-manifest.v1"
    );
    let surface_ids = bundle_json["portable_bundle_manifest"]["included_surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|surface| surface["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        surface_ids,
        vec![
            "ci_evidence_index",
            "install_verification",
            "hosted_release_smoke",
            "release_assembly",
            "release_readiness",
            "release_candidate_handoff",
            "release_cockpit",
            "release_evaluator_decision",
            "storage_support_bundle",
        ]
    );
    let checksums = fs::read_to_string(&checksums_path).unwrap();
    assert!(checksums.contains(build_json["bundle_sha256"].as_str().unwrap()));
    assert_eq!(build_json["bundle_sha256"], canonical_sha256(&bundle_json));

    let verify = ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        bundle_path.to_str().unwrap(),
        "--checksums",
        checksums_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["checksum_verified"], true);
    assert_eq!(verify_json["failure_count"], 0);
}

#[test]
fn release_support_bundle_build_generates_report_contract_verification_from_report_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let paths = write_support_bundle_evidence(&temp.path().join("evidence"));
    let report_path = temp.path().join("report.html");
    let report_index_path = temp.path().join("report.index.json");
    let run_id = "release-auto-report";
    write_minimal_static_report(&report_path, &report_index_path, run_id);

    let out_dir = temp.path().join("support-bundle");
    let build = ao2([
        "release",
        "support-bundle-build",
        "--release-assembly",
        paths.release_assembly.to_str().unwrap(),
        "--readiness",
        paths.readiness.to_str().unwrap(),
        "--handoff",
        paths.handoff.to_str().unwrap(),
        "--cockpit",
        paths.cockpit.to_str().unwrap(),
        "--evaluator-decision",
        paths.evaluator_decision.to_str().unwrap(),
        "--storage-support",
        paths.storage_support.to_str().unwrap(),
        "--replay",
        paths.replay.to_str().unwrap(),
        "--report-target",
        temp.path().to_str().unwrap(),
        "--report-run-id",
        run_id,
        "--report",
        report_path.to_str().unwrap(),
        "--report-index",
        report_index_path.to_str().unwrap(),
        "--install-verification",
        paths.install_verification.to_str().unwrap(),
        "--hosted-release-smoke",
        paths.hosted_release_smoke.to_str().unwrap(),
        "--operator-evidence",
        paths.operator_evidence.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));

    let build_json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        build_json["report_contract_verification_source"],
        "generated_report_verify"
    );

    let bundle_path = out_dir.join("release-support-bundle.json");
    let bundle_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&bundle_path).unwrap()).unwrap();
    assert_eq!(
        bundle_json["report_contract_verification"]["schema_version"],
        "ao2.report-contract-verification.v1"
    );
    assert_eq!(
        bundle_json["report_contract_verification"]["status"],
        "passed"
    );
    assert_eq!(
        bundle_json["report_contract_verification"]["run_id"],
        run_id
    );

    let verify = ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        bundle_path.to_str().unwrap(),
        "--checksums",
        out_dir.join("SHA256SUMS").to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
}

#[test]
fn cli_release_support_bundle_verify_accepts_minimal_cp_bundle_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(&bundle_path, serde_json::Value::Null);
    let bundle_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&bundle_path).unwrap()).unwrap();
    let bundle_sha256 = canonical_sha256(&bundle_json);
    let checksums_path = temp.path().join("SHA256SUMS");
    fs::write(
        &checksums_path,
        format!("{bundle_sha256}  release-support-bundle.json\n"),
    )
    .unwrap();

    let verify = ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        bundle_path.to_str().unwrap(),
        "--checksums",
        checksums_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.release-support-bundle-verification.v1"
    );
    assert_eq!(json["status"], "passed");
    assert_eq!(json["surface_count"], 9);
    assert_eq!(json["checksum_verified"], true);
    assert_eq!(json["failure_count"], 0);
    assert_eq!(
        json["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_approves_release"],
        false
    );
}

#[test]
fn cli_release_support_bundle_verify_rejects_secret_markers() {
    let temp = tempfile::tempdir().unwrap();
    let bundle_path = temp.path().join("release-support-bundle.json");
    write_bundle(
        &bundle_path,
        serde_json::json!({
            "operator_log": "Authorization: Bearer should-not-ship",
            "nested": {"access_token": "should-not-ship"}
        }),
    );

    let verify = ao2([
        "release",
        "support-bundle-verify",
        "--bundle",
        bundle_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !verify.status.success(),
        "support verifier should fail on secret markers"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "failed");
    let failures = json["failures"].as_array().unwrap();
    assert!(failures
        .iter()
        .any(|failure| failure["code"] == "forbidden_secret_marker"));
    assert!(failures
        .iter()
        .any(|failure| failure["code"] == "forbidden_secret_field"));
    assert!(stderr(&verify).contains("release support bundle verification failed"));
}
