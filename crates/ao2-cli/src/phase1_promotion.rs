use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{
    base64_standard, canonical_json_sha256, hex_lower, json_string, json_u64, sha256_bytes_hex,
    sha256_file,
};
use crate::control_plane_http::{control_plane_endpoint, get_json_http, post_json_http};
use crate::factory_compat::read_factory_compat_value;
use crate::release_comparison::{
    read_json_for_verification, release_evidence_bundle_verification_json,
};
use crate::release_crypto::{public_key_pem_from_private_key, sign_bytes_with_private_key};
use crate::release_gate::{
    release_factory_project_run_readback_verification_json,
    release_governed_run_evidence_verification_json,
    release_replacement_smoke_gate_verification_json,
};
use crate::release_summary::release_smoke_summary_verification_json;
use crate::workbench_queue::atomic_write_text;
use crate::{resolve_api_token, trimmed_required};

#[allow(clippy::too_many_arguments)]
pub(crate) fn phase1_promotion_decision_build_json(
    release_gate_path: &Path,
    replacement_smoke_gate_path: Option<&Path>,
    governed_run_evidence_paths: &[PathBuf],
    factory_project_run_summary_paths: &[PathBuf],
    provider_acceptance_preservation_path: Option<&Path>,
    operator: &str,
    rationale: &str,
    out: &Path,
    checklist_out: Option<&Path>,
) -> Result<serde_json::Value> {
    let operator = trimmed_required("--operator", operator)?;
    let rationale = trimmed_required("--rationale", rationale)?;
    let release_gate = read_factory_compat_value(release_gate_path)
        .with_context(|| format!("read release gate {}", release_gate_path.display()))?;
    if release_gate["schema"] != "ao2.release-gate.v1" {
        return Err(anyhow!("release gate schema must be ao2.release-gate.v1"));
    }
    if release_gate["status"] != "verified" {
        return Err(anyhow!("release gate status must be verified"));
    }

    let governed_run_evidence =
        release_governed_run_evidence_verification_json(governed_run_evidence_paths);
    if json_string(&governed_run_evidence, "status") != "verified" {
        return Err(anyhow!(
            "governed run evidence must include accepted macOS, Ubuntu, and Windows AO2-owned governed runs"
        ));
    }
    let release_gate_governed_run = release_gate
        .get("governed_run_evidence")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if json_string(&release_gate_governed_run, "status") != "verified" {
        return Err(anyhow!(
            "release gate must include verified governed_run_evidence"
        ));
    }
    let factory_project_run_readback =
        release_factory_project_run_readback_verification_json(factory_project_run_summary_paths);
    if json_string(&factory_project_run_readback, "status") != "verified" {
        return Err(anyhow!(
            "project-run readback must include accepted macOS, Ubuntu, and Windows replacement-packet handoff proof"
        ));
    }
    let release_gate_project_run_readback = release_gate
        .get("factory_project_run_readback")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if json_string(&release_gate_project_run_readback, "status") != "verified" {
        return Err(anyhow!(
            "release gate must include verified factory_project_run_readback"
        ));
    }
    let replacement_smoke_gate = match replacement_smoke_gate_path {
        Some(path) => Some(
            read_factory_compat_value(path)
                .with_context(|| format!("read replacement smoke gate {}", path.display()))?,
        ),
        None => None,
    };
    let replacement_gate_verification = replacement_smoke_gate_path
        .map(release_replacement_smoke_gate_verification_json)
        .unwrap_or_else(|| {
            serde_json::json!({
                "schema": "ao2.release-replacement-smoke-gate-verification.v1",
                "status": "superseded_by_governed_run",
                "gate_status": "not_required",
                "accepted_os": [],
                "reasons": []
            })
        });
    if replacement_smoke_gate_path.is_some()
        && json_string(&replacement_gate_verification, "status") != "verified"
    {
        return Err(anyhow!("replacement smoke gate must be accepted"));
    }
    let release_gate_replacement = release_gate
        .get("replacement_smoke_gate")
        .cloned()
        .unwrap_or_else(|| replacement_gate_verification.clone());
    if replacement_smoke_gate_path.is_some()
        && (json_string(&release_gate_replacement, "status") != "verified"
            || json_string(&release_gate_replacement, "gate_status") != "accepted")
    {
        return Err(anyhow!(
            "release gate must include verified replacement_smoke_gate"
        ));
    }

    let checklist_path = checklist_out
        .map(PathBuf::from)
        .unwrap_or_else(|| out.with_extension("checklist.json"));
    let release_gate_sha256 = sha256_file(release_gate_path)?;
    let replacement_smoke_gate_sha256 = match replacement_smoke_gate_path {
        Some(path) => Some(sha256_file(path)?),
        None => None,
    };
    let provider_acceptance_preservation = match provider_acceptance_preservation_path {
        Some(path) => Some(provider_acceptance_preservation_verification_json(path)?),
        None => None,
    };
    let governed_run_evidence_paths_json = governed_run_evidence_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let factory_project_run_summary_paths_json = factory_project_run_summary_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let mut checks = vec![
        serde_json::json!({
            "id": "release-gate-verified",
            "status": "passed",
            "evidence": release_gate_path.display().to_string()
        }),
        serde_json::json!({
            "id": "governed-run-evidence-accepted",
            "status": "passed",
            "evidence": governed_run_evidence_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
        }),
        serde_json::json!({
            "id": "factory-v3-parity-only",
            "status": "passed",
            "evidence": governed_run_evidence_paths_json.clone()
        }),
        serde_json::json!({
            "id": "control-plane-read-only",
            "status": "passed",
            "evidence": governed_run_evidence_paths_json.clone()
        }),
        serde_json::json!({
            "id": "factory-project-run-readback-replacement-packet-accepted",
            "status": "passed",
            "evidence": factory_project_run_summary_paths_json.clone()
        }),
    ];
    if let Some(path) = replacement_smoke_gate_path {
        checks.push(serde_json::json!({
            "id": "replacement-smoke-gate-accepted",
            "status": "passed",
            "evidence": path.display().to_string()
        }));
    } else {
        checks.push(serde_json::json!({
            "id": "replacement-smoke-gate-superseded",
            "status": "passed",
            "evidence": governed_run_evidence_paths_json.clone()
        }));
    }
    if let Some(provider_acceptance) = &provider_acceptance_preservation {
        checks.push(serde_json::json!({
            "id": "provider-acceptance-preservation-verified",
            "status": "passed",
            "evidence": json_string(provider_acceptance, "path")
        }));
    }
    let governed_run_accepted_os = governed_run_evidence
        .get("accepted_os")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let checklist = serde_json::json!({
        "schema": "factory-v3/ao2-phase1-promotion-checklist/v1",
        "schema_version": "ao2.phase1-promotion-checklist.v1",
        "status": "passed",
        "phase1_state": "phase1_candidate_ready",
        "next_action": "publish signed Phase 1 promotion decision",
        "created_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "checklist": {
            "provider_readiness": {
                "status": "superseded_by_live_acceptance",
                "phase1_state": "passed",
                "evidence": provider_acceptance_preservation
                    .as_ref()
                    .map(|provider_acceptance| json_string(provider_acceptance, "path"))
                    .or_else(|| replacement_smoke_gate_path.map(|path| path.display().to_string()))
                    .unwrap_or_else(|| "governed-run-evidence".to_string())
            },
            "live_provider_acceptance": {
                "status": if provider_acceptance_preservation.is_some() { "passed" } else { "not_required_for_legacy_phase1_decision" },
                "state": if provider_acceptance_preservation.is_some() { "live_acceptance_complete" } else { "legacy_decision_without_preservation_artifact" },
                "providers": provider_acceptance_preservation
                    .as_ref()
                    .map(|_| serde_json::json!(["codex", "claude", "antigravity"]))
                    .unwrap_or_else(|| serde_json::json!([]))
            },
            "release_gate": {
                "status": "passed",
                "state": "verified",
                "path": release_gate_path.display().to_string(),
                "sha256": release_gate_sha256
            },
            "three_os_smoke": {
                "status": if replacement_smoke_gate_path.is_some() { "passed" } else { "superseded_by_governed_run" },
                "state": if replacement_smoke_gate_path.is_some() { "accepted" } else { "not_required_governed_run_is_primary" },
                "path": replacement_smoke_gate_path.map(|path| path.display().to_string()),
                "sha256": replacement_smoke_gate_sha256,
                "accepted_os": replacement_smoke_gate
                    .as_ref()
                    .and_then(|gate| gate.get("accepted_os").cloned())
                    .unwrap_or_else(|| serde_json::json!([]))
            },
            "three_os_governed_run": {
                "status": "passed",
                "state": "accepted",
                "paths": governed_run_evidence_paths_json.clone(),
                "accepted_os": governed_run_accepted_os.clone(),
                "verification": governed_run_evidence.clone()
            },
            "factory_project_run_readback": {
                "status": "passed",
                "state": "accepted",
                "paths": factory_project_run_summary_paths_json.clone(),
                "accepted_os": factory_project_run_readback["accepted_os"].clone(),
                "verification": factory_project_run_readback.clone()
            }
        },
        "release_gate": {
            "path": release_gate_path.display().to_string(),
            "sha256": release_gate_sha256,
            "status": json_string(&release_gate, "status"),
            "schema": json_string(&release_gate, "schema"),
            "replacement_smoke_gate_verification": release_gate_replacement,
            "governed_run_evidence_verification": release_gate_governed_run,
            "factory_project_run_readback_verification": release_gate_project_run_readback
        },
        "replacement_smoke_gate": {
            "path": replacement_smoke_gate_path.map(|path| path.display().to_string()),
            "sha256": replacement_smoke_gate_sha256,
            "schema_version": replacement_smoke_gate
                .as_ref()
                .map(|gate| json_string(gate, "schema_version"))
                .unwrap_or_default(),
            "status": replacement_smoke_gate
                .as_ref()
                .map(|gate| json_string(gate, "status"))
                .unwrap_or_else(|| "superseded_by_governed_run".to_string()),
            "accepted_os": replacement_smoke_gate
                .as_ref()
                .and_then(|gate| gate.get("accepted_os").cloned())
                .unwrap_or_else(|| serde_json::json!([])),
            "missing_os": replacement_smoke_gate
                .as_ref()
                .and_then(|gate| gate.get("missing_os").cloned())
                .unwrap_or_else(|| serde_json::json!([]))
        },
        "three_os_governed_run": {
            "status": "passed",
            "state": "accepted",
            "paths": governed_run_evidence_paths_json,
            "accepted_os": governed_run_accepted_os,
            "verification": governed_run_evidence
        },
        "factory_project_run_readback": {
            "status": "passed",
            "state": "accepted",
            "paths": factory_project_run_summary_paths_json,
            "accepted_os": factory_project_run_readback["accepted_os"].clone(),
            "verification": factory_project_run_readback
        },
        "provider_acceptance_preservation": provider_acceptance_preservation,
        "checks": checks,
        "trust_boundary": {
            "ao2_decision_owner": "ao2-native-phase1-promotion-decision-builder",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    atomic_write_text(&checklist_path, &serde_json::to_string_pretty(&checklist)?)?;
    let checklist_file_sha256 = sha256_file(&checklist_path)?;
    let checklist_sha256 = canonical_json_sha256(&checklist);
    let mut artifacts = serde_json::Map::new();
    artifacts.insert(
        "phase1_promotion_checklist".to_string(),
        serde_json::json!(checklist_path.display().to_string()),
    );
    artifacts.insert(
        "release_gate".to_string(),
        serde_json::json!(release_gate_path.display().to_string()),
    );
    artifacts.insert(
        "replacement_smoke_gate".to_string(),
        replacement_smoke_gate_path
            .map(|path| serde_json::json!(path.display().to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    artifacts.insert(
        "governed_run_evidence".to_string(),
        serde_json::json!(governed_run_evidence_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()),
    );
    artifacts.insert(
        "factory_project_run_readback".to_string(),
        serde_json::json!(factory_project_run_summary_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()),
    );
    if let Some(path) = provider_acceptance_preservation_path {
        artifacts.insert(
            "provider_acceptance_preservation".to_string(),
            serde_json::json!(path.display().to_string()),
        );
    }
    let decision = serde_json::json!({
        "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
        "status": "passed",
        "decision": "promote_phase1_candidate",
        "phase1_state": "phase1_candidate_ready",
        "checklist_sha256": checklist_sha256,
        "checklist_file_sha256": checklist_file_sha256,
        "operator": operator,
        "rationale": rationale,
        "artifacts": serde_json::Value::Object(artifacts),
        "trust_boundary": {
            "ao2_decision_owner": "ao2-native-phase1-promotion-decision-builder",
            "factory_v3_role": "parity_oracle_only",
            "control_plane_role": "read_only_observer_after_signed_evidence",
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    });
    atomic_write_text(out, &serde_json::to_string_pretty(&decision)?)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.phase1-promotion-decision-build.v1",
        "status": "written",
        "decision_path": out.display().to_string(),
        "checklist_path": checklist_path.display().to_string(),
        "decision": decision,
        "checklist": checklist
    }))
}

fn provider_acceptance_preservation_verification_json(path: &Path) -> Result<serde_json::Value> {
    let summary = read_factory_compat_value(path)
        .with_context(|| format!("read provider acceptance preservation {}", path.display()))?;
    if summary["schema"] != "ao2.provider-pilot-acceptance-preservation.v1" {
        return Err(anyhow!(
            "provider acceptance preservation schema must be ao2.provider-pilot-acceptance-preservation.v1"
        ));
    }
    if summary["status"] != "passed" {
        return Err(anyhow!(
            "provider acceptance preservation status must be passed"
        ));
    }
    let providers = summary
        .get("providers")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("provider acceptance preservation providers must be an object"))?;
    let required = [
        ("codex", "ao2.codex-provider-pilot-acceptance.v1"),
        ("claude", "ao2.claude-provider-pilot-acceptance.v1"),
        (
            "antigravity",
            "ao2.antigravity-provider-pilot-acceptance.v1",
        ),
    ];
    let mut provider_summaries = serde_json::Map::new();
    for (provider, schema) in required {
        let entry = providers
            .get(provider)
            .ok_or_else(|| anyhow!("provider acceptance preservation missing {provider}"))?;
        if json_string(entry, "schema_version") != schema {
            return Err(anyhow!(
                "provider acceptance preservation {provider} schema_version must be {schema}"
            ));
        }
        if json_string(entry, "source_class") != "live" {
            return Err(anyhow!(
                "provider acceptance preservation {provider} source_class must be live"
            ));
        }
        if json_u64(entry, "smoke_score") < json_u64(entry, "minimum_score").max(90) {
            return Err(anyhow!(
                "provider acceptance preservation {provider} score below minimum"
            ));
        }
        if json_string(entry, "replay_status") != "accepted" {
            return Err(anyhow!(
                "provider acceptance preservation {provider} replay_status must be accepted"
            ));
        }
        if json_u64(entry, "digest_failures") != 0 {
            return Err(anyhow!(
                "provider acceptance preservation {provider} digest_failures must be zero"
            ));
        }
        provider_summaries.insert(
            provider.to_string(),
            serde_json::json!({
                "schema_version": schema,
                "source_class": json_string(entry, "source_class"),
                "run_id": json_string(entry, "run_id"),
                "smoke_score": json_u64(entry, "smoke_score"),
                "minimum_score": json_u64(entry, "minimum_score"),
                "replay_status": json_string(entry, "replay_status"),
                "digest_failures": json_u64(entry, "digest_failures"),
                "preserved": json_string(entry, "preserved")
            }),
        );
    }
    Ok(serde_json::json!({
        "schema": "ao2.phase1-provider-acceptance-preservation-verification.v1",
        "status": "verified",
        "path": path.display().to_string(),
        "sha256": sha256_file(path)?,
        "tag": json_string(&summary, "tag"),
        "providers": ["codex", "claude", "antigravity"],
        "provider_summaries": provider_summaries
    }))
}

pub(crate) fn phase1_promotion_decision_publish_to_control_plane_json(
    decision_path: &Path,
    signing_key: &Path,
    signer_id: &str,
    control_plane_url: &str,
    api_token: &str,
) -> Result<serde_json::Value> {
    let api_token = trimmed_required("--api-token", api_token)?;
    let signer_id = trimmed_required("--signer-id", signer_id)?;
    let content = fs::read_to_string(decision_path)
        .with_context(|| format!("read {}", decision_path.display()))?;
    let decision: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse {}", decision_path.display()))?;
    let schema = json_string(&decision, "schema");
    if schema != "factory-v3/ao2-phase1-promotion-decision/v1" {
        return Err(anyhow!(
            "Phase 1 promotion decision publish requires factory-v3/ao2-phase1-promotion-decision/v1, got {schema}"
        ));
    }
    let decision_value = json_string(&decision, "decision");
    if decision_value != "promote_phase1_candidate" {
        return Err(anyhow!(
            "Phase 1 promotion decision must be promote_phase1_candidate, got {decision_value}"
        ));
    }
    let checklist_publish = publish_phase1_promotion_checklist_for_decision(
        control_plane_url,
        &api_token,
        decision_path,
        &decision,
    )?;
    let decision_raw = serde_json::to_string_pretty(&decision)?;
    let signature_bytes = sign_bytes_with_private_key(signing_key, decision_raw.as_bytes())?;
    let public_key_pem = public_key_pem_from_private_key(signing_key)?;
    let signature = serde_json::json!({
        "schema_version": "ao2.cp-phase1-promotion-decision-signature.v1",
        "signature_algorithm": "RSA/SHA-256",
        "signer_id": signer_id,
        "signature_sha256": sha256_bytes_hex(&signature_bytes),
        "signature_hex": hex_lower(&signature_bytes),
        "public_key_sha256": sha256_bytes_hex(public_key_pem.as_bytes()),
        "public_key_pem": public_key_pem
    });
    let endpoint = control_plane_endpoint(
        control_plane_url,
        "/api/v1/phase1/promotion/decision/signed",
    )?;
    let post_body = serde_json::to_string(&serde_json::json!({
        "schema_version": "ao2.cp-phase1-promotion-decision-signed-upload.v1",
        "decision": decision,
        "decision_b64": base64_standard(decision_raw.as_bytes()),
        "signature": signature
    }))?;
    let receipt = post_json_http(&endpoint, &api_token, &post_body)?;
    let receipt_sha = json_string(&receipt, "sha256");
    let detail_url = if receipt_sha.is_empty() {
        String::new()
    } else {
        control_plane_endpoint(
            control_plane_url,
            &format!("/api/v1/phase1/promotion/decision/{receipt_sha}"),
        )?
    };
    let signature_url = if receipt_sha.is_empty() {
        String::new()
    } else {
        control_plane_endpoint(
            control_plane_url,
            &format!("/api/v1/phase1/promotion/decision/{receipt_sha}/signature"),
        )?
    };
    let dashboard_url =
        control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/dashboard")?;
    Ok(serde_json::json!({
        "schema_version": "ao2.phase1-promotion-decision-control-plane-publish.v1",
        "decision_path": decision_path,
        "endpoint": endpoint,
        "detail_url": detail_url,
        "signature_url": signature_url,
        "dashboard_url": dashboard_url,
        "checklist_publish": checklist_publish,
        "signature": signature,
        "signed": true,
        "receipt": receipt
    }))
}

pub(crate) fn phase1_three_os_smoke_build_json(
    summary_path: &Path,
    provenance_path: &Path,
    out: &Path,
) -> Result<serde_json::Value> {
    let summary_text = fs::read_to_string(summary_path)
        .with_context(|| format!("read {}", summary_path.display()))?;
    let summary: serde_json::Value = serde_json::from_str(&summary_text)
        .with_context(|| format!("parse {}", summary_path.display()))?;
    let verification = release_smoke_summary_verification_json(summary_path, &summary, true);
    if json_string(&verification, "status") != "verified" {
        return Err(anyhow!(
            "Phase 1 three-OS smoke summary is not verified: {}",
            serde_json::to_string(&verification["reasons"])?
        ));
    }
    if json_string(&summary, "linux_x86_64_remote_smoke") != "passed" {
        return Err(anyhow!(
            "Phase 1 three-OS smoke requires linux_x86_64_remote_smoke=passed"
        ));
    }
    if summary
        .get("native_windows_required")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err(anyhow!(
            "Phase 1 three-OS smoke requires native_windows_required=true"
        ));
    }

    let provenance_text = fs::read_to_string(provenance_path)
        .with_context(|| format!("read {}", provenance_path.display()))?;
    let provenance: serde_json::Value = serde_json::from_str(&provenance_text)
        .with_context(|| format!("parse {}", provenance_path.display()))?;
    let mut provenance_schema = json_string(&provenance, "schema_version");
    if provenance_schema.is_empty() {
        provenance_schema = json_string(&provenance, "schema");
    }
    if provenance_schema != "ao2.release-provenance.v1" {
        return Err(anyhow!(
            "release provenance schema must be ao2.release-provenance.v1, got {provenance_schema}"
        ));
    }
    let version = trimmed_required(
        "release provenance version",
        &json_string(&provenance, "version"),
    )?;
    let source_commit = trimmed_required(
        "release provenance git_commit",
        &json_string(&provenance, "git_commit"),
    )?;
    if !is_git_sha40(&source_commit) {
        return Err(anyhow!(
            "release provenance git_commit must be a 40-char lowercase hex git sha1"
        ));
    }
    let source_dirty = provenance
        .get("source_dirty")
        .or_else(|| provenance.get("git_dirty"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if source_dirty {
        return Err(anyhow!(
            "Phase 1 three-OS smoke publish requires clean release provenance"
        ));
    }

    let smoke_root = json_string(&summary, "root");
    let root_path = if smoke_root.is_empty() {
        summary_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .display()
            .to_string()
    } else {
        smoke_root
    };
    let report = json_string(&summary, "report");
    let report_path = if report.is_empty() {
        PathBuf::from(&root_path)
            .join("report.md")
            .display()
            .to_string()
    } else {
        report
    };
    let local_log = PathBuf::from(&root_path)
        .join("local-smoke.log")
        .display()
        .to_string();
    let windows_log = {
        let value = json_string(&summary, "windows_log");
        if value.is_empty() {
            PathBuf::from(&root_path)
                .join("windows-smoke.log")
                .display()
                .to_string()
        } else {
            value
        }
    };

    let smoke = serde_json::json!({
        "schema": "ao2-control-plane.three-os-release-smoke.v1",
        "version": version,
        "release_candidate_version": version,
        "status": "passed",
        "source_commit": source_commit,
        "source_dirty": false,
        "report": report_path,
        "root": root_path,
        "remote_command_files": {
            "ubuntu": "scripts/smoke-linux-release-remote.sh",
            "windows": "scripts/smoke-windows-release.ps1"
        },
        "rerun_commands": {
            "all_required": "AO2_REQUIRE_NATIVE_WINDOWS_SMOKE=1 AO2_PHASE1_CP_TOKEN=<local-token> npm run smoke:three-os",
            "phase1_publish": "AO2_PHASE1_PROMOTION_PUBLISH=1 AO2_PHASE1_API_TOKEN_ENV=AO2_PHASE1_CP_TOKEN scripts/phase1-replacement-promotion.sh"
        },
        "targets": {
            "macos": {
                "status": "passed",
                "log": local_log
            },
            "ubuntu": {
                "status": "passed",
                "log": local_log
            },
            "windows": {
                "status": "passed",
                "log": windows_log
            }
        }
    });
    atomic_write_text(out, &serde_json::to_string_pretty(&smoke)?)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.phase1-three-os-smoke-build.v1",
        "status": "written",
        "summary_path": summary_path.display().to_string(),
        "provenance_path": provenance_path.display().to_string(),
        "smoke_path": out.display().to_string(),
        "smoke_sha256": sha256_file(out)?,
        "smoke": smoke
    }))
}

pub(crate) fn phase1_three_os_smoke_publish_to_control_plane_json(
    smoke_path: &Path,
    control_plane_url: &str,
    api_token: Option<&str>,
    api_token_env: Option<&str>,
) -> Result<serde_json::Value> {
    let api_token = resolve_api_token(api_token, api_token_env)?;
    let smoke_text =
        fs::read_to_string(smoke_path).with_context(|| format!("read {}", smoke_path.display()))?;
    let smoke: serde_json::Value = serde_json::from_str(&smoke_text)
        .with_context(|| format!("parse {}", smoke_path.display()))?;
    let mut schema = json_string(&smoke, "schema");
    if schema.is_empty() {
        schema = json_string(&smoke, "schema_version");
    }
    if schema != "ao2-control-plane.three-os-release-smoke.v1" {
        return Err(anyhow!(
            "Phase 1 three-OS smoke publish requires ao2-control-plane.three-os-release-smoke.v1, got {schema}"
        ));
    }
    if json_string(&smoke, "status") != "passed" {
        return Err(anyhow!(
            "Phase 1 three-OS smoke publish requires status=passed"
        ));
    }
    if smoke
        .get("source_dirty")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        return Err(anyhow!(
            "Phase 1 three-OS smoke publish requires source_dirty=false"
        ));
    }
    let source_commit = json_string(&smoke, "source_commit");
    if !is_git_sha40(&source_commit) {
        return Err(anyhow!(
            "Phase 1 three-OS smoke source_commit must be a 40-char lowercase hex git sha1"
        ));
    }
    let targets = smoke
        .get("targets")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("Phase 1 three-OS smoke missing targets object"))?;
    for target in ["macos", "ubuntu", "windows"] {
        let status = targets
            .get(target)
            .map(|value| json_string(value, "status"))
            .unwrap_or_default();
        if status != "passed" {
            return Err(anyhow!(
                "Phase 1 three-OS smoke publish requires {target} status=passed"
            ));
        }
    }

    let endpoint =
        control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/three-os-smoke")?;
    let post_body = serde_json::to_string(&smoke)?;
    let receipt = post_json_http(&endpoint, &api_token, &post_body)?;
    let receipt_sha = json_string(&receipt, "sha256");
    let detail_url = if receipt_sha.is_empty() {
        String::new()
    } else {
        control_plane_endpoint(
            control_plane_url,
            &format!("/api/v1/phase1/promotion/three-os-smoke/{receipt_sha}"),
        )?
    };
    let latest_url = control_plane_endpoint(
        control_plane_url,
        "/api/v1/phase1/promotion/three-os-smoke/latest",
    )?;
    let dashboard_url =
        control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/dashboard")?;
    Ok(serde_json::json!({
        "schema_version": "ao2.phase1-three-os-smoke-control-plane-publish.v1",
        "smoke_path": smoke_path.display().to_string(),
        "endpoint": endpoint,
        "detail_url": detail_url,
        "latest_url": latest_url,
        "dashboard_url": dashboard_url,
        "receipt": receipt
    }))
}

fn publish_phase1_promotion_checklist_for_decision(
    control_plane_url: &str,
    api_token: &str,
    decision_path: &Path,
    decision: &serde_json::Value,
) -> Result<serde_json::Value> {
    let checklist_ref = decision
        .get("artifacts")
        .and_then(|artifacts| artifacts.get("phase1_promotion_checklist"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if checklist_ref.is_empty() {
        return Ok(serde_json::json!({
            "status": "skipped",
            "reason": "decision_artifacts_missing_phase1_promotion_checklist"
        }));
    }

    let checklist_path = resolve_phase1_promotion_checklist_path(decision_path, checklist_ref);
    if !checklist_path.is_file() {
        return Ok(serde_json::json!({
            "status": "skipped",
            "reason": "phase1_promotion_checklist_not_found",
            "reference": checklist_ref,
            "resolved_path": checklist_path.display().to_string()
        }));
    }

    let checklist_raw = fs::read_to_string(&checklist_path)
        .with_context(|| format!("read {}", checklist_path.display()))?;
    let checklist: serde_json::Value = serde_json::from_str(&checklist_raw)
        .with_context(|| format!("parse {}", checklist_path.display()))?;
    let expected_sha = json_string(decision, "checklist_sha256");
    let actual_sha = canonical_json_sha256(&checklist);
    if !expected_sha.is_empty() && expected_sha != actual_sha {
        return Err(anyhow!(
            "Phase 1 promotion decision checklist_sha256 does not match referenced checklist canonical sha256: expected {expected_sha}, got {actual_sha}"
        ));
    }

    let endpoint = control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/checklist")?;
    let receipt = post_json_http(&endpoint, api_token, &serde_json::to_string(&checklist)?)?;
    Ok(serde_json::json!({
        "status": "posted",
        "path": checklist_path.display().to_string(),
        "endpoint": endpoint,
        "canonical_sha256": actual_sha,
        "receipt": receipt
    }))
}

fn resolve_phase1_promotion_checklist_path(decision_path: &Path, checklist_ref: &str) -> PathBuf {
    let candidate = PathBuf::from(checklist_ref);
    if candidate.is_absolute() || candidate.is_file() {
        return candidate;
    }
    let sibling = decision_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&candidate);
    if sibling.is_file() {
        return sibling;
    }
    candidate
}

pub(crate) fn phase1_promotion_history_fetch_from_control_plane_json(
    control_plane_url: &str,
    api_token: Option<&str>,
    api_token_env: Option<&str>,
    out: Option<&Path>,
) -> Result<serde_json::Value> {
    let api_token = resolve_api_token(api_token, api_token_env)?;
    let endpoint =
        control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/history.json")?;
    let history = get_json_http(&endpoint, &api_token)?;
    let schema_version = json_string(&history, "schema_version");
    if schema_version != "ao2.cp-phase1-promotion-history.v1" {
        return Err(anyhow!(
            "Phase 1 promotion history fetch requires ao2.cp-phase1-promotion-history.v1, got {schema_version}"
        ));
    }
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        atomic_write_text(path, &serde_json::to_string_pretty(&history)?)?;
    }
    Ok(serde_json::json!({
        "schema_version": "ao2.phase1-promotion-history-control-plane-fetch.v1",
        "endpoint": endpoint,
        "dashboard_url": control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/dashboard")?,
        "gap_report_url": control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/gap-report.json")?,
        "out": out.map(|path| path.display().to_string()),
        "history": history,
        "trust_boundary": {
            "role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        }
    }))
}

pub(crate) fn phase1_promotion_status_json(
    root: &Path,
    evidence_bundle: Option<&Path>,
) -> Result<serde_json::Value> {
    let release_gate_path = root.join("release-gate.json");
    let decision_path = root.join("phase1-promotion-decision.json");
    let checklist_path = root.join("phase1-promotion-checklist.json");
    let promotion_inputs_verification_path = root.join("promotion-inputs-verification.json");
    let verification_path = root.join("phase1-evidence-bundle-verification.json");
    let dashboard_snapshot_manifest_path = phase1_dashboard_snapshot_manifest_path(root);
    let mut failures = Vec::new();

    let release_gate = read_json_for_verification(&release_gate_path, &mut failures)
        .unwrap_or(serde_json::Value::Null);
    let decision = read_json_for_verification(&decision_path, &mut failures)
        .unwrap_or(serde_json::Value::Null);
    let checklist = read_json_for_verification(&checklist_path, &mut failures)
        .unwrap_or(serde_json::Value::Null);
    let promotion_inputs_verification =
        read_json_for_verification(&promotion_inputs_verification_path, &mut failures)
            .unwrap_or(serde_json::Value::Null);

    let release_gate_status = json_string(&release_gate, "status");
    if release_gate_status != "verified" {
        failures.push(serde_json::json!({
            "code": "release_gate_not_verified",
            "path": release_gate_path,
            "actual": release_gate_status
        }));
    }
    let decision_value = json_string(&decision, "decision");
    if decision_value != "promote_phase1_candidate" {
        failures.push(serde_json::json!({
            "code": "phase1_decision_not_promote",
            "path": decision_path,
            "actual": decision_value
        }));
    }
    let checklist_status = json_string(&checklist, "status");
    if checklist_status != "passed" {
        failures.push(serde_json::json!({
            "code": "phase1_checklist_not_passed",
            "path": checklist_path,
            "actual": checklist_status
        }));
    }
    let promotion_inputs_schema = json_string(&promotion_inputs_verification, "schema_version");
    let promotion_inputs_status = json_string(&promotion_inputs_verification, "status");
    let promotion_inputs_check = if promotion_inputs_status.is_empty() {
        "missing".to_string()
    } else {
        promotion_inputs_status.clone()
    };
    let promotion_inputs_failure_count = json_u64(&promotion_inputs_verification, "failure_count");
    if promotion_inputs_schema != "ao2.phase1-replacement-promotion-inputs-verification.v1"
        || promotion_inputs_status != "accepted"
        || promotion_inputs_failure_count != 0
    {
        failures.push(serde_json::json!({
            "code": "phase1_promotion_inputs_not_verified",
            "path": promotion_inputs_verification_path,
            "actual_schema_version": promotion_inputs_schema,
            "actual_status": promotion_inputs_status,
            "actual_failure_count": promotion_inputs_failure_count
        }));
    }
    let promotion_inputs_trust_boundary = promotion_inputs_verification
        .get("trust_boundary")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    for (field, expected) in [
        ("control_plane_role", "read_only_observer"),
        ("release_acceptance_owner", "factory-v3 evaluator-closer"),
    ] {
        let actual = json_string(&promotion_inputs_trust_boundary, field);
        if actual != expected {
            failures.push(serde_json::json!({
                "code": "phase1_promotion_inputs_trust_boundary_mismatch",
                "path": promotion_inputs_verification_path,
                "field": field,
                "expected": expected,
                "actual": actual
            }));
        }
    }
    for (field, expected) in [
        ("mutates_ao_artifacts", false),
        ("control_plane_approves_release", false),
    ] {
        let actual = promotion_inputs_trust_boundary
            .get(field)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if actual != expected {
            failures.push(serde_json::json!({
                "code": "phase1_promotion_inputs_trust_boundary_mismatch",
                "path": promotion_inputs_verification_path,
                "field": field,
                "expected": expected,
                "actual": actual
            }));
        }
    }

    let evidence_bundle_path = evidence_bundle
        .map(PathBuf::from)
        .or_else(|| latest_phase1_evidence_bundle_archive(root));
    let evidence_bundle_report = if let Some(bundle_path) = evidence_bundle_path.as_deref() {
        release_evidence_bundle_verification_json(bundle_path)?
    } else {
        failures.push(serde_json::json!({
            "code": "phase1_evidence_bundle_missing",
            "message": format!("no Phase 1 evidence bundle archive found under {}", root.display())
        }));
        serde_json::json!({
            "schema_version": "ao2.release-evidence-bundle-verification.v1",
            "status": "missing",
            "failure_count": 1,
            "trust_boundary_verified": false,
            "secret_scan_passed": false
        })
    };
    if json_string(&evidence_bundle_report, "status") != "verified" {
        failures.push(serde_json::json!({
            "code": "phase1_evidence_bundle_not_verified",
            "path": evidence_bundle_path.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
            "actual": json_string(&evidence_bundle_report, "status")
        }));
    }
    let dashboard_snapshot =
        phase1_dashboard_snapshot_status_json(&dashboard_snapshot_manifest_path)?;
    let dashboard_snapshot_index = json_string(&dashboard_snapshot, "index");
    let dashboard_snapshot_status = json_string(&dashboard_snapshot, "status");

    let status = if failures.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.phase1-promotion-status.v1",
        "status": status,
        "root": root,
        "artifacts": {
            "release_gate": release_gate_path,
            "decision": decision_path,
            "checklist": checklist_path,
            "promotion_inputs_verification": promotion_inputs_verification_path,
            "evidence_bundle": evidence_bundle_path,
            "evidence_bundle_verification": verification_path,
            "dashboard_snapshot_manifest": dashboard_snapshot_manifest_path,
            "dashboard_snapshot_index": dashboard_snapshot_index
        },
        "checks": {
            "release_gate": release_gate_status,
            "decision": decision_value,
            "checklist": checklist_status,
            "promotion_inputs": promotion_inputs_check,
            "evidence_bundle": json_string(&evidence_bundle_report, "status"),
            "dashboard_snapshot": dashboard_snapshot_status
        },
        "promotion_inputs_verification": promotion_inputs_verification,
        "evidence_bundle_verification": evidence_bundle_report,
        "control_plane_dashboard_snapshot": dashboard_snapshot,
        "failure_count": failures.len(),
        "failures": failures,
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        }
    }))
}

fn phase1_dashboard_snapshot_status_json(manifest_path: &Path) -> Result<serde_json::Value> {
    if !manifest_path.exists() {
        return Ok(serde_json::json!({
            "status": "missing",
            "manifest": manifest_path,
            "index": serde_json::Value::Null,
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false
            }
        }));
    }
    let manifest = read_factory_compat_value(manifest_path).with_context(|| {
        format!(
            "read dashboard snapshot manifest {}",
            manifest_path.display()
        )
    })?;
    let snapshot_root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let index_path = snapshot_root.join("index.html");
    let schema_version = json_string(&manifest, "schema_version");
    let token_in_output = manifest
        .get("token_in_output")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let surfaces = manifest
        .get("surfaces")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let trust_boundary = manifest.get("trust_boundary").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        })
    });
    let manifest_sha256 = sha256_file(manifest_path)?;
    let index_sha256 = if index_path.exists() {
        serde_json::Value::String(sha256_file(&index_path)?)
    } else {
        serde_json::Value::Null
    };
    let status = if schema_version == "ao2.cp-dashboard-snapshot.v1"
        && !token_in_output
        && index_path.exists()
    {
        "available"
    } else {
        "attention"
    };
    Ok(serde_json::json!({
        "status": status,
        "schema_version": schema_version,
        "manifest": manifest_path,
        "index": index_path,
        "manifest_sha256": manifest_sha256,
        "index_sha256": index_sha256,
        "surface_count": surfaces,
        "token_in_output": token_in_output,
        "base_url": json_string(&manifest, "base_url"),
        "trust_boundary": trust_boundary
    }))
}

fn phase1_dashboard_snapshot_manifest_path(root: &Path) -> PathBuf {
    let in_root = root
        .join("control-plane-dashboard-snapshot")
        .join("manifest.json");
    if in_root.exists() {
        return in_root;
    }
    if let Some(parent) = root.parent() {
        let sibling = parent
            .join("control-plane-dashboard-snapshot")
            .join("manifest.json");
        if sibling.exists() {
            return sibling;
        }
    }
    in_root
}

pub(crate) fn phase1_promotion_inputs_verify_json(
    manifest_path: &Path,
    out: Option<&Path>,
    mode: &str,
) -> Result<serde_json::Value> {
    let mode = mode.trim().replace('-', "_");
    if !matches!(mode.as_str(), "preflight" | "decision_gate") {
        return Err(anyhow!(
            "--mode must be preflight or decision-gate, got {mode}"
        ));
    }
    let manifest = read_factory_compat_value(manifest_path)
        .with_context(|| format!("read promotion inputs manifest {}", manifest_path.display()))?;
    if json_string(&manifest, "schema_version") != "ao2.phase1-replacement-promotion-inputs.v1" {
        return Err(anyhow!(
            "promotion inputs schema_version must be ao2.phase1-replacement-promotion-inputs.v1"
        ));
    }

    let mut missing_required_inputs = Vec::new();
    let mut failures = Vec::new();
    let trust_boundary = manifest
        .get("trust_boundary")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    for (field, expected) in [
        ("control_plane_role", "read_only_observer"),
        ("release_acceptance_owner", "factory-v3 evaluator-closer"),
    ] {
        let actual = json_string(&trust_boundary, field);
        if actual != expected {
            failures.push(serde_json::json!({
                "code": "trust_boundary_mismatch",
                "field": field,
                "expected": expected,
                "actual": actual
            }));
        }
    }
    for (field, expected) in [
        ("mutates_ao_artifacts", false),
        ("control_plane_approves_release", false),
    ] {
        let actual = trust_boundary
            .get(field)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if actual != expected {
            failures.push(serde_json::json!({
                "code": "trust_boundary_mismatch",
                "field": field,
                "expected": expected,
                "actual": actual
            }));
        }
    }

    let inputs = manifest
        .get("inputs")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("promotion inputs manifest inputs must be an object"))?;
    let outputs = manifest
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("promotion inputs manifest outputs must be an object"))?;
    let replacement_smoke_mode = json_string(&manifest, "replacement_smoke_mode");
    if !matches!(
        replacement_smoke_mode.as_str(),
        "governed_run_primary" | "legacy_replacement_smoke_bound"
    ) {
        failures.push(serde_json::json!({
            "code": "replacement_smoke_mode_invalid",
            "actual": replacement_smoke_mode
        }));
    }

    phase1_record_required_platform_inputs(
        "governed_run_evidence",
        inputs.get("governed_run_evidence"),
        &mut missing_required_inputs,
    );
    phase1_record_required_platform_inputs(
        "factory_project_run_summary",
        inputs.get("factory_project_run_summary"),
        &mut missing_required_inputs,
    );
    phase1_record_required_file_input(
        "provider_acceptance_preservation",
        inputs.get("provider_acceptance_preservation"),
        &mut missing_required_inputs,
    );
    if replacement_smoke_mode == "legacy_replacement_smoke_bound" {
        phase1_record_required_platform_inputs(
            "replacement_smoke",
            inputs.get("replacement_smoke"),
            &mut missing_required_inputs,
        );
    }
    if mode == "decision_gate" {
        phase1_record_required_file_input(
            "release_gate",
            outputs.get("release_gate"),
            &mut missing_required_inputs,
        );
        if replacement_smoke_mode == "legacy_replacement_smoke_bound" {
            phase1_record_required_file_input(
                "replacement_smoke_gate",
                outputs.get("replacement_smoke_gate"),
                &mut missing_required_inputs,
            );
        }
    }

    let status = if missing_required_inputs.is_empty() && failures.is_empty() {
        "accepted"
    } else {
        "rejected"
    };
    let failure_count = missing_required_inputs.len() + failures.len();
    let report = serde_json::json!({
        "schema_version": "ao2.phase1-replacement-promotion-inputs-verification.v1",
        "status": status,
        "mode": mode,
        "manifest_path": manifest_path,
        "missing_required_inputs": missing_required_inputs,
        "failure_count": failure_count,
        "failures": failures,
        "trust_boundary": trust_boundary
    });
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create parent dir {}", parent.display()))?;
            }
        }
        atomic_write_text(path, &serde_json::to_string_pretty(&report)?)?;
    }
    Ok(report)
}

pub(crate) fn phase1_promotion_inputs_publish_to_control_plane_json(
    verification_path: &Path,
    control_plane_url: &str,
    api_token: Option<&str>,
    api_token_env: Option<&str>,
) -> Result<serde_json::Value> {
    let api_token = resolve_api_token(api_token, api_token_env)?;
    let report_text = fs::read_to_string(verification_path)
        .with_context(|| format!("read {}", verification_path.display()))?;
    let report: serde_json::Value = serde_json::from_str(&report_text)
        .with_context(|| format!("parse {}", verification_path.display()))?;
    let schema = json_string(&report, "schema_version");
    if schema != "ao2.phase1-replacement-promotion-inputs-verification.v1" {
        return Err(anyhow!(
            "Phase 1 promotion inputs publish requires ao2.phase1-replacement-promotion-inputs-verification.v1, got {schema}"
        ));
    }
    if json_string(&report, "status") != "accepted" {
        return Err(anyhow!(
            "Phase 1 promotion inputs publish requires status=accepted"
        ));
    }
    if json_u64(&report, "failure_count") != 0 {
        return Err(anyhow!(
            "Phase 1 promotion inputs publish requires failure_count=0"
        ));
    }
    let trust_boundary = report
        .get("trust_boundary")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    for (field, expected) in [
        ("control_plane_role", "read_only_observer"),
        ("release_acceptance_owner", "factory-v3 evaluator-closer"),
    ] {
        let actual = json_string(&trust_boundary, field);
        if actual != expected {
            return Err(anyhow!(
                "Phase 1 promotion inputs publish requires trust_boundary.{field}={expected}, got {actual}"
            ));
        }
    }
    for (field, expected) in [
        ("mutates_ao_artifacts", false),
        ("control_plane_approves_release", false),
    ] {
        let actual = trust_boundary
            .get(field)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if actual != expected {
            return Err(anyhow!(
                "Phase 1 promotion inputs publish requires trust_boundary.{field}={expected}, got {actual}"
            ));
        }
    }

    let endpoint = control_plane_endpoint(
        control_plane_url,
        "/api/v1/phase1/promotion/inputs-verification",
    )?;
    let receipt = post_json_http(&endpoint, &api_token, &serde_json::to_string(&report)?)?;
    let receipt_sha = json_string(&receipt, "sha256");
    let detail_url = if receipt_sha.is_empty() {
        String::new()
    } else {
        control_plane_endpoint(
            control_plane_url,
            &format!("/api/v1/phase1/promotion/inputs-verification/{receipt_sha}"),
        )?
    };
    let latest_url = control_plane_endpoint(
        control_plane_url,
        "/api/v1/phase1/promotion/inputs-verification/latest",
    )?;
    let history_url =
        control_plane_endpoint(control_plane_url, "/api/v1/phase1/promotion/history.json")?;
    Ok(serde_json::json!({
        "schema_version": "ao2.phase1-promotion-inputs-control-plane-publish.v1",
        "verification_path": verification_path.display().to_string(),
        "endpoint": endpoint,
        "detail_url": detail_url,
        "latest_url": latest_url,
        "history_url": history_url,
        "receipt": receipt,
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        }
    }))
}

fn phase1_record_required_file_input(
    label: &str,
    value: Option<&serde_json::Value>,
    missing_required_inputs: &mut Vec<serde_json::Value>,
) {
    let path = value
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if path.is_empty() || !Path::new(path).is_file() {
        missing_required_inputs.push(serde_json::json!({
            "label": label,
            "path": path
        }));
    }
}

fn phase1_record_required_platform_inputs(
    group_label: &str,
    group: Option<&serde_json::Value>,
    missing_required_inputs: &mut Vec<serde_json::Value>,
) {
    let group = group.and_then(serde_json::Value::as_object);
    for platform in ["macos", "ubuntu", "windows"] {
        let path = group
            .and_then(|items| items.get(platform))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if path.is_empty() || !Path::new(path).is_file() {
            missing_required_inputs.push(serde_json::json!({
                "label": format!("{platform}_{group_label}"),
                "path": path
            }));
        }
    }
}

fn latest_phase1_evidence_bundle_archive(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut latest: Option<(std::time::SystemTime, PathBuf)> = None;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with("ao2-release-evidence-bundle-") && name.ends_with(".tar.gz") {
                let modified = metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                if latest
                    .as_ref()
                    .is_none_or(|(current_modified, _)| modified > *current_modified)
                {
                    latest = Some((modified, path));
                }
            }
        }
    }
    latest.map(|(_, path)| path)
}

fn is_git_sha40(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
