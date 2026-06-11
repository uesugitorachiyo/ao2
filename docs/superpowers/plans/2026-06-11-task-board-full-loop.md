# Task Board Full Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make AO2's standalone Pulse task-board loop carry executor status evidence into the next generated board and expose a compact board-state summary.

**Architecture:** `pulse:generate-next` will auto-discover AO2's own `target/pulse-task-executor/latest/task-board-status-evidence.json` when no explicit status evidence env var is set. The generator will emit `board-state-summary.json` as a small dashboard/control-plane-friendly summary beside the full board. Tests and CI contracts will exercise the local-only loop: generate board, execute a fixture manifest, validate status evidence, regenerate board with updated statuses.

**Tech Stack:** Bash wrappers with embedded Python, npm script entrypoints, pytest public stabilization tests, GitHub Actions YAML.

---

### Task 1: Auto-Discover Executor Status Evidence

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-generate-next.sh`

- [x] **Step 1: Write the failing test**

Add `test_pulse_generate_next_auto_discovers_executor_status_evidence` to seed `target/pulse-task-executor/latest/task-board-status-evidence.json`, run `pulse:generate-next` without `AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE`, and assert generated task statuses are updated.

- [x] **Step 2: Run red test**

Run: `PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py::test_pulse_generate_next_auto_discovers_executor_status_evidence -q`

Expected: FAIL because auto-discovery does not exist yet.

- [x] **Step 3: Implement minimal auto-discovery**

Set the default status evidence path to `target/pulse-task-executor/latest/task-board-status-evidence.json` when the env var is absent.

- [x] **Step 4: Run green test**

Run the same selector and expect PASS.

### Task 2: Board State Summary

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-generate-next.sh`
- Modify: `scripts/control-plane-fixture-consumer-smoke.sh`

- [x] **Step 1: Write the failing test**

Assert `pulse:generate-next` writes `board-state-summary.json` with schema `ao2.ai-task-board-state-summary.v1`, status counts, next actions, and read-only trust boundary.

- [x] **Step 2: Run red test**

Run the new selector and expect FAIL because the summary artifact is missing.

- [x] **Step 3: Implement summary writer**

Create `board-state-summary.json` beside `summary.json`, reference it from `exports`, and let fixture consumer mirror it into operator view metadata when present.

- [x] **Step 4: Run green test**

Run the selector and expect PASS.

### Task 3: Full Loop Verification Contract

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/VERIFICATION.md`
- Modify: `docs/release/v0.4.81-ai-task-board-control-surface.md`
- Modify: `scripts/pulse-generate-next-contract.sh`

- [x] **Step 1: Write failing test**

Add `test_pulse_task_board_full_loop_generate_execute_validate_regenerate` and a static CI assertion that the focused full-loop selector runs in the Python guard shard.

- [x] **Step 2: Run red tests**

Run both selectors and expect FAIL until implementation/docs/contracts are updated.

- [x] **Step 3: Implement CI/docs/contracts**

Add the focused selector to the Python guard command comments or command surface, update docs, and add contract needles.

- [x] **Step 4: Run green tests**

Run focused selectors and expect PASS.

### Task 4: Verification And Publish

**Files:**
- All modified files above.

- [x] Run bash syntax checks for changed scripts.
- [x] Run `npm run pulse:generate-next:contract`.
- [x] Run focused pytest selectors.
- [x] Run `npm run control-plane:fixture-consumer-smoke`.
- [x] Run `PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q`.
- [ ] Commit, push, and open a draft PR.
