use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_core::{
    annotate_obligation_ledger, check_obligation_ledger,
    obligation_evidence_points_to_existing_line, sha256_hex, ObligationEvidence, ObligationLedger,
    ObligationStatus,
};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{atomic_write_text, json_string, now_unix_ms, run_dir, sha256_file};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};
use crate::workbench_contract::{WorkbenchOperator, WorkbenchSupportSigning};
use crate::workbench_evidence_delivery::workbench_evidence_export_path;
use crate::workbench_queue::append_workbench_audit_event_for_target;

fn workbench_obligation_ledger_path(target: &Path, run_id: &str) -> Result<PathBuf> {
    let run_dir = run_dir(target, run_id);
    let candidates = [
        run_dir.join("evidence-pack").join("obligation-ledger.json"),
        run_dir.join("obligation-ledger.json"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| format!("obligation ledger not found for run {run_id}"))
}

pub(super) fn workbench_obligation_annotation_json(
    target: &Path,
    form: &std::collections::BTreeMap<String, String>,
    operator: &WorkbenchOperator,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let generated_at_ms = now_unix_ms();
    let run_id = form
        .get("run_id")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .context("run_id is required")?;
    let obligation_id = form
        .get("obligation_id")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .context("obligation_id is required")?;
    let evidence_path = form
        .get("evidence_path")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string());
    let evidence = match evidence_path {
        Some(path) => {
            let line = form
                .get("evidence_line")
                .filter(|value| !value.trim().is_empty())
                .context("evidence_line is required with evidence_path")?
                .trim()
                .parse::<usize>()
                .context("evidence_line must be a positive integer")?;
            if line == 0 {
                return Err(anyhow!("evidence_line must be greater than 0"));
            }
            let detail = form
                .get("detail")
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_string())
                .unwrap_or_else(|| "manual operator evidence".to_string());
            Some(ObligationEvidence { path, line, detail })
        }
        None => None,
    };
    if let Some(evidence) = evidence.as_ref() {
        if !obligation_evidence_points_to_existing_line(target, evidence) {
            return Err(anyhow!(
                "obligation evidence must reference an existing target-relative file and line: {}:{}",
                evidence.path,
                evidence.line
            ));
        }
    }
    let waiver = form
        .get("waiver")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string());
    let ledger_path = workbench_obligation_ledger_path(target, &run_id)?;
    let ledger_text = fs::read_to_string(&ledger_path)
        .with_context(|| format!("read {}", ledger_path.display()))?;
    let before_ledger_sha256 = sha256_hex(ledger_text.as_bytes());
    let ledger: ObligationLedger = serde_json::from_str(&ledger_text)
        .with_context(|| format!("parse {}", ledger_path.display()))?;
    let annotated = annotate_obligation_ledger(&ledger, &obligation_id, evidence, waiver)
        .map_err(|error| anyhow!(error))?;
    let annotated = check_obligation_ledger(&annotated, target).with_context(|| {
        format!(
            "validate annotated obligation evidence under {}",
            target.display()
        )
    })?;
    atomic_write_text(&ledger_path, &serde_json::to_string_pretty(&annotated)?)?;
    let after_ledger_sha256 = sha256_file(&ledger_path)?;
    let obligation = annotated
        .obligations
        .iter()
        .find(|obligation| obligation.id == obligation_id)
        .cloned()
        .with_context(|| format!("annotated obligation disappeared: {obligation_id}"))?;
    let audit_event = serde_json::json!({
        "schema_version": "ao2.workbench-audit-event.v1",
        "timestamp_ms": generated_at_ms,
        "action": "obligation_annotate",
        "operator_id": operator.id,
        "operator_role": operator.role.as_str(),
        "run_id": run_id,
        "obligation_id": obligation_id,
        "ledger_path": ledger_path,
        "before_ledger_sha256": before_ledger_sha256,
        "after_ledger_sha256": after_ledger_sha256,
        "status": serde_json::to_value(obligation.status)?,
        "verdict": annotated.verdict,
        "evidence_count": obligation.evidence.len(),
        "waiver_present": obligation.waiver.is_some()
    });
    let evidence_export = workbench_obligation_annotation_evidence_export_json(
        target,
        generated_at_ms,
        &audit_event,
        &annotated,
        support_signing,
    )?;
    let mut audit_event = audit_event;
    if let Some(object) = audit_event.as_object_mut() {
        object.insert(
            "evidence_export_path".to_string(),
            evidence_export["export_path"].clone(),
        );
        object.insert(
            "evidence_export_sha256".to_string(),
            evidence_export["sha256"].clone(),
        );
        object.insert(
            "evidence_signature_verified".to_string(),
            evidence_export["signature"]["signature_verified"].clone(),
        );
    }
    append_workbench_audit_event_for_target(target, audit_event.clone())?;

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-obligation-annotation.v1",
        "run_id": run_id,
        "obligation_id": obligation_id,
        "ledger_path": ledger_path.display().to_string(),
        "audit_event": audit_event,
        "evidence_export": evidence_export,
        "ledger": annotated
    }))
}

pub(super) fn workbench_obligation_gate_json(
    target: &Path,
    form: &std::collections::BTreeMap<String, String>,
    operator: &WorkbenchOperator,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let generated_at_ms = now_unix_ms();
    let run_id = form
        .get("run_id")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .context("run_id is required")?;
    let stage = form
        .get("stage")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .context("stage is required")?;
    // Slice 18: producer-side default-on signing for the workbench HTTP
    // surface, mirroring the CLI flip in `ao2 contract gate`. When the
    // workbench was started without `--support-signing-key`, refuse to
    // produce an unsigned obligation gate unless the operator explicitly
    // opts out via the `allow_unsigned_obligation_gates` form param.
    let allow_unsigned_obligation_gates = form
        .get("allow_unsigned_obligation_gates")
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    if support_signing.is_none() && !allow_unsigned_obligation_gates {
        return Err(anyhow!(
            "POST /api/obligations/gate requires the workbench to be started with \
             --support-signing-key by default (slice 18 producer-side default-on, \
             mirroring slice 17 release-gate HTTP default-on); pass \
             allow_unsigned_obligation_gates=1 to opt out, but downstream \
             /api/release-gate will still reject the unsigned gate unless its own \
             escape valve is also set"
        ));
    }
    let stage_slug = obligation_gate_stage_slug(&stage)?;
    let ledger_path = workbench_obligation_ledger_path(target, &run_id)?;
    let ledger_text = fs::read_to_string(&ledger_path)
        .with_context(|| format!("read {}", ledger_path.display()))?;
    let ledger_sha256 = sha256_hex(ledger_text.as_bytes());
    let ledger: ObligationLedger = serde_json::from_str(&ledger_text)
        .with_context(|| format!("parse {}", ledger_path.display()))?;
    let checked = check_obligation_ledger(&ledger, target)
        .with_context(|| format!("gate obligations under {}", target.display()))?;
    let failed_obligations = checked
        .obligations
        .iter()
        .filter(|obligation| obligation.status == ObligationStatus::Fail)
        .cloned()
        .collect::<Vec<_>>();
    let unverified_obligations = checked
        .obligations
        .iter()
        .filter(|obligation| obligation.status == ObligationStatus::Unverified)
        .cloned()
        .collect::<Vec<_>>();
    let status = if checked.verdict == ao2_core::ObligationVerdict::Accepted {
        "passed"
    } else {
        "failed"
    };
    let gate_path = ledger_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("obligation-gate-{stage_slug}.json"));
    let gate = serde_json::json!({
        "schema_version": "ao2.obligation-gate.v1",
        "stage": stage,
        "status": status,
        "verdict": checked.verdict,
        "summary": checked.summary,
        "ledger_path": ledger_path,
        "target": target,
        "gate_path": gate_path,
        "checked_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "failed_obligations": failed_obligations,
        "unverified_obligations": unverified_obligations,
        "checked_ledger": checked
    });
    atomic_write_text(&gate_path, &serde_json::to_string_pretty(&gate)?)?;
    let gate_sha256 = sha256_file(&gate_path)?;
    let audit_event = serde_json::json!({
        "schema_version": "ao2.workbench-audit-event.v1",
        "timestamp_ms": generated_at_ms,
        "action": "obligation_gate",
        "operator_id": operator.id,
        "operator_role": operator.role.as_str(),
        "run_id": run_id,
        "stage": json_string(&gate, "stage"),
        "status": json_string(&gate, "status"),
        "verdict": gate["verdict"],
        "summary": gate["summary"],
        "ledger_path": ledger_path,
        "ledger_sha256": ledger_sha256,
        "gate_path": gate_path,
        "gate_sha256": gate_sha256
    });
    let evidence_export = workbench_obligation_gate_evidence_export_json(
        target,
        generated_at_ms,
        &audit_event,
        &gate,
        support_signing,
    )?;
    let mut audit_event = audit_event;
    if let Some(object) = audit_event.as_object_mut() {
        object.insert(
            "evidence_export_path".to_string(),
            evidence_export["export_path"].clone(),
        );
        object.insert(
            "evidence_export_sha256".to_string(),
            evidence_export["sha256"].clone(),
        );
        object.insert(
            "evidence_signature_verified".to_string(),
            evidence_export["signature"]["signature_verified"].clone(),
        );
    }
    append_workbench_audit_event_for_target(target, audit_event.clone())?;

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-obligation-gate.v1",
        "run_id": run_id,
        "stage": json_string(&gate, "stage"),
        "gate_path": json_string(&gate, "gate_path"),
        "gate_sha256": gate_sha256,
        "audit_event": audit_event,
        "evidence_export": evidence_export,
        "gate": gate
    }))
}

fn obligation_gate_stage_slug(stage: &str) -> Result<String> {
    let slug = stage
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        return Err(anyhow!("stage must contain an alphanumeric character"));
    }
    Ok(slug)
}

fn workbench_obligation_gate_evidence_export_json(
    target: &Path,
    generated_at_ms: u64,
    audit_event: &serde_json::Value,
    gate: &serde_json::Value,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let export_path = workbench_evidence_export_path(target, generated_at_ms, "obligation-gate");
    let export = serde_json::json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": "obligation-gate",
        "target": target,
        "export": {
            "gate": gate,
            "audit_event": audit_event
        }
    });
    atomic_write_text(&export_path, &serde_json::to_string_pretty(&export)?)?;
    let export_sha256 = sha256_file(&export_path)?;
    let signature = match support_signing {
        Some(signing) => {
            let signature_path = export_path.with_extension("json.sig");
            let public_key_path = export_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("workbench-evidence-signing-public.pem");
            derive_public_key_from_private_key(&signing.key_path, &public_key_path)?;
            sign_file_with_private_key(&signing.key_path, &export_path, &signature_path)?;
            let signature_verified =
                verify_file_signature(&export_path, &signature_path, &public_key_path)?;
            serde_json::json!({
                "present": true,
                "signature_verified": signature_verified,
                "signer_id": signing.signer_id,
                "signature_algorithm": "RSA/SHA-256",
                "signature_path": signature_path,
                "public_key_path": public_key_path,
                "public_key_sha256": sha256_file(&public_key_path)?
            })
        }
        None => serde_json::json!({
            "present": false,
            "signature_verified": false
        }),
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": "obligation-gate",
        "export_path": export_path,
        "sha256": export_sha256,
        "signature": signature,
        "export": {
            "gate": gate,
            "audit_event": audit_event
        }
    }))
}

fn workbench_obligation_annotation_evidence_export_json(
    target: &Path,
    generated_at_ms: u64,
    audit_event: &serde_json::Value,
    ledger: &ObligationLedger,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let export_path =
        workbench_evidence_export_path(target, generated_at_ms, "obligation-annotation");
    let export = serde_json::json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": "obligation-annotation",
        "target": target,
        "export": {
            "annotation": audit_event,
            "ledger": ledger
        }
    });
    atomic_write_text(&export_path, &serde_json::to_string_pretty(&export)?)?;
    let export_sha256 = sha256_file(&export_path)?;
    let signature = match support_signing {
        Some(signing) => {
            let signature_path = export_path.with_extension("json.sig");
            let public_key_path = export_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("workbench-evidence-signing-public.pem");
            derive_public_key_from_private_key(&signing.key_path, &public_key_path)?;
            sign_file_with_private_key(&signing.key_path, &export_path, &signature_path)?;
            let signature_verified =
                verify_file_signature(&export_path, &signature_path, &public_key_path)?;
            serde_json::json!({
                "present": true,
                "signature_verified": signature_verified,
                "signer_id": signing.signer_id,
                "signature_algorithm": "RSA/SHA-256",
                "signature_path": signature_path,
                "public_key_path": public_key_path,
                "public_key_sha256": sha256_file(&public_key_path)?
            })
        }
        None => serde_json::json!({
            "present": false,
            "signature_verified": false
        }),
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": "obligation-annotation",
        "export_path": export_path,
        "sha256": export_sha256,
        "signature": signature,
        "export": {
            "annotation": audit_event,
            "ledger": ledger
        }
    }))
}
