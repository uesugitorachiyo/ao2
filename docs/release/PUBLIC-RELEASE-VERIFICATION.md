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

AO2 also uses `Public Release Consumer Smoke` in
`.github/workflows/public-release-consumer-smoke.yml`. It can be dispatched
manually and runs on schedule. It downloads the public AO2 and control-plane
release archives for each hosted target, verifies each archive against the
published `SHA256SUMS`, safely extracts `RELEASE-MANIFEST.json`, and runs
consumer-facing binary commands without starting release mutation flows.

Expected public consumer evidence artifacts:

- `public-release-consumer-smoke-linux`
- `public-release-consumer-smoke-macos`
- `public-release-consumer-smoke-windows`

Run `npm run release:public-consumer-smoke -- --target-label linux-x86_64`,
`macos-aarch64`, or `windows-x86_64` for the same local check. The emitted
`summary.json` uses `ao2.public-release-consumer-smoke.v1` and records
`downloads_public_release_archives=true`,
`mutates_github_releases=false`, `credential_material_included=false`, and
`control_plane_approves_release=false`.

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
- AO2 `public-release-consumer-smoke-linux`,
  `public-release-consumer-smoke-macos`, and
  `public-release-consumer-smoke-windows` evidence with
  `ao2.public-release-consumer-smoke.v1`, target labels `linux-x86_64`,
  `macos-aarch64`, and `windows-x86_64`, AO2 and control-plane release
  manifest schemas, AO2/control-plane help command smoke statuses, and
  `downloads_public_release_archives=true`;
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
`/api/v1/release/operator-evidence.json` routes then render the same twelve
checks without approving releases or mutating AO2 artifacts.

## Stable release evidence packet

Run `npm run release:stable-evidence-packet` after the stable promotion workflow,
operator release evidence bundle, RSI cross-repo E2E, and RSI eligibility packet
have produced local summaries. The command composes
`ao2.stable-promotion-workflow.v1`, `ao2.operator-release-evidence-bundle.v1`,
`ao2.rsi-cross-repo-e2e.v1`, and `ao2.rsi-eligibility-packet.v1` into
`target/stable-release-evidence-packet/latest/summary.json` and
`dashboard.html`, using schema `ao2.stable-release-evidence-packet.v1`. The
packet also carries the nested `ao2.rsi-improvement-evidence-gate.v1` summary
from RSI E2E and requires `measured_improvement_percent >= 5`. It also carries
`ao2.rsi-improvement-trend.v1` so operators can inspect
`delta_from_previous_percent` across persisted local trend records. It also
carries `ao2.rsi-eligibility-packet.v1`, requiring
`rsi_eligibility_ready=true` across repeated baseline packets while preserving
`claim_publish_authority=false`. The packet also carries
`ao2.rsi-blueprint-authorization-gate.v1`, requiring the RSI slice to be
authorized by AO Blueprint's tiered gate with
`self_authorized_by_rsi=false`, no claim-publication authority, and no AO
Blueprint self-change authority.

The packet is ready only when the stable promotion evidence gate reports
`post_release_evidence_ready=true` with `evidence_gate_status=passed` and the
operator bundle reports `operator_release_evidence_ready=true`. It also requires
the RSI E2E summary to preserve `claim_publish_decision=deny`,
`claim_publish_authority=false`, and the nested
`covenant.rsi-claim-publish-gate.v1` denial evidence. It also requires the
nested AO Blueprint authorization evidence to preserve the tiered
operator-intent boundary. The improvement metric is release evidence for
workflow hardening only, not proof that full autonomous RSI is publishable. The
trend record also preserves that deny/false boundary. It is a read-only
operator surface: it reads local evidence summaries, records
`mutates_releases=false` and `stores_credentials=false`, and does not approve or
publish releases or RSI claims. Use
`AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY=<path>` and
`AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY=<path>` plus
`AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_SUMMARY=<path>` and
`AO2_STABLE_RELEASE_EVIDENCE_PACKET_RSI_ELIGIBILITY_SUMMARY=<path>` to compose
a packet from preserved release-publication and RSI baselines.

AO2 CI publishes the same composed packet as the
`ao2-stable-release-evidence-packet` GitHub Actions artifact from the
`Stable release evidence packet artifacts` job. That hosted artifact contains
the final `packet/summary.json`, `packet/dashboard.html`, the source
`stable-promotion-workflow/summary.json`, and the source
`operator-release-evidence-bundle/summary.json`, plus the source
`rsi-cross-repo-e2e/latest/summary.json` and
`rsi-eligibility-packet/packet/summary.json`. The
`Release readiness artifact consumer` job downloads this artifact and fails
closed unless `stable_release_evidence_ready=true`,
`mutates_releases=false`, and `stores_credentials=false`.

The manual `Stable Release Promotion` GitHub Actions workflow consumes that
hosted packet before it runs the stable-promotion workflow. Leave
`promotion_confirm` empty for a dry-run. To allow release mutation, set
`promotion_confirm=promote-stable-v0.4.80-v0.1.13`; any other non-empty value
is rejected before `npm run release:stable-promotion-workflow` runs. The
optional `stable_release_evidence_run_id` input pins promotion review to a
specific successful CI run; otherwise the workflow downloads the latest
successful `ao2-stable-release-evidence-packet` artifact from `main` CI. The
workflow uploads `ao2-stable-release-promotion-workflow` evidence for review.

## Stable release promotion dry-run audit

After a dry-run `Stable Release Promotion` dispatch, run
`npm run release:stable-promotion-dry-run-audit` against the downloaded
`ao2-stable-release-promotion-workflow` artifact, or dispatch the manual
`Stable Release Promotion Dry-Run Audit` workflow with
`stable_promotion_run_id=<dry-run-run-id>`. The audit emits
`ao2.stable-promotion-dry-run-audit.v1` and fails closed unless the dry-run
artifact proves `dry_run=true`, `confirmed=false`,
`promotion_status=not_attempted`, `post_release_evidence_ready=true`, a ready
`ao2.stable-release-evidence-packet.v1`, ready operator evidence, and
`mutates_releases=false` / `stores_credentials=false`. Treat this audit as the
last review gate before entering the real
`promotion_confirm=promote-stable-v0.4.80-v0.1.13` value.
It preserves RSI improvement evidence for the downstream checklist while
keeping `claim_publish_decision=deny`.

## Stable promotion operator checklist

After the dry-run audit passes, run
`npm run release:stable-promotion-operator-checklist` against
`ao2-stable-release-promotion-dry-run-audit` `report/summary.json`, or dispatch
the manual `Stable Promotion Operator Checklist` workflow with
`stable_promotion_dry_run_audit_run_id=<dry-run-audit-run-id>`. The checklist
emits `ao2.stable-promotion-operator-checklist.v1`, `summary.json`, and
`checklist.md`, then fails closed unless the dry-run audit is ready, unconfirmed,
non-mutating, and backed by passed post-release evidence. The artifact records
the exact `promotion_confirm=promote-stable-v0.4.80-v0.1.13` value for the
operator, but it does not enter the confirmation string or mutate releases. No
provider API keys are required or accepted. The checklist includes
RSI improvement trend metrics for operator review while preserving the
`claim_publish_decision=deny` boundary for the full autonomous RSI claim.

## Stable promotion dry-run checklist

Dispatch the manual `Stable Promotion Dry-Run Checklist` workflow to convert
the latest successful main-CI `ao2-stable-release-evidence-packet` directly
into hosted dry-run audit and operator checklist evidence. The optional
`stable_release_evidence_run_id` input pins the packet source; otherwise the
workflow downloads the latest successful main-CI packet, reruns
`npm run release:stable-promotion-workflow` with
`AO2_STABLE_PROMOTION_CONFIRM=""`, runs the dry-run audit, generates the
operator checklist, and uploads `ao2-stable-promotion-dry-run-checklist`.

This workflow is the preferred no-mutation rehearsal before entering the real
stable-promotion confirmation string. It uses read-only GitHub permissions,
rejects provider API key environment state, records
`confirmation_entered=false`, and does not mutate releases.

## Stable promotion evidence gate

`npm run release:stable-promotion-workflow` uses this hosted evidence before it
can promote AO2 and `ao2-control-plane` releases from prerelease to stable. The
workflow downloads the latest successful AO2 `Post Stable Release Verification`
artifacts, `Public Release Consumer Smoke` artifacts
`public-release-consumer-smoke-linux`,
`public-release-consumer-smoke-macos`, and
`public-release-consumer-smoke-windows`, including
`ao2-dual-public-release-smoke`, the latest successful AO2 `Post Release Pair
Digest Audit` artifact `ao2-public-release-pair-digest-audit`, and the latest
successful control-plane `Post Release Verification` artifacts, then emits
`ao2.stable-promotion-evidence-gate.v1`.

The gate requires:

- AO2 `post-stable-release-smoke-Linux`, `post-stable-release-smoke-macOS`,
  and `post-stable-release-smoke-Windows` artifacts with
  `signature_verified=true` install/update evidence;
- AO2 `public-release-consumer-smoke-linux`,
  `public-release-consumer-smoke-macos`, and
  `public-release-consumer-smoke-windows` artifacts with
  `ao2.public-release-consumer-smoke.v1`, expected target labels, AO2 and
  control-plane release manifest schemas, passed AO2/control-plane command
  smoke statuses, `downloads_public_release_archives=true`,
  `credential_material_included=false`, `mutates_github_releases=false`, and
  `control_plane_approves_release=false`;
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

## Stable promotion evidence index

`npm run release:stable-promotion-evidence-index` creates a read-only
`ao2.stable-promotion-evidence-index.v1` review packet from the hosted
`ao2-stable-release-evidence-packet`, `ao2-public-release-pair-digest-audit`,
and `ao2-release-artifact-size-budget-audit` artifacts. The manual
`Stable Promotion Evidence Index` workflow downloads those artifacts, validates
the stable evidence packet, the embedded post-release verification gate, public
pair digest archive parity, and lightweight approval-packet size budgets, then
uploads `ao2-stable-promotion-evidence-index`.

Use this index as the first operator review surface before opening the stable
promotion checklist. It records `mutates_releases=false`,
`stores_credentials=false`, and `control_plane_approves_release=false`; it does
not enter the stable promotion confirmation string.

## Acceptance Checklist

- AO2 post-stable release verification passes on Ubuntu, macOS, and Windows.
- AO2 public release consumer smoke passes for `linux-x86_64`,
  `macos-aarch64`, and `windows-x86_64` with
  `ao2.public-release-consumer-smoke.v1` evidence.
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
