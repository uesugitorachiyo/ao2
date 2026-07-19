# Task 1 Report: Physical-Windows Lifecycle Evidence

## Status

Task 1 review and re-review findings were addressed in follow-up commits. No
workflow or CI guard file was edited.

## TDD RED Evidence

Production board and hosted-contract RED:

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q
28 failed, 4 passed in 0.32s
```

The old implementation failed first on
`status must prove arbitrary_command_execution is false`, confirming that it
looked for the wrapper field inside the status result. The remaining failures
covered the strict hosted summary, exact row inventory/provenance, top-level
qualification bindings, unsafe status fields, future/stale evidence, malformed
imports, and production-shaped board consumption.

Lifecycle producer RED:

```text
$ python3 -m pytest \
  tests/test_windows_outbound_worker.py::test_physical_lifecycle_probe_reads_the_workspace_version_without_emitting_command_lines -q
1 failed
```

The test found fabricated `request_id`, `result_id`, and `completed_at` fields
in the probe before the package/install/rollback producer was rewritten.

Row diagnostic RED:

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q
1 failed, 31 passed
```

A timeout mutation reported `timeout_state` before `timed_out`; validation was
reordered to produce the specific operator diagnostic.

Install-verification RED:

```text
$ python3 -m pytest \
  tests/test_windows_outbound_worker.py::test_physical_lifecycle_probe_reads_the_workspace_version_without_emitting_command_lines -q
1 failed
```

The probe had not inspected the sidecar's nested offline-verification contract
or release owner. Those checks now gate `install_verification_verified=true`.

Compact-provenance RED:

```text
$ python3 -m pytest \
  tests/test_physical_windows_qualification.py::test_prepare_binds_real_wrapper_task_and_row_provenance \
  tests/test_physical_windows_qualification.py::test_validate_rejects_future_and_stale_completion_times \
  tests/test_physical_windows_qualification.py::test_validate_rejects_mutated_observed_worker_boundaries -q
3 failed
```

The compact evidence initially omitted row outcomes and observed status
boundaries and did not independently expire an old status observation.

Re-review validator RED:

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q
14 failed, 24 passed in 0.32s
```

The strict validator rejected the newly required Scheduled Task result/action
and direct probe-parent fields as unexpected. This also kept the new exact
summary identifier/digest and fractional-expiry cases from passing.

Re-review producer RED:

```text
$ python3 -m pytest \
  tests/test_windows_outbound_worker.py::test_physical_lifecycle_probe_reads_the_workspace_version_without_emitting_command_lines -q
1 failed
```

The probe did not query the current `$PID`, did not bind its direct parent to
the Python worker, and did not call `Get-ScheduledTaskInfo`.

## GREEN Evidence

```text
$ python3 -m pytest tests/test_physical_windows_qualification.py -q
43 passed in 0.09s

$ python3 -m pytest \
  tests/test_windows_outbound_worker.py::test_physical_lifecycle_probe_reads_the_workspace_version_without_emitting_command_lines -q
1 passed in 0.01s

$ python3 -m pytest tests/test_windows_outbound_worker.py tests/test_physical_windows_qualification.py -q
76 passed in 6.64s

$ python3 -m py_compile scripts/physical_windows_qualification.py
(exit 0)

$ git diff --check
(exit 0)
```

## Files Changed

- `scripts/Test-AO2PhysicalWindowsLifecycle.ps1`
- `scripts/physical_windows_qualification.py`
- `tests/test_physical_windows_qualification.py`
- `tests/test_windows_outbound_worker.py`

## Review Findings Addressed

- Fixtures are produced through `WindowsOutboundWorker.result_board` and retain
  its exact board/task/wrapper/result layout.
- Preparation validates both `ao2_cross_host` wrappers, real task IDs, wrapper
  request IDs and completion times, top-level result contract, exact AO2 row
  inventory, and each row's request/source/repository/time/outcome fields.
- Compact evidence binds actual status and qualification observations without
  retaining `bounded_sanitized_output`.
- Status evidence requires the exact worker commit, no inbound ports, null HTTP
  endpoint, no arbitrary command execution, no credential storage/change, and
  no release mutation. The public self-hosted-runner boundary is explicitly
  false.
- The summary schema is
  `ao2.physical-windows-qualification-summary.v1` and includes
  `mode=physical_unique`, `status=passed`, `expires_at`, string-valued passing
  checks, evidence SHA-256, source/version/request/result bindings, freshness,
  boundaries, portable-suite ownership, and equivalence exceptions.
- The PowerShell probe no longer fabricates IDs or timestamps. It verifies
  exact-head debug and release binaries with `ao2 version --json`, packages the
  release binary through `ao2 release package`, inspects manifest/provenance,
  runs extracted `install.ps1`, validates the install sidecar, and uses the
  installed candidate.
- Rollback seeds the source-bound debug binary as a distinct prior, invokes
  `install rollback` through the separate extracted release runner, and proves
  candidate/prior/pre/post digests and post-rollback version use before removing
  the installed binary, rollback, sidecar, and temporary tree.
- Negative tests cover wrong version/mode/schema/status/profile/repositories,
  missing provenance, future/stale times, malformed Base64/non-JSON/duplicate
  JSON, size limits, row omission and provenance mismatch, sibling timeout/
  truncation/failure, unsafe observed/probe boundaries, and noncompact evidence.
- Non-string import payloads now receive a type-specific diagnostic.
- The lifecycle probe queries `Win32_Process` for its own `$PID`, requires its
  unique direct parent to be a Python executable whose command line binds the
  exact normalized repository worker script, and records only process IDs and
  verification booleans.
- The named Scheduled Task must have one PowerShell action whose arguments bind
  the same exact worker script. The probe no longer enumerates Python workers,
  selects a first match, or assumes `taskeng.exe`/`taskhostw.exe` ancestry.
- `Get-ScheduledTaskInfo` binds `LastTaskResult`; `Running` requires `267009`
  (`0x41301`) and an approved `Ready` completion requires `0`.
- The strict summary now exposes `physical_evidence_sha256`,
  `status_request_id`, `status_result_id`, `qualification_request_id`, and
  `qualification_result_id`, with an exact-key regression test.
- UTC expiry formatting preserves production fractional seconds while adding
  exactly 86,400 seconds.
- Negative behavior tests reject false task-action, direct-parent,
  Python-executable, worker-script, and ancestry observations, as well as
  unacceptable Scheduled Task state/result combinations.

## Concerns

- This macOS environment has no PowerShell or Windows Task Scheduler/CIM, so
  the live producer and PowerShell parser cannot be executed here. Static
  producer-contract tests reject the prior enumeration/host-name patterns and
  require the new direct-parent/action/result checks; the later
  physical-Windows workflow must capture live evidence.
