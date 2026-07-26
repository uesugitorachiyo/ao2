use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::artifact_safety::factory_app_run_bundle_reject_secret_markers;
use crate::cli_util::{
    atomic_write_text, canonical_json_sha256, fail_if_provider_api_key_env_present, json_array,
    json_bool, json_string, sha256_file,
};
use crate::plugin_distribution::{
    validate_plugin_observer_trust_boundary, validate_plugin_provider_auth,
};

#[derive(Debug, Subcommand)]
pub(crate) enum SkillContractManifestCommand {
    /// Generate the AO2-produced factory-v3 skill/contract migration manifest.
    Generate {
        #[arg(long = "factory-v3-root", default_value = "../factory-v3")]
        factory_v3_root: PathBuf,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned skill/contract migration manifest guardrail.
    Verify {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long = "manifest-sha256")]
        manifest_sha256: String,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn skill_contract_manifest(command: SkillContractManifestCommand) -> Result<()> {
    match command {
        SkillContractManifestCommand::Generate {
            factory_v3_root,
            out_dir,
            json,
        } => skill_contract_manifest_generate(factory_v3_root, out_dir, json),
        SkillContractManifestCommand::Verify {
            manifest,
            manifest_sha256,
            json,
        } => skill_contract_manifest_verify(manifest, manifest_sha256, json),
    }
}

const SKILL_CONTRACT_REQUIRED_INVENTORY: [&str; 7] = [
    "intake",
    "closure_verification",
    "evaluator_closer_acceptance",
    "provider_auth_rules",
    "redaction_token_safety",
    "cross_platform_proof",
    "plugin_shipment_runbook_rules",
];

fn skill_contract_manifest_generate(
    factory_v3_root: PathBuf,
    out_dir: PathBuf,
    json_output: bool,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let factory_v3_root = fs::canonicalize(&factory_v3_root)
        .with_context(|| format!("canonicalize {}", factory_v3_root.display()))?;
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let entries = serde_json::json!([
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "intake",
            source_relative_path: "agents/intake.toml",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 factory app-run",
                "cli_factory_app_run_derives_evaluator_rubric_before_execution",
                "ao2.factory-app-run.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 owns bounded work intake and must preserve scoped reads/writes, success criteria, and sensitive-field handling.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "closure_verification",
            source_relative_path: "scripts/verify_closure.py",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 factory closer-decision",
                "cli_factory_closer_decision_signs_and_verifies_rubric_bound_closure",
                "ao2.factory-closer-decision.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 signs rubric-bound closer decisions while factory-v3 remains a parity auditor.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "evaluator_closer_acceptance",
            source_relative_path: "agents/evaluator-closer.toml",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 factory evaluator-rubric",
                "cli_factory_evaluator_rubric_emits_signed_acceptance_contract",
                "ao2.factory-acceptance-rubric.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 derives signed acceptance criteria while factory-v3 remains the parity auditor and acceptance-role reference.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "provider_auth_rules",
            source_relative_path: "scripts/factory_doctor.py",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin readiness",
                "cli_plugin_readiness_emits_codex_claude_wrapper_contract",
                "ao2.plugin-readiness.v1"
            )),
            blocker: None,
            trust_boundary_notes: "Provider execution must remain local OAuth CLI-only; provider API-key auth remains forbidden.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "redaction_token_safety",
            source_relative_path: "SETUP.md",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin package-verify",
                "cli_plugin_package_verify_accepts_distributed_archive",
                "ao2.plugin-package-verification.v1"
            )),
            blocker: None,
            trust_boundary_notes: "AO2 artifacts must reject credential-shaped output and preserve token-safe summaries.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "cross_platform_proof",
            source_relative_path: "docs/plans/ao2-factory-v3-replacement-parity-plan.md",
            category: "runtime_critical",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin packaged-replacement-observer-bundle",
                "cli_plugin_packaged_replacement_observer_bundle_packages_three_platform_proofs",
                "ao2.k37-packaged-replacement-hardening-observer-bundle.v1"
            )),
            blocker: None,
            trust_boundary_notes: "macOS, Ubuntu SSH, and direct Windows SSH evidence must be packaged by AO2 before read-only observation.",
        })?,
        skill_contract_manifest_entry(&factory_v3_root, SkillContractManifestEntrySpec {
            name: "plugin_shipment_runbook_rules",
            source_relative_path: "docs/plans/ao2-factory-v3-replacement-parity-plan.md",
            category: "plugin_packaging",
            ao2_disposition: "enforced",
            enforcement: Some((
                "ao2 plugin shipment-readiness",
                "cli_plugin_shipment_readiness_aggregates_operator_handoff_evidence",
                "ao2.plugin-shipment-readiness.v1"
            )),
            blocker: None,
            trust_boundary_notes: "Codex/Claude plugin shipment keeps local OAuth CLI auth, digest gates, token-safe output, and observer-only control-plane boundaries.",
        })?
    ]);

    let manifest = serde_json::json!({
        "schema_version": "ao2.skill-contract-manifest.v1",
        "status": "accepted",
        "producer": "ao2",
        "work_source": "codex-cron AO2 factory-v3 replacement parity",
        "entry_count": entries.as_array().map(Vec::len).unwrap_or_default(),
        "required_inventory": SKILL_CONTRACT_REQUIRED_INVENTORY,
        "entries": entries,
        "entries_sha256": canonical_json_sha256(&entries),
        "guardrails": {
            "runtime_critical_checked": true,
            "runtime_critical_requires_enforcement_or_blocker": true,
            "raw_factory_v3_skill_copy_allowed": false
        },
        "provider_auth": {
            "local_oauth_cli_only": true,
            "provider_api_key_auth_allowed": false,
            "provider_api_key_env_required": false
        },
        "trust_boundary": {
            "execution_owner": "ao2",
            "factory_v3_role": "parity_auditor",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        },
        "control_plane_observation": {
            "role": "read_only_observer",
            "may_observe_evidence_bundle_path": true,
            "may_mutate_evidence": false,
            "may_approve_release": false
        },
        "side_effects": {
            "would_execute_provider": false,
            "would_execute_queue": false,
            "would_write_memory": false,
            "would_mutate_control_plane": false,
            "would_mutate_ao_artifacts": false,
            "would_approve_release": false
        },
        "token_safe_output_verified": true,
        "factory_v3_role": "parity_auditor"
    });
    validate_skill_contract_manifest(&manifest)?;

    let manifest_path = out_dir.join("skill-contract-manifest.json");
    let manifest_body = serde_json::to_string_pretty(&manifest)?;
    atomic_write_text(&manifest_path, &manifest_body)?;
    factory_app_run_bundle_reject_secret_markers(&manifest_path, "skill-contract-manifest.json")?;
    let manifest_sha256 = sha256_file(&manifest_path)?;

    let mut response = manifest;
    response["manifest_path"] = serde_json::json!(manifest_path.display().to_string());
    response["manifest_sha256"] = serde_json::json!(manifest_sha256);
    let response_body = serde_json::to_string_pretty(&response)?;
    if json_output {
        println!("{response_body}");
    } else {
        println!("status=accepted");
        println!("schema_version=ao2.skill-contract-manifest.v1");
        println!("manifest={}", manifest_path.display());
        println!("manifest_sha256={}", response["manifest_sha256"]);
    }
    Ok(())
}

fn skill_contract_manifest_verify(
    manifest: PathBuf,
    manifest_sha256: String,
    json_output: bool,
) -> Result<()> {
    fail_if_provider_api_key_env_present()?;

    let actual_sha256 = sha256_file(&manifest)?;
    if actual_sha256 != manifest_sha256.trim() {
        anyhow::bail!(
            "skill-contract manifest sha256 mismatch for {}: expected {}, actual {}",
            manifest.display(),
            manifest_sha256,
            actual_sha256
        );
    }
    factory_app_run_bundle_reject_secret_markers(&manifest, "skill-contract-manifest.json")?;
    let body =
        fs::read_to_string(&manifest).with_context(|| format!("read {}", manifest.display()))?;
    let manifest_json: serde_json::Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", manifest.display()))?;
    validate_skill_contract_manifest(&manifest_json)?;

    let response = serde_json::json!({
        "schema_version": "ao2.skill-contract-manifest-verification.v1",
        "status": "passed",
        "producer": "ao2",
        "source_schema_version": json_string(&manifest_json, "schema_version"),
        "manifest_path": manifest.display().to_string(),
        "manifest_sha256": actual_sha256,
        "entry_count": json_array(&manifest_json, "entries").len(),
        "runtime_critical_guardrail_verified": true,
        "provider_auth": manifest_json.get("provider_auth").cloned().unwrap_or_else(|| serde_json::json!({})),
        "trust_boundary": manifest_json.get("trust_boundary").cloned().unwrap_or_else(|| serde_json::json!({})),
        "side_effects": manifest_json.get("side_effects").cloned().unwrap_or_else(|| serde_json::json!({})),
        "token_safe_output_verified": json_bool(&manifest_json, "token_safe_output_verified"),
        "factory_v3_role": json_string(&manifest_json, "factory_v3_role")
    });
    let response_body = serde_json::to_string_pretty(&response)?;
    if json_output {
        println!("{response_body}");
    } else {
        println!("status=passed");
        println!("schema_version=ao2.skill-contract-manifest-verification.v1");
        println!("manifest={}", manifest.display());
    }
    Ok(())
}

struct SkillContractManifestEntrySpec<'a> {
    name: &'a str,
    source_relative_path: &'a str,
    category: &'a str,
    ao2_disposition: &'a str,
    enforcement: Option<(&'a str, &'a str, &'a str)>,
    blocker: Option<&'a str>,
    trust_boundary_notes: &'a str,
}

fn skill_contract_manifest_entry(
    factory_v3_root: &Path,
    spec: SkillContractManifestEntrySpec<'_>,
) -> Result<serde_json::Value> {
    let source_path = factory_v3_root.join(spec.source_relative_path);
    if !source_path.is_file() {
        anyhow::bail!(
            "skill-contract source path is missing for {}: {}",
            spec.name,
            source_path.display()
        );
    }
    let source_sha256 = sha256_file(&source_path)?;
    let enforcement = match spec.enforcement {
        Some((ao2_command, ao2_test, ao2_artifact)) => serde_json::json!({
            "ao2_command": ao2_command,
            "ao2_test": ao2_test,
            "ao2_artifact": ao2_artifact
        }),
        None => serde_json::json!({}),
    };
    Ok(serde_json::json!({
        "name": spec.name,
        "source_repo": "factory-v3",
        "source_path": source_path.display().to_string(),
        "source_relative_path": spec.source_relative_path,
        "source_sha256": source_sha256,
        "category": spec.category,
        "ao2_disposition": spec.ao2_disposition,
        "enforcement": enforcement,
        "blocker": spec.blocker,
        "trust_boundary_notes": spec.trust_boundary_notes
    }))
}

fn validate_skill_contract_manifest(manifest: &serde_json::Value) -> Result<()> {
    if json_string(manifest, "schema_version") != "ao2.skill-contract-manifest.v1" {
        anyhow::bail!(
            "skill-contract manifest requires ao2.skill-contract-manifest.v1, got {}",
            json_string(manifest, "schema_version")
        );
    }
    if json_string(manifest, "producer") != "ao2" {
        anyhow::bail!("skill-contract manifest producer must be ao2");
    }
    if json_string(manifest, "status") != "accepted" {
        anyhow::bail!("skill-contract manifest status must be accepted");
    }
    validate_plugin_provider_auth(
        manifest
            .get("provider_auth")
            .context("skill-contract manifest missing provider_auth")?,
        "skill-contract manifest",
    )?;
    validate_plugin_observer_trust_boundary(
        manifest
            .get("trust_boundary")
            .context("skill-contract manifest missing trust_boundary")?,
        "skill-contract manifest",
    )?;
    let side_effects = manifest
        .get("side_effects")
        .context("skill-contract manifest missing side_effects")?;
    for key in [
        "would_execute_provider",
        "would_execute_queue",
        "would_write_memory",
        "would_mutate_control_plane",
        "would_mutate_ao_artifacts",
        "would_approve_release",
    ] {
        if json_bool(side_effects, key) {
            anyhow::bail!("skill-contract manifest side effect {key} must be false");
        }
    }
    if !json_bool(manifest, "token_safe_output_verified") {
        anyhow::bail!("skill-contract manifest must verify token-safe output");
    }

    let entries = json_array(manifest, "entries");
    if entries.len() != SKILL_CONTRACT_REQUIRED_INVENTORY.len() {
        anyhow::bail!(
            "skill-contract manifest requires {} entries, got {}",
            SKILL_CONTRACT_REQUIRED_INVENTORY.len(),
            entries.len()
        );
    }
    let mut names = BTreeSet::new();
    for entry in entries {
        let name = json_string(entry, "name");
        if name.is_empty() {
            anyhow::bail!("skill-contract manifest contains unnamed entry");
        }
        if !names.insert(name.clone()) {
            anyhow::bail!("skill-contract manifest contains duplicate entry {name}");
        }
        let category = json_string(entry, "category");
        if ![
            "runtime_critical",
            "docs_reference_only",
            "plugin_packaging",
            "deprecated_or_not_needed",
        ]
        .contains(&category.as_str())
        {
            anyhow::bail!("skill-contract entry {name} has invalid category {category}");
        }
        let disposition = json_string(entry, "ao2_disposition");
        if !["enforced", "referenced", "blocked", "not_migrated"].contains(&disposition.as_str()) {
            anyhow::bail!("skill-contract entry {name} has invalid AO2 disposition {disposition}");
        }
        let source_path = json_string(entry, "source_path");
        if source_path.is_empty() {
            anyhow::bail!("skill-contract entry {name} missing source path");
        }
        let source_sha256 = json_string(entry, "source_sha256");
        if source_sha256.len() != 64 || !source_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("skill-contract entry {name} source sha256 must be a hex digest");
        }
        let source_path_ref = Path::new(&source_path);
        if source_path_ref.is_file() {
            let actual = sha256_file(source_path_ref)?;
            if actual != source_sha256 {
                anyhow::bail!(
                    "skill-contract entry {name} source sha256 mismatch: expected {}, actual {}",
                    source_sha256,
                    actual
                );
            }
        }
        if json_string(entry, "trust_boundary_notes").is_empty() {
            anyhow::bail!("skill-contract entry {name} missing trust-boundary notes");
        }
        if category == "runtime_critical" {
            let enforcement = entry
                .get("enforcement")
                .and_then(serde_json::Value::as_object);
            let has_enforcement = enforcement
                .map(|enforcement| {
                    ["ao2_command", "ao2_test", "ao2_artifact"]
                        .iter()
                        .all(|key| {
                            enforcement
                                .get(*key)
                                .and_then(serde_json::Value::as_str)
                                .map(|text| !text.trim().is_empty())
                                .unwrap_or(false)
                        })
                })
                .unwrap_or(false);
            let has_blocker = entry
                .get("blocker")
                .and_then(serde_json::Value::as_str)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false);
            if !has_enforcement && !has_blocker {
                anyhow::bail!(
                    "runtime-critical skill-contract entry {name} lacks enforcement or blocker"
                );
            }
        }
    }
    for required in SKILL_CONTRACT_REQUIRED_INVENTORY {
        if !names.contains(required) {
            anyhow::bail!("skill-contract manifest missing required entry {required}");
        }
    }
    Ok(())
}
