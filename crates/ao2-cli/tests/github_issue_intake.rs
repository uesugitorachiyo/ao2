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

fn acquisition(args: &[&str]) -> serde_json::Value {
    let mut full_args = vec!["issue", "acquire"];
    full_args.extend_from_slice(args);
    full_args.push("--json");
    json_output(&full_args)
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

#[test]
fn plans_isolated_repository_acquisition_without_mutation() {
    let value = acquisition(&[
        "--url",
        "https://github.com/uesugitorachiyo/ao2/issues/123",
        "--upstream-url",
        "https://github.com/uesugitorachiyo/ao2.git",
        "--default-branch",
        "main",
        "--target-commit",
        "80ec5321f42d4bab17d5e64fdae6aa099ba59d4a",
    ]);
    assert_eq!(
        value["schema_version"],
        "ao2.github-issue-acquisition-plan.v0.1"
    );
    assert_eq!(value["state"], "acquisition_planned");
    assert_eq!(value["repository_identity"]["target_commit_valid"], true);
    assert_eq!(
        value["repository_identity"]["upstream_matches_issue_repository"],
        true
    );
    assert_eq!(value["acquisition"]["hooks_disabled"], true);
    assert_eq!(value["acquisition"]["safe_git_config"], true);
    assert_eq!(value["acquisition"]["controlled_remotes"], true);
    assert_eq!(value["acquisition"]["sanitized_environment"], true);
    assert_eq!(
        value["acquisition"]["dependency_network_policy"],
        "deny_by_default_operator_approved_allowlist_only"
    );
    assert_eq!(
        value["acquisition"]["mutation_policy"],
        "read_only_acquisition_plan_no_clone_or_checkout_performed_by_this_readback"
    );
    for key in [
        "github_write_performed",
        "feature_generated_pr_opened",
        "issue_write_performed",
        "maintainer_contacted",
        "provider_call_performed",
        "repository_mutated",
    ] {
        assert_eq!(value["denied_actions"][key], false, "{key}");
    }
}

#[test]
fn blocks_acquisition_when_identity_or_target_commit_drift() {
    let wrong_repo = acquisition(&[
        "--url",
        "https://github.com/uesugitorachiyo/ao2/issues/123",
        "--upstream-url",
        "https://github.com/uesugitorachiyo/other.git",
        "--target-commit",
        "80ec5321f42d4bab17d5e64fdae6aa099ba59d4a",
    ]);
    assert_eq!(wrong_repo["state"], "policy_blocked");
    assert_eq!(
        wrong_repo["repository_identity"]["upstream_matches_issue_repository"],
        false
    );

    let bad_sha = acquisition(&[
        "--url",
        "https://github.com/uesugitorachiyo/ao2/issues/123",
        "--upstream-url",
        "https://github.com/uesugitorachiyo/ao2.git",
        "--target-commit",
        "not-a-sha",
    ]);
    assert_eq!(bad_sha["state"], "policy_blocked");
    assert_eq!(bad_sha["repository_identity"]["target_commit_valid"], false);
}
