# Task 2 Report: Read-Only Physical Windows Import Workflow

## Status

Complete and ready for the Task 2 review gate.

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

### GREEN

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q
46 passed in 0.12s

$ python3 -m pytest tests/test_public_stabilization.py -q \
  -k 'physical_windows_import_contract or canonical_hosted_native_dry_run_contract'
2 passed, 214 deselected in 0.58s

$ python3 - <<'PY'  # PyYAML BaseLoader workflow parse
workflow YAML parsed with PyYAML BaseLoader

$ python3 -m py_compile scripts/physical_windows_qualification.py
(exit 0)

$ git diff --check
(exit 0)
```

## Files Changed

- `.github/workflows/import-physical-windows-qualification.yml`
- `tests/test_physical_windows_qualification.py`
- `tests/test_public_stabilization.py`

## Contract

- The workflow is `workflow_dispatch` only and accepts exactly
  `evidence_base64`, `evidence_sha256`, `source_sha`, and `version`.
- Top-level and job permissions are only `contents: read`; it uses
  `ubuntu-latest`, an exact SHA checkout, and `persist-credentials: false`.
- The fixed Python block reads dispatch values only from environment variables.
  It validates source SHA, checkout `HEAD`, `github.sha`, source version,
  encoded/decoded limits, strict Base64, and SHA-256 before invoking the
  existing validator with an argv list and no shell evaluation.
- The block writes the decoded canonical bytes to
  `target/physical-windows-qualification/evidence.json` and the validated
  importer stdout to `target/physical-windows-qualification/summary.json`.
- It uploads exactly one bounded-retention artifact named
  `ao2-physical-windows-qualification`.
- YAML contract tests use PyYAML's `BaseLoader`, not ad hoc text parsing.

## Validator Scope

The validator was not modified. The existing `import` CLI already validates the
strict evidence schema, current freshness, source/version bindings, canonical
JSON, digest, and payload limits. The workflow's fixed Python block supplies
the environment-fed payload to that CLI through `subprocess.run(..., shell=False)`
and materializes its two required files.

## Self-Review

The workflow contains no self-hosted runner, writable permission, persisted
credential, secret reference, release/tag/deployment/publication primitive,
schedule, reusable trigger, or input-defined command path. The static test
suite scans these forbidden primitives and confirms the only artifact upload is
the required evidence artifact.

## Concerns

The hosted workflow has not yet been dispatched because that exercise requires
the merged AO2 source and fresh exact-head physical-Windows evidence. Those are
the planned post-merge tasks; no release, tag, deployment, or public upload was
attempted here.
