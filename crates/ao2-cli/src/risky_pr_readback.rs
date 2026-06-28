use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ao2_runtime::{replay_run, ReplayOptions};
use serde::de::DeserializeOwned;

const RISKY_PR_REQUIRED_REPORT_SECTIONS: &[&str] = &[
    "Objective",
    "Run Health",
    "Policy Decisions",
    "Approvals",
    "Artifacts",
    "Evaluator Closure Evidence",
    "Replay Evidence",
    "Static Export Evidence",
    "Local Run Record",
];

pub(crate) fn report_index_path(report_path: &Path) -> PathBuf {
    let stem = report_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("index");
    report_path.with_file_name(format!("{stem}.report.json"))
}

fn risky_pr_report_contract(report_html: &str) -> serde_json::Value {
    report_contract_json(report_html, RISKY_PR_REQUIRED_REPORT_SECTIONS)
}

fn report_contract_json(report_html: &str, required_sections: &[&str]) -> serde_json::Value {
    let present_sections = required_sections
        .iter()
        .copied()
        .filter(|section| report_html.contains(section))
        .collect::<Vec<_>>();
    let missing_sections = required_sections
        .iter()
        .copied()
        .filter(|section| !present_sections.contains(section))
        .collect::<Vec<_>>();
    let complete = missing_sections.is_empty();
    serde_json::json!({
        "schema_version": "ao2.report-contract.v1",
        "required_sections": required_sections,
        "present_sections": present_sections,
        "missing_sections": missing_sections,
        "complete": complete,
    })
}

pub(crate) fn report_contract_verification_json(
    target: &Path,
    run_id: &str,
    report: Option<PathBuf>,
    index: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let run_dir = run_dir(target, run_id);
    let report_path = report.unwrap_or_else(|| default_report_verify_path(&run_dir));
    let index_path = index.unwrap_or_else(|| report_index_path(&report_path));
    let report_html = fs::read_to_string(&report_path)
        .with_context(|| format!("read report {}", report_path.display()))?;
    let index_json = read_json_file::<serde_json::Value>(&index_path)
        .with_context(|| format!("read report index {}", index_path.display()))?;
    let contract = report_contract_json(&report_html, RISKY_PR_REQUIRED_REPORT_SECTIONS);
    let missing_sections = json_array(&contract, "missing_sections")
        .iter()
        .map(json_value_text)
        .collect::<Vec<_>>();
    let mut failures = Vec::new();
    if json_string(&index_json, "schema_version") != "ao2.risky-pr-static-report-index.v1" {
        failures.push(
            "report index schema_version is not ao2.risky-pr-static-report-index.v1".to_string(),
        );
    }
    if json_string(&index_json, "run_id") != run_id {
        failures.push("report index run_id does not match requested run_id".to_string());
    }
    for section in &missing_sections {
        failures.push(format!("missing required report section: {section}"));
    }
    let complete = failures.is_empty();

    Ok(serde_json::json!({
        "schema_version": "ao2.report-contract-verification.v1",
        "contract_schema_version": "ao2.report-contract.v1",
        "status": if complete { "passed" } else { "failed" },
        "run_id": run_id,
        "target": target,
        "report": report_path,
        "index": index_path,
        "complete": complete,
        "required_sections": RISKY_PR_REQUIRED_REPORT_SECTIONS,
        "present_sections": contract["present_sections"],
        "missing_sections": missing_sections,
        "report_contract": contract,
        "failures": failures,
    }))
}

fn default_report_verify_path(run_dir: &Path) -> PathBuf {
    let report = run_dir.join("report").join("index.html");
    if report.is_file() {
        return report;
    }
    run_dir.join("cockpit").join("index.html")
}

pub(crate) fn render_report_index_for_run(
    target: &Path,
    run_id: &str,
    report_path: &Path,
) -> Result<serde_json::Value> {
    let run_dir = run_dir(target, run_id);
    let run_record_path = run_dir.join("run-record.json");
    let evidence_pack_path = run_dir.join("evidence-pack").join("evidence-pack.json");
    let report_index_path = report_index_path(report_path);
    let evidence_pack: serde_json::Value = read_json_file(&evidence_pack_path)?;
    let report_html = fs::read_to_string(report_path)
        .with_context(|| format!("read rendered report {}", report_path.display()))?;
    let report_contract = risky_pr_report_contract(&report_html);
    let report_contract_complete = json_bool(&report_contract, "complete");
    let replay = replay_run(ReplayOptions {
        target_repo: target.to_path_buf(),
        run_id: run_id.to_string(),
    })?;

    let policy_decisions = json_array(&evidence_pack, "policy_decisions");
    let policy_non_allow = policy_decisions
        .iter()
        .filter(|decision| json_string(decision, "decision") != "allow")
        .count();
    let denied_request_digests = policy_decisions
        .iter()
        .filter(|decision| json_string(decision, "decision") != "allow")
        .filter_map(|decision| {
            let digest = json_string(decision, "request_digest");
            (!digest.is_empty()).then_some(digest)
        })
        .collect::<Vec<_>>();
    let denied_actions = policy_decisions
        .iter()
        .filter(|decision| json_string(decision, "decision") != "allow")
        .map(|decision| {
            serde_json::json!({
                "action": json_string(decision, "action"),
                "resource": json_string(decision, "resource"),
                "request_digest": json_string(decision, "request_digest"),
            })
        })
        .collect::<Vec<_>>();
    let approvals = json_array(&evidence_pack, "approvals");
    let approved = approvals
        .iter()
        .filter(|approval| json_string(approval, "status") == "approved")
        .count();
    let approved_action_digests = approvals
        .iter()
        .filter(|approval| json_string(approval, "status") == "approved")
        .filter_map(|approval| {
            let digest = json_string(approval, "action_digest");
            (!digest.is_empty()).then_some(digest)
        })
        .collect::<Vec<_>>();
    let approved_actions = approvals
        .iter()
        .filter(|approval| json_string(approval, "status") == "approved")
        .map(|approval| {
            serde_json::json!({
                "ticket_id": json_string(approval, "ticket_id"),
                "requested_action": json_string(approval, "requested_action"),
                "scope": json_string(approval, "scope"),
                "action_digest": json_string(approval, "action_digest"),
            })
        })
        .collect::<Vec<_>>();
    let artifacts = json_array(&evidence_pack, "artifacts");
    let closures = json_array(&evidence_pack, "closures");
    let closure_verdict = closures
        .last()
        .map(|closure| json_string(closure, "verdict"))
        .filter(|verdict| !verdict.is_empty())
        .unwrap_or_else(|| json_string(&evidence_pack, "verdict"));
    let artifact_types = artifacts
        .iter()
        .map(|artifact| json_string(artifact, "artifact_type"))
        .collect::<Vec<_>>();
    let test_evidence = artifact_types
        .iter()
        .any(|artifact_type| artifact_type.to_ascii_lowercase().contains("test"));

    Ok(serde_json::json!({
        "schema_version": "ao2.risky-pr-static-report-index.v1",
        "run_id": run_id,
        "status": json_string(&evidence_pack, "verdict"),
        "workflow_id": json_string(&evidence_pack, "workflow_id"),
        "objective": json_string(&evidence_pack, "objective"),
        "closure_verdict": closure_verdict,
        "html_report": report_path,
        "paths": {
            "run_record": run_record_path,
            "evidence_pack": evidence_pack_path,
            "html_report": report_path,
            "report_index": report_index_path,
        },
        "operator_readback": {
            "schema_version": "ao2.risky-pr-operator-readback.v1",
            "run_id": run_id,
            "manual_filesystem_archaeology_required": false,
            "local_run_record": {
                "status": if run_record_path.is_file() { "present" } else { "missing" },
                "path": run_record_path,
            },
            "static_report_export": {
                "status": if report_path.is_file() { "present" } else { "missing" },
                "html_report": report_path,
                "report_index": report_index_path,
                "evidence_pack": evidence_pack_path,
            },
            "evaluator_closure_evidence": {
                "status": if !closure_verdict.is_empty() && !closures.is_empty() { "present" } else { "missing" },
                "verdict": closure_verdict,
                "closure_count": closures.len(),
            },
            "replay_evidence": {
                "status": replay.status,
                "digest_failure_count": replay.digest_failures.len(),
                "event_count": replay.event_count,
                "artifact_count": replay.artifact_count,
            },
            "trust_boundary": {
                "local_only": true,
                "stores_credentials": false,
                "provider_api_key_required": false,
            },
        },
        "policy_decisions": {
            "count": policy_decisions.len(),
            "denied": policy_non_allow,
        },
        "approvals": {
            "count": approvals.len(),
            "approved": approved,
        },
        "approval_boundary": {
            "denied_request_digests": denied_request_digests,
            "approved_action_digests": approved_action_digests,
            "denied_actions": denied_actions,
            "approved_actions": approved_actions,
        },
        "artifacts": {
            "count": artifacts.len(),
            "types": artifact_types,
        },
        "report_contract": report_contract,
        "closures": {
            "count": closures.len(),
        },
        "replay": {
            "status": replay.status,
            "event_count": replay.event_count,
            "artifact_count": replay.artifact_count,
            "digest_failure_count": replay.digest_failures.len(),
            "event_types": replay.event_types,
        },
        "operator_answers": {
            "objective": !json_string(&evidence_pack, "objective").is_empty(),
            "denied_actions": policy_non_allow > 0,
            "approved_actions": approved > 0,
            "changed_files": artifacts.iter().any(|artifact| json_string(artifact, "artifact_type").contains("patch")),
            "test_evidence": test_evidence,
            "closure_verdict": !json_string(&evidence_pack, "verdict").is_empty(),
            "export_path": evidence_pack_path.is_file(),
            "replay_status": replay.digest_failures.is_empty(),
            "report_contract": report_contract_complete,
        },
    }))
}

fn run_dir(target: &Path, run_id: &str) -> PathBuf {
    target.join(".ao2").join("runs").join(run_id)
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

fn json_array<'a>(value: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    value
        .get(key)
        .and_then(|value| value.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn json_bool(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string()
}

fn json_value_text(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string())
}
