use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
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
            "control_plane_approves_release": false
        },
        "readiness": {
            "schema_version": "ao2.cp-release-readiness.v1",
            "status": "ready",
            "operator_decision": {
                "control_plane_approves_release": false,
                "factory_v3_evaluator_closer_required": true
            }
        },
        "handoff": {
            "schema_version": "factory-v3/ao2-release-handoff-checklist/v1",
            "status": "ready_for_evaluator_closer",
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "control_plane_approves_release": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer"
            }
        },
        "cockpit": {"schema_version": "ao2.cp-release-cockpit.v1", "status": "ready"},
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
        "operator_evidence": {
            "factory_v3_evaluator_closer_required": true,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_role": "read_only_observer",
            "control_plane_approves_release": false
        }
    });
    if !overlay.is_null() {
        merge_json(&mut bundle, &mut overlay);
    }
    fs::write(path, serde_json::to_string_pretty(&bundle).unwrap()).unwrap();
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

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
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
    let bundle_sha256 = sha256_path(&bundle_path);
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
