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
`ao2.github-issue-repair-pack.v1` or `ao2.github-issue-repair-pack.v2`.
Duplicate keys at any depth, trailing JSON, unknown fields, null required
fields, malformed JSON, and invalid UTF-8 are rejected. Version 1 remains the
three-artifact structural validation contract. Version 2 adds proof that the
selected issue reproduced as a failing command on the exact source.

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
- `language`: exactly `go` or `rust`
- `fetched_at`: RFC3339 timestamp no more than 7 days old and no more than 5
  minutes in the future at validation time
- `source_archive`, `issue_snapshot`, and `dependency_cache_manifest`: artifact
  objects
- `reproduction_evidence`, `reproduction_fixture`, and
  `reproduction_output`: three additional artifact objects required only by
  version 2
- `toolchain`: strict object with nonempty bounded `name` and `version`
- `extracted_tree_sha256`: lowercase `sha256:<64 hex>`
- `known_fix_fetched`: exactly `false`
- `safety`: the exact passing boundary below

Each artifact object contains only `path`, `size_bytes`, and `sha256`. A path is
exactly one normal UTF-8 direct-child filename beneath the repair pack root;
nested paths are not supported. Sizes bind the exact nonnegative file length.
Digests use lowercase `sha256:<64 hex>` syntax.

Version 1 rejects every reproduction artifact, preserving its original strict
shape. Version 2 rejects any missing or null reproduction artifact.

## Version 2 Reproduction Evidence

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
  selectors are rejected.
- `working_directory`: exactly `.`, the extracted source root
- `fixture_install_path`: the exact source-root Go test filename or Rust
  `tests/<test_identifier>.rs` path where the bound fixture is installed
- `test_identifier`: the focused test selected by `command_argv`. Go names
  begin with `Test` and otherwise use only ASCII alphanumerics or underscore;
  Rust targets use only ASCII alphanumerics, hyphen, or underscore.
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
`reproduction_evidence_sha256`. Text output for version 2 reports the same
eligibility status and digest.

Both readbacks preserve the request, corpus, candidate, repository, issue,
source, license, language, and timestamp identities; bind the manifest,
archive, snapshot, dependency-cache manifest, and extracted-tree digests;
report `failed_rows=0`; repeat the exact L1 safety boundary; and report every
network, Git, GitHub, repair, mutation, execution, and approval flag as
`false`.
