# AO2 Skills

AO2 skills are compact operator guides for running AO2 evidence workflows. They are AO2-native, evidence-first, and scoped to governed delivery: Pulse, RSI, exact-digest approval, provider safety, release readiness, and control-plane readback.

This directory is not a marketplace. Do not import broad skills only to match another stack. Add a skill only when it routes an operator to concrete AO2 commands, artifacts, docs, and trust boundaries.

## Current First Slice

- `ao2-rsi-operator`: run and interpret bounded governed RSI evidence without approving full autonomous RSI claims.
- `ao2-evidence-closure`: close AO2 work only with exact command output, durable artifacts, readback, and residual risk.
- `ao2-pulse-operator`: operate Rust-native Pulse auto-advance, PR/CI gates, STOP files, daemon state, and closure packets.
- `ao2-approval-policy`: verify persisted exact-digest approval tickets before governed provider sandbox promotion.
- `ao2-control-plane-readback`: surface AO2 evidence through read-only ao2-control-plane consumers and dashboards.

## Principles

- Keep each skill short and operational.
- Prefer links to AO2 docs, scripts, and artifacts over pasted long-form docs.
- State trust boundaries explicitly.
- Include exit criteria.
- Do not add plugin wiring unless AO2 has a deliberate plugin integration path.
- Do not import Hermes parity packs, broad marketplace skills, or out-of-scope swarm machinery.
