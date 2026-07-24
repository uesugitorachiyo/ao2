use std::fs;

use anyhow::{anyhow, Context, Result};

use crate::cli_util::{canonical_json_string, json_string, sha256_bytes_hex};
use crate::control_plane_http::{get_json_http, get_text_http};
use crate::{resolve_api_token, CpCommand};

const CP_PROBE_EXTENDED_SCHEMA: &str = "ao2.cp-healthz-extended-probe.v1";
const CP_HEALTHZ_EXTENDED_SCHEMA: &str = "ao2.cp-healthz-extended.v1";
const CP_STATUS_SCHEMA: &str = "ao2.cp-status.v1";
const CP_RELEASE_SNAPSHOT_SCHEMA: &str = "ao2.cp-release-snapshot.v1";

pub(crate) fn cp(command: CpCommand) -> Result<()> {
    match command {
        CpCommand::ProbeExtended {
            cp_url,
            api_token,
            api_token_env,
            write_json,
            json,
        } => {
            let token = resolve_api_token(api_token.as_deref(), api_token_env.as_deref())?;
            let payload = cp_probe_extended(&cp_url, &token)?;
            if let Some(path) = write_json.as_ref() {
                fs::write(path, format!("{}\n", canonical_json_string(&payload)?))
                    .with_context(|| format!("write {}", path.display()))?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                let source = json_string(&payload, "source");
                let schema = json_string(&payload, "observed_schema_version");
                let uptime = payload
                    .get("healthz_extended")
                    .and_then(|h| h.get("uptime_seconds"))
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                let last_error = payload
                    .get("healthz_extended")
                    .and_then(|h| h.get("last_error_utc"))
                    .and_then(serde_json::Value::as_str)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "null".to_string());
                println!("probe_source={source}");
                println!("observed_schema={schema}");
                println!("uptime_seconds={uptime:.1}");
                println!("last_error_utc={last_error}");
            }
            Ok(())
        }
        CpCommand::ReleaseSnapshot {
            cp_url,
            api_token,
            api_token_env,
            write_json,
            json,
        } => {
            let token = resolve_api_token(api_token.as_deref(), api_token_env.as_deref())?;
            let payload = cp_release_snapshot(&cp_url, &token)?;
            if let Some(path) = write_json.as_ref() {
                fs::write(path, format!("{}\n", canonical_json_string(&payload)?))
                    .with_context(|| format!("write {}", path.display()))?;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                let cp = json_string(&payload, "cp_url");
                let captured = json_string(&payload, "captured_at_utc");
                let ok = payload
                    .get("summary")
                    .and_then(|s| s.get("ok_count"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let errors = payload
                    .get("summary")
                    .and_then(|s| s.get("error_count"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let total = payload
                    .get("summary")
                    .and_then(|s| s.get("endpoint_count"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                println!("cp_url={cp}");
                println!("captured_at_utc={captured}");
                println!("endpoints_ok={ok}/{total}");
                println!("endpoints_error={errors}");
                if let Some(endpoints) = payload.get("endpoints").and_then(|v| v.as_object()) {
                    for (name, value) in endpoints {
                        let ok_flag = value
                            .get("ok")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        let schema = value
                            .get("schema")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("-");
                        let bytes = value
                            .get("body_bytes")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        let sha = value
                            .get("body_sha256")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("-");
                        let short_sha: String = sha.chars().take(12).collect();
                        let status = if ok_flag { "ok" } else { "err" };
                        println!(
                            "  {name}: {status} schema={schema} bytes={bytes} sha256={short_sha}"
                        );
                    }
                }
            }
            Ok(())
        }
    }
}

fn cp_probe_extended(cp_url: &str, api_token: &str) -> Result<serde_json::Value> {
    let base = cp_url.trim_end_matches('/');
    let extended_url = format!("{base}/api/v1/healthz/extended");
    let probed_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let endpoint = base.to_string();
    if let Ok(extended) = get_json_http(&extended_url, api_token) {
        if extended.get("schema_version").and_then(|v| v.as_str())
            == Some(CP_HEALTHZ_EXTENDED_SCHEMA)
        {
            return Ok(serde_json::json!({
                "schema_version": CP_PROBE_EXTENDED_SCHEMA,
                "probed_at_utc": probed_at,
                "cp_url": endpoint,
                "source": "healthz_extended",
                "observed_schema_version": CP_HEALTHZ_EXTENDED_SCHEMA,
                "healthz_extended": extended,
            }));
        }
    }
    let status_url = format!("{base}/api/v1/status");
    let status = get_json_http(&status_url, api_token).with_context(|| {
        format!("probe {base}: neither /api/v1/healthz/extended nor /api/v1/status responded")
    })?;
    if status.get("schema_version").and_then(|v| v.as_str()) != Some(CP_STATUS_SCHEMA) {
        return Err(anyhow!(
            "unexpected /api/v1/status schema_version: {:?}",
            status.get("schema_version")
        ));
    }
    let synth = serde_json::json!({
        "schema_version": CP_HEALTHZ_EXTENDED_SCHEMA,
        "version": status.get("build").and_then(|b| b.get("version")).cloned()
            .unwrap_or_else(|| serde_json::Value::String(String::new())),
        "uptime_seconds": status.get("uptime_seconds").cloned()
            .unwrap_or_else(|| serde_json::Value::from(0.0)),
        "started_at_utc": serde_json::Value::Null,
        "last_error_utc": serde_json::Value::Null,
        "request_count": status
            .get("requests").and_then(|r| r.get("total")).cloned()
            .unwrap_or_else(|| serde_json::Value::from(0)),
        "error_request_count": status
            .get("requests").and_then(|r| r.get("errors_4xx_5xx")).cloned()
            .unwrap_or_else(|| serde_json::Value::from(0)),
    });
    Ok(serde_json::json!({
        "schema_version": CP_PROBE_EXTENDED_SCHEMA,
        "probed_at_utc": probed_at,
        "cp_url": endpoint,
        "source": "synthesized_from_status",
        "observed_schema_version": CP_STATUS_SCHEMA,
        "healthz_extended": synth,
    }))
}

fn cp_release_snapshot(cp_url: &str, api_token: &str) -> Result<serde_json::Value> {
    let base = cp_url.trim_end_matches('/');
    let captured_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let endpoints: [(&str, &str); 4] = [
        ("readiness", "/api/v1/release/readiness.json"),
        ("handoff", "/api/v1/release/handoff.json"),
        (
            "support_bundle_status",
            "/api/v1/release/support-bundle.json",
        ),
        ("publication_latest", "/api/v1/release/publication/latest"),
    ];
    let mut documents = serde_json::Map::new();
    let mut ok_count: u64 = 0;
    let mut error_count: u64 = 0;
    for (key, path) in endpoints.iter() {
        let url = format!("{base}{path}");
        let entry = match get_text_http(&url, api_token) {
            Ok(body) => {
                let parsed_schema = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| {
                        v.get("schema_version")
                            .or_else(|| v.get("schema"))
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string())
                    });
                let body_sha256 = sha256_bytes_hex(body.as_bytes());
                ok_count += 1;
                serde_json::json!({
                    "url": url,
                    "ok": true,
                    "schema": parsed_schema,
                    "body_bytes": body.len() as u64,
                    "body_sha256": body_sha256,
                })
            }
            Err(error) => {
                error_count += 1;
                serde_json::json!({
                    "url": url,
                    "ok": false,
                    "error": error.to_string(),
                })
            }
        };
        documents.insert((*key).to_string(), entry);
    }
    Ok(serde_json::json!({
        "schema_version": CP_RELEASE_SNAPSHOT_SCHEMA,
        "captured_at_utc": captured_at,
        "cp_url": base,
        "endpoints": serde_json::Value::Object(documents),
        "summary": {
            "endpoint_count": endpoints.len() as u64,
            "ok_count": ok_count,
            "error_count": error_count,
        },
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
        },
    }))
}
