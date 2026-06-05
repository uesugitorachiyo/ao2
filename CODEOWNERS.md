# AO2 Path Ownership Convention

This repository does not currently enforce GitHub `CODEOWNERS` review gates
with concrete team handles. Until those handles are configured, use this file as
the trust-boundary ownership convention.

```text
*                      codex / ao2 maintainers
/crates/sdd-planner/** claude / factory-v3 maintainers
```

`crates/sdd-planner/**` is the factory-v3/claude-authored planner surface now
merged into the AO2 workspace. AO2 runtime, CLI execution, evidence, and release
trust-boundary code outside that path remain codex/AO2-owned.
