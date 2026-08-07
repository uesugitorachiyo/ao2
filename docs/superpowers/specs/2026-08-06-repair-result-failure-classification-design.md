# Repair Result Failure Classification Design

## Purpose

Add one read-only AO2 command that prevents a pre-existing baseline failure from being reported as a candidate regression. The command classifies supplied evidence; it does not run tests or qualify a repair.

## Command

```text
ao2 issue repair-result classify --baseline <baseline.json> --candidate <candidate.json> --json
```

Both inputs use strict, bounded `ao2.github-issue-repair-verification.v1` JSON. They bind repository, issue, baseline source SHA, command digest, toolchain, completion time, role, source SHA, exit code, output digest, and an ordered failure list. Each failure has a stable identifier and a signature digest. The candidate additionally binds its candidate commit SHA. Inputs must be fresh, regular non-symlink files and must agree on all comparison identities.

## Classification

AO2 compares failures by identifier and signature digest:

- shared: identifier and signature are identical;
- resolved: present only on the baseline;
- changed: identifier is shared but its signature differs;
- candidate-only: present only on the candidate.

`candidate_regression` is true when changed or candidate-only failures exist. A nonzero candidate exit with only exact shared baseline failures reports `candidate_regression=false` and `baseline_failures_retained=true`; it never reports the repair as passed. Duplicate identifiers, malformed digests, stale evidence, identity drift, unsafe boundary flags, oversized input, and role mismatch fail before a readback is emitted.

## Boundaries

The command reads two local files, emits deterministic JSON or text, and performs no network, Git, GitHub, provider, repair, mutation, approval, release, deployment, or publication action. Existing repair-pack versions remain unchanged.

## Verification

Integration tests cover a clean candidate, retained baseline failures, resolved failures, candidate-only failures, changed signatures, identity mismatch, stale or malformed evidence, duplicate failures or JSON keys, unsafe boundaries, symlinks, and oversized input. The focused test and `npm run verify` are required before merge.
