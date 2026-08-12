# Physical Bounded Host Lease Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a strict bounded shared physical-host lease that permits multiple SSH and unrelated interactive work for fixed lifecycle checks without weakening exclusive release qualification.

**Architecture:** Extend the existing strict lease parser with one separate bounded schema and profile allowlist. Reuse the current digest, timestamp, path, and safety validation; branch only where bounded coexistence replaces whole-host exclusivity.

**Tech Stack:** Python standard library, pytest, JSON documentation contracts.

## Global Constraints

- Preserve exclusive v1/v2 behavior and `physical_unique` requirements.
- Permit multiple SSH connections and unrelated interactive/Codex workloads only in bounded lifecycle profiles.
- Reject exact workload, lease, scratch, action, and resource conflicts before execution.
- Add no dependencies, provider paths, credentials, arbitrary commands, session mutation, release, deployment, or publication.

---

### Task 1: Bounded Lease Contract

**Files:**
- Modify: `tests/test_windows_outbound_worker.py`
- Modify: `scripts/ao2_windows_outbound_worker.py`

**Interfaces:**
- Consumes: existing `validate_physical_host_lease(...)` strict parser.
- Produces: support for `ao2.physical-host-bounded-lease.v1` on the fixed `ubuntu_stack_qualification:lifecycle_noop` and `windows_stack_qualification:lifecycle_noop` profiles.

- [ ] Add a failing test that accepts multiple sessions, SSH connections, and unrelated AO workloads under a conflict-free bounded lease.
- [ ] Run the focused test and verify `lease_schema_mismatch` or `unsafe_command_profile`.
- [ ] Add the bounded schema/profile constants and minimum validator branch.
- [ ] Run the focused test and verify it passes.
- [ ] Add table-driven negative tests for conflicting leases, workloads, scratch roots, unsatisfied resource limits, wrong isolation mode, and bounded evidence supplied to `physical_unique`.
- [ ] Run the focused lease test group and verify all cases pass.

### Task 2: Canonical Documentation

**Files:**
- Modify: `AGENTS.md`
- Modify: `scripts/AGENTS.md`
- Modify: `docs/windows-outbound-worker.md`
- Modify: `docs/windows-stack-qualification-inventory.json`

**Interfaces:**
- Consumes: the tested schema and profile names from Task 1.
- Produces: canonical operator and inventory documentation matching implemented behavior.

- [ ] Document bounded shared as the default for lifecycle checks and exclusive leases only for release-sensitive host-global profiles.
- [ ] State explicitly that SSH count and unrelated interactive activity are not conflicts.
- [ ] Validate JSON and run the instruction-layout verifier.

### Task 3: Verification And Native Coexistence Canary

**Files:**
- Create only ignored evidence beneath `target/physical-bounded-lease/`.

**Interfaces:**
- Consumes: the merged contract candidate and offline validator CLI.
- Produces: focused/full gate results and two native bounded coexistence readbacks.

- [ ] Run focused pytest, full worker tests, formatting or syntax checks, `npm run verify`, and `git diff --check`.
- [ ] Validate one fresh five-minute bounded lease on Ubuntu and Windows while unrelated sessions remain active; do not sign out, lock, stop, or clean unrelated work.
- [ ] Record exact source SHA, lease digest, session/SSH counts, exit status, and safety boundaries.
- [ ] Open one bounded AO2 pull request, wait for required CI, merge only when green, synchronize `main`, and remove the task branch/worktree.
