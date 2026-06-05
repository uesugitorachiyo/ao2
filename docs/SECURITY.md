# AO2 Security Notes

AO2 starts from fail-closed local governance.

## Hard Rules

- `OPENAI_API_KEY` and `ANTHROPIC_API_KEY` are forbidden in the runtime
  environment.
- External writes, `git push`, package install, publish, deploy, destructive
  commands, raw secret access, parent-directory traversal, and network egress
  are blocked or require explicit policy handling.
- Approval grants are bound to the exact action digest.
- Evidence must be written before evaluator acceptance.
- Artifacts include digest, producer, lineage, and sensitivity metadata.

## Secret Handling

The MVP avoids provider API-key paths entirely. Future provider integrations must
use local CLI OAuth/session authentication and must keep transcripts redacted
before persistence.

## Current MVP Limit

The first provider-free run simulates the local human approval path so the full
closure loop can be exercised deterministically. A later interactive approval
slice should pause the run and resume after `ao2 approve`.
