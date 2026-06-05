use ao2_policy::{
    evaluate, redact_secrets, secret_redaction_class_counts, secret_redaction_count, ToolRequest,
};

#[test]
fn policy_requires_approval_for_high_risk_side_effects() {
    let cases = [
        request(
            "git",
            "push",
            "origin main",
            &["push", "origin", "main"],
            &[],
        ),
        request("shell", "delete", "repo", &["rm", "-rf", "target"], &[]),
        request(
            "package_manager",
            "install",
            "package.json",
            &["npm", "install"],
            &[],
        ),
        request("network", "egress", "https://example.com", &["curl"], &[]),
        request(
            "filesystem",
            "read",
            "../secrets",
            &["cat", "../secrets"],
            &[],
        ),
        request(
            "filesystem",
            "write_tree",
            ".",
            &["write_tree", "."],
            &["broad_write"],
        ),
        request(
            "secret",
            "read",
            "OPENAI_API_KEY",
            &["printenv", "OPENAI_API_KEY"],
            &["raw_secret_access"],
        ),
    ];

    for case in cases {
        let decision = evaluate(&case);
        assert_eq!(
            decision.decision, "requires_approval",
            "{}:{} should require approval",
            case.tool, case.operation
        );
    }
}

#[test]
fn redaction_masks_provider_api_key_values() {
    let redacted = redact_secrets("OPENAI_API_KEY=abc123\nANTHROPIC_API_KEY=def456\nSAFE_VALUE=ok");
    assert!(redacted.contains("OPENAI_API_KEY=[REDACTED]"));
    assert!(redacted.contains("ANTHROPIC_API_KEY=[REDACTED]"));
    assert!(!redacted.contains("abc123"));
    assert!(!redacted.contains("def456"));
    assert!(redacted.contains("SAFE_VALUE=ok"));
}

#[test]
fn redaction_masks_common_support_log_secret_shapes() {
    let redacted = redact_secrets(
        "TWILIO_AUTH_TOKEN=twilio-secret\n\
         SUPABASE_SERVICE_ROLE_KEY=sb_secret_live\n\
         MCRR_AI_INTAKE_KEY=ai-intake-secret\n\
         Authorization: Bearer bearer-secret\n\
         objective: bearer sk-live-secret-token and api_token=fixture-token and ghp_should_not_leak\n\
         password: hunter2\n\
         SAFE_VALUE=ok",
    );

    assert!(redacted.contains("TWILIO_AUTH_TOKEN=[REDACTED]"));
    assert!(redacted.contains("SUPABASE_SERVICE_ROLE_KEY=[REDACTED]"));
    assert!(redacted.contains("MCRR_AI_INTAKE_KEY=[REDACTED]"));
    assert!(redacted.contains("Authorization: Bearer [REDACTED]"));
    assert!(redacted.contains("password: [REDACTED]"));
    assert!(redacted.contains("SAFE_VALUE=ok"));
    for secret in [
        "twilio-secret",
        "sb_secret_live",
        "ai-intake-secret",
        "bearer-secret",
        "sk-live-secret-token",
        "fixture-token",
        "ghp_should_not_leak",
        "hunter2",
    ] {
        assert!(!redacted.contains(secret), "{secret} should be redacted");
    }
}

#[test]
fn redaction_masks_url_query_secret_values() {
    let input = "callback=https://example.com/hook?token=url-token&access_token=access-secret&api_key=key-secret&signature=sig-secret&safe=ok";
    let redacted = redact_secrets(input);

    assert!(redacted.contains("token=[REDACTED]"));
    assert!(redacted.contains("access_token=[REDACTED]"));
    assert!(redacted.contains("api_key=[REDACTED]"));
    assert!(redacted.contains("signature=[REDACTED]"));
    assert!(redacted.contains("safe=ok"));
    assert_eq!(secret_redaction_count(input), 4);
    for secret in ["url-token", "access-secret", "key-secret", "sig-secret"] {
        assert!(!redacted.contains(secret), "{secret} should be redacted");
    }
}

#[test]
fn redaction_reports_secret_class_counts_without_values() {
    let input = "OPENAI_API_KEY=sk-secret\n\
         TWILIO_AUTH_TOKEN=twilio-secret\n\
         SUPABASE_SERVICE_ROLE_KEY=sb_secret_live\n\
         Authorization: Bearer bearer-secret\n\
         password: hunter2\n\
         callback=https://example.com/hook?token=url-token&api_key=key-secret&signature=sig-secret&safe=ok";
    let classes = secret_redaction_class_counts(input);

    assert_eq!(classes.get("provider_api_key"), Some(&1));
    assert_eq!(classes.get("auth_token"), Some(&1));
    assert_eq!(classes.get("service_role_key"), Some(&1));
    assert_eq!(classes.get("bearer_authorization"), Some(&1));
    assert_eq!(classes.get("password"), Some(&1));
    assert_eq!(classes.get("query_token"), Some(&1));
    assert_eq!(classes.get("query_api_key"), Some(&1));
    assert_eq!(classes.get("query_signature"), Some(&1));
    assert_eq!(
        classes.values().sum::<usize>(),
        secret_redaction_count(input)
    );
}

fn request(
    tool: &str,
    operation: &str,
    resource: &str,
    args: &[&str],
    side_effects: &[&str],
) -> ToolRequest {
    ToolRequest {
        principal: "role:test".to_string(),
        tool: tool.to_string(),
        operation: operation.to_string(),
        resource: resource.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        expected_side_effects: side_effects
            .iter()
            .map(|side_effect| side_effect.to_string())
            .collect(),
    }
}
