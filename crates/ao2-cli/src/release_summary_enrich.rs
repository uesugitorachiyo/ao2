use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};

use crate::atomic_write_text;
use crate::cli_util::{json_array, json_string, json_u64, sha256_file};
use crate::run_reporting::runs_list_json;
use crate::workbench_run_evidence::workbench_run_evidence_summary_json;

pub(crate) fn release_summary_enrich(
    summary: PathBuf,
    target: PathBuf,
    run_id: Option<String>,
    obligation_gate_paths: Vec<PathBuf>,
    out: PathBuf,
    json: bool,
) -> Result<()> {
    let report =
        release_summary_enrich_report_json(summary, target, run_id, obligation_gate_paths, out)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("release_summary_enrich=written");
        println!("out={}", json_string(&report, "out"));
        println!("run_id={}", json_string(&report, "run_id"));
        println!(
            "obligation_gate_count={}",
            report["obligation_gates"]["count"]
                .as_u64()
                .unwrap_or_default()
        );
    }
    Ok(())
}

pub(crate) fn release_summary_enrich_report_json(
    summary: PathBuf,
    target: PathBuf,
    run_id: Option<String>,
    obligation_gate_paths: Vec<PathBuf>,
    out: PathBuf,
) -> Result<serde_json::Value> {
    let body =
        fs::read_to_string(&summary).with_context(|| format!("read {}", summary.display()))?;
    let mut summary_json: serde_json::Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", summary.display()))?;
    if json_string(&summary_json, "schema") != "ao2.three-os-smoke-summary.v1" {
        anyhow::bail!("summary schema must be ao2.three-os-smoke-summary.v1");
    }
    let (source_run_id, evidence_pack, obligation_gates, source) =
        if obligation_gate_paths.is_empty() {
            let source_run_id = match run_id {
                Some(value) => value,
                None => latest_run_id_with_obligation_gates(&target).with_context(|| {
                    format!(
                        "find latest obligation-gated run under {}",
                        target.display()
                    )
                })?,
            };
            let evidence_summary =
                workbench_run_evidence_summary_json(&target, &format!("run_id={source_run_id}"))?;
            let obligation_gates = evidence_summary
                .get("obligation_gates")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"present": false, "count": 0, "gates": []}));
            (
                Some(source_run_id),
                json_string(&evidence_summary, "evidence_pack"),
                obligation_gates,
                "run-history",
            )
        } else {
            (
                run_id,
                String::new(),
                obligation_gate_summary_from_paths(&obligation_gate_paths)?,
                "explicit-artifacts",
            )
        };
    if json_array(&obligation_gates, "gates").is_empty() {
        let source_label = source_run_id.as_deref().unwrap_or(source);
        anyhow::bail!("{source_label} has no obligation gate metadata");
    }
    if let Some(object) = summary_json.as_object_mut() {
        object.insert("obligation_gates".to_string(), obligation_gates.clone());
        object.insert(
            "obligation_gate_source".to_string(),
            serde_json::json!({
                "schema": "ao2.release-obligation-gate-source.v1",
                "source": source,
                "run_id": source_run_id,
                "evidence_pack": evidence_pack,
                "gate_count": json_u64(&obligation_gates, "count"),
                "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
            }),
        );
    } else {
        anyhow::bail!("summary must be a JSON object");
    }
    atomic_write_text(&out, &serde_json::to_string_pretty(&summary_json)?)?;
    Ok(serde_json::json!({
        "schema": "ao2.release-summary-enrich.v1",
        "status": "written",
        "summary": summary,
        "out": out,
        "target": target,
        "run_id": source_run_id,
        "source": source,
        "obligation_gates": obligation_gates
    }))
}

fn obligation_gate_summary_from_paths(paths: &[PathBuf]) -> Result<serde_json::Value> {
    let mut gates = Vec::new();
    for path in paths {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read obligation gate {}", path.display()))?;
        let gate: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("parse obligation gate {}", path.display()))?;
        if json_string(&gate, "schema_version") != "ao2.obligation-gate.v1" {
            anyhow::bail!(
                "obligation gate {} schema_version must be ao2.obligation-gate.v1",
                path.display()
            );
        }
        gates.push(serde_json::json!({
            "schema_version": "ao2.workbench-obligation-gate-summary.v1",
            "stage": json_string(&gate, "stage"),
            "status": json_string(&gate, "status"),
            "verdict": json_string(&gate, "verdict"),
            "summary": gate.get("summary").cloned().unwrap_or(serde_json::Value::Null),
            "path": path,
            "sha256": sha256_file(path).unwrap_or_default(),
            "details": gate
        }));
    }
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-obligation-gates.v1",
        "present": !gates.is_empty(),
        "count": gates.len(),
        "gates": gates
    }))
}

fn latest_run_id_with_obligation_gates(target: &Path) -> Result<String> {
    let runs = runs_list_json(target)?;
    for run in json_array(&runs, "runs") {
        let run_id = json_string(run, "run_id");
        if run_id.is_empty() {
            continue;
        }
        let summary = workbench_run_evidence_summary_json(target, &format!("run_id={run_id}"))?;
        if !json_array(&summary["obligation_gates"], "gates").is_empty() {
            return Ok(run_id);
        }
    }
    anyhow::bail!("no AO2 run with obligation gate metadata found")
}
