//! Coverage for the approval-granting and allow-decision paths of the local
//! policy engine. `policy_hardening.rs` already proves risky requests demand
//! approval and that secrets are redacted; these tests cover the
//! complementary, security-critical branches:
//!
//! - `grant_exact` MUST reject a ticket whose action no longer matches the
//!   request that is being approved (the exact-digest binding that stops an
//!   approved ticket from being reused for a mutated action), and MUST grant
//!   when the digests match.
//! - `evaluate` MUST return a plain `allow` (not `requires_approval`) for
//!   non-risky requests — the happy path that `policy_hardening.rs` never
//!   asserts.

use ao2_policy::{create_approval_ticket, evaluate, grant_exact, ToolRequest};

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

#[test]
fn grant_exact_rejects_action_digest_mismatch() {
    let approved_request = request(
        "git",
        "push",
        "origin main",
        &["push", "origin", "main"],
        &[],
    );
    let ticket = create_approval_ticket("run-1", &approved_request);

    // A different action (extra arg) yields a different digest: the existing
    // ticket must NOT be honored for it.
    let mutated_request = request(
        "git",
        "push",
        "origin main",
        &["push", "origin", "main", "--force"],
        &[],
    );

    let result = grant_exact(&ticket, "human:operator", &mutated_request);
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("approval digest mismatch"),
        "expected digest-mismatch rejection, got: {err}"
    );
}

#[test]
fn grant_exact_approves_when_digest_matches() {
    let request = request(
        "git",
        "push",
        "origin main",
        &["push", "origin", "main"],
        &[],
    );
    let ticket = create_approval_ticket("run-1", &request);
    assert_eq!(ticket.status, "pending");
    assert!(ticket.approver.is_none());

    let granted = grant_exact(&ticket, "human:operator", &request).unwrap();
    assert_eq!(granted.status, "approved");
    assert_eq!(granted.approver.as_deref(), Some("human:operator"));
    // Identity-preserving fields are carried over unchanged.
    assert_eq!(granted.ticket_id, ticket.ticket_id);
    assert_eq!(granted.action_digest, ticket.action_digest);
}

#[test]
fn evaluate_allows_non_risky_requests() {
    let cases = [
        request(
            "filesystem",
            "read",
            "README.md",
            &["cat", "README.md"],
            &[],
        ),
        request("shell", "status", "repo", &["git", "status"], &[]),
        request("compute", "format", "src", &["cargo", "fmt"], &[]),
    ];

    for case in cases {
        let decision = evaluate(&case);
        assert_eq!(
            decision.decision, "allow",
            "{}:{} should be allowed outright",
            case.tool, case.operation
        );
        assert_eq!(decision.reason, "allowed by local MVP policy");
        assert!(decision.approval_ticket_id.is_none());
        // The decision still records the exact request it bound to.
        assert_eq!(decision.request_digest, case.action_digest());
    }
}
