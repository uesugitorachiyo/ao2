# Next Actions Filters Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make AO2's compact next-actions command more operator-friendly and finish quality-filter match telemetry coverage.

**Architecture:** Keep `pulse:next-actions` as a local read-only board reader. Add explicit failure Markdown for bad board inputs, a status allow-list filter driven by `AO2_PULSE_NEXT_ACTIONS_STATUS`, and exact `task_id` telemetry coverage for the quality filter without changing its fail-closed behavior.

**Tech Stack:** Bash wrappers with embedded Python, npm scripts, pytest public stabilization tests.

---

### Task 1: Next Actions Failure Coverage

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-next-actions.sh`

- [x] **Step 1: Write failing tests**

Add missing-board, invalid-schema, and invalid-JSON tests for `pulse:next-actions`. Each test should assert a failed summary is written and `next-actions.md` includes the failure reason.

- [x] **Step 2: Run focused red tests**

Run the three new selectors and expect failure because failure Markdown does not include the reason.

- [x] **Step 3: Implement failure Markdown**

Update `scripts/pulse-next-actions.sh` to render `Reason: <reason>` in `next-actions.md` when the command fails.

- [x] **Step 4: Run focused green tests**

Run the same selectors and expect PASS.

### Task 2: Status Filtering

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-next-actions.sh`
- Modify: `docs/VERIFICATION.md`
- Modify: `docs/release/v0.4.81-ai-task-board-control-surface.md`

- [x] **Step 1: Write failing filter test**

Add `test_pulse_next_actions_filters_by_status` with a hand-written board containing `proposed`, `blocked`, and `passed` tasks. Run with `AO2_PULSE_NEXT_ACTIONS_STATUS=proposed,blocked` and assert only those statuses are emitted plus a `status_filter` field.

- [x] **Step 2: Run focused red test**

Run the new selector and expect failure because no status filter exists.

- [x] **Step 3: Implement filter**

Parse comma-separated `AO2_PULSE_NEXT_ACTIONS_STATUS`, normalize whitespace/lowercase, include `status_filter` in the summary, and filter actions when the allow-list is non-empty.

- [x] **Step 4: Run focused green test**

Run the selector and expect PASS.

### Task 3: Exact Task-ID Telemetry Coverage

**Files:**
- Modify: `tests/test_public_stabilization.py`

- [x] **Step 1: Write exact-match telemetry assertions**

Extend the existing quality-filter status-transition test keyed by generated task ID to assert `status_evidence_matches` includes `matched_by: task_id` and `status_evidence_match_counts.task_id == 2`.

- [x] **Step 2: Run focused test**

Run the selector and expect PASS if the merged telemetry implementation already supports exact IDs.

### Task 4: Verification And Publish

- [x] Run focused selectors for all changed behavior.
- [x] Run `bash -n scripts/pulse-next-actions.sh scripts/pulse-next-task-quality-filter.sh`.
- [x] Run `npm run pulse:next-actions`.
- [x] Run `PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q`.
- [ ] Commit, push, and open a PR.
