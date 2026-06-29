# 08 MVP PRD: AO2 Local Governed Software Delivery

Created: 2026-05-16

## Product Name

AO2 Local Governed Software Delivery MVP

## Primary User

Staff engineer or AI platform engineer using a local coding agent in a Git repo.

Secondary users:

- engineering manager reviewing agent-produced work
- security engineer reviewing policy and approval evidence
- platform engineer evaluating whether AO2 can standardize internal agent work

## Problem

Coding agents can make useful changes, but teams cannot reliably answer:

- what context the agent saw
- what tools the agent invoked
- what risky action was blocked
- what was approved
- what changed
- what evidence proves the work
- why the result was accepted or rejected
- whether the run can be replayed or audited

Most coding-agent products stop at a PR, diff, or transcript. AO2 should own the delivery contract around the agent.

## MVP Promise

Run one local software-delivery task through a governed workflow where risky actions are blocked or approved, evidence is captured, and evaluator closure determines whether the work is accepted.

## First Workflow

Workflow name:

`governed-software-change`

Demo scenario:

`Risky PR Run`

Primary flow:

1. User starts a governed run against a local Git repo with a natural-language objective.
2. AO2 compiles the objective into a fixed software-delivery workflow.
3. Planner creates a scoped plan.
4. Implementer runs through one local CLI adapter.
5. Tool gateway blocks at least one risky action before execution.
6. Human approves, denies, or narrows the action.
7. Tests or verification commands run.
8. Reviewer records concerns.
9. Evaluator rejects if required evidence is missing or concerns are unresolved.
10. Implementer corrects.
11. Evaluator accepts.
12. AO2 exports a final evidence pack.

## MVP Product Surfaces

### CLI

Required commands:

```bash
ao2 init
ao2 run risky-pr.yaml
ao2 status <run-id>
ao2 approve <ticket-id>
ao2 export <run-id>
```

Optional after core loop:

```bash
ao2 cockpit
ao2 replay <run-id>
```

### Local Run Record

The local run record must include:

- event log
- workflow version
- role states
- scoped context artifacts, including bounded SDD surface-map metadata and
  shrink/provenance digests
- policy decisions
- approval tickets
- artifacts
- concerns
- blockers
- test evidence
- closure report

### Minimal Cockpit Or Static Report

The MVP can ship either:

- a minimal local cockpit; or
- a generated static HTML/markdown report.

It must show:

- objective
- workflow version
- timeline
- roles
- denied/approved actions
- artifacts
- diff/test evidence
- concerns/blockers
- closure verdict

## Adapter Choice

Choose exactly one for MVP:

- Codex CLI, or
- Claude Code CLI.

The adapter must be local-only and must not use provider API keys.

If adapter readiness blocks implementation, use a scripted/mock adapter first, but keep the same adapter contract and run-record behavior.

## Acceptance Criteria

### AC-01 Run Creation

Given a clean local Git repo and a user objective, when the user starts a governed run, then AO2 creates a run record with run id, workflow version, objective, role list, initial budget estimate fields, and initial event log.

### AC-02 Scoped Planning

Given a run objective, when the planner role completes, then it emits a plan artifact with scope, likely affected files, expected commands, risks, and acceptance criteria.

### AC-03 Adapter Execution

Given an approved plan, when the implementer role runs, then AO2 captures the adapter transcript, produced artifacts, changed files, concerns, blockers, and role completion state.

### AC-04 Policy Block

Given the implementer attempts a risky action such as broad file write, network access, `git push`, package install, PR creation, or destructive shell command, when the action is requested, then AO2 blocks it before execution and records a policy decision.

### AC-05 Exact-Digest Approval

Given a blocked action, when the user approves it, then the approval is bound to the exact action digest, approver identity, timestamp, and run id. A materially different action must require a new approval.

### AC-06 Evidence Capture

Given tests or verification commands run, when they complete, then AO2 stores command, exit code, output digest, timestamp, and linkage to the role/task that produced it.

### AC-07 Reviewer Concerns

Given implementation output exists, when reviewer runs, then it must emit either `no-concerns` or a structured concern list with severity, evidence reference, and required resolution.

### AC-08 Evaluator Rejection

Given required evidence is missing, tests fail, unresolved high-severity concerns exist, or an unapproved risky action occurred, when evaluator-closer runs, then the run state becomes `rejected` or `blocked`, not `accepted`.

### AC-09 Evaluator Acceptance

Given acceptance criteria are mapped to evidence, required checks pass, risky actions are approved or absent, and no blocking concerns remain, when evaluator-closer runs, then the run state becomes `accepted`.

### AC-10 Evidence Export

Given an accepted or rejected run, when the user exports evidence, then AO2 produces a portable evidence pack containing objective, workflow version, role outputs, changed files summary, policy decisions, approvals, test evidence, concerns, blockers, and closure verdict.

### AC-11 Inspectability

Given any completed run, when the user opens the local report or cockpit, then they can answer: what was requested, who/what acted, what changed, what was blocked, what was approved, what evidence exists, and why closure accepted or rejected.

### AC-12 Fail-Closed Behavior

Given missing provider auth, forbidden API key environment variables, unavailable adapter, schema-invalid artifact, or policy engine failure, when a run proceeds, then AO2 must stop or block rather than silently continue.

## Non-Goals

- No multi-tenant team mode.
- No SSO, SCIM, RBAC, or enterprise audit export.
- No full visual workflow builder.
- No marketplace, adapter registry, or policy pack registry.
- No support for every agent framework.
- No cloud/VPC deployment.
- No compliance certification claims.
- No generic chatbot or no-code automation builder.
- No multi-provider orchestration in the first MVP.
- No full trace-to-eval automation beyond optionally marking a failed run as a future fixture.
- No legacy AO import until the new run record and closure loop work end to end.

## Success Metric

The MVP succeeds when a user can run `Risky PR Run` locally and inspect an evidence pack proving:

- a risky action was blocked before execution;
- only a narrower exact action was approved;
- the first result was rejected for a concrete reason;
- correction was linked to the rejection;
- final acceptance was mapped to evidence.
