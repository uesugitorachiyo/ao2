use super::*;
use crate::cli_util::{escape_html, json_array, json_string, json_u64};
use crate::run_reporting::runs_list_json;
use crate::workbench_contract::WorkbenchOperator;
use crate::workbench_support_latest::latest_workbench_support_packet_json;
pub(super) struct WorkbenchRenderOptions<'a> {
    pub(super) operator: Option<&'a WorkbenchOperator>,
    pub(super) execution_enabled: bool,
    pub(super) can_operate: bool,
    pub(super) release_comparison_signing_enabled: bool,
    pub(super) control_plane_url: Option<&'a str>,
    pub(super) release_gate_artifact_path: Option<&'a str>,
}

pub(super) fn render_workbench(
    target: &Path,
    provenance_dir: &Path,
    options: WorkbenchRenderOptions<'_>,
) -> Result<String> {
    let operator = options.operator;
    let execution_enabled = options.execution_enabled;
    let can_operate = options.can_operate;
    let release_comparison_signing_enabled = options.release_comparison_signing_enabled;
    let control_plane_url = options.control_plane_url;
    let release_gate_artifact_path = options.release_gate_artifact_path;
    let runs = runs_list_json(target)?;
    let doctor = doctor_report_json(
        None,
        provenance_dir.to_path_buf(),
        None,
        None,
        "uesugitorachiyo/ao2".to_string(),
    )?;
    let provider_matrix = provider_matrix_json()?;
    let runs_list = json_array(&runs, "runs");
    let version = env!("CARGO_PKG_VERSION");
    let target_label = runtime_target_label();
    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AO2 Workbench</title>
<style>
:root {{
  color-scheme: light;
  --bg: #f5f7f9;
  --panel: #ffffff;
  --ink: #17202a;
  --muted: #5f6b7a;
  --line: #d9dee7;
  --accent: #176b87;
  --ok: #16794c;
  --warn: #9b5a00;
}}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
main {{ max-width: 1220px; margin: 0 auto; padding: 28px; }}
h1 {{ margin: 0 0 6px; font-size: 30px; letter-spacing: 0; }}
h2 {{ margin: 0 0 14px; font-size: 18px; letter-spacing: 0; }}
p {{ margin: 0; }}
.muted {{ color: var(--muted); }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin: 22px 0; }}
.metric, section {{ background: var(--panel); border: 1px solid var(--line); border-radius: 8px; }}
.metric {{ padding: 14px; min-height: 86px; }}
.label {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
.value {{ margin-top: 6px; font-size: 18px; font-weight: 700; overflow-wrap: anywhere; }}
section {{ margin: 16px 0; padding: 18px; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 9px 8px; border-top: 1px solid var(--line); text-align: left; vertical-align: top; }}
th {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
code {{ background: #eef2f6; border-radius: 4px; padding: 2px 4px; overflow-wrap: anywhere; }}
a {{ color: var(--accent); text-decoration: none; }}
.ok {{ color: var(--ok); }}
.warn {{ color: var(--warn); }}
.commands {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 10px; }}
.command {{ border: 1px solid var(--line); border-radius: 8px; padding: 10px; background: #fbfcfd; }}
form {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; align-items: end; }}
label {{ display: grid; gap: 5px; color: var(--muted); font-size: 12px; text-transform: uppercase; }}
input, select, button {{ min-height: 36px; border: 1px solid var(--line); border-radius: 6px; background: #fff; color: var(--ink); font: inherit; padding: 7px 9px; }}
button {{ background: var(--accent); color: #fff; border-color: var(--accent); font-weight: 700; cursor: pointer; }}
pre {{ margin: 12px 0 0; white-space: pre-wrap; background: #eef2f6; border: 1px solid var(--line); border-radius: 8px; padding: 12px; overflow-wrap: anywhere; }}
.queue-list {{ display: grid; gap: 10px; }}
.queue-job {{ border: 1px solid var(--line); border-radius: 8px; background: #fbfcfd; padding: 10px; }}
.queue-job-header {{ display: flex; gap: 8px; flex-wrap: wrap; align-items: center; justify-content: space-between; }}
.queue-actions {{ display: flex; gap: 8px; flex-wrap: wrap; margin-top: 8px; }}
.queue-actions a, .queue-actions button {{ min-height: 30px; border-radius: 6px; padding: 5px 8px; font: inherit; }}
.queue-actions a {{ border: 1px solid var(--line); background: #fff; }}
.queue-actions button {{ background: #fff; color: var(--accent); }}
.release-rollback-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(210px, 1fr)); gap: 10px; margin-top: 12px; }}
.release-rollback-card {{ border: 1px solid var(--line); border-radius: 8px; background: #fbfcfd; padding: 10px; }}
.release-rollback-card .status {{ font-weight: 700; margin: 4px 0; }}
.release-history-table td, .release-history-table th {{ white-space: nowrap; }}
.release-history-table td:last-child {{ white-space: normal; }}
.provider-score-ready {{ color: var(--ok); }}
.provider-score-warn, .provider-score-fail {{ color: var(--warn); }}
.trend-chart {{ margin: 10px 0; border: 1px solid var(--line); border-radius: 8px; background: #fbfcfd; padding: 10px; }}
.trend-chart svg {{ display: block; width: 100%; max-width: 820px; height: auto; }}
.trend-chart text {{ fill: var(--muted); font-size: 11px; }}
.trend-chart .chart-axis {{ stroke: #c6ced8; stroke-width: 1; }}
.trend-chart .chart-budget {{ fill: #8fb8c8; }}
.trend-chart .chart-cost {{ fill: #176b87; }}
</style>
</head>
<body>
<main data-api-token="{api_token}" data-execution-enabled="{execution_enabled}" data-operator-role="{operator_role}" data-can-operate="{can_operate}" data-release-comparison-signing-enabled="{release_comparison_signing_enabled}" data-default-control-plane-url="{default_control_plane_url}">
<h1>AO2 Workbench</h1>
<p class="muted">Local governed delivery control screen for <code>{target_path}</code>.</p>
<div class="grid">
  <div class="metric"><div class="label">Version</div><div class="value">{version}</div></div>
  <div class="metric"><div class="label">Runtime Target</div><div class="value">{target_label}</div></div>
  <div class="metric"><div class="label">Operator Role</div><div class="value">{operator_label}</div></div>
  <div class="metric"><div class="label">Run Queue</div><div class="value">{run_count} runs</div></div>
  <div class="metric"><div class="label">Doctor</div><div class="value {doctor_class}">{doctor_status}</div></div>
</div>
"#,
        api_token = escape_html(
            operator
                .map(|operator| operator.token.as_str())
                .unwrap_or("")
        ),
        execution_enabled = execution_enabled,
        release_comparison_signing_enabled = release_comparison_signing_enabled,
        operator_role = escape_html(
            operator
                .map(|operator| operator.role.as_str())
                .unwrap_or("")
        ),
        can_operate = can_operate,
        default_control_plane_url = escape_html(control_plane_url.unwrap_or("")),
        operator_label = escape_html(
            &operator
                .map(|operator| format!("{} ({})", operator.id, operator.role.as_str()))
                .unwrap_or_else(|| "offline".to_string())
        ),
        target_path = escape_html(&target.display().to_string()),
        version = escape_html(version),
        target_label = escape_html(&target_label),
        run_count = runs_list.len(),
        doctor_class = if json_string(&doctor, "status") == "ok" {
            "ok"
        } else {
            "warn"
        },
        doctor_status = escape_html(&json_string(&doctor, "status"))
    )?;

    render_workbench_runs(
        &mut html,
        runs_list,
        release_comparison_signing_enabled,
        control_plane_url,
    )?;
    render_workbench_provider_health(&mut html, &doctor)?;
    render_workbench_release_health(&mut html, version, release_gate_artifact_path)?;
    render_workbench_provider_readiness(&mut html, &provider_matrix)?;
    render_workbench_provider_contracts(&mut html)?;
    render_workbench_provider_contract_verification(&mut html)?;
    render_workbench_provider_smoke(&mut html, operator, execution_enabled, can_operate)?;
    render_workbench_provider_pilot(&mut html, operator, execution_enabled, can_operate)?;
    render_workbench_provider_readiness_data(&mut html, &provider_matrix)?;
    render_workbench_memory(
        &mut html,
        operator,
        can_operate,
        release_comparison_signing_enabled,
        control_plane_url,
    )?;
    render_workbench_templates(&mut html)?;
    render_workbench_launcher(&mut html, operator, execution_enabled, can_operate)?;
    render_workbench_queue(&mut html, execution_enabled, can_operate)?;
    render_workbench_support_trust(&mut html, target)?;
    render_workbench_commands(&mut html, version)?;
    render_workbench_script(&mut html)?;
    html.push_str("</main>\n</body>\n</html>\n");
    Ok(html)
}

fn render_workbench_runs(
    html: &mut String,
    runs: &[serde_json::Value],
    support_signing_enabled: bool,
    control_plane_url: Option<&str>,
) -> Result<()> {
    html.push_str("<section>\n<h2>Run Queue</h2>\n<table id=\"runs-table\"><thead><tr><th>Run</th><th>Status</th><th>Workflow</th><th>Digest Failures</th><th>Provider Score</th><th>Obligations</th><th>Artifacts</th><th>Inspect</th></tr></thead><tbody>\n");
    if runs.is_empty() {
        html.push_str("<tr><td colspan=\"8\" class=\"muted\">No AO2 runs found for this repository.</td></tr>\n");
    }
    for run in runs {
        let provider_score = run
            .get("provider_score")
            .unwrap_or(&serde_json::Value::Null);
        let provider_score_cell = if provider_score.is_null() {
            "<span class=\"muted\">not scored</span>".to_string()
        } else {
            let verdict = json_string(provider_score, "verdict");
            let schema = json_string(provider_score, "schema");
            format!(
                "<span class=\"provider-score-{verdict}\"><strong>{score}</strong> {verdict}</span><br><code>{schema}</code>",
                verdict = escape_html(&verdict),
                score = json_u64(provider_score, "score"),
                schema = escape_html(&schema)
            )
        };
        let obligation = run
            .get("obligation_ledger")
            .unwrap_or(&serde_json::Value::Null);
        let obligation_cell = if obligation
            .get("present")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            let summary = obligation
                .get("summary")
                .unwrap_or(&serde_json::Value::Null);
            format!(
                "<span><strong>{}</strong></span><br><code>pass={} fail={} unverified={}</code>",
                escape_html(&json_string(obligation, "verdict")),
                json_u64(summary, "pass"),
                json_u64(summary, "fail"),
                json_u64(summary, "unverified")
            )
        } else {
            "<span class=\"muted\">not emitted</span>".to_string()
        };
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><a href=\"file://{}\">cockpit</a> <a href=\"file://{}\">evidence</a></td><td><button type=\"button\" data-action=\"evidence-summary\" data-run-id=\"{}\">Summary</button></td></tr>",
            escape_html(&json_string(run, "run_id")),
            escape_html(&json_string(run, "status")),
            escape_html(&json_string(run, "workflow_id")),
            run.get("digest_failures").and_then(|value| value.as_u64()).unwrap_or(0),
            provider_score_cell,
            obligation_cell,
            escape_html(&json_string(run, "cockpit")),
            escape_html(&json_string(run, "evidence_pack")),
            escape_html(&json_string(run, "run_id"))
        )?;
    }
    html.push_str("</tbody></table>\n<h2>Run Evidence Summary</h2>\n<div class=\"queue-actions\"><button type=\"button\" id=\"run-evidence-export-summary-button\">Export Summary</button><button type=\"button\" id=\"run-evidence-changes-button\">Changed Since Previous</button><button type=\"button\" id=\"run-evidence-export-changes-button\">Export Changes</button></div>\n<pre id=\"run-evidence-summary-output\">Select Summary on a run to inspect replay, provider score, closure, and evidence links.</pre>\n");
    html.push_str(
        r#"<h3>Obligation Annotation</h3>
<form id="obligation-annotation-form" class="queue-actions">
  <label>Run<input id="obligation-run-id" name="run_id" placeholder="run-id"></label>
  <label>Obligation<input id="obligation-id" name="obligation_id" placeholder="OBL-001"></label>
  <label>Evidence Path<input id="obligation-evidence-path" name="evidence_path" placeholder="README.md"></label>
  <label>Evidence Line<input id="obligation-evidence-line" name="evidence_line" type="number" min="1" placeholder="12"></label>
  <label>Detail<input id="obligation-detail" name="detail" placeholder="operator-facing rule is documented"></label>
  <label>Waiver<input id="obligation-waiver" name="waiver" placeholder="required only for waived obligations"></label>
  <button type="submit">Annotate</button>
</form>
<pre id="obligation-annotation-output">Use an operator token to attach manual path/line evidence or an explicit waiver to a semantic obligation.</pre>
<h3>Obligation Gate</h3>
<form id="obligation-gate-form" class="queue-actions">
  <label>Run<input id="obligation-gate-run-id" name="run_id" placeholder="run-id"></label>
  <label>Allow Unsigned Obligation Gates (escape valve)<input name="allow_unsigned_obligation_gates" type="checkbox" value="1"></label>
  <button type="button" data-obligation-gate-stage="midpoint">Run Midpoint Gate</button>
  <button type="button" data-obligation-gate-stage="closure">Run Closure Gate</button>
</form>
<pre id="obligation-gate-output">Run midpoint or closure gates to verify extracted spec/rubric obligations against the current repository state. As of slice 18, gate production requires the workbench to be started with --support-signing-key by default; check the escape valve to emit an unsigned gate (downstream release-gate will reject it).</pre>
"#,
    );
    html.push_str("<h2>Run Evidence Diff</h2>\n<div class=\"queue-actions\"><label>Left<select id=\"run-evidence-diff-left\">");
    for run in runs {
        let run_id = json_string(run, "run_id");
        writeln!(
            html,
            "<option value=\"{}\">{}</option>",
            escape_html(&run_id),
            escape_html(&run_id)
        )?;
    }
    html.push_str("</select></label><label>Right<select id=\"run-evidence-diff-right\">");
    for run in runs {
        let run_id = json_string(run, "run_id");
        writeln!(
            html,
            "<option value=\"{}\">{}</option>",
            escape_html(&run_id),
            escape_html(&run_id)
        )?;
    }
    html.push_str("</select></label><button type=\"button\" id=\"run-evidence-diff-button\">Diff Evidence</button><button type=\"button\" id=\"run-evidence-export-diff-button\">Export Diff</button></div>\n<pre id=\"run-evidence-diff-output\">Select two runs to compare replay, digest, provider, and closure evidence.</pre>\n");
    if support_signing_enabled {
        write!(
            html,
            r#"<h3>Publish Signed Evidence</h3>
<form id="run-evidence-publish-form" class="queue-actions">
  <label>Run<input name="run_id" placeholder="run-id"></label>
  <label>Kind<select name="kind"><option value="evidence-pack">Evidence Pack</option><option value="operator-packet">Operator Packet</option></select></label>
  <label>Control Plane URL<input name="control_plane_url" value="{control_plane_url}" placeholder="http://127.0.0.1:8744"></label>
  <label>API Token<input name="api_token" type="password" placeholder="control-plane token"></label>
  <button type="submit">Publish Signed Evidence</button>
</form>
"#,
            control_plane_url = escape_html(control_plane_url.unwrap_or(""))
        )?;
    } else {
        html.push_str("<p class=\"muted\">Start Workbench with <code>--support-signing-key</code> to publish signed evidence packs to ao2-control-plane.</p>\n");
    }
    write!(
        html,
        r#"<h3>Open Signed Evidence Detail</h3>
<form id="run-evidence-detail-form" class="queue-actions">
  <label>Pack SHA256<input name="sha256" placeholder="64-character evidence-pack sha"></label>
  <label>Control Plane URL<input name="control_plane_url" value="{control_plane_url}" placeholder="http://127.0.0.1:8744"></label>
  <label>API Token<input name="api_token" type="password" placeholder="control-plane token"></label>
  <button type="submit">Open Evidence Detail</button>
  <button type="button" id="run-evidence-open-published-detail-button" disabled>Open Verified Detail</button>
</form>
<form id="run-evidence-dashboard-form" class="queue-actions">
  <label>Control Plane URL<input name="control_plane_url" value="{control_plane_url}" placeholder="http://127.0.0.1:8744"></label>
  <label>API Token<input name="api_token" type="password" placeholder="control-plane token"></label>
  <label>Gate Filter<select name="gate"><option value="attention">Needs attention</option><option value="all">All signed packs</option></select></label>
  <button type="submit">Open Gate Attention Dashboard</button>
</form>
"#,
        control_plane_url = escape_html(control_plane_url.unwrap_or(""))
    )?;
    html.push_str("<pre id=\"run-evidence-export-output\">Evidence export and publish paths will appear here.</pre>\n</section>\n");
    Ok(())
}

fn render_workbench_provider_health(html: &mut String, doctor: &serde_json::Value) -> Result<()> {
    html.push_str("<section>\n<h2>Provider Health</h2>\n<table><thead><tr><th>Provider</th><th>Available</th><th>Version</th></tr></thead><tbody>\n");
    if let Some(providers) = doctor.get("providers").and_then(|value| value.as_object()) {
        for (name, provider) in providers {
            let available = provider
                .get("available")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            writeln!(
                html,
                "<tr><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>",
                escape_html(name),
                if available { "ok" } else { "warn" },
                available,
                escape_html(&json_string(provider, "version"))
            )?;
        }
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

fn render_workbench_release_health(
    html: &mut String,
    version: &str,
    release_gate_artifact_path: Option<&str>,
) -> Result<()> {
    let release_tag = format!("v{version}");
    let command = format!(
        "ao2 doctor --json --release {release_tag} --release-asset-dir target/release-download/{release_tag} --provenance-dir target/release-download/{release_tag}"
    );
    writeln!(
        html,
        r#"<section>
<h2>Release Health</h2>
<p class="muted">Viewer-token protected release verification for private GitHub assets, local downloaded archives, signed provenance, and installed binary state.</p>
<table><thead><tr><th>Release</th><th>API</th><th>CLI Equivalent</th></tr></thead><tbody><tr><td><code>{release_tag}</code></td><td><code>/api/release-health</code></td><td><code>{command}</code></td></tr></tbody></table>
<div class="queue-actions">
  <label>Release<input id="release-health-release" value="{release_tag}"></label>
  <label>Asset Dir<input id="release-health-asset-dir" value="target/release-download/{release_tag}"></label>
  <label>Provenance Dir<input id="release-health-provenance-dir" value="target/release-download/{release_tag}"></label>
</div>
<div class="queue-actions"><button type="button" id="release-health-refresh">Refresh Release Health</button></div>
<pre id="release-health-output">Release health has not been loaded. Serve the Workbench with an API token, then refresh to run the doctor release check.</pre>
<div id="release-rollback-health" class="release-rollback-grid"></div>
<h3>Release History</h3>
<p class="muted">Compare downloaded private releases by doctor status, signed provenance, asset availability, and rollback evidence.</p>
<div class="queue-actions">
  <label>Download Dir<input id="release-history-dir" value="target/release-download"></label>
  <button type="button" id="release-history-refresh">Refresh Release History</button>
  <button type="button" id="release-history-export">Export Release History</button>
</div>
<div id="release-history-output" class="queue-list">Release history has not been loaded.</div>
<h3>Release Comparison Bundle</h3>
<p class="muted">Generate a signed comparison bundle from downloaded private release evidence, then verify it from the Workbench before handoff.</p>
<div class="queue-actions">
  <label>Output Dir<input id="release-comparison-out-dir" value="target/release-comparison-bundles"></label>
  <label>Bundle Dir<input id="release-comparison-bundle-dir" value=""></label>
  <button type="button" id="release-comparison-latest">Load Latest Verified</button>
  <button type="button" id="release-comparison-generate">Generate Signed Bundle</button>
  <button type="button" id="release-comparison-verify">Verify Bundle</button>
  <button type="button" id="release-comparison-export">Export Verification Evidence</button>
</div>
<pre id="release-comparison-output">Release comparison bundle controls require an API token. Generation also requires an operator token and server-side support signing key.</pre>
<h3>Release Gate</h3>
<p class="muted">Enrich a three-OS smoke summary with the latest obligation gate metadata, then run the same signed release gate used by the CLI.</p>
<form id="release-summary-enrich-form" class="queue-actions">
  <label>Summary<input name="summary" value="target/ao2-three-os-smoke-summary.json"></label>
  <label>Output<input name="out" value="target/ao2-three-os-smoke-summary.enriched.json"></label>
  <label>Run ID<input name="run_id" placeholder="latest obligation-gated run"></label>
  <button type="submit">Enrich Release Summary</button>
</form>
<form id="release-gate-form" class="queue-actions">
  <label>Summary<input name="summary" value="target/ao2-three-os-smoke-summary.enriched.json"></label>
  <label>Provenance Dir<input name="provenance_dir" value="target/release-packages"></label>
  <label>macOS Archive<input name="macos_archive" value=""></label>
  <label>Linux ARM Archive<input name="linux_archive" value=""></label>
  <label>Linux x86_64 Archive<input name="linux_x86_64_archive" value=""></label>
  <label>Windows Archive<input name="windows_archive" value=""></label>
  <label>Artifact Out<input name="artifact_out" value="target/release-gate-artifact.json"></label>
  <label>Require Native Windows<input name="require_native_windows" type="checkbox" value="1"></label>
  <label>Allow Unsigned Obligation Gates (escape valve)<input name="allow_unsigned_obligation_gates" type="checkbox" value="1"></label>
  <button type="submit">Run Release Gate</button>
</form>
<pre id="release-gate-output">Release gate controls require an operator token. Enrich the summary before running the gate.</pre>
<form id="release-gate-artifact-form" class="queue-actions">
  <label>Gate Artifact<input id="release-gate-artifact-path" name="path" value="{release_gate_artifact_path}"></label>
  <button type="submit">Open Release Gate Artifact</button>
</form>
<pre id="release-gate-artifact-output">Load a release gate or dry-run artifact by local path.</pre>
<h3>Release Evidence Retention</h3>
<p class="muted">Preview and prune old downloaded release evidence and signed comparison bundles. Prune requires an operator token and keeps the newest matching directories.</p>
<div class="queue-actions">
  <label>Keep Releases<input id="release-retention-keep-releases" value="3"></label>
  <label>Keep Bundles<input id="release-retention-keep-bundles" value="3"></label>
  <button type="button" id="release-retention-preview">Preview Prune</button>
  <button type="button" id="release-retention-prune">Prune Old Evidence</button>
</div>
<pre id="release-retention-output">Release retention controls require an operator token. Preview before pruning.</pre>
</section>"#,
        release_tag = escape_html(&release_tag),
        command = escape_html(&command),
        release_gate_artifact_path =
            escape_html(release_gate_artifact_path.unwrap_or("target/release-gate-dry-run.json"))
    )?;
    Ok(())
}

fn render_workbench_provider_readiness(
    html: &mut String,
    matrix: &serde_json::Value,
) -> Result<()> {
    html.push_str("<section>\n<h2>Provider Readiness</h2>\n<table><thead><tr><th>Provider</th><th>Available</th><th>Timeout</th><th>Boundary</th><th>Transcript Fields</th><th>Policy Invariants</th></tr></thead><tbody>\n");
    for provider in json_array(matrix, "providers") {
        let available = provider
            .get("doctor")
            .and_then(|doctor| doctor.get("available"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let transcript_fields = json_array(provider, "transcript_fields")
            .iter()
            .map(|field| escape_html(field.as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(", ");
        let policy_invariants = json_array(provider, "policy_invariants")
            .iter()
            .map(|invariant| escape_html(invariant.as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("<br>");
        writeln!(
            html,
            "<tr><td><code>{provider}</code></td><td class=\"{class}\">{available}</td><td>{timeout}s</td><td><code>{boundary}</code></td><td>{fields}</td><td>{invariants}</td></tr>",
            provider = escape_html(&json_string(provider, "provider")),
            class = if available { "ok" } else { "warn" },
            available = available,
            timeout = provider
                .get("timeout_seconds")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            boundary = escape_html(&json_string(provider, "execution_boundary")),
            fields = transcript_fields,
            invariants = policy_invariants
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

fn render_workbench_provider_contracts(html: &mut String) -> Result<()> {
    html.push_str("<section>\n<h2>Provider Contracts</h2>\n");
    html.push_str("<table><thead><tr><th>Provider</th><th>Phase</th><th>Same Contract As</th><th>Boundary</th><th>Live Guard</th><th>Prompt Command</th></tr></thead><tbody>\n");
    for provider in ["scripted", "codex", "claude", "antigravity"] {
        let contract = provider_contract_json(provider)?;
        let args = json_array(&contract["prompt_command"], "args")
            .iter()
            .map(json_value_text)
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(
            html,
            "<tr><td><code>{provider}</code></td><td>{phase}</td><td>{same_contract}</td><td><code>{boundary}</code></td><td><code>{guard}</code></td><td><code>{command} {args}</code></td></tr>",
            provider = escape_html(&json_string(&contract, "provider")),
            phase = escape_html(&json_string(&contract, "phase")),
            same_contract = escape_html(&json_string(&contract, "same_contract_as")),
            boundary = escape_html(&json_string(&contract, "execution_boundary")),
            guard = escape_html(&json_string(&contract, "live_execution_guard_env")),
            command = escape_html(&json_string(&contract["prompt_command"], "command")),
            args = escape_html(&args)
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

pub(super) fn workbench_provider_contracts_json() -> serde_json::Value {
    provider_contract_verify_json(&[
        "codex".to_string(),
        "claude".to_string(),
        "antigravity".to_string(),
    ])
}

fn render_workbench_provider_contract_verification(html: &mut String) -> Result<()> {
    let verification = workbench_provider_contracts_json();
    let reasons = json_array(&verification, "reasons");
    let reason_text = if reasons.is_empty() {
        "none".to_string()
    } else {
        reasons
            .iter()
            .map(|reason| {
                format!(
                    "{}:{} {}",
                    json_string(reason, "provider"),
                    json_string(reason, "code"),
                    json_string(reason, "message")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let required = json_array(&verification, "required_providers")
        .iter()
        .map(json_value_text)
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        html,
        "<section>\n<h2>Provider Contract Verification</h2>\n<p class=\"muted\">Fail-closed release gate for required live provider contracts.</p>\n<table><thead><tr><th>Schema</th><th>Status</th><th>Required Providers</th><th>Reasons</th></tr></thead><tbody><tr><td><code>{schema}</code></td><td class=\"{class}\">{status}</td><td><code>{required}</code></td><td>{reasons}</td></tr></tbody></table>\n<pre id=\"contract-verification-output\">schema={schema}\nstatus={status}\nrequired={required}\nreasons={reason_text}</pre>\n</section>",
        schema = escape_html(&json_string(&verification, "schema")),
        class = if json_string(&verification, "status") == "verified" {
            "ok"
        } else {
            "warn"
        },
        status = escape_html(&json_string(&verification, "status")),
        required = escape_html(&required),
        reasons = escape_html(&reason_text).replace('\n', "<br>"),
        reason_text = escape_html(&reason_text)
    )?;
    Ok(())
}

fn render_workbench_provider_smoke(
    html: &mut String,
    operator: Option<&WorkbenchOperator>,
    execution_enabled: bool,
    can_operate: bool,
) -> Result<()> {
    html.push_str("<section>\n<h2>Provider Smoke</h2>\n");
    html.push_str("<p class=\"muted\">Runs the local scripted provider readiness loop and records <code>.ao2/provider-smoke/history.json</code>.</p>\n");
    if operator.is_none() {
        html.push_str("<p class=\"muted\">Serve with an API token to enable provider smoke actions.</p>\n<div class=\"queue-actions\"><button type=\"button\" id=\"provider-smoke-button\" disabled>Run Provider Smoke</button></div>\n<pre id=\"provider-smoke-output\">Provider smoke is disabled in static export.</pre>\n</section>\n");
        return Ok(());
    }
    if !can_operate {
        html.push_str("<p class=\"muted\">Viewer mode can inspect readiness data. Use an operator token to run provider smoke.</p>\n<pre id=\"provider-smoke-output\">Operator token required.</pre>\n</section>\n");
        return Ok(());
    }
    if !execution_enabled {
        html.push_str("<p class=\"muted\">Start the workbench with <code>--enable-execution</code> to run provider smoke from the UI.</p>\n");
    }
    html.push_str("<div class=\"queue-actions\"><label>Minimum Score<input id=\"provider-smoke-minimum-score\" type=\"number\" min=\"0\" max=\"100\" value=\"90\"></label>");
    html.push_str("<label>Live Provider<select id=\"provider-smoke-live-provider\"><option value=\"\">Scripted only</option><option value=\"codex\">Codex</option><option value=\"claude\">Claude</option></select></label>");
    if execution_enabled {
        html.push_str(
            "<button type=\"button\" id=\"provider-smoke-button\">Run Provider Smoke</button>",
        );
    } else {
        html.push_str("<button type=\"button\" id=\"provider-smoke-button\" disabled>Run Provider Smoke</button>");
    }
    html.push_str("</div>\n<pre id=\"provider-smoke-output\">Provider smoke has not run in this session.</pre>\n</section>\n");
    Ok(())
}

fn render_workbench_provider_pilot(
    html: &mut String,
    operator: Option<&WorkbenchOperator>,
    execution_enabled: bool,
    can_operate: bool,
) -> Result<()> {
    html.push_str("<section>\n<h2>Provider Pilot</h2>\n");
    html.push_str("<p class=\"muted\">Builds a gated provider pilot command preview from provider smoke history. This does not call Codex, Claude, or start a run.</p>\n");
    if operator.is_none() {
        html.push_str("<p class=\"muted\">Serve with an API token to enable provider pilot previews.</p>\n<pre id=\"provider-pilot-output\">Provider pilot is disabled in static export.</pre>\n</section>\n");
        return Ok(());
    }
    if !can_operate {
        html.push_str("<p class=\"muted\">Viewer mode can inspect readiness data. Use an operator token to build provider pilot commands.</p>\n<pre id=\"provider-pilot-output\">Operator token required.</pre>\n</section>\n");
        return Ok(());
    }
    html.push_str("<div class=\"queue-actions\">");
    html.push_str("<label>Provider<select id=\"provider-pilot-provider\"><option value=\"codex\">Codex</option><option value=\"claude\">Claude</option><option value=\"antigravity\">Antigravity</option></select></label>");
    html.push_str("<label>Template<select id=\"provider-pilot-template\">");
    for template in TASK_TEMPLATES {
        writeln!(
            html,
            "<option value=\"{}\">{}</option>",
            escape_html(template.name),
            escape_html(template.name)
        )?;
    }
    html.push_str("</select></label>");
    html.push_str("<label>Run ID<input id=\"provider-pilot-run-id\" placeholder=\"provider-pilot-run\"></label>");
    html.push_str("<label>Prompt File<input id=\"provider-pilot-prompt-file\" placeholder=\"/path/to/pilot-prompt.txt\"></label>");
    html.push_str("<label>Repair Attempts<input id=\"provider-pilot-max-repair-attempts\" type=\"number\" min=\"0\" value=\"1\"></label>");
    html.push_str("<label>Max Budget USD<input id=\"provider-pilot-max-budget-usd\" type=\"number\" min=\"0.01\" step=\"0.01\" value=\"1.00\"></label>");
    html.push_str("<label>Minimum Score<input id=\"provider-pilot-minimum-score\" type=\"number\" min=\"0\" max=\"100\" value=\"90\"></label>");
    html.push_str("<button type=\"button\" id=\"provider-pilot-preflight-button\">Preflight Provider Pilot</button>");
    html.push_str(
        "<button type=\"button\" id=\"provider-pilot-button\">Build Provider Pilot</button>",
    );
    if execution_enabled {
        html.push_str("<button type=\"button\" id=\"provider-pilot-start-button\">Start Provider Pilot</button>");
    } else {
        html.push_str("<button type=\"button\" id=\"provider-pilot-start-button\" disabled>Start Provider Pilot</button>");
    }
    html.push_str("</div>\n<div class=\"queue-actions\">");
    html.push_str("<label>Acceptance Bundle<input id=\"provider-pilot-acceptance-bundle\" placeholder=\"target/provider-pilot-acceptance/v0.4.70/provider-pilot-acceptance.json\"></label>");
    html.push_str("<label>Acceptance Provider<select id=\"provider-pilot-acceptance-provider\"><option value=\"\">All</option><option value=\"codex\">Codex</option><option value=\"claude\">Claude</option><option value=\"antigravity\">Antigravity</option></select></label>");
    html.push_str("<label>Replay<select id=\"provider-pilot-acceptance-replay-status\"><option value=\"\">All</option><option value=\"accepted\">Accepted</option><option value=\"rejected\">Rejected</option></select></label>");
    html.push_str("<label>Min Score<input id=\"provider-pilot-acceptance-min-score\" type=\"number\" min=\"0\" max=\"100\" placeholder=\"90\"></label>");
    html.push_str("<label>Sort<select id=\"provider-pilot-acceptance-sort\"><option value=\"newest\">Newest</option><option value=\"score_desc\">Score High</option><option value=\"score_asc\">Score Low</option><option value=\"provider_asc\">Provider</option><option value=\"run_id_asc\">Run ID</option></select></label>");
    html.push_str("<label>Limit<input id=\"provider-pilot-acceptance-limit\" type=\"number\" min=\"1\" value=\"10\"></label>");
    html.push_str("<button type=\"button\" id=\"provider-pilot-acceptance-latest-button\">Load Latest Acceptance</button>");
    html.push_str("<button type=\"button\" id=\"provider-pilot-acceptance-export-button\">Export Acceptance Evidence</button>");
    html.push_str("<button type=\"button\" id=\"provider-pilot-acceptance-export-latest-button\">Export Latest Acceptance</button>");
    html.push_str("<button type=\"button\" id=\"provider-pilot-cost-ledger-button\">Load Cost Ledger</button>");
    html.push_str(
        "<button type=\"button\" id=\"provider-pilot-cost-trend-button\">Load Cost Trend</button>",
    );
    html.push_str("</div>\n<pre id=\"provider-pilot-output\">Provider pilot command preview will appear here.</pre>\n</section>\n");
    Ok(())
}

fn render_workbench_provider_readiness_data(
    html: &mut String,
    matrix: &serde_json::Value,
) -> Result<()> {
    let matrix_json = serde_json::to_string(matrix)?.replace("</", "<\\/");
    writeln!(
        html,
        "<script type=\"application/json\" id=\"provider-readiness-data\">{matrix_json}</script>"
    )?;
    Ok(())
}

fn render_workbench_memory(
    html: &mut String,
    operator: Option<&WorkbenchOperator>,
    can_operate: bool,
    support_signing_enabled: bool,
    control_plane_url: Option<&str>,
) -> Result<()> {
    let token = operator
        .map(|operator| operator.token.as_str())
        .unwrap_or("");
    writeln!(
        html,
        r#"<section>
<h2>Hermes Memory</h2>
<p class="muted">Search AO2 append-only memory records and export filtered bundles for Hermes or control-plane readers.</p>
<form id="memory-search-form">
  <input type="hidden" name="token" value="{token}">
  <label>Query<input name="query" placeholder="hermes"></label>
  <label>Limit<input name="limit" type="number" value="10" min="1"></label>
  <button type="submit">Search Memory</button>
  <button type="button" id="memory-recent-button">Load Recent</button>
</form>
<pre id="memory-search-output" class="json-output"></pre>
<form id="memory-link-run-form">
  <label>Memory ID<input name="memory_id" placeholder="mem-..."></label>
  <label>Run ID<input name="run_id" placeholder="run-id"></label>
  <label>Relationship<input name="relationship" value="related"></label>
  <button type="submit"{disabled}>Link Memory To Run</button>
</form>
<form id="memory-export-form" method="post" action="/api/memory/export?token={token}">
  <label>Export Query<input name="query" placeholder="hermes"></label>
  <label>Limit<input name="limit" type="number" value="50" min="1"></label>
  <button type="submit"{disabled}>Export Memory Bundle</button>
</form>
<form id="memory-publish-latest-form" method="post" action="/api/memory/publish-latest?token={token}">
  <label>Control Plane URL<input name="control_plane_url" value="{control_plane_url}" placeholder="http://127.0.0.1:8744"></label>
  <label>Control Plane Token<input name="api_token" type="password" placeholder="AO2_CP_API_TOKEN"></label>
  <label><input type="checkbox" name="allow_unsigned_memory_export" value="1"> Allow Unsigned Memory Export (escape valve; slice 19 default-on requires signed export)</label>
  <button type="submit"{disabled}>Publish Latest Export</button>
</form>
<form id="memory-control-plane-dashboard-form" method="post" action="/api/memory/control-plane-dashboard?token={token}">
  <label>Control Plane URL<input name="control_plane_url" value="{control_plane_url}" placeholder="http://127.0.0.1:8744"></label>
  <label>Control Plane Token<input name="api_token" type="password" placeholder="AO2_CP_API_TOKEN"></label>
  <button type="submit"{disabled}>Open Memory Dashboard</button>
</form>
<p class="muted">Export signing: <strong>{signing}</strong>. API: <code>/api/memory/search</code>, <code>/api/memory/recent</code>, <code>/api/memory/link-run</code>, <code>/api/memory/export</code>, <code>/api/memory/publish-latest</code>, and <code>/api/memory/control-plane-dashboard</code>.</p>
</section>"#,
        token = escape_html(token),
        control_plane_url = escape_html(control_plane_url.unwrap_or("")),
        disabled = if can_operate { "" } else { " disabled" },
        signing = if support_signing_enabled {
            "enabled"
        } else {
            "unsigned"
        }
    )?;
    Ok(())
}

fn render_workbench_templates(html: &mut String) -> Result<()> {
    html.push_str("<section>\n<h2>Task Templates</h2>\n<table><thead><tr><th>Name</th><th>Description</th><th>Command</th></tr></thead><tbody>\n");
    for template in TASK_TEMPLATES {
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td><td><code>ao2 run --template {} --target . --provider scripted</code></td></tr>",
            escape_html(template.name),
            escape_html(template.description),
            escape_html(template.name)
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    Ok(())
}

fn render_workbench_launcher(
    html: &mut String,
    operator: Option<&WorkbenchOperator>,
    execution_enabled: bool,
    can_operate: bool,
) -> Result<()> {
    html.push_str("<section>\n<h2>Launch Governed Run</h2>\n");
    html.push_str("<div class=\"queue-job\" id=\"provider-warning-panel\"><h2>Provider Safety Warnings</h2><div id=\"provider-warning-output\" class=\"warn\">Select a provider to review execution boundaries before starting.</div></div>\n");
    if operator.is_none() {
        html.push_str("<p class=\"muted\">Serve the workbench with <code>ao2 workbench serve</code> to enable local API-backed command generation.</p>\n</section>\n");
        return Ok(());
    }
    if !can_operate {
        html.push_str("<p class=\"muted\">Viewer mode can inspect runs, queue status, job details, and audit history. Use an operator token to launch, cancel, retry, or export queue jobs.</p>\n</section>\n");
        return Ok(());
    }
    html.push_str("<form id=\"launch-form\">\n");
    html.push_str("<label>Template<select name=\"template\">\n");
    for template in TASK_TEMPLATES {
        writeln!(
            html,
            "<option value=\"{}\">{}</option>",
            escape_html(template.name),
            escape_html(template.name)
        )?;
    }
    html.push_str("</select></label>\n<label>Provider<select name=\"provider\">\n");
    for profile in provider_profiles() {
        writeln!(
            html,
            "<option value=\"{}\">{}</option>",
            escape_html(profile.name),
            escape_html(profile.name)
        )?;
    }
    html.push_str("</select></label>\n");
    html.push_str("<label>Run ID<input name=\"run_id\" placeholder=\"workbench-run\"></label>\n");
    html.push_str("<label>AO Operator RunSpec<input name=\"ao_operator_runspec\" placeholder=\"/path/to/factory-v3/ao/runspecs/factory-v3-smoke.yaml\"></label>\n");
    html.push_str("<label>Prompt File<input name=\"provider_prompt_file\" placeholder=\"/path/to/prompt.sh\"></label>\n");
    html.push_str("<label>Repair Attempts<input name=\"max_repair_attempts\" type=\"number\" min=\"0\" value=\"1\"></label>\n");
    html.push_str("<label>Minimum Score<input name=\"minimum_score\" type=\"number\" min=\"0\" max=\"100\" placeholder=\"90\"></label>\n");
    let button = if execution_enabled {
        "Start Queued Run"
    } else {
        "Build Command"
    };
    let output = if execution_enabled {
        "Queued run status will appear here."
    } else {
        "Command preview will appear here."
    };
    writeln!(html, "<button type=\"submit\">{button}</button>\n</form>")?;
    writeln!(html, "<pre id=\"launch-output\">{output}</pre>")?;
    html.push_str("<h3>Resume From Rejected Evidence</h3>\n");
    html.push_str("<form id=\"repair-resume-form\">\n");
    html.push_str("<label>Source Evidence Pack<input name=\"evidence_pack\" placeholder=\"/path/to/rejected/evidence-pack.json\"></label>\n");
    html.push_str("<label>Template<select name=\"template\">\n");
    for template in TASK_TEMPLATES {
        writeln!(
            html,
            "<option value=\"{}\">{}</option>",
            escape_html(template.name),
            escape_html(template.name)
        )?;
    }
    html.push_str("</select></label>\n<label>Provider<select name=\"provider\">\n");
    for profile in provider_profiles() {
        writeln!(
            html,
            "<option value=\"{}\">{}</option>",
            escape_html(profile.name),
            escape_html(profile.name)
        )?;
    }
    html.push_str("</select></label>\n");
    html.push_str(
        "<label>New Run ID<input name=\"run_id\" placeholder=\"workbench-repair-run\"></label>\n",
    );
    html.push_str("<label>Repair Prompt File<input name=\"provider_prompt_file\" placeholder=\"/path/to/repair-prompt.sh\"></label>\n");
    html.push_str("<label>Repair Attempts<input name=\"max_repair_attempts\" type=\"number\" min=\"0\" value=\"1\"></label>\n");
    html.push_str("<label>Max Budget USD<input name=\"max_budget_usd\" type=\"number\" min=\"0.01\" step=\"0.01\" placeholder=\"1.00\"></label>\n");
    if execution_enabled {
        html.push_str("<button type=\"submit\">Start Repair Resume</button>\n");
    } else {
        html.push_str("<button type=\"submit\" disabled>Start Repair Resume</button>\n");
    }
    html.push_str("</form>\n<pre id=\"repair-resume-output\">Repair-resume jobs require an operator token and <code>--enable-execution</code>.</pre>\n</section>");
    Ok(())
}

fn render_workbench_queue(
    html: &mut String,
    execution_enabled: bool,
    can_operate: bool,
) -> Result<()> {
    if execution_enabled {
        html.push_str("<section>\n<h2>Execution Queue</h2>\n<div class=\"queue-actions\">\n");
        html.push_str("<label>Status<select id=\"queue-status-filter\"><option value=\"\">All</option><option value=\"queued\">Queued</option><option value=\"running\">Running</option><option value=\"accepted\">Accepted</option><option value=\"rejected\">Rejected</option><option value=\"failed\">Failed</option><option value=\"cancelled\">Cancelled</option><option value=\"interrupted\">Interrupted</option></select></label>\n");
        html.push_str(
            "<label>Template<select id=\"queue-template-filter\"><option value=\"\">All</option>\n",
        );
        for template in TASK_TEMPLATES {
            writeln!(
                html,
                "<option value=\"{}\">{}</option>",
                escape_html(template.name),
                escape_html(template.name)
            )?;
        }
        html.push_str("</select></label>\n");
        if can_operate {
            html.push_str("<button type=\"button\" id=\"queue-export-preview-button\">Preview Support Bundle</button>\n");
            html.push_str("<button type=\"button\" id=\"queue-export-button\">Export Support Bundle</button>\n");
        } else {
            html.push_str("<span class=\"muted\">Viewer mode: queue actions and support bundle export are disabled.</span>\n");
        }
        html.push_str("</div>\n<div id=\"queue-output\" class=\"queue-list\">No queued runs yet.</div>\n<pre id=\"queue-log-output\">Select Logs on a queue job to watch live output.</pre>\n<pre id=\"queue-detail-output\">Select Details on a queue job to inspect logs.</pre>\n</section>\n");
        html.push_str("<section>\n<h2>Queue Audit</h2>\n<div class=\"queue-actions\"><label>Action<select id=\"queue-audit-action-filter\"><option value=\"\">All</option><option value=\"start\">Start</option><option value=\"repair_resume_start\">Repair Resume</option><option value=\"cancel\">Cancel</option><option value=\"retry\">Retry</option></select></label><button type=\"button\" id=\"queue-audit-refresh\">Refresh Audit</button></div>\n<div id=\"queue-audit-output\" class=\"queue-list\">No queue audit events yet.</div>\n</section>\n");
    }
    html.push_str(
        r#"<section>
<h2>Project-Start Next Action</h2>
<p class="muted">Read-only AO2 preview for Hermes/factory project-start completion. It never executes queue jobs, submits queue entries, rebuilds wrappers, or changes AO artifacts.</p>
<form id="project-start-next-action-form" class="queue-actions">
  <label>Run<input id="project-start-next-action-run-id" name="run_id" placeholder="project-start run-id"></label>
  <label>Out Dir<input id="project-start-next-action-out-dir" name="out_dir" placeholder="target/factory-project-start"></label>
  <label>Contract<input id="project-start-next-action-contract" name="contract" value="docs/contracts/hermes-project-start-poll-act-contract.v1.json"></label>
  <button type="submit" id="project-start-next-action-refresh">Preview Next Action</button>
</form>
<pre id="project-start-next-action-output">Load a project-start run to see whether Hermes should wait, ask for operator review, call the completion probe, or publish the compact operator record.</pre>
</section>
<section>
<h2>Project-Start Operator Record</h2>
<p class="muted">Operator-token protected AO2 producer action. It only writes the explicit record path after the next-action preflight reports publish_operator_record.</p>
<form id="project-start-operator-record-form" class="queue-actions">
  <label>Run<input id="project-start-operator-record-run-id" name="run_id" placeholder="project-start run-id"></label>
  <label>Out Dir<input id="project-start-operator-record-out-dir" name="out_dir" placeholder="target/factory-project-start"></label>
  <label>Contract<input id="project-start-operator-record-contract" name="contract" value="docs/contracts/hermes-project-start-poll-act-contract.v1.json"></label>
  <label>Record Out<input id="project-start-operator-record-record-out" name="record_out" placeholder="target/factory-project-start/operator-record.json"></label>
  <button type="submit" id="project-start-operator-record-publish">Publish Operator Record</button>
</form>
<pre id="project-start-operator-record-output">Publish a compact operator record only after the next-action preview is ready.</pre>
</section>
<section>
<h2>Project-Start Hermes Flow Contract</h2>
<p class="muted">Viewer-readable AO2 contract for the Hermes next-action to operator-record loop. It writes only the explicit contract path and never operates the queue.</p>
<form id="project-start-hermes-flow-contract-form" class="queue-actions">
  <label>Contract Out<input id="project-start-hermes-flow-contract-out" name="out" placeholder="target/factory-project-start/hermes-flow-contract.json"></label>
  <button type="submit" id="project-start-hermes-flow-contract-refresh">Fetch Flow Contract</button>
</form>
<pre id="project-start-hermes-flow-contract-output">Fetch the current AO2-owned Hermes project-start flow contract.</pre>
</section>
"#,
    );
    Ok(())
}

fn render_workbench_support_trust(html: &mut String, target: &Path) -> Result<()> {
    let packet = latest_workbench_support_packet_json(target)?;
    writeln!(
        html,
        "<div id=\"support-packet-output\">{}</div>",
        render_workbench_support_packet_html(&packet)?
    )?;
    Ok(())
}

fn render_workbench_support_packet_html(packet: &serde_json::Value) -> Result<String> {
    let present = packet
        .get("present")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !present {
        return Ok(r#"<section>
<h2>Latest Support Packet</h2>
<p class="muted">No signed support bundle has been exported for this repository yet.</p>
<h2>Support Bundle Trust</h2>
<p>Status: <strong>Unsigned</strong></p>
</section>"#
            .to_string());
    }

    let support_metadata = &packet["support_metadata"];
    let status = if support_metadata
        .get("signature_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        "Signature verified"
    } else {
        "Signature not verified"
    };
    let mut html = String::new();
    writeln!(
        html,
        r#"<section>
<h2>Latest Support Packet</h2>
<div class="metrics">
  <div class="metric"><div class="label">Queue Jobs</div><div class="value">{queue_jobs}</div></div>
  <div class="metric"><div class="label">Audit Events</div><div class="value">{audit_events}</div></div>
  <div class="metric"><div class="label">Job Logs</div><div class="value">{job_logs}</div></div>
  <div class="metric"><div class="label">Evidence Exports</div><div class="value">{evidence_exports}</div></div>
  <div class="metric"><div class="label">Redactions</div><div class="value">{redactions}</div></div>
</div>
<p>{bundle_link} <code>{bundle_path}</code></p>
<p>Bundle SHA256: <code>{bundle_sha256}</code></p>
<h2>Support Bundle Trust</h2>
<div class="metrics">
  <div class="metric"><div class="label">Status</div><div class="value">{status}</div></div>
  <div class="metric"><div class="label">Signer</div><div class="value">{signer_id}</div></div>
</div>
<p>Metadata SHA256: <code>{metadata_sha256}</code></p>
<p>Public key SHA256: <code>{public_key_sha256}</code></p>
{hermes_flow_contract}
{redaction_audit}
<h2>Queue Failure Diagnostics</h2>
<table><thead><tr><th>Run</th><th>Failure</th><th>Exit</th><th>Timed Out</th><th>Primary Error</th><th>Recovery</th><th>Stderr</th></tr></thead><tbody>
{queue_diagnoses}
</tbody></table>
<h2>Evidence Exports</h2>
<table><thead><tr><th>Kind</th><th>Subject</th><th>SHA256</th><th>Path</th></tr></thead><tbody>"#,
        queue_jobs = json_u64(packet, "queue_job_count"),
        audit_events = json_u64(packet, "audit_event_count"),
        job_logs = json_u64(packet, "job_log_count"),
        evidence_exports = json_u64(packet, "evidence_export_count"),
        redactions = json_u64(&packet["redaction_audit"], "redaction_count"),
        bundle_link = workbench_file_anchor("Open Bundle", &json_string(packet, "bundle_path")),
        bundle_path = escape_html(&json_string(packet, "bundle_path")),
        bundle_sha256 = escape_html(&json_string(packet, "bundle_sha256")),
        status = escape_html(status),
        signer_id = escape_html(&json_string(support_metadata, "signer_id")),
        metadata_sha256 = escape_html(&json_string(support_metadata, "metadata_sha256")),
        public_key_sha256 = escape_html(&json_string(support_metadata, "public_key_sha256")),
        hermes_flow_contract = render_workbench_support_hermes_flow_contract_html(
            &packet["hermes_project_start_flow_contract"]
        )?,
        redaction_audit = render_workbench_redaction_audit_section(&packet["redaction_audit"]),
        queue_diagnoses = render_workbench_queue_failure_diagnostics_table(
            json_array(packet, "queue_job_diagnoses"),
            7
        )
    )?;

    let evidence_exports = json_array(packet, "evidence_exports");
    if evidence_exports.is_empty() {
        html.push_str("<tr><td colspan=\"4\" class=\"muted\">No evidence exports are attached to the latest support bundle.</td></tr>\n");
    }
    for evidence_export in evidence_exports {
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td><code>{}</code></td><td><code>{}</code></td><td><code>{}</code></td></tr>",
            escape_html(&json_string(evidence_export, "kind")),
            escape_html(&workbench_support_evidence_export_subject(evidence_export)),
            escape_html(&json_string(evidence_export, "sha256")),
            escape_html(&json_string(evidence_export, "path"))
        )?;
    }
    html.push_str("</tbody></table>\n</section>");
    Ok(html)
}

fn render_workbench_support_hermes_flow_contract_html(
    contract: &serde_json::Value,
) -> Result<String> {
    if !contract
        .get("present")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(
            "<h2>Hermes Project-Start Flow Contract</h2>\n<p class=\"muted\">No Hermes project-start flow contract is attached to this support packet.</p>\n"
                .to_string(),
        );
    }
    Ok(format!(
        r#"<h2>Hermes Project-Start Flow Contract</h2>
<div class="metrics">
  <div class="metric"><div class="label">Schema</div><div class="value">{schema}</div></div>
  <div class="metric"><div class="label">Preview Role</div><div class="value">{preview_role}</div></div>
  <div class="metric"><div class="label">Publish Role</div><div class="value">{publish_role}</div></div>
</div>
<p>Contract SHA256: <code>{contract_sha256}</code></p>
<p>Raw queue JSON scrape required: <code>{raw_scrape}</code></p>
<p>Release acceptance owner: <code>{release_owner}</code></p>
<p>Side effects: execute_queue=<code>{execute_queue}</code>, submit_queue_entry=<code>{submit_queue}</code>, rebuild_wrappers=<code>{rebuild_wrappers}</code>, mutate_control_plane=<code>{mutate_cp}</code></p>
"#,
        schema = escape_html(&json_string(contract, "schema_version")),
        preview_role = escape_html(&json_string(contract, "preview_role")),
        publish_role = escape_html(&json_string(contract, "publish_role")),
        contract_sha256 = escape_html(&json_string(contract, "contract_sha256")),
        raw_scrape = contract["raw_queue_json_scrape_required"]
            .as_bool()
            .unwrap_or(false),
        release_owner = escape_html(&json_string(contract, "release_acceptance_owner")),
        execute_queue = contract["would_execute_queue"].as_bool().unwrap_or(false),
        submit_queue = contract["would_submit_queue_entry"]
            .as_bool()
            .unwrap_or(false),
        rebuild_wrappers = contract["would_rebuild_wrappers"]
            .as_bool()
            .unwrap_or(false),
        mutate_cp = contract["would_mutate_control_plane"]
            .as_bool()
            .unwrap_or(false)
    ))
}

fn render_workbench_script(html: &mut String) -> Result<()> {
    html.push_str(
        r#"<script>
(() => {
  const root = document.querySelector('main[data-api-token]');
  const token = root ? root.dataset.apiToken : '';
  const executionEnabled = root ? root.dataset.executionEnabled === 'true' : false;
  const canOperate = root ? root.dataset.canOperate === 'true' : false;
  const releaseComparisonSigningEnabled = root ? root.dataset.releaseComparisonSigningEnabled === 'true' : false;
  const defaultControlPlaneUrl = root ? root.dataset.defaultControlPlaneUrl || '' : '';
  const runsTable = document.getElementById('runs-table');
  const runEvidenceSummaryOutput = document.getElementById('run-evidence-summary-output');
  const obligationAnnotationForm = document.getElementById('obligation-annotation-form');
  const obligationAnnotationOutput = document.getElementById('obligation-annotation-output');
  const obligationAnnotationRunId = document.getElementById('obligation-run-id');
  const obligationGateForm = document.getElementById('obligation-gate-form');
  const obligationGateOutput = document.getElementById('obligation-gate-output');
  const obligationGateRunId = document.getElementById('obligation-gate-run-id');
  const runEvidenceExportSummaryButton = document.getElementById('run-evidence-export-summary-button');
  const runEvidenceChangesButton = document.getElementById('run-evidence-changes-button');
  const runEvidenceExportChangesButton = document.getElementById('run-evidence-export-changes-button');
  const runEvidencePublishForm = document.getElementById('run-evidence-publish-form');
  const runEvidenceDetailForm = document.getElementById('run-evidence-detail-form');
  const runEvidenceDashboardForm = document.getElementById('run-evidence-dashboard-form');
  const runEvidenceOpenPublishedDetailButton = document.getElementById('run-evidence-open-published-detail-button');
  const runEvidenceDiffLeft = document.getElementById('run-evidence-diff-left');
  const runEvidenceDiffRight = document.getElementById('run-evidence-diff-right');
  const runEvidenceDiffButton = document.getElementById('run-evidence-diff-button');
  const runEvidenceExportDiffButton = document.getElementById('run-evidence-export-diff-button');
  const runEvidenceDiffOutput = document.getElementById('run-evidence-diff-output');
  const runEvidenceExportOutput = document.getElementById('run-evidence-export-output');
  const memorySearchForm = document.getElementById('memory-search-form');
  const memorySearchOutput = document.getElementById('memory-search-output');
  const memoryRecentButton = document.getElementById('memory-recent-button');
  const memoryLinkRunForm = document.getElementById('memory-link-run-form');
  const memoryExportForm = document.getElementById('memory-export-form');
  const memoryPublishLatestForm = document.getElementById('memory-publish-latest-form');
  const memoryControlPlaneDashboardForm = document.getElementById('memory-control-plane-dashboard-form');
  const form = document.getElementById('launch-form');
  const output = document.getElementById('launch-output');
  const repairResumeForm = document.getElementById('repair-resume-form');
  const repairResumeOutput = document.getElementById('repair-resume-output');
  const providerSelect = form ? form.querySelector('select[name="provider"]') : null;
  const providerWarningOutput = document.getElementById('provider-warning-output');
  const providerReadinessNode = document.getElementById('provider-readiness-data');
  const contractVerificationOutput = document.getElementById('contract-verification-output');
  const releaseHealthRefresh = document.getElementById('release-health-refresh');
  const releaseHealthRelease = document.getElementById('release-health-release');
  const releaseHealthAssetDir = document.getElementById('release-health-asset-dir');
  const releaseHealthProvenanceDir = document.getElementById('release-health-provenance-dir');
  const releaseHealthOutput = document.getElementById('release-health-output');
  const releaseRollbackHealth = document.getElementById('release-rollback-health');
  const releaseHistoryDir = document.getElementById('release-history-dir');
  const releaseHistoryRefresh = document.getElementById('release-history-refresh');
  const releaseHistoryExport = document.getElementById('release-history-export');
  const releaseHistoryOutput = document.getElementById('release-history-output');
  const releaseComparisonOutDir = document.getElementById('release-comparison-out-dir');
  const releaseComparisonBundleDir = document.getElementById('release-comparison-bundle-dir');
  const releaseComparisonLatest = document.getElementById('release-comparison-latest');
  const releaseComparisonGenerate = document.getElementById('release-comparison-generate');
  const releaseComparisonVerify = document.getElementById('release-comparison-verify');
  const releaseComparisonExport = document.getElementById('release-comparison-export');
  const releaseComparisonOutput = document.getElementById('release-comparison-output');
  const releaseSummaryEnrichForm = document.getElementById('release-summary-enrich-form');
  const releaseGateForm = document.getElementById('release-gate-form');
  const releaseGateOutput = document.getElementById('release-gate-output');
  const releaseGateArtifactForm = document.getElementById('release-gate-artifact-form');
  const releaseGateArtifactPath = document.getElementById('release-gate-artifact-path');
  const releaseGateArtifactOutput = document.getElementById('release-gate-artifact-output');
  const releaseRetentionKeepReleases = document.getElementById('release-retention-keep-releases');
  const releaseRetentionKeepBundles = document.getElementById('release-retention-keep-bundles');
  const releaseRetentionPreview = document.getElementById('release-retention-preview');
  const releaseRetentionPrune = document.getElementById('release-retention-prune');
  const releaseRetentionOutput = document.getElementById('release-retention-output');
  const queueOutput = document.getElementById('queue-output');
  const queueLogOutput = document.getElementById('queue-log-output');
  const queueDetailOutput = document.getElementById('queue-detail-output');
  const queueStatusFilter = document.getElementById('queue-status-filter');
  const queueTemplateFilter = document.getElementById('queue-template-filter');
  const queueAuditOutput = document.getElementById('queue-audit-output');
  const queueAuditActionFilter = document.getElementById('queue-audit-action-filter');
  const queueAuditRefresh = document.getElementById('queue-audit-refresh');
  const queueExportPreviewButton = document.getElementById('queue-export-preview-button');
  const queueExportButton = document.getElementById('queue-export-button');
  const projectStartNextActionForm = document.getElementById('project-start-next-action-form');
  const projectStartNextActionOutput = document.getElementById('project-start-next-action-output');
  const projectStartOperatorRecordForm = document.getElementById('project-start-operator-record-form');
  const projectStartOperatorRecordOutput = document.getElementById('project-start-operator-record-output');
  const projectStartHermesFlowContractForm = document.getElementById('project-start-hermes-flow-contract-form');
  const projectStartHermesFlowContractOutput = document.getElementById('project-start-hermes-flow-contract-output');
  const supportPacketOutput = document.getElementById('support-packet-output');
  const providerSmokeButton = document.getElementById('provider-smoke-button');
  const providerSmokeMinimumScore = document.getElementById('provider-smoke-minimum-score');
  const providerSmokeLiveProvider = document.getElementById('provider-smoke-live-provider');
  const providerSmokeOutput = document.getElementById('provider-smoke-output');
  const providerPilotPreflightButton = document.getElementById('provider-pilot-preflight-button');
  const providerPilotButton = document.getElementById('provider-pilot-button');
  const providerPilotStartButton = document.getElementById('provider-pilot-start-button');
  const providerPilotProvider = document.getElementById('provider-pilot-provider');
  const providerPilotTemplate = document.getElementById('provider-pilot-template');
  const providerPilotRunId = document.getElementById('provider-pilot-run-id');
  const providerPilotPromptFile = document.getElementById('provider-pilot-prompt-file');
  const providerPilotMaxRepairAttempts = document.getElementById('provider-pilot-max-repair-attempts');
  const providerPilotMaxBudgetUsd = document.getElementById('provider-pilot-max-budget-usd');
  const providerPilotMinimumScore = document.getElementById('provider-pilot-minimum-score');
  const providerPilotAcceptanceBundle = document.getElementById('provider-pilot-acceptance-bundle');
  const providerPilotAcceptanceProvider = document.getElementById('provider-pilot-acceptance-provider');
  const providerPilotAcceptanceReplayStatus = document.getElementById('provider-pilot-acceptance-replay-status');
  const providerPilotAcceptanceMinScore = document.getElementById('provider-pilot-acceptance-min-score');
  const providerPilotAcceptanceSort = document.getElementById('provider-pilot-acceptance-sort');
  const providerPilotAcceptanceLimit = document.getElementById('provider-pilot-acceptance-limit');
  const providerPilotAcceptanceLatestButton = document.getElementById('provider-pilot-acceptance-latest-button');
  const providerPilotAcceptanceExportButton = document.getElementById('provider-pilot-acceptance-export-button');
  const providerPilotAcceptanceExportLatestButton = document.getElementById('provider-pilot-acceptance-export-latest-button');
  const providerPilotCostLedgerButton = document.getElementById('provider-pilot-cost-ledger-button');
  const providerPilotCostTrendButton = document.getElementById('provider-pilot-cost-trend-button');
  const providerPilotOutput = document.getElementById('provider-pilot-output');
  let providerReadiness = {providers: []};
  let activeLogJobId = '';
  let activeEvidenceSummaryRunId = '';
  try {
    providerReadiness = JSON.parse(providerReadinessNode ? providerReadinessNode.textContent : '{"providers":[]}');
  } catch (_error) {
    providerReadiness = {providers: []};
  }
  function escapeHtml(value) {
    return String(value || '').replace(/[&<>"']/g, (char) => ({
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      '"': '&quot;',
      "'": '&#39;'
    }[char]));
  }
  function fileLink(label, path) {
    if (!path) return '';
    const safePath = escapeHtml(path);
    return `<a href="file://${safePath}" target="_blank" rel="noreferrer">${label}</a>`;
  }
  function renderRunEvidenceSummary(json) {
    const replay = json.replay || {};
    const scorecard = json.scorecard || {};
    const providerSummaries = json.provider_summaries || [];
    const closures = json.closures || [];
    const obligation = json.obligation_ledger || {};
    const obligationSummary = obligation.summary || {};
    const obligationGates = json.obligation_gates || {};
    const scoreLine = scorecard.present
      ? `provider_score=${scorecard.score || 0} verdict=${scorecard.verdict || ''} summaries=${scorecard.provider_summary_count || 0}`
      : `provider_score=missing ${json.scorecard_error || ''}`;
    const obligationLine = obligation.present
      ? `obligation_ledger=${obligation.verdict || ''} pass=${obligationSummary.pass || 0} fail=${obligationSummary.fail || 0} unverified=${obligationSummary.unverified || 0} ${fileLink('Open Ledger', obligation.path)}`
      : 'obligation_ledger=not_emitted';
    const gateLines = (obligationGates.gates || []).slice(0, 5).map((gate) => {
      const summary = gate.summary || {};
      return `obligation_gates stage=${gate.stage || ''} status=${gate.status || ''} verdict=${gate.verdict || ''} pass=${summary.pass || 0} fail=${summary.fail || 0} unverified=${summary.unverified || 0} waived=${summary.waived || 0} ${fileLink('Open Gate', gate.path)}`;
    });
    const providerLines = providerSummaries.slice(0, 3).map((summary, index) => {
      const changedFiles = (summary.changed_files || []).join(', ');
      return `provider_summary_${index + 1}=${summary.raw_summary || summary.summary || ''}${changedFiles ? ` changed_files=${changedFiles}` : ''}`;
    });
    const closureLines = closures.slice(0, 5).map((closure, index) => {
      return `closure_${index + 1}=${closure.role || ''} verdict=${closure.verdict || ''}`;
    });
    return [
      `run_id=${json.run_id || ''}`,
      `status=${json.status || ''} verdict=${json.verdict || ''}`,
      `workflow=${json.workflow_id || ''}`,
      `objective=${json.objective || ''}`,
      `replay=${replay.status || ''} events=${replay.event_count || 0} artifacts=${replay.artifact_count || 0} digest_failures=${replay.digest_failures || 0}`,
      scoreLine,
      obligationLine,
      ...gateLines,
      ...providerLines,
      ...closureLines,
      `${fileLink('Open Cockpit', json.cockpit)} ${fileLink('Open Evidence', json.evidence_pack)}`
    ].filter(Boolean).join('\n');
  }
  function renderRunEvidenceDiff(json) {
    const comparison = json.comparison || {};
    const left = json.left || {};
    const right = json.right || {};
    const scoreDelta = comparison.score_delta === null || comparison.score_delta === undefined ? 'n/a' : comparison.score_delta;
    return [
      `${left.run_id || ''} -> ${right.run_id || ''}`,
      `status_changed=${comparison.status_changed ? 'true' : 'false'} verdict_changed=${comparison.verdict_changed ? 'true' : 'false'}`,
      `digest_failure_delta=${comparison.digest_failure_delta || 0}`,
      `provider_summary_delta=${comparison.provider_summary_delta || 0}`,
      `score_delta=${scoreDelta}`,
      `closure_verdicts_changed=${comparison.closure_verdicts_changed ? 'true' : 'false'}`,
      `left status=${left.status || ''} verdict=${left.verdict || ''} score=${left.provider_score_present ? left.provider_score : 'missing'} summaries=${left.provider_summary_count || 0}`,
      `right status=${right.status || ''} verdict=${right.verdict || ''} score=${right.provider_score_present ? right.provider_score : 'missing'} summaries=${right.provider_summary_count || 0}`,
      `left ${fileLink('Cockpit', left.cockpit)} ${fileLink('Evidence', left.evidence_pack)}`,
      `right ${fileLink('Cockpit', right.cockpit)} ${fileLink('Evidence', right.evidence_pack)}`
    ].filter(Boolean).join('\n');
  }
  function renderRunEvidenceChanges(json) {
    const selected = json.selected || {};
    const baseline = json.baseline || {};
    return [
      `baseline_run_id=${baseline.run_id || ''}`,
      `selected_run_id=${selected.run_id || ''}`,
      renderRunEvidenceDiff(json.diff || {})
    ].filter(Boolean).join('\n');
  }
  function renderProjectStartNextAction(json) {
    const statusProbe = json.status_probe || {};
    const trustBoundary = json.trust_boundary || {};
    const hermesContract = json.hermes_contract || {};
    const blockers = (statusProbe.blocker_codes || []).join(',');
    return [
      `schema=${json.schema_version || ''}`,
      `run_id=${json.run_id || ''}`,
      `status=${json.status || ''}`,
      `next_action=${json.next_action || ''}`,
      `completion_record_state=${statusProbe.completion_record_state || ''}`,
      `ready_for_operator_review=${statusProbe.ready_for_operator_review === true ? 'true' : 'false'}`,
      `blocker_codes=${blockers}`,
      `read_only=${json.read_only === true ? 'true' : 'false'}`,
      `would_execute_queue=${json.would_execute_queue === true ? 'true' : 'false'}`,
      `would_submit_queue_entry=${json.would_submit_queue_entry === true ? 'true' : 'false'}`,
      `would_rebuild_wrappers=${json.would_rebuild_wrappers === true ? 'true' : 'false'}`,
      `release_acceptance_owner=${trustBoundary.release_acceptance_owner || ''}`,
      `control_plane_approves_release=${trustBoundary.control_plane_approves_release === true ? 'true' : 'false'}`,
      `mutates_ao_artifacts=${trustBoundary.mutates_ao_artifacts === true ? 'true' : 'false'}`,
      `front_end_must_not_scrape_raw_queue_json=${hermesContract.front_end_must_not_scrape_raw_queue_json === true ? 'true' : 'false'}`
    ].join('\n');
  }
  function providerWarnings(providerName) {
    const provider = (providerReadiness.providers || []).find((entry) => entry.provider === providerName);
    if (!provider) return [`provider_unknown=${providerName}`];
    const warnings = [];
    const doctor = provider.doctor || {};
    if (!doctor.available) warnings.push(`provider_unavailable=${doctor.blocker || 'unknown'}`);
    warnings.push(`timeout_seconds=${provider.timeout_seconds || 0}`);
    warnings.push(`execution_boundary=${provider.execution_boundary || ''}`);
    (provider.policy_invariants || []).forEach((invariant) => {
      if (invariant === 'provider cannot write target repo directly') warnings.push(invariant);
    });
    return warnings;
  }
  function renderProviderWarnings() {
    if (!providerWarningOutput || !providerSelect) return;
    const warnings = providerWarnings(providerSelect.value);
    providerWarningOutput.innerHTML = `<ul>${warnings.map((warning) => `<li>${escapeHtml(warning)}</li>`).join('')}</ul>`;
  }
  function renderMemorySearch(json) {
    const records = json.matches || json.records || [];
    if (!records.length) return 'memory_matches=0';
    return records.map((record) => [
      `id=${record.id || ''}`,
      `kind=${record.kind || ''}`,
      `title=${record.title || ''}`,
      `tags=${(record.tags || []).join(',')}`,
      `source_run_id=${(record.source || {}).run_id || ''}`
    ].join('\n')).join('\n\n');
  }
  async function searchMemory(event) {
    event.preventDefault();
    if (!memorySearchForm || !memorySearchOutput || !token) return;
    const formData = new FormData(memorySearchForm);
    const params = new URLSearchParams(formData);
    params.set('token', token);
    const response = await fetch(`/api/memory/search?${params.toString()}`);
    const json = await response.json();
    memorySearchOutput.textContent = response.ok ? renderMemorySearch(json) : (json.error || JSON.stringify(json, null, 2));
  }
  async function loadRecentMemory() {
    if (!memorySearchForm || !memorySearchOutput || !token) return;
    const formData = new FormData(memorySearchForm);
    const limit = formData.get('limit') || '10';
    const params = new URLSearchParams({token, limit});
    const response = await fetch(`/api/memory/recent?${params.toString()}`);
    const json = await response.json();
    memorySearchOutput.textContent = response.ok ? renderMemorySearch(json) : (json.error || JSON.stringify(json, null, 2));
  }
  async function linkMemoryRun(event) {
    event.preventDefault();
    if (!memoryLinkRunForm || !memorySearchOutput || !token || !canOperate) return;
    const response = await fetch(`/api/memory/link-run?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body: new URLSearchParams(new FormData(memoryLinkRunForm))
    });
    const json = await response.json();
    memorySearchOutput.textContent = response.ok
      ? [`memory_id=${json.memory_id || ''}`, `run_id=${json.run_id || ''}`, `relationship=${json.relationship || ''}`].join('\n')
      : (json.error || JSON.stringify(json, null, 2));
  }
  async function exportMemory(event) {
    event.preventDefault();
    if (!memoryExportForm || !memorySearchOutput || !token || !canOperate) return;
    const response = await fetch(`/api/memory/export?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body: new URLSearchParams(new FormData(memoryExportForm))
    });
    const json = await response.json();
    memorySearchOutput.textContent = response.ok
      ? [`memory_export=${json.export_path || ''}`, `sha256=${json.sha256 || ''}`, `records=${json.record_count || 0}`, `links=${json.link_count || 0}`].join('\n')
      : (json.error || JSON.stringify(json, null, 2));
  }
  async function publishLatestMemory(event) {
    event.preventDefault();
    if (!memoryPublishLatestForm || !memorySearchOutput || !token || !canOperate) return;
    const response = await fetch(`/api/memory/publish-latest?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body: new URLSearchParams(new FormData(memoryPublishLatestForm))
    });
    const json = await response.json();
    memorySearchOutput.textContent = response.ok
      ? [`memory_publish=${json.endpoint || ''}`, `signed=${json.signed ? 'true' : 'false'}`, `sha256=${(json.receipt || {}).sha256 || ''}`].join('\n')
      : (json.error || JSON.stringify(json, null, 2));
  }
  async function openMemoryControlPlaneDashboard(event) {
    event.preventDefault();
    if (!memoryControlPlaneDashboardForm || !memorySearchOutput || !token || !canOperate) return;
    const response = await fetch(`/api/memory/control-plane-dashboard?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body: new URLSearchParams(new FormData(memoryControlPlaneDashboardForm))
    });
    const json = await response.json();
    if (!response.ok) {
      memorySearchOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    const opened = window.open('', '_blank');
    if (opened) {
      opened.document.open();
      opened.document.write(json.dashboard_html || '');
      opened.document.close();
      memorySearchOutput.textContent = `dashboard=${json.endpoint || ''}`;
    } else {
      memorySearchOutput.textContent = `dashboard_popup_blocked=${json.endpoint || ''}`;
    }
  }
  function renderContractVerification(json) {
    const reasons = (json.reasons || []).map((reason) => `${reason.provider || ''}:${reason.code || ''} ${reason.message || ''}`);
    return [
      `schema=${json.schema || ''}`,
      `status=${json.status || ''}`,
      `required=${(json.required_providers || []).join(', ')}`,
      reasons.length ? `reasons=${reasons.join('; ')}` : 'reasons=none'
    ].join('\n');
  }
  async function refreshContractVerification() {
    if (!contractVerificationOutput || !token) return;
    const response = await fetch(`/api/provider-contracts?token=${encodeURIComponent(token)}`);
    const json = await response.json();
    contractVerificationOutput.textContent = response.ok ? renderContractVerification(json) : (json.error || JSON.stringify(json, null, 2));
  }
  function renderReleaseHealth(json) {
    const release = json.release || {};
    const install = json.install || {};
    const rollback = release.rollback || {};
    return [
      `status=${json.status || ''}`,
      `release=${release.release_tag || ''}`,
      `asset_source=${release.asset_source || ''}`,
      `assets_available=${release.assets_available ? 'true' : 'false'}`,
      `asset_count=${release.asset_count || 0}`,
      `provenance_verified=${release.provenance_verified ? 'true' : 'false'}`,
      `provenance_tag_matches=${release.provenance_tag_matches ? 'true' : 'false'}`,
      `rollback_status=${rollback.status || 'unavailable'} checked=${rollback.checked ? 'true' : 'false'}`,
      `installed=${install.installed ? 'true' : 'false'} on_path=${install.on_path ? 'true' : 'false'}`,
      release.missing_assets && release.missing_assets.length ? `missing_assets=${release.missing_assets.join(', ')}` : ''
    ].filter(Boolean).join('\n');
  }
  function renderReleaseRollbackHealth(json) {
    const rollback = (json.release || {}).rollback || {};
    if (!rollback.checked) {
      const status = rollback.status || 'missing';
      return `<div class="release-rollback-card"><div class="label">Rollback</div><div class="status warn">rollback_status=${escapeHtml(status)}</div><p class="muted">No downloaded rollback summary was found for this release directory.</p></div>`;
    }
    const platforms = rollback.platforms || {};
    const order = ['macos-aarch64', 'linux-x86_64', 'windows-x86_64'];
    return order.map((platform) => {
      const item = platforms[platform] || {};
      const status = item.status || 'missing';
      const css = status === 'passed' ? 'ok' : 'warn';
      const marker = item.marker || '';
      const log = item.log || '';
      return `<div class="release-rollback-card"><div class="label">${escapeHtml(platform)}</div><div class="status ${css}">rollback_status=${escapeHtml(status)}</div><div>${escapeHtml(marker)}</div><div>${fileLink('Open Evidence', log)}</div></div>`;
    }).join('');
  }
  async function refreshReleaseHealth() {
    if (!releaseHealthOutput || !token) return;
    releaseHealthOutput.textContent = 'Loading release health...';
    if (releaseRollbackHealth) releaseRollbackHealth.innerHTML = '';
    const params = new URLSearchParams();
    params.set('token', token);
    if (releaseHealthRelease && releaseHealthRelease.value) params.set('release', releaseHealthRelease.value);
    if (releaseHealthAssetDir && releaseHealthAssetDir.value) params.set('release_asset_dir', releaseHealthAssetDir.value);
    if (releaseHealthProvenanceDir && releaseHealthProvenanceDir.value) params.set('provenance_dir', releaseHealthProvenanceDir.value);
    const response = await fetch(`/api/release-health?${params.toString()}`);
    const json = await response.json();
    releaseHealthOutput.textContent = response.ok ? renderReleaseHealth(json) : (json.error || JSON.stringify(json, null, 2));
    if (releaseRollbackHealth) {
      releaseRollbackHealth.innerHTML = response.ok ? renderReleaseRollbackHealth(json) : '';
    }
  }
  function renderReleaseHistory(json) {
    const entries = json.entries || [];
    if (!entries.length) {
      return '<p class="muted">No downloaded release directories were found.</p>';
    }
    const trend = json.trend || {};
    const trendSummary = `<div class="metrics">
  <div class="metric"><div class="label">Latest</div><div class="value">${escapeHtml(trend.latest_release_tag || '')}</div></div>
  <div class="metric"><div class="label">Health</div><div class="value">${escapeHtml(trend.latest_health_score || 0)}/${escapeHtml(trend.max_health_score || 0)}</div></div>
  <div class="metric"><div class="label">Attention</div><div class="value">${escapeHtml(trend.attention_count || 0)}</div></div>
  <div class="metric"><div class="label">Regressions</div><div class="value">${escapeHtml(trend.regression_count || 0)}</div></div>
</div>`;
    const rows = entries.map((entry) => {
      const statusClass = entry.status === 'ok' && entry.rollback_status === 'verified' && entry.trend_status !== 'regressed' ? 'ok' : 'warn';
      const platforms = entry.platforms || {};
      const platformLine = [
        `mac=${platforms['macos-aarch64'] || 'missing'}`,
        `ubuntu=${platforms['linux-x86_64'] || 'missing'}`,
        `windows=${platforms['windows-x86_64'] || 'missing'}`
      ].join(' ');
      const changed = (entry.changed_fields || []).join(', ') || 'none';
      return `<tr><td><code>${escapeHtml(entry.release_tag || '')}</code></td><td class="${statusClass}">${escapeHtml(entry.trend_status || '')}</td><td>${escapeHtml(entry.health_score || 0)}/${escapeHtml(entry.max_health_score || 0)}</td><td>${escapeHtml(entry.status || '')}</td><td>${entry.assets_available ? 'true' : 'false'} (${entry.asset_count || 0})</td><td>${entry.provenance_verified ? 'true' : 'false'}</td><td class="${statusClass}">${escapeHtml(entry.rollback_status || '')}</td><td>${escapeHtml(platformLine)}</td><td>${escapeHtml(changed)}</td><td>${fileLink('Doctor', entry.doctor_json)} ${fileLink('Rollback', entry.rollback_summary_json)}</td></tr>`;
    }).join('');
    return `${trendSummary}<table class="release-history-table"><thead><tr><th>Release</th><th>Trend</th><th>Score</th><th>Status</th><th>Assets</th><th>Provenance</th><th>Rollback</th><th>Platforms</th><th>Changed</th><th>Evidence</th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  async function refreshReleaseHistory() {
    if (!releaseHistoryOutput || !token) return;
    releaseHistoryOutput.textContent = 'Loading release history...';
    const params = new URLSearchParams();
    params.set('token', token);
    if (releaseHistoryDir && releaseHistoryDir.value) params.set('release_download_dir', releaseHistoryDir.value);
    const response = await fetch(`/api/release-history?${params.toString()}`);
    const json = await response.json();
    releaseHistoryOutput.innerHTML = response.ok ? renderReleaseHistory(json) : escapeHtml(json.error || JSON.stringify(json, null, 2));
  }
  async function exportReleaseHistory() {
    if (!releaseHistoryOutput || !token) return;
    releaseHistoryOutput.textContent = 'Exporting release history...';
    const body = new URLSearchParams({kind: 'release-history'});
    if (releaseHistoryDir && releaseHistoryDir.value) body.set('release_download_dir', releaseHistoryDir.value);
    const response = await fetch(`/api/runs/evidence/export?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body
    });
    const json = await response.json();
    if (!response.ok) {
      releaseHistoryOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    await refreshSupportPacket();
    releaseHistoryOutput.innerHTML = [
      `<p>Release history export written: ${fileLink('Open Export', json.export_path)} <code>${escapeHtml(json.export_path || '')}</code></p>`,
      renderReleaseHistory(json.export ? json.export.release_history : {})
    ].join('');
  }
  function renderReleaseComparisonResult(json) {
    const comparison = json.release_comparison || {};
    const verification = json.verification || {};
    const trend = (comparison.release_history || {}).trend || {};
    return [
      `schema=${escapeHtml(json.schema_version || '')}`,
      `bundle_dir=${escapeHtml(comparison.bundle_dir || '')}`,
      `latest_release=${escapeHtml(trend.latest_release_tag || '')}`,
      `health=${escapeHtml(trend.latest_health_score || 0)}/${escapeHtml(trend.max_health_score || 0)}`,
      `regressions=${escapeHtml(trend.regression_count || 0)}`,
      `signed=${((comparison.support_metadata || {}).signature_verified) ? 'true' : 'false'}`,
      `verification=${escapeHtml(verification.status || '')}`,
      `manifest_verified=${verification.manifest_verified ? 'true' : 'false'}`,
      `signature_verified=${verification.signature_verified ? 'true' : 'false'}`,
      `${fileLink('Open Bundle', comparison.bundle_path)} ${fileLink('Open SHA256SUMS', comparison.sha256_manifest)}`
    ].filter(Boolean).join('\n');
  }
  function renderReleaseComparisonVerification(json) {
    const verification = json.verification || {};
    const reasons = (verification.reasons || []).map((reason) => `${reason.code || ''}: ${reason.message || ''}`);
    return [
      `schema=${json.schema_version || ''}`,
      `status=${verification.status || ''}`,
      `bundle_dir=${json.bundle_dir || verification.bundle_dir || ''}`,
      `latest_release=${verification.latest_release_tag || ''}`,
      `release_count=${verification.release_count || 0}`,
      `regressions=${verification.regression_count || 0}`,
      `manifest_verified=${verification.manifest_verified ? 'true' : 'false'}`,
      `signature_verified=${verification.signature_verified ? 'true' : 'false'}`,
      `signer=${verification.signer_id || ''}`,
      reasons.length ? `reasons=${reasons.join('; ')}` : 'reasons=none'
    ].join('\n');
  }
  async function generateReleaseComparison() {
    if (!releaseComparisonOutput || !token) return;
    if (!canOperate) {
      releaseComparisonOutput.textContent = 'Operator token required.';
      return;
    }
    if (!releaseComparisonSigningEnabled) {
      releaseComparisonOutput.textContent = 'Start Workbench with --support-signing-key to generate signed release comparison bundles.';
      return;
    }
    releaseComparisonOutput.textContent = 'Generating signed release comparison bundle...';
    const body = new URLSearchParams();
    if (releaseHistoryDir && releaseHistoryDir.value) body.set('release_download_dir', releaseHistoryDir.value);
    if (releaseComparisonOutDir && releaseComparisonOutDir.value) body.set('out_dir', releaseComparisonOutDir.value);
    const response = await fetch(`/api/release-comparison?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body
    });
    const json = await response.json();
    if (!response.ok) {
      releaseComparisonOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    const bundleDir = (json.release_comparison || {}).bundle_dir || '';
    if (releaseComparisonBundleDir) releaseComparisonBundleDir.value = bundleDir;
    releaseComparisonOutput.innerHTML = renderReleaseComparisonResult(json);
  }
  async function verifyReleaseComparison() {
    if (!releaseComparisonOutput || !token) return;
    releaseComparisonOutput.textContent = 'Verifying release comparison bundle...';
    const bundleDir = releaseComparisonBundleDir ? releaseComparisonBundleDir.value : '';
    const params = new URLSearchParams();
    params.set('token', token);
    if (bundleDir) params.set('bundle_dir', bundleDir);
    const response = await fetch(`/api/release-comparison/verify?${params.toString()}`);
    const json = await response.json();
    releaseComparisonOutput.textContent = response.ok ? renderReleaseComparisonVerification(json) : (json.error || renderReleaseComparisonVerification(json));
  }
  async function loadLatestReleaseComparison() {
    if (!releaseComparisonOutput || !token) return;
    releaseComparisonOutput.textContent = 'Loading latest verified release comparison bundle...';
    const params = new URLSearchParams();
    params.set('token', token);
    if (releaseComparisonOutDir && releaseComparisonOutDir.value) params.set('bundle_root', releaseComparisonOutDir.value);
    const response = await fetch(`/api/release-comparison/latest?${params.toString()}`);
    const json = await response.json();
    if (!response.ok) {
      releaseComparisonOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    if (releaseComparisonBundleDir) releaseComparisonBundleDir.value = json.bundle_dir || '';
    releaseComparisonOutput.textContent = renderReleaseComparisonVerification(json);
  }
  async function exportReleaseComparisonVerification() {
    if (!releaseComparisonOutput || !token) return;
    if (!canOperate) {
      releaseComparisonOutput.textContent = 'Operator token required.';
      return;
    }
    releaseComparisonOutput.textContent = 'Exporting release comparison verification evidence...';
    const bundleDir = releaseComparisonBundleDir ? releaseComparisonBundleDir.value : '';
    const body = new URLSearchParams({kind: 'release-comparison-verification'});
    if (bundleDir) body.set('bundle_dir', bundleDir);
    const response = await fetch(`/api/runs/evidence/export?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body
    });
    const json = await response.json();
    if (!response.ok) {
      releaseComparisonOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    await refreshSupportPacket();
    const verification = json.export ? json.export.release_comparison_verification : {};
    releaseComparisonOutput.innerHTML = [
      `<p>Release comparison verification export written: ${fileLink('Open Export', json.export_path)} <code>${escapeHtml(json.export_path || '')}</code></p>`,
      `<pre>${escapeHtml(renderReleaseComparisonVerification({schema_version: json.schema_version, bundle_dir: bundleDir, verification}))}</pre>`
    ].join('');
  }
  function renderReleaseGate(json) {
    const release = json.release || {};
    const smoke = json.smoke || {};
    const obligations = json.obligation_gates || {};
    const reasons = (json.reasons || []).map((reason) => `${reason.code || ''}: ${reason.message || ''}`);
    return [
      `schema=${json.schema || ''}`,
      `status=${json.status || ''}`,
      `provenance_verified=${release.provenance_verified ? 'true' : 'false'}`,
      `archive_count=${release.archive_count || 0}`,
      `smoke=${smoke.status || ''}`,
      `obligation_gates=${obligations.status || ''}`,
      reasons.length ? `reasons=${reasons.join('; ')}` : 'reasons=none'
    ].join('\n');
  }
  async function enrichReleaseSummary(event) {
    event.preventDefault();
    if (!releaseGateOutput || !releaseSummaryEnrichForm || !token) return;
    if (!canOperate) {
      releaseGateOutput.textContent = 'Operator token required.';
      return;
    }
    releaseGateOutput.textContent = 'Enriching release summary...';
    const response = await fetch(`/api/release-summary/enrich?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body: new URLSearchParams(new FormData(releaseSummaryEnrichForm))
    });
    const json = await response.json();
    if (!response.ok) {
      releaseGateOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    const out = json.out || '';
    if (releaseGateForm && out) {
      const summaryInput = releaseGateForm.querySelector('input[name="summary"]');
      if (summaryInput) summaryInput.value = out;
    }
    releaseGateOutput.textContent = [
      `schema=${json.schema || ''}`,
      `status=${json.status || ''}`,
      `run_id=${json.run_id || ''}`,
      `out=${out}`,
      `obligation_gate_count=${(json.obligation_gates || {}).count || 0}`
    ].join('\n');
  }
  async function runReleaseGate(event) {
    event.preventDefault();
    if (!releaseGateOutput || !releaseGateForm || !token) return;
    if (!canOperate) {
      releaseGateOutput.textContent = 'Operator token required.';
      return;
    }
    releaseGateOutput.textContent = 'Running release gate...';
    const response = await fetch(`/api/release-gate?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body: new URLSearchParams(new FormData(releaseGateForm))
    });
    const json = await response.json();
    if (response.ok && json.artifact_path && releaseGateArtifactPath) {
      releaseGateArtifactPath.value = json.artifact_path;
    }
    releaseGateOutput.textContent = response.ok ? renderReleaseGate(json) : (json.error || renderReleaseGate(json));
  }
  async function loadReleaseGateArtifact(event) {
    event.preventDefault();
    if (!releaseGateArtifactOutput || !releaseGateArtifactForm || !token) return;
    const formData = new FormData(releaseGateArtifactForm);
    const params = new URLSearchParams({token});
    const path = String(formData.get('path') || '').trim();
    if (path) params.set('path', path);
    releaseGateArtifactOutput.textContent = 'Loading release gate artifact...';
    const response = await fetch(`/api/release-gate/artifact?${params.toString()}`);
    const json = await response.json();
    if (!response.ok) {
      releaseGateArtifactOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    const artifact = json.artifact || {};
    releaseGateArtifactOutput.textContent = [
      `schema=${json.schema || ''}`,
      `path=${json.path || ''}`,
      `artifact_schema=${artifact.schema || artifact.schema_version || ''}`,
      `status=${artifact.status || ''}`,
      artifact.enriched ? `enriched=${artifact.enriched.status || ''}` : '',
      artifact.malformed ? `malformed=${artifact.malformed.status || ''}` : ''
    ].filter(Boolean).join('\n');
  }
  function renderReleaseRetention(json) {
    const releaseRemoved = (json.removed_release_dirs || []).map((path) => `release=${path}`);
    const bundleRemoved = (json.removed_bundle_dirs || []).map((path) => `bundle=${path}`);
    return [
      `schema=${json.schema_version || ''}`,
      `dry_run=${json.dry_run ? 'true' : 'false'}`,
      `keep_releases=${json.keep_releases || 0} kept=${json.kept_release_count || 0} remove=${json.removed_release_count || 0}`,
      `keep_bundles=${json.keep_bundles || 0} kept=${json.kept_bundle_count || 0} remove=${json.removed_bundle_count || 0}`,
      `total_removed=${json.total_removed_count || 0}`,
      ...releaseRemoved,
      ...bundleRemoved
    ].join('\n');
  }
  async function pruneReleaseRetention(dryRun) {
    if (!releaseRetentionOutput || !token) return;
    if (!canOperate) {
      releaseRetentionOutput.textContent = 'Operator token required.';
      return;
    }
    releaseRetentionOutput.textContent = dryRun ? 'Previewing release evidence prune...' : 'Pruning old release evidence...';
    const body = new URLSearchParams();
    if (releaseHistoryDir && releaseHistoryDir.value) body.set('release_download_dir', releaseHistoryDir.value);
    if (releaseComparisonOutDir && releaseComparisonOutDir.value) body.set('bundle_root', releaseComparisonOutDir.value);
    if (releaseRetentionKeepReleases && releaseRetentionKeepReleases.value) body.set('keep_releases', releaseRetentionKeepReleases.value);
    if (releaseRetentionKeepBundles && releaseRetentionKeepBundles.value) body.set('keep_bundles', releaseRetentionKeepBundles.value);
    body.set('dry_run', dryRun ? '1' : '0');
    const response = await fetch(`/api/release-retention/prune?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body
    });
    const json = await response.json();
    releaseRetentionOutput.textContent = response.ok ? renderReleaseRetention(json) : (json.error || JSON.stringify(json, null, 2));
    if (response.ok && !dryRun) {
      await refreshReleaseHistory();
    }
  }
  function renderSupportTrust(metadata) {
    const present = metadata && metadata.present;
    if (!present) {
      return '<h2>Support Bundle Trust</h2><p>Status: <strong>Unsigned</strong></p>';
    }
    const status = metadata.signature_verified ? 'Signature verified' : 'Signature not verified';
    return `
<h2>Support Bundle Trust</h2>
<div class="metrics">
  <div class="metric"><div class="label">Status</div><div class="value">${escapeHtml(status)}</div></div>
  <div class="metric"><div class="label">Signer</div><div class="value">${escapeHtml(metadata.signer_id || '')}</div></div>
</div>
<p>Metadata SHA256: <code>${escapeHtml(metadata.metadata_sha256 || '')}</code></p>
<p>Public Key SHA256: <code>${escapeHtml(metadata.public_key_sha256 || '')}</code></p>`;
  }
  function renderRedactionAudit(audit) {
    const classes = audit && audit.secret_classes ? audit.secret_classes : {};
    const entries = Object.entries(classes);
    const rows = entries.length === 0
      ? '<tr><td colspan="2" class="muted">No redactions recorded.</td></tr>'
      : entries.map(([klass, count]) => `<tr><td><code>${escapeHtml(klass)}</code></td><td>${escapeHtml(count)}</td></tr>`).join('');
    return `<h2>Redaction Audit</h2>
<p>Total redactions: <strong>${escapeHtml((audit && audit.redaction_count) || 0)}</strong></p>
<table><thead><tr><th>Secret Class</th><th>Count</th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  function evidenceExportSubject(evidence) {
    if (!evidence) return 'unknown';
    if (evidence.run_id) return evidence.run_id;
    if (evidence.baseline_run_id || evidence.selected_run_id) {
      return `${evidence.baseline_run_id || ''}->${evidence.selected_run_id || ''}`;
    }
    if (evidence.left_run_id || evidence.right_run_id) {
      return `${evidence.left_run_id || ''}->${evidence.right_run_id || ''}`;
    }
    if (evidence.latest_release_tag) {
      return `${evidence.latest_release_tag} entries=${evidence.release_entry_count || 0}`;
    }
    if (evidence.release_comparison_latest_release_tag) {
      return `${evidence.release_comparison_latest_release_tag} releases=${evidence.release_comparison_release_count || 0} regressions=${evidence.release_comparison_regression_count || 0}`;
    }
    if (evidence.provider_pilot_run_id) {
      return `${evidence.provider_pilot_provider || ''} ${evidence.provider_pilot_run_id} score=${evidence.provider_pilot_score || 0} replay=${evidence.provider_pilot_replay_status || ''} digest_failures=${evidence.provider_pilot_digest_failure_count || 0}`;
    }
    return 'unknown';
  }
  function renderSupportPacket(packet) {
    if (!packet || !packet.present) {
      return `<section>
<h2>Latest Support Packet</h2>
<p class="muted">No signed support bundle has been exported for this repository yet.</p>
${renderSupportTrust(packet ? packet.support_metadata : null)}
</section>`;
    }
    const evidenceExports = packet.evidence_exports || [];
    const rows = evidenceExports.length === 0
      ? '<tr><td colspan="4" class="muted">No evidence exports are attached to the latest support bundle.</td></tr>'
      : evidenceExports.map((evidence) => `<tr>
<td><code>${escapeHtml(evidence.kind || '')}</code></td>
<td><code>${escapeHtml(evidenceExportSubject(evidence))}</code></td>
<td><code>${escapeHtml(evidence.sha256 || '')}</code></td>
<td><code>${escapeHtml(evidence.path || '')}</code></td>
</tr>`).join('');
    const queueDiagnoses = packet.queue_job_diagnoses || [];
    const diagnosisRows = queueDiagnoses.length === 0
      ? '<tr><td colspan="7" class="muted">No queue failure diagnostics.</td></tr>'
      : queueDiagnoses.map((diagnosis) => {
        const recovery = (diagnosis.recovery_actions || [])[0] || '';
        return `<tr>
<td><code>${escapeHtml(diagnosis.run_id || '')}</code></td>
<td>${escapeHtml(diagnosis.failure_kind || '')}</td>
<td>${escapeHtml(diagnosis.exit_code || 0)}</td>
<td>${escapeHtml(diagnosis.timed_out || false)}</td>
<td>${escapeHtml(diagnosis.primary_error || '')}</td>
<td>${escapeHtml(recovery)}</td>
<td><code>${escapeHtml(diagnosis.stderr_excerpt || '')}</code></td>
</tr>`;
      }).join('');
    return `<section>
<h2>Latest Support Packet</h2>
<div class="metrics">
  <div class="metric"><div class="label">Queue Jobs</div><div class="value">${escapeHtml(packet.queue_job_count || 0)}</div></div>
  <div class="metric"><div class="label">Audit Events</div><div class="value">${escapeHtml(packet.audit_event_count || 0)}</div></div>
  <div class="metric"><div class="label">Job Logs</div><div class="value">${escapeHtml(packet.job_log_count || 0)}</div></div>
  <div class="metric"><div class="label">Evidence Exports</div><div class="value">${escapeHtml(packet.evidence_export_count || 0)}</div></div>
  <div class="metric"><div class="label">Redactions</div><div class="value">${escapeHtml((packet.redaction_audit && packet.redaction_audit.redaction_count) || 0)}</div></div>
</div>
<p>${fileLink('Open Bundle', packet.bundle_path)} <code>${escapeHtml(packet.bundle_path || '')}</code></p>
<p>Bundle SHA256: <code>${escapeHtml(packet.bundle_sha256 || '')}</code></p>
${renderSupportTrust(packet.support_metadata)}
${renderSupportHermesFlowContract(packet.hermes_project_start_flow_contract)}
${renderRedactionAudit(packet.redaction_audit)}
<h2>Queue Failure Diagnostics</h2>
<table><thead><tr><th>Run</th><th>Failure</th><th>Exit</th><th>Timed Out</th><th>Primary Error</th><th>Recovery</th><th>Stderr</th></tr></thead><tbody>${diagnosisRows}</tbody></table>
<h2>Evidence Exports</h2>
<table><thead><tr><th>Kind</th><th>Subject</th><th>SHA256</th><th>Path</th></tr></thead><tbody>${rows}</tbody></table>
</section>`;
  }
  function renderSupportHermesFlowContract(contract) {
    if (!contract || !contract.present) {
      return '<h2>Hermes Project-Start Flow Contract</h2><p class="muted">No Hermes project-start flow contract is attached to this support packet.</p>';
    }
    return `<h2>Hermes Project-Start Flow Contract</h2>
<div class="metrics">
  <div class="metric"><div class="label">Schema</div><div class="value">${escapeHtml(contract.schema_version || '')}</div></div>
  <div class="metric"><div class="label">Preview Role</div><div class="value">${escapeHtml(contract.preview_role || '')}</div></div>
  <div class="metric"><div class="label">Publish Role</div><div class="value">${escapeHtml(contract.publish_role || '')}</div></div>
</div>
<p>Contract SHA256: <code>${escapeHtml(contract.contract_sha256 || '')}</code></p>
<p>Raw queue JSON scrape required: <code>${escapeHtml(String(contract.raw_queue_json_scrape_required === true))}</code></p>
<p>Release acceptance owner: <code>${escapeHtml(contract.release_acceptance_owner || '')}</code></p>
<p>Side effects: execute_queue=<code>${escapeHtml(String(contract.would_execute_queue === true))}</code>, submit_queue_entry=<code>${escapeHtml(String(contract.would_submit_queue_entry === true))}</code>, rebuild_wrappers=<code>${escapeHtml(String(contract.would_rebuild_wrappers === true))}</code>, mutate_control_plane=<code>${escapeHtml(String(contract.would_mutate_control_plane === true))}</code></p>`;
  }
  async function refreshSupportPacket() {
    if (!supportPacketOutput) return;
    const response = await fetch(`/api/support/latest?token=${encodeURIComponent(token)}`);
    const json = await response.json();
    if (!response.ok) {
      supportPacketOutput.innerHTML = `<section><h2>Latest Support Packet</h2><p class="warn">${escapeHtml(json.error || 'Unable to load latest support packet')}</p></section>`;
      return;
    }
    supportPacketOutput.innerHTML = renderSupportPacket(json);
  }
  function actionButton(label, action, job) {
    if (action === 'logs') {
      return `<button type="button" data-action="logs" data-job-id="${escapeHtml(job.job_id)}">${label}</button>`;
    }
    if (action === 'detail') {
      return `<button type="button" data-action="detail" data-job-id="${escapeHtml(job.job_id)}">${label}</button>`;
    }
    if (!canOperate) return '';
    if (action === 'cancel') {
      return `<button type="button" data-action="cancel" data-job-id="${escapeHtml(job.job_id)}">${label}</button>`;
    }
    return `<button type="button" data-action="retry" data-job-id="${escapeHtml(job.job_id)}">${label}</button>`;
  }
  renderProviderWarnings();
  refreshContractVerification();
  if (releaseHealthRefresh) releaseHealthRefresh.addEventListener('click', refreshReleaseHealth);
  if (releaseHistoryRefresh) releaseHistoryRefresh.addEventListener('click', refreshReleaseHistory);
  if (releaseHistoryExport && canOperate) releaseHistoryExport.addEventListener('click', exportReleaseHistory);
  if (releaseComparisonLatest) releaseComparisonLatest.addEventListener('click', loadLatestReleaseComparison);
  if (releaseComparisonGenerate) releaseComparisonGenerate.addEventListener('click', generateReleaseComparison);
  if (releaseComparisonVerify) releaseComparisonVerify.addEventListener('click', verifyReleaseComparison);
  if (releaseComparisonExport && canOperate) releaseComparisonExport.addEventListener('click', exportReleaseComparisonVerification);
  if (releaseSummaryEnrichForm) releaseSummaryEnrichForm.addEventListener('submit', enrichReleaseSummary);
  if (releaseGateForm) releaseGateForm.addEventListener('submit', runReleaseGate);
  if (releaseGateArtifactForm) releaseGateArtifactForm.addEventListener('submit', loadReleaseGateArtifact);
  if (releaseRetentionPreview) releaseRetentionPreview.addEventListener('click', () => pruneReleaseRetention(true));
  if (releaseRetentionPrune) releaseRetentionPrune.addEventListener('click', () => pruneReleaseRetention(false));
  if (providerSelect) providerSelect.addEventListener('change', renderProviderWarnings);
  if (memorySearchForm) memorySearchForm.addEventListener('submit', searchMemory);
  if (memoryRecentButton) memoryRecentButton.addEventListener('click', loadRecentMemory);
  if (memoryLinkRunForm && canOperate) memoryLinkRunForm.addEventListener('submit', linkMemoryRun);
  if (memoryExportForm && canOperate) memoryExportForm.addEventListener('submit', exportMemory);
  if (memoryPublishLatestForm && canOperate) memoryPublishLatestForm.addEventListener('submit', publishLatestMemory);
  if (memoryControlPlaneDashboardForm && canOperate) memoryControlPlaneDashboardForm.addEventListener('submit', openMemoryControlPlaneDashboard);
  if (!token) return;
  async function loadRunEvidenceSummary(runId) {
    activeEvidenceSummaryRunId = runId || '';
    if (!runEvidenceSummaryOutput || !activeEvidenceSummaryRunId) return null;
    if (obligationAnnotationRunId && !obligationAnnotationRunId.value) obligationAnnotationRunId.value = activeEvidenceSummaryRunId;
    if (obligationGateRunId && !obligationGateRunId.value) obligationGateRunId.value = activeEvidenceSummaryRunId;
    runEvidenceSummaryOutput.textContent = 'Loading run evidence summary...';
    const response = await fetch(`/api/runs/evidence?token=${encodeURIComponent(token)}&run_id=${encodeURIComponent(activeEvidenceSummaryRunId)}`);
    const json = await response.json();
    if (!response.ok) {
      runEvidenceSummaryOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return null;
    }
    runEvidenceSummaryOutput.innerHTML = renderRunEvidenceSummary(json);
    return json;
  }
  if (runsTable && runEvidenceSummaryOutput) {
    runsTable.addEventListener('click', async (event) => {
      const button = event.target.closest('button[data-action="evidence-summary"]');
      if (!button) return;
      await loadRunEvidenceSummary(button.dataset.runId || '');
    });
  }
  if (obligationAnnotationForm && obligationAnnotationOutput) {
    obligationAnnotationForm.addEventListener('submit', async (event) => {
      event.preventDefault();
      if (!canOperate) {
        obligationAnnotationOutput.textContent = 'Operator token required to annotate obligation evidence.';
        return;
      }
      const formData = new FormData(obligationAnnotationForm);
      if (!String(formData.get('run_id') || '').trim() && activeEvidenceSummaryRunId) {
        formData.set('run_id', activeEvidenceSummaryRunId);
      }
      obligationAnnotationOutput.textContent = 'Annotating obligation ledger...';
      const response = await fetch(`/api/obligations/annotate?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body: new URLSearchParams(formData)
      });
      const json = await response.json();
      if (!response.ok) {
        obligationAnnotationOutput.textContent = json.error || JSON.stringify(json, null, 2);
        return;
      }
      const summary = (json.ledger && json.ledger.summary) || {};
      obligationAnnotationOutput.innerHTML = [
        `obligation=${escapeHtml(json.obligation_id || '')}`,
        `verdict=${escapeHtml((json.ledger && json.ledger.verdict) || '')} pass=${summary.pass || 0} waived=${summary.waived || 0} unverified=${summary.unverified || 0}`,
        `${fileLink('Open Ledger', json.ledger_path)} ${escapeHtml(json.ledger_path || '')}`
      ].join('\n');
      if (activeEvidenceSummaryRunId && activeEvidenceSummaryRunId === (json.run_id || '')) {
        await loadRunEvidenceSummary(activeEvidenceSummaryRunId);
      }
    });
  }
  if (obligationGateForm && obligationGateOutput) {
    obligationGateForm.addEventListener('click', async (event) => {
      const button = event.target.closest('button[data-obligation-gate-stage]');
      if (!button) return;
      if (!canOperate) {
        obligationGateOutput.textContent = 'Operator token required to run obligation gates.';
        return;
      }
      const formData = new FormData(obligationGateForm);
      if (!String(formData.get('run_id') || '').trim() && activeEvidenceSummaryRunId) {
        formData.set('run_id', activeEvidenceSummaryRunId);
      }
      formData.set('stage', button.dataset.obligationGateStage || '');
      obligationGateOutput.textContent = `Running ${button.dataset.obligationGateStage || ''} obligation gate...`;
      const response = await fetch(`/api/obligations/gate?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body: new URLSearchParams(formData)
      });
      const json = await response.json();
      if (!response.ok) {
        obligationGateOutput.textContent = json.error || JSON.stringify(json, null, 2);
        return;
      }
      const summary = (json.gate && json.gate.summary) || {};
      obligationGateOutput.innerHTML = [
        `stage=${escapeHtml(json.stage || '')}`,
        `status=${escapeHtml((json.gate && json.gate.status) || '')} verdict=${escapeHtml((json.gate && json.gate.verdict) || '')}`,
        `pass=${summary.pass || 0} fail=${summary.fail || 0} unverified=${summary.unverified || 0} waived=${summary.waived || 0}`,
        `${fileLink('Open Gate', json.gate_path)} ${escapeHtml(json.gate_path || '')}`,
        `${fileLink('Open Evidence Export', (json.evidence_export || {}).export_path)}`
      ].join('\n');
      if (activeEvidenceSummaryRunId && activeEvidenceSummaryRunId === (json.run_id || '')) {
        await loadRunEvidenceSummary(activeEvidenceSummaryRunId);
      }
    });
  }
  async function exportEvidence(body) {
    if (!runEvidenceExportOutput) return;
    runEvidenceExportOutput.textContent = 'Exporting evidence...';
    const response = await fetch(`/api/runs/evidence/export?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body
    });
    const json = await response.json();
    if (!response.ok) {
      runEvidenceExportOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    runEvidenceExportOutput.innerHTML = [
      `export_kind=${json.export_kind || ''}`,
      `${fileLink('Open Export', json.export_path)} ${escapeHtml(json.export_path || '')}`
    ].filter(Boolean).join('\n');
  }
  if (runEvidenceExportSummaryButton && runEvidenceExportOutput && canOperate) {
    runEvidenceExportSummaryButton.addEventListener('click', async () => {
      const runId = activeEvidenceSummaryRunId || (runEvidenceDiffLeft ? runEvidenceDiffLeft.value : '');
      await exportEvidence(new URLSearchParams({kind: 'summary', run_id: runId}));
    });
  }
  if (runEvidencePublishForm && runEvidenceExportOutput && canOperate) {
    runEvidencePublishForm.addEventListener('submit', async (event) => {
      event.preventDefault();
      const formData = new FormData(runEvidencePublishForm);
      if (!String(formData.get('run_id') || '').trim() && activeEvidenceSummaryRunId) {
        formData.set('run_id', activeEvidenceSummaryRunId);
      }
      runEvidenceExportOutput.textContent = 'Publishing signed evidence pack...';
      const response = await fetch(`/api/runs/evidence/publish?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body: new URLSearchParams(formData)
      });
      const json = await response.json();
      if (!response.ok) {
        runEvidenceExportOutput.textContent = json.error || JSON.stringify(json, null, 2);
        return;
      }
      persistControlPlaneDefaults(formData);
      const publishedSha = (json.receipt || {}).sha256 || '';
      if (publishedSha && runEvidenceDetailForm) {
        const shaInput = runEvidenceDetailForm.querySelector('input[name="sha256"]');
        const controlPlaneUrlInput = runEvidenceDetailForm.querySelector('input[name="control_plane_url"]');
        const apiTokenInput = runEvidenceDetailForm.querySelector('input[name="api_token"]');
        if (shaInput) shaInput.value = publishedSha;
        if (controlPlaneUrlInput) controlPlaneUrlInput.value = String(formData.get('control_plane_url') || '');
        if (apiTokenInput) apiTokenInput.value = String(formData.get('api_token') || '');
        if (runEvidenceOpenPublishedDetailButton) runEvidenceOpenPublishedDetailButton.disabled = false;
      }
      runEvidenceExportOutput.textContent = [
        `evidence_publish=${json.endpoint || ''}`,
        `detail=${json.detail_url || ''}`,
        `sha256=${(json.receipt || {}).sha256 || ''}`,
        `signed=${json.signed ? 'true' : 'false'}`,
        json.detail_fetch_error ? `detail_fetch_error=${json.detail_fetch_error}` : ''
      ].filter(Boolean).join('\n');
      if (json.detail_html) {
        const opened = window.open('', '_blank');
        if (opened) {
          opened.document.open();
          opened.document.write(json.detail_html || '');
          opened.document.close();
        }
      }
    });
  }
  async function openPublishedEvidenceDetail(formData) {
    if (!runEvidenceExportOutput) return;
    runEvidenceExportOutput.textContent = 'Opening signed evidence detail...';
    const response = await fetch(`/api/runs/evidence/detail?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body: new URLSearchParams(formData)
    });
    const json = await response.json();
    if (!response.ok) {
      runEvidenceExportOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    persistControlPlaneDefaults(formData);
    const opened = window.open('', '_blank');
    if (opened) {
      opened.document.open();
      opened.document.write(json.detail_html || '');
      opened.document.close();
      runEvidenceExportOutput.textContent = `detail=${json.endpoint || ''}`;
    } else {
      runEvidenceExportOutput.textContent = `detail_popup_blocked=${json.endpoint || ''}`;
    }
  }
  if (runEvidenceDetailForm && runEvidenceExportOutput && canOperate) {
    runEvidenceDetailForm.addEventListener('submit', async (event) => {
      event.preventDefault();
      const formData = new FormData(runEvidenceDetailForm);
      await openPublishedEvidenceDetail(formData);
    });
  }
  if (runEvidenceOpenPublishedDetailButton && runEvidenceDetailForm && runEvidenceExportOutput && canOperate) {
    runEvidenceOpenPublishedDetailButton.addEventListener('click', async () => {
      await openPublishedEvidenceDetail(new FormData(runEvidenceDetailForm));
    });
  }
  if (runEvidenceDashboardForm && runEvidenceExportOutput && canOperate) {
    runEvidenceDashboardForm.addEventListener('submit', async (event) => {
      event.preventDefault();
      const formData = new FormData(runEvidenceDashboardForm);
      runEvidenceExportOutput.textContent = 'Opening signed evidence dashboard...';
      const response = await fetch(`/api/runs/evidence/dashboard?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body: new URLSearchParams(formData)
      });
      const json = await response.json();
      if (!response.ok) {
        runEvidenceExportOutput.textContent = json.error || JSON.stringify(json, null, 2);
        return;
      }
      persistControlPlaneDefaults(formData);
      const opened = window.open('', '_blank');
      if (opened) {
        opened.document.open();
        opened.document.write(json.dashboard_html || '');
        opened.document.close();
        runEvidenceExportOutput.textContent = `dashboard=${json.endpoint || ''}`;
      } else {
        runEvidenceExportOutput.textContent = `dashboard_popup_blocked=${json.endpoint || ''}`;
      }
    });
  }
  function loadControlPlaneDefaults() {
    const savedUrl = window.localStorage ? window.localStorage.getItem('ao2.controlPlaneUrl') : '';
    const defaultUrl = defaultControlPlaneUrl || savedUrl;
    if (!defaultUrl) return;
    document.querySelectorAll('input[name="control_plane_url"]').forEach((input) => {
      if (!input.value) input.value = defaultUrl;
    });
  }
  function persistControlPlaneDefaults(formData) {
    const controlPlaneUrl = String(formData.get('control_plane_url') || '').trim();
    if (controlPlaneUrl && window.localStorage) {
      window.localStorage.setItem('ao2.controlPlaneUrl', controlPlaneUrl);
    }
  }
  loadControlPlaneDefaults();
  async function selectedEvidenceRunId() {
    return activeEvidenceSummaryRunId || (runEvidenceDiffRight ? runEvidenceDiffRight.value : '') || (runEvidenceDiffLeft ? runEvidenceDiffLeft.value : '');
  }
  if (runEvidenceChangesButton && runEvidenceDiffOutput) {
    runEvidenceChangesButton.addEventListener('click', async () => {
      const runId = await selectedEvidenceRunId();
      runEvidenceDiffOutput.textContent = 'Loading changed evidence...';
      const response = await fetch(`/api/runs/evidence/changes?token=${encodeURIComponent(token)}&run_id=${encodeURIComponent(runId)}`);
      const json = await response.json();
      if (!response.ok) {
        runEvidenceDiffOutput.textContent = json.error || JSON.stringify(json, null, 2);
        return;
      }
      runEvidenceDiffOutput.innerHTML = renderRunEvidenceChanges(json);
    });
  }
  if (runEvidenceExportChangesButton && runEvidenceExportOutput && canOperate) {
    runEvidenceExportChangesButton.addEventListener('click', async () => {
      await exportEvidence(new URLSearchParams({kind: 'changes', run_id: await selectedEvidenceRunId()}));
    });
  }
  if (runEvidenceDiffButton && runEvidenceDiffOutput && runEvidenceDiffLeft && runEvidenceDiffRight) {
    runEvidenceDiffButton.addEventListener('click', async () => {
      const leftRunId = runEvidenceDiffLeft.value || '';
      const rightRunId = runEvidenceDiffRight.value || '';
      runEvidenceDiffOutput.textContent = 'Loading run evidence diff...';
      const response = await fetch(`/api/runs/evidence/diff?token=${encodeURIComponent(token)}&left_run_id=${encodeURIComponent(leftRunId)}&right_run_id=${encodeURIComponent(rightRunId)}`);
      const json = await response.json();
      if (!response.ok) {
        runEvidenceDiffOutput.textContent = json.error || JSON.stringify(json, null, 2);
        return;
      }
      runEvidenceDiffOutput.innerHTML = renderRunEvidenceDiff(json);
    });
  }
  if (runEvidenceExportDiffButton && runEvidenceExportOutput && runEvidenceDiffLeft && runEvidenceDiffRight && canOperate) {
    runEvidenceExportDiffButton.addEventListener('click', async () => {
      await exportEvidence(new URLSearchParams({
        kind: 'diff',
        left_run_id: runEvidenceDiffLeft.value || '',
        right_run_id: runEvidenceDiffRight.value || ''
      }));
    });
  }
  async function refreshQueue() {
    if (!executionEnabled || !queueOutput) return;
    const params = new URLSearchParams({token});
    if (queueStatusFilter && queueStatusFilter.value) params.set('status', queueStatusFilter.value);
    if (queueTemplateFilter && queueTemplateFilter.value) params.set('template', queueTemplateFilter.value);
    const response = await fetch(`/api/queue?${params.toString()}`);
    const json = await response.json();
    if (!json.jobs || json.jobs.length === 0) {
      queueOutput.textContent = 'No queued runs yet.';
      return;
    }
    queueOutput.innerHTML = json.jobs.map((job) => {
      const canCancel = canOperate && ['queued', 'running'].includes(job.status);
      const canRetry = canOperate && ['failed', 'rejected', 'cancelled', 'interrupted'].includes(job.status);
      const diagnosis = job.diagnosis || {};
      const recovery = diagnosis.recovery_actions && diagnosis.recovery_actions.length ? diagnosis.recovery_actions[0] : '';
      const actions = [
        fileLink('Open Evidence', job.evidence_pack),
        fileLink('Open Cockpit', job.cockpit),
        actionButton('Logs', 'logs', job),
        actionButton('Details', 'detail', job),
        canCancel ? actionButton('Cancel', 'cancel', job) : '',
        canRetry ? actionButton('Retry', 'retry', job) : ''
      ].filter(Boolean).join('');
      return `<div class="queue-job">
        <div class="queue-job-header"><strong>${escapeHtml(job.run_id)}</strong><code>${escapeHtml(job.status)}</code></div>
        <div class="muted">${escapeHtml(job.job_id)} kind=${escapeHtml(job.job_kind || 'run')}${job.retry_of ? ` retry of ${escapeHtml(job.retry_of)}` : ''}</div>
        ${job.repair_source_run_id ? `<div class="muted">repair_source=${escapeHtml(job.repair_source_run_id)} ${fileLink('Source Evidence', job.repair_evidence_pack)}</div>` : ''}
        ${job.error ? `<div class="warn">${escapeHtml(job.error)}</div>` : ''}
        ${diagnosis.failure_kind && diagnosis.failure_kind !== 'none' ? `<div class="muted">diagnosis=${escapeHtml(diagnosis.failure_kind)} exit=${escapeHtml(diagnosis.exit_code)} timed_out=${diagnosis.timed_out ? 'true' : 'false'}</div>` : ''}
        ${recovery ? `<div class="muted">${escapeHtml(recovery)}</div>` : ''}
        <div class="queue-actions">${actions}</div>
      </div>`;
    }).join('');
    if (activeLogJobId && !json.jobs.some((job) => job.job_id === activeLogJobId)) {
      activeLogJobId = '';
      if (queueLogOutput) queueLogOutput.textContent = 'Selected log job is no longer in the queue view.';
    }
  }
  async function refreshSelectedLogs() {
    if (!executionEnabled || !queueLogOutput || !activeLogJobId) return;
    const response = await fetch(`/api/queue/job/logs?token=${encodeURIComponent(token)}&job_id=${encodeURIComponent(activeLogJobId)}&tail_bytes=32768`);
    const json = await response.json();
    if (!response.ok) {
      queueLogOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    const stdoutTruncated = json.stdout && json.stdout.truncated ? ' (tail truncated)' : '';
    const stderrTruncated = json.stderr && json.stderr.truncated ? ' (tail truncated)' : '';
    queueLogOutput.textContent = [
      `${json.job.run_id} ${json.job.status}`,
      json.job.error || '',
      `--- stdout${stdoutTruncated} ---`,
      json.stdout ? json.stdout.text || '' : '',
      `--- stderr${stderrTruncated} ---`,
      json.stderr ? json.stderr.text || '' : ''
    ].join('\n');
  }
  async function refreshAudit() {
    if (!executionEnabled || !queueAuditOutput) return;
    const params = new URLSearchParams({token});
    if (queueAuditActionFilter && queueAuditActionFilter.value) params.set('action', queueAuditActionFilter.value);
    const response = await fetch(`/api/queue/audit?${params.toString()}`);
    const json = await response.json();
    if (!json.events || json.events.length === 0) {
      queueAuditOutput.textContent = 'No queue audit events yet.';
      return;
    }
    queueAuditOutput.innerHTML = json.events.map((event) => {
      const action = escapeHtml(event.action || '');
      const jobId = escapeHtml(event.job_id || '');
      const runId = escapeHtml(event.run_id || '');
      const retryOf = escapeHtml(event.retry_of || '');
      return `<div class="queue-job">
        <div class="queue-job-header"><strong>${action}</strong><code>${escapeHtml(event.timestamp_ms || '')}</code></div>
        <div class="muted">${jobId}${runId ? ` / ${runId}` : ''}${retryOf ? ` retry of ${retryOf}` : ''}</div>
      </div>`;
    }).join('');
  }
  if (queueOutput) {
    queueOutput.addEventListener('click', async (event) => {
      const button = event.target.closest('button[data-action]');
      if (!button) return;
      const action = button.dataset.action;
      const jobId = button.dataset.jobId;
      if (action === 'logs') {
        activeLogJobId = jobId;
        if (queueLogOutput) queueLogOutput.textContent = 'Loading queue logs...';
        await refreshSelectedLogs();
        return;
      }
      if (action === 'detail') {
        window.open(`/queue/job?token=${encodeURIComponent(token)}&job_id=${encodeURIComponent(jobId)}`, '_blank', 'noopener,noreferrer');
        const response = await fetch(`/api/queue/job?token=${encodeURIComponent(token)}&job_id=${encodeURIComponent(jobId)}`);
        const json = await response.json();
        if (queueDetailOutput) {
          queueDetailOutput.textContent = [
            `${json.job.run_id} ${json.job.status}`,
            json.job.error || '',
            '--- stdout ---',
            json.stdout || '',
            '--- stderr ---',
            json.stderr || ''
          ].join('\n');
        }
        return;
      }
      const endpoint = action === 'retry' ? '/api/queue/retry' : '/api/queue/cancel';
      const body = new URLSearchParams({job_id: jobId});
      const response = await fetch(`${endpoint}?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body
      });
      const json = await response.json();
      output.textContent = json.job_id || json.status || json.error || JSON.stringify(json, null, 2);
      await refreshQueue();
      await refreshAudit();
    });
  }
  if (queueAuditRefresh) queueAuditRefresh.addEventListener('click', refreshAudit);
  if (queueAuditActionFilter) queueAuditActionFilter.addEventListener('change', refreshAudit);
  if (queueExportPreviewButton && canOperate) {
    queueExportPreviewButton.addEventListener('click', async () => {
      if (output) output.textContent = 'Previewing support bundle redaction...';
      const response = await fetch(`/api/queue/export-preview?token=${encodeURIComponent(token)}`, {method: 'POST'});
      const json = await response.json();
      const redaction = json.redaction_preview || {};
      const fields = (redaction.redacted_fields || []).map((field) => `${field.path}: ${field.redacted_excerpt}`).join('\n');
      if (output) {
        output.textContent = json.error || [
          `would_write_bundle=${json.would_write_bundle}`,
          `jobs=${json.queue_job_count || 0} logs=${json.job_log_count || 0} audit=${json.audit_event_count || 0} evidence=${json.evidence_export_count || 0}`,
          `redactions=${redaction.redaction_count || 0}`,
          fields
        ].filter(Boolean).join('\n');
      }
    });
  }
  if (queueExportButton && canOperate) {
    queueExportButton.addEventListener('click', async () => {
      if (output) output.textContent = 'Exporting support bundle...';
      const response = await fetch(`/api/queue/export?token=${encodeURIComponent(token)}`, {method: 'POST'});
      const json = await response.json();
      await refreshSupportPacket();
      if (output) output.textContent = json.bundle_path || json.error || JSON.stringify(json, null, 2);
    });
  }
  async function refreshProjectStartNextAction(event) {
    if (event) event.preventDefault();
    if (!projectStartNextActionForm || !projectStartNextActionOutput || !token) return;
    projectStartNextActionOutput.textContent = 'Loading project-start next action...';
    const formData = new FormData(projectStartNextActionForm);
    const params = new URLSearchParams(formData);
    params.set('token', token);
    const response = await fetch(`/api/factory/project-start/next-action?${params.toString()}`);
    const json = await response.json();
    if (!response.ok) {
      projectStartNextActionOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    projectStartNextActionOutput.textContent = renderProjectStartNextAction(json);
  }
  if (projectStartNextActionForm) projectStartNextActionForm.addEventListener('submit', refreshProjectStartNextAction);
  async function publishProjectStartOperatorRecord(event) {
    if (event) event.preventDefault();
    if (!projectStartOperatorRecordForm || !projectStartOperatorRecordOutput || !token) return;
    projectStartOperatorRecordOutput.textContent = 'Publishing project-start operator record...';
    const formData = new FormData(projectStartOperatorRecordForm);
    const body = new URLSearchParams(formData);
    const response = await fetch(`/api/factory/project-start/operator-record?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      headers: {'Content-Type': 'application/x-www-form-urlencoded'},
      body
    });
    const json = await response.json();
    if (!response.ok) {
      projectStartOperatorRecordOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    projectStartOperatorRecordOutput.textContent = [
      `schema=${json.schema_version || ''}`,
      `status=${json.status || ''}`,
      `run_id=${json.run_id || ''}`,
      `record_path=${json.record_path || ''}`,
      `record_sha256=${json.record_sha256 || ''}`,
      `would_execute_queue=${json.would_execute_queue === true ? 'true' : 'false'}`,
      `would_submit_queue_entry=${json.would_submit_queue_entry === true ? 'true' : 'false'}`,
      `would_rebuild_wrappers=${json.would_rebuild_wrappers === true ? 'true' : 'false'}`,
      `would_mutate_control_plane=${json.would_mutate_control_plane === true ? 'true' : 'false'}`
    ].join('\n');
  }
  if (projectStartOperatorRecordForm) projectStartOperatorRecordForm.addEventListener('submit', publishProjectStartOperatorRecord);
  async function refreshProjectStartHermesFlowContract(event) {
    if (event) event.preventDefault();
    if (!projectStartHermesFlowContractForm || !projectStartHermesFlowContractOutput || !token) return;
    projectStartHermesFlowContractOutput.textContent = 'Fetching Hermes project-start flow contract...';
    const formData = new FormData(projectStartHermesFlowContractForm);
    const params = new URLSearchParams(formData);
    params.set('token', token);
    const response = await fetch(`/api/factory/project-start/hermes-flow-contract?${params.toString()}`);
    const json = await response.json();
    if (!response.ok) {
      projectStartHermesFlowContractOutput.textContent = json.error || JSON.stringify(json, null, 2);
      return;
    }
    projectStartHermesFlowContractOutput.textContent = [
      `schema=${json.schema_version || ''}`,
      `status=${json.status || ''}`,
      `contract_path=${json.contract_path || ''}`,
      `contract_sha256=${json.contract_sha256 || ''}`,
      `preview_role=${((json.workflow || {}).preview || {}).minimum_role || ''}`,
      `publish_role=${((json.workflow || {}).publish || {}).minimum_role || ''}`,
      `raw_queue_json_scrape_required=${((json.hermes_contract || {}).raw_queue_json_scrape_required === true) ? 'true' : 'false'}`,
      `would_execute_queue=${((json.side_effects || {}).would_execute_queue === true) ? 'true' : 'false'}`,
      `would_submit_queue_entry=${((json.side_effects || {}).would_submit_queue_entry === true) ? 'true' : 'false'}`,
      `would_rebuild_wrappers=${((json.side_effects || {}).would_rebuild_wrappers === true) ? 'true' : 'false'}`,
      `would_mutate_control_plane=${((json.side_effects || {}).would_mutate_control_plane === true) ? 'true' : 'false'}`
    ].join('\n');
  }
  if (projectStartHermesFlowContractForm) projectStartHermesFlowContractForm.addEventListener('submit', refreshProjectStartHermesFlowContract);
  if (providerSmokeButton && providerSmokeOutput && canOperate && executionEnabled) {
    providerSmokeButton.addEventListener('click', async () => {
      providerSmokeOutput.textContent = 'Running provider smoke...';
      const body = new URLSearchParams({
        minimum_score: providerSmokeMinimumScore ? providerSmokeMinimumScore.value || '90' : '90'
      });
      if (providerSmokeLiveProvider && providerSmokeLiveProvider.value) {
        body.set('live_provider', providerSmokeLiveProvider.value);
      }
      const response = await fetch(`/api/provider-smoke?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body
      });
      const json = await response.json();
      if (!response.ok) {
        providerSmokeOutput.textContent = json.error || JSON.stringify(json, null, 2);
        return;
      }
      const scripted = (json.providers || []).find((provider) => provider.provider === 'scripted') || {};
      const live = (json.providers || []).find((provider) => provider.provider === (providerSmokeLiveProvider ? providerSmokeLiveProvider.value : '')) || null;
      providerSmokeOutput.textContent = [
        `scripted=${scripted.verdict || 'unknown'} score=${scripted.score || 0}`,
        live ? `${live.provider}=${live.verdict || 'unknown'} score=${live.score || 0}` : '',
        `history=${json.history_path || ''}`,
        `entries=${json.history_entry_count || 0}`
      ].filter(Boolean).join('\n');
    });
  }
  if (providerPilotButton && providerPilotOutput && canOperate) {
    function providerPilotBody() {
      const body = new URLSearchParams({
        provider: providerPilotProvider ? providerPilotProvider.value : 'codex',
        template: providerPilotTemplate ? providerPilotTemplate.value : 'bug-fix',
        provider_prompt_file: providerPilotPromptFile ? providerPilotPromptFile.value : '',
        max_repair_attempts: providerPilotMaxRepairAttempts ? providerPilotMaxRepairAttempts.value || '1' : '1',
        max_budget_usd: providerPilotMaxBudgetUsd ? providerPilotMaxBudgetUsd.value || '1.00' : '1.00',
        minimum_score: providerPilotMinimumScore ? providerPilotMinimumScore.value || '90' : '90'
      });
      if (providerPilotRunId && providerPilotRunId.value) {
        body.set('run_id', providerPilotRunId.value);
      }
      return body;
    }
    function renderProviderPilotBlocked(json) {
      const gate = json.gate ? `gate=${json.gate.verdict || 'unknown'}` : '';
      const reasons = json.gate && json.gate.reasons ? json.gate.reasons.map((reason) => `${reason.code}:${reason.provider || ''}`).join(', ') : '';
      return [
        json.status ? `status=${json.status}` : '',
        json.error ? `error=${json.error}` : '',
        gate,
        reasons
      ].filter(Boolean).join('\n') || JSON.stringify(json, null, 2);
    }
    function renderProviderPilotPreflight(json) {
      const checks = (json.checks || []).map((check) => `${check.name || ''}=${check.status || ''}${check.verdict ? ` verdict=${check.verdict}` : ''}${check.message ? ` ${check.message}` : ''}`);
      const pilot = json.pilot || {};
      return [
        `status=${json.status || ''}`,
        `can_start=${json.can_start ? 'true' : 'false'}`,
        ...checks,
        pilot.gate ? `gate=${pilot.gate.verdict || ''}` : '',
        pilot.shell_command || ''
      ].filter(Boolean).join('\n');
    }
    function renderProviderPilotAcceptanceExport(json) {
      const acceptance = json.export ? json.export.provider_pilot_acceptance || {} : {};
      const score = acceptance.score || {};
      const replay = acceptance.replay || {};
      return [
        `export=${escapeHtml(json.export_path || '')}`,
        `provider=${escapeHtml(acceptance.provider || '')}`,
        `run_id=${escapeHtml(acceptance.run_id || '')}`,
        `status=${escapeHtml(acceptance.status || '')}`,
        `score=${escapeHtml(score.score || 0)} verdict=${escapeHtml(score.verdict || '')}`,
        `replay=${escapeHtml(replay.status || '')} digest_failures=${escapeHtml((replay.digest_failures || []).length)}`,
        `${fileLink('Open Export', json.export_path)} ${fileLink('Open Evidence', acceptance.evidence_pack)} ${fileLink('Open Cockpit', acceptance.cockpit)}`
      ].filter(Boolean).join('\n');
    }
    function providerPilotAcceptanceParams() {
      const params = new URLSearchParams({});
      if (providerPilotAcceptanceProvider && providerPilotAcceptanceProvider.value) {
        params.set('provider', providerPilotAcceptanceProvider.value);
      }
      if (providerPilotAcceptanceReplayStatus && providerPilotAcceptanceReplayStatus.value) {
        params.set('history_replay_status', providerPilotAcceptanceReplayStatus.value);
      }
      if (providerPilotAcceptanceMinScore && providerPilotAcceptanceMinScore.value) {
        params.set('history_min_score', providerPilotAcceptanceMinScore.value);
      }
      if (providerPilotAcceptanceSort && providerPilotAcceptanceSort.value) {
        params.set('history_sort', providerPilotAcceptanceSort.value);
      }
      if (providerPilotAcceptanceLimit && providerPilotAcceptanceLimit.value) {
        params.set('history_limit', providerPilotAcceptanceLimit.value);
      }
      return params;
    }
    function renderProviderPilotLatestAcceptance(json) {
      const history = (json.acceptance_history || []).map((entry) => `<tr><td><code>${escapeHtml(entry.release_tag || '')}</code></td><td><code>${escapeHtml(entry.provider || '')}</code></td><td><code>${escapeHtml(entry.run_id || '')}</code></td><td>${escapeHtml(entry.score || 0)}</td><td>${escapeHtml(entry.replay_status || '')}</td><td>${escapeHtml(entry.verdict || '')}</td><td><code>${escapeHtml(entry.acceptance_bundle || '')}</code></td></tr>`).join('');
      const historyTable = history ? `<table id="provider-pilot-acceptance-history"><thead><tr><th>Release</th><th>Provider</th><th>Run</th><th>Score</th><th>Replay</th><th>Verdict</th><th>Bundle</th></tr></thead><tbody>${history}</tbody></table>` : '';
      const filter = json.acceptance_filter || {};
      const trend = json.acceptance_trend || {};
      const trendLine = trend.schema_version ? `trend=${trend.regression ? 'regression' : 'stable'} current=${escapeHtml(trend.current_score || 0)} previous=${escapeHtml(trend.previous_score || '')} delta=${escapeHtml(trend.score_delta ?? '')} best=${escapeHtml(trend.best_score || 0)} worst=${escapeHtml(trend.worst_score || 0)}` : '';
      return [
        `acceptance_bundle=${escapeHtml(json.acceptance_bundle || '')}`,
        `provider=${escapeHtml(json.provider || '')}`,
        `run_id=${escapeHtml(json.run_id || '')}`,
        `status=${escapeHtml(json.status || '')}`,
        `score=${escapeHtml(json.score || 0)} verdict=${escapeHtml(json.verdict || '')}`,
        `replay=${escapeHtml(json.replay_status || '')} digest_failures=${escapeHtml(json.digest_failure_count || 0)}`,
        trendLine,
        `history=${escapeHtml((json.acceptance_history || []).length)} of ${escapeHtml(json.history_total_count || 0)} sort=${escapeHtml(filter.sort || 'newest')}`,
        `${fileLink('Open Evidence', json.acceptance ? json.acceptance.evidence_pack : '')} ${fileLink('Open Cockpit', json.acceptance ? json.acceptance.cockpit : '')}`,
        historyTable
      ].filter(Boolean).join('\n');
    }
    function renderProviderPilotCostLedger(json) {
      const totals = json.totals || {};
      const rows = (json.entries || []).map((entry) => `<tr><td><code>${escapeHtml(entry.release_tag || '')}</code></td><td><code>${escapeHtml(entry.provider || '')}</code></td><td><code>${escapeHtml(entry.run_id || '')}</code></td><td>${escapeHtml(entry.max_budget_usd || 0)}</td><td>${escapeHtml(entry.observed_cost_usd || 0)}</td><td>${escapeHtml(entry.total_tokens || 0)}</td><td>${entry.provider_enforced_budget ? 'yes' : 'no'}</td></tr>`).join('');
      const table = rows ? `<table id="provider-pilot-cost-ledger"><thead><tr><th>Release</th><th>Provider</th><th>Run</th><th>Budget</th><th>Observed Cost</th><th>Tokens</th><th>Enforced</th></tr></thead><tbody>${rows}</tbody></table>` : '';
      return [
        `status=${escapeHtml(json.status || '')}`,
        `entries=${escapeHtml(json.entry_count || 0)} failed_candidates=${escapeHtml(json.failed_candidate_count || 0)}`,
        `budget=${escapeHtml(totals.max_budget_usd || 0)} observed_cost=${escapeHtml(totals.observed_cost_usd || 0)} tokens=${escapeHtml(totals.total_tokens || 0)}`,
        table
      ].filter(Boolean).join('\n');
    }
    function renderProviderPilotCostTrend(json) {
      const delta = json.delta || {};
      const chart = renderProviderPilotCostTrendChart(json);
      const rows = (json.releases || []).map((release) => `<tr><td><code>${escapeHtml(release.release_tag || '')}</code></td><td>${escapeHtml(release.entry_count || 0)}</td><td>${escapeHtml(release.max_budget_usd || 0)}</td><td>${escapeHtml(release.observed_cost_usd || 0)}</td><td>${escapeHtml(release.total_tokens || 0)}</td></tr>`).join('');
      const table = rows ? `<table id="provider-pilot-cost-trend"><thead><tr><th>Release</th><th>Entries</th><th>Budget</th><th>Observed Cost</th><th>Tokens</th></tr></thead><tbody>${rows}</tbody></table>` : '';
      return [
        `status=${escapeHtml(json.status || '')}`,
        `releases=${escapeHtml(json.release_count || 0)} latest=${escapeHtml(json.latest_release_tag || '')} previous=${escapeHtml(json.previous_release_tag || '')}`,
        `delta_budget=${escapeHtml(delta.max_budget_usd || 0)} delta_observed_cost=${escapeHtml(delta.observed_cost_usd || 0)} delta_tokens=${escapeHtml(delta.total_tokens || 0)}`,
        chart,
        table
      ].filter(Boolean).join('\n');
    }
    function renderProviderPilotCostTrendChart(json) {
      const releases = (json.releases || []).slice(-8);
      if (!releases.length) return '';
      const maxValue = Math.max(1, ...releases.flatMap((release) => [Number(release.max_budget_usd || 0), Number(release.observed_cost_usd || 0)]));
      const chartWidth = 720;
      const chartHeight = 220;
      const left = 48;
      const top = 24;
      const bottom = 42;
      const plotHeight = chartHeight - top - bottom;
      const slot = (chartWidth - left - 18) / releases.length;
      const barWidth = Math.max(10, Math.min(24, slot * 0.24));
      const bars = releases.map((release, index) => {
        const x = left + index * slot + slot * 0.28;
        const budget = Number(release.max_budget_usd || 0);
        const cost = Number(release.observed_cost_usd || 0);
        const budgetHeight = Math.round((budget / maxValue) * plotHeight);
        const costHeight = Math.round((cost / maxValue) * plotHeight);
        const label = escapeHtml(release.release_tag || '');
        return `<g>
          <rect class="chart-budget" x="${x}" y="${top + plotHeight - budgetHeight}" width="${barWidth}" height="${budgetHeight}"></rect>
          <rect class="chart-cost" x="${x + barWidth + 4}" y="${top + plotHeight - costHeight}" width="${barWidth}" height="${costHeight}"></rect>
          <text x="${x}" y="${chartHeight - 16}">${label}</text>
        </g>`;
      }).join('');
      return `<div id="provider-pilot-cost-trend-chart" class="trend-chart">
        <svg role="img" aria-label="Provider pilot cost trend chart" viewBox="0 0 ${chartWidth} ${chartHeight}" preserveAspectRatio="xMidYMid meet">
          <title>Provider pilot cost trend chart</title>
          <line class="chart-axis" x1="${left}" y1="${top + plotHeight}" x2="${chartWidth - 12}" y2="${top + plotHeight}"></line>
          <text x="${left}" y="14">Budget vs observed provider cost, latest retained releases</text>
          <text x="${chartWidth - 180}" y="14">budget</text>
          <rect class="chart-budget" x="${chartWidth - 220}" y="5" width="12" height="8"></rect>
          <text x="${chartWidth - 96}" y="14">observed</text>
          <rect class="chart-cost" x="${chartWidth - 138}" y="5" width="12" height="8"></rect>
          ${bars}
        </svg>
      </div>`;
    }
    if (providerPilotPreflightButton) {
      providerPilotPreflightButton.addEventListener('click', async () => {
        providerPilotOutput.textContent = 'Running provider pilot preflight...';
        const response = await fetch(`/api/provider-pilot/preflight?token=${encodeURIComponent(token)}`, {
          method: 'POST',
          headers: {'Content-Type': 'application/x-www-form-urlencoded'},
          body: providerPilotBody()
        });
        const json = await response.json();
        providerPilotOutput.textContent = response.ok ? renderProviderPilotPreflight(json) : (json.error || JSON.stringify(json, null, 2));
      });
    }
    providerPilotButton.addEventListener('click', async () => {
      providerPilotOutput.textContent = 'Building provider pilot...';
      const response = await fetch(`/api/provider-pilot?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body: providerPilotBody()
      });
      const json = await response.json();
      if (response.ok) {
        const approval = json.approval_packet || {};
        providerPilotOutput.textContent = [
          json.shell_command || '',
          `workflow=${json.workflow || ''}`,
          json.max_budget_usd ? `max_budget_usd=${json.max_budget_usd}` : '',
          approval.action_digest ? `approval_action_digest=${approval.action_digest}` : '',
          approval.next_action || '',
          `gate=${json.gate ? json.gate.verdict : ''}`
        ].filter(Boolean).join('\n');
      } else {
        providerPilotOutput.textContent = renderProviderPilotBlocked(json);
      }
    });
    if (providerPilotStartButton && executionEnabled) {
      providerPilotStartButton.addEventListener('click', async () => {
        providerPilotOutput.textContent = 'Building exact approval digest...';
        const previewBody = providerPilotBody();
        const previewResponse = await fetch(`/api/provider-pilot?token=${encodeURIComponent(token)}`, {
          method: 'POST',
          headers: {'Content-Type': 'application/x-www-form-urlencoded'},
          body: previewBody
        });
        const preview = await previewResponse.json();
        if (!previewResponse.ok) {
          providerPilotOutput.textContent = renderProviderPilotBlocked(preview);
          return;
        }
        const startBody = providerPilotBody();
        if (preview.run_id) {
          startBody.set('run_id', preview.run_id);
        }
        const approval = preview.approval_packet || {};
        startBody.set('approval_action_digest', approval.action_digest || '');
        providerPilotOutput.textContent = 'Queueing provider pilot with exact approval digest...';
        const response = await fetch(`/api/provider-pilot/start?token=${encodeURIComponent(token)}`, {
          method: 'POST',
          headers: {'Content-Type': 'application/x-www-form-urlencoded'},
          body: startBody
        });
        const json = await response.json();
        if (response.ok) {
          providerPilotOutput.textContent = [
            `queued=${json.job_id || ''}`,
            `run_id=${json.run_id || ''}`,
            `approval=${json.approval ? json.approval.status : 'approved_exact_action_digest'}`,
            json.max_budget_usd ? `max_budget_usd=${json.max_budget_usd}` : '',
            json.pilot && json.pilot.shell_command ? json.pilot.shell_command : ''
          ].filter(Boolean).join('\n');
          await refreshQueue();
          await refreshAudit();
        } else {
          providerPilotOutput.textContent = renderProviderPilotBlocked(json);
        }
      });
    }
    if (providerPilotAcceptanceExportButton) {
      if (providerPilotAcceptanceLatestButton) {
        providerPilotAcceptanceLatestButton.addEventListener('click', async () => {
          providerPilotOutput.textContent = 'Loading latest provider pilot acceptance bundle...';
          const query = providerPilotAcceptanceParams();
          query.set('token', token);
          const response = await fetch(`/api/provider-pilot/acceptance/latest?${query.toString()}`);
          const json = await response.json();
          if (response.ok) {
            if (providerPilotAcceptanceBundle) {
              providerPilotAcceptanceBundle.value = json.acceptance_bundle || '';
            }
            providerPilotOutput.innerHTML = renderProviderPilotLatestAcceptance(json);
          } else {
            providerPilotOutput.textContent = json.error || JSON.stringify(json, null, 2);
          }
        });
      }
      providerPilotAcceptanceExportButton.addEventListener('click', async () => {
        const acceptanceBundle = providerPilotAcceptanceBundle ? providerPilotAcceptanceBundle.value : '';
        if (!acceptanceBundle) {
          providerPilotOutput.textContent = 'Acceptance bundle path required.';
          return;
        }
        providerPilotOutput.textContent = 'Exporting provider pilot acceptance evidence...';
        const body = new URLSearchParams({
          kind: 'provider-pilot-acceptance',
          acceptance_bundle: acceptanceBundle
        });
        const response = await fetch(`/api/runs/evidence/export?token=${encodeURIComponent(token)}`, {
          method: 'POST',
          headers: {'Content-Type': 'application/x-www-form-urlencoded'},
          body
        });
        const json = await response.json();
        if (response.ok) {
          providerPilotOutput.innerHTML = renderProviderPilotAcceptanceExport(json);
          await refreshSupportPacket();
        } else {
          providerPilotOutput.textContent = json.error || JSON.stringify(json, null, 2);
        }
      });
      if (providerPilotAcceptanceExportLatestButton) {
        providerPilotAcceptanceExportLatestButton.addEventListener('click', async () => {
          providerPilotOutput.textContent = 'Exporting latest provider pilot acceptance evidence...';
          const body = providerPilotAcceptanceParams();
          const response = await fetch(`/api/provider-pilot/acceptance/export-latest?token=${encodeURIComponent(token)}`, {
            method: 'POST',
            headers: {'Content-Type': 'application/x-www-form-urlencoded'},
            body
          });
          const json = await response.json();
          if (response.ok) {
            if (providerPilotAcceptanceBundle && json.latest) {
              providerPilotAcceptanceBundle.value = json.latest.acceptance_bundle || '';
            }
            providerPilotOutput.innerHTML = renderProviderPilotAcceptanceExport(json.export || {});
            await refreshSupportPacket();
          } else {
            providerPilotOutput.textContent = json.error || JSON.stringify(json, null, 2);
          }
        });
      }
      if (providerPilotCostLedgerButton) {
        providerPilotCostLedgerButton.addEventListener('click', async () => {
          providerPilotOutput.textContent = 'Loading provider pilot cost ledger...';
          const response = await fetch(`/api/provider-pilot/cost-ledger?token=${encodeURIComponent(token)}`);
          const json = await response.json();
          if (response.ok) {
            providerPilotOutput.innerHTML = renderProviderPilotCostLedger(json);
          } else {
            providerPilotOutput.textContent = json.error || JSON.stringify(json, null, 2);
          }
        });
      }
      if (providerPilotCostTrendButton) {
        providerPilotCostTrendButton.addEventListener('click', async () => {
          providerPilotOutput.textContent = 'Loading provider pilot cost trend...';
          const response = await fetch(`/api/provider-pilot/cost-trend?token=${encodeURIComponent(token)}`);
          const json = await response.json();
          if (response.ok) {
            providerPilotOutput.innerHTML = renderProviderPilotCostTrend(json);
          } else {
            providerPilotOutput.textContent = json.error || JSON.stringify(json, null, 2);
          }
        });
      }
    }
  }
  if (form && output && canOperate) {
    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      output.textContent = executionEnabled ? 'Queueing run...' : 'Building command...';
      const body = new URLSearchParams(new FormData(form));
      const endpoint = executionEnabled ? '/api/queue/start' : '/api/launch';
      const response = await fetch(`${endpoint}?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body
      });
      const json = await response.json();
      if (json.shell_command) {
        const discovery = json.role_contract_discovery || {};
        const preflight = json.launch_preflight || {};
        output.textContent = [
          json.shell_command,
          `role_contract_discovery=${discovery.mode || 'unknown'} loaded=${discovery.loaded_count ?? 0}`,
          `ao2_auto_loaded_role_contracts=${preflight.ao2_auto_loaded_role_contracts === true ? 'true' : 'false'}`,
          `preflight_plan=${preflight.plan_path || ''}`
        ].join('\n');
      } else {
        output.textContent = json.job_id || json.error || JSON.stringify(json, null, 2);
      }
      await refreshQueue();
      await refreshAudit();
    });
  }
  if (repairResumeForm && repairResumeOutput && canOperate && executionEnabled) {
    repairResumeForm.addEventListener('submit', async (event) => {
      event.preventDefault();
      repairResumeOutput.textContent = 'Queueing repair resume...';
      const body = new URLSearchParams(new FormData(repairResumeForm));
      const response = await fetch(`/api/repair/resume/start?token=${encodeURIComponent(token)}`, {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body
      });
      const json = await response.json();
      repairResumeOutput.textContent = response.ok
        ? [
            `job_id=${json.job_id || ''}`,
            `run_id=${json.run_id || ''}`,
            `source_run_id=${json.source_run_id || ''}`,
            `repair_evidence_pack=${json.repair_evidence_pack || ''}`
          ].join('\n')
        : (json.error || JSON.stringify(json, null, 2));
      await refreshQueue();
      await refreshAudit();
    });
  }
  if (queueStatusFilter) queueStatusFilter.addEventListener('change', refreshQueue);
  if (queueTemplateFilter) queueTemplateFilter.addEventListener('change', refreshQueue);
  refreshQueue();
  refreshSelectedLogs();
  refreshAudit();
  if (executionEnabled) {
    setInterval(async () => {
      await refreshQueue();
      await refreshSelectedLogs();
    }, 1500);
    setInterval(refreshAudit, 5000);
  }
})();
</script>
"#,
    );
    Ok(())
}

fn render_workbench_commands(html: &mut String, version: &str) -> Result<()> {
    let release_tag = format!("v{version}");
    write!(
        html,
        r#"<section>
<h2>Operator Commands</h2>
<div class="commands">
  <div class="command"><div class="label">Run Browser</div><code>ao2 runs list --target .</code></div>
  <div class="command"><div class="label">Cockpit Index</div><code>ao2 cockpit index --target . --open</code></div>
  <div class="command"><div class="label">Health Check</div><code>ao2 doctor --json</code></div>
  <div class="command"><div class="label">Upgrade</div><code>ao2 upgrade apply --github-release {release_tag} --repo uesugitorachiyo/ao2</code></div>
  <div class="command"><div class="label">Fleet Monitoring</div><code>ao2 control-plane refresh --sources fleet-sources.json --out fleet-snapshot.json --history fleet-history</code></div>
  <div class="command"><div class="label">Fleet Health</div><code>ao2 control-plane health --fleet fleet-snapshot.json --history fleet-history --json</code></div>
  <div class="command"><div class="label">Fleet Diff</div><code>ao2 control-plane history diff --history fleet-history --json</code></div>
  <div class="command"><div class="label">Fleet History</div><code>ao2 control-plane history export --history fleet-history --out fleet-history/index.html --json</code></div>
</div>
</section>
"#,
        release_tag = escape_html(&release_tag)
    )?;
    Ok(())
}

pub(super) fn render_workbench_job_detail_page(detail: &serde_json::Value) -> String {
    let job = &detail["job"];
    let run_id = json_string(job, "run_id");
    let status = json_string(job, "status");
    let evidence_pack = json_string(job, "evidence_pack");
    let cockpit = json_string(job, "cockpit");
    let stdout = detail["stdout"].as_str().unwrap_or("");
    let stderr = detail["stderr"].as_str().unwrap_or("");
    let diagnosis = &detail["diagnosis"];
    let recovery_actions = json_array(diagnosis, "recovery_actions")
        .iter()
        .map(|action| format!("<li>{}</li>", escape_html(action.as_str().unwrap_or(""))))
        .collect::<Vec<_>>()
        .join("");
    let evidence_link = workbench_file_anchor("Open Evidence", &evidence_pack);
    let cockpit_link = workbench_file_anchor("Open Cockpit", &cockpit);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>AO2 Queue Job {run_id}</title>
  <style>
    body {{ margin: 0; font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; color: #18202f; background: #f6f7f9; }}
    main {{ max-width: 1120px; margin: 0 auto; padding: 32px 20px 48px; }}
    h1 {{ margin: 0 0 4px; font-size: 30px; line-height: 1.15; }}
    .muted {{ color: #5f6b7a; font-size: 14px; }}
    .toolbar {{ display: flex; gap: 10px; margin: 18px 0 24px; flex-wrap: wrap; }}
    .toolbar a {{ border: 1px solid #cbd3dc; border-radius: 6px; color: #152238; padding: 8px 10px; text-decoration: none; background: #fff; }}
    .metrics {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 10px; margin: 18px 0; }}
    .metric {{ background: #fff; border: 1px solid #dbe1e8; border-radius: 8px; padding: 12px; }}
    .metric span {{ display: block; color: #596677; font-size: 12px; margin-bottom: 6px; }}
    .metric strong {{ font-size: 18px; }}
    pre {{ background: #111827; color: #f3f4f6; border-radius: 8px; padding: 14px; overflow: auto; white-space: pre-wrap; min-height: 80px; }}
    .diagnosis {{ background: #fff; border: 1px solid #d8e0ea; border-radius: 8px; padding: 14px; }}
    .diagnosis ul {{ margin: 8px 0 0; padding-left: 20px; }}
    section {{ margin-top: 24px; }}
  </style>
</head>
<body>
  <main class="queue-detail-page">
    <h1>{run_id}</h1>
    <div class="muted">Job {job_id} / {status}</div>
    <div class="toolbar">{evidence_link}{cockpit_link}</div>
    <section class="metrics" aria-label="Runtime metrics">
      <div class="metric"><span>Queue Wait</span><strong>{queue_wait_ms} ms</strong></div>
      <div class="metric"><span>Duration</span><strong>{duration_ms} ms</strong></div>
      <div class="metric"><span>Exit Code</span><strong>{exit_code}</strong></div>
      <div class="metric"><span>Retry Count</span><strong>{retry_count}</strong></div>
    </section>
    <section class="diagnosis">
      <h2>Failure Diagnosis</h2>
      <div class="muted">kind={failure_kind} timed_out={timed_out}</div>
      <p>{primary_error}</p>
      <ul>{recovery_actions}</ul>
      <h3>Stderr Excerpt</h3>
      <pre>{stderr_excerpt}</pre>
      <h3>Stdout Excerpt</h3>
      <pre>{stdout_excerpt}</pre>
    </section>
    <section>
      <h2>Stdout</h2>
      <pre>{stdout}</pre>
    </section>
    <section>
      <h2>Stderr</h2>
      <pre>{stderr}</pre>
    </section>
  </main>
</body>
</html>"#,
        run_id = escape_html(&run_id),
        job_id = escape_html(&json_string(job, "job_id")),
        status = escape_html(&status),
        evidence_link = evidence_link,
        cockpit_link = cockpit_link,
        queue_wait_ms = json_u64(job, "queue_wait_ms"),
        duration_ms = json_u64(job, "duration_ms"),
        exit_code = job["exit_code"].as_i64().unwrap_or(-1),
        retry_count = json_u64(job, "retry_count"),
        failure_kind = escape_html(&json_string(diagnosis, "failure_kind")),
        timed_out = diagnosis
            .get("timed_out")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        primary_error = escape_html(&json_string(diagnosis, "primary_error")),
        recovery_actions = recovery_actions,
        stderr_excerpt = escape_html(&json_string(diagnosis, "stderr_excerpt")),
        stdout_excerpt = escape_html(&json_string(diagnosis, "stdout_excerpt")),
        stdout = escape_html(stdout),
        stderr = escape_html(stderr)
    )
}

fn workbench_file_anchor(label: &str, path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    format!(
        r#"<a href="file://{href}">{label}</a>"#,
        href = escape_html(path),
        label = escape_html(label)
    )
}
