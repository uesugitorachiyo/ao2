# Public Release Verification

This is the operator index for verifying public release evidence across
`uesugitorachiyo/ao2` and `uesugitorachiyo/ao2-control-plane`.

The current public release pair is:

- AO2 stable release: `v0.4.80`
- AO2 control-plane stable release: `v0.1.13`

All checks below are read-only. They download release assets or GitHub Actions
artifacts, verify checksums and summaries, and do not approve AO2 runs, mutate
AO artifacts, mutate GitHub releases, or include credential material.

## Hosted Release Workflows

AO2 uses `Post Stable Release Verification` in
`.github/workflows/post-stable-release-verification.yml`. It can be dispatched
manually and runs on schedule. It downloads AO2 `v0.4.80` release archives,
`SHA256SUMS`, signed provenance sidecars, and the signing public key, then runs
install/update, `version --json`, `doctor --json`, and
`adapter doctor --provider scripted` on Ubuntu, macOS, and Windows.

Expected AO2 evidence artifact:

- `post-stable-release-smoke-${{ runner.os }}`
- `ao2-dual-public-release-smoke`

The `ao2-dual-public-release-smoke` artifact is the cross-repository public
archive interoperability proof. It downloads the published AO2 Linux x86_64
archive and the published control-plane Linux x86_64 archive, verifies each
against its public `SHA256SUMS`, starts the published control-plane server, and
uses the published AO2 binary identity plus an `ao2.ai-task-board.v1` fixture to
exercise task-board ingest/readback. Its `summary.json` uses
`ao2.dual-public-release-smoke.v1` and records that the smoke is read-only,
including `mutates_github_releases=false`, no stored bearer value, and no
release approval authority.

The control-plane uses `Post Release Verification` in
`.github/workflows/post-release-verification.yml`. It can be dispatched
manually and runs on schedule. It downloads all public `v0.1.13` release
assets, verifies `SHA256SUMS`, and writes a release publication closure summary
on Ubuntu, macOS, and Windows.

Expected control-plane evidence artifacts:

- `ao2-control-plane-post-release-verification-ubuntu`
- `ao2-control-plane-post-release-verification-macos`
- `ao2-control-plane-post-release-verification-windows`

## CI Closure Artifacts

AO2 pull-request and main CI publish release-readiness closure artifacts:

- `ao2-release-publication-closure`
- `ao2-dual-repo-release-publication-closure-index`

The dual-repo index downloads AO2's release publication closure plus the latest
successful control-plane `ao2-control-plane-release-publication-closure`
artifact and validates both summaries.

Required schema versions:

- `ao2.release-publication-dry-run-closure.v1`
- `ao2.dual-repo-release-publication-closure-index.v1`
- `ao2.cp-release-publication-closure.v1`

The control-plane CI publishes:

- `ao2-control-plane-release-publication-closure`

That summary must include `checksum_verified=true` and trust-boundary values
equivalent to `mutates_github_releases=false` and
`credential_material_included=false`.

## Download Evidence

Use GitHub Actions artifacts as the durable hosted evidence source. Replace
`<run-id>` with the workflow run ID you are inspecting.

```sh
gh run download <run-id> --repo uesugitorachiyo/ao2 \
  --name ao2-dual-repo-release-publication-closure-index \
  --dir target/release-verification/ao2-dual-repo

gh run download <run-id> --repo uesugitorachiyo/ao2-control-plane \
  --name ao2-control-plane-post-release-verification-ubuntu \
  --dir target/release-verification/control-plane-ubuntu
```

For a full control-plane post-release verification run, download all three
per-OS artifacts and inspect each `summary.json`.

## Operator release evidence bundle

Run `npm run release:operator-evidence-bundle` to download the complete
operator-facing release evidence set into
`target/operator-release-evidence-bundle/latest`. The command emits
`ao2.operator-release-evidence-bundle.v1` and verifies:

- AO2 `ao2-dual-repo-release-publication-closure-index`;
- AO2 `post-stable-release-smoke-Linux`,
  `post-stable-release-smoke-macOS`, and
  `post-stable-release-smoke-Windows` install/update evidence with
  `signature_verified=true`;
- AO2 `ao2-dual-public-release-smoke` evidence with
  `ao2.dual-public-release-smoke.v1`, the published AO2 Linux x86_64 archive,
  the published control-plane Linux x86_64 archive, and task-board readback
  schemas `ao2.cp-ai-task-board-readback.v1` and
  `ao2.cp-ai-task-board-dashboard.v1`;
- AO2 `ao2-public-release-pair-digest-audit` evidence with
  `ao2.public-release-pair-digest-audit.v1` and
  `archive_parity.status=passed`, proving every AO2 and control-plane release
  archive in the dual-repo closure index has matching public GitHub Release
  digest and size evidence;
- control-plane `ao2-control-plane-post-release-verification-ubuntu`,
  `ao2-control-plane-post-release-verification-macos`, and
  `ao2-control-plane-post-release-verification-windows` summaries with
  `checksum_verified=true`;
- read-only trust-boundary values showing
  `mutates_github_releases=false` and `credential_material_included=false`.

Use `AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR=<path>` or
`--fixture-dir <path>` for offline fixture verification.

The `Operator Release Evidence Audit` workflow runs this bundle assembly on a
weekly schedule and by manual dispatch. It uploads the complete
`ao2-operator-release-evidence-bundle` artifact, including `summary.json`, as a
read-only hosted baseline. To inspect it from `ao2-control-plane`, download that
artifact and start the server with
`AO2_CP_OPERATOR_RELEASE_EVIDENCE_SUMMARY=<downloaded-artifact>/summary.json`;
the control-plane `/api/v1/release/operator-evidence` and
`/api/v1/release/operator-evidence.json` routes then render the same seven
checks without approving releases or mutating AO2 artifacts.

## Stable promotion evidence gate

`npm run release:stable-promotion-workflow` uses this hosted evidence before it
can promote AO2 and `ao2-control-plane` releases from prerelease to stable. The
workflow downloads the latest successful AO2 `Post Stable Release Verification`
artifacts, including `ao2-dual-public-release-smoke`, the latest successful AO2
`Post Release Pair Digest Audit` artifact `ao2-public-release-pair-digest-audit`,
and the latest successful control-plane `Post Release Verification` artifacts,
then emits `ao2.stable-promotion-evidence-gate.v1`.

The gate requires:

- AO2 `post-stable-release-smoke-Linux`, `post-stable-release-smoke-macOS`,
  and `post-stable-release-smoke-Windows` artifacts with
  `signature_verified=true` install/update evidence;
- AO2 `ao2-dual-public-release-smoke` with
  `ao2.dual-public-release-smoke.v1`, passed task-board readback/dashboard
  schemas, `auth_value_stored=false`, `credential_material_in_urls=false`,
  `mutates_github_releases=false`, and
  `control_plane_approves_release=false`;
- AO2 `ao2-public-release-pair-digest-audit` with
  `ao2.public-release-pair-digest-audit.v1`, `status=passed`,
  `archive_parity.status=passed`, `mutates_releases=false`, and
  `stores_credentials=false`. Downloaded GitHub artifact contents place the
  summary at `post-release-pair-digest-audit/summary.json` under the artifact
  root; the gate also accepts the legacy fixture path
  `target/post-release-pair-digest-audit/summary.json`;
- control-plane `ao2-control-plane-post-release-verification-ubuntu`,
  `ao2-control-plane-post-release-verification-macos`, and
  `ao2-control-plane-post-release-verification-windows` artifacts with
  `checksum_verified=true`;
- control-plane trust-boundary values showing
  `mutates_github_releases=false` and `credential_material_included=false`.

`AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD=1` is only for local dry-run
inspection. It makes the evidence gate emit a skipped/not-ready summary and
keeps confirmed stable promotion blocked.

## Acceptance Checklist

- AO2 post-stable release verification passes on Ubuntu, macOS, and Windows.
- AO2 `ao2-dual-public-release-smoke` proves the published AO2 and
  control-plane archives interoperate from downloaded release assets.
- AO2 `ao2-public-release-pair-digest-audit` proves every required AO2 and
  control-plane archive has public digest/size parity with the dual-repo
  closure index.
- Control-plane post-release verification passes on Ubuntu, macOS, and Windows.
- AO2 `ao2-dual-repo-release-publication-closure-index` validates
  `ao2-control-plane-release-publication-closure`.
- Control-plane closure summaries report `checksum_verified=true`.
- Trust-boundary summaries remain read-only and report
  `mutates_github_releases=false` and `credential_material_included=false`.
