use crate::cli::ContractCommand;
use crate::cli_util::{atomic_write_text, json_string};
use crate::contract_gate_signing::{
    contract_obligation_gate_signing_survey_json, contract_verify_obligation_gate_signing_json,
    emit_contract_gate_signed_wrapper,
};
use anyhow::{anyhow, Context, Result};
use ao2_core::{
    annotate_obligation_ledger, check_obligation_ledger, extract_obligation_ledger,
    ObligationEvidence, ObligationLedger, ObligationStatus,
};
use chrono::{SecondsFormat, Utc};
use std::fs;
use std::path::PathBuf;
pub(crate) fn contract(command: ContractCommand) -> Result<()> {
    match command {
        ContractCommand::Extract { spec, out, json } => {
            let content =
                fs::read_to_string(&spec).with_context(|| format!("read {}", spec.display()))?;
            let ledger = extract_obligation_ledger(&spec.to_string_lossy(), &content);
            let body = serde_json::to_string_pretty(&ledger)?;
            atomic_write_text(&out, &body)?;
            if json {
                println!("{body}");
            } else {
                println!("obligation_ledger={}", out.display());
                println!(
                    "verdict={}",
                    json_string(&serde_json::to_value(&ledger)?, "verdict")
                );
                println!("obligation_count={}", ledger.obligations.len());
            }
            Ok(())
        }
        ContractCommand::Check {
            ledger,
            target,
            out,
            json,
        } => {
            let content = fs::read_to_string(&ledger)
                .with_context(|| format!("read {}", ledger.display()))?;
            let ledger: ObligationLedger = serde_json::from_str(&content)
                .with_context(|| format!("parse {}", ledger.display()))?;
            let checked = check_obligation_ledger(&ledger, &target)
                .with_context(|| format!("check obligations under {}", target.display()))?;
            let body = serde_json::to_string_pretty(&checked)?;
            atomic_write_text(&out, &body)?;
            if json {
                println!("{body}");
            } else {
                println!("checked_obligation_ledger={}", out.display());
                println!(
                    "verdict={}",
                    json_string(&serde_json::to_value(&checked)?, "verdict")
                );
                println!("pass={}", checked.summary.pass);
                println!("fail={}", checked.summary.fail);
                println!("unverified={}", checked.summary.unverified);
            }
            if checked.verdict == ao2_core::ObligationVerdict::Accepted {
                Ok(())
            } else {
                Err(anyhow!(
                    "obligation check rejected: pass={} fail={} unverified={}",
                    checked.summary.pass,
                    checked.summary.fail,
                    checked.summary.unverified
                ))
            }
        }
        ContractCommand::Gate {
            ledger,
            target,
            stage,
            out,
            json,
            support_signing_key,
            support_signer_id,
            support_operator_role,
            support_run_id,
            exports_dir,
            allow_unsigned_obligation_gates,
        } => {
            let stage = stage.trim();
            if stage.is_empty() {
                return Err(anyhow!("--stage must not be empty"));
            }
            if support_signing_key.is_none() && !allow_unsigned_obligation_gates {
                return Err(anyhow!(
                    "`ao2 contract gate` requires --support-signing-key by default \
                     (slice 18 producer-side default-on, mirroring slice 11 release-gate \
                     consumer-side flip); pass --allow-unsigned-obligation-gates to opt \
                     out, but downstream `ao2 release gate` and POST /api/release-gate \
                     will still reject the unsigned gate unless their own escape valves \
                     are also set"
                ));
            }
            let content = fs::read_to_string(&ledger)
                .with_context(|| format!("read {}", ledger.display()))?;
            let ledger_value: ObligationLedger = serde_json::from_str(&content)
                .with_context(|| format!("parse {}", ledger.display()))?;
            let checked = check_obligation_ledger(&ledger_value, &target)
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
            let gate = serde_json::json!({
                "schema_version": "ao2.obligation-gate.v1",
                "stage": stage,
                "status": status,
                "verdict": checked.verdict,
                "summary": checked.summary,
                "ledger_path": ledger.display().to_string(),
                "target": target.display().to_string(),
                "gate_path": out.display().to_string(),
                "checked_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                "failed_obligations": failed_obligations,
                "unverified_obligations": unverified_obligations,
                "checked_ledger": checked
            });
            let body = serde_json::to_string_pretty(&gate)?;
            atomic_write_text(&out, &body)?;

            let signing_evidence = if let Some(key_path) = support_signing_key.as_ref() {
                let operator_role = support_operator_role.trim();
                if operator_role.is_empty() {
                    return Err(anyhow!(
                        "--support-operator-role must be non-empty when --support-signing-key is set"
                    ));
                }
                let signer_id = support_signer_id.trim();
                if signer_id.is_empty() {
                    return Err(anyhow!(
                        "--support-signer-id must be non-empty when --support-signing-key is set"
                    ));
                }
                let resolved_exports_dir = exports_dir.clone().unwrap_or_else(|| {
                    out.parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| PathBuf::from("."))
                });
                let signed = emit_contract_gate_signed_wrapper(
                    &gate,
                    &resolved_exports_dir,
                    key_path,
                    signer_id,
                    operator_role,
                    support_run_id.trim(),
                )?;
                Some(signed)
            } else {
                None
            };

            if json {
                if let Some(signing) = signing_evidence.as_ref() {
                    let mut emitted = gate.clone();
                    if let Some(object) = emitted.as_object_mut() {
                        object.insert("support_signing_evidence".to_string(), signing.clone());
                    }
                    println!("{}", serde_json::to_string_pretty(&emitted)?);
                } else {
                    println!("{body}");
                }
            } else {
                println!("obligation_gate={}", out.display());
                println!("stage={}", json_string(&gate, "stage"));
                println!("status={}", json_string(&gate, "status"));
                println!("verdict={}", json_string(&gate, "verdict"));
                println!("fail={}", gate["summary"]["fail"].as_u64().unwrap_or(0));
                println!(
                    "unverified={}",
                    gate["summary"]["unverified"].as_u64().unwrap_or(0)
                );
                if let Some(signing) = signing_evidence.as_ref() {
                    println!("wrapper_path={}", json_string(signing, "wrapper_path"));
                    println!("signature_path={}", json_string(signing, "signature_path"));
                    println!(
                        "public_key_path={}",
                        json_string(signing, "public_key_path")
                    );
                    println!("signature_verified={}", signing["signature_verified"]);
                }
            }
            if json_string(&gate, "status") == "passed" {
                Ok(())
            } else {
                Err(anyhow!(
                    "obligation gate failed at {}: pass={} fail={} unverified={}",
                    json_string(&gate, "stage"),
                    gate["summary"]["pass"].as_u64().unwrap_or(0),
                    gate["summary"]["fail"].as_u64().unwrap_or(0),
                    gate["summary"]["unverified"].as_u64().unwrap_or(0)
                ))
            }
        }
        ContractCommand::SignObligationGate {
            gate,
            support_signing_key,
            support_signer_id,
            support_operator_role,
            support_run_id,
            exports_dir,
            json,
        } => {
            let gate_text =
                fs::read_to_string(&gate).with_context(|| format!("read {}", gate.display()))?;
            let gate_json: serde_json::Value = serde_json::from_str(&gate_text)
                .with_context(|| format!("parse {}", gate.display()))?;
            if json_string(&gate_json, "schema_version") != "ao2.obligation-gate.v1" {
                return Err(anyhow!(
                    "contract sign-obligation-gate requires ao2.obligation-gate.v1: {}",
                    gate.display()
                ));
            }
            let operator_role = support_operator_role.trim();
            if operator_role.is_empty() {
                return Err(anyhow!("--support-operator-role must be non-empty"));
            }
            let signer_id = support_signer_id.trim();
            if signer_id.is_empty() {
                return Err(anyhow!("--support-signer-id must be non-empty"));
            }
            let run_id = support_run_id.trim();
            if run_id.is_empty() {
                return Err(anyhow!("--support-run-id must be non-empty"));
            }
            let resolved_exports_dir = exports_dir.unwrap_or_else(|| {
                gate.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });
            let signing = emit_contract_gate_signed_wrapper(
                &gate_json,
                &resolved_exports_dir,
                &support_signing_key,
                signer_id,
                operator_role,
                run_id,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&signing)?);
            } else {
                println!("wrapper_path={}", json_string(&signing, "wrapper_path"));
                println!("signature_path={}", json_string(&signing, "signature_path"));
                println!(
                    "public_key_path={}",
                    json_string(&signing, "public_key_path")
                );
                println!("signature_verified={}", signing["signature_verified"]);
            }
            Ok(())
        }
        ContractCommand::Annotate {
            ledger,
            obligation_id,
            evidence_path,
            evidence_line,
            detail,
            waiver,
            out,
            json,
        } => {
            let content = fs::read_to_string(&ledger)
                .with_context(|| format!("read {}", ledger.display()))?;
            let ledger: ObligationLedger = serde_json::from_str(&content)
                .with_context(|| format!("parse {}", ledger.display()))?;
            let evidence = match evidence_path {
                Some(path) => {
                    let line = evidence_line
                        .context("--evidence-line is required with --evidence-path")?;
                    if line == 0 {
                        return Err(anyhow!("--evidence-line must be greater than 0"));
                    }
                    Some(ObligationEvidence {
                        path,
                        line,
                        detail: detail
                            .filter(|detail| !detail.trim().is_empty())
                            .unwrap_or_else(|| "manual operator evidence".to_string()),
                    })
                }
                None => None,
            };
            let annotated = annotate_obligation_ledger(&ledger, &obligation_id, evidence, waiver)
                .map_err(|error| anyhow!(error))?;
            let body = serde_json::to_string_pretty(&annotated)?;
            atomic_write_text(&out, &body)?;
            if json {
                println!("{body}");
            } else {
                println!("annotated_obligation_ledger={}", out.display());
                println!(
                    "verdict={}",
                    json_string(&serde_json::to_value(&annotated)?, "verdict")
                );
                println!("pass={}", annotated.summary.pass);
                println!("waived={}", annotated.summary.waived);
                println!("unverified={}", annotated.summary.unverified);
            }
            Ok(())
        }
        ContractCommand::VerifyObligationGateSigning {
            gate,
            evidence_exports_dir,
            public_key,
            json,
        } => {
            let result = contract_verify_obligation_gate_signing_json(
                &gate,
                evidence_exports_dir.as_deref(),
                public_key.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("gate_path={}", json_string(&result, "gate_path"));
                println!("stage={}", json_string(&result, "stage"));
                println!("signing_status={}", json_string(&result, "signing_status"));
                println!("signature_verified={}", result["signature_verified"]);
                println!(
                    "matched_wrapper={}",
                    json_string(&result, "matched_wrapper_path")
                );
                println!("ao2_owned={}", result["ao2_owned"]);
            }
            if json_string(&result, "signing_status") != "signed-and-verified" {
                return Err(anyhow!(
                    "obligation gate {} is not signed-and-verified ({})",
                    gate.display(),
                    json_string(&result, "signing_status")
                ));
            }
            Ok(())
        }
        ContractCommand::ObligationGateSigningSurvey {
            target,
            summary,
            json,
        } => {
            let result = contract_obligation_gate_signing_survey_json(
                target.as_deref(),
                summary.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let sources = result["sources"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                println!("sources={sources}");
                if !json_string(&result, "target").is_empty() {
                    println!("target={}", json_string(&result, "target"));
                }
                if !json_string(&result, "summary").is_empty() {
                    println!("summary={}", json_string(&result, "summary"));
                }
                println!("total_gates={}", result["total_gates"]);
                println!("signed_and_verified={}", result["signed_and_verified"]);
                println!("unsigned={}", result["unsigned"]);
                println!("status={}", json_string(&result, "status"));
            }
            Ok(())
        }
    }
}
