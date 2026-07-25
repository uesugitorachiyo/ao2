use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::cli_util::{
    atomic_write_text, escape_html, json_array, json_string, now_unix_ms, read_json_file,
    sha256_file,
};
use crate::control_plane_http::{control_plane_endpoint, get_text_http};
use crate::evidence_publish::{
    evidence_pack_publish_to_control_plane_json, operator_packet_publish_to_control_plane_json,
};
use crate::release_comparison::release_comparison_bundle_verification_json;
use crate::release_history::workbench_release_history_for_dir;
use crate::workbench_contract::WorkbenchSupportSigning;
use crate::workbench_provider_pilot_acceptance::provider_pilot_acceptance_verification_json;
use crate::workbench_run_evidence::{
    workbench_run_evidence_changes_json, workbench_run_evidence_diff_json,
    workbench_run_evidence_summary_json,
};
use crate::{form_value_owned, is_sha256_hex};

pub(super) fn workbench_evidence_export_path(
    target: &Path,
    generated_at_ms: u64,
    kind: &str,
) -> PathBuf {
    target
        .join(".ao2")
        .join("workbench")
        .join("evidence-exports")
        .join(format!("evidence-export-{generated_at_ms}-{kind}.json"))
}

pub(super) fn workbench_evidence_export_json(
    target: &Path,
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let kind = form_value_owned(form, "kind").unwrap_or_else(|| "summary".to_string());
    let generated_at_ms = now_unix_ms();
    let export_body = match kind.as_str() {
        "summary" => {
            let run_id = form_value_owned(form, "run_id").context("run_id is required")?;
            let summary = workbench_run_evidence_summary_json(target, &format!("run_id={run_id}"))?;
            serde_json::json!({
                "summary": summary
            })
        }
        "diff" => {
            let left_run_id =
                form_value_owned(form, "left_run_id").context("left_run_id is required")?;
            let right_run_id =
                form_value_owned(form, "right_run_id").context("right_run_id is required")?;
            let diff = workbench_run_evidence_diff_json(
                target,
                &format!("left_run_id={left_run_id}&right_run_id={right_run_id}"),
            )?;
            serde_json::json!({
                "diff": diff
            })
        }
        "changes" => {
            let run_id = form_value_owned(form, "run_id").context("run_id is required")?;
            let changes = workbench_run_evidence_changes_json(target, &format!("run_id={run_id}"))?;
            serde_json::json!({
                "changes": changes
            })
        }
        "operator-packet" => {
            let run_id = form_value_owned(form, "run_id").context("run_id is required")?;
            let operator_packet = workbench_operator_evidence_packet_json(target, &run_id)?;
            serde_json::json!({
                "operator_packet": operator_packet
            })
        }
        "release-history" => {
            let release_download_dir = form_value_owned(form, "release_download_dir")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("target/release-download"));
            let release_history = workbench_release_history_for_dir(release_download_dir)?;
            serde_json::json!({
                "release_history": release_history
            })
        }
        "release-comparison-verification" => {
            let bundle_dir = form_value_owned(form, "bundle_dir")
                .map(PathBuf::from)
                .context("bundle_dir is required")?;
            let verification = release_comparison_bundle_verification_json(&bundle_dir)?;
            if json_string(&verification, "status") != "verified" {
                anyhow::bail!(
                    "release comparison bundle must verify before export: status={}",
                    json_string(&verification, "status")
                );
            }
            serde_json::json!({
                "release_comparison_bundle_dir": bundle_dir,
                "release_comparison_verification": verification
            })
        }
        "provider-pilot-acceptance" => {
            let acceptance_bundle = form_value_owned(form, "acceptance_bundle")
                .map(PathBuf::from)
                .context("acceptance_bundle is required")?;
            let acceptance = provider_pilot_acceptance_verification_json(&acceptance_bundle)?;
            serde_json::json!({
                "provider_pilot_acceptance_bundle": acceptance_bundle,
                "provider_pilot_acceptance": acceptance
            })
        }
        other => return Err(anyhow!("unsupported evidence export kind {other}")),
    };
    let export_path = workbench_evidence_export_path(target, generated_at_ms, &kind);
    let export = serde_json::json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": kind,
        "target": target,
        "export": export_body
    });
    atomic_write_text(&export_path, &serde_json::to_string_pretty(&export)?)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-evidence-export.v1",
        "generated_at_ms": generated_at_ms,
        "export_kind": kind,
        "export_path": export_path,
        "export": export_body
    }))
}

fn workbench_operator_evidence_packet_json(
    target: &Path,
    run_id: &str,
) -> Result<serde_json::Value> {
    let summary = workbench_run_evidence_summary_json(target, &format!("run_id={run_id}"))?;
    let run_record_path = PathBuf::from(json_string(&summary, "run_record"));
    let evidence_pack_path = PathBuf::from(json_string(&summary, "evidence_pack"));
    let static_report_path = PathBuf::from(json_string(&summary, "static_report"));
    let cockpit_path = PathBuf::from(json_string(&summary, "cockpit"));
    let run_record = read_json_file::<serde_json::Value>(&run_record_path)?;
    let evidence_pack = read_json_file::<serde_json::Value>(&evidence_pack_path)?;
    let static_report_html = fs::read_to_string(&static_report_path)
        .with_context(|| format!("read {}", static_report_path.display()))?;
    let cockpit_html = fs::read_to_string(&cockpit_path).unwrap_or_default();
    Ok(serde_json::json!({
        "schema_version": "ao2.operator-evidence-packet.v1",
        "run_id": run_id,
        "target": target,
        "generated_at_ms": now_unix_ms(),
        "summary": summary,
        "artifacts": {
            "run_record": {
                "path": run_record_path,
                "sha256": sha256_file(&run_record_path)?
            },
            "evidence_pack": {
                "path": evidence_pack_path,
                "sha256": sha256_file(&evidence_pack_path)?
            },
            "static_report": {
                "path": static_report_path,
                "sha256": sha256_file(&static_report_path)?,
                "html": static_report_html
            },
            "cockpit": {
                "path": cockpit_path,
                "sha256": sha256_file(&cockpit_path).unwrap_or_default(),
                "html": cockpit_html
            }
        },
        "run_record": run_record,
        "evidence_pack": evidence_pack,
        "evaluator_closure": {
            "verdict": json_string(&summary, "verdict"),
            "closures": json_array(&summary, "closures")
        },
        "replay": summary["replay"].clone(),
        "provider_scorecard": summary["scorecard"].clone(),
        "report_sections": summary["report_sections"].clone()
    }))
}

pub(super) fn workbench_evidence_publish_json(
    target: &Path,
    form: &BTreeMap<String, String>,
    support_signing: Option<&WorkbenchSupportSigning>,
) -> Result<serde_json::Value> {
    let signing = support_signing
        .context("start workbench with --support-signing-key to publish signed evidence")?;
    let kind = form_value_owned(form, "kind").unwrap_or_else(|| "evidence-pack".to_string());
    let run_id = form_value_owned(form, "run_id").context("run_id is required")?;
    let control_plane_url =
        form_value_owned(form, "control_plane_url").context("control_plane_url is required")?;
    let api_token = form_value_owned(form, "api_token").context("api_token is required")?;
    let mut result = match kind.as_str() {
        "evidence-pack" | "pack" => {
            let summary = workbench_run_evidence_summary_json(target, &format!("run_id={run_id}"))?;
            let evidence_pack = PathBuf::from(json_string(&summary, "evidence_pack"));
            evidence_pack_publish_to_control_plane_json(
                &evidence_pack,
                &signing.key_path,
                &signing.signer_id,
                &control_plane_url,
                &api_token,
            )?
        }
        "operator-packet" => {
            let mut export_form = BTreeMap::new();
            export_form.insert("kind".to_string(), "operator-packet".to_string());
            export_form.insert("run_id".to_string(), run_id.clone());
            let export = workbench_evidence_export_json(target, &export_form)?;
            let export_path = PathBuf::from(json_string(&export, "export_path"));
            operator_packet_publish_to_control_plane_json(
                &export_path,
                &signing.key_path,
                &signing.signer_id,
                &control_plane_url,
                &api_token,
            )?
        }
        other => return Err(anyhow!("unsupported evidence publish kind {other}")),
    };
    let receipt_html = evidence_publish_receipt_html(&result);
    if let Some(object) = result.as_object_mut() {
        object.insert("publish_kind".to_string(), serde_json::json!(kind));
        object.insert("detail_html".to_string(), serde_json::json!(receipt_html));
    }
    Ok(result)
}

pub(super) fn workbench_evidence_control_plane_detail_json(
    form: &BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let sha = form_value_owned(form, "sha256").context("sha256 is required")?;
    if !is_sha256_hex(&sha) {
        return Err(anyhow!(
            "sha256 must be a 64-character lowercase hex digest"
        ));
    }
    let control_plane_url =
        form_value_owned(form, "control_plane_url").context("control_plane_url is required")?;
    let api_token = form_value_owned(form, "api_token").context("api_token is required")?;
    let endpoint = control_plane_endpoint(
        &control_plane_url,
        &format!("/api/v1/evidence-pack/{sha}/detail"),
    )?;
    let detail_html = get_text_http(&endpoint, &api_token)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.evidence-control-plane-detail.v1",
        "sha256": sha,
        "endpoint": endpoint,
        "detail_html": detail_html
    }))
}

fn evidence_publish_receipt_html(result: &serde_json::Value) -> String {
    let receipt = &result["receipt"];
    let signature = &result["signature"];
    let sha = json_string(receipt, "sha256");
    let signer_id = json_string(signature, "signer_id");
    let public_key_sha256 = json_string(signature, "public_key_sha256");
    let signature_sha256 = json_string(signature, "signature_sha256");
    let detail_url = json_string(result, "detail_url");
    let dashboard_url = json_string(result, "dashboard_url");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>AO2 Evidence Publish Receipt</title><style>body{{font-family:system-ui,sans-serif;margin:2rem;max-width:72rem}}dl{{display:grid;grid-template-columns:max-content 1fr;gap:.5rem 1rem}}dt{{font-weight:700}}code{{font-family:ui-monospace,monospace;overflow-wrap:anywhere}}.verified{{color:#096b36;font-weight:700}}.muted{{color:#555}}</style></head><body><main><h1>AO2 Evidence Publish Receipt</h1><p class=\"muted\">Token-safe local receipt. The control-plane detail URL below still requires an authenticated operator request.</p><dl><dt>SHA256</dt><dd><code>{sha}</code></dd><dt>Signer</dt><dd>{signer_id}</dd><dt>Signature</dt><dd><span class=\"verified\">Signed upload accepted</span></dd><dt>Public key SHA256</dt><dd><code>{public_key_sha256}</code></dd><dt>Signature SHA256</dt><dd><code>{signature_sha256}</code></dd><dt>Control-plane detail</dt><dd><code>{detail_url}</code></dd><dt>Control-plane dashboard</dt><dd><code>{dashboard_url}</code></dd></dl></main></body></html>",
        sha = escape_html(&sha),
        signer_id = escape_html(&signer_id),
        public_key_sha256 = escape_html(&public_key_sha256),
        signature_sha256 = escape_html(&signature_sha256),
        detail_url = escape_html(&detail_url),
        dashboard_url = escape_html(&dashboard_url),
    )
}
