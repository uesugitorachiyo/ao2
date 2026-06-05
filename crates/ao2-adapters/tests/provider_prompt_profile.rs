use std::fs;

use ao2_adapters::{
    build_provider_prompt_command, parse_provider_transcript, run_provider_prompt_in_sandbox,
    ProviderKind, ProviderPromptRequest,
};

#[test]
fn provider_prompt_profiles_build_safe_codex_and_claude_invocations() {
    let codex = build_provider_prompt_command(
        ProviderKind::Codex,
        "Implement validation and tests.",
        "implementer",
        Some(900_000),
        Some(0.20),
    )
    .unwrap();
    assert_eq!(codex.command.to_string_lossy(), "codex");
    assert_eq!(codex.role_id, "implementer");
    assert!(codex.args.contains(&"exec".to_string()));
    assert!(codex.args.contains(&"--skip-git-repo-check".to_string()));
    assert!(codex.args.contains(&"--sandbox".to_string()));
    assert!(codex.args.contains(&"workspace-write".to_string()));
    assert!(codex.args.contains(&"--ephemeral".to_string()));
    assert!(!codex.args.contains(&"--ask-for-approval".to_string()));
    assert!(!codex.args.contains(&"--max-budget-usd".to_string()));
    let codex_prompt = codex.args.last().unwrap();
    assert!(codex_prompt.contains("You are running inside an AO2 disposable sandbox copy"));
    assert!(codex_prompt.contains("Do not edit files outside the current repository"));
    assert!(codex_prompt.contains("Summary:"));
    assert!(codex_prompt.contains("Changed files:"));
    assert!(codex_prompt.contains("Concern:"));
    assert!(codex_prompt.contains("Blocker:"));
    assert!(codex_prompt.contains("Implement validation and tests."));

    let claude = build_provider_prompt_command(
        ProviderKind::Claude,
        "Implement validation and tests.",
        "implementer",
        Some(900_000),
        Some(0.20),
    )
    .unwrap();
    assert_eq!(claude.command.to_string_lossy(), "claude");
    assert_eq!(claude.role_id, "implementer");
    assert!(claude.args.contains(&"--print".to_string()));
    assert!(claude.args.contains(&"--permission-mode".to_string()));
    assert!(claude.args.contains(&"bypassPermissions".to_string()));
    assert!(claude.args.contains(&"--allowedTools".to_string()));
    assert!(claude.args.contains(&"Bash,Read,Write,Edit".to_string()));
    assert!(claude.args.contains(&"--max-budget-usd".to_string()));
    assert!(claude.args.contains(&"0.20".to_string()));
    assert!(claude
        .args
        .contains(&"--no-session-persistence".to_string()));
    let claude_prompt = claude.args.last().unwrap();
    assert!(claude_prompt.contains("Execute the following shell script exactly"));
    assert!(claude_prompt.contains("Do not ask follow-up questions"));
    assert!(claude_prompt.contains("Implement validation and tests."));
    assert!(claude_prompt.contains("Summary:"));
    assert!(claude_prompt.contains("Changed files:"));
}

#[test]
fn scripted_provider_prompt_runs_in_sandbox_without_mutating_target() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("value.txt"), "before\n").unwrap();

    let result = run_provider_prompt_in_sandbox(ProviderPromptRequest {
        provider: ProviderKind::Scripted,
        target_repo: target.clone(),
        prompt: "printf 'after\\n' > value.txt".to_string(),
        role_id: "scripted-profile".to_string(),
        keep_sandbox: false,
        timeout_ms: Some(900_000),
        max_budget_usd: None,
    })
    .unwrap();

    assert!(result.adapter.blocker.is_none());
    assert_eq!(result.adapter.provider, ProviderKind::Scripted);
    assert_eq!(
        fs::read_to_string(target.join("value.txt")).unwrap(),
        "before\n"
    );
    assert_eq!(result.changed_files, vec!["value.txt"]);
    assert!(result.diff_summary.contains("modified: value.txt"));
    assert_eq!(result.transcript_summary.changed_files, vec!["value.txt"]);
}

#[test]
fn sandbox_prompt_ignores_generated_and_dependency_directories() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("node_modules/pkg")).unwrap();
    fs::create_dir_all(target.join(".git/hooks")).unwrap();
    fs::create_dir_all(target.join(".venv/bin")).unwrap();
    fs::create_dir_all(target.join("src")).unwrap();
    fs::write(target.join("node_modules/pkg/index.js"), "before\n").unwrap();
    fs::write(target.join(".git/config"), "before\n").unwrap();
    fs::write(target.join(".venv/pyvenv.cfg"), "before\n").unwrap();
    fs::write(target.join("src/value.txt"), "before\n").unwrap();

    let result = run_provider_prompt_in_sandbox(ProviderPromptRequest {
        provider: ProviderKind::Scripted,
        target_repo: target.clone(),
        prompt: r#"
test ! -e node_modules/pkg/index.js
test ! -e .git/config
test ! -e .venv/pyvenv.cfg
printf 'after\n' > src/value.txt
"#
        .to_string(),
        role_id: "scripted-profile".to_string(),
        keep_sandbox: false,
        timeout_ms: Some(900_000),
        max_budget_usd: None,
    })
    .unwrap();

    assert!(result.adapter.blocker.is_none());
    assert_eq!(result.changed_files, vec!["src/value.txt"]);
    assert_eq!(
        fs::read_to_string(target.join("node_modules/pkg/index.js")).unwrap(),
        "before\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("src/value.txt")).unwrap(),
        "before\n"
    );
}

#[test]
fn provider_transcript_parser_extracts_changes_concerns_blockers_and_usage() {
    let transcript = r#"
Summary: added validation around discount math.
Changed files: discount_service/discounts.py, tests/test_discounts.py
Concern: medium - tests cover boundary cases but not type errors
Blocker: package install denied by policy
Input tokens: 1200
Output tokens: 340
Total tokens: 1540
Cost: $0.042
"#;

    let summary = parse_provider_transcript(
        ProviderKind::Codex,
        transcript,
        &["discount_service/discounts.py".to_string()],
    );

    assert_eq!(summary.provider, ProviderKind::Codex);
    assert_eq!(
        summary.changed_files,
        vec![
            "discount_service/discounts.py".to_string(),
            "tests/test_discounts.py".to_string()
        ]
    );
    assert_eq!(summary.concerns.len(), 1);
    assert_eq!(summary.concerns[0].severity, "medium");
    assert_eq!(
        summary.concerns[0].message,
        "tests cover boundary cases but not type errors"
    );
    assert_eq!(summary.blockers.len(), 1);
    assert_eq!(summary.blockers[0].kind, "provider_reported_blocker");
    assert_eq!(summary.usage.input_tokens, Some(1200));
    assert_eq!(summary.usage.output_tokens, Some(340));
    assert_eq!(summary.usage.total_tokens, Some(1540));
    assert_eq!(summary.cost_usd, Some(0.042));
    assert_eq!(
        summary.raw_summary,
        Some("added validation around discount math.".to_string())
    );
}

#[test]
fn codex_transcript_parser_normalizes_json_usage_metadata() {
    let transcript = r#"
Summary: updated retry handling.
Changed files: src/retry.rs
{"session_id":"codex-session-123","usage":{"input_tokens":2400,"output_tokens":620,"total_tokens":3020,"cost_usd":0.073}}
"#;

    let summary = parse_provider_transcript(ProviderKind::Codex, transcript, &[]);

    assert_eq!(summary.usage.input_tokens, Some(2400));
    assert_eq!(summary.usage.output_tokens, Some(620));
    assert_eq!(summary.usage.total_tokens, Some(3020));
    assert_eq!(summary.cost_usd, Some(0.073));
    assert!(summary
        .transcript_ids
        .iter()
        .any(|id| id.kind == "session_id" && id.value == "codex-session-123"));
}

#[test]
fn provider_transcript_parser_ignores_prompt_templates_in_command_section() {
    let transcript = r#"
provider: Codex
role_id: implementer
command: codex exec At the end, print:
  Summary: <short summary>
  Changed files: <comma-separated files>
  Concern: <severity - message, only if any>
  Blocker: <message, only if any>
exit_code: Some(2)
stdout:

stderr:
error: unexpected argument
"#;

    let summary = parse_provider_transcript(ProviderKind::Codex, transcript, &[]);

    assert_eq!(summary.changed_files, Vec::<String>::new());
    assert!(summary.concerns.is_empty());
    assert!(summary.blockers.is_empty());
    assert_eq!(summary.raw_summary, None);
}

#[test]
fn provider_prompt_profiles_build_safe_antigravity_invocations() {
    let antigravity = build_provider_prompt_command(
        ProviderKind::Antigravity,
        "Implement validation and tests.",
        "implementer",
        Some(900_000),
        None,
    )
    .unwrap();
    assert_eq!(antigravity.command.to_string_lossy(), "agy");
    assert_eq!(antigravity.role_id, "implementer");
    assert!(antigravity.args.contains(&"--add-dir".to_string()));
    assert!(antigravity.args.contains(&"--print".to_string()));
    assert!(antigravity.args.contains(&"--sandbox".to_string()));
    assert!(antigravity.args.contains(&"--print-timeout".to_string()));
    assert!(antigravity.args.contains(&"5m".to_string()));
    assert!(antigravity
        .args
        .contains(&"--dangerously-skip-permissions".to_string()));
    let print_index = antigravity
        .args
        .iter()
        .position(|arg| arg == "--print")
        .expect("antigravity print flag");
    let add_dir_index = antigravity
        .args
        .iter()
        .position(|arg| arg == "--add-dir")
        .expect("antigravity workspace flag");
    assert_eq!(
        antigravity.args[add_dir_index + 1],
        "{ao2_adapter_working_dir}",
        "agy needs an absolute --add-dir injected after AO2 resolves the disposable sandbox path"
    );
    assert!(
        antigravity.args[print_index + 1].contains("Implement validation and tests."),
        "agy treats the argument immediately after --print as the prompt"
    );
    let antigravity_prompt = &antigravity.args[print_index + 1];
    assert!(antigravity_prompt.contains("You are running inside an AO2 disposable sandbox copy"));
    assert!(antigravity_prompt.contains("Do not edit files outside the current repository"));
    assert!(antigravity_prompt.contains("Summary:"));
    assert!(antigravity_prompt.contains("Changed files:"));
    assert!(antigravity_prompt.contains("Concern:"));
    assert!(antigravity_prompt.contains("Blocker:"));
    assert!(antigravity_prompt.contains("Implement validation and tests."));
}
