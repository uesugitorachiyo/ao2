# AO2 Runtime Scope Instructions

## Runtime Contract

- `ao2-runtime` owns run state transitions, orchestration, replay, retries, side-effect sequencing, and evidence handoff. Keep transitions explicit, deterministic, and recoverable.
- Evaluate policy and verify any exact-digest approval before a side effect. A retry must not reuse stale approval, duplicate an irreversible action, or erase the prior attempt.
- Append evidence with source, digest, lineage, and failure context before evaluator closure. Missing or conflicting state fails closed.
- Preserve sandbox and artifact boundaries. Adapters return normalized results; they do not become the durable source of run state or authority.

## Verification

- Run the smallest relevant integration test under `crates/ao2-runtime/tests/`.
- Run `cargo test -p ao2-runtime` for this scope, then the root-required broad gate when the change can affect workspace consumers.
- Report any intentionally skipped live-provider test; do not enable provider credentials for verification.
