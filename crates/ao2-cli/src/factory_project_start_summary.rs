use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ao2_core::sha256_hex;

use crate::factory_compat::{read_factory_compat_value, reject_factory_provider_api_key_auth};
use crate::factory_project_contract::factory_project_start_bundle_verify_trust_boundary;
use crate::factory_project_start::factory_project_start_bundle_raw_path;

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("open {}", path.display()))?;
    Ok(sha256_hex(bytes))
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn factory_project_start_summary_json(
    project_start_path: &Path,
    bundle_verification_path: &Path,
) -> Result<serde_json::Value> {
    let project_start = read_factory_compat_value(project_start_path)?;
    reject_factory_provider_api_key_auth(
        "factory_project_start_summary_project_start",
        &project_start,
    )?;
    let bundle_verification = read_factory_compat_value(bundle_verification_path)?;
    reject_factory_provider_api_key_auth(
        "factory_project_start_summary_bundle_verification",
        &bundle_verification,
    )?;

    let project_start_base = project_start_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut failures = Vec::<serde_json::Value>::new();
    let mut artifacts = serde_json::Map::new();
    for (summary_key, artifact_key, sha_key) in [
        ("project_plan", "project_plan", "project_plan_sha256"),
        (
            "acceptance_rubric",
            "acceptance_rubric",
            "acceptance_rubric_sha256",
        ),
        (
            "project_run",
            "factory_project_run",
            "factory_project_run_sha256",
        ),
        (
            "release_review_package",
            "release_review_package",
            "release_review_package_sha256",
        ),
        (
            "project_acceptance_review",
            "project_acceptance_review",
            "project_acceptance_review_sha256",
        ),
        (
            "project_start_bundle",
            "project_start_bundle",
            "project_start_bundle_sha256",
        ),
    ] {
        let expected = json_string(&project_start["artifacts"], sha_key);
        artifacts.insert(
            summary_key.to_string(),
            factory_project_start_summary_artifact(
                project_start_base,
                &json_string(&project_start["artifacts"], artifact_key),
                &expected,
                summary_key,
                &mut failures,
            ),
        );
    }

    let bundle_verification_sha256 = sha256_file(bundle_verification_path).with_context(|| {
        format!(
            "hash project-start bundle verification {}",
            bundle_verification_path.display()
        )
    })?;
    artifacts.insert(
        "project_start_bundle_verification".to_string(),
        serde_json::json!({
            "path": bundle_verification_path.display().to_string(),
            "exists": bundle_verification_path.is_file(),
            "sha256": bundle_verification_sha256,
            "expected_sha256": bundle_verification_sha256,
            "status": json_string(&bundle_verification, "status")
        }),
    );
    if !bundle_verification_path.is_file() {
        failures.push(serde_json::json!({
            "code": "bundle_verification_missing",
            "path": bundle_verification_path,
            "message": "project-start bundle verification file must exist"
        }));
    }

    let project_start_bundle_sha256 =
        json_string(&project_start["artifacts"], "project_start_bundle_sha256");
    if json_string(&bundle_verification, "bundle_sha256") != project_start_bundle_sha256 {
        failures.push(serde_json::json!({
            "code": "bundle_verification_digest_mismatch",
            "expected": project_start_bundle_sha256,
            "actual": json_string(&bundle_verification, "bundle_sha256"),
            "message": "bundle verification must reference the project-start bundle digest"
        }));
    }
    if json_string(&bundle_verification, "status") != "accepted" {
        failures.push(serde_json::json!({
            "code": "bundle_verification_not_accepted",
            "status": json_string(&bundle_verification, "status"),
            "message": "project-start summary requires accepted bundle verification"
        }));
    }
    if json_string(&project_start, "schema_version") != "ao2.factory-project-start.v1" {
        failures.push(serde_json::json!({
            "code": "project_start_schema_invalid",
            "message": "project-start summary requires ao2.factory-project-start.v1"
        }));
    }
    if json_string(&project_start, "status") != "accepted" {
        failures.push(serde_json::json!({
            "code": "project_start_not_accepted",
            "status": json_string(&project_start, "status"),
            "message": "project-start summary requires accepted project-start"
        }));
    }
    if !factory_project_start_bundle_verify_trust_boundary(
        &project_start["factory_replacement_boundary"],
    ) && !factory_project_start_bundle_verify_trust_boundary(&project_start["trust_boundary"])
    {
        failures.push(serde_json::json!({
            "code": "project_start_trust_boundary_invalid",
            "message": "project-start summary requires evaluator-closer and observer-only trust boundary"
        }));
    }
    if !factory_project_start_bundle_verify_trust_boundary(&bundle_verification["trust_boundary"]) {
        failures.push(serde_json::json!({
            "code": "bundle_verification_trust_boundary_invalid",
            "message": "bundle verification must preserve observer-only trust boundary"
        }));
    }

    let status = if failures.is_empty() {
        "accepted"
    } else {
        "failed"
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-project-start-operator-summary.v1",
        "status": status,
        "run_id": json_string(&project_start, "run_id"),
        "project_start": project_start_path.display().to_string(),
        "project_start_sha256": sha256_file(project_start_path)?,
        "project_status": json_string(&project_start, "status"),
        "bundle_verification_status": json_string(&bundle_verification, "status"),
        "artifacts": artifacts,
        "checks": {
            "project_start_accepted": json_string(&project_start, "status") == "accepted",
            "bundle_verification_accepted": json_string(&bundle_verification, "status") == "accepted",
            "bundle_digest_matches": json_string(&bundle_verification, "bundle_sha256") == project_start_bundle_sha256,
            "trust_boundary_verified": failures.iter().all(|failure| {
                let code = json_string(failure, "code");
                !code.contains("trust_boundary")
            })
        },
        "failure_count": failures.len(),
        "failures": failures,
        "trust_boundary": {
            "execution_owner": "ao2",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    }))
}

fn factory_project_start_summary_artifact(
    base: &Path,
    raw_path: &str,
    expected_sha256: &str,
    label: &str,
    failures: &mut Vec<serde_json::Value>,
) -> serde_json::Value {
    let resolved = factory_project_start_bundle_raw_path(base, raw_path)
        .unwrap_or_else(|_| PathBuf::from(raw_path));
    let mut exists = resolved.is_file();
    let mut actual_sha256 = String::new();
    if raw_path.trim().is_empty() {
        exists = false;
        failures.push(serde_json::json!({
            "code": "summary_artifact_missing_path",
            "label": label,
            "message": "required project-start artifact path is missing"
        }));
    } else if !exists {
        failures.push(serde_json::json!({
            "code": "summary_artifact_missing",
            "label": label,
            "path": resolved,
            "message": "required project-start artifact is missing"
        }));
    } else {
        match sha256_file(&resolved) {
            Ok(sha256) => {
                actual_sha256 = sha256;
                if expected_sha256.trim().is_empty() || actual_sha256 != expected_sha256 {
                    failures.push(serde_json::json!({
                        "code": "summary_artifact_digest_mismatch",
                        "label": label,
                        "path": resolved,
                        "expected": expected_sha256,
                        "actual": actual_sha256,
                        "message": "required project-start artifact digest mismatch"
                    }));
                }
            }
            Err(error) => {
                failures.push(serde_json::json!({
                    "code": "summary_artifact_unreadable",
                    "label": label,
                    "path": resolved,
                    "message": error.to_string()
                }));
            }
        }
    }
    serde_json::json!({
        "path": resolved.display().to_string(),
        "exists": exists,
        "sha256": actual_sha256,
        "expected_sha256": expected_sha256
    })
}

pub(crate) fn factory_project_start_summary_markdown(summary: &serde_json::Value) -> String {
    let mut body = String::new();
    body.push_str("# Project-Start Operator Summary\n\n");
    body.push_str(&format!("status: {}\n\n", json_string(summary, "status")));
    body.push_str(&format!("run_id: {}\n\n", json_string(summary, "run_id")));
    body.push_str("## Artifacts\n\n");
    if let Some(artifacts) = summary["artifacts"].as_object() {
        for (label, artifact) in artifacts {
            body.push_str(&format!(
                "- {}: status={} exists={} sha256={}\n",
                label,
                json_string(artifact, "status"),
                artifact["exists"].as_bool().unwrap_or(false),
                json_string(artifact, "sha256")
            ));
            body.push_str(&format!("  path: `{}`\n", json_string(artifact, "path")));
        }
    }
    body.push_str("\n## Trust Boundary\n\n");
    body.push_str("- release_acceptance_owner: factory-v3 evaluator-closer\n");
    body.push_str("- control_plane_role: read_only_observer_after_signed_evidence\n");
    body.push_str("- control_plane_approves_release: false\n");
    body.push_str("- mutates_ao_artifacts: false\n");
    body
}
