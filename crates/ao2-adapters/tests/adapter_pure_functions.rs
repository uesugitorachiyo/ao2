//! Coverage for the small, decision-bearing helper functions in
//! `ao2-adapters` that the existing integration tests reach only indirectly
//! (or not at all):
//!
//! - `parse_provider` — the CLI/string → `ProviderKind` gate. Untested
//!   directly; only `parse_provider_transcript` was exercised.
//! - `scripted_prompt_prefers_posix_shell` — picks POSIX `sh` vs. the native
//!   shell for a scripted prompt. Its Windows-marker veto and POSIX-marker
//!   detection decide which interpreter runs a deterministic role on Windows.
//! - `antigravity_sandbox_execution_prompt` — wraps a task in the sandbox
//!   execution contract; the wording is a behavioral contract for the model.
//! - `AdapterRunResult::scripted` — the deterministic no-provider result
//!   constructor.
//! - `posix_shell_command` — resolves the POSIX shell used for scripted roles.
//!
//! These are pure (or environment-only) functions with branchy logic; pinning
//! them directly catches regressions that an end-to-end run would only surface
//! as a confusing downstream failure.

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use ao2_adapters::posix_shell_command;
use ao2_adapters::{
    antigravity_sandbox_execution_prompt, parse_provider, scripted_prompt_prefers_posix_shell,
    AdapterRunResult, ProviderKind,
};

#[test]
fn parse_provider_accepts_every_known_provider() {
    assert_eq!(parse_provider("codex").unwrap(), ProviderKind::Codex);
    assert_eq!(parse_provider("claude").unwrap(), ProviderKind::Claude);
    assert_eq!(
        parse_provider("antigravity").unwrap(),
        ProviderKind::Antigravity
    );
    assert_eq!(parse_provider("scripted").unwrap(), ProviderKind::Scripted);
}

#[test]
fn parse_provider_rejects_unknown_and_names_the_alternatives() {
    let err = parse_provider("gpt5").unwrap_err().to_string();
    assert!(err.contains("unknown provider: gpt5"), "got: {err}");
    // The error must enumerate the valid options so a typo is self-correcting.
    for expected in ["codex", "claude", "antigravity", "scripted"] {
        assert!(
            err.contains(expected),
            "error should list {expected}: {err}"
        );
    }
}

#[test]
fn parse_provider_is_case_sensitive_and_does_not_trim() {
    // The match is exact: capitalization or surrounding whitespace is unknown.
    assert!(parse_provider("Codex").is_err());
    assert!(parse_provider(" codex").is_err());
    assert!(parse_provider("").is_err());
}

#[test]
fn scripted_prompt_prefers_posix_when_posix_markers_present() {
    // Heredoc, printf, test, export, sleep, `if [`, grep -q, repair vars.
    assert!(scripted_prompt_prefers_posix_shell("printf 'hi\\n'"));
    assert!(scripted_prompt_prefers_posix_shell("cat > out.txt"));
    assert!(scripted_prompt_prefers_posix_shell("export FOO=bar"));
    assert!(scripted_prompt_prefers_posix_shell("sleep 1"));
    assert!(scripted_prompt_prefers_posix_shell(
        "if [ -f x ]; then :; fi"
    ));
    assert!(scripted_prompt_prefers_posix_shell("cat <<EOF\nbody\nEOF"));
    assert!(scripted_prompt_prefers_posix_shell("grep -q needle file"));
    assert!(scripted_prompt_prefers_posix_shell(
        "echo $AO2_REPAIR_REASON"
    ));
}

#[test]
fn scripted_prompt_rejects_posix_when_windows_markers_present() {
    // A PowerShell marker vetoes POSIX selection even if POSIX markers also
    // appear — running a PowerShell script under `sh` would corrupt it.
    assert!(!scripted_prompt_prefers_posix_shell("$env:FOO = 'bar'"));
    assert!(!scripted_prompt_prefers_posix_shell(
        "Set-Content out.txt 'x'"
    ));
    assert!(!scripted_prompt_prefers_posix_shell(
        "New-Item -ItemType File x"
    ));
    assert!(!scripted_prompt_prefers_posix_shell("Write-Output 'hi'"));
    assert!(!scripted_prompt_prefers_posix_shell("Start-Sleep 1"));
    assert!(!scripted_prompt_prefers_posix_shell("if ($x -eq 1) { }"));

    // Windows veto wins over a co-occurring POSIX marker.
    let mixed = "$env:FOO = 'bar'\nexport BAZ=qux";
    assert!(!scripted_prompt_prefers_posix_shell(mixed));
}

#[test]
fn scripted_prompt_defaults_to_native_shell_without_markers() {
    // No POSIX and no Windows markers: do not force POSIX.
    assert!(!scripted_prompt_prefers_posix_shell("echo hello world"));
    assert!(!scripted_prompt_prefers_posix_shell(""));
}

#[test]
fn antigravity_prompt_embeds_task_and_sandbox_contract() {
    let task = "rename the widget module and update its imports";
    let prompt = antigravity_sandbox_execution_prompt(task);

    // The caller's task is embedded verbatim.
    assert!(prompt.contains(task), "task body must be embedded");
    // The sandbox execution contract the model must obey.
    assert!(prompt.contains("disposable sandbox"));
    assert!(prompt.contains("Do not ask follow-up questions."));
    assert!(prompt.contains("Do not edit files outside the current repository."));
    // The structured completion-report fields downstream parsing depends on.
    for field in [
        "Summary:",
        "Changed files:",
        "Concern:",
        "Blocker:",
        "Task:",
    ] {
        assert!(prompt.contains(field), "prompt must prompt for {field}");
    }
}

#[test]
fn scripted_run_result_is_deterministic_and_provider_free() {
    let result = AdapterRunResult::scripted("evaluator-closer", "TRANSCRIPT BODY");

    assert_eq!(result.provider, ProviderKind::Scripted);
    assert_eq!(result.role_id, "evaluator-closer");
    assert_eq!(result.command, "scripted://deterministic-role");
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    assert_eq!(result.transcript, "TRANSCRIPT BODY");
    assert!(
        result.blocker.is_none(),
        "a scripted result is never blocked"
    );
}

#[cfg(unix)]
#[test]
fn posix_shell_command_resolves_sh_on_unix() {
    // `sh` is guaranteed on every supported unix host, so the resolver must
    // find it on PATH and return the bare `sh` name (not an absolute probe).
    assert_eq!(posix_shell_command(), Some(PathBuf::from("sh")));
}
