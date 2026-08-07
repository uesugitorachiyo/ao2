# Repair Result Failure Classification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a strict read-only AO2 command that distinguishes retained baseline failures from candidate regressions.

**Architecture:** A new issue subcommand reads two existing bounded-input JSON files, validates comparison identity and safety, and compares stable failure identifiers plus signature digests. It emits one deterministic readback and does not execute tests or mutate state.

**Tech Stack:** Rust, clap, serde/serde_json, chrono, sha2, existing AO2 bounded-input helpers.

## Global Constraints

- No new dependency or generic test-output parser.
- Existing repair-pack v1/v2 behavior remains unchanged.
- Inputs are strict regular non-symlink files of at most 65,536 bytes each.
- The command performs no network, Git, GitHub, provider, repair, mutation, approval, release, deployment, or publication action.

---

### Task 1: Comparison Contract

**Files:**
- Create: `crates/ao2-cli/tests/github_issue_repair_result.rs`
- Create: `crates/ao2-cli/src/github_issue_repair_result.rs`
- Modify: `crates/ao2-cli/src/cli.rs`
- Modify: `crates/ao2-cli/src/github_issue_intake.rs`

**Interfaces:**
- Consumes: `ao2.github-issue-repair-verification.v1` baseline and candidate JSON paths.
- Produces: `ao2.github-issue-repair-result-classification.v1` readback.

- [ ] **Step 1: Write a failing retained-baseline test**

Create strict baseline and candidate fixtures with one identical nonzero failure. Invoke `ao2 issue repair-result classify --baseline ... --candidate ... --json` and assert `candidate_regression=false`, `baseline_failures_retained=true`, and one shared failure.

- [ ] **Step 2: Verify RED**

Run `cargo test -p ao2-cli --test github_issue_repair_result -- --nocapture`. Expect clap to reject the missing `repair-result` command.

- [ ] **Step 3: Implement the minimal classifier**

Add `IssueCommand::RepairResult`, dispatch it from `github_issue_intake.rs`, read both summaries with `github_issue_draft::read_bounded_bytes`, parse strict structs, validate repository/SHA/digest/timestamp/safety/role and matching comparison identities, reject duplicate failure identifiers, then partition failures into shared, resolved, changed, and candidate-only sets.

- [ ] **Step 4: Verify GREEN**

Run the focused integration test and expect it to pass.

- [ ] **Step 5: Commit the first behavior**

Commit only the four Task 1 paths.

### Task 2: Fail-Closed Cases And Public Contract

**Files:**
- Modify: `crates/ao2-cli/tests/github_issue_repair_result.rs`
- Modify: `crates/ao2-cli/src/github_issue_repair_result.rs`
- Modify: `docs/contracts/GITHUB-ISSUE-REPAIR-PACK.md`

**Interfaces:**
- Consumes: the Task 1 command and schemas.
- Produces: complete positive/negative coverage and operator documentation.

- [ ] **Step 1: Add failing tests one behavior at a time**

Cover clean, resolved, candidate-only, changed-signature, wrong role, identity mismatch, duplicate failure identifiers, duplicate JSON keys, stale timestamp, malformed digest, unsafe boundary, symlink, and oversized input. Verify each new case fails for the missing validation before implementation.

- [ ] **Step 2: Add only the validation required by each RED test**

Keep validation local to the classifier and reuse existing bounded-file input code. Sort all emitted failure arrays by identifier for deterministic output.

- [ ] **Step 3: Document the command and non-authority boundary**

Add a repair-result classification section to the existing GitHub issue repair contract. State explicitly that `candidate_regression=false` is not a repair-passed or merge-qualified verdict.

- [ ] **Step 4: Run focused and broad verification**

Run `cargo fmt --all -- --check`, the focused integration test, `npm run verify`, and `git diff --check`.

- [ ] **Step 5: Commit and open one bounded PR**

Push `codex/repair-result-failure-classification`, open one ready PR, wait for required checks, merge only when green, synchronize `main`, and remove the branch and worktree.
