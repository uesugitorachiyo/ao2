//! Behavioural coverage for the Claude transcript parser.
//!
//! `parse_transcript` is how AO2 turns a Claude Code CLI completion report into
//! the structured `changed_files` / usage / blocker signal the rest of the
//! pipeline acts on. The in-crate unit test pins one happy-path transcript;
//! these tests pin the edge cases that decide correctness in the field:
//! stdout/stderr fencing, file-list normalisation, JSON usage metadata,
//! number/cost robustness, and graceful handling of empty or junk input.

use ao2_adapter_claude::parse_transcript;

fn parse(transcript: &str) -> ao2_adapter_claude::TranscriptSummary {
    parse_transcript(transcript, &[])
}

#[test]
fn changed_files_merge_sandbox_and_transcript_sorted_and_deduped() {
    // Sandbox already reported one file; transcript reports two more plus a
    // duplicate. Result is the sorted, de-duplicated union.
    let summary = parse_transcript(
        "Changed files: src/b.rs, src/a.rs, src/b.rs",
        &["src/c.rs".to_string(), "src/a.rs".to_string()],
    );
    assert_eq!(
        summary.changed_files,
        vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "src/c.rs".to_string(),
        ]
    );
}

#[test]
fn changed_files_only_read_from_stdout_not_stderr() {
    // The parser fences on the stdout/stderr markers. A "Changed files:" line
    // that appears in the stderr stream must not be harvested — stderr is noise,
    // not the completion report.
    let transcript = "header\nstdout:\nChanged files: real.rs\nstderr:\nChanged files: noise.rs\n";
    let summary = parse(transcript);
    assert_eq!(summary.changed_files, vec!["real.rs".to_string()]);
    assert!(!summary.changed_files.contains(&"noise.rs".to_string()));
}

#[test]
fn file_tokens_are_cleaned_and_path_separators_normalised() {
    // Backtick/quote wrapping, a leading "- " bullet, and Windows backslashes
    // all get normalised; semicolons and commas both split the list.
    let summary = parse(r#"Changed files: `src/a.rs`; - "docs\guide.md"; 'crates\x\y.rs'"#);
    assert_eq!(
        summary.changed_files,
        vec![
            "crates/x/y.rs".to_string(),
            "docs/guide.md".to_string(),
            "src/a.rs".to_string(),
        ]
    );
}

#[test]
fn modified_added_deleted_lines_contribute_files() {
    let transcript = "modified: src/m.rs\nadded: src/a.rs\ndeleted: src/d.rs\n";
    let summary = parse(transcript);
    assert_eq!(
        summary.changed_files,
        vec![
            "src/a.rs".to_string(),
            "src/d.rs".to_string(),
            "src/m.rs".to_string(),
        ]
    );
}

#[test]
fn concern_splits_severity_and_falls_back_to_unspecified() {
    let summary = parse("Concern: high - possible data loss\nConcern: just a heads up");
    assert_eq!(summary.concerns.len(), 2);
    assert_eq!(summary.concerns[0].severity, "high");
    assert_eq!(summary.concerns[0].message, "possible data loss");
    assert_eq!(summary.concerns[1].severity, "unspecified");
    assert_eq!(summary.concerns[1].message, "just a heads up");
}

#[test]
fn blocker_is_captured_with_provider_kind() {
    let summary = parse("Blocker: provider budget denied");
    assert_eq!(summary.blockers.len(), 1);
    assert_eq!(summary.blockers[0].kind, "provider_reported_blocker");
    assert_eq!(summary.blockers[0].message, "provider budget denied");
}

#[test]
fn usage_and_cost_parse_through_commas_and_currency() {
    // Thousands separators and a `$` prefix must not defeat parsing.
    let summary =
        parse("Input tokens: 1,200\nOutput tokens: 3,400\nTotal tokens: 4,600\nCost: $0.042");
    assert_eq!(summary.usage.input_tokens, Some(1200));
    assert_eq!(summary.usage.output_tokens, Some(3400));
    assert_eq!(summary.usage.total_tokens, Some(4600));
    assert_eq!(summary.cost_usd, Some(0.042));
}

#[test]
fn json_usage_object_is_parsed_including_nested_and_string_numbers() {
    // A nested `usage` object with string-typed numbers (a common provider
    // serialisation) is coerced correctly. The cost is read from within the
    // same `usage` object the token counts come from.
    let transcript =
        r#"{"usage":{"input_tokens":"10","output_tokens":20,"total_tokens":30,"cost_usd":"0.5"}}"#;
    let summary = parse(transcript);
    assert_eq!(summary.usage.input_tokens, Some(10));
    assert_eq!(summary.usage.output_tokens, Some(20));
    assert_eq!(summary.usage.total_tokens, Some(30));
    assert_eq!(summary.cost_usd, Some(0.5));
}

#[test]
fn transcript_ids_extracted_from_json_and_text_lines() {
    let json_summary = parse(r#"{"session_id":"sess-1","conversation_id":"conv-2"}"#);
    assert!(json_summary
        .transcript_ids
        .iter()
        .any(|id| id.kind == "session_id" && id.value == "sess-1"));
    assert!(json_summary
        .transcript_ids
        .iter()
        .any(|id| id.kind == "conversation_id" && id.value == "conv-2"));

    let text_summary = parse("Response ID: resp-9");
    assert!(text_summary
        .transcript_ids
        .iter()
        .any(|id| id.kind == "response_id" && id.value == "resp-9"));
}

#[test]
fn first_summary_line_wins() {
    let summary = parse("Summary: the real summary\nSummary: a later override");
    assert_eq!(summary.raw_summary, Some("the real summary".to_string()));
}

#[test]
fn empty_and_whitespace_only_transcript_yields_empty_summary() {
    let summary = parse("   \n\n\t\n");
    assert!(summary.changed_files.is_empty());
    assert!(summary.concerns.is_empty());
    assert!(summary.blockers.is_empty());
    assert_eq!(summary.cost_usd, None);
    assert_eq!(summary.usage.input_tokens, None);
    assert!(summary.raw_summary.is_none());
}

#[test]
fn junk_and_unicode_input_does_not_panic() {
    // Defensive: arbitrary noisy provider output must never panic the parser.
    for transcript in [
        "🦀 changed files: src/🦀.rs",
        "Cost: not-a-number",
        "Input tokens:",
        "{malformed json",
        "::::",
        "Changed files:",
    ] {
        let _ = parse(transcript);
    }
}

#[test]
fn equals_delimited_labels_are_also_supported() {
    // Labels accept `=` as well as `:`.
    let summary = parse("changed_files=src/eq.rs\ninput_tokens=42");
    assert_eq!(summary.changed_files, vec!["src/eq.rs".to_string()]);
    assert_eq!(summary.usage.input_tokens, Some(42));
}
