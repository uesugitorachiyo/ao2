# GitHub Issue Repair Pack Contract

`ao2 issue repair-pack validate` is a local, read-only validator for sanitized
historical GitHub issue repair packs. It validates evidence bindings only. It
does not unpack archives, execute repairs, access the network, invoke Git or
GitHub, mutate repositories, approve work, or grant authority.

## Command

```text
ao2 issue repair-pack validate --manifest <manifest.json> --root <pack-root> [--json]
```

Validation errors exit nonzero before a passing readback is emitted.

## Strict Manifests

The manifest is strict JSON with schema version
`ao2.github-issue-repair-pack.v1`, `ao2.github-issue-repair-pack.v2`, or
`ao2.github-issue-repair-pack.v3`.
Duplicate keys at any depth, trailing JSON, unknown fields, null required
fields, malformed JSON, and invalid UTF-8 are rejected. Version 1 remains the
three-artifact structural validation contract. Version 2 adds Go and Rust proof
that the selected issue reproduced as a failing command on the exact source.
Version 3 preserves that contract and adds a bounded direct Python pytest
target.

Required fields are:

- `schema_version`
- `request_id`, `corpus_id`, and `candidate_id`: nonempty identifiers of at
  most 128 bytes using ASCII letters, digits, `.`, `_`, or `-`
- `repository`: canonical GitHub `owner/name`. The owner is 1 to 39 ASCII
  characters, starts and ends with an alphanumeric character, contains only
  alphanumeric characters or single internal hyphens, and has no consecutive
  hyphens. The name is 1 to 100 ASCII characters, is neither `.` nor `..`,
  does not end with `.`, contains only alphanumeric characters, `.`, `_`, or
  `-`, and has no case-insensitive `.git` suffix.
- `issue_number`: positive integer
- `source_sha`: exactly 40 lowercase hexadecimal characters
- `license`: exactly `MIT`, `Apache-2.0`, `BSD-2-Clause`, or `BSD-3-Clause`
- `language`: exactly `go` or `rust` in versions 1 and 2; version 3 also accepts
  `python`
- `fetched_at`: RFC3339 timestamp no more than 7 days old and no more than 5
  minutes in the future at validation time
- `source_archive`, `issue_snapshot`, and `dependency_cache_manifest`: artifact
  objects
- `reproduction_evidence`, `reproduction_fixture`, and
  `reproduction_output`: three additional artifact objects required only by
  versions 2 and 3
- `toolchain`: strict object with nonempty bounded `name` and `version`
  (`name` is exactly `python` for a version 3 Python pack)
- `extracted_tree_sha256`: lowercase `sha256:<64 hex>`
- `known_fix_fetched`: exactly `false`
- `safety`: the exact passing boundary below

Each artifact object contains only `path`, `size_bytes`, and `sha256`. A path is
exactly one normal UTF-8 direct-child filename beneath the repair pack root;
nested paths are not supported. Sizes bind the exact nonnegative file length.
Digests use lowercase `sha256:<64 hex>` syntax.

Version 1 rejects every reproduction artifact, preserving its original strict
shape. Versions 2 and 3 reject any missing or null reproduction artifact.

For version 3, `issue_snapshot` is JSON containing unique required `number` and
`url` fields. Other sanitized issue fields are allowed. `number` must equal
`issue_number`, and `url` must equal
`https://github.com/<repository>/issues/<issue_number>`. Versions 1 and 2 retain
their historical byte-and-digest binding without this semantic requirement.

## Version 2 And 3 Reproduction Evidence

The digest-bound reproduction artifact is strict JSON with schema version
`ao2.github-issue-reproduction-evidence.v1`. It contains only these fields:

- `schema_version`: exactly `ao2.github-issue-reproduction-evidence.v1`
- `request_id`, `candidate_id`, and `source_sha`: exact matches for the repair
  pack manifest
- `command_argv`: an array of 1 to 64 nonempty UTF-8 arguments. Each argument
  is at most 256 bytes, the combined argument bytes are at most 4,096, and
  ASCII control characters are rejected. Go evidence must begin with
  `["go", "test"]`; Rust evidence must begin with `["cargo", "test"]`.
  Paths, wrappers, shells, other executables, and non-test subcommands are not
  accepted. Go requires the source-root package and an exact
  `-run ^<test_identifier>$` selector. Rust requires exactly one
  `--test <test_identifier>` target; broad library, binary, or test-suite
  selectors are rejected. Python is accepted only by version 3 and requires
  exactly `python -m pytest <fixture_install_path>::<test_identifier>`.
  Alternate executables, shell wrappers, `python -c`, broad pytest paths,
  options, plugins, and additional selectors are rejected.
- `working_directory`: exactly `.`, the extracted source root
- `fixture_install_path`: the exact source-root Go test filename, Rust
  `tests/<test_identifier>.rs` path, or version 3 Python test path where the
  bound fixture is installed. Python paths are relative normal components
  beneath the source root, have a filename beginning `test_`, end in `.py`, and
  reject absolute paths, parent traversal, empty components, backslashes, and
  platform prefixes.
- `test_identifier`: the focused test selected by `command_argv`. Go names
  begin with `Test` and otherwise use only ASCII alphanumerics or underscore;
  Rust targets use only ASCII alphanumerics, hyphen, or underscore. Python
  identifiers begin with `test_` and use only ASCII alphanumerics or underscore.
- `toolchain`: exact name and version match for the repair-pack manifest
- `fixture_sha256`: digest of the issue-derived regression fixture
- `output_sha256`: digest of the complete captured failing command output
- `failure_signature`: 8 to 1,024 bytes of printable issue-specific text,
  including at least one alphanumeric byte, that must occur verbatim in the
  captured output
- `failure_signature_sha256`: digest of the normalized issue-specific failure
  signature used to classify the observed failure
- `result`: exactly `reproduced_failure`
- `expected_exit_code` and `observed_exit_code`: equal nonzero process exit
  codes from 1 through 255. A reproduced failure normally uses exit code 1.
- `network`: exactly `none`
- `git_history_present`, `oracle_present`, and `credentials_present`: exactly
  `false`
- `external_effects`: exactly `0`
- `completed_at`: RFC3339, no more than 7 days old, no more than 5 minutes in
  the future at validation time, and no later than the manifest `fetched_at`

The validator binds and validates the fixture, output, and evidence artifacts,
checks that the declared failure signature occurs in the captured output, then
reopens and compares all three artifacts before emitting a readback.
Passing evidence describes a prior bounded reproduction; validation does not
execute `command_argv`.

The required safety object is exactly:

```json
{
  "authority_level": "L1",
  "network": "none",
  "git_history_present": false,
  "oracle_present": false,
  "credentials_present": false,
  "campaign_root_mounted": false,
  "repair_pack_read_only": true,
  "scratch_read_write": true,
  "third_party_mutation_authorized": false
}
```

## Bounds And Filesystem Safety

- Manifest maximum: 65,536 bytes.
- Issue snapshot maximum: 262,144 bytes.
- Dependency-cache manifest maximum: 262,144 bytes.
- Reproduction evidence maximum: 65,536 bytes.
- Reproduction fixture maximum: 262,144 bytes.
- Reproduction output maximum: 1,048,576 bytes.
- Source archive maximum: 1,073,741,824 bytes.
- Total referenced artifacts maximum: 2,147,483,648 bytes.
- The manifest and every referenced artifact must be direct children of the
  repair pack root. Nested paths are rejected.
- The root, manifest, and artifacts must not be symlinks.
- Artifacts must be regular, single-link files inside the canonical root.
- Absolute paths, parent traversal, empty components, platform prefixes,
  directories, devices, sockets, hardlinks, artifact aliases, and aliases
  between the manifest and any artifact are rejected.
- One root directory handle remains open for the entire validation. On Unix its
  device/inode identity is bound to the expected canonical root and every
  direct child is opened relative to that handle with audited `openat`,
  `O_NOFOLLOW`, and close-on-exec flags. On Windows the first retained root
  handle uses backup-directory and open-reparse-point flags with a restrictive
  share mode that denies root deletion or rename; canonical and metadata checks
  occur while that handle is retained, and children use non-reparse opens.
- File identity, link count, and size are checked before and after reads. Unix
  uses device/inode identity; Windows uses volume serial, full file index,
  `nNumberOfLinks`, and non-reparse disk handles.
- Exact declared size and SHA-256 are verified.
- Artifact sizes and SHA-256 values are streamed in fixed 65,536-byte chunks;
  source archives are never allocated at archive size. Only the separately
  bounded manifest JSON is retained in memory for strict parsing.

The validator does not unpack `source_archive`. It preserves
`extracted_tree_sha256` as a binding for the separately governed pack builder
and extraction verifier.

## Passing Readback

Version 1 JSON output remains
`ao2.github-issue-repair-pack-validation.v1` with status `passed` and its
original fields. Version 2 JSON output uses
`ao2.github-issue-repair-pack-validation.v2`, status `passed`, and
`eligibility_status=reproduced`; it also binds the exact artifact digest as
`reproduction_evidence_sha256`. Version 3 uses the corresponding
`ao2.github-issue-repair-pack-validation.v3` readback. Text output for versions
2 and 3 reports the same eligibility status and digest.

Both readbacks preserve the request, corpus, candidate, repository, issue,
source, license, language, and timestamp identities; bind the manifest,
archive, snapshot, dependency-cache manifest, and extracted-tree digests;
report `failed_rows=0`; repeat the exact L1 safety boundary; and report every
network, Git, GitHub, repair, mutation, execution, and approval flag as
`false`.

## Repair Result Failure Classification

The separate read-only command

```text
ao2 issue repair-result classify --baseline <baseline.json> --candidate <candidate.json> --json
```

compares two strict `ao2.github-issue-repair-verification.v1` summaries. Each
input is a regular non-symlink file of at most 65,536 bytes and binds its role,
repository, issue number, baseline source SHA, exact source SHA, command
SHA-256, toolchain name and version, completion timestamp, exit code, output
SHA-256, failures, and offline effect-free safety state. Candidate evidence
also binds a distinct candidate commit SHA. Evidence older than seven days or
more than five minutes in the future is rejected.

Each failure binds a printable identifier of at most 256 bytes and a lowercase
`sha256:<64 hex>` signature digest. Identifiers must be unique within an input.
The baseline and candidate must have identical repository, issue, baseline
source, command, and toolchain identities. A zero exit code requires no
failures; a nonzero exit requires at least one bound failure.

The `ao2.github-issue-repair-result-classification.v1` readback sorts and
classifies failures as:

- `shared_failures`: identifier and signature digest are identical;
- `resolved_failures`: present only on the baseline;
- `changed_failures`: identifier is shared but the signature digest differs;
- `candidate_only_failures`: present only on the candidate.

Changed or candidate-only failures set `candidate_regression=true`. Exact
shared failures set `baseline_failures_retained=true` without classifying them
as candidate regressions. The readback binds both input-file digests and emits
all network, Git, GitHub, provider, repair, mutation, approval, release,
deployment, and publication flags as zero or false.

`candidate_regression=false` means only that the supplied candidate evidence
introduced no changed or candidate-only failure relative to the supplied
baseline evidence. It is not a repair-passed, qualification, approval, or
merge verdict. The command does not run tests or perform any side effect.

## Repair Qualification

The separate offline command

```text
ao2 issue repair-qualification verify --bundle <bundle.json> --json
```

validates one `ao2.github-issue-repair-qualification-bundle.v1` JSON file. The
bundle must be a regular non-symlink file of at most 65,536 bytes. It binds the
exact repository and immutable upstream repository ID, authorized operator
owner, issue, baseline and candidate SHAs, source and dependency
digests, toolchain and platforms, a failing reproduction, focused baseline RED
and candidate GREEN results, full-suite classification, candidate seal,
independent review, operator-fork draft state, and a zero-effect safety record.
All timestamps must be no more than seven days old or five minutes in the
future and preserve source, reproduction, regression, full-suite, candidate
seal, review, and draft-capture lifecycle order.

The bundle's `artifact_sha256` map must include these direct sibling files:

```text
source.json
reproduction.json
regression.json
full-suite.json
candidate-seal.json
review.json
draft-pr.json
```

The map contains exactly those seven roles. Each file is strict JSON containing
the same repository ID, issue, baseline and candidate SHAs plus the role's
exact bundle evidence object. AO2 retains and revalidates the parent directory,
opens direct children through that root, rejects symlinks, hardlinks, aliases,
and root replacement, then verifies each SHA-256 and semantic object. Missing,
oversized, digest-altered, or semantically divergent evidence fails before a
passing result.

The v2 bundle adds one explicit `process_lifecycle` qualification profile for
repairs that own a child process or transport lifecycle. It preserves every v1
field and adds:

```text
schema_version=ao2.github-issue-repair-qualification-bundle.v2
qualification_profile=process_lifecycle
process_lifecycle.completed_at
process_lifecycle.evidence_sha256
process_lifecycle.process_death_observed=true
process_lifecycle.list_tools_failure_typed=true
process_lifecycle.tool_call_failure_typed=true
process_lifecycle.lifecycle_wakeup_observed=true
process_lifecycle.disconnected_state_truthful=true
process_lifecycle.explicit_close_passed=true
process_lifecycle.repeated_close_passed=true
process_lifecycle.initialization_failure_passed=true
process_lifecycle.reinitialization_passed=true
process_lifecycle.orphan_processes=0
process_lifecycle.timeout_seconds=1..300
```

V2 requires an eighth direct sibling, `process-lifecycle.json`, bound with the
same repository, issue, source, candidate, digest, and strict-JSON rules as the
other evidence files. Its completion timestamp falls after focused regression
and before full-suite completion. Missing, failed, stale, reordered, altered,
or linked lifecycle evidence rejects the bundle.

V1 remains the generic repair qualification contract and rejects v2-only
fields. A producer that classifies a repair as process-lifecycle-sensitive must
use v2; passing v1 does not make a process-lifecycle claim. A v2 success emits
`ao2.github-issue-repair-qualification.v2` and records the profile, lifecycle
evidence digest, zero orphan count, bounded timeout, and
`process_lifecycle_passed=true`.

Qualification requires a nonzero reproduction exit, a nonzero focused
baseline exit, a zero focused candidate exit, no changed or candidate-only
full-suite failure, no unresolved P1 or P2 review finding, and an open,
unmerged, exact-head draft whose immutable repository ID, explicit fork flag,
parent repository and ID, and owner match the authorized operator and upstream.
Network, credentials, Git history, repair oracles, provider calls, external
effects, upstream mutations, release mutations, deployments, and publications
must all be absent.

Success emits `ao2.github-issue-repair-qualification.v1` with
`result=repair_qualified`, the bundle SHA-256, an aggregate qualification
digest, all evidence bindings, and every execution, mutation, approval,
release, deployment, and publication flag false. Rejection exits nonzero and,
with `--json`, emits `result=repair_rejected` and the stable reason
`invalid_bundle` before the diagnostic is written to stderr.

`repair_qualified` means only that the supplied local evidence proves the
bounded repair under this contract. It is not maintainer acceptance, merge
approval, promotion authority, or release authorization. The command does not
run tests, invoke Git or GitHub, access the network, execute repairs, mutate a
repository, approve work, or publish anything.
