# AO2 Workflow Scope Instructions

## Workflow Boundaries

- Keep workflow permissions least-privilege and explicit. Do not add credential, write, release, deployment, or publication authority to a read-only or verification job.
- Preserve artifact producer/consumer contracts: stable names, schemas, source heads, digests, retention expectations, and fail-closed missing-artifact behavior.
- Release and promotion workflows must retain manual/operator gates and dry-run defaults. Readback, a green readiness artifact, or a workflow dispatch alone must not publish.
- Pin or update actions according to the repository's existing supply-chain policy and preserve cross-platform coverage rather than silently dropping a required runner.
- Treat pull-request content, downloaded artifacts, and external output as untrusted. Do not expose secrets to untrusted code or echo credential material.

## Verification

- Run the local contract or dry-run script named for the changed workflow and compare its commands with the workflow.
- Run `git diff --check` and rely on pull-request CI for hosted matrix execution; report any unavailable platform or manually gated job.
- Never dispatch a release, deployment, publication, or live-provider workflow merely to validate this instruction file.
