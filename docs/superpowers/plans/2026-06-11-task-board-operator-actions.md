# Task Board Operator Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve AO2-alone task-board operations by making state failures inspectable, adding a compact next-actions command, and exposing quality-filter status-evidence match telemetry.

**Architecture:** Keep all commands local-first and AO2-owned. `pulse:task-board-state` remains the source of current board state and gets explicit failure summaries; `pulse:next-actions` reads the current board directly and emits a compact operator artifact; `pulse:next-task-quality-filter` reports whether status evidence matched generated `task_id` or stable `stable_task_id`.

**Tech Stack:** Bash wrappers with embedded Python, npm scripts, pytest public stabilization tests.

---

### Task 1: State Reader Failure Summaries

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-task-board-state.sh`

- [x] **Step 1: Write failing tests**

Add tests for missing board, invalid schema, and invalid JSON. Each test should run `npm run pulse:task-board-state`, expect a non-zero exit, and assert `summary.json` exists with `schema_version == "ao2.pulse-task-board-state.v1"`, `status == "failed"`, and a reason of `task_board_missing`, `task_board_schema_invalid`, or `task_board_invalid_json:<line>`.

- [x] **Step 2: Run focused red tests**

Run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py::test_pulse_task_board_state_reports_missing_board tests/test_public_stabilization.py::test_pulse_task_board_state_reports_invalid_board_schema tests/test_public_stabilization.py::test_pulse_task_board_state_reports_invalid_board_json -q
```

Expected: at least invalid JSON fails because the script exits before writing a summary.

- [x] **Step 3: Implement JSON failure summary**

Update `scripts/pulse-task-board-state.sh` so board JSON is parsed once inside `try/except json.JSONDecodeError`; invalid JSON should write `reason: task_board_invalid_json:<line>` before exiting non-zero.

- [x] **Step 4: Run focused green tests**

Run the same selector and expect PASS.

### Task 2: Compact Next Actions Command

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `package.json`
- Create: `scripts/pulse-next-actions.sh`
- Modify: `docs/VERIFICATION.md`
- Modify: `docs/release/v0.4.81-ai-task-board-control-surface.md`

- [x] **Step 1: Write failing test**

Add `test_pulse_next_actions_reads_current_board_actions` that generates a task board, runs `npm run pulse:next-actions` with `AO2_PULSE_NEXT_ACTIONS_BOARD` and `AO2_PULSE_NEXT_ACTIONS_ROOT`, and asserts an `ao2.pulse-next-actions.v1` summary with `status == "passed"`, non-empty `next_actions`, and local-only/no-credentials trust boundary.

- [x] **Step 2: Run focused red test**

Run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py::test_pulse_next_actions_reads_current_board_actions -q
```

Expected: FAIL because `pulse:next-actions` does not exist.

- [x] **Step 3: Implement command**

Create `scripts/pulse-next-actions.sh` that reads the current task board, writes `target/pulse-next-actions/latest/summary.json` with schema `ao2.pulse-next-actions.v1`, writes a compact `next-actions.md`, prints the action lines to stdout, and exits non-zero with a summary for missing/invalid board inputs.

- [x] **Step 4: Run focused green test**

Run the same selector and expect PASS.

### Task 3: Quality Filter Match Telemetry

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-next-task-quality-filter.sh`

- [x] **Step 1: Write failing telemetry assertions**

Extend the stable-ID quality-filter test to assert `status_evidence_matches == [{"evidence_task_id": "complete-task", "task_id": "complete-task-g7", "stable_task_id": "complete-task", "matched_by": "stable_task_id"}]` and `status_evidence_match_counts == {"task_id": 0, "stable_task_id": 1}`.

- [x] **Step 2: Run focused red test**

Run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py::test_pulse_next_task_quality_filter_accepts_stable_status_evidence_task_id -q
```

Expected: FAIL because telemetry fields are missing.

- [x] **Step 3: Implement telemetry**

Replace the quality filter's allowed-ID set with an ID metadata map, then append match records for accepted status evidence keys and count `task_id` vs `stable_task_id` matches. Preserve the existing unknown-ID and stale-generation blockers.

- [x] **Step 4: Run focused green test**

Run the same selector and expect PASS.

### Task 4: Verification And Publish

- [x] Run focused selectors for all new/changed behavior.
- [x] Run `bash -n scripts/pulse-task-board-state.sh scripts/pulse-next-actions.sh scripts/pulse-next-task-quality-filter.sh`.
- [x] Run `npm run pulse:next-actions` against the current generated board when present.
- [x] Run `PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q`.
- [ ] Commit, push, and open a PR.
