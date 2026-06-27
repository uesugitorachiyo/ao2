# Cleanup Agent Loop

This loop is intentionally bounded. It may improve task records, prompts,
checklists, and validation choices, but it must not silently expand scope or
publish changes.

## Loop Contract

1. One task record.
2. One cleanup category.
3. One allowed path set.
4. One validation plan.
5. One diff review.
6. One handoff summary.

## Step 1: Scan

Run:

```sh
bash scripts/refactor-scan.sh
git status --short
```

Inspect files with `rg` and direct reads before editing.

## Step 2: Propose

Create a task record from `REFACTOR_TASK_TEMPLATE.md`. The record must reject
vague scope and state exactly which paths can change.

## Step 3: Edit

Make a small change. Stop if the task requires files outside the allowed path
set.

## Step 4: Validate

Run the declared command. If it fails, either repair within scope or record the
failure and stop.

## Step 5: Review

Run:

```sh
git diff --stat
git diff --check
```

Read the hunks. Do not rely on line counts alone.

## Step 6: Record

Fill in `REFACTOR_AGENT_HANDOFF_TEMPLATE.md` or update the task record with:

- changed files;
- validation evidence;
- known risks;
- rollback commands;
- one next safe task, if any.

## Step 7: Stop Or Continue By New Task

Continue only by creating a new task record. The next task must have its own
category, scope, validation, and approval threshold.
