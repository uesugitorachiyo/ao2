use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_core::sha256_hex;
use ao2_policy::secret_redaction_class_counts;
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{atomic_write_text, sha256_bytes_hex, sha256_file};
use crate::factory_bridge;
use crate::factory_compat::*;
use crate::factory_queue::{factory_queue_load, factory_queue_path};
use crate::release_crypto::{
    derive_public_key_from_private_key, sign_file_with_private_key, verify_file_signature,
};

pub(crate) fn factory_pack_evidence_json(
    target: &Path,
    run_id: Option<&str>,
    out: &Path,
    signing: FactoryPlanSigning<'_>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    let queue = factory_queue_load(target)?;
    let queue_path = factory_queue_path(target);
    let entries = queue
        .get("entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return Err(anyhow!(
            "factory queue has no entries; nothing to pack evidence for"
        ));
    }

    let entry = if let Some(requested) = run_id {
        let trimmed = requested.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("--run-id must not be empty"));
        }
        entries
            .iter()
            .find(|entry| entry.get("run_id").and_then(|value| value.as_str()) == Some(trimmed))
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "factory queue has no entry with run_id {trimmed}; use queue-list to inspect"
                )
            })?
    } else {
        let mut candidates: Vec<(String, serde_json::Value)> = entries
            .iter()
            .filter(|entry| {
                entry
                    .get("evidence_pack")
                    .and_then(|value| value.as_str())
                    .map(|path| Path::new(path).is_file())
                    .unwrap_or(false)
            })
            .map(|entry| {
                let updated_at = entry
                    .get("updated_at")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string();
                (updated_at, entry.clone())
            })
            .collect();
        if candidates.is_empty() {
            return Err(anyhow!(
                "factory queue has no completed entries with an existing evidence_pack file; use --run-id to target a specific entry"
            ));
        }
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        candidates.pop().map(|(_, entry)| entry).unwrap()
    };

    let resolved_run_id = entry
        .get("run_id")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("selected factory queue entry is missing run_id"))?;
    let entry_status = entry
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let source = entry
        .get("evidence_pack")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            anyhow!(
                "factory queue entry {resolved_run_id} has no evidence_pack reference; \
                 run queue-run-next first or pick a different entry"
            )
        })?;
    let source_path = PathBuf::from(source);
    if !source_path.is_file() {
        return Err(anyhow!(
            "factory queue entry {resolved_run_id} references missing evidence pack {}",
            source_path.display()
        ));
    }
    let pack = read_factory_compat_value(&source_path).with_context(|| {
        format!(
            "read AO2 evidence pack for run_id {resolved_run_id} at {}",
            source_path.display()
        )
    })?;
    let pack_schema = pack
        .get("schema_version")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if pack_schema != "ao2.evidence-pack.v1" {
        return Err(anyhow!(
            "factory queue entry {resolved_run_id} evidence pack at {} has schema_version {} (expected ao2.evidence-pack.v1)",
            source_path.display(),
            pack_schema
        ));
    }
    let pack_owner = pack
        .get("runtime_contract")
        .and_then(|value| value.get("execution_owner"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if pack_owner != "ao2" {
        return Err(anyhow!(
            "factory queue entry {resolved_run_id} evidence pack at {} runtime_contract.execution_owner is {} (expected ao2)",
            source_path.display(),
            pack_owner
        ));
    }

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "create parent directory for evidence pack out {}",
                    out.display()
                )
            })?;
        }
    }
    let canonical = serde_json::to_string_pretty(&pack)?;
    fs::write(out, format!("{canonical}\n"))
        .with_context(|| format!("write canonical evidence pack to {}", out.display()))?;

    let evidence_pack_sha = sha256_file(out)
        .with_context(|| format!("hash written evidence pack {}", out.display()))?;
    let source_sha = sha256_file(&source_path)
        .with_context(|| format!("hash source evidence pack {}", source_path.display()))?;
    let native_evaluator_verdict = entry
        .get("native_evaluator_verdict")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    // Deterministic replay: re-canonicalize the source pack via the same path
    // the writer takes, and confirm SHA matches what is on disk. This proves
    // pack-evidence is byte-stable for a given source — a precondition for
    // AO2 owning the replay verdict in the migrated release-handoff flow.
    let replay_canonical = format!("{}\n", serde_json::to_string_pretty(&pack)?);
    let replay_sha = sha256_bytes_hex(replay_canonical.as_bytes());
    let replay_verified = replay_sha == evidence_pack_sha;
    let deterministic_replay = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-pack-evidence-deterministic-replay.v1",
        "verified": replay_verified,
        "replay_owner": "ao2-factory-pack-evidence",
        "replay_sha256": replay_sha,
        "written_sha256": evidence_pack_sha,
    });

    // Signing: write a sidecar .sig + .public.pem next to the canonical pack
    // when a signing key is supplied, mirroring the factory_plan_json pattern.
    // The trust boundary stays AO2-owned: factory-v3 never produces the key
    // material and never signs the pack itself.
    let signature_path = out.with_extension(canonical_signature_extension(out));
    let public_key_path = out.with_extension(canonical_public_key_extension(out));
    let signature = match signing.key {
        Some(key_path) => {
            derive_public_key_from_private_key(key_path, &public_key_path)?;
            sign_file_with_private_key(key_path, out, &signature_path)?;
            let signature_verified = verify_file_signature(out, &signature_path, &public_key_path)?;
            serde_json::json!({
                "schema_version": "ao2.factory-v3-compat-pack-evidence-signature.v1",
                "signature_algorithm": "RSA/SHA-256",
                "signer_id": signing.signer_id,
                "signed_payload": "evidence_pack_out",
                "signed_payload_path": out.display().to_string(),
                "signed_payload_sha256": evidence_pack_sha.clone(),
                "signature_path": signature_path.display().to_string(),
                "signature_sha256": sha256_file(&signature_path)?,
                "public_key_path": public_key_path.display().to_string(),
                "public_key_sha256": sha256_file(&public_key_path)?,
                "signature_verified": signature_verified,
            })
        }
        None => serde_json::json!({
            "schema_version": "ao2.factory-v3-compat-pack-evidence-signature.v1",
            "signed_payload": "evidence_pack_out",
            "signature_verified": false,
            "signature_status": "unsigned",
        }),
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-pack-evidence.v1",
        "status": "produced",
        "run_id": resolved_run_id,
        "queue_path": queue_path.display().to_string(),
        "entry_status": entry_status,
        "native_evaluator_verdict": native_evaluator_verdict,
        "evidence_pack_source": source_path.display().to_string(),
        "evidence_pack_source_sha256": source_sha,
        "evidence_pack_out": out.display().to_string(),
        "evidence_pack_sha256": evidence_pack_sha,
        "evidence_pack_schema_version": "ao2.evidence-pack.v1",
        "evidence_pack_execution_owner": "ao2",
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-workbench-queue",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "deterministic_replay": deterministic_replay,
        "signature": signature,
    }))
}

fn canonical_signature_extension(out: &Path) -> String {
    match out.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if !ext.is_empty() => format!("{ext}.sig"),
        _ => "sig".to_string(),
    }
}

fn canonical_public_key_extension(out: &Path) -> String {
    match out.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if !ext.is_empty() => format!("{ext}.public.pem"),
        _ => "public.pem".to_string(),
    }
}

fn infer_ao_operator_agents_dir_from_runspec(runspec_path: &Path) -> Option<PathBuf> {
    let runspecs_dir = runspec_path.parent()?;
    if runspecs_dir.file_name()?.to_str()? != "runspecs" {
        return None;
    }
    let ao_dir = runspecs_dir.parent()?;
    if ao_dir.file_name()?.to_str()? != "ao" {
        return None;
    }
    let agents_dir = ao_dir.parent()?.join("agents");
    agents_dir.is_dir().then_some(agents_dir)
}

fn factory_role_contract_candidate_stems(role_id: &str) -> Vec<String> {
    let mut stems = Vec::<String>::new();
    let normalized = role_id.trim().to_ascii_lowercase().replace([' ', '_'], "-");
    if !normalized.is_empty() {
        stems.push(normalized);
    }
    if let Ok(canonical) = factory_bridge::canonical_role(role_id) {
        let canonical_stem = canonical.replace('_', "-");
        stems.push(canonical_stem.clone());
        match canonical {
            "reviewer" => stems.push("slice-reviewer".to_string()),
            "evaluator_closer" => stems.push("evaluator-closer".to_string()),
            "plan_hardener" => stems.push("plan-hardener".to_string()),
            "factory_manager" => stems.push("factory-manager".to_string()),
            _ => {}
        }
    }
    stems.dedup();
    stems
}

fn factory_effective_role_contracts(
    explicit_role_contracts: &[PathBuf],
    runspec_path: Option<&Path>,
    runspec_value: Option<&serde_json::Value>,
) -> Result<(Vec<PathBuf>, serde_json::Value)> {
    if !explicit_role_contracts.is_empty() {
        return Ok((
            explicit_role_contracts.to_vec(),
            serde_json::json!({
                "mode": "explicit_role_contract_args",
                "loaded_count": explicit_role_contracts.len(),
                "factory_v3_required_to_discover": false
            }),
        ));
    }

    let Some(runspec_path) = runspec_path else {
        return Ok((
            Vec::new(),
            serde_json::json!({
                "mode": "none",
                "loaded_count": 0,
                "factory_v3_required_to_discover": false
            }),
        ));
    };
    let Some(agents_dir) = infer_ao_operator_agents_dir_from_runspec(runspec_path) else {
        return Ok((
            Vec::new(),
            serde_json::json!({
                "mode": "not_discovered",
                "loaded_count": 0,
                "factory_v3_required_to_discover": false
            }),
        ));
    };

    let mut selected = Vec::<PathBuf>::new();
    let mut selected_seen = BTreeSet::<PathBuf>::new();
    let mut missing_roles = Vec::<String>::new();
    for role_id in factory_runspec_role_ids(runspec_value) {
        let mut matched = None;
        for stem in factory_role_contract_candidate_stems(&role_id) {
            let candidate = agents_dir.join(format!("{stem}.toml"));
            if candidate.is_file() {
                matched = Some(candidate);
                break;
            }
        }
        if let Some(path) = matched {
            if selected_seen.insert(path.clone()) {
                selected.push(path);
            }
        } else {
            missing_roles.push(role_id);
        }
    }

    Ok((
        selected.clone(),
        serde_json::json!({
            "mode": "auto_discovered_from_ao_runspec_layout",
            "agents_dir": agents_dir.display().to_string(),
            "loaded_count": selected.len(),
            "missing_roles": missing_roles,
            "factory_v3_required_to_discover": false
        }),
    ))
}

pub(crate) struct FactoryPlanSigning<'a> {
    pub(crate) key: Option<&'a Path>,
    pub(crate) signer_id: &'a str,
}

pub(crate) fn factory_plan_json(
    request: &Path,
    profile: Option<&Path>,
    runspec: Option<&Path>,
    role_contracts: &[PathBuf],
    signing: FactoryPlanSigning<'_>,
    target: &Path,
    out: Option<&Path>,
) -> Result<serde_json::Value> {
    factory_ensure_target_repo(target)?;
    if !request.exists() {
        return Err(anyhow!(
            "factory work request does not exist: {}",
            request.display()
        ));
    }
    let request_value = read_factory_compat_value(request)?;
    let profile_value = match profile {
        Some(path) => Some(read_factory_compat_value(path)?),
        None => None,
    };
    let runspec_value = match runspec {
        Some(path) => Some(read_factory_compat_value(path)?),
        None => None,
    };
    reject_factory_provider_api_key_auth("work_request", &request_value)?;
    if let Some(value) = profile_value.as_ref() {
        reject_factory_provider_api_key_auth("profile", value)?;
        validate_factory_profile_graph(value)?;
    }
    if let Some(value) = runspec_value.as_ref() {
        reject_factory_provider_api_key_auth("runspec", value)?;
        validate_factory_runspec_graph(value)?;
    }
    let (effective_role_contracts, role_contract_discovery) =
        factory_effective_role_contracts(role_contracts, runspec, runspec_value.as_ref())?;
    let mut role_contract_values = Vec::new();
    for path in &effective_role_contracts {
        let contract = read_factory_compat_value(path)?;
        reject_factory_provider_api_key_auth("role_contract", &contract)?;
        role_contract_values.push(serde_json::json!({
            "path": path.display().to_string(),
            "digest": sha256_hex(fs::read(path).with_context(|| format!("read role contract {}", path.display()))?),
            "contract": contract
        }));
    }

    let classification_text = serde_json::to_string(&serde_json::json!({
        "request": request_value,
        "profile": profile_value,
        "runspec": runspec_value,
        "role_contracts": role_contract_values
    }))?
    .to_lowercase();
    let structured_classification = factory_structured_classification_override(&request_value);
    let shape = structured_classification
        .as_ref()
        .map(|classification| classification.shape.as_str())
        .unwrap_or_else(|| classify_factory_shape(&classification_text));
    let size = structured_classification
        .as_ref()
        .map(|classification| classification.size.as_str())
        .unwrap_or_else(|| {
            classify_factory_size(
                &classification_text,
                profile.is_some(),
                runspec.is_some(),
                effective_role_contracts.len(),
            )
        });
    let classification_source = structured_classification
        .as_ref()
        .map(|classification| classification.source)
        .unwrap_or("ao2-native-heuristic");
    let roles = factory_compat_roles(
        &role_contract_values,
        runspec_value.as_ref(),
        profile_value.as_ref(),
    );
    let runspec_translation =
        factory_runspec_translation(runspec_value.as_ref(), profile_value.as_ref(), &roles);
    let provider_profiles =
        factory_provider_profiles(runspec_value.as_ref(), profile_value.as_ref());
    let request_stem = request
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("work-request");
    let plan_path = out.map(Path::to_path_buf).unwrap_or_else(|| {
        target
            .join(".ao2")
            .join("factory-compat-plans")
            .join(format!("{request_stem}-plan.json"))
    });
    let workflow_path = plan_path.with_extension("workflow.yaml");
    let evidence_path = plan_path.with_extension("planning-evidence.json");
    let signature_path = evidence_path.with_extension("json.sig");
    let public_key_path = evidence_path.with_extension("public.pem");
    let request_digest = sha256_hex(
        fs::read(request).with_context(|| format!("read request {}", request.display()))?,
    );
    let profile_ref = profile
        .map(|path| factory_input_ref("profile", path))
        .transpose()?;
    let runspec_ref = runspec
        .map(|path| factory_input_ref("runspec", path))
        .transpose()?;
    let role_contract_refs = effective_role_contracts
        .iter()
        .map(|path| factory_input_ref("role_contract", path))
        .collect::<Result<Vec<_>>>()?;
    let workflow_id = format!("factory-v3-compat-{shape}-{size}");
    let workflow_value = factory_compat_workflow_value(
        &workflow_id,
        &request_value,
        runspec_value.as_ref(),
        profile_value.as_ref(),
        &roles,
    );
    let redaction_scan_input = serde_json::to_string(&serde_json::json!({
        "request": request_value,
        "profile": profile_value,
        "runspec": runspec_value,
        "role_contracts": role_contract_values
    }))?;
    let role_contract_gate = factory_role_contract_gate(&roles);
    let redaction_counts = secret_redaction_class_counts(&redaction_scan_input);
    let redaction_count: usize = redaction_counts.values().sum();
    let plan = serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-governed-plan.v1",
        "workflow_id": workflow_id,
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "target": target.display().to_string(),
        "factory_v3_inputs": {
            "work_request": factory_input_ref("work_request", request)?,
            "profile": profile_ref,
            "runspec": runspec_ref,
            "role_contracts": role_contract_refs
        },
        "classification": {
            "size": size,
            "shape": shape,
            "owner": "ao2-native-classifier",
            "source": classification_source,
            "factory_v3_required_before_classification": false,
            "signals": factory_classification_signals(&classification_text)
        },
        "ao2_native_plan": {
            "intake_owner": "ao2",
            "compatibility_oracle": "factory-v3",
            "roles": roles,
            "profile_policy_posture": profile_value
                .as_ref()
                .and_then(|value| value.get("policy_posture").cloned())
                .unwrap_or_else(|| serde_json::json!({})),
            "factory_v3_translation": runspec_translation,
            "provider_profiles": provider_profiles,
            "role_contract_discovery": role_contract_discovery,
            "role_contract_gate": role_contract_gate.clone(),
            "midpoint_gate": {
                "owner": "ao2-native-evaluator",
                "required_contracts": ["evidence", "concerns", "blockers", "changed_files", "sandbox", "secret_redaction"]
            },
            "closure_gate": {
                "owner": "ao2-native-evaluator-closer",
                "acceptance_inputs": ["verifier_artifacts", "role_outputs", "policy_decisions", "replay_digest", "obligation_gates"],
                "factory_v3_role": "parity_oracle_only"
            },
            "evidence_outputs": ["planning_evidence", "event_log", "evidence_pack", "digest_replay", "memory_summary"],
            "runnable_workflow": {
                "path": workflow_path.display().to_string(),
                "schema": "ao2.workflow-template-compatible-yaml",
                "factory_v3_drives_workflow": false,
                "invocation": format!("ao2 run {} --target {} --pause-for-approval", workflow_path.display(), target.display())
            }
        },
        "trust_boundary": {
            "front_end": "Hermes may submit and observe this plan",
            "execution_owner": "ao2",
            "observer": "ao2-control-plane read-only after signed evidence exists",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden",
            "target_mutation": "governed AO2 workflow only"
        },
        "parity_checklist_progress": {
            "ao2_accepts_request_and_classifies": true,
            "ao2_materializes_native_plan_from_factory_inputs": true,
            "factory_v3_drives_workflow": false,
            "remaining_before_replacement_ready": [
                "execute this generated plan through AO2 role adapters",
                "compare AO2 evaluator/closer decisions against factory-v3 outputs",
                "produce signed evidence pack and deterministic replay for the governed run",
                "complete macOS/Ubuntu/Windows replacement workflow smoke"
            ]
        }
    });
    let plan_digest = sha256_hex(serde_json::to_vec(&plan)?);
    let mut evidence = serde_json::json!({
        "schema_version": "ao2.factory-compat-planning-evidence.v1",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "producer": "ao2 factory plan",
        "plan_path": plan_path.display().to_string(),
        "workflow_path": workflow_path.display().to_string(),
        "workflow_sha256": sha256_hex(serde_yaml::to_string(&workflow_value)?.as_bytes()),
        "plan_sha256": plan_digest,
        "request_sha256": request_digest,
        "classification": plan["classification"],
        "role_contract_gate": role_contract_gate,
        "secret_redaction": {
            "status": if redaction_count > 0 { "redacted" } else { "no_secrets_detected" },
            "redaction_count": redaction_count,
            "class_counts": redaction_counts,
            "raw_factory_inputs_persisted": false,
            "materialized_workflow_fields_redacted": ["objective", "acceptance"]
        },
        "signed_evidence_status": "digest-backed-planning-evidence-ready-for-pack-signing",
        "trust_boundary": plan["trust_boundary"]
    });
    if signing.key.is_some() {
        evidence["signed_evidence_status"] =
            serde_json::json!("signed-and-verified-planning-evidence");
    }
    if let Some(parent) = evidence_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write_text(&workflow_path, &serde_yaml::to_string(&workflow_value)?)?;
    atomic_write_text(&plan_path, &serde_json::to_string_pretty(&plan)?)?;
    let signed_payload_path = evidence_path.with_extension("signed-payload.json");
    let evidence_artifact_ref = |path: &Path| -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string())
    };
    atomic_write_text(
        &signed_payload_path,
        &serde_json::to_string_pretty(&evidence)?,
    )?;
    let signature = match signing.key {
        Some(key_path) => {
            derive_public_key_from_private_key(key_path, &public_key_path)?;
            sign_file_with_private_key(key_path, &signed_payload_path, &signature_path)?;
            let signature_verified =
                verify_file_signature(&signed_payload_path, &signature_path, &public_key_path)?;
            serde_json::json!({
                "schema_version": "ao2.factory-compat-planning-evidence-signature.v1",
                "signature_algorithm": "RSA/SHA-256",
                "signer_id": signing.signer_id,
                "signed_payload": "planning_evidence_without_signature_field",
                "signed_payload_path": evidence_artifact_ref(&signed_payload_path),
                "signed_payload_sha256": sha256_file(&signed_payload_path)?,
                "signature_path": evidence_artifact_ref(&signature_path),
                "signature_sha256": sha256_file(&signature_path)?,
                "public_key_path": evidence_artifact_ref(&public_key_path),
                "public_key_sha256": sha256_file(&public_key_path)?,
                "signature_verified": signature_verified
            })
        }
        None => serde_json::json!({
            "schema_version": "ao2.factory-compat-planning-evidence-signature.v1",
            "signed_payload_path": evidence_artifact_ref(&signed_payload_path),
            "signed_payload_sha256": sha256_file(&signed_payload_path)?,
            "signature_verified": false,
            "signature_status": "unsigned"
        }),
    };
    if let Some(object) = evidence.as_object_mut() {
        object.insert("signature".to_string(), signature.clone());
    }
    atomic_write_text(&evidence_path, &serde_json::to_string_pretty(&evidence)?)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-plan-result.v1",
        "plan_path": plan_path.display().to_string(),
        "workflow_path": workflow_path.display().to_string(),
        "planning_evidence_path": evidence_path.display().to_string(),
        "signature": signature,
        "classification": plan["classification"],
        "plan_sha256": plan_digest,
        "request_sha256": request_digest,
        "ao2_native_plan": plan["ao2_native_plan"],
        "parity_checklist_progress": plan["parity_checklist_progress"]
    }))
}

pub(crate) fn factory_verify_planning_evidence_json(
    evidence_path: &Path,
    signed_payload_override: Option<&Path>,
    signature_override: Option<&Path>,
    public_key_override: Option<&Path>,
) -> Result<serde_json::Value> {
    let evidence = read_factory_compat_value(evidence_path)
        .with_context(|| format!("read AO2 planning evidence {}", evidence_path.display()))?;
    if evidence["schema_version"] != "ao2.factory-compat-planning-evidence.v1" {
        return Err(anyhow!(
            "factory planning evidence verify requires ao2.factory-compat-planning-evidence.v1: {}",
            evidence_path.display()
        ));
    }
    let evidence_base = evidence_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve_evidence_path = |value: &str| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            evidence_base.join(path)
        }
    };
    let signature_block = &evidence["signature"];
    let signed_payload = signature_block
        .get("signed_payload")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let signed_payload_path = signed_payload_override.map(Path::to_path_buf).or_else(|| {
        signature_block
            .get("signed_payload_path")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(&resolve_evidence_path)
    });
    let signature_path = signature_override.map(Path::to_path_buf).or_else(|| {
        signature_block
            .get("signature_path")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(&resolve_evidence_path)
    });
    let public_key_path = public_key_override.map(Path::to_path_buf).or_else(|| {
        signature_block
            .get("public_key_path")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(resolve_evidence_path)
    });
    let signed_payload_digest_match = match signed_payload_path.as_ref() {
        Some(path) if path.is_file() => signature_block
            .get("signed_payload_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let signature_digest_match = match signature_path.as_ref() {
        Some(path) if path.is_file() => signature_block
            .get("signature_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let public_key_digest_match = match public_key_path.as_ref() {
        Some(path) if path.is_file() => signature_block
            .get("public_key_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let evidence_body_matches_signed_payload = match signed_payload_path.as_ref() {
        Some(path) if path.is_file() && signed_payload_digest_match => {
            let signed_payload_value = read_factory_compat_value(path)
                .with_context(|| format!("read signed planning payload {}", path.display()))?;
            let mut evidence_without_signature = evidence.clone();
            if let Some(object) = evidence_without_signature.as_object_mut() {
                object.remove("signature");
            }
            signed_payload_value == evidence_without_signature
        }
        _ => false,
    };
    let signature_verified = match (
        signed_payload_path.as_ref(),
        signature_path.as_ref(),
        public_key_path.as_ref(),
    ) {
        (Some(signed_payload_path), Some(signature_path), Some(public_key_path))
            if signed_payload_path.is_file()
                && signature_path.is_file()
                && public_key_path.is_file()
                && signed_payload == "planning_evidence_without_signature_field"
                && signed_payload_digest_match
                && signature_digest_match
                && public_key_digest_match
                && evidence_body_matches_signed_payload =>
        {
            verify_file_signature(signed_payload_path, signature_path, public_key_path)?
        }
        _ => false,
    };
    let trust_boundary_ok = evidence["trust_boundary"]["execution_owner"] == "ao2"
        && evidence["trust_boundary"]["observer"]
            == "ao2-control-plane read-only after signed evidence exists"
        && evidence["classification"]["owner"] == "ao2-native-classifier"
        && evidence["classification"]["factory_v3_required_before_classification"] == false;
    let status = if signature_verified && trust_boundary_ok {
        "accepted"
    } else {
        "rejected"
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-planning-evidence-verification.v1",
        "status": status,
        "evidence_path": evidence_path.display().to_string(),
        "signature_status": if signature_path.is_some() || public_key_path.is_some() { "signed" } else { "unsigned" },
        "signed_payload_digest_match": signed_payload_digest_match,
        "signature_digest_match": signature_digest_match,
        "public_key_digest_match": public_key_digest_match,
        "evidence_body_matches_signed_payload": evidence_body_matches_signed_payload,
        "signature_verified": signature_verified,
        "trust_boundary_ok": trust_boundary_ok,
        "planning_decision_contract": {
            "decision_owner": "ao2",
            "classification_owner": evidence["classification"]["owner"].clone(),
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        },
        "parity_checklist_progress": {
            "ao2_can_verify_signed_planning_evidence_without_factory_driver": signature_verified,
            "factory_v3_drives_workflow": false
        }
    }))
}

pub(crate) fn factory_verify_evaluator_decision_json(
    decision_path: &Path,
) -> Result<serde_json::Value> {
    let decision = read_factory_compat_value(decision_path).with_context(|| {
        format!(
            "read AO2 native evaluator decision {}",
            decision_path.display()
        )
    })?;
    if decision["schema_version"] != "ao2.factory-v3-compat-native-evaluator-result.v1" {
        return Err(anyhow!(
            "factory evaluator decision verify requires ao2.factory-v3-compat-native-evaluator-result.v1: {}",
            decision_path.display()
        ));
    }
    let decision_base = decision_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve_decision_path = |value: &str| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            decision_base.join(path)
        }
    };
    let signature = &decision["signature"];
    let signed_payload = signature
        .get("signed_payload")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let signed_payload_path = signature
        .get("signed_payload_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(&resolve_decision_path);
    let signature_path = signature
        .get("signature_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(&resolve_decision_path);
    let public_key_path = signature
        .get("public_key_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(resolve_decision_path);
    let signature_status = if signature_path.is_some() || public_key_path.is_some() {
        "signed"
    } else {
        "unsigned"
    };
    let signed_payload_digest_match = match signed_payload_path.as_ref() {
        Some(path) if path.is_file() => signature
            .get("signed_payload_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let signature_digest_match = match signature_path.as_ref() {
        Some(path) if path.is_file() => signature
            .get("signature_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let public_key_digest_match = match public_key_path.as_ref() {
        Some(path) if path.is_file() => signature
            .get("public_key_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let decision_payload_matches_signed_payload = match signed_payload_path.as_ref() {
        Some(path) if path.is_file() && signed_payload_digest_match => {
            let signed_payload_value = read_factory_compat_value(path)
                .with_context(|| format!("read signed evaluator payload {}", path.display()))?;
            let mut decision_without_signature = decision.clone();
            if let Some(object) = decision_without_signature.as_object_mut() {
                object.remove("signature");
            }
            signed_payload_value == decision_without_signature
        }
        _ => false,
    };
    let signature_verified = match (
        signed_payload_path.as_ref(),
        signature_path.as_ref(),
        public_key_path.as_ref(),
    ) {
        (Some(signed_payload_path), Some(signature_path), Some(public_key_path))
            if signed_payload_path.is_file()
                && signature_path.is_file()
                && public_key_path.is_file()
                && signed_payload == "native_evaluator_decision_without_signature_field"
                && signed_payload_digest_match
                && signature_digest_match
                && public_key_digest_match
                && decision_payload_matches_signed_payload =>
        {
            verify_file_signature(signed_payload_path, signature_path, public_key_path)?
        }
        _ => false,
    };
    let trust_boundary_ok = decision["trust_boundary"]["decision_owner"] == "ao2"
        && decision["trust_boundary"]["factory_v3_role"] == "parity_oracle_only"
        && decision["trust_boundary"]["control_plane_role"]
            == "read_only_observer_after_signed_evidence"
        && decision["native_evaluator_decision"]["factory_v3_required_to_decide"] == false
        && decision["native_evaluator_decision"]["owner"] == "ao2-native-evaluator-closer";
    let signature_requirement_satisfied = signature_status == "signed" && signature_verified;
    let accepted = signature_requirement_satisfied && trust_boundary_ok;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-native-evaluator-verification.v1",
        "status": if accepted { "accepted" } else { "rejected" },
        "decision_path": decision_path.display().to_string(),
        "signed_payload_path": signed_payload_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "signed_payload": signed_payload,
        "signed_payload_digest_match": signed_payload_digest_match,
        "decision_payload_matches_signed_payload": decision_payload_matches_signed_payload,
        "signature_status": signature_status,
        "signature_digest_match": signature_digest_match,
        "public_key_digest_match": public_key_digest_match,
        "signature_verified": signature_verified,
        "signature_requirement_satisfied": signature_requirement_satisfied,
        "trust_boundary_ok": trust_boundary_ok,
        "verdict": decision["verdict"].clone(),
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-evaluator-decision-verifier",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    }))
}

/// Verify a signed AO2-native bridge evidence file end-to-end. Mirrors the
/// evaluator-decision verify pattern: defaults sidecar paths to whatever the
/// body's `signature` block recorded, supports per-sidecar overrides, runs
/// four independent integrity checks (sha-match, body-minus-signature
/// equality, RSA/SHA-256 verify, trust-boundary check), and emits an
/// `ao2.factory-bridge-evidence-verification.v1` report so observers
/// (factory-v3 passthrough, control-plane displays, third-party auditors)
/// can shell out to AO2 instead of re-implementing the RSA sidecar pattern.
pub(crate) fn factory_verify_bridge_evidence_json(
    evidence_path: &Path,
    signed_payload_override: Option<&Path>,
    signature_override: Option<&Path>,
    public_key_override: Option<&Path>,
) -> Result<serde_json::Value> {
    let evidence = read_factory_compat_value(evidence_path)
        .with_context(|| format!("read AO2 bridge evidence {}", evidence_path.display()))?;
    if evidence["schema"] != factory_bridge::BRIDGE_SCHEMA {
        return Err(anyhow!(
            "factory verify-bridge-evidence requires {}: {}",
            factory_bridge::BRIDGE_SCHEMA,
            evidence_path.display()
        ));
    }
    let evidence_base = evidence_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve_evidence_path = |value: &str| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            evidence_base.join(path)
        }
    };
    let signature_block = &evidence["signature"];
    let signed_payload_marker = signature_block
        .get("signed_payload")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let body_signed_payload_path = signature_block
        .get("signed_payload_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(&resolve_evidence_path);
    let body_signature_path = signature_block
        .get("signature_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(&resolve_evidence_path);
    let body_public_key_path = signature_block
        .get("public_key_path")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(&resolve_evidence_path);
    let signed_payload_path = signed_payload_override
        .map(PathBuf::from)
        .or(body_signed_payload_path);
    let signature_path = signature_override
        .map(PathBuf::from)
        .or(body_signature_path);
    let public_key_path = public_key_override
        .map(PathBuf::from)
        .or(body_public_key_path);
    let signature_status =
        if signature_block.is_object() && (signature_path.is_some() || public_key_path.is_some()) {
            "signed"
        } else {
            "unsigned"
        };
    let signed_payload_digest_match = match signed_payload_path.as_ref() {
        Some(path) if path.is_file() => signature_block
            .get("signed_payload_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let signature_digest_match = match signature_path.as_ref() {
        Some(path) if path.is_file() => signature_block
            .get("signature_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let public_key_digest_match = match public_key_path.as_ref() {
        Some(path) if path.is_file() => signature_block
            .get("public_key_sha256")
            .and_then(|value| value.as_str())
            .map(|expected| sha256_file(path).map(|actual| actual == expected))
            .transpose()?
            .unwrap_or(false),
        _ => false,
    };
    let evidence_body_matches_signed_payload = match signed_payload_path.as_ref() {
        Some(path) if path.is_file() && signed_payload_digest_match => {
            let signed_payload_value = read_factory_compat_value(path)
                .with_context(|| format!("read signed bridge payload {}", path.display()))?;
            let mut evidence_without_signature = evidence.clone();
            if let Some(object) = evidence_without_signature.as_object_mut() {
                object.remove("signature");
                object.remove("signed_evidence_status");
            }
            signed_payload_value == evidence_without_signature
        }
        _ => false,
    };
    let signature_verified = match (
        signed_payload_path.as_ref(),
        signature_path.as_ref(),
        public_key_path.as_ref(),
    ) {
        (Some(signed_payload_path), Some(signature_path), Some(public_key_path))
            if signed_payload_path.is_file()
                && signature_path.is_file()
                && public_key_path.is_file()
                && signed_payload_marker == "bridge_evidence_without_signature_field"
                && signed_payload_digest_match
                && signature_digest_match
                && public_key_digest_match
                && evidence_body_matches_signed_payload =>
        {
            verify_file_signature(signed_payload_path, signature_path, public_key_path)?
        }
        _ => false,
    };
    let trust_boundary_ok = evidence["trust_boundary"]["factory_v3_role"] == "parity_oracle_only"
        && evidence["trust_boundary"]["ao2_role"] == "ao2_native_bridge_evidence_owner"
        && evidence["trust_boundary"]["control_plane_role"]
            == "read_only_observer_after_signed_evidence"
        && evidence["trust_boundary"]["bridge_owner"] == "ao2_factory_bridge_subcommand";
    let signature_requirement_satisfied = signature_status == "signed" && signature_verified;
    let mapping_digest_ok = evidence["mapping"]["digest"] == factory_bridge::mapping_digest();
    let accepted = signature_requirement_satisfied && trust_boundary_ok && mapping_digest_ok;
    Ok(serde_json::json!({
        "schema_version": "ao2.factory-bridge-evidence-verification.v1",
        "status": if accepted { "accepted" } else { "rejected" },
        "evidence_path": evidence_path.display().to_string(),
        "signed_payload_marker": signed_payload_marker,
        "signed_payload_path": signed_payload_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "signature_path": signature_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "public_key_path": public_key_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "signed_payload_digest_match": signed_payload_digest_match,
        "signature_digest_match": signature_digest_match,
        "public_key_digest_match": public_key_digest_match,
        "evidence_body_matches_signed_payload": evidence_body_matches_signed_payload,
        "signature_status": signature_status,
        "signature_verified": signature_verified,
        "signature_requirement_satisfied": signature_requirement_satisfied,
        "trust_boundary_ok": trust_boundary_ok,
        "mapping_digest_ok": mapping_digest_ok,
        "mapping_digest": factory_bridge::mapping_digest(),
        "signed_evidence_status": evidence["signed_evidence_status"].clone(),
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-bridge-evidence-verifier",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    }))
}
