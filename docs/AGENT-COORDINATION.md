# Agent Coordination

AO2 is public and local-first. Use this file as the public-safe coordination
surface for non-trivial writes.

## Write Scope Reservations

Before editing tracked files, add a short local handoff note in the active
conversation or in ignored `target/` artifacts with:

- agent name;
- intended files or directories;
- start time;
- expected verification command.

If another active agent already owns the same scope, coordinate before writing.
Use this note to reserve the intended scope, then release the scope in the final
handoff by listing changed files and verification results.

## Public-Safe Rules

- Do not record secrets, bearer tokens, private key material, local account
  identifiers, or private repo paths in tracked files.
- Keep local schedules, Pulse event-loop artifacts, run evidence, and scratch
  plans under ignored `target/`.
- Keep the MVP local-first and provider-key-free.
- Do not add `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` paths.
- Do not let side-effecting tool actions bypass policy.
- Evidence must exist before evaluator closure accepts a run.

## Current Reserved Scopes

No persistent reservation is active in this public file. Short-lived reservations
may exist in the current agent conversation or ignored local `target/` notes.
