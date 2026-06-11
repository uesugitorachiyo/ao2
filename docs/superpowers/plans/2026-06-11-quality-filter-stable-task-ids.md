# Quality Filter Stable Task IDs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `pulse:next-task-quality-filter` accept task-board status evidence keyed by either generation-specific task IDs or stable task IDs.

**Architecture:** Reuse the standalone AO2 task-board model from `pulse:generate-next`: keep generation-specific `task_id` values for traceability, add every task's `stable_task_id` to the quality filter's allowed evidence key set, and preserve the existing stale-generation and unknown-task fail-closed behavior.

**Tech Stack:** Bash wrapper with embedded Python, npm scripts, pytest public stabilization tests.

---

### Task 1: Accept Stable Evidence IDs

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `scripts/pulse-next-task-quality-filter.sh`

- [x] **Step 1: Write the failing test**

Add `test_pulse_next_task_quality_filter_accepts_stable_status_evidence_task_id` near the existing quality-filter status-evidence tests. The test should create a task board with:

```json
{
  "schema_version": "ao2.ai-task-board.v1",
  "status": "ready",
  "release_objective": "Expose Pulse work as an operator-readable task board.",
  "source_recommendation": {"generation": 7},
  "tasks": [
    {
      "task_id": "complete-task-g7",
      "stable_task_id": "complete-task",
      "title": "Complete task",
      "status": "proposed",
      "required_evidence": ["ao2.ai-task-board.v1"],
      "stop_conditions": ["Stop if readback requires credentials."]
    }
  ],
  "trust_boundary": {"local_only": true, "stores_credentials": false}
}
```

and status evidence keyed by `complete-task`. Assert the command exits `0`, `status_evidence_gate == "passed"`, and `status_evidence_blockers == []`.

- [x] **Step 2: Run the focused red test**

Run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py::test_pulse_next_task_quality_filter_accepts_stable_status_evidence_task_id -q
```

Expected: FAIL with `status_evidence_unknown_task_id:complete-task`.

- [x] **Step 3: Implement stable ID allow-listing**

In `scripts/pulse-next-task-quality-filter.sh`, collect both `task_id` and non-empty `stable_task_id` values into `task_board_task_ids` while reading the task board. Do not loosen generation checks or accept arbitrary unknown IDs.

- [x] **Step 4: Run the focused green test**

Run the same selector and expect PASS.

### Task 2: Contract And Docs

**Files:**
- Modify: `tests/test_public_stabilization.py`
- Modify: `docs/VERIFICATION.md`
- Modify: `docs/release/v0.4.81-ai-task-board-control-surface.md`

- [x] **Step 1: Extend static contract coverage**

Update `test_pulse_generate_next_auto_registration_contract` to assert the quality filter script and verification docs mention `stable_task_id`.

- [x] **Step 2: Update docs**

Document that `pulse:next-task-quality-filter` accepts status evidence keyed by generation-specific `task_id` or stable `stable_task_id`, while still rejecting stale generations and truly unknown IDs.

- [x] **Step 3: Run focused contract checks**

Run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py::test_pulse_generate_next_auto_registration_contract tests/test_public_stabilization.py::test_pulse_next_task_quality_filter_accepts_stable_status_evidence_task_id tests/test_public_stabilization.py::test_pulse_next_task_quality_filter_rejects_unknown_status_evidence_task_id tests/test_public_stabilization.py::test_pulse_next_task_quality_filter_rejects_stale_status_evidence_generation -q
```

Expected: PASS.

### Task 3: Verification And Publish

- [x] Run `bash -n scripts/pulse-next-task-quality-filter.sh`.
- [x] Run `npm run pulse:next-task-quality-filter` against the latest generated AO2 packet/board if present.
- [x] Run `PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q`.
- [x] Commit, push, and open a PR.
