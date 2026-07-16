use std::process::Command;

fn ao2(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .output()
        .unwrap()
}

fn json_output(args: &[&str]) -> serde_json::Value {
    let output = ao2(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn rejected(url: &str) -> serde_json::Value {
    json_output(&["issue", "intake", "--url", url, "--json"])
}

#[test]
fn canonicalizes_standard_github_issue_url() {
    let value = json_output(&[
        "issue",
        "intake",
        "--url",
        "https://github.com/uesugitorachiyo/ao2/issues/123?utm_source=test#ignored",
        "--json",
    ]);
    assert_eq!(value["schema_version"], "ao2.github-issue-intake.v0.1");
    assert_eq!(value["state"], "intake_validated");
    assert_eq!(value["host"], "github.com");
    assert_eq!(value["owner"], "uesugitorachiyo");
    assert_eq!(value["repo"], "ao2");
    assert_eq!(value["issue_number"], 123);
    assert_eq!(
        value["canonical_url"],
        "https://github.com/uesugitorachiyo/ao2/issues/123"
    );
    assert_eq!(value["github_write_performed"], false);
    assert_eq!(value["feature_generated_pr_opened"], false);
}

#[test]
fn rejects_non_issue_github_surfaces() {
    assert_eq!(
        rejected("https://github.com/uesugitorachiyo/ao2/pull/123")["state"],
        "invalid_url"
    );
    assert_eq!(
        rejected("https://github.com/uesugitorachiyo/ao2/discussions/123")["state"],
        "invalid_url"
    );
    assert_eq!(
        rejected("https://github.com/uesugitorachiyo/ao2/actions/runs/123")["state"],
        "invalid_url"
    );
}

#[test]
fn rejects_unsupported_or_ambiguous_urls() {
    assert_eq!(
        rejected("https://example.com/org/repo/issues/1")["state"],
        "unsupported_host"
    );
    assert_eq!(
        rejected("https://github.com/org/repo/issues/not-a-number")["state"],
        "invalid_url"
    );
    assert_eq!(
        rejected("https://github.com/org/repo/issues/0")["state"],
        "invalid_url"
    );
    assert_eq!(
        rejected("https://github.com/org/repo/issues/1/../2")["state"],
        "invalid_url"
    );
}
