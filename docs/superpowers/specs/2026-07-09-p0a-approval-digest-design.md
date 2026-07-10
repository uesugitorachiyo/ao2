# P0-A Content-Bound Approval Digest Design

**Status:** Approved for implementation planning only

**Mission:** `mission-4d91b0a9e4ab273e`

**Atlas node:** `ao2-approval-digest-p0-candidate-node`

**Blueprint authorization SHA256:** `b7e18a1967b31b2806184444ab9aeab5e984e050f66261431ec57ece4cc833ee`

## Purpose

AO2 currently computes a sandbox patch approval digest from changed path labels
and a human-readable diff summary. The digest does not bind the target
repository, target base commit, exact before and after file states, operation
type, symlink target, executable mode, or operation order. An approval can
therefore remain valid after material parts of the proposed mutation change.

P0-A replaces that digest input with one canonical, machine-readable approval
subject. Preview emits the subject and its digest. Apply reconstructs the same
subject from current target and sandbox state and rejects any mismatch before
the first target write.

This design does not grant approval or execute a provider. It does not remove
the current automatic approval path; that is P0-B. It does not make apply
transactional; that is P0-C. It does not change Covenant ticket integrity or
consumption; those are P0-D.

## Existing Failure

`sandbox_patch_digest` currently hashes this payload:

```json
{
  "changed_files": ["src/value.txt"],
  "diff_summary": "modified: src/value.txt"
}
```

The snapshot stores only regular-file content hashes. Symlinks are omitted,
file modes are omitted, repository state is omitted, and the digest payload is
derived from presentation text rather than a typed contract.

## Considered Approaches

### 1. Typed canonical subject in `ao2-adapters` (selected)

Create a focused `sandbox_patch` module that owns repository identity,
canonical paths, file-state snapshots, ordered operations, digest generation,
and pre-write revalidation. Runtime and CLI continue to consume the adapter
API.

This keeps mutation identity beside preview and apply, avoids duplicate
canonicalization, and gives policy tickets the stronger digest without moving
execution behavior into policy.

### 2. Hash canonical `git diff --binary` output

This binds content and mode for Git-tracked paths, but it does not provide a
stable typed operation contract, complicates untracked additions and symlinks,
and makes behavior depend on Git output formatting. It is useful as an
independent test oracle, not as the approval contract.

### 3. Move patch digest construction into `ao2-policy`

Policy should validate an action identifier, not inspect worktrees or define
filesystem mutation semantics. Moving snapshots into policy would couple the
policy kernel to Git and filesystem behavior and duplicate adapter logic.

## Contract

The canonical approval subject is serialized as a Rust struct in declaration
order with `serde_json::to_vec`. It has this wire shape:

```json
{
  "schema_version": "ao2.sandbox-patch-approval-subject.v1",
  "repository_identity": "sha256:<64 lowercase hex>",
  "base_commit": "<40 or 64 lowercase hex object id>",
  "operation_type": "sandbox_patch_apply",
  "operations": [
    {
      "order": 0,
      "path": "src/value.txt",
      "kind": "modified",
      "before": {
        "kind": "regular_file",
        "content_sha256": "sha256:<64 lowercase hex>",
        "symlink_target_sha256": null,
        "unix_mode": 420
      },
      "after": {
        "kind": "regular_file",
        "content_sha256": "sha256:<64 lowercase hex>",
        "symlink_target_sha256": null,
        "unix_mode": 420
      }
    }
  ]
}
```

`repository_identity` is the SHA256 of the normalized UTF-8 form of the
canonical Git common-directory path. Preview rejects a common-directory path
that is not valid UTF-8 instead of hashing a lossy conversion. The path itself
is never emitted. Linked worktrees of one repository therefore share an
identity, while a different repository with the same bytes does not.

`base_commit` is the exact `HEAD` object ID from the target repository at
preview time. Preview fails if the target is not a Git worktree or `HEAD`
cannot be resolved.

`path` uses `/` separators and accepts only relative `Normal` path components.
Empty paths, absolute paths, `.`, `..`, root prefixes, and platform prefixes are
rejected before digest construction or apply.

`kind` is one of `added`, `modified`, or `deleted`. `before` is absent only for
an addition; `after` is absent only for a deletion.

`FileState.kind` is `regular_file` or `symlink`. Regular files bind their byte
digest. Symlinks bind the byte digest of the link target without following the
link. On Unix, `unix_mode` binds permission bits masked to `0o777`; on other
platforms it is null. Directory symlinks are recorded as symlinks and never
traversed. A platform that cannot safely inspect or recreate a symlink rejects
the operation before digest approval instead of treating it as a regular file.

Operations are sorted by canonical path and assigned contiguous order values
starting at zero. The order field remains part of the digest. Unit tests also
construct subjects directly to prove that reordering operations changes the
digest.

## Data Flow

1. Preview validates that target and sandbox are directories.
2. Preview resolves target repository identity and `HEAD`.
3. Snapshot walks target and sandbox without following symlinks.
4. Snapshot normalizes each path and captures typed file state.
5. Diff constructs sorted, ordered operations.
6. Preview serializes the subject and hashes the exact serialized bytes.
7. Runtime places that digest in the existing `ToolRequest` and approval
   ticket.
   If an approved sandbox ticket exists for the same run role but its digest no
   longer matches the current subject, runtime rejects the run instead of
   creating a replacement pending ticket.
8. Apply reruns steps 1 through 6 from current state.
9. Apply compares the recomputed digest with `expected_digest` before creating,
   copying, chmodding, or deleting any target path.
10. Apply uses the already validated operation list and reports the exact
    subject and digest in its result.

The initial sandbox copy preserves symlinks as links without following them.
Otherwise an unchanged target symlink would appear as a deletion in every
sandbox preview.

## Failure Behavior

Preview and apply fail closed when:

- target or sandbox is missing or not a directory;
- target is not a Git worktree;
- repository identity or `HEAD` cannot be resolved;
- a path is absolute, empty, aliased, or contains traversal;
- an entry is neither a regular file nor a symlink;
- the target, sandbox, base commit, mode, link target, operation kind, operation
  order, or content differs from the approved subject; or
- canonical serialization fails.

Every mismatch is detected before target writes. P0-A does not promise rollback
after writes begin; P0-C adds that guarantee.

P0-A does not close the narrow filesystem race between final revalidation and
each file operation. P0-C's isolated worktree and transaction journal must
close that remaining apply-time race.

## Compatibility

`SandboxPatchPreview` keeps `changed_files`, `diff_summary`, and
`action_digest` for current consumers and adds `approval_subject`.

`SandboxPatchApplyRequest` keeps `expected_digest` and `approver`. P0-B will
replace the caller-supplied approver flow with an externally validated exact
approval. P0-A does not imply that the current approver field is trustworthy.

`SandboxPatchApplyResult` adds `approval_subject` so evidence records show what
the digest covered. Existing runtime tickets continue to bind
`preview.action_digest`.

Current non-Git adapter sandbox tests must initialize and commit their fixture
target before preview or apply. Provider-only sandbox execution that does not
preview a patch remains compatible with plain directories.

## Test Matrix

The P0-A regression corpus must prove:

- same path with different after bytes changes the digest;
- same patch bytes at a different target `HEAD` changes the digest;
- identical bytes in a different repository changes the digest;
- added, modified, and deleted operations have distinct subjects;
- path traversal and normalized aliases are rejected;
- symlink target changes change the digest without following the link;
- executable-mode changes change the digest on Unix;
- reordered operations change the digest;
- target content changed after preview is rejected before write;
- sandbox content changed after preview is rejected before write;
- target `HEAD` changed after preview is rejected before write;
- wrong expected digest is rejected before write; and
- an unchanged approved subject still applies successfully.

## Verification

Implementation verification is:

```sh
cargo fmt --all --check
cargo test -p ao2-adapters --test sandbox_adapter --no-fail-fast
cargo test -p ao2-runtime --test provider_backed_run --no-fail-fast
cargo test -p ao2-runtime --test approval_replay --no-fail-fast
cargo test -p ao2-cli --test cli_approval_replay cli_adapter_patch_preview_and_apply_promotes_exact_digest -- --exact
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
git diff --check
```

No test may invoke a live provider, inspect credentials, publish evidence,
release, deploy, tag, or mutate a repository outside its temporary fixture.

## Authorization Boundary

The Blueprint authorization approves planning. Atlas has selected P0-A but
still records `safe_to_execute=false` and `authority_boundary=atlas_compile_only`.
This design and its implementation plan may be committed for review. No
production-code implementation begins until AO Mission records a separate
Foundry/Covenant/Sentinel execution clearance for this exact node and scope.
