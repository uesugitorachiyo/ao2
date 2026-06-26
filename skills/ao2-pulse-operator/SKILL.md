---
name: ao2-pulse-operator
description: Use when operating AO2 Pulse auto-advance, event-loop smoke, STOP files, duplicate ledgers, PR/CI gates, local-only blocked mode, daemon controls, task-board state, next actions, or closure packets.
---

# AO2 Pulse Operator

Pulse auto-advance is Rust-native runtime behavior with npm and shell compatibility wrappers. Treat `crates/ao2-runtime/src/pulse_event_loop.rs`, `scripts/pulse-*.sh`, and `docs/VERIFICATION.md` as the source of truth.

## Operations

| Need | Command or path | Evidence |
| --- | --- | --- |
| Event-loop smoke | `npm run pulse:ao2-event-loop-smoke` | `target/pulse-next-recommended-tasks` and `target/pulse-event-loop` |
| Register latest prompt | `npm run pulse:register-auto-advance` | `target/pulse-auto-advance-registration/latest/summary.json` |
| Run auto-advance | `npm run pulse:auto-advance` | `target/pulse-auto-advance/latest/summary.json` |
| Runner contract | `npm run pulse:auto-advance-runner-contract` | contract summary under `target/pulse-auto-advance-runner-contract` |
| Integration gate | `npm run pulse:auto-advance-integration-gate` | `target/pulse-auto-advance-integration-gate/latest/summary.json` |
| Stop loop | `.ao2-local/pulse/STOP` | auto-advance summary reports stopped state |
| Dedup ledger | `.ao2-local/pulse/pulse-auto-advance-ledger.jsonl` | duplicate eval-loop digest rejection |
| PR/CI gate update | `npm run pulse:pr-ci-gate:update` | `.ao2-local/pulse/pr-ci-gate.json` and `target/pulse-pr-ci-gate-update/latest/summary.json` |
| Local-only while blocked | `AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED=1 npm run pulse:auto-advance` | local-only evidence, no normal product-code advancement |
| Daemon controls | `npm run pulse:daemon:start`, `npm run pulse:daemon:status`, `npm run pulse:daemon:stop`, `npm run pulse:daemon:restart` | `target/pulse-daemon/latest/summary.json` |
| Task-board readback | `npm run pulse:task-board-state` | `target/pulse-task-board-state/latest/summary.json` |
| Next actions | `npm run pulse:next-actions` | `target/pulse-next-actions/latest/next-actions.md` |
| Closure packet | `npm run pulse:task-board-closure-packet` | `target/pulse-task-board-closure-packet/latest/summary.json` |

## Decision Rules

- Honor `.ao2-local/pulse/STOP` before advancing.
- Reject duplicate eval-loop digests unless the configured path explicitly allows duplicates.
- Treat `.ao2-local/pulse/pr-ci-gate.json` as the local PR/CI gate state.
- Skip normal product-code advancement while PR/CI is blocked.
- Use local-only mode only for evidence generation while blocked.
- Invoke direct-main publication only through `npm run pulse:direct-main-publish` and its guarded contract.

## Trust Boundaries

- Do not move Pulse decisions back into ad hoc shell or Python glue.
- Do not bypass the Rust runtime gate state with manual edits and then claim closure.
- Do not use provider API keys or side-effecting commands outside AO2 policy.
- Do not treat daemon activity as proof; inspect summaries and artifact paths.

## Exit Criteria

- The latest summary, heartbeat or ledger evidence, PR/CI gate state, and task-board state support the recommendation.
- STOP, duplicate ledger, blocked PR/CI, and local-only behavior are accounted for.
- Any command that was skipped because it is lengthy, blocked, or requires sibling repo state is named as residual risk.
