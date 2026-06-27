# Refactor Agent Handoff Template

Use this when passing a cleanup task to another agent or recording a completed
cleanup slice.

## Scope

- Repo:
- Branch:
- Task record:
- Cleanup category:
- Allowed paths:
- Forbidden paths:

## Context

Summarize the relevant code/docs behavior and link the files inspected.

## Current Workspace State

Paste or summarize:

```sh
git status --short
bash scripts/refactor-scan.sh
```

Call out unrelated dirty files.

## Work Completed Or Requested

List concrete edits requested or completed. Keep this to the current cleanup
category.

## Validation Evidence

For each command:

- command:
- exit code:
- important output:
- artifact path, if any:

## Diff Summary

Include:

```sh
git diff --stat
git diff --check
```

Summarize risky hunks or explicitly state that none were found.

## Risks And Follow-Ups

- Missing tests:
- Ambiguous ownership:
- Commands not run:
- Suggested next cleanup task:

## Rollback

Provide exact commands for the files owned by this handoff only.
