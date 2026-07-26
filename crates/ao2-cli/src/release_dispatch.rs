use crate::cli::ReleaseCommand;
use crate::cli_util::{json_string, json_u64, resolve_api_token};
use crate::phase1_promotion::{
    phase1_promotion_decision_build_json, phase1_promotion_decision_publish_to_control_plane_json,
    phase1_promotion_history_fetch_from_control_plane_json,
    phase1_promotion_inputs_publish_to_control_plane_json, phase1_promotion_inputs_verify_json,
    phase1_promotion_status_json, phase1_three_os_smoke_build_json,
    phase1_three_os_smoke_publish_to_control_plane_json,
};
use crate::release_comparison::{
    release_compare, release_compare_verify, release_evidence_bundle_json,
    release_evidence_bundle_verification_json, release_support_bundle_build,
    release_support_bundle_verify,
};
use crate::release_gate::release_gate;
use crate::release_handoff::{
    release_evaluator_decision_build, release_evaluator_decision_markdown,
    release_handoff_checklist_build, release_handoff_checklist_markdown,
};
use crate::release_package::package_release;
use crate::release_provenance::{release_sign_provenance, release_verify_provenance};
use crate::release_summary::release_smoke_summary;
use crate::release_summary_enrich::release_summary_enrich;
use anyhow::{Context, Result};
use std::fs;
pub(crate) fn release(command: ReleaseCommand) -> Result<()> {
    match command {
        ReleaseCommand::Package {
            out_dir,
            version,
            binary,
            target_label,
        } => package_release(out_dir, version, binary, target_label),
        ReleaseCommand::SmokeSummary {
            summary,
            require_native_windows,
        } => release_smoke_summary(summary, require_native_windows),
        ReleaseCommand::SummaryEnrich {
            summary,
            target,
            run_id,
            obligation_gates,
            out,
            json,
        } => release_summary_enrich(summary, target, run_id, obligation_gates, out, json),
        ReleaseCommand::Gate {
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
            allow_unsigned_obligation_gates,
            require_obligation_gate_signing: _legacy_require_obligation_gate_signing,
        } => release_gate(
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
            !allow_unsigned_obligation_gates,
        ),
        ReleaseCommand::Compare {
            release_download_dir,
            out_dir,
            signing_key,
            signer_id,
            json,
        } => release_compare(release_download_dir, out_dir, signing_key, signer_id, json),
        ReleaseCommand::CompareVerify { bundle_dir, json } => {
            release_compare_verify(bundle_dir, json)
        }
        ReleaseCommand::SupportBundleBuild {
            release_assembly,
            readiness,
            handoff,
            cockpit,
            evaluator_decision,
            storage_support,
            replay,
            report_contract_verification,
            install_verification,
            hosted_release_smoke,
            report_target,
            report_run_id,
            report,
            report_index,
            operator_evidence,
            out_dir,
            json,
        } => release_support_bundle_build(
            release_assembly,
            readiness,
            handoff,
            cockpit,
            evaluator_decision,
            storage_support,
            replay,
            report_contract_verification,
            install_verification,
            hosted_release_smoke,
            report_target,
            report_run_id,
            report,
            report_index,
            operator_evidence,
            out_dir,
            json,
        ),
        ReleaseCommand::SupportBundleVerify {
            bundle,
            checksums,
            json,
        } => release_support_bundle_verify(bundle, checksums, json),
        ReleaseCommand::EvidenceBundle {
            out_dir,
            artifacts,
            json,
        } => {
            let result = release_evidence_bundle_json(out_dir, &artifacts)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("archive={}", json_string(&result, "archive"));
                println!("sha256={}", json_string(&result, "sha256"));
                println!("artifact_count={}", json_u64(&result, "artifact_count"));
            }
            Ok(())
        }
        ReleaseCommand::EvidenceBundleVerify { bundle, json } => {
            let report = release_evidence_bundle_verification_json(&bundle)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "release_evidence_bundle_verification={}",
                    json_string(&report, "status")
                );
                println!("bundle={}", bundle.display());
                println!(
                    "manifest_verified={}",
                    report["manifest_verified"].as_bool().unwrap_or(false)
                );
                println!(
                    "trust_boundary_verified={}",
                    report["trust_boundary_verified"].as_bool().unwrap_or(false)
                );
                println!(
                    "secret_scan_passed={}",
                    report["secret_scan_passed"].as_bool().unwrap_or(false)
                );
                println!("failure_count={}", json_u64(&report, "failure_count"));
            }
            if json_string(&report, "status") != "verified" {
                anyhow::bail!("release evidence bundle verification failed");
            }
            Ok(())
        }
        ReleaseCommand::Phase1DecisionBuild {
            release_gate,
            replacement_smoke_gate,
            governed_run_evidence,
            factory_project_run_summaries,
            provider_acceptance_preservation,
            operator,
            rationale,
            out,
            checklist_out,
            json,
        } => {
            let result = phase1_promotion_decision_build_json(
                &release_gate,
                replacement_smoke_gate.as_deref(),
                &governed_run_evidence,
                &factory_project_run_summaries,
                provider_acceptance_preservation.as_deref(),
                &operator,
                &rationale,
                &out,
                checklist_out.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("decision={}", json_string(&result, "decision_path"));
                println!("checklist={}", json_string(&result, "checklist_path"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1DecisionPublish {
            decision,
            signing_key,
            signer_id,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let api_token = resolve_api_token(api_token.as_deref(), api_token_env.as_deref())?;
            let result = phase1_promotion_decision_publish_to_control_plane_json(
                &decision,
                &signing_key,
                &signer_id,
                &control_plane_url,
                &api_token,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("decision={}", json_string(&result, "decision_path"));
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
                println!("signature_url={}", json_string(&result, "signature_url"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1ThreeOsSmokeBuild {
            summary,
            provenance,
            out,
            json,
        } => {
            let result = phase1_three_os_smoke_build_json(&summary, &provenance, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("smoke={}", json_string(&result, "smoke_path"));
                println!("summary={}", json_string(&result, "summary_path"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1ThreeOsSmokePublish {
            smoke,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let result = phase1_three_os_smoke_publish_to_control_plane_json(
                &smoke,
                &control_plane_url,
                api_token.as_deref(),
                api_token_env.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("smoke={}", json_string(&result, "smoke_path"));
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
            }
            Ok(())
        }
        ReleaseCommand::Phase1HistoryFetch {
            control_plane_url,
            api_token,
            api_token_env,
            out,
            json,
        } => {
            let result = phase1_promotion_history_fetch_from_control_plane_json(
                &control_plane_url,
                api_token.as_deref(),
                api_token_env.as_deref(),
                out.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("dashboard={}", json_string(&result, "dashboard_url"));
                if let Some(path) = out {
                    println!("out={}", path.display());
                }
                println!(
                    "checklists={}",
                    json_u64(&result["history"]["counts"], "checklists")
                );
                println!(
                    "signed_decisions={}",
                    json_u64(&result["history"]["counts"], "signed_decisions")
                );
                println!(
                    "three_os_smokes={}",
                    json_u64(&result["history"]["counts"], "three_os_smokes")
                );
            }
            Ok(())
        }
        ReleaseCommand::Phase1PromotionStatus {
            root,
            evidence_bundle,
            json,
        } => {
            let result = phase1_promotion_status_json(&root, evidence_bundle.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("root={}", root.display());
                println!(
                    "release_gate={}",
                    json_string(&result["artifacts"], "release_gate")
                );
                println!("decision={}", json_string(&result["artifacts"], "decision"));
                println!(
                    "checklist={}",
                    json_string(&result["artifacts"], "checklist")
                );
                println!(
                    "evidence_bundle={}",
                    json_string(&result["artifacts"], "evidence_bundle")
                );
                println!(
                    "dashboard_snapshot={}",
                    json_string(&result["checks"], "dashboard_snapshot")
                );
                println!(
                    "dashboard_snapshot_index={}",
                    json_string(&result["artifacts"], "dashboard_snapshot_index")
                );
                println!("failure_count={}", json_u64(&result, "failure_count"));
            }
            if json_string(&result, "status") != "ready" {
                anyhow::bail!("Phase 1 promotion status is not ready");
            }
            Ok(())
        }
        ReleaseCommand::Phase1PromotionInputsVerify {
            manifest,
            out,
            mode,
            json,
        } => {
            let result = phase1_promotion_inputs_verify_json(&manifest, out.as_deref(), &mode)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!("manifest={}", manifest.display());
                if let Some(path) = out {
                    println!("out={}", path.display());
                }
                println!(
                    "missing_required_inputs={}",
                    result["missing_required_inputs"]
                        .as_array()
                        .map(|items| items.len())
                        .unwrap_or(0)
                );
            }
            if json_string(&result, "status") != "accepted" {
                anyhow::bail!("Phase 1 promotion inputs verification failed");
            }
            Ok(())
        }
        ReleaseCommand::Phase1PromotionInputsPublish {
            verification,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let result = phase1_promotion_inputs_publish_to_control_plane_json(
                &verification,
                &control_plane_url,
                api_token.as_deref(),
                api_token_env.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("verification={}", json_string(&result, "verification_path"));
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
            }
            Ok(())
        }
        ReleaseCommand::HandoffChecklistBuild {
            handoff,
            write_json,
            write_md,
            expected_repo_head,
            allow_skipped,
            json,
        } => {
            let payload =
                release_handoff_checklist_build(&handoff, &expected_repo_head, allow_skipped)?;
            if let Some(path) = write_json.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let mut text = serde_json::to_string_pretty(&payload)?;
                text.push('\n');
                fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
            }
            if let Some(path) = write_md.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let md = release_handoff_checklist_markdown(&payload);
                fs::write(path, md).with_context(|| format!("write {}", path.display()))?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                println!("{}", json_string(&payload, "status"));
            }
            Ok(())
        }
        ReleaseCommand::EvaluatorDecisionBuild {
            readiness,
            handoff_checklist,
            support_bundle_status,
            write_json,
            write_md,
            json,
        } => {
            let payload = release_evaluator_decision_build(
                &readiness,
                &handoff_checklist,
                &support_bundle_status,
            )?;
            if let Some(path) = write_json.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let mut text = serde_json::to_string_pretty(&payload)?;
                text.push('\n');
                fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
            }
            if let Some(path) = write_md.as_deref() {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("create parent dir {}", parent.display()))?;
                    }
                }
                let md = release_evaluator_decision_markdown(&payload);
                fs::write(path, md).with_context(|| format!("write {}", path.display()))?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                println!("{}", json_string(&payload, "status"));
            }
            Ok(())
        }
        ReleaseCommand::SignProvenance {
            version,
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            private_key,
            release_tag,
            json,
        } => release_sign_provenance(
            version,
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            private_key,
            release_tag,
            json,
        ),
        ReleaseCommand::VerifyProvenance {
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            public_key,
            json,
        } => release_verify_provenance(
            macos_archive,
            linux_archive,
            linux_x86_64_archive,
            windows_archive,
            provenance_dir,
            public_key,
            json,
        ),
    }
}
