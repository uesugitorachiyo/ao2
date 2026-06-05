//! Decision-path coverage for the deny-by-default policy core.
//!
//! `evaluate` / `deny` / `create_approval_ticket` / `grant_exact` are the gate
//! between an agent's intent and an action that touches the world. The existing
//! suite proves the happy path (non-risky → allow), a bundle of high-risk side
//! effects → requires_approval, and the grant_exact digest check. What it does
//! *not* pin:
//!
//! - each individual risk trigger in `is_risky`, fired in isolation against a
//!   proven-benign baseline (the bundled cases conflate several triggers, so a
//!   regression that dropped one branch could still pass);
//! - the near-miss boundaries that must stay `allow` (e.g. `git status`, where
//!   only `git push` is risky) — the guard against over-broad denial;
//! - the full decision envelope on the `requires_approval` and `deny` paths
//!   (action formatting, digest binding, policy version, fresh ids);
//! - `deny`'s output, which has no test at all;
//! - `create_approval_ticket`'s fields and its 30-minute expiry window;
//! - `action_digest` determinism and per-field sensitivity — the property the
//!   exact-approval security model rests on.

use ao2_policy::{
    create_approval_ticket, deny, evaluate, grant_exact, ToolRequest, POLICY_VERSION,
};

fn request(
    tool: &str,
    operation: &str,
    resource: &str,
    args: &[&str],
    side_effects: &[&str],
) -> ToolRequest {
    ToolRequest {
        principal: "agent:test".to_string(),
        tool: tool.to_string(),
        operation: operation.to_string(),
        resource: resource.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        expected_side_effects: side_effects.iter().map(|s| s.to_string()).collect(),
    }
}

/// A request with no risk signal whatsoever: read tool, read op, no traversal,
/// no destructive args, no risky side effects. Used both as a positive `allow`
/// case and as the baseline that each isolated trigger is layered onto.
fn benign() -> ToolRequest {
    request(
        "filesystem",
        "read",
        "README.md",
        &["cat", "README.md"],
        &[],
    )
}

#[test]
fn benign_baseline_is_allowed() {
    // If this ever flips, every "isolated trigger" assertion below is suspect:
    // the trigger could be incidental, not causal.
    let decision = evaluate(&benign());
    assert_eq!(decision.decision, "allow");
}

#[test]
fn each_risk_trigger_requires_approval_in_isolation() {
    // (label, request) — every case is the benign baseline with exactly one
    // risk signal turned on, so a passing assertion attributes the
    // `requires_approval` to that specific branch of `is_risky`.
    let cases = [
        (
            "git push",
            request("git", "push", "origin", &["origin"], &[]),
        ),
        (
            "network tool",
            request("network", "read", "host", &["ping"], &[]),
        ),
        (
            "package_manager tool",
            request("package_manager", "list", "pkgs", &["ls"], &[]),
        ),
        (
            "secret tool",
            request("secret", "read", "vault", &["read"], &[]),
        ),
        (
            "write_tree operation",
            request("filesystem", "write_tree", "dir", &["apply"], &[]),
        ),
        (
            "operation contains delete",
            request("storage", "delete_object", "obj", &["go"], &[]),
        ),
        (
            "rm -rf in args",
            request("shell", "run", "tmp", &["rm", "-rf", "tmp"], &[]),
        ),
        (
            "path traversal in args",
            request("filesystem", "read", "f", &["cat", "../etc/shadow"], &[]),
        ),
        (
            "side effect: external_write",
            request("custom", "act", "r", &["x"], &["external_write"]),
        ),
        (
            "side effect: network_egress",
            request("custom", "act", "r", &["x"], &["network_egress"]),
        ),
        (
            "side effect: destructive",
            request("custom", "act", "r", &["x"], &["destructive"]),
        ),
        (
            "side effect: package_install",
            request("custom", "act", "r", &["x"], &["package_install"]),
        ),
        (
            "side effect: broad_write",
            request("custom", "act", "r", &["x"], &["broad_write"]),
        ),
        (
            "side effect: raw_secret_access",
            request("custom", "act", "r", &["x"], &["raw_secret_access"]),
        ),
    ];

    for (label, case) in cases {
        let decision = evaluate(&case);
        assert_eq!(
            decision.decision, "requires_approval",
            "trigger `{label}` must escalate to requires_approval"
        );
        assert_eq!(
            decision.reason, "risky side effect requires exact-digest human approval",
            "trigger `{label}` must carry the escalation reason"
        );
    }
}

#[test]
fn near_miss_requests_stay_allowed() {
    // Each of these is adjacent to a trigger but must NOT escalate — proof the
    // policy isn't over-broad. `git` is only risky on `push`; an unknown side
    // effect string isn't in the escalation set; a benign tool/op is allowed.
    let cases = [
        (
            "git status (only push is risky)",
            request("git", "status", "repo", &["status"], &[]),
        ),
        (
            "git commit",
            request("git", "commit", "repo", &["commit", "-m", "x"], &[]),
        ),
        (
            "unrecognized side effect",
            request("custom", "act", "r", &["x"], &["telemetry_emit"]),
        ),
        (
            "plain read",
            request("filesystem", "read", "a.txt", &["cat", "a.txt"], &[]),
        ),
    ];

    for (label, case) in cases {
        let decision = evaluate(&case);
        assert_eq!(
            decision.decision, "allow",
            "`{label}` must remain allow, not escalate"
        );
        assert_eq!(decision.reason, "allowed by local MVP policy");
    }
}

#[test]
fn evaluate_allow_envelope_is_fully_bound() {
    let req = benign();
    let decision = evaluate(&req);

    assert_eq!(decision.decision, "allow");
    assert_eq!(decision.principal, "agent:test");
    // action is "tool:operation".
    assert_eq!(decision.action, "filesystem:read");
    assert_eq!(decision.resource, "README.md");
    // The decision is cryptographically bound to the exact request.
    assert_eq!(decision.request_digest, req.action_digest());
    assert_eq!(decision.policy_version, POLICY_VERSION);
    // No ticket is minted on the allow path.
    assert!(decision.approval_ticket_id.is_none());
    assert!(decision.decision_id.starts_with("pol-"));
}

#[test]
fn evaluate_mints_a_fresh_decision_id_each_call() {
    let req = benign();
    let first = evaluate(&req);
    let second = evaluate(&req);
    // Same request → identical digest, but each decision is independently
    // identifiable for the audit log.
    assert_eq!(first.request_digest, second.request_digest);
    assert_ne!(
        first.decision_id, second.decision_id,
        "every decision must get its own id"
    );
}

#[test]
fn deny_envelope_is_fully_bound() {
    // `deny` had no direct coverage. It must stamp decision="deny", echo the
    // caller's reason verbatim, bind the request digest, and mint no ticket.
    let req = request("network", "egress", "https://x", &["curl"], &[]);
    let decision = deny(&req, "blocked by allowlist");

    assert_eq!(decision.decision, "deny");
    assert_eq!(decision.reason, "blocked by allowlist");
    assert_eq!(decision.action, "network:egress");
    assert_eq!(decision.resource, "https://x");
    assert_eq!(decision.request_digest, req.action_digest());
    assert_eq!(decision.policy_version, POLICY_VERSION);
    assert!(decision.approval_ticket_id.is_none());
    assert!(decision.decision_id.starts_with("pol-"));
}

#[test]
fn approval_ticket_envelope_and_expiry_window() {
    let req = request("git", "push", "origin main", &["push"], &["external_write"]);
    let ticket = create_approval_ticket("run-42", &req);

    assert!(ticket.ticket_id.starts_with("approval-"));
    assert_eq!(ticket.run_id, "run-42");
    assert_eq!(ticket.requested_action, "git:push");
    // The ticket is bound to the exact action it approves.
    assert_eq!(ticket.action_digest, req.action_digest());
    assert_eq!(ticket.risk_class, "external_write");
    assert_eq!(ticket.requester, "agent:test");
    assert_eq!(ticket.scope, "origin main");
    // A freshly created ticket is unapproved and pending.
    assert!(ticket.approver.is_none());
    assert_eq!(ticket.status, "pending");
    // Expiry is a bounded ~30-minute window after creation, so a stale approval
    // can't be replayed indefinitely. `created_at` and `expires_at` sample the
    // clock separately, so the delta is 30 minutes plus the sub-millisecond
    // drift between those two calls — assert the window, not an exact equality.
    let window = ticket.expires_at - ticket.created_at;
    assert!(
        window >= chrono::Duration::minutes(30)
            && window < chrono::Duration::minutes(30) + chrono::Duration::seconds(1),
        "approval window should be ~30 minutes, got {window}"
    );
}

#[test]
fn action_digest_is_deterministic_and_field_sensitive() {
    let base = request("git", "push", "origin", &["a", "b"], &["external_write"]);
    let digest = base.action_digest();

    // Deterministic: same content → same digest.
    assert_eq!(digest, base.action_digest());
    assert_eq!(digest.len(), 64, "sha256 hex is 64 chars");
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));

    // Every field is part of the identity: changing any one must change the
    // digest, or grant_exact's "modified action requires new approval" check
    // could be bypassed.
    let mut p = base.clone();
    p.principal = "agent:other".into();
    assert_ne!(digest, p.action_digest(), "principal must affect digest");

    let mut t = base.clone();
    t.tool = "shell".into();
    assert_ne!(digest, t.action_digest(), "tool must affect digest");

    let mut o = base.clone();
    o.operation = "pull".into();
    assert_ne!(digest, o.action_digest(), "operation must affect digest");

    let mut r = base.clone();
    r.resource = "upstream".into();
    assert_ne!(digest, r.action_digest(), "resource must affect digest");

    let mut a = base.clone();
    a.args.push("c".into());
    assert_ne!(digest, a.action_digest(), "args must affect digest");

    let mut s = base.clone();
    s.expected_side_effects.push("network_egress".into());
    assert_ne!(digest, s.action_digest(), "side effects must affect digest");
}

#[test]
fn grant_exact_preserves_ticket_fields_other_than_approval() {
    let req = request("git", "push", "origin main", &["push"], &["external_write"]);
    let ticket = create_approval_ticket("run-7", &req);

    let granted = grant_exact(&ticket, "human:operator", &req).expect("matching digest grants");

    // Only approver + status change on grant.
    assert_eq!(granted.approver.as_deref(), Some("human:operator"));
    assert_eq!(granted.status, "approved");
    // Everything else is carried through unchanged — the approval applies to the
    // same ticket, not a re-minted one.
    assert_eq!(granted.ticket_id, ticket.ticket_id);
    assert_eq!(granted.run_id, ticket.run_id);
    assert_eq!(granted.requested_action, ticket.requested_action);
    assert_eq!(granted.action_digest, ticket.action_digest);
    assert_eq!(granted.risk_class, ticket.risk_class);
    assert_eq!(granted.requester, ticket.requester);
    assert_eq!(granted.scope, ticket.scope);
    assert_eq!(granted.created_at, ticket.created_at);
    assert_eq!(granted.expires_at, ticket.expires_at);
}
