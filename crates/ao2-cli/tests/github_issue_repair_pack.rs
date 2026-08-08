use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const SOURCE_BYTES: &[u8] = b"sanitized source archive";
const SNAPSHOT_BYTES: &[u8] = b"sanitized issue snapshot";
const DEPENDENCY_CACHE_BYTES: &[u8] = b"sanitized dependency cache manifest";
const SOURCE_SHA256: &str =
    "sha256:0a7e768fe4cbff8db5cb4b847d3b240aea390866f090a4e9bb8a5619cff24709";
const SNAPSHOT_SHA256: &str =
    "sha256:efcd0e1fcc4d1a063ed40f63f0457aac63a57aa7e4b7e14d78b71e09b50b0e4b";
const DEPENDENCY_CACHE_SHA256: &str =
    "sha256:832a373b36cffe46c94a00b2f5c31b1cd7ad76422b97cb431045274373c1a116";
const TREE_SHA256: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const REPRODUCTION_PATH: &str = "reproduction-evidence.json";
const REPRODUCTION_FIXTURE_PATH: &str = "reproduction-fixture.rs";
const REPRODUCTION_OUTPUT_PATH: &str = "reproduction-output.txt";
const REPRODUCTION_FIXTURE_BYTES: &[u8] = b"issue-derived regression fixture";
const FAILURE_SIGNATURE: &str = "issue-specific assertion failed";
const REPRODUCTION_OUTPUT_BYTES: &[u8] = b"test failed: issue-specific assertion failed\n";

fn valid_manifest() -> Value {
    let fetched_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    json!({
        "schema_version": "ao2.github-issue-repair-pack.v1",
        "request_id": "request-20260801-001",
        "corpus_id": "month-1-blind-corpus",
        "candidate_id": "candidate-001",
        "repository": "example/project",
        "issue_number": 17,
        "source_sha": "0123456789abcdef0123456789abcdef01234567",
        "license": "Apache-2.0",
        "language": "rust",
        "fetched_at": fetched_at,
        "source_archive": {
            "path": "source.tar.gz",
            "size_bytes": SOURCE_BYTES.len(),
            "sha256": SOURCE_SHA256,
        },
        "issue_snapshot": {
            "path": "issue.json",
            "size_bytes": SNAPSHOT_BYTES.len(),
            "sha256": SNAPSHOT_SHA256,
        },
        "dependency_cache_manifest": {
            "path": "dependency-cache.json",
            "size_bytes": DEPENDENCY_CACHE_BYTES.len(),
            "sha256": DEPENDENCY_CACHE_SHA256,
        },
        "toolchain": {
            "name": "rust",
            "version": "1.83.0",
        },
        "extracted_tree_sha256": TREE_SHA256,
        "known_fix_fetched": false,
        "safety": {
            "authority_level": "L1",
            "network": "none",
            "git_history_present": false,
            "oracle_present": false,
            "credentials_present": false,
            "campaign_root_mounted": false,
            "repair_pack_read_only": true,
            "scratch_read_write": true,
            "third_party_mutation_authorized": false,
        },
    })
}

fn valid_v2_pack() -> (Value, Value) {
    let mut manifest = valid_manifest();
    manifest["schema_version"] = json!("ao2.github-issue-repair-pack.v2");
    let reproduction = json!({
        "schema_version": "ao2.github-issue-reproduction-evidence.v1",
        "request_id": manifest["request_id"],
        "candidate_id": manifest["candidate_id"],
        "source_sha": manifest["source_sha"],
        "command_argv": ["cargo", "test", "--test", "withdrawn-issue-regression"],
        "working_directory": ".",
        "fixture_install_path": "tests/withdrawn-issue-regression.rs",
        "test_identifier": "withdrawn-issue-regression",
        "toolchain": {
            "name": manifest["toolchain"]["name"],
            "version": manifest["toolchain"]["version"],
        },
        "fixture_sha256": format!("sha256:{:x}", Sha256::digest(REPRODUCTION_FIXTURE_BYTES)),
        "output_sha256": format!("sha256:{:x}", Sha256::digest(REPRODUCTION_OUTPUT_BYTES)),
        "failure_signature": FAILURE_SIGNATURE,
        "failure_signature_sha256": format!("sha256:{:x}", Sha256::digest(FAILURE_SIGNATURE.as_bytes())),
        "result": "reproduced_failure",
        "expected_exit_code": 1,
        "observed_exit_code": 1,
        "network": "none",
        "git_history_present": false,
        "oracle_present": false,
        "credentials_present": false,
        "external_effects": 0,
        "completed_at": manifest["fetched_at"],
    });
    let bytes = serde_json::to_vec(&reproduction).unwrap();
    manifest["reproduction_evidence"] = json!({
        "path": REPRODUCTION_PATH,
        "size_bytes": bytes.len(),
        "sha256": format!("sha256:{:x}", Sha256::digest(&bytes)),
    });
    manifest["reproduction_fixture"] = json!({
        "path": REPRODUCTION_FIXTURE_PATH,
        "size_bytes": REPRODUCTION_FIXTURE_BYTES.len(),
        "sha256": format!("sha256:{:x}", Sha256::digest(REPRODUCTION_FIXTURE_BYTES)),
    });
    manifest["reproduction_output"] = json!({
        "path": REPRODUCTION_OUTPUT_PATH,
        "size_bytes": REPRODUCTION_OUTPUT_BYTES.len(),
        "sha256": format!("sha256:{:x}", Sha256::digest(REPRODUCTION_OUTPUT_BYTES)),
    });
    (manifest, reproduction)
}

fn valid_v3_python_pack() -> (Value, Value) {
    let (mut manifest, mut reproduction) = valid_v2_pack();
    manifest["schema_version"] = json!("ao2.github-issue-repair-pack.v3");
    manifest["language"] = json!("python");
    manifest["toolchain"] = json!({
        "name": "python",
        "version": "3.13.12",
    });
    reproduction["command_argv"] = json!([
        "python",
        "-m",
        "pytest",
        "tests/test_withdrawn_issue.py::test_withdrawn_issue"
    ]);
    reproduction["fixture_install_path"] = json!("tests/test_withdrawn_issue.py");
    reproduction["test_identifier"] = json!("test_withdrawn_issue");
    reproduction["toolchain"] = manifest["toolchain"].clone();
    (manifest, reproduction)
}

fn write_pack_at(root: &Path, manifest: &Value) -> std::path::PathBuf {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("source.tar.gz"), SOURCE_BYTES).unwrap();
    fs::write(root.join("issue.json"), SNAPSHOT_BYTES).unwrap();
    fs::write(root.join("dependency-cache.json"), DEPENDENCY_CACHE_BYTES).unwrap();
    let manifest_path = root.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec(manifest).unwrap()).unwrap();
    manifest_path
}

fn write_pack(temp: &tempfile::TempDir, manifest: &Value) -> std::path::PathBuf {
    write_pack_at(temp.path(), manifest)
}

fn write_v2_pack_at(root: &Path, manifest: &mut Value, reproduction: &Value) -> std::path::PathBuf {
    let bytes = serde_json::to_vec(reproduction).unwrap();
    write_v2_pack_bytes_at(root, manifest, &bytes)
}

fn write_v2_pack_bytes_at(root: &Path, manifest: &mut Value, bytes: &[u8]) -> std::path::PathBuf {
    manifest["reproduction_evidence"]["size_bytes"] = json!(bytes.len());
    manifest["reproduction_evidence"]["sha256"] =
        json!(format!("sha256:{:x}", Sha256::digest(bytes)));
    let manifest_path = write_pack_at(root, manifest);
    fs::write(root.join(REPRODUCTION_PATH), bytes).unwrap();
    fs::write(
        root.join(REPRODUCTION_FIXTURE_PATH),
        REPRODUCTION_FIXTURE_BYTES,
    )
    .unwrap();
    fs::write(
        root.join(REPRODUCTION_OUTPUT_PATH),
        REPRODUCTION_OUTPUT_BYTES,
    )
    .unwrap();
    manifest_path
}

fn write_v2_pack(
    temp: &tempfile::TempDir,
    manifest: &mut Value,
    reproduction: &Value,
) -> std::path::PathBuf {
    write_v2_pack_at(temp.path(), manifest, reproduction)
}

fn validate(manifest: &Path, root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "issue",
            "repair-pack",
            "validate",
            "--manifest",
            manifest.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap()
}

fn assert_rejected(output: Output) {
    assert!(
        !output.status.success(),
        "unsafe pack passed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stdout.is_empty(),
        "failure emitted a passing readback"
    );
}

fn manifest_digest(path: &Path) -> String {
    format!("sha256:{:x}", Sha256::digest(fs::read(path).unwrap()))
}

#[test]
fn validates_a_strict_read_only_repair_pack() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = valid_manifest();
    let fetched_at = manifest["fetched_at"].clone();
    let manifest_path = write_pack(&temp, &manifest);

    let output = validate(&manifest_path, temp.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        readback,
        json!({
            "schema_version": "ao2.github-issue-repair-pack-validation.v1",
            "status": "passed",
            "request_id": "request-20260801-001",
            "corpus_id": "month-1-blind-corpus",
            "candidate_id": "candidate-001",
            "repository": "example/project",
            "issue_number": 17,
            "source_sha": "0123456789abcdef0123456789abcdef01234567",
            "license": "Apache-2.0",
            "language": "rust",
            "fetched_at": fetched_at,
            "manifest_sha256": manifest_digest(&manifest_path),
            "source_archive_sha256": SOURCE_SHA256,
            "issue_snapshot_sha256": SNAPSHOT_SHA256,
            "dependency_cache_manifest_sha256": DEPENDENCY_CACHE_SHA256,
            "extracted_tree_sha256": TREE_SHA256,
            "failed_rows": 0,
            "authority_level": "L1",
            "network": "none",
            "git_history_present": false,
            "oracle_present": false,
            "credentials_present": false,
            "campaign_root_mounted": false,
            "repair_pack_read_only": true,
            "scratch_read_write": true,
            "third_party_mutation_authorized": false,
            "network_accessed": false,
            "git_invoked": false,
            "github_read_performed": false,
            "github_write_performed": false,
            "repair_executed": false,
            "mutation_performed": false,
            "executes_work": false,
            "approves_work": false,
        })
    );
}

#[test]
fn validates_v2_only_with_digest_bound_reproduced_failure_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, reproduction) = valid_v2_pack();
    let fetched_at = manifest["fetched_at"].clone();
    let manifest_path = write_v2_pack(&temp, &mut manifest, &reproduction);
    let reproduction_sha256 = manifest["reproduction_evidence"]["sha256"].clone();
    let fixture_sha256 = manifest["reproduction_fixture"]["sha256"].clone();
    let output_sha256 = manifest["reproduction_output"]["sha256"].clone();

    let output = validate(&manifest_path, temp.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        readback,
        json!({
            "schema_version": "ao2.github-issue-repair-pack-validation.v2",
            "status": "passed",
            "eligibility_status": "reproduced",
            "request_id": "request-20260801-001",
            "corpus_id": "month-1-blind-corpus",
            "candidate_id": "candidate-001",
            "repository": "example/project",
            "issue_number": 17,
            "source_sha": "0123456789abcdef0123456789abcdef01234567",
            "license": "Apache-2.0",
            "language": "rust",
            "fetched_at": fetched_at,
            "manifest_sha256": manifest_digest(&manifest_path),
            "source_archive_sha256": SOURCE_SHA256,
            "issue_snapshot_sha256": SNAPSHOT_SHA256,
            "dependency_cache_manifest_sha256": DEPENDENCY_CACHE_SHA256,
            "reproduction_evidence_sha256": reproduction_sha256,
            "reproduction_fixture_sha256": fixture_sha256,
            "reproduction_output_sha256": output_sha256,
            "extracted_tree_sha256": TREE_SHA256,
            "failed_rows": 0,
            "authority_level": "L1",
            "network": "none",
            "git_history_present": false,
            "oracle_present": false,
            "credentials_present": false,
            "campaign_root_mounted": false,
            "repair_pack_read_only": true,
            "scratch_read_write": true,
            "third_party_mutation_authorized": false,
            "network_accessed": false,
            "git_invoked": false,
            "github_read_performed": false,
            "github_write_performed": false,
            "repair_executed": false,
            "mutation_performed": false,
            "executes_work": false,
            "approves_work": false,
        })
    );
}

#[test]
fn validates_v3_python_only_with_a_bound_direct_pytest_target() {
    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, reproduction) = valid_v3_python_pack();
    let fetched_at = manifest["fetched_at"].clone();
    let manifest_path = write_v2_pack(&temp, &mut manifest, &reproduction);
    let reproduction_sha256 = manifest["reproduction_evidence"]["sha256"].clone();
    let fixture_sha256 = manifest["reproduction_fixture"]["sha256"].clone();
    let output_sha256 = manifest["reproduction_output"]["sha256"].clone();

    let output = validate(&manifest_path, temp.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        readback,
        json!({
            "schema_version": "ao2.github-issue-repair-pack-validation.v3",
            "status": "passed",
            "eligibility_status": "reproduced",
            "request_id": "request-20260801-001",
            "corpus_id": "month-1-blind-corpus",
            "candidate_id": "candidate-001",
            "repository": "example/project",
            "issue_number": 17,
            "source_sha": "0123456789abcdef0123456789abcdef01234567",
            "license": "Apache-2.0",
            "language": "python",
            "fetched_at": fetched_at,
            "manifest_sha256": manifest_digest(&manifest_path),
            "source_archive_sha256": SOURCE_SHA256,
            "issue_snapshot_sha256": SNAPSHOT_SHA256,
            "dependency_cache_manifest_sha256": DEPENDENCY_CACHE_SHA256,
            "reproduction_evidence_sha256": reproduction_sha256,
            "reproduction_fixture_sha256": fixture_sha256,
            "reproduction_output_sha256": output_sha256,
            "extracted_tree_sha256": TREE_SHA256,
            "failed_rows": 0,
            "authority_level": "L1",
            "network": "none",
            "git_history_present": false,
            "oracle_present": false,
            "credentials_present": false,
            "campaign_root_mounted": false,
            "repair_pack_read_only": true,
            "scratch_read_write": true,
            "third_party_mutation_authorized": false,
            "network_accessed": false,
            "git_invoked": false,
            "github_read_performed": false,
            "github_write_performed": false,
            "repair_executed": false,
            "mutation_performed": false,
            "executes_work": false,
            "approves_work": false,
        })
    );
}

#[test]
fn rejects_python_across_the_v2_v3_schema_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, reproduction) = valid_v3_python_pack();
    manifest["schema_version"] = json!("ao2.github-issue-repair-pack.v2");
    let path = write_v2_pack(&temp, &mut manifest, &reproduction);

    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn preserves_v2_go_identifier_compatibility_beyond_the_v3_python_bound() {
    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, mut reproduction) = valid_v2_pack();
    let identifier = format!("Test{}", "A".repeat(125));
    let fixture_path = "long_identifier_test.go";
    manifest["language"] = json!("go");
    manifest["toolchain"] = json!({"name": "go", "version": "1.26.4"});
    manifest["reproduction_fixture"]["path"] = json!(fixture_path);
    reproduction["command_argv"] = json!(["go", "test", ".", "-run", format!("^{identifier}$")]);
    reproduction["fixture_install_path"] = json!(fixture_path);
    reproduction["test_identifier"] = json!(identifier);
    reproduction["toolchain"] = manifest["toolchain"].clone();
    let path = write_v2_pack(&temp, &mut manifest, &reproduction);
    fs::rename(
        temp.path().join(REPRODUCTION_FIXTURE_PATH),
        temp.path().join(fixture_path),
    )
    .unwrap();

    let output = validate(&path, temp.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rejects_v3_python_wrappers_broad_targets_and_unbound_fixtures() {
    let mutations: &[fn(&mut Value)] = &[
        |value| {
            value["command_argv"] = json!([
                "python3",
                "-m",
                "pytest",
                "tests/test_withdrawn_issue.py::test_withdrawn_issue"
            ])
        },
        |value| value["command_argv"] = json!(["python", "-c", "import pytest"]),
        |value| value["command_argv"] = json!(["sh", "-c", "python -m pytest"]),
        |value| value["command_argv"] = json!(["python", "-m", "pytest"]),
        |value| {
            value["command_argv"] =
                json!(["python", "-m", "pytest", "tests/test_withdrawn_issue.py"])
        },
        |value| {
            value["command_argv"] = json!([
                "python",
                "-m",
                "pytest",
                "tests/test_withdrawn_issue.py::test_withdrawn_issue",
                "-q"
            ])
        },
        |value| {
            value["command_argv"] =
                json!(["python", "-m", "unittest", "tests.test_withdrawn_issue"])
        },
        |value| value["fixture_install_path"] = json!("../tests/test_withdrawn_issue.py"),
        |value| value["fixture_install_path"] = json!("/tmp/test_withdrawn_issue.py"),
        |value| value["fixture_install_path"] = json!(r"tests\test_withdrawn_issue.py"),
        |value| value["fixture_install_path"] = json!("tests//test_withdrawn_issue.py"),
        |value| value["fixture_install_path"] = json!("tests/test_withdrawn_issue.txt"),
        |value| value["fixture_install_path"] = json!("tests/withdrawn_issue.py"),
        |value| value["test_identifier"] = json!("withdrawn_issue"),
        |value| value["test_identifier"] = json!("test_withdrawn_issue[param]"),
        |value| value["test_identifier"] = json!("test_other_issue"),
    ];
    for mutation in mutations {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, mut reproduction) = valid_v3_python_pack();
        mutation(&mut reproduction);
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_v3_python_identity_digest_freshness_toolchain_and_safety_drift() {
    let mutations: &[fn(&mut Value, &mut Value)] = &[
        |_, evidence| evidence["source_sha"] = json!("1111111111111111111111111111111111111111"),
        |_, evidence| evidence["fixture_sha256"] = json!(TREE_SHA256),
        |_, evidence| evidence["output_sha256"] = json!(TREE_SHA256),
        |_, evidence| {
            evidence["completed_at"] =
                json!((Utc::now() - Duration::days(8)).to_rfc3339_opts(SecondsFormat::Secs, true))
        },
        |manifest, evidence| {
            manifest["toolchain"]["name"] = json!("python3");
            evidence["toolchain"]["name"] = json!("python3");
        },
        |manifest, _| manifest["safety"]["network"] = json!("host"),
        |manifest, _| manifest["safety"]["credentials_present"] = json!(true),
        |manifest, _| manifest["known_fix_fetched"] = json!(true),
    ];
    for mutation in mutations {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, mut reproduction) = valid_v3_python_pack();
        mutation(&mut manifest, &mut reproduction);
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_v3_python_missing_malformed_or_oversized_evidence() {
    for missing in [
        "reproduction_evidence",
        "reproduction_fixture",
        "reproduction_output",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, reproduction) = valid_v3_python_pack();
        manifest.as_object_mut().unwrap().remove(missing);
        let path = if missing == "reproduction_evidence" {
            write_pack(&temp, &manifest)
        } else {
            write_v2_pack(&temp, &mut manifest, &reproduction)
        };
        assert_rejected(validate(&path, temp.path()));
    }

    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, _) = valid_v3_python_pack();
    let path = write_v2_pack_bytes_at(temp.path(), &mut manifest, b"{");
    assert_rejected(validate(&path, temp.path()));

    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, _) = valid_v3_python_pack();
    manifest["reproduction_evidence"]["size_bytes"] = json!(65_537_u64);
    let path = write_pack(&temp, &manifest);
    fs::write(temp.path().join(REPRODUCTION_PATH), vec![b'x'; 65_537]).unwrap();
    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn rejects_v2_without_reproduction_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, _) = valid_v2_pack();
    manifest
        .as_object_mut()
        .unwrap()
        .remove("reproduction_evidence");
    let path = write_pack(&temp, &manifest);

    assert_rejected(validate(&path, temp.path()));

    for missing in ["reproduction_fixture", "reproduction_output"] {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, reproduction) = valid_v2_pack();
        manifest.as_object_mut().unwrap().remove(missing);
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_reproduction_evidence_across_the_v1_v2_schema_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let (mut v1_with_reproduction, reproduction) = valid_v2_pack();
    v1_with_reproduction["schema_version"] = json!("ao2.github-issue-repair-pack.v1");
    let path = write_v2_pack(&temp, &mut v1_with_reproduction, &reproduction);
    assert_rejected(validate(&path, temp.path()));

    let temp = tempfile::tempdir().unwrap();
    let (mut v2_with_null, _) = valid_v2_pack();
    v2_with_null["reproduction_evidence"] = Value::Null;
    let path = write_pack(&temp, &v2_with_null);
    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn rejects_v2_pass_no_failure_mismatch_unsafe_shell_and_side_effect_evidence() {
    let mutations: &[fn(&mut Value)] = &[
        |value| value["result"] = json!("passed"),
        |value| value["expected_exit_code"] = json!(0),
        |value| value["observed_exit_code"] = json!(0),
        |value| value["observed_exit_code"] = json!(2),
        |value| value["expected_exit_code"] = json!(256),
        |value| value["observed_exit_code"] = json!(256),
        |value| value["request_id"] = json!("request-mismatch"),
        |value| value["candidate_id"] = json!("candidate-mismatch"),
        |value| value["source_sha"] = json!("1111111111111111111111111111111111111111"),
        |value| value["network"] = json!("host"),
        |value| value["git_history_present"] = json!(true),
        |value| value["oracle_present"] = json!(true),
        |value| value["credentials_present"] = json!(true),
        |value| value["external_effects"] = json!(1),
        |value| value["command_argv"] = json!([]),
        |value| value["command_argv"] = json!(["false"]),
        |value| value["command_argv"] = json!(["cargo test"]),
        |value| value["command_argv"] = json!(["sh", "-c", "exit 1"]),
        |value| value["command_argv"] = json!([r"C:\Windows\System32\cmd.exe", "/c", "exit 1"]),
        |value| value["command_argv"] = json!(["/bin/ash", "-c", "exit 1"]),
        |value| value["command_argv"] = json!(["cargo", "build"]),
        |value| {
            value["command_argv"] =
                json!(["cargo", "test", "--manifest-path", "../outside/Cargo.toml"])
        },
        |value| {
            value["command_argv"] = json!(["cargo", "test", "--config", "build.rustc-wrapper=sh"])
        },
        |value| value["command_argv"] = json!(["cargo", "test", "--tests"]),
        |value| value["command_argv"] = json!(["cargo", "test", "\nunsafe"]),
        |value| value["command_argv"] = json!(["x".repeat(257)]),
        |value| value["command_argv"] = json!(vec!["x"; 65]),
        |value| value["command_argv"] = json!(vec!["x".repeat(256); 17]),
        |value| value["working_directory"] = json!(".."),
        |value| value["fixture_install_path"] = json!("tests/unrelated.rs"),
        |value| value["test_identifier"] = json!("unrelated"),
        |value| {
            value["command_argv"] = json!(["cargo", "test", "--test", "target.name"]);
            value["fixture_install_path"] = json!("tests/target.name.rs");
            value["test_identifier"] = json!("target.name");
        },
        |value| value["toolchain"]["version"] = json!("mismatch"),
        |value| value["fixture_sha256"] = json!("sha256:bad"),
        |value| value["output_sha256"] = json!("sha256:bad"),
        |value| value["failure_signature_sha256"] = json!("sha256:bad"),
        |value| value["failure_signature"] = json!("invented unrelated failure"),
        |value| {
            value["failure_signature"] = json!("x");
            value["failure_signature_sha256"] = json!(format!("sha256:{:x}", Sha256::digest(b"x")));
        },
    ];
    for mutation in mutations {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, mut reproduction) = valid_v2_pack();
        mutation(&mut reproduction);
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        assert_rejected(validate(&path, temp.path()));
    }

    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, mut reproduction) = valid_v2_pack();
    reproduction.as_object_mut().unwrap().remove("command_argv");
    reproduction["command"] = json!("cargo test");
    let path = write_v2_pack(&temp, &mut manifest, &reproduction);
    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn rejects_v2_output_that_does_not_contain_the_bound_failure_signature() {
    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, mut reproduction) = valid_v2_pack();
    let unrelated_output = b"test failed for an unrelated reason\n";
    let unrelated_digest = format!("sha256:{:x}", Sha256::digest(unrelated_output));
    reproduction["output_sha256"] = json!(unrelated_digest);
    manifest["reproduction_output"]["size_bytes"] = json!(unrelated_output.len());
    manifest["reproduction_output"]["sha256"] = json!(unrelated_digest);
    let path = write_v2_pack(&temp, &mut manifest, &reproduction);
    fs::write(temp.path().join(REPRODUCTION_OUTPUT_PATH), unrelated_output).unwrap();

    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn rejects_v2_malformed_or_stale_reproduction_evidence() {
    let (_, reproduction) = valid_v2_pack();
    let valid = serde_json::to_string(&reproduction).unwrap();
    let malformed = [
        valid.replacen('{', "{\"request_id\":\"duplicate\",", 1),
        valid.replacen('{', "{\"unknown\":true,", 1),
        format!("{valid} true"),
        "{".to_string(),
    ];
    for bytes in malformed.iter().map(String::as_bytes).chain([&[0xff][..]]) {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, _) = valid_v2_pack();
        let path = write_v2_pack_bytes_at(temp.path(), &mut manifest, bytes);
        assert_rejected(validate(&path, temp.path()));
    }

    for completed_at in [
        (Utc::now() - Duration::days(8)).to_rfc3339_opts(SecondsFormat::Secs, true),
        (Utc::now() + Duration::minutes(6)).to_rfc3339_opts(SecondsFormat::Secs, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, mut reproduction) = valid_v2_pack();
        reproduction["completed_at"] = json!(completed_at);
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        assert_rejected(validate(&path, temp.path()));
    }

    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, mut reproduction) = valid_v2_pack();
    reproduction["completed_at"] =
        json!((Utc::now() + Duration::minutes(1)).to_rfc3339_opts(SecondsFormat::Secs, true));
    let path = write_v2_pack(&temp, &mut manifest, &reproduction);
    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn rejects_v2_unsafe_aliased_linked_or_oversized_reproduction_artifact() {
    for invalid_path in [
        "/tmp/reproduction-evidence.json",
        "../reproduction-evidence.json",
        "nested/reproduction-evidence.json",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, reproduction) = valid_v2_pack();
        manifest["reproduction_evidence"]["path"] = json!(invalid_path);
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        assert_rejected(validate(&path, temp.path()));
    }

    for alias in [
        "source_archive",
        "issue_snapshot",
        "dependency_cache_manifest",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, reproduction) = valid_v2_pack();
        manifest["reproduction_evidence"] = manifest[alias].clone();
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        assert_rejected(validate(&path, temp.path()));
    }

    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, _reproduction) = valid_v2_pack();
    manifest["reproduction_evidence"]["size_bytes"] = json!(65_537_u64);
    let path = write_pack(&temp, &manifest);
    fs::write(temp.path().join(REPRODUCTION_PATH), vec![b'x'; 65_537]).unwrap();
    assert_rejected(validate(&path, temp.path()));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let (mut manifest, reproduction) = valid_v2_pack();
        let path = write_v2_pack(&temp, &mut manifest, &reproduction);
        let evidence = temp.path().join(REPRODUCTION_PATH);
        fs::remove_file(&evidence).unwrap();
        symlink(temp.path().join("issue.json"), &evidence).unwrap();
        assert_rejected(validate(&path, temp.path()));
    }

    let temp = tempfile::tempdir().unwrap();
    let (mut manifest, reproduction) = valid_v2_pack();
    let path = write_v2_pack(&temp, &mut manifest, &reproduction);
    fs::hard_link(
        temp.path().join(REPRODUCTION_PATH),
        temp.path().join("reproduction-alias.json"),
    )
    .unwrap();
    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn rejects_digest_size_identity_and_enum_drift() {
    for mutation in [
        |value: &mut Value| value["source_archive"]["sha256"] = json!(TREE_SHA256),
        |value: &mut Value| value["source_archive"]["size_bytes"] = json!(1),
        |value: &mut Value| value["dependency_cache_manifest"]["sha256"] = json!(TREE_SHA256),
        |value: &mut Value| value["dependency_cache_manifest"]["size_bytes"] = json!(1),
        |value: &mut Value| value["source_sha"] = json!("ABCDEF"),
        |value: &mut Value| value["license"] = json!("GPL-3.0"),
        |value: &mut Value| value["language"] = json!("python"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        mutation(&mut manifest);
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_every_unsafe_boundary_value() {
    let unsafe_values = [
        ("authority_level", json!("L2")),
        ("network", json!("host")),
        ("git_history_present", json!(true)),
        ("oracle_present", json!(true)),
        ("credentials_present", json!(true)),
        ("campaign_root_mounted", json!(true)),
        ("repair_pack_read_only", json!(false)),
        ("scratch_read_write", json!(false)),
        ("third_party_mutation_authorized", json!(true)),
    ];
    for (field, unsafe_value) in unsafe_values {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest["safety"][field] = unsafe_value;
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_malformed_timestamp_identifiers_repository_and_required_values() {
    let mutations = [
        ("fetched_at", json!("2026-08-01 12:34:56")),
        ("request_id", json!("")),
        ("corpus_id", json!("x".repeat(129))),
        ("candidate_id", Value::Null),
        ("repository", json!("not-canonical")),
        ("issue_number", json!(0)),
        ("known_fix_fetched", json!(true)),
        ("extracted_tree_sha256", json!("SHA256:bad")),
    ];
    for (field, invalid_value) in mutations {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest[field] = invalid_value;
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_noncanonical_github_repository_grammar() {
    let repositories = [
        "-owner/project",
        "owner-/project",
        "own--er/project",
        "owner_name/project",
        "owner.name/project",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/project",
        "owner/project.",
        "owner/project name",
        "owner/project@name",
        "owner/project.git",
        "owner/PROJECT.GIT",
        "owner/",
        "/project",
        "owner/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ];
    for repository in repositories {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest["repository"] = json!(repository);
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }

    for repository in [
        "a/b",
        "owner/.github",
        "owner-with-single-hyphens/repo_name.v1-beta",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest["repository"] = json!(repository);
        let path = write_pack(&temp, &manifest);
        let output = validate(&path, temp.path());
        assert!(
            output.status.success(),
            "valid repository {repository} rejected: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let temp = tempfile::tempdir().unwrap();
    let mut manifest = valid_manifest();
    manifest["repository"] = json!(format!("{}/project", "a".repeat(39)));
    let path = write_pack(&temp, &manifest);
    let output = validate(&path, temp.path());
    assert!(
        output.status.success(),
        "39-character owner rejected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rejects_stale_or_future_fetched_at() {
    for fetched_at in [
        (Utc::now() - Duration::days(8)).to_rfc3339_opts(SecondsFormat::Secs, true),
        (Utc::now() + Duration::minutes(6)).to_rfc3339_opts(SecondsFormat::Secs, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest["fetched_at"] = json!(fetched_at);
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_dot_repository_components() {
    for repository in ["./project", "owner/.."] {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest["repository"] = json!(repository);
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_duplicate_unknown_trailing_malformed_and_invalid_utf8_json() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_pack(&temp, &valid_manifest());
    let valid = fs::read_to_string(&path).unwrap();
    let malformed = [
        valid.replacen('{', "{\"request_id\":\"duplicate\",", 1),
        valid.replacen(
            "\"name\":\"rust\"",
            "\"name\":\"rust\",\"name\":\"duplicate\"",
            1,
        ),
        valid.replacen('{', "{\"unknown\":true,", 1),
        format!("{valid} true"),
        "{".to_string(),
    ];
    for bytes in malformed.iter().map(String::as_bytes).chain([&[0xff][..]]) {
        fs::write(&path, bytes).unwrap();
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_oversized_manifest_snapshot_and_declared_archive() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_pack(&temp, &valid_manifest());
    fs::write(&path, vec![b' '; 65_537]).unwrap();
    assert_rejected(validate(&path, temp.path()));

    let mut manifest = valid_manifest();
    manifest["dependency_cache_manifest"]["size_bytes"] = json!(262_145_u64);
    let path = write_pack(&temp, &manifest);
    fs::write(
        temp.path().join("dependency-cache.json"),
        vec![b'x'; 262_145],
    )
    .unwrap();
    assert_rejected(validate(&path, temp.path()));

    let mut manifest = valid_manifest();
    manifest["issue_snapshot"]["size_bytes"] = json!(262_145_u64);
    let path = write_pack(&temp, &manifest);
    fs::write(temp.path().join("issue.json"), vec![b'x'; 262_145]).unwrap();
    assert_rejected(validate(&path, temp.path()));

    let mut manifest = valid_manifest();
    manifest["source_archive"]["size_bytes"] = json!(1_073_741_825_u64);
    let path = write_pack(&temp, &manifest);
    assert_rejected(validate(&path, temp.path()));
}

#[test]
fn rejects_unsafe_and_aliased_artifact_paths() {
    for invalid_path in [
        "/tmp/source.tar.gz",
        "../source.tar.gz",
        "artifacts//source.tar.gz",
        "artifacts/source.tar.gz",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest["source_archive"]["path"] = json!(invalid_path);
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }

    for (left, right) in [
        ("source_archive", "issue_snapshot"),
        ("source_archive", "dependency_cache_manifest"),
        ("issue_snapshot", "dependency_cache_manifest"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = valid_manifest();
        manifest[right] = manifest[left].clone();
        let path = write_pack(&temp, &manifest);
        assert_rejected(validate(&path, temp.path()));
    }
}

#[test]
fn rejects_manifest_outside_root_or_reached_through_linked_parent() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("pack-root");
    fs::create_dir(&root).unwrap();
    let manifest = write_pack_at(&root, &valid_manifest());
    let outside = temp.path().join("outside-manifest.json");
    fs::rename(&manifest, &outside).unwrap();
    assert_rejected(validate(&outside, &root));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let real_parent = root.join("real-parent");
        fs::create_dir(&real_parent).unwrap();
        let real_manifest = real_parent.join("manifest.json");
        fs::write(
            &real_manifest,
            serde_json::to_vec(&valid_manifest()).unwrap(),
        )
        .unwrap();
        let linked_parent = root.join("linked-parent");
        symlink(&real_parent, &linked_parent).unwrap();
        assert_rejected(validate(&linked_parent.join("manifest.json"), &root));
    }
}

#[test]
fn rejects_nested_manifest_even_without_links() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("pack-root");
    let manifest = write_pack_at(&root, &valid_manifest());
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    let nested_manifest = nested.join("manifest.json");
    fs::rename(manifest, &nested_manifest).unwrap();

    assert_rejected(validate(&nested_manifest, &root));
}

#[test]
fn rejects_manifest_alias_of_a_referenced_artifact_before_digest_validation() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_path = write_pack(&temp, &valid_manifest());
    let manifest_bytes = fs::read(&manifest_path).unwrap();
    let mut manifest = valid_manifest();
    manifest["source_archive"]["path"] = json!("manifest.json");
    manifest["source_archive"]["size_bytes"] = json!(manifest_bytes.len());
    manifest["source_archive"]["sha256"] =
        json!(format!("sha256:{:x}", Sha256::digest(&manifest_bytes)));
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let output = validate(&manifest_path, temp.path());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("manifest must not alias"),
        "wrong rejection class: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rejects_symlinks_hardlinks_and_non_regular_files() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let path = write_pack(&temp, &valid_manifest());
        let source = temp.path().join("source.tar.gz");
        fs::remove_file(&source).unwrap();
        symlink(temp.path().join("issue.json"), &source).unwrap();
        assert_rejected(validate(&path, temp.path()));
    }

    let temp = tempfile::tempdir().unwrap();
    let path = write_pack(&temp, &valid_manifest());
    let source = temp.path().join("source.tar.gz");
    fs::hard_link(&source, temp.path().join("source-alias.tar.gz")).unwrap();
    assert_rejected(validate(&path, temp.path()));

    let temp = tempfile::tempdir().unwrap();
    let path = write_pack(&temp, &valid_manifest());
    let source = temp.path().join("source.tar.gz");
    fs::remove_file(&source).unwrap();
    fs::create_dir(&source).unwrap();
    assert_rejected(validate(&path, temp.path()));
}
