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
