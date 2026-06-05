//! Claude Code CLI provider profile for AO2's adapter contract.

use std::collections::{BTreeMap, BTreeSet};

pub const PROVIDER_NAME: &str = "claude";
pub const METADATA_SOURCE: &str = "ao2-adapter-claude";
pub const DESCRIPTION: &str = "Claude Code CLI OAuth provider for implementation roles.";
pub const ADAPTER_KIND: &str = "local_oauth_cli";
pub const REGISTRY_PHASE: &str = "phase_1_guarded_live_pilot";
pub const SMOKE_SCRIPT: &str = "scripts/smoke-claude-provider-pilot.sh";
pub const SMOKE_GUARD_ENV: &str = "AO2_LIVE_CLAUDE_SMOKE";
pub const PILOT_GUARD_ENV: &str = "AO2_LIVE_CLAUDE_PILOT";
pub const DOCTOR_COMMAND: &str = "claude";
pub const DOCTOR_ARGS: &[&str] = &["--version"];
pub const TRANSCRIPT_FIELDS: &[&str] = &[
    "changed_files",
    "concerns",
    "blockers",
    "usage",
    "cost_usd",
    "raw_summary",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub provider_name: &'static str,
    pub metadata_source: &'static str,
    pub description: &'static str,
    pub adapter_kind: &'static str,
    pub registry_phase: &'static str,
    pub smoke_script: &'static str,
    pub smoke_guard_env: &'static str,
    pub pilot_guard_env: &'static str,
    pub doctor_command: &'static str,
    pub doctor_args: &'static [&'static str],
    pub transcript_fields: &'static [&'static str],
}

pub fn metadata() -> ProviderMetadata {
    ProviderMetadata {
        provider_name: PROVIDER_NAME,
        metadata_source: METADATA_SOURCE,
        description: DESCRIPTION,
        adapter_kind: ADAPTER_KIND,
        registry_phase: REGISTRY_PHASE,
        smoke_script: SMOKE_SCRIPT,
        smoke_guard_env: SMOKE_GUARD_ENV,
        pilot_guard_env: PILOT_GUARD_ENV,
        doctor_command: DOCTOR_COMMAND,
        doctor_args: DOCTOR_ARGS,
        transcript_fields: TRANSCRIPT_FIELDS,
    }
}

pub fn build_args(prompt: &str, max_budget_usd_arg: Option<String>) -> Vec<String> {
    let mut args = vec![
        "--print".to_string(),
        "--permission-mode".to_string(),
        "bypassPermissions".to_string(),
        "--allowedTools".to_string(),
        "Bash,Read,Write,Edit".to_string(),
        "--no-session-persistence".to_string(),
        "--output-format".to_string(),
        "text".to_string(),
    ];
    if let Some(max_budget_usd) = max_budget_usd_arg {
        args.push("--max-budget-usd".to_string());
        args.push(max_budget_usd);
    }
    args.push(sandbox_execution_prompt(prompt));
    args
}

pub fn sandbox_execution_prompt(prompt: &str) -> String {
    format!(
        r#"You are running inside an AO2 disposable sandbox copy of the target repository.
Execute the following shell script exactly in the current repository using Bash.
Do not ask follow-up questions.
Do not edit files outside the current repository.
After the script finishes, print a concise completion report that includes:
Summary: <short summary of the completed change>
Changed files: <comma-separated files changed>
Concern: <severity - message, only if any>
Blocker: <message, only if any>

Shell script:
```sh
{prompt}
```
"#
    )
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptSummary {
    pub changed_files: Vec<String>,
    pub concerns: Vec<TranscriptConcern>,
    pub blockers: Vec<TranscriptBlocker>,
    pub usage: TranscriptUsage,
    pub cost_usd: Option<f64>,
    pub transcript_ids: Vec<TranscriptId>,
    pub raw_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptConcern {
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptBlocker {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptId {
    pub kind: String,
    pub value: String,
}

pub fn parse_transcript(transcript: &str, sandbox_changed_files: &[String]) -> TranscriptSummary {
    parse_generic_transcript(transcript, sandbox_changed_files)
}

fn parse_generic_transcript(
    transcript: &str,
    sandbox_changed_files: &[String],
) -> TranscriptSummary {
    let parse_body = transcript_parse_body(transcript);
    let mut changed_files = sandbox_changed_files
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut concerns = Vec::new();
    let mut blockers = Vec::new();
    let mut usage = TranscriptUsage::default();
    let mut cost_usd = None;
    let mut transcript_ids = BTreeMap::new();
    let mut raw_summary = None;

    for line in parse_body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            merge_transcript_ids(&value, &mut transcript_ids);
            if merge_usage_metadata(&value, &mut usage, &mut cost_usd) {
                continue;
            }
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(value) = value_after_label(trimmed, &lower, &["summary"]) {
            if raw_summary.is_none() && !value.is_empty() {
                raw_summary = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = value_after_label(
            trimmed,
            &lower,
            &["changed files", "changed_files", "files changed"],
        ) {
            for file in split_file_list(value) {
                changed_files.insert(file);
            }
            continue;
        }
        if lower.starts_with("modified:")
            || lower.starts_with("added:")
            || lower.starts_with("deleted:")
        {
            if let Some((_, value)) = trimmed.split_once(':') {
                if let Some(file) = clean_file_token(value) {
                    changed_files.insert(file);
                }
            }
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["concern"]) {
            concerns.push(parse_concern(value));
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["blocker"]) {
            blockers.push(TranscriptBlocker {
                kind: "provider_reported_blocker".to_string(),
                message: value.to_string(),
            });
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["input tokens", "input_tokens"]) {
            usage.input_tokens = parse_u64(value);
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["output tokens", "output_tokens"])
        {
            usage.output_tokens = parse_u64(value);
            continue;
        }
        if let Some(value) =
            value_after_label(trimmed, &lower, &["total tokens", "total_tokens", "tokens"])
        {
            usage.total_tokens = parse_u64(value);
            continue;
        }
        if let Some(value) = value_after_label(trimmed, &lower, &["cost", "cost usd"]) {
            cost_usd = parse_cost(value);
            continue;
        }
        if let Some((kind, value)) = transcript_id_after_label(trimmed, &lower) {
            transcript_ids.insert(kind, value);
        }
    }

    TranscriptSummary {
        changed_files: changed_files.into_iter().collect(),
        concerns,
        blockers,
        usage,
        cost_usd,
        transcript_ids: transcript_ids
            .into_iter()
            .map(|(kind, value)| TranscriptId { kind, value })
            .collect(),
        raw_summary,
    }
}

fn transcript_parse_body(transcript: &str) -> &str {
    if let Some((_, after_stdout)) = transcript.split_once("\nstdout:\n") {
        return after_stdout
            .split_once("\nstderr:\n")
            .map(|(stdout, _)| stdout)
            .unwrap_or(after_stdout);
    }
    transcript
}

fn value_after_label<'a>(line: &'a str, lower: &str, labels: &[&str]) -> Option<&'a str> {
    for label in labels {
        let prefix_colon = format!("{label}:");
        let prefix_equals = format!("{label}=");
        if lower.starts_with(&prefix_colon) {
            return Some(line[prefix_colon.len()..].trim());
        }
        if lower.starts_with(&prefix_equals) {
            return Some(line[prefix_equals.len()..].trim());
        }
    }
    None
}

fn split_file_list(value: &str) -> Vec<String> {
    value
        .split([',', ';'])
        .filter_map(clean_file_token)
        .collect()
}

fn clean_file_token(value: &str) -> Option<String> {
    let cleaned = value
        .trim()
        .trim_start_matches("- ")
        .trim_matches(['`', '"', '\''])
        .replace('\\', "/");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn parse_concern(value: &str) -> TranscriptConcern {
    if let Some((severity, message)) = value.split_once(" - ") {
        TranscriptConcern {
            severity: severity.trim().to_ascii_lowercase(),
            message: message.trim().to_string(),
        }
    } else {
        TranscriptConcern {
            severity: "unspecified".to_string(),
            message: value.trim().to_string(),
        }
    }
}

fn parse_u64(value: &str) -> Option<u64> {
    value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn parse_cost(value: &str) -> Option<f64> {
    value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .ok()
}

fn transcript_id_after_label(line: &str, lower: &str) -> Option<(String, String)> {
    for label in [
        "session_id",
        "session id",
        "conversation_id",
        "conversation id",
        "transcript_id",
        "transcript id",
        "response_id",
        "response id",
        "thread_id",
        "thread id",
    ] {
        if let Some(value) =
            value_after_label(line, lower, &[label]).filter(|value| !value.is_empty())
        {
            return Some((normalize_transcript_id_kind(label), value.to_string()));
        }
    }
    None
}

fn merge_transcript_ids(value: &serde_json::Value, ids: &mut BTreeMap<String, String>) {
    for key in [
        "session_id",
        "conversation_id",
        "transcript_id",
        "response_id",
        "thread_id",
    ] {
        if let Some(text) = value.get(key).and_then(serde_json::Value::as_str) {
            if !text.trim().is_empty() {
                ids.insert(key.to_string(), text.trim().to_string());
            }
        }
    }
}

fn normalize_transcript_id_kind(label: &str) -> String {
    label.replace(' ', "_")
}

fn merge_usage_metadata(
    value: &serde_json::Value,
    usage: &mut TranscriptUsage,
    cost_usd: &mut Option<f64>,
) -> bool {
    let usage_value = value.get("usage").unwrap_or(value);
    let mut found = false;

    if let Some(value) =
        json_u64_like(usage_value, "input_tokens").or_else(|| json_u64_like(usage_value, "input"))
    {
        usage.input_tokens = Some(value);
        found = true;
    }
    if let Some(value) =
        json_u64_like(usage_value, "output_tokens").or_else(|| json_u64_like(usage_value, "output"))
    {
        usage.output_tokens = Some(value);
        found = true;
    }
    if let Some(value) =
        json_u64_like(usage_value, "total_tokens").or_else(|| json_u64_like(usage_value, "total"))
    {
        usage.total_tokens = Some(value);
        found = true;
    }
    if let Some(value) =
        json_f64_like(usage_value, "cost_usd").or_else(|| json_f64_like(usage_value, "cost"))
    {
        *cost_usd = Some(value);
        found = true;
    }

    found
}

fn json_u64_like(value: &serde_json::Value, key: &str) -> Option<u64> {
    match value.get(key)? {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => parse_u64(text),
        _ => None,
    }
}

fn json_f64_like(value: &serde_json::Value, key: &str) -> Option<f64> {
    match value.get(key)? {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => parse_cost(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_profile_preserves_ao2_sandbox_contract_and_budget_guard() {
        let args = build_args("Implement validation and tests.", Some("0.20".to_string()));

        assert_eq!(PROVIDER_NAME, "claude");
        assert!(args.contains(&"--print".to_string()));
        assert!(args.contains(&"--permission-mode".to_string()));
        assert!(args.contains(&"bypassPermissions".to_string()));
        assert!(args.contains(&"--allowedTools".to_string()));
        assert!(args.contains(&"Bash,Read,Write,Edit".to_string()));
        assert!(args.contains(&"--no-session-persistence".to_string()));
        assert!(args.contains(&"--max-budget-usd".to_string()));
        assert!(args.contains(&"0.20".to_string()));
        let prompt = args.last().unwrap();
        assert!(prompt.contains("Execute the following shell script exactly"));
        assert!(prompt.contains("Do not edit files outside the current repository"));
        assert!(prompt.contains("Summary:"));
        assert!(prompt.contains("Changed files:"));
        assert!(prompt.contains("Implement validation and tests."));
    }

    #[test]
    fn claude_crate_owns_transcript_parser_and_doctor_metadata() {
        let transcript = r#"
Summary: shell repair completed.
Changed files: src/repair.rs
Blocker: provider budget denied
Thread ID: claude-thread-9
Input tokens: 1,200
Output tokens: 300
Total tokens: 1,500
Cost: $0.042
"#;

        let summary = parse_transcript(transcript, &["src/lib.rs".to_string()]);

        assert_eq!(DOCTOR_COMMAND, "claude");
        assert_eq!(DOCTOR_ARGS, &["--version"]);
        assert_eq!(
            summary.changed_files,
            vec!["src/lib.rs".to_string(), "src/repair.rs".to_string()]
        );
        assert_eq!(
            summary.raw_summary,
            Some("shell repair completed.".to_string())
        );
        assert_eq!(summary.blockers[0].kind, "provider_reported_blocker");
        assert_eq!(summary.usage.input_tokens, Some(1200));
        assert_eq!(summary.usage.output_tokens, Some(300));
        assert_eq!(summary.usage.total_tokens, Some(1500));
        assert_eq!(summary.cost_usd, Some(0.042));
        assert!(summary
            .transcript_ids
            .iter()
            .any(|id| id.kind == "thread_id" && id.value == "claude-thread-9"));
    }
}
