use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli_util::{json_array, json_string, json_u64};

pub(crate) fn release_smoke_summary(summary: PathBuf, require_native_windows: bool) -> Result<()> {
    let body =
        fs::read_to_string(&summary).with_context(|| format!("read {}", summary.display()))?;
    let summary_json: serde_json::Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", summary.display()))?;
    let report =
        release_smoke_summary_verification_json(&summary, &summary_json, require_native_windows);
    println!("{}", serde_json::to_string_pretty(&report)?);
    if json_string(&report, "status") != "verified" {
        anyhow::bail!("release smoke summary verification failed");
    }
    Ok(())
}

pub(crate) fn release_smoke_summary_verification_json(
    summary_path: &Path,
    summary: &serde_json::Value,
    require_native_windows: bool,
) -> serde_json::Value {
    let mut reasons = Vec::new();
    if json_string(summary, "schema") != "ao2.three-os-smoke-summary.v1" {
        reasons.push(serde_json::json!({
            "code": "invalid_schema",
            "message": "summary schema must be ao2.three-os-smoke-summary.v1"
        }));
    }
    if json_string(summary, "local_smoke") != "passed" {
        reasons.push(serde_json::json!({
            "code": "local_smoke_not_passed",
            "message": "macOS, Ubuntu, provenance, provider contract, and Windows archive smoke must pass"
        }));
    }
    if summary.get("linux_x86_64_remote_smoke").is_some()
        && json_string(summary, "linux_x86_64_remote_smoke") != "passed"
    {
        reasons.push(serde_json::json!({
            "code": "linux_x86_64_remote_smoke_not_passed",
            "message": "native Ubuntu x86_64 release smoke must pass"
        }));
    }
    let summary_requires_windows = summary["native_windows_required"]
        .as_bool()
        .unwrap_or(false);
    if (require_native_windows || summary_requires_windows)
        && json_string(summary, "windows_native_smoke") != "passed"
    {
        reasons.push(serde_json::json!({
            "code": "native_windows_not_passed",
            "message": "native Windows smoke is required but did not pass",
            "windows_status": json_string(summary, "windows_native_smoke"),
            "windows_skip_reason": json_string(summary, "windows_skip_reason")
        }));
    }
    reasons.extend(windows_smoke_log_failure_reasons(summary_path, summary));
    serde_json::json!({
        "schema": "ao2.three-os-smoke-summary-verification.v1",
        "status": if reasons.is_empty() { "verified" } else { "failed" },
        "summary_path": summary_path,
        "require_native_windows": require_native_windows,
        "summary": summary,
        "reasons": reasons
    })
}

fn windows_smoke_log_failure_reasons(
    summary_path: &Path,
    summary: &serde_json::Value,
) -> Vec<serde_json::Value> {
    if json_string(summary, "windows_native_smoke") != "passed" {
        return Vec::new();
    }
    let windows_log = json_string(summary, "windows_log");
    if windows_log.is_empty() {
        return Vec::new();
    }
    let log_path = resolve_summary_sidecar_path(summary_path, &windows_log);
    let log = match fs::read_to_string(&log_path) {
        Ok(value) => value,
        Err(error) => {
            return vec![serde_json::json!({
                "code": "windows_smoke_log_unreadable",
                "message": format!("read Windows smoke log {}: {error}", log_path.display()),
                "windows_log": log_path
            })];
        }
    };
    windows_smoke_log_hard_failure_snippets(&log)
        .into_iter()
        .map(|snippet| {
            serde_json::json!({
                "code": "windows_smoke_log_hard_failure",
                "message": snippet,
                "windows_log": log_path
            })
        })
        .collect()
}

pub(crate) fn resolve_summary_sidecar_path(summary_path: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.exists() {
        return path;
    }
    let summary_dir = summary_path.parent().unwrap_or_else(|| Path::new("."));
    if path.is_absolute() || path.has_root() {
        if let Some(name) = path.file_name() {
            let relocated = summary_dir.join(name);
            if relocated.exists() {
                return relocated;
            }
        }
        if path.is_absolute() {
            return path;
        }
    }
    summary_dir.join(path)
}

pub(crate) fn resolve_cli_artifact_reference(summary_path: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() || path.exists() {
        path
    } else {
        summary_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

fn windows_smoke_log_hard_failure_snippets(log: &str) -> Vec<String> {
    let mut snippets = Vec::new();
    for line in log.lines() {
        let lower = line.to_ascii_lowercase();
        let hard_failure = lower.contains("bash : the term 'bash' is not recognized")
            || lower.contains("missing ao2-control-plane release archive")
            || lower.contains("missing ao2 release archive")
            || lower.contains("cannot bind argument to parameter 'value' because it is null")
            || lower.contains("verify-releasesupportbundle.ps1")
            || lower.contains("fullyqualifiederrorid");
        if hard_failure {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                snippets.push(trimmed.chars().take(240).collect());
            }
        }
    }
    snippets.sort();
    snippets.dedup();
    snippets
}

pub(crate) fn release_obligation_gate_verification_json(
    summary: &serde_json::Value,
) -> serde_json::Value {
    let mut reasons = Vec::new();
    let obligation_gates = summary
        .get("obligation_gates")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if obligation_gates.is_null() {
        reasons.push(serde_json::json!({
            "code": "missing_obligation_gate_metadata",
            "message": "release summary must include uploaded obligation gate metadata"
        }));
    } else {
        if obligation_gates
            .get("present")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            reasons.push(serde_json::json!({
                "code": "obligation_gate_metadata_not_present",
                "message": "release summary obligation_gates.present must be true"
            }));
        }
        if json_u64(&obligation_gates, "count") == 0 {
            reasons.push(serde_json::json!({
                "code": "obligation_gate_metadata_empty",
                "message": "release summary must include at least one obligation gate"
            }));
        }
        let gates = json_array(&obligation_gates, "gates");
        if !gates
            .iter()
            .any(|gate| json_string(gate, "stage") == "closure")
        {
            reasons.push(serde_json::json!({
                "code": "missing_closure_obligation_gate",
                "message": "release summary must include a closure obligation gate"
            }));
        }
        for gate in gates {
            let summary = gate.get("summary").unwrap_or(&serde_json::Value::Null);
            let fail = json_u64(summary, "fail");
            let unverified = json_u64(summary, "unverified");
            if json_string(gate, "status") != "passed"
                || json_string(gate, "verdict") != "accepted"
                || fail > 0
                || unverified > 0
            {
                reasons.push(serde_json::json!({
                    "code": "obligation_gate_not_clean",
                    "stage": json_string(gate, "stage"),
                    "status": json_string(gate, "status"),
                    "verdict": json_string(gate, "verdict"),
                    "fail": fail,
                    "unverified": unverified,
                    "message": "all release obligation gates must be passed, accepted, and free of failed or unverified items"
                }));
            }
        }
    }
    serde_json::json!({
        "schema": "ao2.release-obligation-gate-verification.v1",
        "status": if reasons.is_empty() { "verified" } else { "failed" },
        "obligation_gates": obligation_gates,
        "reasons": reasons
    })
}
