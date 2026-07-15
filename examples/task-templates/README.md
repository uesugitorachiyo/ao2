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

The stable `v0.5.0` binary includes the Rust/Cargo template in
`ao2 template list` and `ao2 template show`.

```sh
ao2 run examples/task-templates/rust-cargo-bug-fix.yaml \
  --target /path/to/rust-crate \
  --provider codex \
  --provider-prompt-file prompt.txt
```

The initial template set covers:

- `bug-fix`
- `small-refactor`
- `dependency-upgrade`
- `test-generation`
- `rust-cargo-bug-fix`

## Rust/Cargo Runs

Use `rust-cargo-bug-fix` for Rust crate repair work:

```sh
ao2 template show rust-cargo-bug-fix > rust-cargo-bug-fix.yaml
ao2 run rust-cargo-bug-fix.yaml \
  --target /path/to/rust-crate \
  --provider codex \
  --provider-prompt-file prompt.txt
```

The generic `bug-fix` template uses `python -m pytest` as its verifier. It is
the Python/default template. For Rust runs, use `rust-cargo-bug-fix` so AO2 asks
the test-engineer and evaluator-closer to judge the ecosystem's native
`cargo test` verifier instead of nudging the run toward a Python pytest wrapper.

This workflow/template guidance does not start a new release train by default.

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
