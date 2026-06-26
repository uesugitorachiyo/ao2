---
name: ao2-rsi-operator
description: Use when operating or auditing AO2 RSI evidence, claim readiness, governed self-change rehearsal, baseline packets, eligibility packets, cross-repo E2E, and release-readiness dashboard readback.
---

# AO2 RSI Operator

Use this skill to run and interpret AO2 recursive self-improvement evidence. It does not publish, approve, or claim `full_autonomous_self_mutating_rsi`.

## Claim Boundary

- Supported claim: `bounded_governed_rsi`, meaning local-first Pulse continuation, governed self-change rehearsal, exact evidence gates, and control-plane readback.
- Unsupported claim: `full_autonomous_self_mutating_rsi`, unless future evidence proves production authority, claim-publish approval, and Covenant acceptance.
- AO2 must not write its own permission slip. Blueprint/Covenant authorization and human/operator approval remain outside the mutation loop.

## Command Flow

| Purpose | Command | Primary evidence |
| --- | --- | --- |
| Claim-readiness audit | `npm run rsi:claim-readiness` | `target/rsi-claim-readiness/latest/summary.json` |
| Governed self-change dry run | `npm run rsi:self-change-dry-run` | `target/rsi-self-change-dry-run/latest/summary.json` |
| Live rehearsal | `AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1 npm run rsi:live-self-change-rehearsal` | `target/rsi-live-self-change-rehearsal/latest/summary.json` |
| Rehearsal readback index | `npm run rsi:live-self-change-readback-index` | `target/rsi-live-self-change-readback-index/latest/summary.json` |
| Improvement gate | `npm run rsi:improvement-evidence-gate` | `target/rsi-improvement-evidence-gate/latest/summary.json` |
| Improvement trend | `npm run rsi:improvement-trend` | `target/rsi-improvement-trend/latest/summary.json` |
| Control-plane dashboard smoke | `npm run rsi:control-plane-release-readiness-dashboard-smoke` | `target/rsi-control-plane-release-readiness-dashboard-smoke/latest/summary.json` |
| Cross-repo E2E | `npm run rsi:cross-repo-e2e` | `target/rsi-cross-repo-e2e/latest/summary.json` |
| Baseline packet | `npm run rsi:baseline-packet` | `target/rsi-baseline-packet/latest/summary.json` and `dashboard.html` |
| Eligibility packet | `npm run rsi:eligibility-packet` | `target/rsi-eligibility-packet/latest/summary.json` and `dashboard.html` |

## Operating Notes

Read `README.md` RSI sections and `docs/VERIFICATION.md` before changing claim language. For PR #221 and later, baseline and eligibility packets must include dashboard readback evidence with `dashboard_link_ready=true`.

When a command spans repositories, verify sibling checkout assumptions before running it. Expected siblings include `ao2-control-plane` and `ao-covenant` when the cross-repo gate requires them.

## Trust Boundaries

- Do not use `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`.
- Do not convert a rehearsal, dry run, or dashboard readback into publication authority.
- Do not approve the full RSI claim from AO2 evidence alone.
- Do not bypass exact-digest approval, Covenant claim boundaries, or release-readiness gates.

## Exit Criteria

- The exact commands run are named with exit status and key output paths.
- Summary artifacts exist and support the stated claim.
- `bounded_governed_rsi` and `full_autonomous_self_mutating_rsi` remain separated in the final assessment.
- Any failed, skipped, missing sibling-repo, or stale-dashboard evidence is reported as residual risk.
