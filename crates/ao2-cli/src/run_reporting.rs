use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ao2_runtime::{replay_run, ReplayOptions};

use super::cli_util::{
    concerns_text, escape_html, json_array, json_string, json_u64, open_report_target, pills,
    pills_from_strings, run_dir, string_array_text, usage_text,
};
use super::provider_ops::provider_score_json;
use super::risky_pr_readback::{
    render_report_index_for_run, report_contract_verification_json, report_index_path,
};

pub(super) fn runs_list(target: PathBuf, json: bool) -> Result<()> {
    let result = runs_list_json(&target)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        for run in json_array(&result, "runs") {
            println!(
                "{}\t{}\t{} digest failures",
                json_string(run, "run_id"),
                json_string(run, "status"),
                run.get("digest_failures")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0)
            );
        }
    }
    Ok(())
}

pub(super) fn runs_list_json(target: &Path) -> Result<serde_json::Value> {
    let runs_dir = target.join(".ao2").join("runs");
    let mut runs = Vec::new();
    if runs_dir.is_dir() {
        for entry in
            fs::read_dir(&runs_dir).with_context(|| format!("read {}", runs_dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let run_id = entry.file_name().to_string_lossy().to_string();
            if let Ok(summary) = run_summary_json(target, &run_id) {
                runs.push(summary);
            }
        }
    }
    runs.sort_by(|left, right| {
        json_string(right, "updated_at")
            .cmp(&json_string(left, "updated_at"))
            .then_with(|| json_string(left, "run_id").cmp(&json_string(right, "run_id")))
    });
    let result = serde_json::json!({
        "schema_version": "ao2.runs-list.v1",
        "target": target,
        "runs": runs,
    });
    Ok(result)
}

pub(super) fn runs_show(target: PathBuf, run_id: String, json: bool) -> Result<()> {
    let run = run_summary_json(&target, &run_id)?;
    let result = serde_json::json!({
        "schema_version": "ao2.runs-show.v1",
        "target": target,
        "run": run,
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("run_id={}", result["run"]["run_id"].as_str().unwrap_or(""));
        println!("status={}", result["run"]["status"].as_str().unwrap_or(""));
        println!(
            "digest_failures={}",
            result["run"]["digest_failures"].as_u64().unwrap_or(0)
        );
        println!(
            "evidence_pack={}",
            result["run"]["evidence_pack"].as_str().unwrap_or("")
        );
        println!(
            "cockpit={}",
            result["run"]["cockpit"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

pub(super) fn run_summary_json(target: &Path, run_id: &str) -> Result<serde_json::Value> {
    let run_dir = run_dir(target, run_id);
    let evidence_pack_path = run_dir.join("evidence-pack").join("evidence-pack.json");
    let evidence_pack = fs::read_to_string(&evidence_pack_path)
        .with_context(|| format!("read {}", evidence_pack_path.display()))?;
    let evidence_pack: serde_json::Value = serde_json::from_str(&evidence_pack)
        .with_context(|| format!("parse {}", evidence_pack_path.display()))?;
    let replay = replay_run(ReplayOptions {
        target_repo: target.to_path_buf(),
        run_id: run_id.to_string(),
    })?;
    let replay_status_value = serde_json::to_value(replay.status)?;
    let replay_status = replay_status_value.as_str().unwrap_or("unknown");
    let run_record = run_dir.join("run-record.json");
    let report = run_dir.join("report").join("index.html");
    let report_index = report_index_path(&report);
    let cockpit = run_dir.join("cockpit").join("index.html");
    let updated_at = fs::metadata(&evidence_pack_path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_default();
    let provider_score = provider_score_json(target, run_id).unwrap_or(serde_json::Value::Null);
    let obligation_ledger = obligation_ledger_summary_json(&run_dir, &evidence_pack);
    Ok(serde_json::json!({
        "run_id": run_id,
        "workflow_id": json_string(&evidence_pack, "workflow_id"),
        "objective": json_string(&evidence_pack, "objective"),
        "status": replay_status,
        "verdict": json_string(&evidence_pack, "verdict"),
        "event_count": replay.event_count,
        "artifact_count": replay.artifact_count,
        "digest_failures": replay.digest_failures.len(),
        "updated_at": updated_at,
        "run_record": run_record,
        "evidence_pack": evidence_pack_path,
        "report": report,
        "report_index": report_index,
        "cockpit": cockpit,
        "provider_score": provider_score,
        "obligation_ledger": obligation_ledger,
    }))
}

fn obligation_ledger_summary_json(
    run_dir: &Path,
    evidence_pack: &serde_json::Value,
) -> serde_json::Value {
    let embedded = evidence_pack
        .get("obligation_ledger")
        .filter(|value| !value.is_null());
    let sidecar_paths = [
        run_dir.join("evidence-pack").join("obligation-ledger.json"),
        run_dir.join("obligation-ledger.json"),
    ];
    let sidecar = sidecar_paths.iter().find_map(|path| {
        let text = fs::read_to_string(path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        Some((path, value))
    });

    let (path, ledger) = match (embedded, sidecar) {
        (Some(value), _) => (String::new(), value.clone()),
        (None, Some((path, value))) => (path.display().to_string(), value),
        (None, None) => {
            return serde_json::json!({
                "present": false
            });
        }
    };
    serde_json::json!({
        "present": true,
        "schema_version": json_string(&ledger, "schema_version"),
        "verdict": json_string(&ledger, "verdict"),
        "summary": ledger.get("summary").cloned().unwrap_or(serde_json::Value::Null),
        "path": path,
        "details": ledger
    })
}

pub(super) fn report(
    target: PathBuf,
    run_id: String,
    out: Option<PathBuf>,
    open: bool,
) -> Result<()> {
    let run_dir = run_dir(&target, &run_id);
    let (html, _) = render_report_for_run(&target, &run_id)?;
    let report_path = out.unwrap_or_else(|| run_dir.join("cockpit").join("index.html"));
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&report_path, html).with_context(|| format!("write {}", report_path.display()))?;
    let report_index = render_report_index_for_run(&target, &run_id, &report_path)?;
    let report_index_path = report_index_path(&report_path);
    fs::write(
        &report_index_path,
        serde_json::to_string_pretty(&report_index)? + "\n",
    )
    .with_context(|| format!("write {}", report_index_path.display()))?;
    println!("report={}", report_path.display());
    if open {
        open_report_target(&report_path)?;
        println!("open_target={}", report_path.display());
    }
    Ok(())
}

pub(super) fn report_verify(
    target: PathBuf,
    run_id: String,
    report: Option<PathBuf>,
    index: Option<PathBuf>,
) -> Result<()> {
    let verification = report_contract_verification_json(&target, &run_id, report, index)?;
    println!("{}", serde_json::to_string_pretty(&verification)?);
    if json_string(&verification, "status") != "passed" {
        anyhow::bail!("report contract verification failed");
    }
    Ok(())
}

pub(super) fn render_report_for_run(target: &Path, run_id: &str) -> Result<(String, PathBuf)> {
    let run_dir = run_dir(target, run_id);
    let evidence_pack_path = run_dir.join("evidence-pack").join("evidence-pack.json");
    let evidence_pack = fs::read_to_string(&evidence_pack_path)
        .with_context(|| format!("read {}", evidence_pack_path.display()))?;
    let evidence_pack: serde_json::Value = serde_json::from_str(&evidence_pack)
        .with_context(|| format!("parse {}", evidence_pack_path.display()))?;
    let replay = replay_run(ReplayOptions {
        target_repo: target.to_path_buf(),
        run_id: run_id.to_string(),
    })?;
    let html = render_evidence_cockpit(&evidence_pack, &evidence_pack_path, &replay)?;
    Ok((html, evidence_pack_path))
}

pub(super) fn cockpit_index(target: PathBuf, out: Option<PathBuf>, open: bool) -> Result<()> {
    let html = render_cockpit_index(&target)?;
    let path = out.unwrap_or_else(|| target.join(".ao2").join("cockpit").join("index.html"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, html).with_context(|| format!("write {}", path.display()))?;
    println!("cockpit_index={}", path.display());
    if open {
        open_report_target(&path)?;
        println!("open_target={}", path.display());
    }
    Ok(())
}

pub(super) fn render_cockpit_index(target: &Path) -> Result<String> {
    let list = runs_list_json(target)?;
    let runs = json_array(&list, "runs");
    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AO2 Cockpit</title>
<style>
body {{ margin: 0; background: #f7f8fa; color: #17202a; font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
main {{ max-width: 1180px; margin: 0 auto; padding: 28px; }}
h1 {{ margin: 0 0 6px; font-size: 28px; letter-spacing: 0; }}
p {{ margin: 0 0 18px; color: #5f6b7a; }}
table {{ width: 100%; border-collapse: collapse; background: #fff; border: 1px solid #d9dee7; border-radius: 8px; overflow: hidden; }}
th, td {{ padding: 10px 12px; border-top: 1px solid #d9dee7; text-align: left; vertical-align: top; }}
th {{ color: #5f6b7a; font-size: 12px; text-transform: uppercase; }}
a {{ color: #176b87; text-decoration: none; }}
code {{ background: #eef2f6; border-radius: 4px; padding: 2px 4px; overflow-wrap: anywhere; }}
</style>
</head>
<body>
<main>
<h1>AO2 Cockpit</h1>
<p>{count} governed run artifacts in <code>{target}</code></p>
<table>
<thead><tr><th>Run</th><th>Status</th><th>Workflow</th><th>Digest Failures</th><th>Artifacts</th></tr></thead>
<tbody>
"#,
        count = runs.len(),
        target = escape_html(&target.display().to_string())
    )?;
    for run in runs {
        let run_id = json_string(run, "run_id");
        let status = json_string(run, "status");
        let workflow = json_string(run, "workflow_id");
        let failures = run
            .get("digest_failures")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let cockpit = json_string(run, "cockpit");
        let evidence = json_string(run, "evidence_pack");
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td><a href=\"file://{}\">cockpit</a> <a href=\"file://{}\">evidence</a></td></tr>",
            escape_html(&run_id),
            escape_html(&status),
            escape_html(&workflow),
            failures,
            escape_html(&cockpit),
            escape_html(&evidence)
        )?;
    }
    html.push_str("</tbody>\n</table>\n</main>\n</body>\n</html>\n");
    Ok(html)
}

fn render_evidence_cockpit(
    pack: &serde_json::Value,
    evidence_pack_path: &Path,
    replay: &ao2_runtime::ReplaySummary,
) -> Result<String> {
    let replay_status_value = serde_json::to_value(replay.status)?;
    let replay_status = replay_status_value.as_str().unwrap_or("unknown");
    let run_id = json_string(pack, "run_id");
    let workflow_id = json_string(pack, "workflow_id");
    let objective = json_string(pack, "objective");
    let verdict = json_string(pack, "verdict");

    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AO2 Evidence Cockpit {run_id}</title>
<style>
:root {{
  color-scheme: light;
  --bg: #f7f8fa;
  --panel: #ffffff;
  --ink: #17202a;
  --muted: #5f6b7a;
  --line: #d9dee7;
  --accent: #176b87;
  --ok: #16794c;
  --warn: #9b5a00;
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0;
  background: var(--bg);
  color: var(--ink);
  font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}}
main {{
  max-width: 1180px;
  margin: 0 auto;
  padding: 28px;
}}
h1 {{ margin: 0 0 6px; font-size: 28px; letter-spacing: 0; }}
h2 {{ margin: 0 0 14px; font-size: 18px; letter-spacing: 0; }}
p {{ margin: 0; }}
.muted {{ color: var(--muted); }}
.summary-grid {{
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
  gap: 12px;
  margin: 22px 0;
}}
.metric, section {{
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 8px;
}}
.metric {{ padding: 14px; min-height: 82px; }}
.metric .label {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
.metric .value {{ margin-top: 6px; font-size: 18px; font-weight: 700; overflow-wrap: anywhere; }}
section {{ margin: 16px 0; padding: 18px; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 9px 8px; border-top: 1px solid var(--line); text-align: left; vertical-align: top; }}
th {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
code {{ background: #eef2f6; border-radius: 4px; padding: 2px 4px; overflow-wrap: anywhere; }}
.pill {{ display: inline-block; border: 1px solid var(--line); border-radius: 999px; padding: 2px 8px; margin: 2px 4px 2px 0; }}
.ok {{ color: var(--ok); }}
.warn {{ color: var(--warn); }}
ul {{ margin: 0; padding-left: 18px; }}
li + li {{ margin-top: 6px; }}
</style>
</head>
<body>
<main>
<h1>AO2 Evidence Cockpit</h1>
<p class="muted">Local governance report generated from <code>{evidence}</code>.</p>
"#,
        run_id = escape_html(&run_id),
        evidence = escape_html(&evidence_pack_path.display().to_string())
    )?;

    write!(
        html,
        r#"<div class="summary-grid">
<div class="metric"><div class="label">Run</div><div class="value">{run_id}</div></div>
<div class="metric"><div class="label">Workflow</div><div class="value">{workflow_id}</div></div>
<div class="metric"><div class="label">Closure Verdict</div><div class="value ok">{verdict}</div></div>
<div class="metric"><div class="label">Replay</div><div class="value ok">Replay {replay_status}</div></div>
</div>
<section>
<h2>Objective</h2>
<p>{objective}</p>
</section>
"#,
        run_id = escape_html(&run_id),
        workflow_id = escape_html(&workflow_id),
        verdict = escape_html(&verdict),
        replay_status = escape_html(replay_status),
        objective = escape_html(&objective)
    )?;

    render_static_evidence_links(&mut html, evidence_pack_path)?;
    render_provider_summaries(&mut html, pack)?;
    render_run_health(&mut html, pack)?;
    render_policy_decisions(&mut html, pack)?;
    render_approvals(&mut html, pack)?;
    render_artifacts(&mut html, pack)?;
    render_closures(&mut html, pack)?;
    render_repair_attempts(&mut html, pack)?;
    render_replay(&mut html, replay, replay_status)?;
    render_markers(&mut html, pack)?;

    html.push_str("</main>\n</body>\n</html>\n");
    Ok(html)
}

fn render_static_evidence_links(html: &mut String, evidence_pack_path: &Path) -> Result<()> {
    let run_record = run_artifact_path(evidence_pack_path, &["run-record.json"]);
    let static_report = run_artifact_path(evidence_pack_path, &["report", "index.html"]);
    write!(
        html,
        r#"<section>
<h2>Local Run Record</h2>
<p class="muted">Primary local run evidence: <code>{run_record}</code>.</p>
</section>
<section>
<h2>Static Export Evidence</h2>
<table><thead><tr><th>Artifact</th><th>Local Path</th></tr></thead><tbody>
<tr><td>Evidence Pack</td><td><code>{evidence_pack}</code></td></tr>
<tr><td>Static Report</td><td><code>{static_report}</code></td></tr>
</tbody></table>
</section>
"#,
        run_record = escape_html(&run_record),
        evidence_pack = escape_html(&evidence_pack_path.display().to_string()),
        static_report = escape_html(&static_report)
    )?;
    Ok(())
}

fn run_artifact_path(evidence_pack_path: &Path, relative_path: &[&str]) -> String {
    let Some(run_dir) = evidence_pack_path
        .parent()
        .and_then(|parent| parent.parent())
    else {
        return String::new();
    };
    let mut path = run_dir.to_path_buf();
    for segment in relative_path {
        path.push(segment);
    }
    path.display().to_string()
}

fn render_provider_summaries(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Provider Summaries</h2>\n");
    let summaries = json_array(pack, "provider_summaries");
    if summaries.is_empty() {
        html.push_str("<p class=\"muted\">No provider transcript summaries were embedded.</p>\n");
    } else {
        html.push_str("<table><thead><tr><th>Provider</th><th>Summary</th><th>Changed Files</th><th>Concerns</th><th>Usage</th></tr></thead><tbody>\n");
        for summary in summaries {
            let provider = json_string(summary, "provider");
            let raw_summary = json_string(summary, "raw_summary");
            let changed_files = pills(json_array(summary, "changed_files"));
            let concerns = concerns_text(json_array(summary, "concerns"));
            let usage = usage_text(summary.get("usage"));
            writeln!(
                html,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&provider),
                escape_html(&raw_summary),
                changed_files,
                escape_html(&concerns),
                escape_html(&usage)
            )?;
        }
        html.push_str("</tbody></table>\n");
    }
    html.push_str("</section>\n");
    Ok(())
}

fn render_run_health(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    let Some(health) = pack.get("run_health") else {
        return Ok(());
    };
    let attention = health
        .get("attention_required")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let attention_label = if attention { "yes" } else { "no" };
    let attention_class = if attention { "warn" } else { "ok" };
    write!(
        html,
        r#"<section>
<h2>Run Health</h2>
<div class="summary-grid">
<div class="metric"><div class="label">Repair Status</div><div class="value">{repair_status}</div></div>
<div class="metric"><div class="label">Repair Attempts</div><div class="value">{attempts}</div></div>
<div class="metric"><div class="label">Accepted Repairs</div><div class="value">{accepted}</div></div>
<div class="metric"><div class="label">Attention Required</div><div class="value {attention_class}">{attention_label}</div></div>
</div>
<p>{next_action}</p>
<p>{concerns}</p>
</section>
"#,
        repair_status = escape_html(&json_string(health, "repair_status")),
        attempts = json_u64(health, "repair_attempt_count"),
        accepted = json_u64(health, "accepted_repair_attempts"),
        attention_class = attention_class,
        attention_label = attention_label,
        next_action = escape_html(&json_string(health, "next_action")),
        concerns = pills(json_array(health, "unresolved_concerns"))
    )?;
    Ok(())
}

fn render_policy_decisions(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Policy Decisions</h2>\n<table><thead><tr><th>Action</th><th>Decision</th><th>Resource</th><th>Request Digest</th><th>Reason</th></tr></thead><tbody>\n");
    for decision in json_array(pack, "policy_decisions") {
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            escape_html(&json_string(decision, "action")),
            escape_html(&json_string(decision, "decision")),
            escape_html(&json_string(decision, "resource")),
            escape_html(&json_string(decision, "request_digest")),
            escape_html(&json_string(decision, "reason"))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

fn render_approvals(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Approvals</h2>\n<table><thead><tr><th>Ticket</th><th>Status</th><th>Action</th><th>Scope</th><th>Action Digest</th><th>Approver</th></tr></thead><tbody>\n");
    for approval in json_array(pack, "approvals") {
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            escape_html(&json_string(approval, "ticket_id")),
            escape_html(&json_string(approval, "status")),
            escape_html(&json_string(approval, "requested_action")),
            escape_html(&json_string(approval, "scope")),
            escape_html(&json_string(approval, "action_digest")),
            escape_html(&json_string(approval, "approver"))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

fn render_artifacts(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Artifacts</h2>\n<table><thead><tr><th>Type</th><th>Producer</th><th>Digest</th><th>URI</th></tr></thead><tbody>\n");
    for artifact in json_array(pack, "artifacts") {
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td></tr>",
            escape_html(&json_string(artifact, "artifact_type")),
            escape_html(&json_string(artifact, "producer")),
            escape_html(&json_string(artifact, "digest")),
            escape_html(&json_string(artifact, "uri"))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

fn render_closures(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Evaluator Closure Evidence</h2>\n<p class=\"muted\">Closure Reports</p>\n<table><thead><tr><th>Verdict</th><th>Acceptance Criteria</th><th>Unresolved Concerns</th><th>Blockers</th></tr></thead><tbody>\n");
    for closure in json_array(pack, "closures") {
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&json_string(closure, "verdict")),
            escape_html(&string_array_text(json_array(
                closure,
                "acceptance_criteria_results"
            ))),
            escape_html(&string_array_text(json_array(
                closure,
                "unresolved_concerns"
            ))),
            escape_html(&string_array_text(json_array(closure, "blockers")))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

fn render_repair_attempts(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Repair Attempts</h2>\n");
    let attempts = json_array(pack, "repair_attempts");
    if attempts.is_empty() {
        html.push_str("<p class=\"muted\">No repair attempts were recorded.</p>\n");
    } else {
        html.push_str("<table><thead><tr><th>Attempt</th><th>Trigger</th><th>Status</th><th>Summary</th></tr></thead><tbody>\n");
        for attempt in attempts {
            writeln!(
                html,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&json_string(attempt, "attempt")),
                escape_html(&json_string(attempt, "trigger")),
                escape_html(&json_string(attempt, "status")),
                escape_html(&json_string(attempt, "summary"))
            )?;
        }
        html.push_str("</tbody></table>\n");
    }
    html.push_str("</section>\n");
    Ok(())
}

fn render_replay(
    html: &mut String,
    replay: &ao2_runtime::ReplaySummary,
    replay_status: &str,
) -> Result<()> {
    write!(
        html,
        r#"<section>
<h2>Replay Evidence</h2>
<p class="muted">Replay Integrity</p>
<div class="summary-grid">
<div class="metric"><div class="label">Status</div><div class="value ok">Replay {status}</div></div>
<div class="metric"><div class="label">Events</div><div class="value">{events}</div></div>
<div class="metric"><div class="label">Artifacts</div><div class="value">{artifacts}</div></div>
<div class="metric"><div class="label">Digest Failures</div><div class="value">{failures}</div></div>
</div>
<p>{event_types}</p>
</section>
"#,
        status = escape_html(replay_status),
        events = replay.event_count,
        artifacts = replay.artifact_count,
        failures = replay.digest_failures.len(),
        event_types = pills_from_strings(&replay.event_types)
    )?;
    Ok(())
}

fn render_markers(html: &mut String, pack: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Run Markers</h2>\n<p>");
    html.push_str(&pills(json_array(pack, "markers")));
    html.push_str("</p>\n</section>\n");
    Ok(())
}
