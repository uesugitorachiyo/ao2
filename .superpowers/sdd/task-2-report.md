# Task 2 Report: Read-Only Physical Windows Import Workflow

## Status

Task 2 review findings fixed and ready for re-review.

## TDD Evidence

### RED

The first workflow-contract run failed because the workflow did not exist:

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q -x
1 failed, 43 passed
```

The failure was `FileNotFoundError` for
`.github/workflows/import-physical-windows-qualification.yml`.

One additional boundary test was added for checkout credential persistence:

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q \
  -k import_workflow_is_manual_read_only_and_binds_exact_source
1 failed, 45 deselected
```

The failure required `persist-credentials: false` on the exact-source checkout.

The review-fix RED run required a fixed importer, environment-only payload
transport, direct behavior coverage, and explicit artifact inventory:

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q \
  -k 'import_script or import_workflow'
10 failed, 2 passed, 43 deselected
```

The workflow test found the old embedded wrapper. The other nine failures were
`FileNotFoundError` for the not-yet-created fixed importer script.

### GREEN

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q
55 passed in 1.37s

$ python3 -m pytest tests/test_public_stabilization.py -q \
  -k 'physical_windows_import_contract or canonical_hosted_native_dry_run_contract'
2 passed, 214 deselected in 0.59s

$ python3 - <<'PY'  # PyYAML BaseLoader workflow and fixed import check
workflow YAML parsed and fixed import contract verified

$ python3 -m py_compile \
  scripts/physical_windows_qualification.py \
  scripts/import_physical_windows_qualification.py
(exit 0)

$ git diff --check
(exit 0)
```

## Files Changed

- `.github/workflows/import-physical-windows-qualification.yml`
- `scripts/import_physical_windows_qualification.py`
- `tests/test_physical_windows_qualification.py`
- `tests/test_public_stabilization.py`

## Contract

- The workflow is `workflow_dispatch` only and accepts exactly
  `evidence_base64`, `evidence_sha256`, `source_sha`, and `version`.
- Top-level and job permissions are only `contents: read`; it uses
  `ubuntu-latest`, an exact SHA checkout, and `persist-credentials: false`.
- The workflow invokes only
  `python3 scripts/import_physical_windows_qualification.py`, with no arguments
  or task-controlled command/path input.
- The fixed script reads the five dispatch bindings only from environment
  variables. The payload is never placed in argv or a child command line.
- The script verifies source/digest syntax, exact repository `HEAD`,
  `GITHUB_SHA`, and discovered version before calling
  `decode_import_payload` and `validate_evidence` directly from
  `physical_windows_qualification.py`.
- The existing validator owns encoded/decoded limits, strict Base64, SHA-256,
  canonical JSON, strict schema, freshness, source, and version validation.
- Canonical evidence and summary bytes are written to a new private staging
  directory with file-level replacement, recursively checked for exactly
  `evidence.json` and `summary.json`, and promoted by a same-parent directory
  rename. Failures remove staging and leave no destination artifact.
- It uploads exactly one bounded-retention artifact named
  `ao2-physical-windows-qualification`, with the upload path explicitly listing
  only the two contract files.
- YAML contract tests use PyYAML's `BaseLoader`, not ad hoc text parsing.

## Validator Scope

The validator was not modified. The fixed importer imports its existing
functions directly, so there is no second importer process and no payload in a
child process command line.

## Self-Review

The workflow contains no self-hosted runner, writable permission, persisted
credential, secret reference, release/tag/deployment/publication primitive,
schedule, reusable trigger, or input-defined command path. Direct tests use
temporary git repositories and cover successful canonical materialization, bad
digest/source/version bindings, strict validation failure, preexisting and
extra-file inventory, staging cleanup after an injected write failure, and the
exact fixed child argv inventory.

## Concerns

The hosted workflow has not yet been dispatched because that exercise requires
the merged AO2 source and fresh exact-head physical-Windows evidence. Those are
the planned post-merge tasks; no release, tag, deployment, or public upload was
attempted here.
