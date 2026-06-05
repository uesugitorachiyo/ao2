# AO2 Agent Instructions

AO2 is a new private successor project. Use the build-facing docs in `docs/` as the source of truth.

Rules:

- Read `docs/AGENT-COORDINATION.md` before non-trivial writes. If Codex,
  Claude Code, and Antigravity are active at the same time, reserve your write
  scope there first and release it in the handoff entry when done.
- Build against `docs/PRD.md`, `docs/SDD-risky-pr-run.md`, `docs/SCHEMAS-AND-INTERFACES.md`, and `docs/IMPLEMENTATION-SLICES.md`.
- Keep the MVP local-first.
- Do not add provider API-key auth paths.
- Do not use `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`.
- No side-effecting tool action should bypass policy.
- Evidence must exist before evaluator closure accepts a run.
