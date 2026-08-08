use super::{
    digest, is_digest, read_guarded_file, BoundArtifact, Bundle, RootGuard, MAX_INPUT_BYTES,
};
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

pub(super) fn validate_digests(
    values: &BTreeMap<String, String>,
    process_lifecycle_required: bool,
) -> Result<()> {
    let required_count = if process_lifecycle_required { 8 } else { 7 };
    if values.len() != required_count {
        if process_lifecycle_required {
            bail!("artifact_sha256 must contain exactly eight evidence roles");
        }
        bail!("artifact_sha256 must contain exactly seven evidence roles");
    }
    for required in [
        "source.json",
        "reproduction.json",
        "regression.json",
        "full-suite.json",
        "candidate-seal.json",
        "review.json",
        "draft-pr.json",
    ] {
        if !values.contains_key(required) {
            bail!("artifact_sha256 is missing required evidence roles");
        }
    }
    if process_lifecycle_required && !values.contains_key("process-lifecycle.json") {
        bail!("artifact_sha256 is missing required process lifecycle evidence");
    }
    for (name, value) in values {
        if name.is_empty()
            || name.len() > 128
            || name.contains("..")
            || !name.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.')
            })
        {
            bail!("artifact digest names must be bounded safe identifiers");
        }
        if !is_digest(value) {
            bail!("artifact values must use lowercase sha256:<64 hex>");
        }
    }
    Ok(())
}

pub(super) fn validate_files(root: &RootGuard, bundle_name: &str, bundle: &Bundle) -> Result<()> {
    validate(root, bundle_name, bundle, "source.json", &bundle.source)?;
    validate(
        root,
        bundle_name,
        bundle,
        "reproduction.json",
        &bundle.reproduction,
    )?;
    validate(
        root,
        bundle_name,
        bundle,
        "regression.json",
        &bundle.regression,
    )?;
    validate(
        root,
        bundle_name,
        bundle,
        "full-suite.json",
        &bundle.full_suite,
    )?;
    validate(
        root,
        bundle_name,
        bundle,
        "candidate-seal.json",
        &bundle.candidate_seal,
    )?;
    validate(root, bundle_name, bundle, "review.json", &bundle.review)?;
    validate(root, bundle_name, bundle, "draft-pr.json", &bundle.draft_pr)?;
    if let Some(process_lifecycle) = &bundle.process_lifecycle {
        validate(
            root,
            bundle_name,
            bundle,
            "process-lifecycle.json",
            process_lifecycle,
        )?;
    }
    Ok(())
}

fn validate<T: DeserializeOwned + PartialEq>(
    root: &RootGuard,
    bundle_name: &str,
    bundle: &Bundle,
    name: &str,
    expected_evidence: &T,
) -> Result<()> {
    if name == bundle_name {
        bail!("qualification bundle must not reference itself");
    }
    let bytes = read_guarded_file(root, name, MAX_INPUT_BYTES, name)?;
    if digest(&bytes) != bundle.artifact_sha256[name] {
        bail!("artifact digest mismatch for {name}");
    }
    let artifact: BoundArtifact<T> = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse strict qualification artifact {name}"))?;
    if artifact.repository != bundle.repository
        || artifact.upstream_repository_id != bundle.upstream_repository_id
        || artifact.issue_number != bundle.issue_number
        || artifact.baseline_source_sha != bundle.baseline_source_sha
        || artifact.candidate_sha != bundle.candidate_sha
        || artifact.evidence != *expected_evidence
    {
        bail!("artifact semantics mismatch for {name}");
    }
    Ok(())
}
