---
name: ao2-approval-policy
description: Use when auditing or changing AO2 exact-digest approval tickets, provider sandbox patch promotion, approval evidence, replay boundaries, or governed provider pilot apply paths.
---

# AO2 Approval Policy

Use this skill for AO2 approval work where a side effect must be bound to persisted exact-digest evidence. Approval is not a raw digest string; approval is a durable ticket with matching run, action, target, approver, status, expiry, and consumption state.

## Source Of Truth

| Area | Route |
| --- | --- |
| Runtime enforcement | `crates/ao2-runtime/src/lib.rs` provider sandbox approval paths |
| Approval shape | `schemas/approval-ticket.schema.json` |
| Security boundary | `AGENTS.md`, `docs/SECURITY.md` |
| Verification docs | `docs/VERIFICATION.md` exact-digest and provider sections |
| Main local gate | `npm run approval:exact-digest-gate` |
| Provider regression gates | `npm run provider:phase2-contract-hardening`, `npm run provider:adversarial-corpus` |

## Required Checks

For governed provider sandbox apply, verify all of these before treating approval as valid:

- `run_id` matches the current run.
- `action_digest` matches the recomputed sandbox patch preview.
- operation/resource matches `sandbox:apply` / `sandbox_patch`.
- requester/principal matches the role requesting the sandbox apply.
- approver identity is present and non-empty.
- ticket status is `approved`.
- ticket is not expired.
- ticket has not already been consumed.

## Evidence Chain

Expected durable linkage is:

`provider_prompt_transcript` -> `sandbox_patch_preview` -> `approval_ticket_requested` -> `approval_ticket_granted` -> `sandbox_patch_apply` -> patch summary or closure evidence.

Look for emitted events including `approval.requested`, `approval.accepted`, `approval.denied`, and `sandbox.patch.applied`.

## Trust Boundaries

- Do not accept a raw digest as approval in governed provider promotion.
- Do not use `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`.
- Do not apply a changed sandbox after approval; recompute preview immediately before apply.
- Do not reuse consumed, expired, wrong-run, wrong-target, or wrong-requester tickets.
- Do not let CLI, workbench, or provider pilot paths bypass persisted approval evidence.

## Exit Criteria

- The exact approval gate or targeted provider regression gate has been run, or the reason it was not run is stated.
- The evidence pack contains preview, requested ticket, granted ticket, apply artifact, and relevant event log when apply succeeds.
- Denials fail closed and name the mismatched condition.
- Residual risk names any untested path that can still apply a provider sandbox patch.
