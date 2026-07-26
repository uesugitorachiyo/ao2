use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::{json_array, json_string};
use crate::contract_verify_obligation_gate_signing_json;
use crate::factory_compat::read_factory_compat_value;
use crate::factory_governance::{
    json_path, normalize_factory_replacement_smoke_os, require_json_bool, require_json_eq,
    FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS, GREENFIELD_THREE_OS_REQUIRED_OS,
};
use crate::release_crypto::verify_release_archive_signature;
use crate::release_provenance::verify_release_provenance_signature;
use crate::release_summary::{
    release_obligation_gate_verification_json, release_smoke_summary_verification_json,
    resolve_summary_sidecar_path,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn release_gate(
    summary: PathBuf,
    provenance_dir: PathBuf,
    macos_archive: Option<PathBuf>,
    linux_archive: PathBuf,
    linux_x86_64_archive: PathBuf,
    windows_archive: PathBuf,
    require_native_windows: bool,
    replacement_smoke_gate: Option<PathBuf>,
    greenfield_three_os_smoke_gate: Option<PathBuf>,
    governed_run_evidence: Vec<PathBuf>,
    factory_project_run_summaries: Vec<PathBuf>,
    require_obligation_gate_signing: bool,
) -> Result<()> {
    let report = release_gate_report_json(
        summary,
        provenance_dir,
        macos_archive,
        linux_archive,
        linux_x86_64_archive,
        windows_archive,
        require_native_windows,
        replacement_smoke_gate,
        greenfield_three_os_smoke_gate,
        governed_run_evidence,
        factory_project_run_summaries,
        require_obligation_gate_signing,
    )?;
    let status = json_string(&report, "status");
    println!("{}", serde_json::to_string_pretty(&report)?);
    if status != "verified" {
        anyhow::bail!("release gate failed");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn release_gate_report_json(
    summary: PathBuf,
    provenance_dir: PathBuf,
    macos_archive: Option<PathBuf>,
    linux_archive: PathBuf,
    linux_x86_64_archive: PathBuf,
    windows_archive: PathBuf,
    require_native_windows: bool,
    replacement_smoke_gate: Option<PathBuf>,
    greenfield_three_os_smoke_gate: Option<PathBuf>,
    governed_run_evidence: Vec<PathBuf>,
    factory_project_run_summaries: Vec<PathBuf>,
    require_obligation_gate_signing: bool,
) -> Result<serde_json::Value> {
    let body =
        fs::read_to_string(&summary).with_context(|| format!("read {}", summary.display()))?;
    let summary_json: serde_json::Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", summary.display()))?;
    let smoke =
        release_smoke_summary_verification_json(&summary, &summary_json, require_native_windows);
    let obligation_gates = release_obligation_gate_verification_json(&summary_json);

    let public_key = provenance_dir.join("ao2-release-signing-public.pem");
    let provenance_json = provenance_dir.join("ao2-release-provenance.json");
    let provenance_signature = provenance_dir.join("ao2-release-provenance.json.sig");
    let provenance_verified =
        verify_release_provenance_signature(&provenance_json, &provenance_signature, &public_key);

    let mut archives = Vec::new();
    if let Some(macos_archive) = macos_archive {
        archives.push(("macos", macos_archive));
    }
    archives.push(("linux-aarch64", linux_archive));
    archives.push(("linux-x86_64", linux_x86_64_archive));
    archives.push(("windows", windows_archive));
    let mut archive_results = Vec::new();
    let mut reasons = Vec::new();
    if !provenance_verified {
        reasons.push(serde_json::json!({
            "code": "provenance_signature_failed",
            "message": "release provenance signature verification failed"
        }));
    }
    for (platform, archive) in &archives {
        let verified = match verify_release_archive_signature(archive, &provenance_dir) {
            Ok(()) => true,
            Err(error) => {
                reasons.push(serde_json::json!({
                    "code": "archive_signature_failed",
                    "platform": platform,
                    "archive": archive,
                    "message": error.to_string()
                }));
                false
            }
        };
        archive_results.push(serde_json::json!({
            "platform": platform,
            "archive": archive,
            "verified": verified
        }));
    }
    if json_string(&smoke, "status") != "verified" {
        reasons.push(serde_json::json!({
            "code": "smoke_summary_failed",
            "message": "release smoke summary verification failed"
        }));
    }
    if json_string(&obligation_gates, "status") != "verified" {
        reasons.push(serde_json::json!({
            "code": "obligation_gate_metadata_failed",
            "message": "release obligation gate metadata verification failed"
        }));
    }

    let replacement_smoke_gate =
        replacement_smoke_gate.map(|path| release_replacement_smoke_gate_verification_json(&path));
    if replacement_smoke_gate
        .as_ref()
        .map(|report| json_string(report, "status") != "verified")
        .unwrap_or(false)
    {
        reasons.push(serde_json::json!({
            "code": "replacement_smoke_gate_failed",
            "message": "AO2 replacement smoke gate must be accepted across macOS, Ubuntu, and Windows"
        }));
    }

    let greenfield_three_os_smoke_gate = greenfield_three_os_smoke_gate
        .map(|path| release_greenfield_three_os_smoke_gate_verification_json(&path));
    if greenfield_three_os_smoke_gate
        .as_ref()
        .map(|report| json_string(report, "status") != "verified")
        .unwrap_or(false)
    {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_failed",
            "message": "AO2 greenfield three-OS smoke gate must be accepted across macOS, Ubuntu, and Windows"
        }));
    }

    let governed_run_evidence = if governed_run_evidence.is_empty() {
        None
    } else {
        Some(release_governed_run_evidence_verification_json(
            &governed_run_evidence,
        ))
    };
    if governed_run_evidence
        .as_ref()
        .map(|report| json_string(report, "status") != "verified")
        .unwrap_or(false)
    {
        reasons.push(serde_json::json!({
            "code": "governed_run_evidence_failed",
            "message": "AO2 governed run evidence must be accepted across macOS, Ubuntu, and Windows"
        }));
    }

    let factory_project_run_readback = if factory_project_run_summaries.is_empty() {
        None
    } else {
        Some(release_factory_project_run_readback_verification_json(
            &factory_project_run_summaries,
        ))
    };
    if factory_project_run_readback
        .as_ref()
        .map(|report| json_string(report, "status") != "verified")
        .unwrap_or(false)
    {
        reasons.push(serde_json::json!({
            "code": "factory_project_run_readback_failed",
            "message": "AO2 project-run readback must prove accepted replacement-packet handoff across macOS, Ubuntu, and Windows"
        }));
    }

    let obligation_gate_signing = if require_obligation_gate_signing {
        let report = release_obligation_gate_signing_verification_json(
            &summary,
            summary_json.get("obligation_gates"),
        );
        if json_string(&report, "status") != "verified" {
            reasons.push(serde_json::json!({
                "code": "obligation_gate_signing_unverified",
                "message": "every release obligation gate must be AO2-signed and verifiable via the workbench evidence-export wrapper"
            }));
        }
        Some(report)
    } else {
        None
    };

    let status = if reasons.is_empty() {
        "verified"
    } else {
        "failed"
    };
    let mut report = serde_json::json!({
        "schema": "ao2.release-gate.v1",
        "status": status,
        "release": {
            "provenance_dir": provenance_dir,
            "provenance_json": provenance_json,
            "provenance_signature": provenance_signature,
            "public_key": public_key,
            "provenance_verified": provenance_verified,
            "archive_count": archives.len(),
            "archives": archive_results,
        },
        "smoke": smoke,
        "obligation_gates": obligation_gates,
        "reasons": reasons
    });
    if let Some(signing_report) = obligation_gate_signing {
        if let Some(object) = report.as_object_mut() {
            object.insert("obligation_gate_signing".to_string(), signing_report);
        }
    }
    if let Some(replacement_report) = replacement_smoke_gate {
        if let Some(object) = report.as_object_mut() {
            object.insert("replacement_smoke_gate".to_string(), replacement_report);
        }
    }
    if let Some(greenfield_report) = greenfield_three_os_smoke_gate {
        if let Some(object) = report.as_object_mut() {
            object.insert(
                "greenfield_three_os_smoke_gate".to_string(),
                greenfield_report,
            );
        }
    }
    if let Some(governed_run_report) = governed_run_evidence {
        if let Some(object) = report.as_object_mut() {
            object.insert("governed_run_evidence".to_string(), governed_run_report);
        }
    }
    if let Some(readback_report) = factory_project_run_readback {
        if let Some(object) = report.as_object_mut() {
            object.insert("factory_project_run_readback".to_string(), readback_report);
        }
    }
    Ok(report)
}

pub(crate) fn release_factory_project_run_readback_verification_json(
    paths: &[PathBuf],
) -> serde_json::Value {
    let mut duplicate_os = Vec::<String>::new();
    let mut unknown_os = Vec::<String>::new();
    let mut input_errors = Vec::<serde_json::Value>::new();
    let mut observed_os = BTreeSet::<String>::new();
    let mut accepted_os = BTreeSet::<String>::new();
    let mut per_os = Vec::<serde_json::Value>::new();

    for path in paths {
        let summary = match read_factory_compat_value(path) {
            Ok(value) => value,
            Err(error) => {
                input_errors.push(serde_json::json!({
                    "path": path.display().to_string(),
                    "error": error.to_string()
                }));
                continue;
            }
        };
        let Some(os_label) = infer_governed_run_evidence_os(path, Some(&summary)) else {
            unknown_os.push(path.display().to_string());
            input_errors.push(serde_json::json!({
                "path": path.display().to_string(),
                "error": "could not infer project-run readback OS from artifact metadata or path"
            }));
            continue;
        };
        if !observed_os.insert(os_label.to_string()) {
            duplicate_os.push(os_label.to_string());
            input_errors.push(serde_json::json!({
                "path": path.display().to_string(),
                "os": os_label,
                "error": "duplicate project-run readback for OS"
            }));
            continue;
        }

        let mut reasons = Vec::<serde_json::Value>::new();
        validate_factory_project_run_replacement_readback(&summary, &mut reasons);
        let status = if reasons.is_empty() {
            accepted_os.insert(os_label.to_string());
            "accepted"
        } else {
            "rejected"
        };
        per_os.push(serde_json::json!({
            "os": os_label,
            "status": status,
            "path": path.display().to_string(),
            "run_id": json_string(&summary, "run_id"),
            "reasons": reasons
        }));
    }

    let missing_os = FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS
        .iter()
        .filter(|required_os| !observed_os.contains::<str>(*required_os))
        .map(|required_os| required_os.to_string())
        .collect::<Vec<_>>();
    let accepted_os = FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS
        .iter()
        .filter(|required_os| accepted_os.contains::<str>(*required_os))
        .map(|required_os| required_os.to_string())
        .collect::<Vec<_>>();
    let status = if missing_os.is_empty()
        && duplicate_os.is_empty()
        && unknown_os.is_empty()
        && input_errors.is_empty()
        && accepted_os.len() == FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS.len()
        && per_os
            .iter()
            .all(|item| json_string(item, "status") == "accepted")
    {
        "verified"
    } else {
        "failed"
    };
    serde_json::json!({
        "schema": "ao2.release-factory-project-run-readback-verification.v1",
        "status": status,
        "required_os": FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS,
        "accepted_os": accepted_os,
        "missing_os": missing_os,
        "duplicate_os": duplicate_os,
        "unknown_os": unknown_os,
        "input_errors": input_errors,
        "per_os": per_os,
        "reasons": if status == "verified" {
            serde_json::json!([])
        } else {
            serde_json::json!([{
                "code": "factory_project_run_readback_not_verified",
                "message": "project-run readback must include accepted macOS, Ubuntu, and Windows replacement-packet handoff proof"
            }])
        }
    })
}

fn validate_factory_project_run_replacement_readback(
    summary: &serde_json::Value,
    reasons: &mut Vec<serde_json::Value>,
) {
    let mut string_reasons = Vec::<String>::new();
    require_json_eq(
        summary,
        &["schema_version"],
        "ao2.factory-project-run-smoke.v1",
        &mut string_reasons,
    );
    require_json_eq(summary, &["status"], "passed", &mut string_reasons);
    require_json_eq(
        summary,
        &["factory_project_schema"],
        "ao2.factory-project-run.v1",
        &mut string_reasons,
    );
    require_json_eq(
        summary,
        &["queued_auto_replacement_packet_status"],
        "packaged",
        &mut string_reasons,
    );
    require_json_eq(
        summary,
        &["queued_auto_replacement_packet_verification_status"],
        "accepted",
        &mut string_reasons,
    );
    require_json_eq(
        summary,
        &["queued_replacement_packet_schema"],
        "ao2.factory-replacement-packet.v1",
        &mut string_reasons,
    );
    require_json_eq(
        summary,
        &["queued_replacement_packet_status"],
        "packaged",
        &mut string_reasons,
    );
    require_json_eq(
        summary,
        &["queued_replacement_packet_factory_v3_role"],
        "evaluator_closer_and_sampling_auditor",
        &mut string_reasons,
    );
    require_json_eq(
        summary,
        &["queued_replacement_packet_verification_schema"],
        "ao2.factory-replacement-packet-verification.v1",
        &mut string_reasons,
    );
    require_json_eq(
        summary,
        &["queued_replacement_packet_verification_status"],
        "accepted",
        &mut string_reasons,
    );
    for path in [
        &["queued_auto_replacement_packet_verification_checksums_verified"][..],
        &["queued_auto_replacement_packet_verification_trust_boundary_verified"][..],
        &["queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver"][..],
        &["queued_replacement_packet_verification_checksums_verified"][..],
        &["queued_replacement_packet_verification_trust_boundary_verified"][..],
        &["queued_replacement_packet_verification_ao2_replacement_driver_verified"][..],
        &["queued_replacement_packet_verification_factory_v3_evaluator_closer_verified"][..],
    ] {
        require_json_bool(summary, path, true, &mut string_reasons);
    }
    if json_string(summary, "queued_replacement_packet_sha256").len() != 64 {
        string_reasons
            .push("queued_replacement_packet_sha256 must be a 64-character hex digest".to_string());
    }
    for message in string_reasons {
        reasons.push(serde_json::json!({
            "code": "factory_project_run_readback_contract_failed",
            "message": message
        }));
    }
}

pub(crate) fn release_governed_run_evidence_verification_json(
    paths: &[PathBuf],
) -> serde_json::Value {
    let mut duplicate_os = Vec::<String>::new();
    let mut unknown_os = Vec::<String>::new();
    let mut input_errors = Vec::<serde_json::Value>::new();
    let mut observed_os = BTreeSet::<String>::new();
    let mut accepted_os = BTreeSet::<String>::new();
    let mut per_os = Vec::<serde_json::Value>::new();

    for path in paths {
        let Some(os_label) = infer_governed_run_evidence_os(path, None) else {
            unknown_os.push(path.display().to_string());
            input_errors.push(serde_json::json!({
                "path": path.display().to_string(),
                "error": "could not infer governed run OS from artifact metadata or path"
            }));
            continue;
        };
        if !observed_os.insert(os_label.to_string()) {
            duplicate_os.push(os_label.to_string());
            input_errors.push(serde_json::json!({
                "path": path.display().to_string(),
                "os": os_label,
                "error": "duplicate governed run evidence for OS"
            }));
            continue;
        }
        let mut reasons = Vec::<serde_json::Value>::new();
        let evidence = match read_factory_compat_value(path) {
            Ok(value) => {
                if let Some(metadata_os) = infer_governed_run_evidence_os(path, Some(&value)) {
                    if metadata_os != os_label {
                        reasons.push(serde_json::json!({
                            "code": "governed_run_evidence_os_mismatch",
                            "message": "artifact OS metadata conflicts with path-derived OS",
                            "metadata_os": metadata_os,
                            "path_os": os_label
                        }));
                    }
                }
                value
            }
            Err(error) => {
                reasons.push(serde_json::json!({
                    "code": "governed_run_evidence_unreadable",
                    "message": error.to_string()
                }));
                serde_json::json!({})
            }
        };
        validate_release_governed_run_evidence(&evidence, &mut reasons);
        let status = if reasons.is_empty() {
            accepted_os.insert(os_label.to_string());
            "accepted"
        } else {
            "rejected"
        };
        per_os.push(serde_json::json!({
            "os": os_label,
            "status": status,
            "path": path.display().to_string(),
            "run_id": json_string(&evidence, "run_id"),
            "reasons": reasons
        }));
    }

    let missing_os = FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS
        .iter()
        .filter(|required_os| !observed_os.contains::<str>(*required_os))
        .map(|required_os| required_os.to_string())
        .collect::<Vec<_>>();
    let accepted_os = FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS
        .iter()
        .filter(|required_os| accepted_os.contains::<str>(*required_os))
        .map(|required_os| required_os.to_string())
        .collect::<Vec<_>>();
    let status = if missing_os.is_empty()
        && duplicate_os.is_empty()
        && unknown_os.is_empty()
        && input_errors.is_empty()
        && accepted_os.len() == FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS.len()
        && per_os
            .iter()
            .all(|item| json_string(item, "status") == "accepted")
    {
        "verified"
    } else {
        "failed"
    };
    serde_json::json!({
        "schema": "ao2.release-governed-run-evidence-verification.v1",
        "status": status,
        "required_os": FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS,
        "accepted_os": accepted_os,
        "missing_os": missing_os,
        "duplicate_os": duplicate_os,
        "unknown_os": unknown_os,
        "input_errors": input_errors,
        "per_os": per_os,
        "artifact_path_policy": "content-based gate; referenced paths inside governed-run artifacts are not required to exist on this machine",
        "reasons": if status == "verified" {
            serde_json::json!([])
        } else {
            serde_json::json!([{
                "code": "governed_run_evidence_not_verified",
                "message": "governed run evidence must include accepted macOS, Ubuntu, and Windows AO2-owned runs"
            }])
        }
    })
}

fn infer_governed_run_evidence_os(
    path: &Path,
    evidence: Option<&serde_json::Value>,
) -> Option<&'static str> {
    if let Some(evidence) = evidence {
        for key in ["os", "platform", "host_os"] {
            let label = json_string(evidence, key);
            if let Some(normalized) = normalize_factory_replacement_smoke_os(label.trim()) {
                return Some(normalized);
            }
        }
    }
    for component in path.components() {
        let label = component.as_os_str().to_string_lossy();
        if let Some(normalized) = normalize_factory_replacement_smoke_os(label.trim()) {
            return Some(normalized);
        }
    }
    let path_text = path.display().to_string().to_ascii_lowercase();
    if path_text.contains("windows") {
        Some("windows")
    } else if path_text.contains("ubuntu") {
        Some("ubuntu")
    } else if path_text.contains("macos") || path_text.contains("darwin") {
        Some("macos")
    } else {
        None
    }
}

fn validate_release_governed_run_evidence(
    evidence: &serde_json::Value,
    reasons: &mut Vec<serde_json::Value>,
) {
    let mut string_reasons = Vec::<String>::new();
    require_json_eq(
        evidence,
        &["schema_version"],
        "ao2.factory-v3-compat-governed-run.v1",
        &mut string_reasons,
    );
    require_json_eq(evidence, &["status"], "accepted", &mut string_reasons);
    require_json_eq(
        evidence,
        &["factory_v3_role"],
        "parity_oracle_only",
        &mut string_reasons,
    );
    require_json_eq(
        evidence,
        &["ao2_decision_owner"],
        "ao2-native-governed-run",
        &mut string_reasons,
    );
    require_json_eq(
        evidence,
        &["control_plane_role"],
        "read_only_observer_after_signed_evidence",
        &mut string_reasons,
    );
    require_json_eq(
        evidence,
        &["run_result_verification", "status"],
        "accepted",
        &mut string_reasons,
    );
    require_json_eq(
        evidence,
        &["pack_evidence", "status"],
        "produced",
        &mut string_reasons,
    );
    require_json_eq(
        evidence,
        &["evaluator_decision", "verdict"],
        "accepted",
        &mut string_reasons,
    );
    require_json_eq(
        evidence,
        &["evaluator_decision_verification", "status"],
        "accepted",
        &mut string_reasons,
    );
    require_json_eq(
        evidence,
        &["plan", "ao2_native_plan", "role_contract_discovery", "mode"],
        "auto_discovered_from_ao_runspec_layout",
        &mut string_reasons,
    );
    for path in [
        &["pack_evidence", "signature", "signature_verified"][..],
        &["evaluator_decision", "signature", "signature_verified"][..],
        &["evaluator_decision_verification", "signature_verified"][..],
        &[
            "governed_run_checklist",
            "ao2_planned_factory_compat_workflow",
        ][..],
        &[
            "governed_run_checklist",
            "ao2_queue_executed_factory_compat_workflow",
        ][..],
        &["governed_run_checklist", "ao2_verified_primary_run_result"][..],
        &["governed_run_checklist", "ao2_packed_primary_evidence"][..],
        &["governed_run_checklist", "ao2_signed_evaluator_closure"][..],
        &["governed_run_checklist", "ao2_auto_loaded_role_contracts"][..],
    ] {
        require_json_bool(evidence, path, true, &mut string_reasons);
    }
    require_json_bool(
        evidence,
        &["governed_run_checklist", "factory_v3_drives_workflow"],
        false,
        &mut string_reasons,
    );
    let loaded_count = json_path(
        evidence,
        &[
            "plan",
            "ao2_native_plan",
            "role_contract_discovery",
            "loaded_count",
        ],
    )
    .and_then(serde_json::Value::as_u64)
    .unwrap_or_default();
    if loaded_count == 0 {
        string_reasons.push(
            "plan.ao2_native_plan.role_contract_discovery.loaded_count must be greater than 0"
                .to_string(),
        );
    }
    for message in string_reasons {
        reasons.push(serde_json::json!({
            "code": "governed_run_evidence_contract_failed",
            "message": message
        }));
    }
}

pub(crate) fn release_replacement_smoke_gate_verification_json(path: &Path) -> serde_json::Value {
    let mut reasons = Vec::new();
    let gate = match read_factory_compat_value(path) {
        Ok(value) => value,
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "replacement_smoke_gate_unreadable",
                "message": error.to_string(),
            }));
            serde_json::json!({})
        }
    };
    if gate["schema_version"] != "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1" {
        reasons.push(serde_json::json!({
            "code": "replacement_smoke_gate_invalid_schema",
            "message": "replacement smoke gate schema_version must be ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1"
        }));
    }
    if gate["status"] != "accepted" {
        reasons.push(serde_json::json!({
            "code": "replacement_smoke_gate_not_accepted",
            "message": "replacement smoke gate status must be accepted",
            "gate_status": json_string(&gate, "status"),
        }));
    }
    for required_os in FACTORY_REPLACEMENT_SMOKE_REQUIRED_OS {
        let accepted = json_array(&gate, "accepted_os")
            .iter()
            .any(|value| value.as_str() == Some(required_os));
        if !accepted {
            reasons.push(serde_json::json!({
                "code": "replacement_smoke_gate_missing_os",
                "os": required_os,
                "message": "replacement smoke gate accepted_os must include macOS, Ubuntu, and Windows"
            }));
        }
    }
    for list_key in ["missing_os", "duplicate_os", "unknown_os", "input_errors"] {
        if !json_array(&gate, list_key).is_empty() {
            reasons.push(serde_json::json!({
                "code": "replacement_smoke_gate_has_rejected_inputs",
                "field": list_key,
                "message": "replacement smoke gate must have no missing, duplicate, unknown, or parse-error inputs"
            }));
        }
    }
    if gate["factory_v3_role"] != "parity_oracle_only" {
        reasons.push(serde_json::json!({
            "code": "replacement_smoke_gate_factory_role_invalid",
            "message": "factory_v3_role must be parity_oracle_only"
        }));
    }
    if gate["ao2_decision_owner"] != "ao2-native-three-os-replacement-smoke-gate" {
        reasons.push(serde_json::json!({
            "code": "replacement_smoke_gate_owner_invalid",
            "message": "ao2_decision_owner must be ao2-native-three-os-replacement-smoke-gate"
        }));
    }
    if gate["control_plane_role"] != "read_only_observer_after_signed_evidence" {
        reasons.push(serde_json::json!({
            "code": "replacement_smoke_gate_control_plane_role_invalid",
            "message": "control_plane_role must be read_only_observer_after_signed_evidence"
        }));
    }
    if gate["three_os_contract"]["path_separator_safe_artifacts"] != true {
        reasons.push(serde_json::json!({
            "code": "replacement_smoke_gate_path_contract_failed",
            "message": "three_os_contract.path_separator_safe_artifacts must be true"
        }));
    }
    serde_json::json!({
        "schema": "ao2.release-replacement-smoke-gate-verification.v1",
        "status": if reasons.is_empty() { "verified" } else { "failed" },
        "path": path,
        "gate_status": json_string(&gate, "status"),
        "accepted_os": gate.get("accepted_os").cloned().unwrap_or_else(|| serde_json::json!([])),
        "reasons": reasons,
    })
}

pub(crate) fn release_greenfield_three_os_smoke_gate_verification_json(
    path: &Path,
) -> serde_json::Value {
    let mut reasons = Vec::new();
    let gate = match read_factory_compat_value(path) {
        Ok(value) => value,
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "greenfield_three_os_smoke_gate_unreadable",
                "message": error.to_string(),
            }));
            serde_json::json!({})
        }
    };
    if gate["schema_version"] != "ao2.greenfield-three-os-smoke-gate.v1" {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_invalid_schema",
            "message": "greenfield three-OS smoke gate schema_version must be ao2.greenfield-three-os-smoke-gate.v1"
        }));
    }
    if gate["status"] != "accepted" {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_not_accepted",
            "message": "greenfield three-OS smoke gate status must be accepted",
            "gate_status": json_string(&gate, "status"),
        }));
    }
    for required_os in GREENFIELD_THREE_OS_REQUIRED_OS {
        let accepted = json_array(&gate, "accepted_os")
            .iter()
            .any(|value| value.as_str() == Some(required_os));
        if !accepted {
            reasons.push(serde_json::json!({
                "code": "greenfield_three_os_smoke_gate_missing_os",
                "os": required_os,
                "message": "greenfield three-OS smoke gate accepted_os must include macOS, Ubuntu, and Windows"
            }));
        }
    }
    for list_key in ["missing_os", "duplicate_os", "unknown_os", "input_errors"] {
        if !json_array(&gate, list_key).is_empty() {
            reasons.push(serde_json::json!({
                "code": "greenfield_three_os_smoke_gate_has_rejected_inputs",
                "field": list_key,
                "message": "greenfield three-OS smoke gate must have no missing, duplicate, unknown, or parse-error inputs"
            }));
        }
    }
    if gate["factory_v3_role"] != "parity_oracle_only" {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_factory_role_invalid",
            "message": "factory_v3_role must be parity_oracle_only"
        }));
    }
    if gate["ao2_decision_owner"] != "ao2-native-greenfield-three-os-smoke-gate" {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_owner_invalid",
            "message": "ao2_decision_owner must be ao2-native-greenfield-three-os-smoke-gate"
        }));
    }
    if gate["control_plane_role"] != "read_only_observer_after_signed_evidence" {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_control_plane_role_invalid",
            "message": "control_plane_role must be read_only_observer_after_signed_evidence"
        }));
    }
    if gate["three_os_contract"]["path_separator_safe_artifacts"] != true {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_path_contract_failed",
            "message": "three_os_contract.path_separator_safe_artifacts must be true"
        }));
    }
    if gate["trust_boundary"]["execution_owner"] != "ao2" {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_execution_owner_invalid",
            "message": "trust_boundary.execution_owner must be ao2"
        }));
    }
    if gate["trust_boundary"]["release_acceptance_owner"] != "factory-v3 evaluator-closer" {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_release_owner_invalid",
            "message": "trust_boundary.release_acceptance_owner must be factory-v3 evaluator-closer"
        }));
    }
    if gate["trust_boundary"]["control_plane_approves_release"] != false {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_control_plane_approval_invalid",
            "message": "trust_boundary.control_plane_approves_release must be false"
        }));
    }
    if gate["trust_boundary"]["mutates_ao_artifacts"] != false {
        reasons.push(serde_json::json!({
            "code": "greenfield_three_os_smoke_gate_mutation_boundary_invalid",
            "message": "trust_boundary.mutates_ao_artifacts must be false"
        }));
    }
    serde_json::json!({
        "schema": "ao2.release-greenfield-three-os-smoke-gate-verification.v1",
        "status": if reasons.is_empty() { "verified" } else { "failed" },
        "path": path,
        "gate_status": json_string(&gate, "status"),
        "accepted_os": gate.get("accepted_os").cloned().unwrap_or_else(|| serde_json::json!([])),
        "reasons": reasons,
    })
}

fn release_obligation_gate_signing_verification_json(
    summary_path: &Path,
    obligation_gates: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut per_gate = Vec::new();
    let mut reasons = Vec::new();
    let gates = obligation_gates
        .and_then(|value| value.get("gates"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if gates.is_empty() {
        reasons.push(serde_json::json!({
            "code": "obligation_gate_signing_no_gates",
            "message": "release summary contains no obligation gates to audit for signing"
        }));
    }
    for gate in gates {
        let stage = json_string(&gate, "stage");
        let path_str = json_string(&gate, "path");
        if path_str.is_empty() {
            reasons.push(serde_json::json!({
                "code": "obligation_gate_path_missing",
                "stage": stage,
                "message": "release summary obligation gate is missing the on-disk path required to audit signing"
            }));
            per_gate.push(serde_json::json!({
                "stage": stage,
                "path": null,
                "signing_status": "path-missing",
                "signature_verified": false,
                "ao2_owned": false,
                "error": "obligation gate summary entry has no `path` field"
            }));
            continue;
        }
        let path = resolve_summary_sidecar_path(summary_path, &path_str);
        match contract_verify_obligation_gate_signing_json(&path, None, None) {
            Ok(audit) => {
                let signing_status = json_string(&audit, "signing_status");
                if signing_status != "signed-and-verified" {
                    reasons.push(serde_json::json!({
                        "code": "obligation_gate_signing_not_verified",
                        "stage": stage,
                        "path": path,
                        "signing_status": signing_status,
                        "message": "obligation gate must report `signed-and-verified` from contract_verify_obligation_gate_signing"
                    }));
                }
                per_gate.push(serde_json::json!({
                    "stage": stage,
                    "path": path,
                    "signing_status": signing_status,
                    "signature_verified": audit.get("signature_verified").cloned().unwrap_or(serde_json::Value::Bool(false)),
                    "ao2_owned": audit.get("ao2_owned").cloned().unwrap_or(serde_json::Value::Bool(false)),
                    "matched_wrapper_path": audit.get("matched_wrapper_path").cloned().unwrap_or(serde_json::Value::Null),
                    "audit": audit
                }));
            }
            Err(error) => {
                reasons.push(serde_json::json!({
                    "code": "obligation_gate_signing_check_errored",
                    "stage": stage,
                    "path": path,
                    "message": error.to_string()
                }));
                per_gate.push(serde_json::json!({
                    "stage": stage,
                    "path": path,
                    "signing_status": "check-errored",
                    "signature_verified": false,
                    "ao2_owned": false,
                    "error": error.to_string()
                }));
            }
        }
    }
    serde_json::json!({
        "schema": "ao2.release-obligation-gate-signing-verification.v1",
        "status": if reasons.is_empty() { "verified" } else { "failed" },
        "per_gate": per_gate,
        "reasons": reasons,
        "ao2_decision_owner": "ao2-native-obligation-gate-signing-auditor"
    })
}
