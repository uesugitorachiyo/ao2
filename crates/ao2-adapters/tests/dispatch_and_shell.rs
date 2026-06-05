//! Coverage for two small-but-load-bearing dispatch/shell boundaries.
//!
//! `parse_provider` is the string → `ProviderKind` gate: every operator-facing
//! provider selection flows through it, and a wrong answer silently routes a
//! run to the wrong adapter (e.g. launching `codex` when `claude` was asked
//! for). `scripted_prompt_prefers_posix_shell` and `posix_shell_command`
//! decide which interpreter executes a sandboxed script — picking PowerShell
//! for a POSIX heredoc, or vice versa, breaks the run. Both were exercised
//! only indirectly; these pin the contracts directly.

use ao2_adapters::{
    parse_provider, posix_shell_command, scripted_prompt_prefers_posix_shell, ProviderKind,
};

#[test]
fn parse_provider_accepts_exactly_the_four_known_providers() {
    assert_eq!(parse_provider("codex").unwrap(), ProviderKind::Codex);
    assert_eq!(parse_provider("claude").unwrap(), ProviderKind::Claude);
    assert_eq!(
        parse_provider("antigravity").unwrap(),
        ProviderKind::Antigravity
    );
    assert_eq!(parse_provider("scripted").unwrap(), ProviderKind::Scripted);
}

#[test]
fn parse_provider_is_case_sensitive_and_untrimmed() {
    // The match is exact: capitalization, surrounding whitespace, and partial
    // names must all be rejected rather than coerced — there's no "did you
    // mean" normalization that could route to an unintended adapter.
    for bad in [
        "Codex", "CLAUDE", "Claude", " codex", "codex ", "cod", "claude\n",
    ] {
        assert!(
            parse_provider(bad).is_err(),
            "{bad:?} must not parse to a provider"
        );
    }
}

#[test]
fn parse_provider_error_names_the_input_and_the_valid_set() {
    let err = parse_provider("gpt").unwrap_err().to_string();
    assert!(err.contains("gpt"), "error should echo the offending input");
    // The four accepted values are listed so the operator can self-correct.
    for expected in ["codex", "claude", "antigravity", "scripted"] {
        assert!(err.contains(expected), "error should list `{expected}`");
    }
}

#[test]
fn posix_markers_select_the_posix_shell() {
    // Representative POSIX-only constructs the heuristic keys off of.
    for script in [
        "printf 'hello\\n'",
        "cat > out.txt",
        "test -f foo",
        "export FOO=bar",
        "sleep 1",
        "if [ -f x ]; then echo y; fi",
        "run_thing <<EOF\nbody\nEOF",
        "ls | grep -q needle",
        "echo \"$AO2_REPAIR_TOKEN\"",
    ] {
        assert!(
            scripted_prompt_prefers_posix_shell(script),
            "{script:?} should prefer the POSIX shell"
        );
    }
}

#[test]
fn windows_markers_decline_the_posix_shell() {
    for script in [
        "$env:FOO = 'bar'",
        "Set-Content -Path out.txt -Value hi",
        "New-Item -ItemType File foo",
        "Write-Output 'hi'",
        "Start-Sleep -Seconds 1",
        "if ($x -eq 1) { echo y }",
    ] {
        assert!(
            !scripted_prompt_prefers_posix_shell(script),
            "{script:?} should not prefer the POSIX shell"
        );
    }
}

#[test]
fn windows_markers_win_when_a_script_mixes_both_dialects() {
    // A script carrying *any* PowerShell marker is treated as Windows even if
    // it also contains POSIX-looking lines — the Windows check short-circuits
    // first, so a mixed script never gets dispatched to `sh`.
    let mixed = "printf 'posix-looking'\n$env:WINDOWS = 'true'";
    assert!(!scripted_prompt_prefers_posix_shell(mixed));
}

#[test]
fn plain_script_with_no_dialect_markers_does_not_force_posix() {
    // Neutral commands must not trip the heuristic; the default is "don't
    // assume POSIX".
    assert!(!scripted_prompt_prefers_posix_shell("echo hello"));
    assert!(!scripted_prompt_prefers_posix_shell(""));
}

#[test]
fn posix_shell_resolves_to_sh_on_posix_hosts() {
    // On any unix host (where these tests run), `sh` is on PATH and must be the
    // resolved interpreter. This guards the happy path the sandbox relies on.
    if cfg!(unix) {
        let resolved = posix_shell_command().expect("a POSIX host must resolve a shell");
        let name = resolved
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        assert!(
            name == "sh" || name == "sh.exe",
            "resolved shell should be sh, got {resolved:?}"
        );
    }
}
