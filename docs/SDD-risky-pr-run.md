# 09 SDD: Risky PR Run

Created: 2026-05-16

## Purpose

`Risky PR Run` is the first governed software-delivery vertical slice for AO2.

It exists to prove:

> AO2 can govern a software-delivery agent run from objective to accepted evidence, including policy denial, scoped approval, reviewer concern, evaluator rejection, correction, final acceptance, and evidence export.

This is not a happy-path code-generation demo. The run must include at least one blocked or rejected state before final acceptance.

## Fixture

Use a small local Git repo fixture.

Fixture requirements:

- one simple function with missing input validation;
- one test file with a failing or missing test;
- no external network dependency;
- deterministic test command;
- no production credentials;
- no dependency install required during the demo.

Example objective:

> Add input validation to `calculate_discount` so negative prices and discount rates outside 0-1 are rejected, and update tests.

Example verifier:

```bash
python -m pytest
```

## Required Roles

### Planner

Input:

- objective
- repository summary
- allowed context bundle

Output artifact:

- scoped plan
- likely affected files
- expected commands
- risk list
- acceptance criteria

### Implementer

Input:

- objective
- planner artifact
- allowed context bundle
- tool scope

Output artifacts:

- transcript
- patch summary
- changed files summary
- concerns
- blockers

The implementer must attempt one disallowed action in the demo. This can be scripted if the live adapter is not ready.

### Reviewer

Input:

- planner artifact
- implementation artifacts
- changed files summary
- test evidence if present

Output artifact:

- structured concern list or `no-concerns`

The first reviewer pass must find one concrete issue or missing evidence.

### Test Engineer

Input:

- repository state
- verifier command

Output artifact:

- command log
- exit code
- output digest
- pass/fail status

### Evaluator-Closer

Input:

- objective
- acceptance criteria
- planner artifact
- implementation artifacts
- reviewer concerns
- test evidence
- policy decisions
- approvals

Output artifact:

- closure report

Closure states:

- `accepted`
- `accepted-with-concerns`
- `rejected`
- `blocked`
- `needs-human-decision`

## Required Demo Events

1. User submits objective.
2. AO2 compiles workflow and role contracts.
3. Planner emits scoped plan.
4. Implementer attempts one disallowed action.
5. Policy blocks it before execution.
6. Human approves only a narrower action digest.
7. Implementer produces patch.
8. Reviewer finds one concrete issue or missing evidence.
9. Test evidence is missing or failing.
10. Evaluator rejects the run.
11. Implementer corrects the issue.
12. Tests pass.
13. Evaluator accepts.
14. Cockpit/report shows timeline, policy decision, artifacts, concerns, initial cost estimate fields, and closure.
15. Evidence pack exports.
16. Rejected run state can be marked as a future eval fixture.

## Policy Requirements

Risky actions that must be blocked before execution:

- destructive shell commands;
- broad file writes outside the fixture repo;
- parent directory reads outside allowed context;
- network egress;
- `git push`;
- PR creation;
- package publish;
- package install;
- cloud CLI mutation;
- raw secret access;
- unregistered MCP server/tool.

Approval rules:

- approval grants one exact action digest only;
- modified command/resource requires a new approval;
- approval must include approver identity, timestamp, risk class, policy version, and expiry;
- agent transcript cannot self-approve.

## Security Requirements

Minimum sandbox profiles:

- `no-tools`
- `read-only-repo`
- `repo-write-no-network`
- `repo-write-approved-network`

MVP defaults:

- planner: `read-only-repo`
- implementer: `repo-write-no-network`
- reviewer: `read-only-repo`
- test-engineer: `repo-write-no-network`
- evaluator-closer: `read-only-repo`

Secret rules:

- no `OPENAI_API_KEY`
- no `ANTHROPIC_API_KEY`
- no raw secrets in prompts, env, artifacts, traces, logs, or tool results
- secrets redacted before persistence

## Acceptance Bar

Accept the demo only if an evaluator can answer these from run record, cockpit/report, or evidence pack:

- What was requested?
- Which workflow version ran?
- Which roles participated?
- What context did each role receive?
- What tool action was denied?
- Why was it denied?
- What exact action was later approved?
- What changed in the repo?
- What tests or verifiers ran?
- What concern caused rejection?
- What correction resolved it?
- Why did evaluator-closer accept the final state?
- Where is the exported evidence pack?

Reject the demo if any answer requires verbal narration or manual filesystem archaeology.

## UAT Matrix

| ID | Scenario | Pass Condition |
|---|---|---|
| UAT-01 | Workflow compilation | Run reaches `compiled`; schema validation passes; role contracts are visible. |
| UAT-02 | Scoped context | Planner and implementer have bounded context bundle artifacts with digest and lineage; SDD planning records full and shrunken surface-map digests, file counts, budget, and shrink-enabled status. |
| UAT-03 | Policy denial | Risky action records `tool.requested` and `tool.denied` before execution. |
| UAT-04 | Narrow approval | Approved action digest is exact; original denied action remains denied. |
| UAT-05 | Evidence artifacts | Plan, patch, command log, test log, review, and closure artifacts have provenance. |
| UAT-06 | Reviewer concern | Concern includes severity, affected artifact, reason, and required resolution. |
| UAT-07 | Evaluator rejection | Missing/failing evidence or unresolved concern yields `rejected`, not `accepted`. |
| UAT-08 | Correction loop | Correction artifacts link back to rejection and concern. |
| UAT-09 | Final acceptance | Closure maps each acceptance criterion to evidence. |
| UAT-10 | Cockpit/report inspection | User can answer what happened, why, what changed, and why accepted. |
| UAT-11 | Evidence export | Evidence pack includes objective, workflow, roles, policy, approvals, artifacts, tests, concerns, closure, digests. |
| UAT-12 | Eval fixture marker | Initial rejected state can be marked as a future regression fixture. |
