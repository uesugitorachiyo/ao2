use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

const PLAN_SCHEMA: &str = "ao2.hosted-release-promotion-plan.v1";
const MAX_PLAN_BYTES: u64 = 2 * 1024 * 1024;
const TARGETS: [(&str, &str, &str); 3] = [
    ("macos-aarch64", "macos-latest", "aarch64-apple-darwin"),
    ("linux-x86_64", "ubuntu-latest", "x86_64-unknown-linux-gnu"),
    ("windows-x86_64", "windows-latest", "x86_64-pc-windows-msvc"),
];

pub struct HostedReleaseVerification {
    pub report: Value,
    pub verified: bool,
    pub tag_matches: bool,
}

pub fn expected_hosted_release_assets(version: &str) -> Vec<String> {
    vec![
        format!("ao2-{version}-macos-aarch64.tar.gz"),
        format!("ao2-{version}-linux-x86_64.tar.gz"),
        format!("ao2-{version}-windows-x86_64.tar.gz"),
        "promotion-plan.json".to_string(),
        "SHA256SUMS".to_string(),
    ]
}

pub fn is_hosted_release_directory(root: &Path) -> bool {
    root.join("promotion-plan.json").is_file()
}

pub fn expected_doctor_release_assets(root: Option<&Path>, version: &str) -> Vec<String> {
    if root.is_none() || root.is_some_and(is_hosted_release_directory) {
        return expected_hosted_release_assets(version);
    }
    let mut names = Vec::new();
    for target in [
        "macos-aarch64",
        "linux-aarch64",
        "linux-x86_64",
        "windows-x86_64",
    ] {
        let archive = format!("ao2-{version}-{target}.tar.gz");
        names.push(archive.clone());
        names.push(format!("{archive}.sha256"));
        names.push(format!("{archive}.sig"));
    }
    names.extend(
        [
            "ao2-release-provenance.json",
            "ao2-release-provenance.json.sig",
            "ao2-release-signing-public.pem",
        ]
        .map(str::to_string),
    );
    names
}

pub fn verify_hosted_release_directory(
    root: &Path,
    version: &str,
    release_tag: &str,
) -> HostedReleaseVerification {
    let expected_assets = expected_hosted_release_assets(version);
    let mut errors = Vec::new();
    let checksums = match verify_checksums(root, &expected_assets) {
        Ok(checksums) => Some(checksums),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    let plan = match verify_plan(root, version, release_tag, checksums.as_ref()) {
        Ok(plan) => Some(plan),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    let checksums_verified = checksums.is_some();
    let promotion_plan_verified = plan.is_some();
    let tag_matches = plan
        .as_ref()
        .and_then(|value| value.get("tag"))
        .and_then(Value::as_str)
        == Some(release_tag);
    let source_sha = plan
        .as_ref()
        .and_then(|value| value.get("source_sha"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let verified = checksums_verified && promotion_plan_verified && tag_matches;
    HostedReleaseVerification {
        report: serde_json::json!({
            "checked": true,
            "schema_version": PLAN_SCHEMA,
            "status": if verified { "verified" } else { "invalid" },
            "checksums_verified": checksums_verified,
            "promotion_plan_verified": promotion_plan_verified,
            "release_tag_matches": tag_matches,
            "source_sha": source_sha,
            "errors": errors,
        }),
        verified,
        tag_matches,
    }
}

fn verify_checksums(
    root: &Path,
    expected_assets: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let contents = fs::read_to_string(root.join("SHA256SUMS"))
        .map_err(|_| "missing or unreadable SHA256SUMS".to_string())?;
    let expected = expected_assets
        .iter()
        .filter(|name| name.as_str() != "SHA256SUMS")
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut parsed = BTreeMap::new();
    for line in contents.lines() {
        let Some((digest, name)) = line.split_once("  ") else {
            return Err("invalid SHA256SUMS row".to_string());
        };
        if !is_lower_hex(digest, 64)
            || name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || parsed
                .insert(name.to_string(), digest.to_string())
                .is_some()
        {
            return Err("invalid SHA256SUMS row".to_string());
        }
    }
    if parsed.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err("SHA256SUMS inventory does not match hosted assets".to_string());
    }
    for (name, expected_digest) in &parsed {
        let path = root.join(name);
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| format!("missing hosted asset: {name}"))?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(format!("unsafe hosted asset: {name}"));
        }
        let actual_digest =
            sha256_file(&path).map_err(|_| format!("unreadable hosted asset: {name}"))?;
        if &actual_digest != expected_digest {
            return Err(format!("hosted asset digest mismatch: {name}"));
        }
    }
    Ok(parsed)
}

fn verify_plan(
    root: &Path,
    version: &str,
    release_tag: &str,
    checksums: Option<&BTreeMap<String, String>>,
) -> Result<Value, String> {
    let plan_path = root.join("promotion-plan.json");
    let metadata =
        fs::symlink_metadata(&plan_path).map_err(|_| "missing promotion-plan.json".to_string())?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("unsafe promotion-plan.json".to_string());
    }
    if metadata.len() > MAX_PLAN_BYTES {
        return Err("promotion-plan.json exceeds size limit".to_string());
    }
    let plan: Value = serde_json::from_slice(
        &fs::read(&plan_path).map_err(|_| "unreadable promotion-plan.json".to_string())?,
    )
    .map_err(|_| "invalid promotion-plan.json".to_string())?;
    let object = plan
        .as_object()
        .ok_or_else(|| "promotion plan must be an object".to_string())?;
    let expected_keys = [
        "schema_version",
        "status",
        "version",
        "tag",
        "source_sha",
        "approved_asset_manifest_sha256",
        "physical_windows_evidence_sha256",
        "artifacts",
        "windows",
        "rejection_policy",
        "trust_boundary",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if object.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_keys {
        return Err("promotion plan keys do not match hosted contract".to_string());
    }
    for (key, expected) in [
        ("schema_version", PLAN_SCHEMA),
        ("status", "passed"),
        ("version", version),
        ("tag", release_tag),
    ] {
        if object.get(key).and_then(Value::as_str) != Some(expected) {
            return Err(format!("promotion plan {key} mismatch"));
        }
    }
    for key in [
        "approved_asset_manifest_sha256",
        "physical_windows_evidence_sha256",
    ] {
        if !object
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| is_lower_hex(value, 64))
        {
            return Err(format!("promotion plan {key} is invalid"));
        }
    }
    if !object
        .get("source_sha")
        .and_then(Value::as_str)
        .is_some_and(|value| is_lower_hex(value, 40))
    {
        return Err("promotion plan source_sha is invalid".to_string());
    }
    verify_artifacts(object.get("artifacts"), version, checksums)?;
    verify_windows_boundary(object.get("windows"))?;
    verify_trust_boundary(object.get("trust_boundary"))?;
    verify_rejection_policy(object.get("rejection_policy"))?;
    Ok(plan)
}

fn verify_artifacts(
    artifacts: Option<&Value>,
    version: &str,
    checksums: Option<&BTreeMap<String, String>>,
) -> Result<(), String> {
    let rows = artifacts
        .and_then(Value::as_array)
        .ok_or_else(|| "promotion plan artifacts are invalid".to_string())?;
    if rows.len() != TARGETS.len() {
        return Err("promotion plan artifact inventory mismatch".to_string());
    }
    let expected_keys = [
        "target",
        "runner",
        "target_triple",
        "archive",
        "sha256",
        "canonical_public_archive",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for row in rows {
        let object = row
            .as_object()
            .ok_or_else(|| "promotion plan artifact must be an object".to_string())?;
        if object.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_keys {
            return Err("promotion plan artifact keys mismatch".to_string());
        }
        let target = object
            .get("target")
            .and_then(Value::as_str)
            .ok_or_else(|| "promotion plan artifact target is invalid".to_string())?;
        let Some((_, runner, target_triple)) =
            TARGETS.iter().find(|(expected, _, _)| *expected == target)
        else {
            return Err("promotion plan artifact target mismatch".to_string());
        };
        if !seen.insert(target)
            || object.get("runner").and_then(Value::as_str) != Some(*runner)
            || object.get("target_triple").and_then(Value::as_str) != Some(*target_triple)
            || object
                .get("canonical_public_archive")
                .and_then(Value::as_bool)
                != Some(true)
        {
            return Err(format!(
                "promotion plan artifact contract mismatch: {target}"
            ));
        }
        let expected_name = format!("ao2-{version}-{target}.tar.gz");
        let archive = object
            .get("archive")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            });
        let digest = object.get("sha256").and_then(Value::as_str);
        if archive.as_deref() != Some(&expected_name)
            || !digest.is_some_and(|value| is_lower_hex(value, 64))
            || checksums.and_then(|values| values.get(&expected_name).map(String::as_str)) != digest
        {
            return Err(format!("promotion plan artifact digest mismatch: {target}"));
        }
    }
    if seen.len() != TARGETS.len() {
        return Err("promotion plan artifact inventory mismatch".to_string());
    }
    Ok(())
}

fn verify_windows_boundary(value: Option<&Value>) -> Result<(), String> {
    let expected = serde_json::json!({
        "canonical_target_triple": "x86_64-pc-windows-msvc",
        "canonical_runner": "windows-latest",
        "linux_mingw_cross_build": {
            "target_triple": "x86_64-pc-windows-gnu",
            "classification": "non_authoritative",
            "canonical_public_windows_archive": false,
        },
    });
    if value != Some(&expected) {
        return Err("promotion plan Windows boundary mismatch".to_string());
    }
    Ok(())
}

fn verify_trust_boundary(value: Option<&Value>) -> Result<(), String> {
    let expected = serde_json::json!({
        "build_jobs_mutate_releases": false,
        "plan_job_mutates_releases": false,
        "stores_credentials": false,
        "uses_workflow_scoped_github_token": true,
    });
    if value != Some(&expected) {
        return Err("promotion plan trust boundary mismatch".to_string());
    }
    Ok(())
}

fn verify_rejection_policy(value: Option<&Value>) -> Result<(), String> {
    let expected = serde_json::json!([
        "missing_artifact",
        "duplicate_artifact",
        "stale_source_sha",
        "substituted_archive",
        "unexpected_artifact",
        "version_tag_mismatch",
        "approved_manifest_mismatch",
        "physical_windows_evidence_mismatch",
        "incorrect_live_confirmation",
    ]);
    if value != Some(&expected) {
        return Err("promotion plan rejection policy mismatch".to_string());
    }
    Ok(())
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
