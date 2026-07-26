use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli_util::{json_array, json_string, json_u64, now_unix_ms};
use crate::control_plane_ops::workbench_support_bundle_verify_json;
use crate::workbench_support::empty_workbench_redaction_audit;

pub(crate) fn latest_workbench_support_packet_json(target: &Path) -> Result<serde_json::Value> {
    let Some(summary) = latest_workbench_support_bundle_summary(target)? else {
        return Ok(serde_json::json!({
            "schema_version": "ao2.workbench-support-latest.v1",
            "present": false,
            "generated_at_ms": now_unix_ms(),
            "queue_job_count": 0,
            "queue_job_diagnosis_count": 0,
            "queue_job_diagnoses": [],
            "audit_event_count": 0,
            "job_log_count": 0,
            "evidence_export_count": 0,
            "evidence_exports": [],
            "redaction_audit": empty_workbench_redaction_audit(),
            "hermes_project_start_flow_contract": {
                "present": false
            },
            "support_metadata": {
                "present": false,
                "signature_verified": false
            },
            "files": []
        }));
    };

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-support-latest.v1",
        "present": true,
        "generated_at_ms": now_unix_ms(),
        "bundle_dir": json_string(&summary, "bundle_dir"),
        "bundle_path": json_string(&summary, "bundle_path"),
        "bundle_sha256": json_string(&summary, "bundle_sha256"),
        "queue_job_count": json_u64(&summary, "queue_job_count"),
        "queue_job_diagnosis_count": json_u64(&summary, "queue_job_diagnosis_count"),
        "queue_job_diagnoses": json_array(&summary, "queue_job_diagnoses"),
        "audit_event_count": json_u64(&summary, "audit_event_count"),
        "job_log_count": json_u64(&summary, "job_log_count"),
        "evidence_export_count": json_u64(&summary, "evidence_export_count"),
        "evidence_exports": json_array(&summary, "evidence_exports"),
        "redaction_audit": summary["redaction_audit"].clone(),
        "hermes_project_start_flow_contract": summary
            .get("hermes_project_start_flow_contract")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "present": false })),
        "support_metadata": summary.get("support_metadata").cloned().unwrap_or(serde_json::Value::Null),
        "files": json_array(&summary, "files")
    }))
}

fn latest_workbench_support_bundle_summary(target: &Path) -> Result<Option<serde_json::Value>> {
    let support_dir = target
        .join(".ao2")
        .join("workbench")
        .join("support-bundles");
    if !support_dir.is_dir() {
        return Ok(None);
    }
    let mut bundle_dirs = fs::read_dir(&support_dir)
        .with_context(|| format!("read {}", support_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir() && path.join("support-bundle.json").is_file())
        .collect::<Vec<_>>();
    bundle_dirs.sort();
    let Some(bundle_dir) = bundle_dirs.pop() else {
        return Ok(None);
    };
    Ok(Some(workbench_support_bundle_verify_json(&bundle_dir)?))
}
