use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::artifact_safety::factory_app_run_bundle_reject_secret_markers;
use crate::cli_util::{atomic_write_text, json_string, sanitize_greenfield_id, sha256_file};
use crate::factory_compat::reject_factory_provider_api_key_auth;
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};

pub(crate) struct FactoryEvaluatorRubricOptions<'a> {
    pub(crate) spec: &'a Path,
    pub(crate) run_id: String,
    pub(crate) verifier_command: String,
    pub(crate) signing_key: Option<PathBuf>,
    pub(crate) signer_id: String,
    pub(crate) out: &'a Path,
}

pub(crate) fn factory_project_acceptance_criteria(
    project_spec_text: &str,
    verifier_command: &str,
) -> Vec<serde_json::Value> {
    let mut criteria = Vec::new();
    let mut in_acceptance = false;
    for line in project_spec_text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if trimmed.starts_with('#') || lower.ends_with(':') {
            in_acceptance = lower.contains("acceptance") || lower.contains("success criteria");
            continue;
        }
        if in_acceptance {
            if let Some(item) = trimmed.strip_prefix("- ") {
                let item = item.trim();
                if !item.is_empty() {
                    criteria.push(serde_json::json!({
                        "id": format!("spec-criterion-{}", criteria.len() + 1),
                        "criterion": item,
                        "required": true
                    }));
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('-') {
                in_acceptance = false;
            }
        }
    }
    if criteria.is_empty() {
        criteria.push(serde_json::json!({
            "id": "default-verifier-green",
            "criterion": format!("configured verifier command exits 0: {verifier_command}"),
            "required": true
        }));
        criteria.push(serde_json::json!({
            "id": "default-trust-boundary",
            "criterion": "AO2-owned execution preserves factory-v3 evaluator-closer and read-only control-plane boundaries",
            "required": true
        }));
    }
    criteria
}

pub(crate) fn factory_project_spec_title(project_spec_text: &str) -> String {
    project_spec_text
        .lines()
        .find_map(|line| line.trim().strip_prefix("# "))
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "AO2 Project".to_string())
}

pub(crate) fn factory_evaluator_rubric_json(
    options: FactoryEvaluatorRubricOptions<'_>,
) -> Result<serde_json::Value> {
    if !options.spec.is_file() {
        anyhow::bail!(
            "factory evaluator rubric spec does not exist: {}",
            options.spec.display()
        );
    }
    let spec_text = fs::read_to_string(options.spec)
        .with_context(|| format!("read evaluator rubric spec {}", options.spec.display()))?;
    reject_factory_provider_api_key_auth(
        "factory_evaluator_rubric_spec",
        &serde_json::json!({ "spec": spec_text }),
    )?;

    if let Some(parent) = options
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let signed_payload_path = options.out.with_extension("signed-payload.json");
    let signature_path = options.out.with_extension("json.sig");
    let public_key_path = options.out.with_extension("public.pem");
    let signing_key = options
        .signing_key
        .as_deref()
        .context("factory evaluator rubric requires --signing-key for signed AO2 acceptance bar")?;
    let run_id = sanitize_greenfield_id(&options.run_id);
    let title = factory_project_spec_title(&spec_text);
    let spec_sha256 = sha256_file(options.spec)?;
    let criteria = factory_project_acceptance_criteria(&spec_text, &options.verifier_command);
    let trust_boundary = serde_json::json!({
        "execution_owner": "ao2",
        "acceptance_bar_owner": "ao2 evaluator-closer",
        "factory_v3_role": "parity_auditor",
        "factory_v3_drives_workflow": false,
        "control_plane_role": "read_only_observer",
        "control_plane_approves_release": false,
        "mutates_ao_artifacts": false,
        "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
    });
    let mut rubric = serde_json::json!({
        "schema_version": "ao2.factory-evaluator-rubric.v1",
        "status": "accepted",
        "run_id": run_id,
        "title": title,
        "source_spec": options.spec.display().to_string(),
        "source_spec_sha256": spec_sha256,
        "verifier_command": options.verifier_command,
        "rubric_kind": "ao2_native_acceptance_bar",
        "release_acceptance": {
            "primary_owner": "ao2 evaluator-closer",
            "factory_v3_role": "parity_auditor",
            "factory_v3_compares_or_audits": true,
            "factory_v3_drives_workflow": false
        },
        "criteria": criteria,
        "thresholds": {
            "failed_criteria_count": 0,
            "verifier_exit_code": 0,
            "required_signature_status": "signed"
        },
        "required_evidence_refs": [
            "rubric_sha256",
            "governed_run",
            "provider_evidence",
            "verification",
            "closure_decision"
        ],
        "downstream_contract": {
            "verifier_outputs_must_reference": "rubric_sha256",
            "closer_outputs_must_reference": "rubric_sha256",
            "factory_v3_may_compare_or_audit": true,
            "factory_v3_must_not_be_primary_producer": true
        },
        "trust_boundary": trust_boundary
    });
    atomic_write_text(
        &signed_payload_path,
        &serde_json::to_string_pretty(&rubric)?,
    )?;
    factory_app_run_bundle_reject_secret_markers(
        &signed_payload_path,
        "evaluator-rubric-signed-payload.json",
    )?;
    derive_public_key_from_private_key(signing_key, &public_key_path)?;
    sign_file_with_private_key(signing_key, &signed_payload_path, &signature_path)?;
    let signature_verified =
        verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?;
    let signature = serde_json::json!({
        "schema_version": "ao2.factory-evaluator-rubric-signature.v1",
        "signature_algorithm": "RSA/SHA-256",
        "signer_id": options.signer_id,
        "signed_payload": "evaluator_rubric_without_signature_field",
        "signed_payload_path": signed_payload_path.display().to_string(),
        "signed_payload_sha256": sha256_file(&signed_payload_path)?,
        "signature_path": signature_path.display().to_string(),
        "signature_sha256": sha256_file(&signature_path)?,
        "public_key_path": public_key_path.display().to_string(),
        "public_key_sha256": sha256_file(&public_key_path)?,
        "signature_status": "signed",
        "signature_verified": signature_verified
    });
    rubric["signature"] = signature;
    atomic_write_text(options.out, &serde_json::to_string_pretty(&rubric)?)?;
    factory_app_run_bundle_reject_secret_markers(options.out, "evaluator-rubric.json")?;
    let rubric_sha256 = sha256_file(options.out)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-evaluator-rubric-result.v1",
        "status": "accepted",
        "run_id": json_string(&rubric, "run_id"),
        "rubric_sha256": rubric_sha256,
        "rubric": rubric,
        "artifacts": {
            "rubric": options.out.display().to_string(),
            "signed_payload": signed_payload_path.display().to_string(),
            "signature": if signature_path.is_file() {
                serde_json::Value::String(signature_path.display().to_string())
            } else {
                serde_json::Value::Null
            },
            "public_key": if public_key_path.is_file() {
                serde_json::Value::String(public_key_path.display().to_string())
            } else {
                serde_json::Value::Null
            }
        }
    }))
}
