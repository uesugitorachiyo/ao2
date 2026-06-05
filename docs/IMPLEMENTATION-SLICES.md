# 11 MVP Slices And Acceptance Gates

Created: 2026-05-16

## Build Strategy

Build one local-only governed software-delivery vertical slice before expanding.

Strict build order:

1. Canonical local run model
2. Provider-free runtime kernel
3. Policy and approval gateway
4. One agent adapter
5. Evaluator closure loop
6. Minimal cockpit or static report

## Slice 1: Canonical Local Run Model

Goal:

Define the core schemas and sample workflow.

Build:

- workflow schema
- role schema
- task schema
- run record schema
- event schema
- artifact schema
- policy decision schema
- approval ticket schema
- closure schema
- `risky-pr-run` sample workflow

Done criteria:

- schemas validate in CI;
- sample workflow compiles;
- sample run record round-trips;
- every artifact has digest, producer role, lineage, and sensitivity;
- no untyped task payloads for MVP task kinds.

Acceptance gate:

- A reviewer can read the schemas and understand exactly what a run stores and what evidence closure uses.

## Slice 2: Provider-Free Runtime Kernel

Goal:

Execute deterministic workflow tasks without live model calls.

Build:

- local event store
- deterministic scheduler
- run state machine
- filesystem artifact store
- CLI commands: `run`, `status`, `export`
- deterministic stub tasks for planner, reviewer, test-engineer, evaluator

Done criteria:

- can execute provider-free `Risky PR Run` skeleton;
- event log is append-only;
- pause/resume works for approval state;
- replay reconstructs run state;
- evidence pack exports from local artifacts.

Acceptance gate:

- A run can be reconstructed from stored events and artifacts without relying on terminal history.

## Slice 3: Policy And Approval Gateway

Goal:

Make side effects policy-gated and approval-bound.

Build:

- deny-by-default policy engine
- tool gateway for shell/git/file classifications
- approval ticket model
- exact action digest approval
- forbidden env var preflight
- redaction before persistence

Initial hard denies:

- destructive commands
- broad file writes
- parent directory reads
- network egress
- `git push`
- PR creation
- package install
- package publish
- cloud deploy
- raw secret access

Done criteria:

- risky command or broad write is blocked before execution;
- approval grants only exact action digest;
- denied and approved decisions appear in run record;
- forbidden API-key env vars are detected;
- external writes, `git push`, and PR creation require approval.

Acceptance gate:

- The demo visibly proves that policy acts before side effects, not after.

## Slice 4: One Agent Adapter

Goal:

Run one bounded implementer role through a local agent.

Choose one:

- Codex CLI; or
- Claude Code CLI.

Build:

- adapter contract
- process wrapper
- transcript capture
- role output parser
- artifact extraction
- normalized blocker/error taxonomy

Done criteria:

- adapter runs one implementer role locally;
- role output includes evidence, concerns, blockers;
- adapter transcript is captured as artifact;
- file/tool side effects cannot bypass gateway;
- failures normalize into blocker/error taxonomy.

Acceptance gate:

- An existing coding agent can do useful work while AO2 owns workflow state, policy, evidence, and acceptance.

## Slice 5: Evaluator Closure Loop

Goal:

Make acceptance explicit and evidence-bound.

Build:

- evaluator contract
- closure report
- acceptance criteria mapping
- concern/blocker taxonomy
- rejected-run resume path
- `accepted-with-concerns` behavior

Done criteria:

- demo fails when evidence/tests are missing;
- implementer correction resumes same run;
- evaluator accepts only after mapped evidence exists;
- closure report is readable by engineering/security reviewer;
- final run state is accepted, rejected, blocked, or accepted-with-concerns, never vague `done`.

Acceptance gate:

- The first implementation is rejected for a concrete reason, then accepted after correction with evidence mapping.

## Slice 6: Minimal Cockpit Or Static Report

Goal:

Make the run inspectable.

Build either:

- static local HTML/markdown report; or
- minimal local web cockpit.

Required views:

- objective
- workflow version
- role timeline
- denied actions
- approval tickets
- artifacts
- diff/test evidence
- concerns/blockers
- closure verdict
- evidence export path

Done criteria:

- user can answer from cockpit/report alone:
  - objective;
  - roles/models;
  - context/tools;
  - blocked/approved actions;
  - changed artifacts;
  - tests/evidence;
  - concerns;
  - evaluator verdict;
  - replay/export entry points.

Acceptance gate:

- No manual filesystem archaeology is needed to understand the run.

## Build Deferrals

Do not build before Slice 6:

- multiple adapters
- full MCP gateway
- LangGraph/CrewAI/n8n import
- team mode
- RBAC
- Postgres
- SSO
- audit export for enterprise
- full trace-to-eval automation
- legacy AO import
- marketplace
- policy pack registry
- full visual workflow builder

## MVP Completion Gate

The MVP is complete only when `Risky PR Run` demonstrates:

```text
objective enters
workflow compiles
agent attempts risky action
policy blocks it
human approves narrower exact action
implementation produces artifacts
review/test evidence is captured
evaluator rejects once
correction resumes same run
evaluator accepts
cockpit/report explains the whole run
evidence exports
```

Anything less is not yet AO2's core product.
