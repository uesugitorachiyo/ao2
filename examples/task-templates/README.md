# AO2 Task Templates

These templates are starting points for governed real-project runs. They keep
the same policy shape as the MVP risky PR workflow:

- deny by default;
- exact action digest approval;
- replay required;
- evidence cockpit required.

List embedded templates from an installed binary:

```sh
ao2 template list
```

Print a template:

```sh
ao2 template show bug-fix > bug-fix.yaml
ao2 run bug-fix.yaml --target /path/to/repo --provider codex --provider-prompt-file prompt.txt
```

The initial template set covers:

- `bug-fix`
- `small-refactor`
- `dependency-upgrade`
- `test-generation`
- `rust-cargo-bug-fix`

## Rust/Cargo Beta Runs

Use `rust-cargo-bug-fix` for Rust crate repair work during the beta:

```sh
ao2 template show rust-cargo-bug-fix > rust-cargo-bug-fix.yaml
ao2 run rust-cargo-bug-fix.yaml \
  --target /path/to/rust-crate \
  --provider codex \
  --provider-prompt-file prompt.txt
```

The generic `bug-fix` template uses `python -m pytest` as its verifier. For
Rust beta runs, use `cargo test` through `rust-cargo-bug-fix` so AO2 asks the
test-engineer and evaluator-closer to judge the ecosystem's native verifier
instead of nudging the run toward a Python pytest wrapper.

This is beta workflow/template guidance only. It does not require a new binary
release, tag, upload, deployment, or publication step.

## C++ To Rust Stretch Case

For a small C++ to Rust migration, keep the reference behavior explicit and
make the Rust crate prove parity with `cargo test`.

Example prompt shape:

```text
Reference behavior:
- C++ function `risk_score(events, severity)` returns 0 for no events.
- It clamps severity below 0 to 0 and above 10 to 10.
- It returns events * clamped_severity, capped at 100.

Rust target:
- Implement the same behavior in `src/lib.rs` as
  `pub fn risk_score(events: u32, severity: i32) -> u32`.
- Add Rust tests for empty input, severity clamping, normal scoring, and the
  100-point cap.

Verifier:
- Run `cargo test`.

AO2 expectations:
- Approval remains exact-action-digest bound for risky actions.
- Replay must have zero digest failures.
- Evidence must include the plan, patch summary, `cargo test` log, and closure
  report before evaluator acceptance.
```
