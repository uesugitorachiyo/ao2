# Task Board State UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make AO2 task-board status carry across generated board generations while exposing current board state without regeneration.

**Architecture:** Keep generation-suffixed `task_id` values for traceability, add `stable_task_id` for cross-generation matching, and let status evidence match either exact or stable IDs. Add `pulse:task-board-state` as a read-only local command that reports the latest board summary/state summary. Render stale evidence warnings in Markdown/HTML.

**Tech Stack:** Bash wrappers with embedded Python, npm scripts, pytest public stabilization tests.

---

### Task 1: Stable Task IDs

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-generate-next.sh`

- [x] **Step 1: Write failing test**

Add a test that generates generation 1, writes status evidence keyed by generation 1 task IDs, then generates generation 2 and asserts statuses carry via `stable_task_id`.

- [x] **Step 2: Run red test**

Run the focused selector and expect FAIL.

- [x] **Step 3: Implement stable matching**

Add `stable_task_id` by stripping trailing `-gN`, and match status evidence by exact task id or stable id.

- [x] **Step 4: Run green test**

Run the focused selector and expect PASS.

### Task 2: Task Board State Reader

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `package.json`
- Create: `scripts/pulse-task-board-state.sh`
- Modify: `docs/VERIFICATION.md`

- [x] **Step 1: Write failing test**

Add tests for `npm run pulse:task-board-state` and package script exposure.

- [x] **Step 2: Run red test**

Run focused selectors and expect FAIL.

- [x] **Step 3: Implement read-only command**

Read latest board and board-state summary, emit `ao2.pulse-task-board-state.v1`.

- [x] **Step 4: Run green test**

Run focused selectors and expect PASS.

### Task 3: Stale Evidence UX

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-generate-next.sh`
- Modify: `docs/release/v0.4.81-ai-task-board-control-surface.md`

- [x] **Step 1: Write failing test**

Add a test that stale status evidence appears as ignored in board Markdown and HTML.

- [x] **Step 2: Run red test**

Run focused selector and expect FAIL.

- [x] **Step 3: Implement UX text**

Render stale generation status in Markdown/HTML and document the release note.

- [x] **Step 4: Run green test**

Run focused selector and expect PASS.

### Task 4: Verification And Publish

- [x] Run bash syntax checks.
- [x] Run focused pytest selectors.
- [x] Run `npm run pulse:generate-next:contract`.
- [x] Run `npm run pulse:task-board-state`.
- [x] Run full public stabilization tests.
- [ ] Commit, push, and open a draft PR.
