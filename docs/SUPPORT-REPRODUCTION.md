# AO2 Support Reproduction Checklist

Use this checklist when opening a public AO2 support issue or when reproducing
a workflow reliability problem locally. Keep the report small and public-safe.

Do not paste credentials, tokens, provider secrets, private repository contents,
private logs, private evidence values, or unreleased customer data into a public
issue.

## Required Context

- AO2 version: paste `ao2 version --json` output after removing local paths if
  needed.
- platform: include OS, CPU architecture, shell, and install method.
- command: include the exact AO2 command that failed.
- expected result: state what the docs or command help said should happen.
- actual result: include the status, error category, and short redacted output.
- Evidence path: include only the path category and basename when the path is
  private, for example `.ao2/runs/<run-id>/run-record.json`.
- Checksum state: say whether archive checksum verification passed, failed, or
  was not run.
- Manifest state: say whether release or workflow manifest verification passed,
  failed, or was not run.

## Approval And Replay Cases

Include these fields when the issue involves approval or replay behavior:

- approval status, for example `pending`, `approved`, or `rejected`
- required digest field name, usually `action_digest`
- action digest shown by AO2
- resume or approval command shown by AO2
- replay state, for example `waiting_for_approval`, `accepted`, or `rejected`
- digest failure summary, if AO2 prints one
- evidence path where AO2 wrote the run record

Do not approve a digest mismatch just to continue a reproduction. Preserve the
failing state and report the mismatch category.

## Manifest Or Checksum Cases

Include these fields when the issue involves a manifest or checksum mismatch:

- manifest SHA-256
- release or staging directory, with private path segments redacted
- asset filename or basename
- observed failure category
- expected hash, when it is already public-safe
- observed hash, when it is already public-safe
- exact verifier command
- sanitized logs

- missing asset
- unexpected asset
- hash mismatch
- duplicate basename
- malformed hash
- disallowed path or traversal
- manifest digest mismatch

Do not paste full private directory listings. Include asset basenames and the
command output category.
