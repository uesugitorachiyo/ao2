# Public Release Verification

This is the operator index for verifying public release evidence across
`uesugitorachiyo/ao2` and `uesugitorachiyo/ao2-control-plane`.

The current public release pair is:

- AO2 stable release: `v0.4.80`
- AO2 control-plane prerelease: `v0.1.12`

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

The control-plane uses `Post Release Verification` in
`.github/workflows/post-release-verification.yml`. It can be dispatched
manually and runs on schedule. It downloads all public `v0.1.12` release
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

## Stable promotion evidence gate

`npm run release:stable-promotion-workflow` uses this hosted evidence before it
can promote AO2 and `ao2-control-plane` releases from prerelease to stable. The
workflow downloads the latest successful AO2 `Post Stable Release Verification`
artifacts and the latest successful control-plane `Post Release Verification`
artifacts, then emits `ao2.stable-promotion-evidence-gate.v1`.

The gate requires:

- AO2 `post-stable-release-smoke-Linux`, `post-stable-release-smoke-macOS`,
  and `post-stable-release-smoke-Windows` artifacts with
  `signature_verified=true` install/update evidence;
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
- Control-plane post-release verification passes on Ubuntu, macOS, and Windows.
- AO2 `ao2-dual-repo-release-publication-closure-index` validates
  `ao2-control-plane-release-publication-closure`.
- Control-plane closure summaries report `checksum_verified=true`.
- Trust-boundary summaries remain read-only and report
  `mutates_github_releases=false` and `credential_material_included=false`.
