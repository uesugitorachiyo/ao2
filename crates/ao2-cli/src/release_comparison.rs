use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{
    canonical_json_sha256, create_tar_gz, json_array, json_bool, json_string, json_u64,
    read_json_file, sha256_file,
};
use crate::release_crypto::{
    derive_public_key_from_private_key, extract_tar_gz, sign_file_with_private_key,
    verify_file_signature,
};
use crate::release_history::workbench_release_history_for_dir;
use crate::release_support_bundle_ci::release_support_bundle_ci_evidence_index;
use crate::risky_pr_readback::report_contract_verification_json;
use crate::{atomic_write_text, now_unix_ms, runtime_git_commit, runtime_target_label};

pub(crate) fn release_compare(
    release_download_dir: PathBuf,
    out_dir: PathBuf,
    signing_key: Option<PathBuf>,
    signer_id: String,
    json: bool,
) -> Result<()> {
    let result = release_comparison_bundle_json(
        release_download_dir,
        out_dir,
        signing_key.as_deref(),
        &signer_id,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("release_comparison={}", json_string(&result, "bundle_path"));
        println!(
            "release_history={}",
            json_string(&result, "release_history_path")
        );
        println!(
            "latest_release={}",
            json_string(&result["release_history"]["trend"], "latest_release_tag")
        );
        println!(
            "regressions={}",
            json_u64(&result["release_history"]["trend"], "regression_count")
        );
        if result["support_metadata"]["present"]
            .as_bool()
            .unwrap_or(false)
        {
            println!(
                "signature_verified={}",
                result["support_metadata"]["signature_verified"]
                    .as_bool()
                    .unwrap_or(false)
            );
        }
    }
    Ok(())
}

pub(crate) fn release_comparison_bundle_json(
    release_download_dir: PathBuf,
    out_dir: PathBuf,
    signing_key: Option<&Path>,
    signer_id: &str,
) -> Result<serde_json::Value> {
    let release_history = workbench_release_history_for_dir(release_download_dir.clone())?;
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let generated_at_ms = now_unix_ms();
    let bundle_dir = out_dir.join(format!("release-comparison-{generated_at_ms}"));
    fs::create_dir_all(&bundle_dir).with_context(|| format!("create {}", bundle_dir.display()))?;
    let release_history_path = bundle_dir.join("release-history.json");
    let comparison_path = bundle_dir.join("release-comparison.json");
    let sha256_path = bundle_dir.join("SHA256SUMS");
    let mut files = vec![
        serde_json::json!({"path": "release-history.json", "role": "release_history"}),
        serde_json::json!({"path": "release-comparison.json", "role": "portable_bundle"}),
        serde_json::json!({"path": "SHA256SUMS", "role": "checksum_manifest"}),
    ];
    if signing_key.is_some() {
        files.push(serde_json::json!({
            "path": "release-comparison-metadata.json",
            "role": "support_metadata"
        }));
        files.push(serde_json::json!({
            "path": "release-comparison-metadata.json.sig",
            "role": "support_metadata_signature"
        }));
        files.push(serde_json::json!({
            "path": "release-comparison-signing-public.pem",
            "role": "support_metadata_public_key"
        }));
    }
    atomic_write_text(
        &release_history_path,
        &serde_json::to_string_pretty(&release_history)?,
    )?;
    let comparison = serde_json::json!({
        "schema_version": "ao2.release-comparison-bundle.v1",
        "generated_at_ms": generated_at_ms,
        "bundle_dir": bundle_dir,
        "release_download_dir": release_download_dir,
        "release_history": release_history,
        "files": files
    });
    atomic_write_text(
        &comparison_path,
        &serde_json::to_string_pretty(&comparison)?,
    )?;

    let support_metadata = if let Some(signing_key_path) = signing_key {
        sign_release_comparison_metadata(
            &bundle_dir,
            &comparison,
            &comparison_path,
            &release_history_path,
            signing_key_path,
            signer_id,
            generated_at_ms,
        )?
    } else {
        serde_json::json!({
            "present": false,
            "signature_verified": false
        })
    };

    let manifest = json_array(&comparison, "files")
        .iter()
        .filter_map(|file| {
            let relative_path = json_string(file, "path");
            if relative_path == "SHA256SUMS" {
                None
            } else {
                Some(relative_path)
            }
        })
        .map(|relative_path| {
            let file_path = bundle_dir.join(&relative_path);
            Ok(format!("{}  {}\n", sha256_file(&file_path)?, relative_path))
        })
        .collect::<Result<String>>()?;
    atomic_write_text(&sha256_path, &manifest)?;

    let result = serde_json::json!({
        "schema_version": "ao2.release-comparison-bundle.v1",
        "generated_at_ms": generated_at_ms,
        "bundle_dir": json_string(&comparison, "bundle_dir"),
        "bundle_path": comparison_path,
        "release_history_path": release_history_path,
        "release_download_dir": json_string(&comparison, "release_download_dir"),
        "release_history": comparison["release_history"].clone(),
        "support_metadata": support_metadata,
        "files": json_array(&comparison, "files"),
        "sha256_manifest": sha256_path
    });
    Ok(result)
}

fn sign_release_comparison_metadata(
    bundle_dir: &Path,
    comparison: &serde_json::Value,
    comparison_path: &Path,
    release_history_path: &Path,
    signing_key: &Path,
    signer_id: &str,
    generated_at_ms: u64,
) -> Result<serde_json::Value> {
    let metadata_path = bundle_dir.join("release-comparison-metadata.json");
    let signature_path = bundle_dir.join("release-comparison-metadata.json.sig");
    let public_key_path = bundle_dir.join("release-comparison-signing-public.pem");
    derive_public_key_from_private_key(signing_key, &public_key_path)?;
    let trend = &comparison["release_history"]["trend"];
    let metadata = serde_json::json!({
        "schema_version": "ao2.release-comparison-metadata.v1",
        "generated_at_ms": generated_at_ms,
        "signer_id": signer_id,
        "signature_algorithm": "RSA/SHA-256",
        "producer": {
            "package": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "git_commit": runtime_git_commit(),
            "target": runtime_target_label()
        },
        "release_comparison_path": comparison_path,
        "release_comparison_sha256": sha256_file(comparison_path)?,
        "release_history_path": release_history_path,
        "release_history_sha256": sha256_file(release_history_path)?,
        "public_key_sha256": sha256_file(&public_key_path)?,
        "release_count": json_u64(trend, "entry_count"),
        "latest_release_tag": json_string(trend, "latest_release_tag"),
        "latest_health_score": json_u64(trend, "latest_health_score"),
        "max_health_score": json_u64(trend, "max_health_score"),
        "attention_count": json_u64(trend, "attention_count"),
        "regression_count": json_u64(trend, "regression_count"),
        "bundle_files": json_array(comparison, "files")
    });
    atomic_write_text(&metadata_path, &serde_json::to_string_pretty(&metadata)?)?;
    sign_file_with_private_key(signing_key, &metadata_path, &signature_path)?;
    release_comparison_metadata_verification_json(bundle_dir)
}

fn release_comparison_metadata_verification_json(bundle_dir: &Path) -> Result<serde_json::Value> {
    let metadata_path = bundle_dir.join("release-comparison-metadata.json");
    let signature_path = bundle_dir.join("release-comparison-metadata.json.sig");
    let public_key_path = bundle_dir.join("release-comparison-signing-public.pem");
    if !metadata_path.exists() && !signature_path.exists() && !public_key_path.exists() {
        return Ok(serde_json::json!({
            "present": false,
            "signature_verified": false
        }));
    }
    if !metadata_path.is_file() || !signature_path.is_file() || !public_key_path.is_file() {
        return Err(anyhow!(
            "release comparison metadata is incomplete; metadata, signature, and public key are all required"
        ));
    }
    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&metadata_path)
            .with_context(|| format!("read {}", metadata_path.display()))?,
    )
    .with_context(|| format!("parse {}", metadata_path.display()))?;
    if json_string(&metadata, "schema_version") != "ao2.release-comparison-metadata.v1" {
        return Err(anyhow!(
            "release comparison metadata must use schema ao2.release-comparison-metadata.v1"
        ));
    }
    let signature_verified =
        verify_file_signature(&metadata_path, &signature_path, &public_key_path)?;
    if !signature_verified {
        return Err(anyhow!(
            "release comparison metadata signature verification failed"
        ));
    }
    Ok(serde_json::json!({
        "present": true,
        "signature_verified": true,
        "metadata_path": metadata_path,
        "signature_path": signature_path,
        "public_key_path": public_key_path,
        "metadata_sha256": sha256_file(&metadata_path)?,
        "signature_sha256": sha256_file(&signature_path)?,
        "public_key_sha256": sha256_file(&public_key_path)?,
        "signer_id": json_string(&metadata, "signer_id"),
        "signature_algorithm": json_string(&metadata, "signature_algorithm"),
        "metadata": metadata
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn release_support_bundle_build(
    release_assembly: PathBuf,
    readiness: PathBuf,
    handoff: PathBuf,
    cockpit: PathBuf,
    evaluator_decision: PathBuf,
    storage_support: PathBuf,
    replay: PathBuf,
    report_contract_verification: Option<PathBuf>,
    install_verification: PathBuf,
    hosted_release_smoke: PathBuf,
    report_target: Option<PathBuf>,
    report_run_id: Option<String>,
    report: Option<PathBuf>,
    report_index: Option<PathBuf>,
    operator_evidence: PathBuf,
    out_dir: PathBuf,
    json: bool,
) -> Result<()> {
    let result = release_support_bundle_build_json(
        &release_assembly,
        &readiness,
        &handoff,
        &cockpit,
        &evaluator_decision,
        &storage_support,
        &replay,
        report_contract_verification.as_deref(),
        &install_verification,
        &hosted_release_smoke,
        report_target.as_deref(),
        report_run_id.as_deref(),
        report,
        report_index,
        &operator_evidence,
        &out_dir,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "release_support_bundle_build={}",
            json_string(&result, "status")
        );
        println!("bundle={}", json_string(&result, "bundle"));
        println!("checksums={}", json_string(&result, "checksums"));
        println!("bundle_sha256={}", json_string(&result, "bundle_sha256"));
        println!(
            "report_contract_verification_source={}",
            json_string(&result, "report_contract_verification_source")
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn release_support_bundle_build_json(
    release_assembly: &Path,
    readiness: &Path,
    handoff: &Path,
    cockpit: &Path,
    evaluator_decision: &Path,
    storage_support: &Path,
    replay: &Path,
    report_contract_verification: Option<&Path>,
    install_verification: &Path,
    hosted_release_smoke: &Path,
    report_target: Option<&Path>,
    report_run_id: Option<&str>,
    report: Option<PathBuf>,
    report_index: Option<PathBuf>,
    operator_evidence: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let bundle_path = out_dir.join("release-support-bundle.json");
    let checksums_path = out_dir.join("SHA256SUMS");
    let (report_contract_verification, report_contract_verification_source) =
        release_support_bundle_report_contract_verification(
            report_contract_verification,
            report_target,
            report_run_id,
            report,
            report_index,
        )?;
    let mut bundle = serde_json::json!({
        "schema_version": "ao2.cp-release-support-bundle.v1",
        "bundle_kind": "release_support",
        "release_assembly": release_support_bundle_read_surface("release_assembly", release_assembly)?,
        "readiness": release_support_bundle_read_surface("readiness", readiness)?,
        "handoff": release_support_bundle_read_surface("handoff", handoff)?,
        "cockpit": release_support_bundle_read_surface("cockpit", cockpit)?,
        "evaluator_decision": release_support_bundle_read_surface("evaluator_decision", evaluator_decision)?,
        "storage_support": release_support_bundle_read_surface("storage_support", storage_support)?,
        "replay": release_support_bundle_read_surface("replay", replay)?,
        "report_contract_verification": report_contract_verification,
        "install_verification": release_support_bundle_read_surface("install_verification", install_verification)?,
        "hosted_release_smoke": release_support_bundle_read_surface("hosted_release_smoke", hosted_release_smoke)?,
        "operator_evidence": release_support_bundle_read_surface("operator_evidence", operator_evidence)?,
        "ci_evidence_index": release_support_bundle_ci_evidence_index(),
        "trust_boundary": {
            "frontend": "Hermes front end / queue / memory surface",
            "governed_backend": "factory-v3 / AO Operator evaluator-closer",
            "trusted_execution": "ao2 signed evidence boundary",
            "role": "read_only_observer",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        },
        "producer": {
            "package": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "target": runtime_target_label()
        }
    });
    bundle["portable_bundle_manifest"] = release_support_bundle_portable_manifest(&bundle)?;
    atomic_write_text(&bundle_path, &serde_json::to_string_pretty(&bundle)?)?;
    let bundle_sha256 = canonical_json_sha256(&bundle);
    atomic_write_text(
        &checksums_path,
        &format!("{bundle_sha256}  release-support-bundle.json\n"),
    )?;

    let verification =
        release_support_bundle_verification_json(&bundle_path, Some(checksums_path.as_path()))?;
    if json_string(&verification, "status") != "passed" {
        anyhow::bail!("built release support bundle did not pass verification");
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.release-support-bundle-build.v1",
        "status": "built",
        "bundle": bundle_path.display().to_string(),
        "checksums": checksums_path.display().to_string(),
        "bundle_sha256": bundle_sha256,
        "report_contract_verification_source": report_contract_verification_source,
        "verification": verification
    }))
}

fn release_support_bundle_report_contract_verification(
    report_contract_verification: Option<&Path>,
    report_target: Option<&Path>,
    report_run_id: Option<&str>,
    report: Option<PathBuf>,
    report_index: Option<PathBuf>,
) -> Result<(serde_json::Value, &'static str)> {
    let has_report_inputs = report_target.is_some()
        || report_run_id.is_some()
        || report.is_some()
        || report_index.is_some();
    if let Some(path) = report_contract_verification {
        if has_report_inputs {
            anyhow::bail!(
                "use either --report-contract-verification or --report-target/--report-run-id report inputs, not both"
            );
        }
        let verification =
            release_support_bundle_read_surface("report_contract_verification", path)?;
        return Ok((verification, "explicit_path"));
    }

    let target = report_target.ok_or_else(|| {
        anyhow::anyhow!(
            "support bundle build requires --report-contract-verification or --report-target with --report-run-id"
        )
    })?;
    let run_id = report_run_id.ok_or_else(|| {
        anyhow::anyhow!(
            "support bundle build requires --report-contract-verification or --report-target with --report-run-id"
        )
    })?;
    let verification = report_contract_verification_json(target, run_id, report, report_index)?;
    if json_string(&verification, "status") != "passed" {
        anyhow::bail!("generated report contract verification did not pass");
    }
    Ok((verification, "generated_report_verify"))
}

fn release_support_bundle_read_surface(name: &str, path: &Path) -> Result<serde_json::Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read release support bundle {name}: {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse release support bundle {name}: {}", path.display()))?;
    if !value.is_object() {
        anyhow::bail!(
            "release support bundle {name} must be a JSON object: {}",
            path.display()
        );
    }
    Ok(value)
}

const RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES: [(&str, &str, &str); 9] = [
    (
        "ci_evidence_index",
        "ci_evidence_index",
        "$.ci_evidence_index",
    ),
    (
        "install_verification",
        "install_verification",
        "$.install_verification",
    ),
    (
        "hosted_release_smoke",
        "hosted_release_smoke",
        "$.hosted_release_smoke",
    ),
    ("release_assembly", "release_assembly", "$.release_assembly"),
    ("release_readiness", "readiness", "$.readiness"),
    ("release_candidate_handoff", "handoff", "$.handoff"),
    ("release_cockpit", "cockpit", "$.cockpit"),
    (
        "release_evaluator_decision",
        "evaluator_decision",
        "$.evaluator_decision",
    ),
    (
        "storage_support_bundle",
        "storage_support",
        "$.storage_support",
    ),
];

fn release_support_bundle_portable_manifest(
    bundle: &serde_json::Value,
) -> Result<serde_json::Value> {
    let mut included_surfaces = Vec::new();
    let mut surface_sha256 = serde_json::Map::new();
    for (id, key, path) in RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES {
        let surface = bundle
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("missing release support bundle surface {key}"))?;
        let sha256 = canonical_json_sha256(surface);
        surface_sha256.insert(id.to_string(), serde_json::Value::String(sha256.clone()));
        included_surfaces.push(serde_json::json!({
            "id": id,
            "schema_version": json_string(surface, "schema_version"),
            "path": path,
            "sha256": sha256,
        }));
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.cp-release-support-bundle-manifest.v1",
        "included_surfaces": included_surfaces,
        "integrity": {
            "algorithm": "sha256-ao2-cp-canonical-json-v1",
            "scope": "embedded_support_bundle_surfaces",
            "surface_sha256": surface_sha256,
            "verification_plan": {
                "surface_count": RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES.len(),
                "expected_fail_closed": true,
                "cross_platform_commands": {
                    "macos_ubuntu": "python3 verify_release_support_bundle.py --json --checksums SHA256SUMS release-support-bundle.json",
                    "windows_powershell": "pwsh -File Verify-ReleaseSupportBundle.ps1 -Json -Checksums SHA256SUMS -Path release-support-bundle.json"
                }
            }
        }
    }))
}

pub(crate) fn release_support_bundle_verify(
    bundle: PathBuf,
    checksums: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let report = release_support_bundle_verification_json(&bundle, checksums.as_deref())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "release_support_bundle_verification={}",
            json_string(&report, "status")
        );
        println!("bundle={}", bundle.display());
        println!("surface_count={}", json_u64(&report, "surface_count"));
        println!(
            "checksum_verified={}",
            report["checksum_verified"].as_bool().unwrap_or(false)
        );
        println!("failure_count={}", json_array(&report, "failures").len());
    }
    if json_string(&report, "status") != "passed" {
        anyhow::bail!("release support bundle verification failed");
    }
    Ok(())
}

fn release_support_bundle_verification_json(
    bundle_path: &Path,
    checksums_path: Option<&Path>,
) -> Result<serde_json::Value> {
    let mut failures = Vec::new();
    let bundle = match read_json_for_verification(bundle_path, &mut failures) {
        Some(value) => value,
        None => serde_json::Value::Null,
    };
    if !bundle.is_null()
        && json_string(&bundle, "schema_version") != "ao2.cp-release-support-bundle.v1"
    {
        failures.push(serde_json::json!({
            "code": "invalid_schema",
            "message": "release support bundle must use schema ao2.cp-release-support-bundle.v1"
        }));
    }

    let required_surfaces = [
        "release_assembly",
        "readiness",
        "handoff",
        "cockpit",
        "evaluator_decision",
        "storage_support",
        "report_contract_verification",
        "install_verification",
        "hosted_release_smoke",
    ];
    let mut surfaces = Vec::new();
    for surface in required_surfaces {
        let present = bundle.get(surface).is_some_and(|value| !value.is_null());
        if !present {
            failures.push(serde_json::json!({
                "code": "missing_surface",
                "surface": surface,
                "message": format!("release support bundle is missing required surface {surface}")
            }));
        }
        surfaces.push(serde_json::json!({
            "name": surface,
            "present": present,
            "schema_version": json_string(&bundle[surface], "schema_version"),
            "status": json_string(&bundle[surface], "status")
        }));
    }

    let replay = bundle.get("replay").unwrap_or(&serde_json::Value::Null);
    let replay_present = replay.is_object();
    let replay_status = json_string(replay, "status");
    let replay_digest_failures = json_array(replay, "digest_failures");
    if !replay_present {
        failures.push(serde_json::json!({
            "code": "missing_replay_evidence",
            "message": "release support bundle must include replay evidence"
        }));
    } else {
        if replay_status != "accepted" {
            failures.push(serde_json::json!({
                "code": "replay_not_accepted",
                "message": "release support bundle replay status must be accepted",
                "observed": replay_status
            }));
        }
        if !replay_digest_failures.is_empty() {
            failures.push(serde_json::json!({
                "code": "replay_digest_failures",
                "message": "release support bundle replay must not report digest failures",
                "digest_failure_count": replay_digest_failures.len()
            }));
        }
    }

    let release_assembly = bundle
        .get("release_assembly")
        .unwrap_or(&serde_json::Value::Null);
    let readiness = bundle.get("readiness").unwrap_or(&serde_json::Value::Null);
    let handoff = bundle.get("handoff").unwrap_or(&serde_json::Value::Null);
    let evaluator_decision = bundle
        .get("evaluator_decision")
        .unwrap_or(&serde_json::Value::Null);
    let operator_evidence = bundle
        .get("operator_evidence")
        .unwrap_or(&serde_json::Value::Null);
    let report_contract_verification = bundle
        .get("report_contract_verification")
        .unwrap_or(&serde_json::Value::Null);
    let install_verification = bundle
        .get("install_verification")
        .unwrap_or(&serde_json::Value::Null);
    let hosted_release_smoke = bundle
        .get("hosted_release_smoke")
        .unwrap_or(&serde_json::Value::Null);
    let portable_bundle_manifest = bundle
        .get("portable_bundle_manifest")
        .unwrap_or(&serde_json::Value::Null);
    let report_contract_verification_present = report_contract_verification.is_object();
    let install_verification_present = install_verification.is_object();
    let hosted_release_smoke_present = hosted_release_smoke.is_object();
    let operator_evidence_present = operator_evidence.is_object();
    let readiness_operator = readiness
        .pointer("/operator_decision")
        .unwrap_or(&serde_json::Value::Null);
    let handoff_trust = handoff
        .pointer("/trust_boundary")
        .unwrap_or(&serde_json::Value::Null);
    let evaluator_trust = evaluator_decision
        .pointer("/trust_boundary")
        .unwrap_or(&serde_json::Value::Null);

    let control_plane_approves_release = [
        json_bool(release_assembly, "control_plane_approves_release"),
        json_bool(readiness_operator, "control_plane_approves_release"),
        json_bool(handoff_trust, "control_plane_approves_release"),
        json_bool(evaluator_trust, "control_plane_approves_release"),
        json_bool(operator_evidence, "control_plane_approves_release"),
    ]
    .into_iter()
    .any(|approves| approves);
    if control_plane_approves_release {
        failures.push(serde_json::json!({
            "code": "control_plane_approved_release",
            "message": "control plane must remain a read-only observer and must not approve release"
        }));
    }

    if !operator_evidence_present {
        failures.push(serde_json::json!({
            "code": "missing_operator_evidence",
            "message": "release support bundle must include operator evidence"
        }));
    }

    if install_verification_present {
        if let Err(error) =
            release_evidence_bundle_validate_install_verification(install_verification)
        {
            failures.push(serde_json::json!({
                "code": "install_verification_invalid",
                "message": error.to_string()
            }));
        }
    }
    if hosted_release_smoke_present {
        if let Err(error) =
            release_support_bundle_validate_hosted_release_smoke(hosted_release_smoke)
        {
            failures.push(serde_json::json!({
                "code": "hosted_release_smoke_invalid",
                "message": error.to_string()
            }));
        }
    }
    release_support_bundle_portable_manifest_failures(
        &bundle,
        portable_bundle_manifest,
        &mut failures,
    );
    release_support_bundle_candidate_correlation_failures(
        release_assembly,
        readiness,
        handoff,
        bundle.get("cockpit").unwrap_or(&serde_json::Value::Null),
        &mut failures,
    );

    let report_contract_missing_sections =
        json_array(report_contract_verification, "missing_sections");
    let report_contract_failures = json_array(report_contract_verification, "failures");
    let report_contract_complete = json_bool(report_contract_verification, "complete");
    let report_contract_status = json_string(report_contract_verification, "status");
    if !report_contract_verification_present {
        failures.push(serde_json::json!({
            "code": "missing_report_contract_verification",
            "message": "release support bundle must include report contract verification evidence"
        }));
    } else {
        if json_string(report_contract_verification, "schema_version")
            != "ao2.report-contract-verification.v1"
        {
            failures.push(serde_json::json!({
                "code": "invalid_report_contract_verification_schema",
                "message": "report contract verification must use schema ao2.report-contract-verification.v1",
                "observed": json_string(report_contract_verification, "schema_version")
            }));
        }
        if json_string(report_contract_verification, "contract_schema_version")
            != "ao2.report-contract.v1"
        {
            failures.push(serde_json::json!({
                "code": "invalid_report_contract_schema",
                "message": "report contract verification must reference ao2.report-contract.v1",
                "observed": json_string(report_contract_verification, "contract_schema_version")
            }));
        }
        if report_contract_status != "passed"
            || !report_contract_complete
            || !report_contract_missing_sections.is_empty()
            || !report_contract_failures.is_empty()
        {
            failures.push(serde_json::json!({
                "code": "report_contract_verification_failed",
                "message": "release support bundle report contract verification must pass with no missing sections",
                "status": report_contract_status,
                "complete": report_contract_complete,
                "missing_section_count": report_contract_missing_sections.len(),
                "failure_count": report_contract_failures.len()
            }));
        }
    }

    let evaluator_required = json_bool(readiness_operator, "factory_v3_evaluator_closer_required")
        || json_bool(operator_evidence, "factory_v3_evaluator_closer_required");
    if !evaluator_required {
        failures.push(serde_json::json!({
            "code": "operator_evaluator_closer_not_required",
            "message": "operator evidence must require factory-v3 evaluator-closer ownership"
        }));
    }

    let owner_values = [
        json_string(handoff_trust, "release_acceptance_owner"),
        json_string(evaluator_trust, "release_acceptance_owner"),
        json_string(operator_evidence, "release_acceptance_owner"),
    ];
    if owner_values
        .iter()
        .any(|owner| owner != "factory-v3 evaluator-closer")
    {
        failures.push(serde_json::json!({
            "code": "release_acceptance_owner_mismatch",
            "message": "release acceptance owner must be factory-v3 evaluator-closer",
            "observed": owner_values
        }));
    }

    let role_values = [
        json_string(handoff_trust, "control_plane_role"),
        json_string(evaluator_trust, "control_plane_role"),
        json_string(operator_evidence, "control_plane_role"),
    ];
    if role_values.iter().any(|role| role != "read_only_observer") {
        failures.push(serde_json::json!({
            "code": "control_plane_role_mismatch",
            "message": "control plane role must be read_only_observer",
            "observed": role_values
        }));
    }

    release_support_bundle_secret_marker_failures("$", &bundle, &mut failures);

    let bundle_sha256 = if bundle.is_object() {
        canonical_json_sha256(&bundle)
    } else if bundle_path.is_file() {
        sha256_file(bundle_path)?
    } else {
        String::new()
    };
    let mut checksum_verified = false;
    let mut checksum_manifest = serde_json::Value::Null;
    if let Some(path) = checksums_path {
        match fs::read_to_string(path) {
            Ok(body) => {
                let manifest = checksum_manifest_map(&body, &mut failures);
                let names = [
                    bundle_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string(),
                    bundle_path.display().to_string(),
                ];
                let expected = names.iter().find_map(|name| manifest.get(name));
                match expected {
                    Some(expected_sha256) if expected_sha256 == &bundle_sha256 => {
                        checksum_verified = true;
                    }
                    Some(expected_sha256) => failures.push(serde_json::json!({
                        "code": "checksum_mismatch",
                        "message": format!("{} digest did not match SHA256SUMS", bundle_path.display()),
                        "expected": expected_sha256,
                        "actual": bundle_sha256
                    })),
                    None => failures.push(serde_json::json!({
                        "code": "checksum_missing",
                        "message": format!("SHA256SUMS does not include {}", bundle_path.display())
                    })),
                }
                checksum_manifest = serde_json::json!({
                    "path": path,
                    "entry_count": manifest.len()
                });
            }
            Err(error) => failures.push(serde_json::json!({
                "code": "checksum_read_failed",
                "message": format!("read {}: {error}", path.display())
            })),
        }
    }

    let status = if failures.is_empty() {
        "passed"
    } else {
        "failed"
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.release-support-bundle-verification.v1",
        "status": status,
        "bundle_path": bundle_path,
        "bundle_sha256": bundle_sha256,
        "checksum_verified": checksum_verified,
        "checksum_manifest": checksum_manifest,
        "surface_count": surfaces.iter().filter(|surface| {
            surface.get("present").and_then(serde_json::Value::as_bool).unwrap_or(false)
        }).count(),
        "surfaces": surfaces,
        "replay": {
            "present": replay_present,
            "status": replay_status,
            "digest_failure_count": replay_digest_failures.len()
        },
        "operator_evidence": {
            "present": operator_evidence_present,
            "factory_v3_evaluator_closer_required": evaluator_required,
            "control_plane_approves_release": control_plane_approves_release
        },
        "report_contract_verification": {
            "present": report_contract_verification_present,
            "schema_version": json_string(report_contract_verification, "schema_version"),
            "contract_schema_version": json_string(report_contract_verification, "contract_schema_version"),
            "status": report_contract_status,
            "complete": report_contract_complete,
            "missing_section_count": report_contract_missing_sections.len(),
            "failure_count": report_contract_failures.len()
        },
        "install_verification": {
            "present": install_verification_present,
            "schema_version": json_string(install_verification, "schema_version"),
            "status": json_string(install_verification, "status"),
            "offline_status": json_string(&install_verification["offline_verification"], "status"),
            "provider_api_keys_required": install_verification.get("provider_api_keys_required").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "control_plane_approves_release": install_verification.get("control_plane_approves_release").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "mutates_ao_artifacts": install_verification.get("mutates_ao_artifacts").and_then(serde_json::Value::as_bool).unwrap_or(true)
        },
        "hosted_release_smoke": {
            "present": hosted_release_smoke_present,
            "schema_version": json_string(hosted_release_smoke, "schema_version"),
            "status": json_string(hosted_release_smoke, "status"),
            "install_verification_schema": json_string(hosted_release_smoke, "install_verification_schema"),
            "provider_api_keys_required": hosted_release_smoke.get("provider_api_keys_required").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "control_plane_approves_release": hosted_release_smoke.get("control_plane_approves_release").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "mutates_ao_artifacts": hosted_release_smoke.get("mutates_ao_artifacts").and_then(serde_json::Value::as_bool).unwrap_or(true)
        },
        "portable_bundle_manifest": {
            "present": portable_bundle_manifest.is_object(),
            "schema_version": json_string(portable_bundle_manifest, "schema_version"),
            "surface_count": portable_bundle_manifest
                .get("included_surfaces")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0)
        },
        "failure_count": failures.len(),
        "failures": failures,
        "trust_boundary": {
            "frontend": "Hermes front end / queue / memory surface",
            "trusted_execution": "ao2 signed evidence boundary",
            "governed_backend": "factory-v3 / AO Operator evaluator-closer",
            "control_plane_role": "read_only_observer",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false,
            "mutates_ao_artifacts": false
        }
    }))
}

fn release_support_bundle_portable_manifest_failures(
    bundle: &serde_json::Value,
    manifest: &serde_json::Value,
    failures: &mut Vec<serde_json::Value>,
) {
    if !manifest.is_object() {
        failures.push(serde_json::json!({
            "code": "missing_portable_bundle_manifest",
            "message": "release support bundle must include portable_bundle_manifest for control-plane verification"
        }));
        return;
    }
    if json_string(manifest, "schema_version") != "ao2.cp-release-support-bundle-manifest.v1" {
        failures.push(serde_json::json!({
            "code": "invalid_portable_bundle_manifest_schema",
            "message": "portable_bundle_manifest must use schema ao2.cp-release-support-bundle-manifest.v1",
            "observed": json_string(manifest, "schema_version")
        }));
    }

    let included_surfaces = json_array(manifest, "included_surfaces");
    if included_surfaces.len() != RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES.len() {
        failures.push(serde_json::json!({
            "code": "portable_surface_count_mismatch",
            "message": "portable_bundle_manifest included surface count does not match the public support-bundle contract",
            "expected": RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES.len(),
            "observed": included_surfaces.len()
        }));
    }

    let integrity = manifest
        .get("integrity")
        .unwrap_or(&serde_json::Value::Null);
    if json_string(integrity, "algorithm") != "sha256-ao2-cp-canonical-json-v1" {
        failures.push(serde_json::json!({
            "code": "portable_integrity_algorithm_mismatch",
            "message": "portable_bundle_manifest integrity must use AO2/control-plane canonical JSON digests",
            "observed": json_string(integrity, "algorithm")
        }));
    }
    let declared_surface_count = integrity
        .get("verification_plan")
        .and_then(|plan| plan.get("surface_count"))
        .and_then(serde_json::Value::as_u64);
    if declared_surface_count != Some(RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES.len() as u64) {
        failures.push(serde_json::json!({
            "code": "portable_verification_plan_surface_count_mismatch",
            "message": "portable_bundle_manifest verification plan has the wrong required surface count",
            "expected": RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES.len(),
            "observed": declared_surface_count
        }));
    }
    let surface_sha256 = integrity
        .get("surface_sha256")
        .unwrap_or(&serde_json::Value::Null);

    let mut seen_ids = std::collections::BTreeSet::new();
    for surface in included_surfaces {
        let id = json_string(surface, "id");
        if !seen_ids.insert(id.clone()) {
            failures.push(serde_json::json!({
                "code": "portable_duplicate_surface",
                "surface": id,
                "message": "portable_bundle_manifest repeats a surface id"
            }));
        }
    }

    for (id, key, path) in RELEASE_SUPPORT_BUNDLE_PUBLIC_SURFACES {
        if !seen_ids.contains(id) {
            failures.push(serde_json::json!({
                "code": "portable_missing_surface",
                "surface": id,
                "message": format!("portable_bundle_manifest is missing required surface {id}")
            }));
            continue;
        }
        let Some(entry) = included_surfaces
            .iter()
            .find(|surface| json_string(surface, "id") == id)
        else {
            continue;
        };
        let Some(embedded) = bundle.get(key) else {
            failures.push(serde_json::json!({
                "code": "portable_embedded_surface_missing",
                "surface": id,
                "message": format!("portable_bundle_manifest declares {id}, but bundle key {key} is missing")
            }));
            continue;
        };
        let expected_sha256 = canonical_json_sha256(embedded);
        let manifest_sha256 = json_string(entry, "sha256");
        let integrity_sha256 = json_string(surface_sha256, id);
        if json_string(entry, "path") != path {
            failures.push(serde_json::json!({
                "code": "portable_surface_path_mismatch",
                "surface": id,
                "expected": path,
                "observed": json_string(entry, "path")
            }));
        }
        if json_string(entry, "schema_version") != json_string(embedded, "schema_version") {
            failures.push(serde_json::json!({
                "code": "portable_surface_schema_mismatch",
                "surface": id,
                "expected": json_string(embedded, "schema_version"),
                "observed": json_string(entry, "schema_version")
            }));
        }
        if manifest_sha256 != expected_sha256 || integrity_sha256 != expected_sha256 {
            failures.push(serde_json::json!({
                "code": "portable_surface_digest_mismatch",
                "surface": id,
                "expected": expected_sha256,
                "manifest_sha256": manifest_sha256,
                "integrity_sha256": integrity_sha256
            }));
        }
    }
}

fn release_support_bundle_candidate_correlation_failures(
    release_assembly: &serde_json::Value,
    readiness: &serde_json::Value,
    handoff: &serde_json::Value,
    cockpit: &serde_json::Value,
    failures: &mut Vec<serde_json::Value>,
) {
    let required = [
        (
            "release_cockpit",
            "candidate_correlation",
            cockpit.get("candidate_correlation"),
        ),
        (
            "release_candidate_handoff",
            "candidate_correlation",
            handoff.get("candidate_correlation"),
        ),
        (
            "release_readiness",
            "candidate_correlation",
            readiness.get("candidate_correlation"),
        ),
        (
            "release_assembly",
            "candidate_correlation_detail",
            release_assembly.get("candidate_correlation_detail"),
        ),
    ];
    let mut hashes = Vec::new();
    for (surface, field, value) in required {
        let Some(value) = value else {
            failures.push(serde_json::json!({
                "code": "candidate_correlation_missing",
                "surface": surface,
                "field": field,
                "message": format!("{surface}.{field} is required for operator triage")
            }));
            continue;
        };
        if !value.is_object() {
            failures.push(serde_json::json!({
                "code": "candidate_correlation_invalid",
                "surface": surface,
                "field": field,
                "message": format!("{surface}.{field} must be a JSON object")
            }));
            continue;
        }
        let status = json_string(value, "status");
        if status != "matched" && status != "mismatched" {
            failures.push(serde_json::json!({
                "code": "candidate_correlation_status_invalid",
                "surface": surface,
                "field": field,
                "observed": status
            }));
        }
        if !value
            .get("blockers")
            .is_some_and(serde_json::Value::is_array)
        {
            failures.push(serde_json::json!({
                "code": "candidate_correlation_blockers_invalid",
                "surface": surface,
                "field": field,
                "message": format!("{surface}.{field}.blockers must be an array")
            }));
        }
        hashes.push((surface, canonical_json_sha256(value)));
    }
    let distinct = hashes
        .iter()
        .map(|(_, sha)| sha.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if distinct.len() > 1 {
        failures.push(serde_json::json!({
            "code": "candidate_correlation_cross_surface_mismatch",
            "message": "operator-triage surfaces must embed byte-identical candidate_correlation objects",
            "surface_sha256": hashes
        }));
    }
}

fn release_support_bundle_secret_marker_failures(
    path: &str,
    value: &serde_json::Value,
    failures: &mut Vec<serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}.{key}");
                let key_lower = key.to_ascii_lowercase();
                if matches!(
                    key_lower.as_str(),
                    "token" | "access_token" | "refresh_token"
                ) {
                    failures.push(serde_json::json!({
                        "code": "forbidden_secret_field",
                        "path": child_path,
                        "message": "release support bundle must not expose token fields"
                    }));
                }
                release_support_bundle_secret_marker_failures(&child_path, child, failures);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                release_support_bundle_secret_marker_failures(
                    &format!("{path}[{index}]"),
                    child,
                    failures,
                );
            }
        }
        serde_json::Value::String(text) => {
            for marker in [
                "Authorization: Bearer ",
                "AO2_CP_API_TOKEN=",
                "OPENAI_API_KEY=",
                "ANTHROPIC_API_KEY=",
            ] {
                if text.contains(marker) {
                    failures.push(serde_json::json!({
                        "code": "forbidden_secret_marker",
                        "path": path,
                        "marker": marker,
                        "message": "release support bundle contains a forbidden secret marker"
                    }));
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn release_evidence_bundle_json(
    out_dir: PathBuf,
    artifact_specs: &[String],
) -> Result<serde_json::Value> {
    if artifact_specs.is_empty() {
        anyhow::bail!("at least one --artifact <label=path> is required");
    }

    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let created_at_ms = now_unix_ms();
    let bundle_name = format!("ao2-release-evidence-bundle-{created_at_ms}");
    let stage_dir = out_dir.join(format!(".{bundle_name}.stage"));
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .with_context(|| format!("remove stale {}", stage_dir.display()))?;
    }
    fs::create_dir_all(stage_dir.join("artifacts"))
        .with_context(|| format!("create {}", stage_dir.join("artifacts").display()))?;

    let mut seen_labels = BTreeSet::new();
    let mut artifacts = Vec::new();
    let mut checksum_entries: Vec<(String, String)> = Vec::new();
    let mut install_verification_artifact_labels = Vec::new();
    for spec in artifact_specs {
        let Some((raw_label, raw_path)) = spec.split_once('=') else {
            anyhow::bail!("artifact must be in <label>=<path> form: {spec}");
        };
        let label = raw_label.trim();
        if !is_valid_release_evidence_artifact_label(label) {
            anyhow::bail!(
                "invalid artifact label {label:?}; use only ASCII letters, digits, '.', '_', or '-'"
            );
        }
        if !seen_labels.insert(label.to_string()) {
            anyhow::bail!("duplicate artifact label: {label}");
        }
        let source = PathBuf::from(raw_path);
        if !source.is_file() {
            anyhow::bail!("artifact {label} is not a file: {}", source.display());
        }
        let file_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .with_context(|| format!("artifact {label} filename is not utf8"))?;
        let relative_path = format!("artifacts/{label}/{file_name}");
        let staged_path = stage_dir.join(&relative_path);
        fs::create_dir_all(
            staged_path
                .parent()
                .context("staged artifact has parent directory")?,
        )
        .with_context(|| format!("create parent for {}", staged_path.display()))?;
        fs::copy(&source, &staged_path)
            .with_context(|| format!("copy {} to {}", source.display(), staged_path.display()))?;
        let source_sha256 = sha256_file(&source)?;
        let bundle_sha256 = sha256_file(&staged_path)?;
        if source_sha256 != bundle_sha256 {
            anyhow::bail!("artifact digest changed while staging {label}");
        }
        let artifact_json = read_json_file::<serde_json::Value>(&source).ok();
        if release_evidence_bundle_install_verification_candidate(label, artifact_json.as_ref()) {
            let Some(json) = artifact_json.as_ref() else {
                anyhow::bail!("install verification evidence must be valid JSON: {label}");
            };
            release_evidence_bundle_validate_install_verification(json)?;
            install_verification_artifact_labels.push(label.to_string());
        }
        let size_bytes = fs::metadata(&staged_path)
            .with_context(|| format!("stat {}", staged_path.display()))?
            .len();
        checksum_entries.push((relative_path.clone(), bundle_sha256.clone()));
        artifacts.push(serde_json::json!({
            "label": label,
            "source_path": source,
            "bundle_path": relative_path,
            "sha256": bundle_sha256,
            "size_bytes": size_bytes
        }));
    }
    if install_verification_artifact_labels.is_empty() {
        anyhow::bail!("install verification evidence is required");
    }

    let manifest_path = stage_dir.join("EVIDENCE-BUNDLE-MANIFEST.json");
    // AO2 assembles producer evidence here for factory-v3 evaluator-closer
    // review; the control plane remains a read-only observer.
    let manifest = serde_json::json!({
        "schema_version": "ao2.release-evidence-bundle.v1",
        "created_at": created_at,
        "created_at_ms": created_at_ms,
        "artifact_count": artifacts.len(),
        "artifacts": artifacts,
        "files": checksum_entries.iter().map(|(path, sha256)| {
            serde_json::json!({
                "path": path,
                "sha256": sha256
            })
        }).collect::<Vec<_>>(),
        "install_verification_evidence": {
            "included": !install_verification_artifact_labels.is_empty(),
            "artifact_labels": install_verification_artifact_labels,
        },
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        }
    });
    let mut manifest_text = serde_json::to_string_pretty(&manifest)?;
    manifest_text.push('\n');
    atomic_write_text(&manifest_path, &manifest_text)?;
    checksum_entries.push((
        "EVIDENCE-BUNDLE-MANIFEST.json".to_string(),
        sha256_file(&manifest_path)?,
    ));
    checksum_entries.sort_by(|left, right| left.0.cmp(&right.0));
    let checksum_text = checksum_entries
        .iter()
        .map(|(path, sha256)| format!("{sha256}  {path}\n"))
        .collect::<String>();
    atomic_write_text(&stage_dir.join("SHA256SUMS"), &checksum_text)?;

    let archive_path = out_dir.join(format!("{bundle_name}.tar.gz"));
    create_tar_gz(&stage_dir, &archive_path)?;
    fs::remove_dir_all(&stage_dir).with_context(|| format!("remove {}", stage_dir.display()))?;
    let archive_sha256 = sha256_file(&archive_path)?;

    Ok(serde_json::json!({
        "schema_version": "ao2.release-evidence-bundle.v1",
        "created_at": manifest["created_at"].clone(),
        "created_at_ms": created_at_ms,
        "archive": archive_path,
        "sha256": archive_sha256,
        "artifact_count": manifest["artifact_count"].clone(),
        "manifest_entry": "EVIDENCE-BUNDLE-MANIFEST.json",
        "checksum_entry": "SHA256SUMS",
        "artifacts": manifest["artifacts"].clone(),
        "install_verification_evidence": manifest["install_verification_evidence"].clone(),
        "trust_boundary": manifest["trust_boundary"].clone()
    }))
}

fn is_valid_release_evidence_artifact_label(label: &str) -> bool {
    !label.is_empty()
        && label != "."
        && label != ".."
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn release_evidence_bundle_install_verification_candidate(
    label: &str,
    json: Option<&serde_json::Value>,
) -> bool {
    label == "install-verification"
        || json
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_str)
            == Some("ao2.install-verification-evidence.v1")
}

fn release_evidence_bundle_validate_install_verification(json: &serde_json::Value) -> Result<()> {
    if json_string(json, "schema_version") != "ao2.install-verification-evidence.v1" {
        anyhow::bail!(
            "install verification evidence schema_version must be ao2.install-verification-evidence.v1"
        );
    }
    if json_string(json, "status") != "verified" {
        anyhow::bail!("install verification evidence status must be verified");
    }
    if json_string(&json["offline_verification"], "status") != "verified" {
        anyhow::bail!("install verification evidence offline_verification.status must be verified");
    }
    if json
        .get("provider_api_keys_required")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        anyhow::bail!("install verification evidence must not require provider API keys");
    }
    if json
        .get("control_plane_approves_release")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        anyhow::bail!(
            "install verification evidence must not approve releases through the control plane"
        );
    }
    if json
        .get("mutates_ao_artifacts")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        anyhow::bail!("install verification evidence must not mutate AO artifacts");
    }
    Ok(())
}

fn release_support_bundle_validate_hosted_release_smoke(json: &serde_json::Value) -> Result<()> {
    if json_string(json, "schema_version") != "ao2.release-archive-hosted-smoke.v1" {
        anyhow::bail!(
            "hosted release smoke schema_version must be ao2.release-archive-hosted-smoke.v1"
        );
    }
    if json_string(json, "status") != "passed" {
        anyhow::bail!("hosted release smoke status must be passed");
    }
    if json_string(json, "install_verification_schema") != "ao2.install-verification-evidence.v1" {
        anyhow::bail!("hosted release smoke must reference ao2.install-verification-evidence.v1");
    }
    if json_string(json, "install_verification_evidence").is_empty() {
        anyhow::bail!("hosted release smoke must reference install_verification_evidence");
    }
    if json
        .get("provider_api_keys_required")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        anyhow::bail!("hosted release smoke must not require provider API keys");
    }
    if json
        .get("control_plane_approves_release")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        anyhow::bail!("hosted release smoke must not approve releases through the control plane");
    }
    if json
        .get("mutates_ao_artifacts")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        anyhow::bail!("hosted release smoke must not mutate AO artifacts");
    }
    if json_string(json, "release_acceptance_owner") != "factory-v3 evaluator-closer" {
        anyhow::bail!(
            "hosted release smoke release_acceptance_owner must be factory-v3 evaluator-closer"
        );
    }
    Ok(())
}

pub(crate) fn release_evidence_bundle_verification_json(
    bundle_path: &Path,
) -> Result<serde_json::Value> {
    let mut failures = Vec::new();
    let bundle_sha256 = match sha256_file(bundle_path) {
        Ok(sha256) => sha256,
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "bundle_unreadable",
                "message": format!("read bundle: {error}")
            }));
            return Ok(release_evidence_bundle_verification_report(
                ReleaseEvidenceBundleVerificationReport {
                    bundle_path,
                    bundle_sha256: "",
                    artifact_count: 0,
                    files_checked: 0,
                    manifest_verified: false,
                    trust_boundary_verified: false,
                    secret_scan_passed: false,
                    failures,
                },
            ));
        }
    };

    let extract_dir = std::env::temp_dir().join(format!(
        "ao2-release-evidence-bundle-verify-{}-{}",
        std::process::id(),
        now_unix_ms()
    ));
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir)
            .with_context(|| format!("remove stale {}", extract_dir.display()))?;
    }
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("create {}", extract_dir.display()))?;
    if let Err(error) = extract_tar_gz(bundle_path, &extract_dir) {
        failures.push(serde_json::json!({
            "code": "bundle_extract_failed",
            "message": error.to_string()
        }));
        let _ = fs::remove_dir_all(&extract_dir);
        return Ok(release_evidence_bundle_verification_report(
            ReleaseEvidenceBundleVerificationReport {
                bundle_path,
                bundle_sha256: &bundle_sha256,
                artifact_count: 0,
                files_checked: 0,
                manifest_verified: false,
                trust_boundary_verified: false,
                secret_scan_passed: false,
                failures,
            },
        ));
    }

    let manifest_path = extract_dir.join("EVIDENCE-BUNDLE-MANIFEST.json");
    let checksum_path = extract_dir.join("SHA256SUMS");
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                failures.push(serde_json::json!({
                    "code": "manifest_json_invalid",
                    "message": format!("parse EVIDENCE-BUNDLE-MANIFEST.json: {error}")
                }));
                serde_json::Value::Null
            }
        },
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "manifest_missing",
                "message": format!("read EVIDENCE-BUNDLE-MANIFEST.json: {error}")
            }));
            serde_json::Value::Null
        }
    };
    if !manifest.is_null()
        && json_string(&manifest, "schema_version") != "ao2.release-evidence-bundle.v1"
    {
        failures.push(serde_json::json!({
            "code": "invalid_manifest_schema",
            "message": "EVIDENCE-BUNDLE-MANIFEST.json must use schema ao2.release-evidence-bundle.v1"
        }));
    }

    let mut manifest_reasons = Vec::new();
    let checksum_manifest = match fs::read_to_string(&checksum_path) {
        Ok(body) => checksum_manifest_map(&body, &mut manifest_reasons),
        Err(error) => {
            failures.push(serde_json::json!({
                "code": "sha256_manifest_missing",
                "message": format!("read SHA256SUMS: {error}")
            }));
            BTreeMap::new()
        }
    };
    failures.extend(manifest_reasons);

    let mut manifest_verified = !manifest.is_null() && !checksum_manifest.is_empty();
    let mut files_checked = 0_usize;
    for (relative_path, expected_sha256) in &checksum_manifest {
        if !release_evidence_bundle_relative_path_allowed(relative_path) {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "unsafe_bundle_path",
                "path": relative_path,
                "message": "SHA256SUMS contains an absolute or parent-directory path"
            }));
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if !file_path.is_file() {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "bundle_file_missing",
                "path": relative_path,
                "message": "SHA256SUMS references a missing file"
            }));
            continue;
        }
        match sha256_file(&file_path) {
            Ok(actual_sha256) if actual_sha256 == *expected_sha256 => {
                files_checked += 1;
            }
            Ok(actual_sha256) => {
                manifest_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_mismatch",
                    "path": relative_path,
                    "expected": expected_sha256,
                    "actual": actual_sha256,
                    "message": "bundle file digest does not match SHA256SUMS"
                }));
            }
            Err(error) => {
                manifest_verified = false;
                failures.push(serde_json::json!({
                    "code": "sha256_unreadable",
                    "path": relative_path,
                    "message": format!("hash bundle file: {error}")
                }));
            }
        }
    }

    let artifacts = json_array(&manifest, "artifacts");
    for artifact in artifacts {
        let label = json_string(artifact, "label");
        let bundle_path = json_string(artifact, "bundle_path");
        let sha256 = json_string(artifact, "sha256");
        if !is_valid_release_evidence_artifact_label(&label) {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "invalid_artifact_label",
                "label": label,
                "message": "manifest contains an invalid artifact label"
            }));
        }
        if !bundle_path.starts_with("artifacts/") || !checksum_manifest.contains_key(&bundle_path) {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "artifact_not_checksummed",
                "label": label,
                "path": bundle_path,
                "message": "artifact bundle_path must be covered by SHA256SUMS"
            }));
        } else if checksum_manifest
            .get(&bundle_path)
            .is_some_and(|expected| expected != &sha256)
        {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "artifact_sha256_mismatch",
                "label": label,
                "path": bundle_path,
                "message": "artifact sha256 does not match SHA256SUMS"
            }));
        }
    }
    if !checksum_manifest.contains_key("EVIDENCE-BUNDLE-MANIFEST.json") {
        manifest_verified = false;
        failures.push(serde_json::json!({
            "code": "manifest_not_checksummed",
            "message": "SHA256SUMS must include EVIDENCE-BUNDLE-MANIFEST.json"
        }));
    }

    let install_verification = &manifest["install_verification_evidence"];
    let install_labels = json_array(install_verification, "artifact_labels");
    if install_verification
        .get("included")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
        || install_labels.is_empty()
    {
        manifest_verified = false;
        failures.push(serde_json::json!({
            "code": "install_verification_missing",
            "message": "release evidence bundle must include install verification evidence"
        }));
    }
    for label_value in install_labels {
        let Some(label) = label_value.as_str() else {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "install_verification_label_invalid",
                "message": "install verification artifact label must be a string"
            }));
            continue;
        };
        let Some(artifact) = artifacts
            .iter()
            .find(|artifact| json_string(artifact, "label") == label)
        else {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "install_verification_artifact_missing",
                "label": label,
                "message": "install verification manifest label does not match an artifact"
            }));
            continue;
        };
        let bundle_path = json_string(artifact, "bundle_path");
        if !checksum_manifest.contains_key(&bundle_path) {
            manifest_verified = false;
            failures.push(serde_json::json!({
                "code": "install_verification_not_checksummed",
                "label": label,
                "path": bundle_path,
                "message": "install verification artifact must be covered by SHA256SUMS"
            }));
            continue;
        }
        if !release_evidence_bundle_relative_path_allowed(&bundle_path) {
            continue;
        }
        let file_path = extract_dir.join(&bundle_path);
        match read_json_file::<serde_json::Value>(&file_path) {
            Ok(json) => {
                if let Err(error) = release_evidence_bundle_validate_install_verification(&json) {
                    manifest_verified = false;
                    failures.push(serde_json::json!({
                        "code": "install_verification_invalid",
                        "label": label,
                        "path": bundle_path,
                        "message": error.to_string()
                    }));
                }
            }
            Err(error) => {
                manifest_verified = false;
                failures.push(serde_json::json!({
                    "code": "install_verification_unreadable",
                    "label": label,
                    "path": bundle_path,
                    "message": format!("read install verification evidence: {error}")
                }));
            }
        }
    }

    let trust_boundary_verified = release_evidence_bundle_trust_boundary_verified(&manifest);
    if !trust_boundary_verified {
        failures.push(serde_json::json!({
            "code": "trust_boundary_invalid",
            "message": "bundle must preserve read-only control-plane and factory-v3 evaluator-closer ownership"
        }));
    }

    let mut secret_scan_passed = true;
    for relative_path in checksum_manifest.keys() {
        if !release_evidence_bundle_relative_path_allowed(relative_path) {
            continue;
        }
        let file_path = extract_dir.join(relative_path);
        if file_path.is_file() {
            release_evidence_bundle_secret_marker_failures(
                relative_path,
                &file_path,
                &mut failures,
                &mut secret_scan_passed,
            );
        }
    }

    let _ = fs::remove_dir_all(&extract_dir);
    Ok(release_evidence_bundle_verification_report(
        ReleaseEvidenceBundleVerificationReport {
            bundle_path,
            bundle_sha256: &bundle_sha256,
            artifact_count: artifacts.len(),
            files_checked,
            manifest_verified,
            trust_boundary_verified,
            secret_scan_passed,
            failures,
        },
    ))
}

struct ReleaseEvidenceBundleVerificationReport<'a> {
    bundle_path: &'a Path,
    bundle_sha256: &'a str,
    artifact_count: usize,
    files_checked: usize,
    manifest_verified: bool,
    trust_boundary_verified: bool,
    secret_scan_passed: bool,
    failures: Vec<serde_json::Value>,
}

fn release_evidence_bundle_verification_report(
    report: ReleaseEvidenceBundleVerificationReport<'_>,
) -> serde_json::Value {
    let status = if report.failures.is_empty() {
        "verified"
    } else {
        "failed"
    };
    serde_json::json!({
        "schema_version": "ao2.release-evidence-bundle-verification.v1",
        "status": status,
        "bundle": report.bundle_path,
        "bundle_sha256": report.bundle_sha256,
        "artifact_count": report.artifact_count,
        "files_checked": report.files_checked,
        "manifest_verified": report.manifest_verified,
        "trust_boundary_verified": report.trust_boundary_verified,
        "secret_scan_passed": report.secret_scan_passed,
        "failure_count": report.failures.len(),
        "failures": report.failures,
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": false
        }
    })
}

pub(crate) fn release_evidence_bundle_relative_path_allowed(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn release_evidence_bundle_trust_boundary_verified(manifest: &serde_json::Value) -> bool {
    let trust = &manifest["trust_boundary"];
    json_string(trust, "control_plane_role") == "read_only_observer"
        && trust
            .get("mutates_ao_artifacts")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && json_string(trust, "release_acceptance_owner") == "factory-v3 evaluator-closer"
        && trust
            .get("control_plane_approves_release")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
}

pub(crate) fn release_evidence_bundle_secret_marker_failures(
    relative_path: &str,
    file_path: &Path,
    failures: &mut Vec<serde_json::Value>,
    secret_scan_passed: &mut bool,
) {
    let Ok(bytes) = fs::read(file_path) else {
        *secret_scan_passed = false;
        failures.push(serde_json::json!({
            "code": "secret_scan_unreadable",
            "path": relative_path,
            "message": "could not read bundle file for secret scan"
        }));
        return;
    };
    let text = String::from_utf8_lossy(&bytes);
    for marker in [
        "Authorization: Bearer ",
        "AO2_CP_API_TOKEN=",
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
    ] {
        if text.contains(marker) {
            *secret_scan_passed = false;
            failures.push(serde_json::json!({
                "code": "forbidden_secret_marker",
                "path": relative_path,
                "marker": marker,
                "message": "release evidence bundle contains a forbidden secret marker"
            }));
        }
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
        release_evidence_bundle_json_secret_field_failures(
            "$",
            relative_path,
            &value,
            failures,
            secret_scan_passed,
        );
    }
}

fn release_evidence_bundle_json_secret_field_failures(
    json_path: &str,
    relative_path: &str,
    value: &serde_json::Value,
    failures: &mut Vec<serde_json::Value>,
    secret_scan_passed: &mut bool,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{json_path}.{key}");
                let key_lower = key.to_ascii_lowercase();
                if matches!(
                    key_lower.as_str(),
                    "token" | "access_token" | "refresh_token"
                ) {
                    *secret_scan_passed = false;
                    failures.push(serde_json::json!({
                        "code": "forbidden_secret_field",
                        "path": relative_path,
                        "json_path": child_path,
                        "message": "release evidence bundle contains a forbidden secret field"
                    }));
                }
                release_evidence_bundle_json_secret_field_failures(
                    &child_path,
                    relative_path,
                    child,
                    failures,
                    secret_scan_passed,
                );
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                release_evidence_bundle_json_secret_field_failures(
                    &format!("{json_path}[{index}]"),
                    relative_path,
                    child,
                    failures,
                    secret_scan_passed,
                );
            }
        }
        _ => {}
    }
}

pub(crate) fn release_compare_verify(bundle_dir: PathBuf, json: bool) -> Result<()> {
    let report = release_comparison_bundle_verification_json(&bundle_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "release_comparison_verification={}",
            json_string(&report, "status")
        );
        println!("bundle_dir={}", bundle_dir.display());
        println!(
            "latest_release={}",
            json_string(&report, "latest_release_tag")
        );
        println!(
            "manifest_verified={}",
            report["manifest_verified"].as_bool().unwrap_or(false)
        );
        println!(
            "signature_verified={}",
            report["signature_verified"].as_bool().unwrap_or(false)
        );
        println!("reason_count={}", json_array(&report, "reasons").len());
    }
    if json_string(&report, "status") != "verified" {
        anyhow::bail!("release comparison bundle verification failed");
    }
    Ok(())
}

pub(crate) fn release_comparison_bundle_verification_json(
    bundle_dir: &Path,
) -> Result<serde_json::Value> {
    let comparison_path = bundle_dir.join("release-comparison.json");
    let release_history_path = bundle_dir.join("release-history.json");
    let sha256_path = bundle_dir.join("SHA256SUMS");
    let mut reasons = Vec::new();
    if !bundle_dir.is_dir() {
        reasons.push(serde_json::json!({
            "code": "bundle_dir_missing",
            "message": format!("bundle directory does not exist: {}", bundle_dir.display())
        }));
    }

    let comparison = match read_json_for_verification(&comparison_path, &mut reasons) {
        Some(value) => value,
        None => serde_json::Value::Null,
    };
    let release_history = match read_json_for_verification(&release_history_path, &mut reasons) {
        Some(value) => value,
        None => serde_json::Value::Null,
    };
    if !comparison.is_null()
        && json_string(&comparison, "schema_version") != "ao2.release-comparison-bundle.v1"
    {
        reasons.push(serde_json::json!({
            "code": "invalid_comparison_schema",
            "message": "release-comparison.json must use schema ao2.release-comparison-bundle.v1"
        }));
    }
    if !release_history.is_null()
        && json_string(&release_history, "schema_version") != "ao2.release-history.v1"
    {
        reasons.push(serde_json::json!({
            "code": "invalid_release_history_schema",
            "message": "release-history.json must use schema ao2.release-history.v1"
        }));
    }
    if !comparison.is_null()
        && !release_history.is_null()
        && comparison["release_history"] != release_history
    {
        reasons.push(serde_json::json!({
            "code": "release_history_mismatch",
            "message": "release-history.json must match release_comparison.release_history"
        }));
    }

    let files = json_array(&comparison, "files");
    let expected_paths = files
        .iter()
        .map(|file| json_string(file, "path"))
        .filter(|path| !path.is_empty())
        .collect::<BTreeSet<_>>();
    let manifest_reason_start = reasons.len();
    for required in [
        "release-comparison.json",
        "release-history.json",
        "SHA256SUMS",
        "release-comparison-metadata.json",
        "release-comparison-metadata.json.sig",
        "release-comparison-signing-public.pem",
    ] {
        if !expected_paths.contains(required) {
            reasons.push(serde_json::json!({
                "code": "missing_expected_file_entry",
                "message": format!("bundle manifest is missing {required}")
            }));
        }
    }

    let mut manifest_verified = true;
    let manifest = match fs::read_to_string(&sha256_path) {
        Ok(body) => checksum_manifest_map(&body, &mut reasons),
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "sha256_manifest_unreadable",
                "message": format!("read {}: {error}", sha256_path.display())
            }));
            BTreeMap::new()
        }
    };
    for relative_path in expected_paths
        .iter()
        .filter(|path| path.as_str() != "SHA256SUMS")
    {
        let Some(expected_sha256) = manifest.get(relative_path) else {
            manifest_verified = false;
            reasons.push(serde_json::json!({
                "code": "missing_sha256_entry",
                "message": format!("SHA256SUMS is missing {relative_path}")
            }));
            continue;
        };
        let file_path = bundle_dir.join(relative_path);
        if !file_path.is_file() {
            manifest_verified = false;
            reasons.push(serde_json::json!({
                "code": "bundle_file_missing",
                "message": format!("bundle file is missing: {relative_path}")
            }));
            continue;
        }
        match sha256_file(&file_path) {
            Ok(actual_sha256) if actual_sha256 == *expected_sha256 => {}
            Ok(actual_sha256) => {
                manifest_verified = false;
                reasons.push(serde_json::json!({
                    "code": "sha256_mismatch",
                    "message": format!("{relative_path} sha256 mismatch"),
                    "expected": expected_sha256,
                    "actual": actual_sha256
                }));
            }
            Err(error) => {
                manifest_verified = false;
                reasons.push(serde_json::json!({
                    "code": "sha256_unreadable",
                    "message": format!("hash {relative_path}: {error}")
                }));
            }
        }
    }
    for relative_path in manifest.keys() {
        if !expected_paths.contains(relative_path) {
            manifest_verified = false;
            reasons.push(serde_json::json!({
                "code": "unexpected_sha256_entry",
                "message": format!("SHA256SUMS contains unexpected file {relative_path}")
            }));
        }
    }
    if manifest.values().any(|sha256| sha256.len() != 64) {
        manifest_verified = false;
    }
    if reasons.len() > manifest_reason_start {
        manifest_verified = false;
    }

    let metadata_report = match release_comparison_metadata_verification_json(bundle_dir) {
        Ok(report) => report,
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "metadata_verification_failed",
                "message": error.to_string()
            }));
            serde_json::json!({
                "present": false,
                "signature_verified": false
            })
        }
    };
    let signature_verified = metadata_report["signature_verified"]
        .as_bool()
        .unwrap_or(false);
    if !signature_verified {
        reasons.push(serde_json::json!({
            "code": "signature_not_verified",
            "message": "signed release comparison metadata is required and must verify"
        }));
    }
    let metadata = metadata_report
        .get("metadata")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    verify_release_comparison_metadata_consistency(
        &metadata,
        &comparison,
        &comparison_path,
        &release_history_path,
        bundle_dir,
        &mut reasons,
    );

    let status = if reasons.is_empty() {
        "verified"
    } else {
        "failed"
    };
    let trend = &comparison["release_history"]["trend"];
    Ok(serde_json::json!({
        "schema_version": "ao2.release-comparison-verification.v1",
        "status": status,
        "bundle_dir": bundle_dir,
        "comparison_path": comparison_path,
        "release_history_path": release_history_path,
        "sha256_manifest": sha256_path,
        "manifest_verified": manifest_verified,
        "signature_verified": signature_verified,
        "signer_id": json_string(&metadata, "signer_id"),
        "latest_release_tag": json_string(trend, "latest_release_tag"),
        "release_count": json_u64(trend, "entry_count"),
        "latest_health_score": json_u64(trend, "latest_health_score"),
        "max_health_score": json_u64(trend, "max_health_score"),
        "attention_count": json_u64(trend, "attention_count"),
        "regression_count": json_u64(trend, "regression_count"),
        "files_checked": expected_paths.len(),
        "reasons": reasons,
        "support_metadata": metadata_report
    }))
}

pub(crate) fn read_json_for_verification(
    path: &Path,
    reasons: &mut Vec<serde_json::Value>,
) -> Option<serde_json::Value> {
    match fs::read_to_string(path) {
        Ok(body) => match serde_json::from_str(&body) {
            Ok(value) => Some(value),
            Err(error) => {
                reasons.push(serde_json::json!({
                    "code": "json_parse_failed",
                    "message": format!("parse {}: {error}", path.display())
                }));
                None
            }
        },
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "json_read_failed",
                "message": format!("read {}: {error}", path.display())
            }));
            None
        }
    }
}

pub(crate) fn checksum_manifest_map(
    body: &str,
    reasons: &mut Vec<serde_json::Value>,
) -> BTreeMap<String, String> {
    let mut manifest = BTreeMap::new();
    for (index, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((sha256, path)) = line.split_once("  ") else {
            reasons.push(serde_json::json!({
                "code": "invalid_sha256_line",
                "message": format!("SHA256SUMS line {} is invalid", index + 1)
            }));
            continue;
        };
        if sha256.len() != 64 || !sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
            reasons.push(serde_json::json!({
                "code": "invalid_sha256_digest",
                "message": format!("SHA256SUMS line {} has an invalid digest", index + 1)
            }));
        }
        manifest.insert(path.to_string(), sha256.to_ascii_lowercase());
    }
    manifest
}

fn verify_release_comparison_metadata_consistency(
    metadata: &serde_json::Value,
    comparison: &serde_json::Value,
    comparison_path: &Path,
    release_history_path: &Path,
    bundle_dir: &Path,
    reasons: &mut Vec<serde_json::Value>,
) {
    if metadata.is_null() {
        return;
    }
    let expected_comparison_sha256 = match sha256_file(comparison_path) {
        Ok(sha256) => sha256,
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "comparison_sha256_unreadable",
                "message": format!("hash {}: {error}", comparison_path.display())
            }));
            String::new()
        }
    };
    let expected_release_history_sha256 = match sha256_file(release_history_path) {
        Ok(sha256) => sha256,
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "release_history_sha256_unreadable",
                "message": format!("hash {}: {error}", release_history_path.display())
            }));
            String::new()
        }
    };
    let public_key_path = bundle_dir.join("release-comparison-signing-public.pem");
    let expected_public_key_sha256 = match sha256_file(&public_key_path) {
        Ok(sha256) => sha256,
        Err(error) => {
            reasons.push(serde_json::json!({
                "code": "public_key_sha256_unreadable",
                "message": format!("hash {}: {error}", public_key_path.display())
            }));
            String::new()
        }
    };
    for (field, expected) in [
        ("release_comparison_sha256", expected_comparison_sha256),
        ("release_history_sha256", expected_release_history_sha256),
        ("public_key_sha256", expected_public_key_sha256),
    ] {
        if !expected.is_empty() && json_string(metadata, field) != expected {
            reasons.push(serde_json::json!({
                "code": "metadata_hash_mismatch",
                "message": format!("metadata field {field} does not match bundle contents")
            }));
        }
    }
    let trend = &comparison["release_history"]["trend"];
    for (field, expected) in [
        ("release_count", json_u64(trend, "entry_count")),
        (
            "latest_health_score",
            json_u64(trend, "latest_health_score"),
        ),
        ("max_health_score", json_u64(trend, "max_health_score")),
        ("attention_count", json_u64(trend, "attention_count")),
        ("regression_count", json_u64(trend, "regression_count")),
    ] {
        if json_u64(metadata, field) != expected {
            reasons.push(serde_json::json!({
                "code": "metadata_trend_mismatch",
                "message": format!("metadata field {field} does not match release history trend")
            }));
        }
    }
    if json_string(metadata, "latest_release_tag") != json_string(trend, "latest_release_tag") {
        reasons.push(serde_json::json!({
            "code": "metadata_latest_release_mismatch",
            "message": "metadata latest_release_tag does not match release history trend"
        }));
    }
}
