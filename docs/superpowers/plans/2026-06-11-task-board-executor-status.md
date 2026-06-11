# Task Board Executor Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect Pulse task execution results back into the AI task-board control surface.

**Architecture:** `pulse-task-executor` will emit an `ao2.ai-task-board-status-evidence.v1` artifact keyed by task id. `pulse-next-task-quality-filter` will validate that status evidence references only current task-board ids and generation metadata. `pulse-generate-next` will add `next_action` fields to every generated task-board task and export them to Markdown/HTML/operator views.

**Tech Stack:** Bash wrappers with embedded Python, npm script entrypoints, pytest-based public stabilization tests.

---

### Task 1: Executor Status Evidence

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-task-executor.sh`
- Modify: `docs/VERIFICATION.md`

- [x] **Step 1: Write failing test**

Add a pytest that runs `npm run pulse:task-executor` with one product-code task and one evidence-gate task, then asserts `summary.json` points at `task-board-status-evidence.json` using `ao2.ai-task-board-status-evidence.v1`.

- [x] **Step 2: Run red test**

Run: `PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py::test_pulse_task_executor_emits_task_board_status_evidence -q`

Expected: FAIL because `status_evidence` is absent.

- [x] **Step 3: Implement minimal evidence writer**

Write `task-board-status-evidence.json` after executor results are known. Map materialized product-code packets to `ready`, passing executable gates to `passed`, and failing tasks to `blocked`.

- [x] **Step 4: Run green test**

Run the same pytest selector and expect PASS.

### Task 2: Status Evidence Quality Gate

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-next-task-quality-filter.sh`

- [x] **Step 1: Write failing tests**

Add one test that rejects status evidence with an unknown task id and another that rejects stale task-board generation metadata.

- [x] **Step 2: Run red tests**

Run both selectors and expect FAIL because the quality filter ignores status evidence.

- [x] **Step 3: Implement validation**

Add `AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE`; validate schema, referenced task ids, and `task_board_generation` against `task_board.source_recommendation.generation`.

- [x] **Step 4: Run green tests**

Run both selectors and expect PASS.

### Task 3: Operator Next Actions

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-generate-next.sh`
- Modify: `scripts/pulse-generate-next-contract.sh`
- Modify: `scripts/control-plane-fixture-consumer-smoke.sh`
- Modify: `docs/release/v0.4.81-ai-task-board-control-surface.md`

- [x] **Step 1: Write failing test**

Assert each generated task-board task has a non-empty `next_action`, and that board Markdown/HTML include it.

- [x] **Step 2: Run red test**

Run the selector and expect FAIL because `next_action` is missing.

- [x] **Step 3: Implement next action rendering**

Populate `next_action` from executable command, product-code verification command, or task objective. Render it in task-board Markdown/HTML and the control-plane operator view.

- [x] **Step 4: Run green test**

Run the selector and expect PASS.

### Task 4: Verification And Publish

**Files:**
- All modified files above.

- [x] Run bash syntax checks for changed scripts.
- [x] Run focused pytest selectors.
- [x] Run `npm run pulse:generate-next:contract`.
- [x] Run `npm run control-plane:fixture-consumer-smoke`.
- [x] Run `PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q`.
- [x] Commit, push, and open a draft PR.
