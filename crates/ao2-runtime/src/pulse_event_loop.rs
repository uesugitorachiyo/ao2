use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
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
