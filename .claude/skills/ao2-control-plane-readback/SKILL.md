---
name: ao2-control-plane-readback
description: Use when AO2 evidence must be surfaced, linked, smoked, or audited through ao2-control-plane read-only consumers, release-readiness dashboards, bridge artifacts, or dashboard readback.
---

# AO2 Control-Plane Readback

Use this skill when AO2 needs proof that evidence can be consumed by `ao2-control-plane`. The control plane is a read-only observer for AO2 evidence; it must not mutate AO2 runs, approve releases, or publish RSI claims.

## Command Routes

| Need | Command | Evidence |
| --- | --- | --- |
| Evidence-pack control-plane smoke | `npm run smoke:evidence-control-plane` | `ao2.cp-evidence-pack-dashboard.v1`, detail, latest, ingest receipt |
| Cross-repo observer | `npm run control-plane:cross-repo-observer` | AO2/control-plane observer summary under `target/` |
| Release train bridge | `npm run release:train-control-plane-bridge` | `target/release-train-control-plane-bridge/latest/summary.json` |
| Release readiness dashboard readback | `npm run rsi:control-plane-release-readiness-dashboard-smoke` | `target/rsi-control-plane-release-readiness-dashboard-smoke/latest/summary.json` |
| Dashboard QA, when scoped | `npm run evidence:dashboard-browser-qa`, `npm run evidence:dashboard-accessibility-audit` | dashboard QA summaries and screenshots |
| Dual-repo approval closure | `npm run release:dual-repo-public-approval-closure` | `target/dual-repo-public-approval-closure/latest/summary.json` |

## Readback Rules

- Confirm the sibling `../ao2-control-plane` checkout exists and is on the expected branch when a cross-repo command requires it.
- Prefer fixture-backed or local read-only smoke before relying on hosted CI artifacts.
- Preserve bridge artifact paths and schemas named in `docs/VERIFICATION.md`.
- For RSI release-readiness dashboard evidence, require `dashboard_link_ready=true` before treating baseline or eligibility packets as ready.
- Keep dashboard/browser QA scoped to AO2 evidence dashboards. Do not import generic browser QA workflows.

## Trust Boundaries

- Do not grant release, approval, or RSI publication authority from readback alone.
- Do not store bearer tokens, API keys, or raw secrets in tracked files or artifacts.
- Do not let control-plane consumers mutate AO2 repository state.
- Do not hide missing sibling repo state, stale artifacts, or skipped dashboard evidence.

## Exit Criteria

- The final report names the command, exit status, and control-plane artifact paths.
- The readback schema names and dashboard links are present when required.
- Any CI artifact dependency is explicit: local fixture, latest successful main artifact, or specific run id.
- Residual risk names missing dashboards, unavailable sibling repos, skipped browser QA, or unverified hosted artifacts.
