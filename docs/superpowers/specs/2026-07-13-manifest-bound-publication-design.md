# Manifest-Bound AO2 Publication Design

## Purpose

Bind every live AO2 publication to the exact 23-asset manifest approved by the
operator. Keep unapproved candidate-generation dry runs available, but label
them explicitly as unbound and non-authorized.

## Interface

`scripts/release-ship.sh` accepts two new environment variables:

- `AO2_RELEASE_EXPECTED_ASSET_MANIFEST`
- `AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256`

Live publication requires both before build or external access. A dry run with
neither variable remains an unapproved candidate-generation rehearsal and
prints:

```text
release_approval_bound=false
release_approved_asset_manifest_sha256=not_supplied
```

A dry run with exactly one variable fails immediately. A dry run with both
variables verifies the approved manifest and prints:

```text
release_approval_bound=true
release_approved_asset_manifest_sha256=<verified lowercase SHA-256>
```

Only the fully bound dry run is an approval-bound publication rehearsal.

## Verifier

Add `scripts/release-verify-approved-assets.py`. It uses only Python's standard
library and receives the expected manifest, its expected digest, the staged
publication directory, and the staged publication list through explicit
arguments.

The verifier opens the manifest once with `os.open`, refuses symlinks with
`O_NOFOLLOW` where available, verifies the descriptor is a regular file, and
reads the bytes from that descriptor. It hashes and parses the same byte buffer
to avoid verifying one file version and parsing another.

Each non-empty manifest line must match exactly:

```text
<64 lowercase hexadecimal characters><two spaces><basename>
```

Names must be basenames. The verifier rejects absolute paths, `/` or `\`
separators, `.` and `..`, traversal, duplicate names, malformed hashes, and
symlinked staged assets. It reads the staged publication list with the same
basename restrictions, requires both name sets to be identical, then hashes
each regular staged asset and compares it with the approved value. It stops at
the first failure with a concise diagnostic that contains no asset bytes or
secret material.

Successful output is machine-readable:

```text
release_approved_asset_manifest_sha256=<digest>
release_approved_asset_count=23
release_approved_assets=passed
```

## Publisher order

`release-ship.sh` validates approval-variable presence immediately after its
existing confirmation guard and before moving-head checks, retention, builds,
GitHub reads, or any other external action. This makes missing live approval
fail before build.

After build, native smoke, release gate, staging, and the existing publication
contract, the publisher invokes the strict verifier when both approval
variables are present. Verification completes before local or remote tag
checks, `git tag`, `git push`, `gh release create`, or upload.

The publisher prints the binding state and verified digest in both dry-run and
successful live output. Existing prerelease, `latest=false`, explicit-notes,
provider-disabled, signing, dirty-head, moving-head, existing-tag, and
no-overwrite guards remain unchanged.

## Tests

Focused tests create isolated 23-asset fixtures and exercise the verifier as a
process. They cover exact success, one-byte drift, missing and extra assets,
manifest-digest drift, duplicates, absolute paths, separators, traversal,
symlinked manifests, and symlinked staged assets.

Publisher tests cover the four variable modes and verify source ordering. A
mutation-sentinel harness places tag, push, release, and upload markers after
verification and proves every invalid fixture exits before any marker is
written. Existing publication-contract assertions continue to cover
prerelease, `latest=false`, explicit notes, disabled providers, and immutable
assets.

Run Python compilation, shell syntax checks, the focused release-packaging test
target, and normal required CI before merge.

## Requalification and authorization packet

After merge, rebuild AO2 from the exact new `main` commit. Regenerate four
archives, four standalone SBOMs, sidecars, signatures, `SHA256SUMS`, provenance,
public key, closure/readiness summaries, the 23-entry publication list, and the
authorization packet. Control Plane remains at
`f1702b387607566cac457458af9adb5871a5c412` unless its repository or staged
assets changed.

The new packet records both component manifests and their own SHA-256 digests,
the exact publication variables and commands, release notes and flags, and an
intentional one-byte-drift failure before mutation. `PUBLIC_RELEASE_AUTHORIZED`
remains `NO`.

## Cleanup

Delete only the merged PR #268 branch, prune only worktree records whose paths
are missing in AO2 and AO Mission, and remove only AO Mission's generated
`target/ao2-beta-publication-recovery` directory. Preserve every unmerged branch
commit and unrelated untracked file.
