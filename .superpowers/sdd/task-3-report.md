# Task 3 Report: Strict Hosted Release Consumption

## Status

Complete. The hosted release consumer now validates the exact imported
physical-Windows artifact with the shared strict validator before native
candidate builds or promotion-plan assembly.

## TDD Evidence

### RED

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q \
    -k 'public_release_consumes_only or public_release_promotion_plan_binds'
2 failed, 55 deselected
```

The initial failures showed that the physical verification job exposed no
`physical_evidence_sha256` output and that `assemble-promotion-plan` did not
depend on physical verification.

### GREEN

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q
57 passed

$ python3 -m pytest tests/test_public_stabilization.py -q \
    -k 'canonical_hosted_native_dry_run_contract or physical_windows_import_contract'
2 passed, 214 deselected

$ cargo test -p ao2-cli --test release_packaging \
    release_build_all_script_and_manual_workflow_cover_public_release_sequence
1 passed, 79 filtered out

$ python3 - <<'PY'  # PyYAML contract parse
workflow YAML and physical evidence dependency: ok

$ git diff --check
(exit 0)
```

## Changes

- `verify-physical-windows-qualification` now downloads the named artifact,
  rejects any nested/non-file entry or inventory other than exactly
  `evidence.json` and `summary.json`, and invokes
  `scripts/physical_windows_qualification.py validate` against the immutable
  bound source SHA, discovered release version, and validator current UTC
  time.
- The consumer requires the supplied summary bytes to equal the canonical
  shared-validator output and requires its `physical_evidence_sha256` to equal
  the SHA-256 of the exact downloaded canonical evidence bytes. It exposes that
  validated digest as a job output.
- `assemble-promotion-plan` now depends on physical verification, receives the
  digest only through an explicit environment variable, rejects a non-lowercase
  SHA-256 value, writes `physical_windows_evidence_sha256` into the immutable
  promotion plan, and adds `physical_windows_evidence_mismatch` to the
  rejection policy.
- Existing dry-run and publication boundaries are unchanged.

## Files Changed

- `.github/workflows/public-release-build.yml`
- `tests/test_physical_windows_qualification.py`
- `tests/test_public_stabilization.py`
- `crates/ao2-cli/tests/release_packaging.rs`

## Self-Review

The old recursive first-`summary.json` selection and inline Python assertions
are gone from the physical qualification consumer. The validation work uses no
production `assert`; malformed, stale, failed, source-mismatched,
version-mismatched, or digest-mismatched evidence fails through the shared
validator. The validation sidecar is outside the downloaded readback artifact,
so the artifact inventory remains exactly two files. No runner, permission,
credential, tag, release, upload, deployment, or publication behavior was
added or relaxed.

## Concerns

This local task verifies the contract and YAML but cannot execute the hosted
GitHub Actions download path. The required next exercise remains dispatching
the merged import workflow with fresh physical-Windows evidence, independently
downloading its artifact, and then running the hosted release workflow in
`dry_run=true` mode.
