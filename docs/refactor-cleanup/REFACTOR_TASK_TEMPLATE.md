# Refactor Task Template

Copy this file into a new task record before editing product code.

## Task

- Task id:
- Date:
- Owner or agent:
- Cleanup category:
- Target files or modules:
- Allowed changed paths:
- Paths that must not change:

## Objective

State the smallest useful cleanup outcome in one sentence.

## Non-Goals

- No broad repository rewrite.
- No behavior change unless covered by the validation plan below.
- No generated, release, secret, deployment, or lockfile changes unless named in
  allowed changed paths.

## Current State

Record:

```sh
git status --short
bash scripts/refactor-scan.sh
```

Summarize unrelated dirty files and risky paths.

## Proposed Change

Describe the planned edit and why it is safe.

## Validation Plan

Run the narrowest command that proves the task:

```sh
bash scripts/refactor-check.sh docs
```

Replace the command when the task touches Rust, shell scripts, tests, or
cross-repo evidence behavior.

Expected evidence:

- command:
- expected exit code:
- expected output marker:

## Diff Review Checklist

- [ ] One cleanup category only.
- [ ] All changed files are in allowed changed paths.
- [ ] No unrelated dirty files modified.
- [ ] No generated/cache/release artifacts modified.
- [ ] `git diff --check` passes.
- [ ] Declared validation command ran.
- [ ] Behavior-changing edits have tests or added validation.

## Result

- Status:
- Commands run:
- Files changed:
- Validation evidence:
- Follow-up tasks:

## Rollback

List exact rollback commands for only this task's owned files.
