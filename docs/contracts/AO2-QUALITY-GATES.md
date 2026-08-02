# AO2 Quality Gate Execution Contract

## Status

Version 1 is the strict local execution contract for source-owned AO Stack
quality manifests. AO Architecture owns `ao.quality-gates.v1`; each maintained
repository owns its root `ao-quality-gates.json`; AO2 consumes the declaration
without inventing repository commands.

## Commands

```sh
ao2 quality check commit --target /path/to/repository --json
ao2 quality check push --target /path/to/repository --base <base-commit> --json
ao2 quality check full --target /path/to/repository --json
ao2 quality hooks status --target /path/to/repository --json
ao2 quality hooks install --target /path/to/repository --json
```

`--manifest` may name the root manifest explicitly, but it must resolve to the
regular, non-symlinked `ao-quality-gates.json` directly under `--target`.
`--out` writes the same bounded JSON result emitted by `--json`. Relative
result paths resolve below `--target` and must remain under the manifest's
literal `local_artifact_root`; an absolute external evidence path is also
accepted. Traversal and symlinked result paths fail before step execution.

## Optional Git Hooks

Hook installation is an explicit local opt-in. `status` is read-only and
classifies both `pre-commit` and `pre-push` as absent, current, stale,
unmanaged, or unsafe. `install` writes only missing wrappers or wrappers with a
recognized older AO2 marker. It refuses a custom `core.hooksPath`, unmanaged
content, symlinks, non-regular files, and oversized hook files before writing
either wrapper. Repeating installation against current wrappers changes
nothing.

The managed wrappers contain only a version marker and an `exec` of the hidden
AO2 `quality hook-run` entry point. Gate selection, manifest validation,
snapshot construction, and safety policy remain in AO2. A normal pre-push
binds one exact remote base and local `HEAD`; a new branch conservatively runs
the full exact-head level. Ambiguous multi-head or multi-base push input fails
closed. Deletion-only pushes need no source verification.

Hooks are optional accelerators, never merge authority. The hook path performs
no provider call or network operation, and it cannot repair, modify source,
commit, push, release, deploy, publish, or promote. Required hosted checks
remain authoritative even when local hooks are absent or bypassed.

## Snapshot Binding

The commit level reads only the Git index. Its snapshot digest binds the entire
`git ls-files --stage -z` result and the cached changed paths relative to
`HEAD`; unstaged worktree bytes cannot affect selection or identity. Unmerged
index entries fail closed.

The push level resolves an explicit `--base` or the current upstream, requires
that base to be an ancestor of `HEAD`, and binds the exact base, head, ordered
outgoing commit IDs, and base-to-head changed paths. Uncommitted worktree bytes
cannot affect that snapshot. The full level binds the exact `HEAD` commit and
tree.

All changed paths must be bounded UTF-8 repository-relative paths. Snapshot
evidence uses SHA-256 over a canonical typed payload; Git object IDs remain
separate identity fields.

## Validation And Selection

AO2 rejects manifests that are missing, oversized, malformed, contain duplicate
keys or trailing JSON, use unknown fields or versions, mismatch the repository
origin identity, follow a symlink, declare unsafe paths, exceed level or step
budgets, request evaluated shell text, invoke a provider, or declare a known
network operation in a network-disabled level.

`maximum_result_bytes` must be between 4,096 and 1,048,576 bytes so every
execution can return bounded identity and failure evidence.

Commit and push steps are selected only when a changed path matches a declared
glob. The result records every matched trigger and path plus a digest of the
argv vector. Full checks select all full-level steps. No selected step is a
successful `not_applicable` result, not fabricated execution evidence.

## Execution Boundary

Steps run as direct program and argument vectors with no shell construction.
AO2 scrubs known provider and credential environment names, disables prompts,
sets common package tools to offline mode for network-disabled levels, bounds
each process by the smaller step and remaining level deadline, and drains output
without persisting its content. Results contain byte counts, truncation flags,
and SHA-256 digests only after AO2 secret redaction; raw output and raw-output
digests are never written. Output beyond the one-MiB capture bound fails closed.

AO2 compares the tracked, staged, and untracked Git state before and after the
selected steps. A changed state fails with `SOURCE_MUTATION_DETECTED`. This is a
detect-and-deny control, not an operating-system sandbox; source owners must
still declare non-mutating fast commands, and hosted CI remains authoritative.

The result always reports `provider_calls=0`. This command does not approve,
commit, push, repair, release, deploy, publish, promote, or confer authority.

## Result

`ao2.quality-check-result.v1` binds repository, level, manifest digest, source
head, snapshot, selection reasons, step exits and deadlines, mutation status,
provider-call count, and failure codes. The manifest's evidence byte limit
applies before output is written.
