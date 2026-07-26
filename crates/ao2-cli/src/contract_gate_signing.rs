use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::cli_util::{atomic_write_text, json_string, now_unix_ms, sha256_file};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};

/// Emit an `ao2.workbench-evidence-export.v1` wrapper alongside a raw
/// obligation gate, signed with the supplied RSA private key. Mirrors the
/// shape `workbench_obligation_gate_evidence_export_json` produces from the
/// workbench HTTP serve path, but exposes the same surface as a CLI flag so
/// non-workbench producers (the factory-v3 nightly script, etc.) can emit
/// AO2-signed gates without spinning up a workbench server. The wrapper +
/// `.json.sig` + `workbench-evidence-signing-public.pem` land in
/// `exports_dir`; downstream `ao2 contract verify-obligation-gate-signing`
/// finds them via the gate-parent-dir fallback (or the explicit
/// `--evidence-exports-dir` flag).
pub(crate) fn emit_contract_gate_signed_wrapper(
    gate: &serde_json::Value,
    exports_dir: &Path,
    private_key: &Path,
    signer_id: &str,
    operator_role: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    fs::create_dir_all(exports_dir)
        .with_context(|| format!("create exports dir {}", exports_dir.display()))?;
    let generated_at_ms = now_unix_ms();
    let audit_event = serde_json::json!({
        "schema_version": "ao2.workbench-audit-event.v1",
        "timestamp_ms": generated_at_ms,
        "action": "obligation_gate",
        "operator_id": signer_id,
        "operator_role": operator_role,
        "run_id": run_id,
        "stage": gate.get("stage").cloned().unwrap_or(serde_json::Value::Null),
        "status": gate.get("status").cloned().unwrap_or(serde_json::Value::Null),
        "verdict": gate.get("verdict").cloned().unwrap_or(serde_json::Value::Null),
        "summary": gate.get("summary").cloned().unwrap_or(serde_json::Value::Null),
        "ledger_path": gate.get("ledger_path").cloned().unwrap_or(serde_json::Value::Null),
        "gate_path": gate.get("gate_path").cloned().unwrap_or(serde_json::Value::Null)
    });
    let wrapper = serde_json::json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": "obligation-gate",
        "target": gate.get("target").cloned().unwrap_or(serde_json::Value::Null),
        "export": {
            "gate": gate,
            "audit_event": audit_event
        }
    });
    let wrapper_path = exports_dir.join(format!(
        "evidence-export-{generated_at_ms}-obligation-gate.json"
    ));
    atomic_write_text(&wrapper_path, &serde_json::to_string_pretty(&wrapper)?)?;
    let signature_path = wrapper_path.with_extension("json.sig");
    let public_key_path = exports_dir.join("workbench-evidence-signing-public.pem");
    derive_public_key_from_private_key(private_key, &public_key_path)?;
    sign_file_with_private_key(private_key, &wrapper_path, &signature_path)?;
    let signature_verified =
        verify_file_signature(&wrapper_path, &signature_path, &public_key_path)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.contract-gate-support-signing-evidence.v1",
        "generated_at_ms": generated_at_ms,
        "exports_dir": exports_dir.display().to_string(),
        "wrapper_path": wrapper_path.display().to_string(),
        "wrapper_sha256": sha256_file(&wrapper_path)?,
        "signature_path": signature_path.display().to_string(),
        "public_key_path": public_key_path.display().to_string(),
        "public_key_sha256": sha256_file(&public_key_path)?,
        "signer_id": signer_id,
        "signer_role": operator_role,
        "signer_run_id": run_id,
        "signature_algorithm": "RSA/SHA-256",
        "signature_verified": signature_verified
    }))
}

/// Audit signing status for a single raw `obligation-gate-<stage>.json`.
///
/// Closure verdicts and release verification consume raw obligation gate
/// files. AO2 is the only producer that ever signs these gates, via the
/// workbench evidence-export wrapper (`ao2.workbench-evidence-export.v1`)
/// with a sidecar `.json.sig` and a directory-shared
/// `workbench-evidence-signing-public.pem`. This function walks the
/// `evidence_exports_dir` looking for a wrapper whose embedded `export.gate`
/// equals the supplied raw gate, then verifies the wrapper's RSA/SHA-256
/// signature. The verdict surface lets observers (CI, release gates,
/// control-plane displays) detect gates that were never signed (the
/// `ao2 contract gate` path produces unsigned gates by default) and
/// confirm that any signed gate is AO2-owned (the wrapper carries an
/// `audit_event.operator_role` that this audit checks against the AO2
/// workbench-operator role set).
pub(crate) fn contract_verify_obligation_gate_signing_json(
    gate_path: &Path,
    evidence_exports_dir: Option<&Path>,
    public_key_override: Option<&Path>,
) -> Result<serde_json::Value> {
    let raw_gate_text = fs::read_to_string(gate_path)
        .with_context(|| format!("read obligation gate {}", gate_path.display()))?;
    let raw_gate: serde_json::Value = serde_json::from_str(&raw_gate_text)
        .with_context(|| format!("parse obligation gate {}", gate_path.display()))?;
    if json_string(&raw_gate, "schema_version") != "ao2.obligation-gate.v1" {
        return Err(anyhow!(
            "contract verify-obligation-gate-signing requires ao2.obligation-gate.v1: {}",
            gate_path.display()
        ));
    }
    let raw_gate_sha = sha256_file(gate_path)?;
    let stage = json_string(&raw_gate, "stage");
    let gate_target_field = json_string(&raw_gate, "target");

    let default_exports_dir = gate_path
        .parent()
        .and_then(|parent| parent.parent())
        .and_then(|run_dir| run_dir.parent())
        .and_then(|runs_dir| runs_dir.parent())
        .and_then(|ao2_dir| ao2_dir.parent())
        .map(|target_value| {
            target_value
                .join(".ao2")
                .join("workbench")
                .join("evidence-exports")
        });
    let target_field_dir = if gate_target_field.is_empty() {
        None
    } else {
        Some(
            PathBuf::from(&gate_target_field)
                .join(".ao2")
                .join("workbench")
                .join("evidence-exports"),
        )
    };
    // The CLI-emitted signed wrapper (from `ao2 contract gate
    // --support-signing-key`) lands next to the raw gate by default, so the
    // verifier walks the gate's parent dir as an additional fallback. Walking
    // all candidates in priority order lets the first dir containing a
    // matching wrapper win, instead of stopping at the first dir that merely
    // EXISTS.
    let gate_parent_dir = gate_path.parent().map(Path::to_path_buf);
    let candidate_dirs: Vec<PathBuf> = [
        evidence_exports_dir.map(Path::to_path_buf),
        target_field_dir.clone(),
        default_exports_dir.clone(),
        gate_parent_dir.clone(),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mut matched_wrapper_path: Option<PathBuf> = None;
    let mut matched_export: Option<serde_json::Value> = None;
    let mut matched_in_dir: Option<PathBuf> = None;
    'outer: for dir in &candidate_dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with("evidence-export-") || !name.ends_with("-obligation-gate.json") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if json_string(&wrapper, "schema_version") != "ao2.workbench-evidence-export.v1" {
                continue;
            }
            if json_string(&wrapper, "export_kind") != "obligation-gate" {
                continue;
            }
            let embedded_gate = &wrapper["export"]["gate"];
            if embedded_gate == &raw_gate {
                matched_wrapper_path = Some(path);
                matched_export = Some(wrapper);
                matched_in_dir = Some(dir.clone());
                break 'outer;
            }
        }
    }

    // Preserve the historical `evidence_exports_dir` audit field: when the
    // wrapper is found, report the dir it was found in; otherwise fall back
    // to the first probed candidate (matching prior single-dir behavior).
    let exports_dir = matched_in_dir.clone().or_else(|| {
        evidence_exports_dir
            .map(Path::to_path_buf)
            .or(target_field_dir)
            .or(default_exports_dir)
    });

    let default_public_key_path = matched_wrapper_path
        .as_ref()
        .and_then(|path| path.parent())
        .map(|dir| dir.join("workbench-evidence-signing-public.pem"));
    let public_key_path = public_key_override
        .map(PathBuf::from)
        .or(default_public_key_path);
    let signature_path = matched_wrapper_path
        .as_ref()
        .map(|path| path.with_extension("json.sig"));

    let wrapper_sha = match matched_wrapper_path.as_ref() {
        Some(path) => sha256_file(path)?,
        None => String::new(),
    };
    let signature_present = signature_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    let public_key_present = public_key_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    let signature_verified = match (
        matched_wrapper_path.as_ref(),
        signature_path.as_ref(),
        public_key_path.as_ref(),
    ) {
        (Some(wrapper), Some(signature), Some(public_key))
            if wrapper.is_file() && signature.is_file() && public_key.is_file() =>
        {
            verify_file_signature(wrapper, signature, public_key)?
        }
        _ => false,
    };

    let ao2_owned = matched_export
        .as_ref()
        .map(|wrapper| {
            let operator_role = json_string(&wrapper["export"]["audit_event"], "operator_role");
            let action = json_string(&wrapper["export"]["audit_event"], "action");
            let schema = json_string(wrapper, "schema_version");
            schema == "ao2.workbench-evidence-export.v1"
                && action == "obligation_gate"
                && !operator_role.is_empty()
        })
        .unwrap_or(false);

    let signing_status = if matched_wrapper_path.is_none() {
        "wrapper-not-found"
    } else if !signature_present || !public_key_present {
        "signature-missing"
    } else if !signature_verified {
        "signature-invalid"
    } else if !ao2_owned {
        "wrapper-not-ao2-owned"
    } else {
        "signed-and-verified"
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.obligation-gate-signing-audit.v1",
        "signing_status": signing_status,
        "gate_path": gate_path.display().to_string(),
        "stage": stage,
        "gate_sha256": raw_gate_sha,
        "evidence_exports_dir": exports_dir
            .as_ref()
            .map(|path| path.display().to_string()),
        "matched_wrapper_path": matched_wrapper_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "matched_wrapper_sha256": wrapper_sha,
        "signature_path": signature_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "signature_present": signature_present,
        "public_key_path": public_key_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "public_key_present": public_key_present,
        "signature_verified": signature_verified,
        "ao2_owned": ao2_owned,
        "factory_v3_role": "no_role",
        "ao2_decision_owner": "ao2-native-obligation-gate-signing-auditor",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    }))
}

pub(crate) fn contract_obligation_gate_signing_survey_json(
    target: Option<&Path>,
    summary: Option<&Path>,
) -> Result<serde_json::Value> {
    if target.is_none() && summary.is_none() {
        return Err(anyhow!(
            "contract obligation-gate-signing-survey requires --target <PATH>, --summary <PATH>, or both"
        ));
    }
    let mut per_gate: Vec<serde_json::Value> = Vec::new();
    let mut by_path: Vec<(PathBuf, usize)> = Vec::new();
    let mut sources: Vec<&'static str> = Vec::new();

    if let Some(target_path) = target {
        sources.push("runs-dir-scan");
        scan_runs_dir(target_path, &mut per_gate, &mut by_path)?;
    }
    if let Some(summary_path) = summary {
        sources.push("release-summary");
        scan_release_summary(summary_path, &mut per_gate, &mut by_path)?;
    }

    let mut signed_and_verified: u64 = 0;
    let mut unsigned: u64 = 0;
    let mut error_count: u64 = 0;
    let mut missing_count: u64 = 0;
    for entry in &per_gate {
        match json_string(entry, "signing_status").as_str() {
            "signed-and-verified" => signed_and_verified += 1,
            "check-errored" => error_count += 1,
            "gate-file-missing" => missing_count += 1,
            _ => unsigned += 1,
        }
    }
    let total_gates = per_gate.len() as u64;
    let status = if total_gates == 0 {
        "empty"
    } else if unsigned == 0 && error_count == 0 && missing_count == 0 {
        "all-signed-and-verified"
    } else {
        "remediation-required"
    };

    let runs_dir = target.map(|path| path.join(".ao2").join("runs"));
    Ok(serde_json::json!({
        "schema_version": "ao2.obligation-gate-signing-survey.v1",
        "target": target.map(|path| path.display().to_string()).unwrap_or_default(),
        "runs_dir": runs_dir.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
        "summary": summary.map(|path| path.display().to_string()).unwrap_or_default(),
        "sources": sources,
        "total_gates": total_gates,
        "signed_and_verified": signed_and_verified,
        "unsigned": unsigned,
        "missing": missing_count,
        "errors": error_count,
        "status": status,
        "per_gate": per_gate,
        "factory_v3_role": "no_role",
        "ao2_decision_owner": "ao2-native-obligation-gate-signing-surveyor",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "remediation_command_template": "ao2 workbench obligation-gate --target <target> --run-id <run_id> --stage <stage> --support-signing-key <PEM>"
    }))
}

fn upsert_gate_entry(
    per_gate: &mut Vec<serde_json::Value>,
    by_path: &mut Vec<(PathBuf, usize)>,
    gate_path: &Path,
    source: &str,
    build_entry: impl FnOnce() -> serde_json::Value,
) {
    if let Some((_, index)) = by_path.iter().find(|(path, _)| path == gate_path) {
        let existing = &mut per_gate[*index];
        let already_present = existing["sources"]
            .as_array()
            .map(|values| values.iter().any(|value| value.as_str() == Some(source)))
            .unwrap_or(false);
        if !already_present {
            if let Some(values) = existing["sources"].as_array_mut() {
                values.push(serde_json::Value::String(source.to_string()));
            }
        }
        return;
    }
    let entry = build_entry();
    by_path.push((gate_path.to_path_buf(), per_gate.len()));
    per_gate.push(entry);
}

fn scan_runs_dir(
    target: &Path,
    per_gate: &mut Vec<serde_json::Value>,
    by_path: &mut Vec<(PathBuf, usize)>,
) -> Result<()> {
    let runs_dir = target.join(".ao2").join("runs");
    if !runs_dir.is_dir() {
        return Ok(());
    }
    let mut run_entries: Vec<PathBuf> = fs::read_dir(&runs_dir)
        .with_context(|| format!("read {}", runs_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
        })
        .map(|entry| entry.path())
        .collect();
    run_entries.sort();
    for run_dir in run_entries {
        let run_id = run_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let evidence_dir = run_dir.join("evidence-pack");
        if !evidence_dir.is_dir() {
            continue;
        }
        let mut gate_entries: Vec<PathBuf> = fs::read_dir(&evidence_dir)
            .with_context(|| format!("read {}", evidence_dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with("obligation-gate-") && name.ends_with(".json")
            })
            .map(|entry| entry.path())
            .collect();
        gate_entries.sort();
        for gate_path in gate_entries {
            let stage = gate_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .and_then(|name| {
                    name.strip_prefix("obligation-gate-")
                        .and_then(|tail| tail.strip_suffix(".json"))
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let target_display = target.display().to_string();
            let run_id_clone = run_id.clone();
            let stage_clone = stage.clone();
            let gate_path_clone = gate_path.clone();
            upsert_gate_entry(per_gate, by_path, &gate_path, "runs-dir-scan", || {
                build_gate_entry(
                    &gate_path_clone,
                    &stage_clone,
                    Some(&run_id_clone),
                    Some(&target_display),
                    "runs-dir-scan",
                )
            });
        }
    }
    Ok(())
}

fn scan_release_summary(
    summary_path: &Path,
    per_gate: &mut Vec<serde_json::Value>,
    by_path: &mut Vec<(PathBuf, usize)>,
) -> Result<()> {
    let summary_text = fs::read_to_string(summary_path)
        .with_context(|| format!("read release summary {}", summary_path.display()))?;
    let summary: serde_json::Value = serde_json::from_str(&summary_text)
        .with_context(|| format!("parse release summary {}", summary_path.display()))?;
    let gates = summary
        .get("obligation_gates")
        .and_then(|value| value.get("gates"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for gate_value in gates {
        let path_str = json_string(&gate_value, "path");
        if path_str.is_empty() {
            continue;
        }
        let gate_path = PathBuf::from(&path_str);
        let stage = json_string(&gate_value, "stage");
        let stage_clone = stage.clone();
        let gate_path_clone = gate_path.clone();
        upsert_gate_entry(per_gate, by_path, &gate_path, "release-summary", || {
            build_gate_entry(
                &gate_path_clone,
                &stage_clone,
                None,
                None,
                "release-summary",
            )
        });
    }
    Ok(())
}

fn build_gate_entry(
    gate_path: &Path,
    stage: &str,
    run_id: Option<&str>,
    target_display: Option<&str>,
    source: &str,
) -> serde_json::Value {
    if !gate_path.is_file() {
        return serde_json::json!({
            "run_id": run_id.unwrap_or_default(),
            "stage": stage,
            "gate_path": gate_path.display().to_string(),
            "signing_status": "gate-file-missing",
            "signature_verified": false,
            "ao2_owned": false,
            "matched_wrapper_path": null,
            "suggested_remediation": format!(
                "gate file referenced by release summary is missing on disk: {} — restore the file or update the producer to emit a signed wrapper at the new path",
                gate_path.display()
            ),
            "sources": [source]
        });
    }
    match contract_verify_obligation_gate_signing_json(gate_path, None, None) {
        Ok(audit) => {
            let signing_status = json_string(&audit, "signing_status");
            let remediation = if signing_status == "signed-and-verified" {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(remediation_command(
                    target_display,
                    run_id,
                    stage,
                    gate_path,
                ))
            };
            serde_json::json!({
                "run_id": run_id.unwrap_or_default(),
                "stage": stage,
                "gate_path": gate_path.display().to_string(),
                "signing_status": signing_status,
                "signature_verified": audit
                    .get("signature_verified")
                    .cloned()
                    .unwrap_or(serde_json::Value::Bool(false)),
                "ao2_owned": audit
                    .get("ao2_owned")
                    .cloned()
                    .unwrap_or(serde_json::Value::Bool(false)),
                "matched_wrapper_path": audit
                    .get("matched_wrapper_path")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "suggested_remediation": remediation,
                "sources": [source],
                "audit": audit
            })
        }
        Err(error) => serde_json::json!({
            "run_id": run_id.unwrap_or_default(),
            "stage": stage,
            "gate_path": gate_path.display().to_string(),
            "signing_status": "check-errored",
            "signature_verified": false,
            "ao2_owned": false,
            "matched_wrapper_path": null,
            "suggested_remediation": remediation_command(target_display, run_id, stage, gate_path),
            "sources": [source],
            "error": error.to_string()
        }),
    }
}

fn remediation_command(
    target_display: Option<&str>,
    run_id: Option<&str>,
    stage: &str,
    gate_path: &Path,
) -> String {
    match (target_display, run_id) {
        (Some(target), Some(run)) => format!(
            "ao2 workbench obligation-gate --target {} --run-id {} --stage {} --support-signing-key <PEM>"
            , target, run, stage
        ),
        _ => format!(
            "obligation gate at {} is referenced by a release summary but lives outside .ao2/runs/; sign it by re-emitting via `ao2 workbench obligation-gate --support-signing-key <PEM>` from the producer that owns it, or migrate the producer to AO2-native signed emission",
            gate_path.display()
        ),
    }
}
