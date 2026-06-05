//! Safety-edge coverage for the Claude adapter profile.
//!
//! `build_args` decides what the Claude CLI is actually launched with — the
//! sandbox permission mode, the allowed-tools allow-list, and the budget guard.
//! The inline tests cover the with-budget happy path with loose `contains()`
//! checks; these pin the structural bindings (a flag must carry its intended
//! value, not just appear somewhere), the no-budget path, and prompt
//! positioning. `parse_transcript` turns untrusted provider output into the
//! TranscriptSummary that feeds evidence — its malformed/framed/JSON paths were
//! unexercised.

use ao2_adapter_claude::{build_args, parse_transcript, sandbox_execution_prompt};

/// The fixed safety preamble `build_args` always emits, before any budget arg
/// and the trailing prompt.
const SAFETY_PREFIX: &[&str] = &[
    "--print",
    "--permission-mode",
    "bypassPermissions",
    "--allowedTools",
    "Bash,Read,Write,Edit",
    "--no-session-persistence",
    "--output-format",
    "text",
];

/// Index of the value immediately following the first occurrence of `flag`.
fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

#[test]
fn build_args_without_budget_omits_the_budget_flag_and_ends_with_the_prompt() {
    let args = build_args("do the thing", None);

    // Exact structure: the fixed safety preamble, then the prompt — nothing
    // else, and crucially no budget flag when none was requested.
    assert_eq!(&args[..SAFETY_PREFIX.len()], SAFETY_PREFIX);
    assert_eq!(args.len(), SAFETY_PREFIX.len() + 1);
    assert!(
        !args.iter().any(|a| a == "--max-budget-usd"),
        "no budget flag must be emitted when budget is None"
    );
    assert_eq!(
        args.last().unwrap(),
        &sandbox_execution_prompt("do the thing")
    );
}

#[test]
fn build_args_binds_each_safety_flag_to_its_intended_value() {
    // `contains()` can't tell `--allowedTools X` from `X ... --allowedTools`.
    // Assert each flag carries exactly its intended value, so the sandbox can
    // never be launched with a broader tool set or a weakened permission mode.
    let args = build_args("script", Some("0.50".to_string()));

    assert_eq!(
        value_after(&args, "--permission-mode"),
        Some("bypassPermissions")
    );
    assert_eq!(
        value_after(&args, "--allowedTools"),
        Some("Bash,Read,Write,Edit"),
        "allowed-tools must be exactly this set — no broadening"
    );
    assert_eq!(value_after(&args, "--output-format"), Some("text"));
    assert_eq!(value_after(&args, "--max-budget-usd"), Some("0.50"));
}

#[test]
fn build_args_places_budget_immediately_before_the_trailing_prompt() {
    let args = build_args("script", Some("1.25".to_string()));
    // Order: [..safety prefix.., --max-budget-usd, <value>, <prompt>]
    assert_eq!(&args[..SAFETY_PREFIX.len()], SAFETY_PREFIX);
    assert_eq!(args[SAFETY_PREFIX.len()], "--max-budget-usd");
    assert_eq!(args[SAFETY_PREFIX.len() + 1], "1.25");
    assert_eq!(args.last().unwrap(), &sandbox_execution_prompt("script"));
    assert_eq!(args.len(), SAFETY_PREFIX.len() + 3);
}

#[test]
fn sandbox_prompt_embeds_the_script_and_the_guardrails() {
    let prompt = sandbox_execution_prompt("rm -rf build && make");
    // The user script is embedded verbatim inside the fenced shell block.
    assert!(prompt.contains("```sh\nrm -rf build && make\n```"));
    // The non-negotiable guardrails and the structured-report contract are
    // present so the provider's output can be parsed back.
    assert!(prompt.contains("disposable sandbox"));
    assert!(prompt.contains("Do not edit files outside the current repository"));
    assert!(prompt.contains("Summary:"));
    assert!(prompt.contains("Changed files:"));
    assert!(prompt.contains("Concern:"));
    assert!(prompt.contains("Blocker:"));
}

// ---- parse_transcript edges ---------------------------------------------

#[test]
fn parse_transcript_empty_yields_defaults_but_keeps_sandbox_files() {
    let empty = parse_transcript("", &[]);
    assert!(empty.changed_files.is_empty());
    assert!(empty.concerns.is_empty());
    assert!(empty.blockers.is_empty());
    assert!(empty.raw_summary.is_none());
    assert!(empty.cost_usd.is_none());
    assert_eq!(empty.usage, Default::default());

    // The sandbox-observed file list is always carried through, even with no
    // parseable transcript content.
    let with_sandbox = parse_transcript("   \n\n  ", &["src/a.rs".to_string()]);
    assert_eq!(with_sandbox.changed_files, vec!["src/a.rs".to_string()]);
}

#[test]
fn parse_transcript_reads_only_the_stdout_section_when_framed() {
    // When the transcript is framed with stdout/stderr sections, only stdout is
    // parsed — stderr noise must not leak into the summary.
    let framed = "exit: 0\n\
                  stdout:\n\
                  Summary: from stdout\n\
                  Changed files: stdout_only.rs\n\
                  stderr:\n\
                  Changed files: stderr_only.rs\n";
    let summary = parse_transcript(framed, &[]);
    assert_eq!(summary.raw_summary, Some("from stdout".to_string()));
    assert_eq!(summary.changed_files, vec!["stdout_only.rs".to_string()]);
    assert!(
        !summary.changed_files.iter().any(|f| f == "stderr_only.rs"),
        "stderr content must not be parsed"
    );
}

#[test]
fn parse_transcript_concern_severity_with_and_without_separator() {
    let with_sep = parse_transcript("Concern: high - validation missing", &[]);
    assert_eq!(with_sep.concerns.len(), 1);
    assert_eq!(with_sep.concerns[0].severity, "high");
    assert_eq!(with_sep.concerns[0].message, "validation missing");

    let without_sep = parse_transcript("Concern: just a heads up", &[]);
    assert_eq!(without_sep.concerns[0].severity, "unspecified");
    assert_eq!(without_sep.concerns[0].message, "just a heads up");
}

#[test]
fn parse_transcript_collects_modified_added_deleted_file_lines_sorted_and_deduped() {
    let transcript = "Modified: src/z.rs\n\
                      Added: src/a.rs\n\
                      Deleted: src/a.rs\n\
                      Changed files: dir\\win.rs, src/z.rs";
    let summary = parse_transcript(transcript, &[]);
    // Backslashes normalized to forward slashes; duplicates collapsed; sorted.
    assert_eq!(
        summary.changed_files,
        vec![
            "dir/win.rs".to_string(),
            "src/a.rs".to_string(),
            "src/z.rs".to_string(),
        ]
    );
}

#[test]
fn parse_transcript_reads_usage_and_ids_from_json_lines() {
    // Providers may emit a JSON metadata line; usage, cost, and transcript ids
    // must be extracted from it (not only from "Label: value" lines).
    let transcript = r#"{"session_id":"sess-42","usage":{"input_tokens":11,"output_tokens":22,"total_tokens":33,"cost_usd":0.07}}"#;
    let summary = parse_transcript(transcript, &[]);
    assert_eq!(summary.usage.input_tokens, Some(11));
    assert_eq!(summary.usage.output_tokens, Some(22));
    assert_eq!(summary.usage.total_tokens, Some(33));
    assert_eq!(summary.cost_usd, Some(0.07));
    assert!(summary
        .transcript_ids
        .iter()
        .any(|id| id.kind == "session_id" && id.value == "sess-42"));
}

#[test]
fn parse_transcript_without_summary_leaves_raw_summary_none() {
    let summary = parse_transcript("Changed files: only.rs\nBlocker: nope", &[]);
    assert!(summary.raw_summary.is_none());
    assert_eq!(summary.changed_files, vec!["only.rs".to_string()]);
    assert_eq!(summary.blockers.len(), 1);
    assert_eq!(summary.blockers[0].kind, "provider_reported_blocker");
}
