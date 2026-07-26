use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::Subcommand;

use crate::cli_util::{
    base64_standard, hex_lower, json_array, json_string, sha256_bytes_hex, sha256_file,
};
use crate::cli_util::{resolve_api_token, trimmed_required};
use crate::control_plane_http::{control_plane_endpoint, post_json_http};
use crate::release_crypto::{public_key_pem_from_private_key, sign_bytes_with_private_key};

#[derive(Debug, Subcommand)]
pub(crate) enum EvidenceCommand {
    Publish {
        #[arg(long = "evidence-pack")]
        evidence_pack: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-evidence")]
        signer_id: String,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
    PublishOperatorPacket {
        #[arg(long = "operator-packet")]
        operator_packet: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-operator")]
        signer_id: String,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn evidence(command: EvidenceCommand) -> Result<()> {
    match command {
        EvidenceCommand::Publish {
            evidence_pack,
            signing_key,
            signer_id,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let api_token = resolve_api_token(api_token.as_deref(), api_token_env.as_deref())?;
            let result = evidence_pack_publish_to_control_plane_json(
                &evidence_pack,
                &signing_key,
                &signer_id,
                &control_plane_url,
                &api_token,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "evidence_pack={}",
                    json_string(&result, "evidence_pack_path")
                );
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
            }
            Ok(())
        }
        EvidenceCommand::PublishOperatorPacket {
            operator_packet,
            signing_key,
            signer_id,
            control_plane_url,
            api_token,
            api_token_env,
            json,
        } => {
            let api_token = resolve_api_token(api_token.as_deref(), api_token_env.as_deref())?;
            let result = operator_packet_publish_to_control_plane_json(
                &operator_packet,
                &signing_key,
                &signer_id,
                &control_plane_url,
                &api_token,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "operator_packet={}",
                    json_string(&result, "operator_packet_path")
                );
                println!("endpoint={}", json_string(&result, "endpoint"));
                println!("sha256={}", json_string(&result["receipt"], "sha256"));
                println!("detail_url={}", json_string(&result, "detail_url"));
            }
            Ok(())
        }
    }
}

pub(crate) fn evidence_pack_publish_to_control_plane_json(
    evidence_pack_path: &Path,
    signing_key: &Path,
    signer_id: &str,
    control_plane_url: &str,
    api_token: &str,
) -> Result<serde_json::Value> {
    let api_token = trimmed_required("--api-token", api_token)?;
    let signer_id = trimmed_required("--signer-id", signer_id)?;
    let content = fs::read_to_string(evidence_pack_path)
        .with_context(|| format!("read {}", evidence_pack_path.display()))?;
    let evidence_pack: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse {}", evidence_pack_path.display()))?;
    let schema_version = json_string(&evidence_pack, "schema_version");
    if schema_version != "ao2.evidence-pack.v1" {
        return Err(anyhow!(
            "evidence publish requires ao2.evidence-pack.v1, got {schema_version}"
        ));
    }
    let evidence_pack =
        enrich_evidence_pack_with_obligation_gates(evidence_pack_path, evidence_pack)?;
    let evidence_raw = serde_json::to_string_pretty(&evidence_pack)?;
    let signature_bytes = sign_bytes_with_private_key(signing_key, evidence_raw.as_bytes())?;
    let public_key_pem = public_key_pem_from_private_key(signing_key)?;
    let signature = serde_json::json!({
        "schema_version": "ao2.cp-evidence-pack-signature.v1",
        "signature_algorithm": "RSA/SHA-256",
        "signer_id": signer_id,
        "signature_sha256": sha256_bytes_hex(&signature_bytes),
        "signature_hex": hex_lower(&signature_bytes),
        "public_key_sha256": sha256_bytes_hex(public_key_pem.as_bytes()),
        "public_key_pem": public_key_pem
    });
    let endpoint = control_plane_endpoint(control_plane_url, "/api/v1/evidence-pack/signed")?;
    let post_body = serde_json::to_string(&serde_json::json!({
        "schema_version": "ao2.cp-evidence-pack-signed-upload.v1",
        "evidence_pack": evidence_pack,
        // Exact bytes the signature covers: the enriched, pretty-printed pack
        // (`evidence_raw`). Lets the control plane verify over these, not a lossy
        // re-serialization of `evidence_pack`.
        "evidence_pack_b64": base64_standard(evidence_raw.as_bytes()),
        "signature": signature
    }))?;
    let receipt = post_json_http(&endpoint, &api_token, &post_body)?;
    let receipt_sha = json_string(&receipt, "sha256");
    let detail_url = if receipt_sha.is_empty() {
        String::new()
    } else {
        control_plane_endpoint(
            control_plane_url,
            &format!("/api/v1/evidence-pack/{receipt_sha}/detail"),
        )?
    };
    let dashboard_url =
        control_plane_endpoint(control_plane_url, "/api/v1/evidence-pack/dashboard")?;
    Ok(serde_json::json!({
        "schema_version": "ao2.evidence-pack-control-plane-publish.v1",
        "evidence_pack_path": evidence_pack_path,
        "endpoint": endpoint,
        "detail_url": detail_url,
        "dashboard_url": dashboard_url,
        "signature": signature,
        "signed": true,
        "receipt": receipt
    }))
}

pub(crate) fn operator_packet_publish_to_control_plane_json(
    operator_packet_path: &Path,
    signing_key: &Path,
    signer_id: &str,
    control_plane_url: &str,
    api_token: &str,
) -> Result<serde_json::Value> {
    let api_token = trimmed_required("--api-token", api_token)?;
    let signer_id = trimmed_required("--signer-id", signer_id)?;
    let content = fs::read_to_string(operator_packet_path)
        .with_context(|| format!("read {}", operator_packet_path.display()))?;
    let input: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parse {}", operator_packet_path.display()))?;
    let operator_packet = match json_string(&input, "schema_version").as_str() {
        "ao2.operator-evidence-packet.v1" => input,
        "ao2.workbench-evidence-export.v1"
            if json_string(&input, "export_kind") == "operator-packet" =>
        {
            input
                .pointer("/export/operator_packet")
                .cloned()
                .ok_or_else(|| {
                    anyhow!("operator-packet evidence export missing export.operator_packet")
                })?
        }
        other => {
            return Err(anyhow!(
                "operator packet publish requires ao2.operator-evidence-packet.v1 or operator-packet workbench export, got {other}"
            ));
        }
    };
    let schema_version = json_string(&operator_packet, "schema_version");
    if schema_version != "ao2.operator-evidence-packet.v1" {
        return Err(anyhow!(
            "operator packet publish requires ao2.operator-evidence-packet.v1, got {schema_version}"
        ));
    }
    let operator_packet_raw = serde_json::to_string_pretty(&operator_packet)?;
    let signature_bytes = sign_bytes_with_private_key(signing_key, operator_packet_raw.as_bytes())?;
    let public_key_pem = public_key_pem_from_private_key(signing_key)?;
    let signature = serde_json::json!({
        "schema_version": "ao2.cp-operator-packet-signature.v1",
        "signature_algorithm": "RSA/SHA-256",
        "signer_id": signer_id,
        "signature_sha256": sha256_bytes_hex(&signature_bytes),
        "signature_hex": hex_lower(&signature_bytes),
        "public_key_sha256": sha256_bytes_hex(public_key_pem.as_bytes()),
        "public_key_pem": public_key_pem
    });
    let endpoint = control_plane_endpoint(control_plane_url, "/api/v1/operator-packet/signed")?;
    let post_body = serde_json::to_string(&serde_json::json!({
        "schema_version": "ao2.cp-operator-packet-signed-upload.v1",
        "operator_packet": operator_packet,
        "operator_packet_b64": base64_standard(operator_packet_raw.as_bytes()),
        "signature": signature
    }))?;
    let receipt = post_json_http(&endpoint, &api_token, &post_body)?;
    let receipt_sha = json_string(&receipt, "sha256");
    let detail_url = if receipt_sha.is_empty() {
        String::new()
    } else {
        control_plane_endpoint(
            control_plane_url,
            &format!("/api/v1/operator-packet/{receipt_sha}/detail"),
        )?
    };
    let dashboard_url =
        control_plane_endpoint(control_plane_url, "/api/v1/operator-packet/dashboard")?;
    Ok(serde_json::json!({
        "schema_version": "ao2.operator-packet-control-plane-publish.v1",
        "operator_packet_path": operator_packet_path,
        "endpoint": endpoint,
        "detail_url": detail_url,
        "dashboard_url": dashboard_url,
        "signature": signature,
        "signed": true,
        "receipt": receipt
    }))
}

fn enrich_evidence_pack_with_obligation_gates(
    evidence_pack_path: &Path,
    mut evidence_pack: serde_json::Value,
) -> Result<serde_json::Value> {
    let parent = evidence_pack_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let gates = obligation_gate_history_json(parent);
    if json_array(&gates, "gates").is_empty() {
        return Ok(evidence_pack);
    }
    if let Some(object) = evidence_pack.as_object_mut() {
        object.insert("obligation_gates".to_string(), gates);
    }
    Ok(evidence_pack)
}

pub(crate) fn obligation_gate_history_json(evidence_dir: &Path) -> serde_json::Value {
    let mut gates = Vec::new();
    if let Ok(entries) = fs::read_dir(evidence_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with("obligation-gate-") || !name.ends_with(".json") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(gate) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if json_string(&gate, "schema_version") != "ao2.obligation-gate.v1" {
                continue;
            }
            gates.push(serde_json::json!({
                "schema_version": "ao2.workbench-obligation-gate-summary.v1",
                "stage": json_string(&gate, "stage"),
                "status": json_string(&gate, "status"),
                "verdict": json_string(&gate, "verdict"),
                "summary": gate.get("summary").cloned().unwrap_or(serde_json::Value::Null),
                "path": path,
                "sha256": sha256_file(&path).unwrap_or_default(),
                "details": gate
            }));
        }
    }
    gates.sort_by(|left, right| {
        json_string(left, "stage")
            .cmp(&json_string(right, "stage"))
            .then_with(|| json_string(left, "path").cmp(&json_string(right, "path")))
    });
    serde_json::json!({
        "schema_version": "ao2.workbench-obligation-gates.v1",
        "present": !gates.is_empty(),
        "count": gates.len(),
        "gates": gates
    })
}
