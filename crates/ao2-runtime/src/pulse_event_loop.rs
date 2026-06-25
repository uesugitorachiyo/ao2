use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const CODEX_CRON_DECISION_SCHEMA: &str = "codex-cron.event-loop-decision.v1";
pub const AO2_PULSE_DECISION_SCHEMA: &str = "ao2.pulse-event-loop-decision.v1";
pub const AO2_PULSE_RUN_SCHEMA: &str = "ao2.pulse-event-loop-run.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseEventLoopAction {
    Continue,
    Stop,
    Backoff,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PulseEventLoopDecisionValue {
    pub action: PulseEventLoopAction,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub next_task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PulseIterationDecision {
    pub iteration: u32,
    pub action: String,
    pub reason: Option<String>,
    pub next_task_id: Option<String>,
    pub decision_source: String,
    pub decision_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseEventLoopRunSummary {
    pub schema_version: String,
    pub status: String,
    pub iterations: u32,
    pub max_chain_runs: u32,
    pub max_runtime_seconds: u64,
    pub decision_source: String,
    pub decision_path: Option<String>,
    pub reasons: Vec<String>,
    pub next_task_id: Option<String>,
    pub decisions: Vec<PulseIterationDecision>,
}

/// A simple robust command string parser that splits arguments by whitespace
/// while respecting single and double quotes.
pub fn split_command(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_double_quote = false;
    let mut in_single_quote = false;
    for c in cmd.chars() {
        match c {
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            ' ' | '\t' | '\r' | '\n' if !in_double_quote && !in_single_quote => {
                if !current.is_empty() {
                    args.push(current);
                    current = String::new();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(target_os = "windows")]
fn resolve_program(prog: &str) -> String {
    if prog == "npm" || prog == "npx" || prog == "pnpm" || prog == "yarn" {
        format!("{}.cmd", prog)
    } else {
        prog.to_string()
    }
}

#[cfg(not(target_os = "windows"))]
fn resolve_program(prog: &str) -> String {
    prog.to_string()
}

pub fn parse_event_loop_decision(text: &str) -> Result<PulseEventLoopDecisionValue> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        if let Some(decision) = decision_from_value(&value) {
            return Ok(decision);
        }
    }

    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{'))
    {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        let Ok(value) = parsed else {
            if line.contains(CODEX_CRON_DECISION_SCHEMA) || line.contains(AO2_PULSE_DECISION_SCHEMA)
            {
                return Err(anyhow!("malformed event-loop decision json"));
            }
            continue;
        };
        if let Some(decision) = decision_from_value(&value) {
            return Ok(decision);
        }
    }

    Err(anyhow!("no event-loop decision emitted"))
}

fn decision_from_value(value: &serde_json::Value) -> Option<PulseEventLoopDecisionValue> {
    let schema = value
        .get("schema_version")
        .and_then(serde_json::Value::as_str)?;
    if schema != CODEX_CRON_DECISION_SCHEMA && schema != AO2_PULSE_DECISION_SCHEMA {
        return None;
    }
    let loop_value = value.get("event_loop")?;
    serde_json::from_value(loop_value.clone()).ok()
}

pub fn run_pulse_event_loop(
    command_str: &str,
    decision_file_path: Option<&Path>,
    max_chain_runs: u32,
    max_runtime_seconds: u64,
    out_dir: &Path,
    stdout_fallback: bool,
    apply_root: &Path,
) -> Result<PulseEventLoopRunSummary> {
    fs::create_dir_all(out_dir).context("failed to create output directory")?;
    let logs_dir = out_dir.join("logs");
    fs::create_dir_all(&logs_dir).context("failed to create logs directory")?;

    let started = Instant::now();
    let mut iterations = 0u32;
    let mut reasons = Vec::new();
    let mut decisions = Vec::new();
    let mut status = "max_chain_reached".to_string();
    let mut next_task_id = None;
    let mut decision_source = "missing".to_string();

    let resolved_decision_path = decision_file_path.map(|p| {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            apply_root.join(p)
        }
    });

    while iterations < max_chain_runs {
        if started.elapsed() >= Duration::from_secs(max_runtime_seconds) {
            status = "max_runtime_reached".to_string();
            break;
        }

        iterations += 1;
        let iteration = iterations;

        // Execute command
        let cmd_parts = split_command(command_str);
        if cmd_parts.is_empty() {
            status = "failed".to_string();
            let err_msg = "empty command string".to_string();
            reasons.push(err_msg.clone());
            decisions.push(PulseIterationDecision {
                iteration,
                action: "fail".to_string(),
                reason: Some(err_msg),
                next_task_id: None,
                decision_source: "command".to_string(),
                decision_file: None,
            });
            break;
        }

        let prog = resolve_program(&cmd_parts[0]);
        let args = &cmd_parts[1..];

        let mut cmd = Command::new(&prog);
        cmd.args(args);
        cmd.current_dir(apply_root);

        let output = match cmd.output() {
            Ok(out) => out,
            Err(e) => {
                status = "failed".to_string();
                let err_msg = format!("failed to start command '{}': {}", prog, e);
                reasons.push(err_msg.clone());
                decisions.push(PulseIterationDecision {
                    iteration,
                    action: "fail".to_string(),
                    reason: Some(err_msg),
                    next_task_id: None,
                    decision_source: "command".to_string(),
                    decision_file: None,
                });
                break;
            }
        };

        // Write iteration logs
        let log_file = logs_dir.join(format!("iteration-{:02}.log", iteration));
        let mut log_content = format!("$ {}\n", command_str);
        log_content.push_str(&format!(
            "exit_code: {}\n\n",
            output.status.code().unwrap_or(-1)
        ));
        log_content.push_str("--- stdout ---\n");
        log_content.push_str(&String::from_utf8_lossy(&output.stdout));
        log_content.push_str("\n--- stderr ---\n");
        log_content.push_str(&String::from_utf8_lossy(&output.stderr));
        let _ = fs::write(&log_file, log_content);

        // Check command exit code
        if !output.status.success() {
            status = "failed".to_string();
            let err_msg = format!("command exited with status {}", output.status);
            reasons.push(err_msg.clone());
            decisions.push(PulseIterationDecision {
                iteration,
                action: "fail".to_string(),
                reason: Some(err_msg),
                next_task_id: None,
                decision_source: "command".to_string(),
                decision_file: None,
            });
            break;
        }

        // Determine decision source & parse decision
        let mut current_source = "missing".to_string();
        let mut current_file = None;

        let parsed_decision = if let Some(ref path) = resolved_decision_path {
            if path.exists() {
                current_source = "file".to_string();
                current_file = Some(path.to_string_lossy().to_string());
                match fs::read_to_string(path) {
                    Ok(text) => parse_event_loop_decision(&text).unwrap_or_else(|e| {
                        PulseEventLoopDecisionValue {
                            action: PulseEventLoopAction::Fail,
                            reason: Some(format!("malformed decision in file: {}", e)),
                            next_task_id: None,
                        }
                    }),
                    Err(e) => PulseEventLoopDecisionValue {
                        action: PulseEventLoopAction::Fail,
                        reason: Some(format!("failed to read decision file: {}", e)),
                        next_task_id: None,
                    },
                }
            } else if stdout_fallback {
                current_source = "stdout".to_string();
                let text = String::from_utf8_lossy(&output.stdout);
                parse_event_loop_decision(&text).unwrap_or_else(|e| PulseEventLoopDecisionValue {
                    action: PulseEventLoopAction::Fail,
                    reason: Some(format!("malformed decision in stdout: {}", e)),
                    next_task_id: None,
                })
            } else {
                PulseEventLoopDecisionValue {
                    action: PulseEventLoopAction::Fail,
                    reason: Some(format!("decision file missing: {}", path.display())),
                    next_task_id: None,
                }
            }
        } else if stdout_fallback {
            current_source = "stdout".to_string();
            let text = String::from_utf8_lossy(&output.stdout);
            parse_event_loop_decision(&text).unwrap_or_else(|e| PulseEventLoopDecisionValue {
                action: PulseEventLoopAction::Fail,
                reason: Some(format!("malformed decision in stdout: {}", e)),
                next_task_id: None,
            })
        } else {
            PulseEventLoopDecisionValue {
                action: PulseEventLoopAction::Fail,
                reason: Some("no decision file specified and stdout fallback disabled".to_string()),
                next_task_id: None,
            }
        };

        decision_source = current_source.clone();
        let action_str = format!("{:?}", parsed_decision.action).to_lowercase();
        let reason_opt = parsed_decision.reason.clone();
        next_task_id = parsed_decision.next_task_id.clone();

        decisions.push(PulseIterationDecision {
            iteration,
            action: action_str,
            reason: reason_opt.clone(),
            next_task_id: next_task_id.clone(),
            decision_source: current_source,
            decision_file: current_file,
        });

        if let Some(r) = reason_opt {
            reasons.push(r);
        }

        match parsed_decision.action {
            PulseEventLoopAction::Continue => {}
            PulseEventLoopAction::Stop => {
                status = "stopped".to_string();
                break;
            }
            PulseEventLoopAction::Backoff => {
                status = "backoff".to_string();
                break;
            }
            PulseEventLoopAction::Fail => {
                status = "failed".to_string();
                break;
            }
        }

        // Write summary at the end of each iteration for durability
        let summary = PulseEventLoopRunSummary {
            schema_version: AO2_PULSE_RUN_SCHEMA.to_string(),
            status: status.clone(),
            iterations,
            max_chain_runs,
            max_runtime_seconds,
            decision_source: decision_source.clone(),
            decision_path: resolved_decision_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            reasons: reasons.clone(),
            next_task_id: next_task_id.clone(),
            decisions: decisions.clone(),
        };
        let summary_file = out_dir.join("summary.json");
        let _ = fs::write(
            &summary_file,
            serde_json::to_string_pretty(&summary).unwrap_or_default() + "\n",
        );
    }

    if started.elapsed() >= Duration::from_secs(max_runtime_seconds) {
        status = "max_runtime_reached".to_string();
    }

    let summary = PulseEventLoopRunSummary {
        schema_version: AO2_PULSE_RUN_SCHEMA.to_string(),
        status,
        iterations,
        max_chain_runs,
        max_runtime_seconds,
        decision_source,
        decision_path: resolved_decision_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        reasons,
        next_task_id,
        decisions,
    };

    let summary_file = out_dir.join("summary.json");
    fs::write(
        &summary_file,
        serde_json::to_string_pretty(&summary)? + "\n",
    )
    .context("failed to write summary file")?;

    Ok(summary)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseAutoAdvanceRun {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub status: String,
    pub resume_json: String,
    pub max_iterations: u32,
    pub forever: bool,
    pub sleep_seconds: u64,
    pub completed_iterations: u32,
    pub heartbeat_count: u32,
    pub stop_file: String,
    pub ledger: String,
    pub pr_ci_gate: PrCiGate,
    pub local_only_while_pr_blocked: LocalOnlyWhilePrBlocked,
    pub direct_main_publish: DirectMainPublishInfo,
    pub results: Vec<PulseAutoAdvanceIterationResult>,
    pub trust_boundary: TrustBoundary,
    #[serde(default)]
    pub observed_eval_loop_sha256: Option<String>,
    #[serde(default)]
    pub auto_advance: Option<serde_json::Value>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub current_iteration: Option<u32>,
    #[serde(default)]
    pub current_task_count: Option<usize>,
    #[serde(default)]
    pub pulse_task_manifest_path: Option<String>,
    #[serde(default)]
    pub task_execution_mode: Option<String>,
    #[serde(default)]
    pub pr_ci_gate_update: Option<PrCiGateUpdateInfo>,
    #[serde(default)]
    pub pulse_generate_next: Option<PulseGenerateNextInfo>,
    #[serde(default)]
    pub generated_next_packet: bool,
    #[serde(default)]
    pub generated_local_only_packet: bool,
    #[serde(default)]
    pub register_next_packet: bool,
    #[serde(default)]
    pub sha256_matches: Option<bool>,
    #[serde(default)]
    pub operator_prompt_sha256: Option<String>,
    #[serde(default)]
    pub operator_prompt_sha256_matches: Option<bool>,
    #[serde(default)]
    pub operator_prompt_observed_sha256: Option<String>,
    #[serde(default)]
    pub pulse_eval_loop_path: Option<String>,
    #[serde(default)]
    pub pulse_eval_loop_sha256: Option<String>,
    #[serde(default)]
    pub operator_prompt_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrCiGate {
    pub schema_version: String,
    pub enabled: bool,
    pub update_enabled: bool,
    pub state_path: String,
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub required_checks: Vec<serde_json::Value>,
    #[serde(default)]
    pub source_schema_version: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pr: Option<serde_json::Value>,
    #[serde(default)]
    pub trigger: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalOnlyWhilePrBlocked {
    pub enabled: bool,
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub gate_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMainPublishInfo {
    pub enabled: bool,
    pub status: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub log: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustBoundary {
    pub local_only: bool,
    pub stores_credentials: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseAutoAdvanceIterationResult {
    pub iteration: u32,
    pub index: usize,
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub manifest: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    pub status: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub log: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub expected_evidence: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrCiGateUpdateInfo {
    pub command: String,
    pub status: String,
    pub exit_code: i32,
    pub log: String,
    pub state_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseGenerateNextInfo {
    pub command: String,
    pub status: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub log: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub gate_status: Option<String>,
    #[serde(default)]
    pub local_only_while_pr_blocked: Option<bool>,
    #[serde(default)]
    pub sleep_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseAutoAdvanceLedgerEntry {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub pulse_eval_loop_path: String,
    pub pulse_eval_loop_sha256: String,
    pub status: String,
    pub task_count: usize,
}

fn sha256_path(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(ao2_core::sha256_hex(&bytes))
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn load_seen(ledger: &Path) -> HashSet<String> {
    let mut seen = HashSet::new();
    if ledger.is_file() {
        if let Ok(content) = fs::read_to_string(ledger) {
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(item) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(digest) =
                        item.get("pulse_eval_loop_sha256").and_then(|v| v.as_str())
                    {
                        seen.insert(digest.to_string());
                    }
                }
            }
        }
    }
    seen
}

fn check_status(check: &serde_json::Value) -> String {
    let status = check
        .get("status")
        .or_else(|| check.get("state"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let conclusion = check
        .get("conclusion")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    if ["failure", "cancelled", "timed_out", "action_required"].contains(&conclusion.as_str()) {
        return "blocked".to_string();
    }
    if [
        "failure",
        "failed",
        "cancelled",
        "timed_out",
        "action_required",
    ]
    .contains(&status.as_str())
    {
        return "blocked".to_string();
    }
    if ["success", "neutral", "skipped"].contains(&conclusion.as_str()) {
        return "passed".to_string();
    }
    if ["success", "passed"].contains(&status.as_str()) {
        return "passed".to_string();
    }
    if status == "completed" && ["", "success", "neutral", "skipped"].contains(&conclusion.as_str())
    {
        return "passed".to_string();
    }
    "waiting".to_string()
}

fn load_pr_ci_gate(pr_ci_gate_enabled: bool, pr_ci_gate_state: &Path, trigger: &str) -> PrCiGate {
    let mut gate = PrCiGate {
        schema_version: "ao2.pulse-pr-ci-gate.v1".to_string(),
        enabled: pr_ci_gate_enabled,
        update_enabled: true,
        state_path: pr_ci_gate_state.to_string_lossy().to_string(),
        status: "passed".to_string(),
        reason: Some("state_missing".to_string()),
        required_checks: Vec::new(),
        source_schema_version: None,
        branch: None,
        pr: None,
        trigger: Some(trigger.to_string()),
        detail: None,
    };

    if !pr_ci_gate_enabled {
        gate.reason = Some("gate_disabled".to_string());
        return gate;
    }
    if !pr_ci_gate_state.is_file() {
        return gate;
    }

    let content = match fs::read_to_string(pr_ci_gate_state) {
        Ok(text) => text,
        Err(e) => {
            gate.status = "waiting".to_string();
            gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
            gate.detail = Some(format!("failed to read gate state: {}", e));
            return gate;
        }
    };

    let state: serde_json::Value = match serde_json::from_str(&content) {
        Ok(val) => val,
        Err(exc) => {
            gate.status = "waiting".to_string();
            gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
            gate.detail = Some(format!("gate_state_invalid_json: {}", exc));
            return gate;
        }
    };

    if !state.is_object() {
        gate.status = "waiting".to_string();
        gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
        gate.detail = Some("gate_state_not_object".to_string());
        return gate;
    }

    gate.source_schema_version = state
        .get("schema_version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    gate.branch = state
        .get("branch")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(pr) = state.get("pr") {
        if pr.is_object() {
            gate.pr = Some(pr.clone());
            let pr_state = pr
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let is_draft = pr
                .get("is_draft")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_draft {
                gate.status = "waiting".to_string();
                gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
                gate.detail = Some("pr_draft".to_string());
            } else if pr_state == "open" {
                gate.status = "waiting".to_string();
                gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
                gate.detail = Some("pr_open".to_string());
            }
        }
    }

    let state_status = state
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    if ["waiting", "pending", "failed", "blocked", "open", "draft"].contains(&state_status.as_str())
    {
        gate.status = "waiting".to_string();
        gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
        if gate.detail.is_none() {
            gate.detail = Some(format!("gate_status_{}", state_status));
        }
    }

    if let Some(checks) = state.get("required_checks").and_then(|v| v.as_array()) {
        gate.required_checks = checks.clone();
        for check in checks {
            if !check.is_object() {
                gate.status = "waiting".to_string();
                gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
                gate.detail = Some("required_check_not_object".to_string());
                continue;
            }
            let check_gate_status = check_status(check);
            if check_gate_status != "passed" {
                gate.status = "waiting".to_string();
                gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
                if gate.detail.is_none() {
                    gate.detail = Some(format!("required_check_{}", check_gate_status));
                }
            }
        }
    } else {
        gate.status = "waiting".to_string();
        gate.reason = Some("waiting_for_pr_merge_or_ci".to_string());
        gate.detail = Some("required_checks_not_list".to_string());
    }

    if gate.status == "passed"
        && ["passed", "green", "ready", "merged", "closed_success"].contains(&state_status.as_str())
    {
        gate.reason = Some("passed".to_string());
    }

    gate
}

fn run_command_str_in_dir(
    command_str: &str,
    cwd: &Path,
    envs: &[(&str, &str)],
    log_file: &Path,
) -> Result<i32> {
    let parts = split_command(command_str);
    if parts.is_empty() {
        anyhow::bail!("empty command string");
    }
    let prog = resolve_program(&parts[0]);
    let args = &parts[1..];
    let mut cmd = Command::new(&prog);
    cmd.args(args);
    cmd.current_dir(cwd);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut log_content = format!("$ {}\n", command_str);
    for (k, v) in envs {
        log_content.push_str(&format!("{}={}\n", k, v));
    }
    log_content.push_str("\n");

    let output = cmd.output()?;
    log_content.push_str("--- stdout ---\n");
    log_content.push_str(&String::from_utf8_lossy(&output.stdout));
    log_content.push_str("\n--- stderr ---\n");
    log_content.push_str(&String::from_utf8_lossy(&output.stderr));
    fs::write(log_file, log_content)?;

    Ok(output.status.code().unwrap_or(-1))
}

pub fn run_pulse_auto_advance(
    resume_json: &Path,
    out_dir: &Path,
    ledger: &Path,
    stop_file: &Path,
    max_iterations_opt: Option<u32>,
    allow_duplicate: bool,
    forever: bool,
    sleep_seconds: u64,
    generate_next: u32,
    generate_next_sleep_seconds_opt: Option<u64>,
    pr_ci_gate: u32,
    pr_ci_gate_state: &Path,
    pr_ci_gate_update: u32,
    local_only_while_pr_blocked: bool,
    direct_main_publish: bool,
    apply_root: &Path,
) -> Result<String> {
    if out_dir.exists() {
        let _ = fs::remove_dir_all(out_dir);
    }
    fs::create_dir_all(out_dir).context("failed to create output directory")?;
    let logs_dir = out_dir.join("logs");
    fs::create_dir_all(&logs_dir).context("failed to create logs directory")?;
    if let Some(parent) = ledger.parent() {
        fs::create_dir_all(parent).context("failed to create ledger directory")?;
    }

    let max_iterations = if let Some(val) = max_iterations_opt {
        val
    } else if forever {
        0
    } else {
        1
    };

    let generate_next_sleep_seconds = generate_next_sleep_seconds_opt.unwrap_or(sleep_seconds);

    let mut payload = PulseAutoAdvanceRun {
        schema_version: "ao2.pulse-auto-advance-run.v1".to_string(),
        generated_at_utc: utc_now(),
        status: "failed".to_string(),
        resume_json: resume_json.to_string_lossy().to_string(),
        max_iterations,
        forever,
        sleep_seconds,
        completed_iterations: 0,
        heartbeat_count: 0,
        stop_file: stop_file.to_string_lossy().to_string(),
        ledger: ledger.to_string_lossy().to_string(),
        pr_ci_gate: PrCiGate {
            schema_version: "ao2.pulse-pr-ci-gate.v1".to_string(),
            enabled: pr_ci_gate == 1,
            update_enabled: pr_ci_gate_update == 1,
            state_path: pr_ci_gate_state.to_string_lossy().to_string(),
            status: "not_checked".to_string(),
            reason: None,
            detail: None,
            required_checks: Vec::new(),
            source_schema_version: None,
            branch: None,
            pr: None,
            trigger: None,
        },
        local_only_while_pr_blocked: LocalOnlyWhilePrBlocked {
            enabled: local_only_while_pr_blocked,
            status: "not_checked".to_string(),
            reason: None,
            gate_status: None,
        },
        direct_main_publish: DirectMainPublishInfo {
            enabled: direct_main_publish,
            status: "not_checked".to_string(),
            command: None,
            exit_code: None,
            log: None,
            summary: None,
            reason: None,
        },
        results: Vec::new(),
        trust_boundary: TrustBoundary {
            local_only: true,
            stores_credentials: false,
        },
        observed_eval_loop_sha256: None,
        auto_advance: None,
        reason: None,
        current_iteration: None,
        current_task_count: None,
        pulse_task_manifest_path: None,
        task_execution_mode: None,
        pr_ci_gate_update: None,
        pulse_generate_next: None,
        generated_next_packet: false,
        generated_local_only_packet: false,
        register_next_packet: false,
        sha256_matches: None,
        operator_prompt_sha256: None,
        operator_prompt_sha256_matches: None,
        operator_prompt_observed_sha256: None,
        pulse_eval_loop_path: None,
        pulse_eval_loop_sha256: None,
        operator_prompt_path: None,
    };

    let write_summary = |payload: &PulseAutoAdvanceRun| -> Result<()> {
        let summary_file = out_dir.join("summary.json");
        let content = serde_json::to_string_pretty(payload)? + "\n";
        fs::write(&summary_file, content)?;
        Ok(())
    };

    let write_heartbeat = |payload: &mut PulseAutoAdvanceRun,
                           reason: &str,
                           resume: Option<&serde_json::Value>,
                           digest: Option<&str>|
     -> Result<()> {
        payload.schema_version = "ao2.pulse-auto-advance-heartbeat.v1".to_string();
        payload.status = "waiting".to_string();
        payload.reason = Some(reason.to_string());
        payload.generated_at_utc = utc_now();
        payload.heartbeat_count += 1;
        if let Some(r) = resume {
            if let Some(auto_advance) = r.get("auto_advance") {
                payload.auto_advance = Some(auto_advance.clone());
            }
        }
        if let Some(d) = digest {
            payload.observed_eval_loop_sha256 = Some(d.to_string());
        }
        write_summary(payload)?;
        Ok(())
    };

    let refresh_pr_ci_gate = |payload: &mut PulseAutoAdvanceRun, reason: &str| -> Result<bool> {
        if pr_ci_gate != 1 || pr_ci_gate_update != 1 {
            payload.pr_ci_gate_update = Some(PrCiGateUpdateInfo {
                command: "pulse:pr-ci-gate:update".to_string(),
                status: "skipped".to_string(),
                exit_code: 0,
                log: "".to_string(),
                state_path: pr_ci_gate_state.to_string_lossy().to_string(),
                reason: reason.to_string(),
            });
            return Ok(true);
        }
        let timestamp = Utc::now().timestamp();
        let log_path = logs_dir.join(format!("pulse_pr_ci_gate_update-{}.log", timestamp));

        let mut envs = Vec::new();
        let state_str = pr_ci_gate_state.to_string_lossy().to_string();
        envs.push(("AO2_PULSE_PR_CI_GATE_UPDATE_STATE", state_str.as_str()));
        let update_root = out_dir.join("pr-ci-gate-update");
        let update_root_str = update_root.to_string_lossy().to_string();
        envs.push(("AO2_PULSE_PR_CI_GATE_UPDATE_ROOT", update_root_str.as_str()));

        let exit_code = run_command_str_in_dir(
            "npm run pulse:pr-ci-gate:update",
            apply_root,
            &envs,
            &log_path,
        )?;

        payload.pr_ci_gate_update = Some(PrCiGateUpdateInfo {
            command: "pulse:pr-ci-gate:update".to_string(),
            status: if exit_code == 0 {
                "passed".to_string()
            } else {
                "failed".to_string()
            },
            exit_code,
            log: log_path.to_string_lossy().to_string(),
            state_path: pr_ci_gate_state.to_string_lossy().to_string(),
            reason: reason.to_string(),
        });

        if exit_code != 0 {
            payload.pr_ci_gate = PrCiGate {
                schema_version: "ao2.pulse-pr-ci-gate.v1".to_string(),
                enabled: pr_ci_gate == 1,
                update_enabled: pr_ci_gate_update == 1,
                state_path: pr_ci_gate_state.to_string_lossy().to_string(),
                status: "waiting".to_string(),
                reason: Some("waiting_for_pr_merge_or_ci".to_string()),
                detail: Some("pr_ci_gate_update_failed".to_string()),
                required_checks: Vec::new(),
                source_schema_version: None,
                branch: None,
                pr: None,
                trigger: None,
            };
            payload.pulse_generate_next = Some(PulseGenerateNextInfo {
                command: "pulse:generate-next".to_string(),
                status: "skipped".to_string(),
                exit_code: None,
                log: None,
                reason: "waiting_for_pr_merge_or_ci".to_string(),
                gate_status: Some("waiting".to_string()),
                local_only_while_pr_blocked: None,
                sleep_seconds: None,
            });
            payload.generated_next_packet = false;
            payload.generated_local_only_packet = false;
            payload.register_next_packet = false;
            payload.status = "waiting".to_string();
            payload.reason = Some("waiting_for_pr_merge_or_ci".to_string());
            payload.generated_at_utc = utc_now();
            write_summary(payload)?;
            return Ok(false);
        }
        Ok(true)
    };

    let pulse_generate_next = |payload: &mut PulseAutoAdvanceRun, reason: &str| -> Result<bool> {
        if !forever || generate_next != 1 {
            return Ok(false);
        }
        if !refresh_pr_ci_gate(payload, reason)? {
            return Ok(false);
        }
        let gate = load_pr_ci_gate(pr_ci_gate == 1, pr_ci_gate_state, reason);
        payload.pr_ci_gate = gate.clone();
        if gate.status != "passed" {
            if local_only_while_pr_blocked {
                let timestamp = Utc::now().timestamp();
                let log_path =
                    logs_dir.join(format!("pulse_generate_next-local-only-{}.log", timestamp));
                let mut envs = Vec::new();
                envs.push(("AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY", "1"));

                let exit_code = run_command_str_in_dir(
                    "npm run pulse:generate-next",
                    apply_root,
                    &envs,
                    &log_path,
                )?;

                payload.pulse_generate_next = Some(PulseGenerateNextInfo {
                    command: "pulse:generate-next".to_string(),
                    status: if exit_code == 0 {
                        "passed".to_string()
                    } else {
                        "failed".to_string()
                    },
                    exit_code: Some(exit_code),
                    log: Some(log_path.to_string_lossy().to_string()),
                    reason: reason.to_string(),
                    gate_status: Some(gate.status),
                    local_only_while_pr_blocked: Some(true),
                    sleep_seconds: Some(generate_next_sleep_seconds),
                });
                payload.local_only_while_pr_blocked = LocalOnlyWhilePrBlocked {
                    enabled: true,
                    status: if exit_code == 0 {
                        "passed".to_string()
                    } else {
                        "failed".to_string()
                    },
                    reason: Some("pr_ci_gate_blocked_normal_generation".to_string()),
                    gate_status: Some(payload.pr_ci_gate.status.clone()),
                };
                payload.status = if exit_code == 0 {
                    "waiting".to_string()
                } else {
                    "failed".to_string()
                };
                payload.generated_next_packet = false;
                payload.generated_local_only_packet = exit_code == 0;
                payload.register_next_packet = exit_code == 0;
                payload.reason = Some(if exit_code == 0 {
                    "generated_local_only_packet".to_string()
                } else {
                    "generate_next_failed".to_string()
                });
                payload.generated_at_utc = utc_now();
                write_summary(payload)?;
                return Ok(exit_code == 0);
            }
            payload.pulse_generate_next = Some(PulseGenerateNextInfo {
                command: "pulse:generate-next".to_string(),
                status: "skipped".to_string(),
                exit_code: None,
                log: None,
                reason: "waiting_for_pr_merge_or_ci".to_string(),
                gate_status: Some(gate.status),
                local_only_while_pr_blocked: None,
                sleep_seconds: None,
            });
            payload.generated_next_packet = false;
            payload.generated_local_only_packet = false;
            payload.register_next_packet = false;
            payload.status = "waiting".to_string();
            payload.reason = Some("waiting_for_pr_merge_or_ci".to_string());
            payload.generated_at_utc = utc_now();
            write_summary(payload)?;
            return Ok(false);
        }

        let timestamp = Utc::now().timestamp();
        let log_path = logs_dir.join(format!("pulse_generate_next-{}.log", timestamp));
        let exit_code =
            run_command_str_in_dir("npm run pulse:generate-next", apply_root, &[], &log_path)?;

        payload.pulse_generate_next = Some(PulseGenerateNextInfo {
            command: "pulse:generate-next".to_string(),
            status: if exit_code == 0 {
                "passed".to_string()
            } else {
                "failed".to_string()
            },
            exit_code: Some(exit_code),
            log: Some(log_path.to_string_lossy().to_string()),
            reason: reason.to_string(),
            gate_status: None,
            local_only_while_pr_blocked: None,
            sleep_seconds: Some(generate_next_sleep_seconds),
        });
        payload.status = if exit_code == 0 {
            "waiting".to_string()
        } else {
            "failed".to_string()
        };
        payload.generated_next_packet = exit_code == 0;
        payload.generated_local_only_packet = false;
        payload.register_next_packet = exit_code == 0;
        payload.reason = Some(if exit_code == 0 {
            "generated_next_packet".to_string()
        } else {
            "generate_next_failed".to_string()
        });
        payload.generated_at_utc = utc_now();
        write_summary(payload)?;
        Ok(exit_code == 0)
    };

    let pulse_direct_main_publish =
        |payload: &mut PulseAutoAdvanceRun, reason: &str| -> Result<bool> {
            if !direct_main_publish {
                payload.direct_main_publish = DirectMainPublishInfo {
                    enabled: false,
                    status: "skipped".to_string(),
                    command: None,
                    exit_code: None,
                    log: None,
                    summary: None,
                    reason: Some("direct_main_publish_disabled".to_string()),
                };
                return Ok(true);
            }
            let timestamp = Utc::now().timestamp();
            let log_path = logs_dir.join(format!("pulse_direct_main_publish-{}.log", timestamp));
            let mut envs = Vec::new();
            envs.push(("AO2_PULSE_DIRECT_MAIN_PUBLISH_REASON", reason));
            let publish_root = out_dir.join("direct-main-publish");
            let publish_root_str = publish_root.to_string_lossy().to_string();
            envs.push((
                "AO2_PULSE_DIRECT_MAIN_PUBLISH_ROOT",
                publish_root_str.as_str(),
            ));

            let exit_code = run_command_str_in_dir(
                "npm run pulse:direct-main-publish",
                apply_root,
                &envs,
                &log_path,
            )?;

            payload.direct_main_publish = DirectMainPublishInfo {
                enabled: true,
                command: Some("pulse:direct-main-publish".to_string()),
                status: if exit_code == 0 {
                    "passed".to_string()
                } else {
                    "failed".to_string()
                },
                exit_code: Some(exit_code),
                log: Some(log_path.to_string_lossy().to_string()),
                summary: Some(
                    publish_root
                        .join("summary.json")
                        .to_string_lossy()
                        .to_string(),
                ),
                reason: Some(reason.to_string()),
            };

            if exit_code != 0 {
                payload.status = "failed".to_string();
                payload.reason = Some("direct_main_publish_failed".to_string());
                payload.generated_at_utc = utc_now();
                write_summary(payload)?;
                return Ok(false);
            }
            write_summary(payload)?;
            Ok(true)
        };

    if stop_file.exists() {
        payload.status = "stopped".to_string();
        payload.reason = Some("stop_file_present".to_string());
        write_summary(&payload)?;
        return Ok("stopped".to_string());
    }

    if !resume_json.is_file() {
        payload.reason = Some(format!("resume_json_missing: {}", resume_json.display()));
        write_summary(&payload)?;
        return Ok("failed".to_string());
    }

    let mut iteration = 0;
    loop {
        if stop_file.exists() {
            payload.status = "stopped".to_string();
            payload.reason = Some("stop_file_present".to_string());
            break;
        }

        if !resume_json.is_file() {
            if forever {
                write_heartbeat(&mut payload, "waiting_for_resume_json", None, None)?;
                std::thread::sleep(Duration::from_secs(sleep_seconds));
                continue;
            }
            payload.reason = Some(format!("resume_json_missing: {}", resume_json.display()));
            break;
        }

        let resume_content = fs::read_to_string(resume_json)?;
        let resume: serde_json::Value = serde_json::from_str(&resume_content)?;

        let pulse_eval_loop_path_rel = resume
            .get("pulse_eval_loop_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("missing pulse_eval_loop_path in resume.json"))?;
        let resume_parent = resume_json.parent().unwrap_or_else(|| Path::new("."));
        let eval_loop_path = resume_parent.join(pulse_eval_loop_path_rel);

        if !eval_loop_path.is_file() {
            payload.reason = Some(format!(
                "eval_loop_path_missing: {}",
                eval_loop_path.display()
            ));
            break;
        }

        let eval_loop_sha256 = sha256_path(&eval_loop_path)?;
        let expected_eval_loop_sha256 = resume
            .get("pulse_eval_loop_sha256")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let operator_prompt_path_rel = resume
            .get("operator_prompt_path")
            .and_then(|v| v.as_str())
            .unwrap_or("operator-prompt.txt");
        let operator_prompt_path = resume_parent.join(operator_prompt_path_rel);
        let operator_prompt_sha256 = if operator_prompt_path.is_file() {
            Some(sha256_path(&operator_prompt_path)?)
        } else {
            None
        };
        let expected_operator_prompt_sha256 = resume
            .get("operator_prompt_sha256")
            .and_then(|v| v.as_str());

        let sha256_matches = eval_loop_sha256 == expected_eval_loop_sha256;
        let operator_prompt_sha256_matches = match (
            expected_operator_prompt_sha256,
            operator_prompt_sha256.as_ref(),
        ) {
            (Some(expected), Some(observed)) => expected == observed,
            (None, None) => true,
            _ => false,
        };

        payload.pulse_eval_loop_path = Some(eval_loop_path.to_string_lossy().to_string());
        payload.pulse_eval_loop_sha256 = Some(expected_eval_loop_sha256.to_string());
        payload.observed_eval_loop_sha256 = Some(eval_loop_sha256.clone());
        payload.operator_prompt_path = Some(operator_prompt_path.to_string_lossy().to_string());
        payload.operator_prompt_sha256 = expected_operator_prompt_sha256.map(|s| s.to_string());
        payload.auto_advance = resume.get("auto_advance").cloned();
        payload.sha256_matches = Some(sha256_matches);
        payload.operator_prompt_sha256_matches = Some(operator_prompt_sha256_matches);
        payload.operator_prompt_observed_sha256 = operator_prompt_sha256.clone();

        if !sha256_matches {
            payload.reason = Some("eval_loop_hash_mismatch".to_string());
            break;
        }
        if !operator_prompt_sha256_matches {
            payload.reason = Some("operator_prompt_hash_mismatch".to_string());
            break;
        }

        let continue_until_exit_gate = resume
            .get("auto_advance")
            .and_then(|v| v.get("continue_until_exit_gate"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !continue_until_exit_gate {
            payload.reason = Some("auto_advance_continue_until_exit_gate_missing".to_string());
            break;
        }

        let event_loop_decision_path = resume_parent.join("ao2-event-loop-decision.json");
        if event_loop_decision_path.is_file() {
            let decision_content = fs::read_to_string(&event_loop_decision_path)?;
            let decision: serde_json::Value = serde_json::from_str(&decision_content)?;
            let action = decision
                .get("event_loop")
                .and_then(|v| v.get("action"))
                .and_then(|v| v.as_str());
            if action == Some("stop") {
                payload.status = "stopped".to_string();
                payload.reason = Some("readiness_exit_gate_satisfied".to_string());
                break;
            }
        }

        let seen = load_seen(ledger);
        if seen.contains(&eval_loop_sha256) && !allow_duplicate {
            if forever {
                if pulse_generate_next(&mut payload, "duplicate_eval_loop_digest")? {
                    std::thread::sleep(Duration::from_secs(generate_next_sleep_seconds));
                    continue;
                }
                if payload.reason.as_deref() == Some("waiting_for_pr_merge_or_ci") {
                    break;
                }
                write_heartbeat(
                    &mut payload,
                    "waiting_for_new_eval_loop_digest",
                    Some(&resume),
                    Some(&eval_loop_sha256),
                )?;
                std::thread::sleep(Duration::from_secs(sleep_seconds));
                continue;
            }
            payload.status = "stopped".to_string();
            payload.reason = Some("duplicate_eval_loop_digest".to_string());
            break;
        }

        let eval_loop_content = fs::read_to_string(&eval_loop_path)?;
        let eval_loop: serde_json::Value = serde_json::from_str(&eval_loop_content)?;
        let tasks = eval_loop
            .get("recommended_tasks")
            .and_then(|v| v.as_array());
        let tasks = match tasks {
            Some(t) if !t.is_empty() => t,
            _ => {
                payload.reason = Some("recommended_tasks_missing".to_string());
                break;
            }
        };

        let pulse_task_manifest_path = eval_loop_path.with_file_name("pulse-task-manifest.json");

        iteration += 1;
        payload.schema_version = "ao2.pulse-auto-advance-run.v1".to_string();
        payload.status = "running".to_string();
        payload.reason = Some("executing_recommended_tasks".to_string());
        payload.generated_at_utc = utc_now();
        payload.current_iteration = Some(iteration);
        payload.current_task_count = Some(tasks.len());
        payload.pulse_task_manifest_path = if pulse_task_manifest_path.is_file() {
            Some(pulse_task_manifest_path.to_string_lossy().to_string())
        } else {
            None
        };

        write_summary(&payload)?;

        let mut iteration_results = Vec::new();
        let expected_result_count;

        if pulse_task_manifest_path.is_file() {
            payload.task_execution_mode = Some("structured_manifest".to_string());
            expected_result_count = 1;

            let executor_root = out_dir
                .join("task-executor")
                .join(format!("iteration-{:02}", iteration));
            let log_path = logs_dir.join(format!(
                "iteration-{:02}-pulse-task-executor.log",
                iteration
            ));

            let mut envs = Vec::new();
            let manifest_str = pulse_task_manifest_path.to_string_lossy().to_string();
            envs.push(("AO2_PULSE_TASK_EXECUTOR_MANIFEST", manifest_str.as_str()));
            let exec_root_str = executor_root.to_string_lossy().to_string();
            envs.push(("AO2_PULSE_TASK_EXECUTOR_ROOT", exec_root_str.as_str()));

            let exit_code = run_command_str_in_dir(
                "npm run pulse:task-executor",
                apply_root,
                &envs,
                &log_path,
            )?;

            let status = if exit_code == 0 {
                "passed".to_string()
            } else {
                "failed".to_string()
            };
            iteration_results.push(PulseAutoAdvanceIterationResult {
                iteration,
                index: 1,
                id: "pulse-task-executor".to_string(),
                title: Some("Pulse structured task manifest executor".to_string()),
                command: Some("npm run pulse:task-executor".to_string()),
                manifest: Some(pulse_task_manifest_path.to_string_lossy().to_string()),
                summary: Some(
                    executor_root
                        .join("summary.json")
                        .to_string_lossy()
                        .to_string(),
                ),
                status,
                exit_code: Some(exit_code),
                log: Some(log_path.to_string_lossy().to_string()),
                reason: None,
                expected_evidence: None,
            });
        } else {
            payload.task_execution_mode = Some("recommended_tasks".to_string());
            expected_result_count = tasks.len();

            for (index, task) in tasks.iter().enumerate() {
                let index = index + 1;
                let task_id = task.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let command = task.get("command").and_then(|v| v.as_str()).unwrap_or("");
                if command.is_empty() {
                    iteration_results.push(PulseAutoAdvanceIterationResult {
                        iteration,
                        index,
                        id: task_id.to_string(),
                        title: task
                            .get("title")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        command: None,
                        manifest: None,
                        summary: None,
                        status: "failed".to_string(),
                        exit_code: None,
                        log: None,
                        reason: Some("command_missing".to_string()),
                        expected_evidence: task.get("expected_evidence").cloned(),
                    });
                    break;
                }

                let safe_id: String = task_id
                    .chars()
                    .map(|ch| {
                        if ch.is_alphanumeric() || "._-".contains(ch) {
                            ch
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let log_path = logs_dir.join(format!(
                    "iteration-{:02}-{:02}-{}.log",
                    iteration, index, safe_id
                ));

                let exit_code = run_command_str_in_dir(command, apply_root, &[], &log_path)?;

                let status = if exit_code == 0 {
                    "passed".to_string()
                } else {
                    "failed".to_string()
                };
                iteration_results.push(PulseAutoAdvanceIterationResult {
                    iteration,
                    index,
                    id: task_id.to_string(),
                    title: task
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    command: Some(command.to_string()),
                    manifest: None,
                    summary: None,
                    status,
                    exit_code: Some(exit_code),
                    log: Some(log_path.to_string_lossy().to_string()),
                    reason: None,
                    expected_evidence: task.get("expected_evidence").cloned(),
                });

                if exit_code != 0 {
                    break;
                }
            }
        }

        payload.results.extend(iteration_results.clone());
        let all_passed = iteration_results.iter().all(|item| item.status == "passed")
            && iteration_results.len() == expected_result_count;

        if all_passed {
            payload.completed_iterations = iteration;
        } else {
            payload.status = "failed".to_string();
            payload.reason = Some("task_failed".to_string());
            break;
        }

        let ledger_entry = PulseAutoAdvanceLedgerEntry {
            schema_version: "ao2.pulse-auto-advance-ledger-entry.v1".to_string(),
            generated_at_utc: utc_now(),
            pulse_eval_loop_path: eval_loop_path.to_string_lossy().to_string(),
            pulse_eval_loop_sha256: eval_loop_sha256,
            status: "passed".to_string(),
            task_count: tasks.len(),
        };

        let entry_str = serde_json::to_string(&ledger_entry)? + "\n";
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ledger)?;
        use std::io::Write;
        file.write_all(entry_str.as_bytes())?;

        if !pulse_direct_main_publish(&mut payload, "completed_iteration")? {
            break;
        }

        if !forever {
            payload.status = "passed".to_string();
            break;
        }

        if max_iterations > 0 && iteration >= max_iterations {
            payload.status = "passed".to_string();
            payload.reason = Some("max_iterations_reached".to_string());
            break;
        }

        if pulse_generate_next(&mut payload, "completed_iteration")? {
            std::thread::sleep(Duration::from_secs(generate_next_sleep_seconds));
            continue;
        }

        if payload.reason.as_deref() == Some("waiting_for_pr_merge_or_ci") {
            break;
        }

        payload.status = "waiting".to_string();
        payload.reason = Some("waiting_for_new_eval_loop_digest".to_string());
        write_summary(&payload)?;
        std::thread::sleep(Duration::from_secs(sleep_seconds));
    }

    write_summary(&payload)?;
    Ok(payload.status)
}
