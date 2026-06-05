//! Property-based tests for the secret-redaction engine and the action-digest
//! canonicalization in `ao2-policy`.
//!
//! The example-based tests in `policy_hardening.rs` pin specific known secret
//! shapes. These tests assert *invariants* that must hold across a huge space
//! of randomly generated inputs — the kind of coverage that catches the class
//! of bug example tests miss:
//!
//!   * the redactor walks lines with hand-rolled byte indexing and several
//!     `expect("valid char boundary")` calls — a UTF-8 boundary slip would
//!     panic. The "never panics on arbitrary input" properties exercise that
//!     directly with multi-byte unicode.
//!   * the value-survival property is the core security guarantee: a sensitive
//!     assignment's value must never appear in the output.
//!   * the digest properties pin that approval enforcement is deterministic and
//!     tamper-evident (a modified request can never reuse an old ticket).

use ao2_policy::{
    create_approval_ticket, grant_exact, redact_secrets, secret_redaction_class_counts,
    secret_redaction_count, ToolRequest,
};
use proptest::prelude::*;

/// Sensitive assignment-key suffixes recognised by `sensitive_key_class`.
const SENSITIVE_SUFFIXES: &[&str] = &[
    "_API_KEY",
    "_AUTH_TOKEN",
    "_SERVICE_ROLE_KEY",
    "_INTAKE_KEY",
    "_PRIVATE_KEY",
    "_ACCESS_KEY",
    "_TOKEN",
    "_SECRET",
    "_PASSWORD",
];

/// Keys that must never be treated as secret.
const SAFE_KEYS: &[&str] = &[
    "SAFE_VALUE",
    "PATH",
    "HOME",
    "BUILD_DIR",
    "NODE_ENV",
    "STAGE",
];

fn tool_request_strategy() -> impl Strategy<Value = ToolRequest> {
    (
        "\\PC{0,24}",
        "\\PC{0,24}",
        "\\PC{0,24}",
        "\\PC{0,24}",
        prop::collection::vec("\\PC{0,24}", 0..5),
        prop::collection::vec("\\PC{0,24}", 0..3),
    )
        .prop_map(
            |(principal, tool, operation, resource, args, expected_side_effects)| ToolRequest {
                principal,
                tool,
                operation,
                resource,
                args,
                expected_side_effects,
            },
        )
}

proptest! {
    // The whole value of a sensitive assignment is replaced wholesale, so no
    // matter what the value contains it must not survive in the output. The
    // value is lowercase so it can never be a substring of the upper-case key
    // name or of the literal `[REDACTED]`, which keeps the assertion exact.
    #[test]
    fn sensitive_assignment_value_never_survives(
        prefix in "[A-Z][A-Z0-9_]{0,12}",
        suffix in prop::sample::select(SENSITIVE_SUFFIXES),
        value in "[a-z][a-z0-9]{7,40}",
    ) {
        let key = format!("{prefix}{suffix}");
        let line = format!("{key}={value}");
        let out = redact_secrets(&line);

        prop_assert!(!out.contains(&value), "secret value leaked: {out:?}");
        prop_assert!(out.contains("[REDACTED]"), "expected redaction marker: {out:?}");
        prop_assert!(out.starts_with(&format!("{key}=")), "key mangled: {out:?}");
        prop_assert!(secret_redaction_count(&line) >= 1, "secret not counted: {line:?}");
    }

    // A non-sensitive assignment with a benign value (lowercase alphanumeric:
    // cannot contain `sk-`/`ghp_`/`github_pat_`, `Bearer `, or query/header
    // delimiters) must pass through completely untouched, and must register
    // zero redactions.
    #[test]
    fn safe_assignment_is_preserved_verbatim(
        key in prop::sample::select(SAFE_KEYS),
        value in "[a-z][a-z0-9]{0,40}",
    ) {
        let line = format!("{key}={value}");
        let out = redact_secrets(&line);

        prop_assert_eq!(&out, &line, "benign line was altered");
        prop_assert_eq!(secret_redaction_count(&line), 0, "benign line counted as secret");
    }

    // Redaction is idempotent: once a string is redacted, redacting it again
    // changes nothing. Catches replacement markers that themselves look like
    // secrets, or unstable scanning.
    #[test]
    fn redaction_is_idempotent(input in "(?s)\\PC{0,200}") {
        let once = redact_secrets(&input);
        let twice = redact_secrets(&once);
        prop_assert_eq!(once, twice);
    }

    // Redaction is line-structure preserving: it operates within each line and
    // never adds or drops a line.
    #[test]
    fn redaction_preserves_line_count(input in "(?s)\\PC{0,200}") {
        let out = redact_secrets(&input);
        prop_assert_eq!(out.lines().count(), input.lines().count());
    }

    // The detector and the redactor are independent code paths; they must agree
    // on whether a line carries a secret. If nothing is counted, the redactor
    // must be a no-op — otherwise one path sees a secret the other misses.
    #[test]
    fn no_counted_secrets_implies_no_change(input in "(?s)\\PC{0,200}") {
        prop_assume!(secret_redaction_count(&input) == 0);
        prop_assert_eq!(redact_secrets(&input), input);
    }

    // The aggregate count is exactly the sum of the per-class counts — the two
    // public surfaces can never drift apart.
    #[test]
    fn count_equals_sum_of_class_counts(input in "(?s)\\PC{0,200}") {
        let total = secret_redaction_count(&input);
        let by_class: usize = secret_redaction_class_counts(&input).values().sum();
        prop_assert_eq!(total, by_class);
    }

    // The redaction/counting entry points must never panic on arbitrary input,
    // including multi-byte unicode that stresses the hand-rolled byte indexing.
    #[test]
    fn redaction_never_panics_on_arbitrary_input(input in "(?s)\\PC{0,300}") {
        let _ = redact_secrets(&input);
        let _ = secret_redaction_count(&input);
        let _ = secret_redaction_class_counts(&input);
    }

    // The action digest is a pure function of the request: equal requests hash
    // equal, and the output is always a 64-char SHA-256 hex string.
    #[test]
    fn action_digest_is_deterministic(request in tool_request_strategy()) {
        let digest = request.action_digest();
        prop_assert_eq!(digest.len(), 64, "expected sha256 hex");
        prop_assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        prop_assert_eq!(&digest, &request.clone().action_digest());
    }

    // Mutating the request (appending an arg) changes the digest — the basis
    // for tamper-evident approval.
    #[test]
    fn action_digest_changes_when_request_changes(
        request in tool_request_strategy(),
        extra in "\\PC{1,12}",
    ) {
        let before = request.action_digest();
        let mut modified = request.clone();
        modified.args.push(extra);
        prop_assert_ne!(before, modified.action_digest());
    }

    // An approval ticket grants only for the exact request it was minted for.
    // A modified request must be rejected, forcing a fresh approval.
    #[test]
    fn approval_ticket_binds_to_exact_request(
        request in tool_request_strategy(),
        extra in "\\PC{1,12}",
    ) {
        let ticket = create_approval_ticket("run-prop", &request);
        prop_assert!(grant_exact(&ticket, "approver", &request).is_ok());

        let mut modified = request.clone();
        modified.args.push(extra);
        prop_assert!(grant_exact(&ticket, "approver", &modified).is_err());
    }
}
