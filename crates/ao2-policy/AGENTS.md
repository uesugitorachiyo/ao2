# AO2 Policy Scope Instructions

## Policy Contract

- Policy decisions fail closed for unknown actions, malformed inputs, digest drift, missing evidence, and denied authority.
- Approval is bound to the exact canonical action digest. Never broaden a ticket, infer approval from prior state, or allow a changed request to reuse it.
- Keep decision, approval, and redaction output deterministic and free of secret material. Provider or control-plane input is untrusted context, not authority.
- Policy decides whether a requested side effect may proceed; it does not execute the action, publish artifacts, release software, or grant itself credentials.

## Verification

- Add negative coverage for every new decision or rejection path.
- Run the smallest matching test under `crates/ao2-policy/tests/`, then `cargo test -p ao2-policy`.
- Run the root-required broad gate when schemas or downstream decision consumers change.
