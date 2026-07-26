use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::artifact_safety::factory_app_run_bundle_reject_secret_markers;
use crate::cli_util::{atomic_write_text, json_string, sanitize_greenfield_id, sha256_file};
use crate::factory_compat::{read_factory_compat_value, reject_factory_provider_api_key_auth};
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

pub(crate) fn factory_native_evaluator_decision(
    report_path: &Path,
    evidence_pack_path: &Path,
) -> Result<serde_json::Value> {
    let report = if report_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        read_factory_compat_value(report_path)
            .with_context(|| format!("read AO2 native report {}", report_path.display()))?
    } else {
        serde_json::json!({})
    };
    let evidence_pack = read_factory_compat_value(evidence_pack_path)
        .with_context(|| format!("read AO2 evidence pack {}", evidence_pack_path.display()))?;
    let closure = if !report["closure"].is_null() {
        report["closure"].clone()
    } else {
        evidence_pack["closures"]
            .as_array()
            .and_then(|closures| closures.last().cloned())
            .unwrap_or_else(|| serde_json::json!({}))
    };
    let verdict = closure
        .get("verdict")
        .and_then(|value| value.as_str())
        .or_else(|| {
            evidence_pack
                .get("verdict")
                .and_then(|value| value.as_str())
        })
        .unwrap_or("blocked")
        .to_string();
    let unresolved_concerns = closure
        .get("unresolved_concerns")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let blockers = closure
        .get("blockers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(serde_json::json!({
        "schema_version": "ao2.native-evaluator-decision.v1",
        "owner": "ao2-native-evaluator-closer",
        "verdict": verdict,
        "closure": closure,
        "unresolved_concern_count": unresolved_concerns.len(),
        "blocker_count": blockers.len(),
        "report_path": report_path.display().to_string(),
        "evidence_pack_path": evidence_pack_path.display().to_string(),
        "factory_v3_required_to_decide": false
    }))
}

pub(crate) fn factory_evaluate_json(
    evidence_pack_path: &Path,
    report_path: Option<&Path>,
    factory_decision_path: Option<&Path>,
    signing_key: Option<&Path>,
    signer_id: &str,
    out: Option<&Path>,
) -> Result<serde_json::Value> {
    let report_path = report_path.unwrap_or(evidence_pack_path);
    let native_decision = factory_native_evaluator_decision(report_path, evidence_pack_path)?;
    let parity_comparison = match factory_decision_path {
        Some(path) => factory_evaluator_parity_comparison(path, &native_decision)?,
        None => serde_json::json!({
            "schema_version": "ao2.factory-v3-evaluator-parity-comparison.v1",
            "status": "not_requested",
            "factory_v3_role": "parity_oracle_only",
            "ao2_decision_owner": "ao2-native-evaluator-closer"
        }),
    };
    let decision_path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| evidence_pack_path.with_extension("native-evaluator-decision.json"));
    if let Some(parent) = decision_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut result = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-native-evaluator-result.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "owner": "ao2-native-evaluator-closer",
        "verdict": native_decision["verdict"].clone(),
        "decision_path": decision_path.display().to_string(),
        "native_evaluator_decision": native_decision,
        "factory_v3_evaluator_parity": parity_comparison,
        "trust_boundary": {
            "decision_owner": "ao2",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "parity_checklist_progress": {
            "ao2_can_evaluate_existing_evidence_without_factory_driver": true,
            "ao2_owns_evaluator_closer_decision": true,
            "factory_v3_drives_workflow": false,
            "factory_v3_evaluator_compared_when_supplied": factory_decision_path.is_some(),
            "ao2_can_sign_native_evaluator_decision": signing_key.is_some()
        }
    });
    let signed_payload_path = decision_path.with_extension("signed-payload.json");
    atomic_write_text(
        &signed_payload_path,
        &serde_json::to_string_pretty(&result)?,
    )?;
    let signature = match signing_key {
        Some(key_path) => {
            let signature_path = decision_path.with_extension("json.sig");
            let public_key_path = decision_path.with_extension("public.pem");
            derive_public_key_from_private_key(key_path, &public_key_path)?;
            sign_file_with_private_key(key_path, &signed_payload_path, &signature_path)?;
            serde_json::json!({
                "schema_version": "ao2.factory-compat-native-evaluator-signature.v1",
                "signature_algorithm": "RSA/SHA-256",
                "signer_id": signer_id,
                "signed_payload": "native_evaluator_decision_without_signature_field",
                "signed_payload_path": signed_payload_path.display().to_string(),
                "signed_payload_sha256": sha256_file(&signed_payload_path)?,
                "signature_path": signature_path.display().to_string(),
                "signature_sha256": sha256_file(&signature_path)?,
                "public_key_path": public_key_path.display().to_string(),
                "public_key_sha256": sha256_file(&public_key_path)?,
                "signature_verified": verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?
            })
        }
        None => serde_json::json!({
            "schema_version": "ao2.factory-compat-native-evaluator-signature.v1",
            "signed_payload_path": signed_payload_path.display().to_string(),
            "signed_payload_sha256": sha256_file(&signed_payload_path)?,
            "signature_verified": false,
            "signature_status": "unsigned"
        }),
    };
    if let Some(object) = result.as_object_mut() {
        object.insert("signature".to_string(), signature);
    }
    atomic_write_text(&decision_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn factory_evaluator_parity_comparison(
    factory_decision_path: &Path,
    native_decision: &serde_json::Value,
) -> Result<serde_json::Value> {
    let factory_decision = read_factory_compat_value(factory_decision_path).with_context(|| {
        format!(
            "read factory-v3 evaluator decision {}",
            factory_decision_path.display()
        )
    })?;
    let factory_verdict =
        factory_extract_verdict(&factory_decision).unwrap_or_else(|| "unknown".to_string());
    let ao2_verdict = json_string(native_decision, "verdict");
    let verdict_matches = factory_verdict == ao2_verdict;
    let status = if factory_verdict == "unknown" {
        "factory_verdict_unavailable"
    } else if verdict_matches {
        "matched"
    } else {
        "mismatched"
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-evaluator-parity-comparison.v1",
        "status": status,
        "factory_decision_path": factory_decision_path.display().to_string(),
        "factory_verdict": factory_verdict,
        "ao2_verdict": ao2_verdict,
        "verdict_matches": verdict_matches,
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-evaluator-closer"
    }))
}

fn factory_extract_verdict(value: &serde_json::Value) -> Option<String> {
    for key in ["verdict", "decision", "status", "outcome"] {
        if let Some(text) = value.get(key).and_then(|candidate| candidate.as_str()) {
            if let Some(verdict) = normalize_factory_verdict(text) {
                return Some(verdict);
            }
        }
        if let Some(nested) = value.get(key).and_then(factory_extract_verdict) {
            return Some(nested);
        }
    }
    if let Some(accepted) = value
        .get("accepted")
        .and_then(|candidate| candidate.as_bool())
    {
        return Some(if accepted { "accepted" } else { "rejected" }.to_string());
    }
    None
}

fn normalize_factory_verdict(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    if normalized.contains("accept") || normalized == "passed" || normalized == "pass" {
        Some("accepted".to_string())
    } else if normalized.contains("reject") || normalized == "failed" || normalized == "fail" {
        Some("rejected".to_string())
    } else if normalized.contains("block") {
        Some("blocked".to_string())
    } else {
        None
    }
}
