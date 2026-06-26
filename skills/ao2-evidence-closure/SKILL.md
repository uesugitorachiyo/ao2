---
name: ao2-evidence-closure
description: Use when closing AO2 work, PRs, release-readiness tasks, provider approval tasks, Pulse tasks, RSI tasks, or control-plane readback tasks that require durable evidence before completion.
---

# AO2 Evidence Closure

Close AO2 work only after evidence exists. A passing impression is not closure; exact command output, durable artifacts, and residual risk are closure inputs.

## Closure Checklist

1. State the claim being closed.
2. Run the smallest command set that proves or disproves that claim.
3. Record exact command names, exit status, and important output lines.
4. Link the durable artifact paths or summaries.
5. Check trust boundaries from `AGENTS.md`, `docs/SECURITY.md`, and the relevant `docs/VERIFICATION.md` section.
6. Report residual risk, skipped checks, and any required human approval.

## Command Families

| Area | Common gates |
| --- | --- |
| Exact-digest approval | `npm run approval:exact-digest-gate` |
| Provider safety | `npm run provider:adversarial-corpus`, `npm run provider:pilot-safety-regression-matrix`, `npm run provider:pilot-command-safety-audit` |
| Pulse | `npm run pulse:ao2-event-loop-smoke`, `npm run pulse:auto-advance-runner-contract`, `npm run pulse:auto-advance-integration-gate`, `npm run pulse:task-board-closure-packet` |
| RSI | `npm run rsi:claim-readiness`, `npm run rsi:cross-repo-e2e`, `npm run rsi:baseline-packet`, `npm run rsi:eligibility-packet` |
| Control-plane readback | `npm run smoke:evidence-control-plane`, `npm run control-plane:cross-repo-observer`, `npm run release:train-control-plane-bridge` |
| Release readiness | `npm run release:readiness`, `npm run release:readiness:artifact-consumer`, `npm run release:readiness:final-closure-verifier`, `npm run release:evidence-closure` |

## Evidence To Look For

- Provider approval: persisted ticket, preview digest, granted approval, consumed status, apply artifact, and event log.
- Pulse: `.ao2-local/pulse` state, STOP handling, dedup ledger, PR/CI gate state, heartbeat, and `target/pulse-*` summaries.
- RSI: claim-readiness, self-change dry-run or rehearsal evidence, readback index, improvement gate/trend, cross-repo E2E, baseline and eligibility packets.
- Control-plane: read-only consumer output, dashboard link readiness, release-readiness consumer dashboard, and bridge artifacts.

## Trust Boundaries

- Do not close on missing evidence, stale artifacts, or assumed CI state.
- Do not use provider API-key paths or persist raw secrets.
- Do not treat a raw digest as approval for governed provider promotion.
- Do not mark a claim accepted when the evidence only supports a narrower claim.

## Exit Criteria

- Every closure statement has a command and artifact behind it.
- Approval, Pulse, RSI, provider, release, and control-plane evidence are included when relevant.
- Failures and skipped checks are explicit.
- Residual risk is short, concrete, and tied to missing or future evidence.
