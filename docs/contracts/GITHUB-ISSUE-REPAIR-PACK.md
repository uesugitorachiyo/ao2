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

## Strict Manifest

The manifest is strict JSON with schema version
`ao2.github-issue-repair-pack.v1`. Duplicate keys at any depth, trailing JSON,
unknown fields, null required fields, malformed JSON, and invalid UTF-8 are
rejected.

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
- `toolchain`: strict object with nonempty bounded `name` and `version`
- `extracted_tree_sha256`: lowercase `sha256:<64 hex>`
- `known_fix_fetched`: exactly `false`
- `safety`: the exact passing boundary below

Each artifact object contains only `path`, `size_bytes`, and `sha256`. A path is
exactly one normal UTF-8 direct-child filename beneath the repair pack root;
nested paths are not supported. Sizes bind the exact nonnegative file length.
Digests use lowercase `sha256:<64 hex>` syntax.

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

JSON output uses schema version
`ao2.github-issue-repair-pack-validation.v1` and status `passed`. It preserves
the request, corpus, candidate, repository, issue, source, license, language,
and timestamp identities; binds the manifest, archive, snapshot, dependency
cache manifest, and extracted tree digests; reports `failed_rows=0`; repeats
the exact L1 safety boundary; and reports every network, Git, GitHub, repair,
mutation, execution, and approval flag as `false`.
