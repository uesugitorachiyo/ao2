# AO2 Agent Instructions

AO2 is a public, local-first governed software-delivery project. Use the
build-facing docs in `docs/` as the source of truth.

Rules:

- Before non-trivial writes, reserve your write scope in the active
  conversation or ignored `target/` artifacts, then release it in the handoff
  when done.
- Build against `docs/PRD.md`, `docs/SDD-risky-pr-run.md`, `docs/SCHEMAS-AND-INTERFACES.md`, and `docs/IMPLEMENTATION-SLICES.md`.
- Keep the MVP local-first.
- Do not add provider API-key auth paths.
- Do not use `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`.
- Do not record secrets, bearer tokens, private key material, local account
  identifiers, or private repo paths in tracked files.
- No side-effecting tool action should bypass policy.
- Evidence must exist before evaluator closure accepts a run.
