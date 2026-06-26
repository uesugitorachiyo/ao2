# AO2

[Watch the AO2 overview video](https://youtu.be/p222b0iCpbg)

[![Latest release](https://img.shields.io/github/v/release/uesugitorachiyo/ao2?label=latest%20stable%20release)](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.4.80)

AO2 is a local-first governed software-delivery system for running agent work
with policy checks, exact-digest approvals, replayable evidence, evaluator
closure, and release-readiness gates.

The first public workflow is the `Risky PR Run`:

```text
objective -> workflow compile -> scoped plan -> policy-denied risky action
-> exact-digest approval -> patch/evidence -> reviewer concern
-> evaluator rejection -> correction -> evaluator acceptance -> evidence export
```

AO2 owns execution and evidence production. The optional
[`ao2-control-plane`](https://github.com/uesugitorachiyo/ao2-control-plane)
repo is a separate self-hosted read-only observer for signed AO2 evidence.

## AO Stack Architecture

This repository is part of the AO agent orchestration stack. Start with the
central architecture guide at
[uesugitorachiyo/ao-architecture](https://github.com/uesugitorachiyo/ao-architecture);
the AO2-specific architecture page is
[ao2](https://github.com/uesugitorachiyo/ao-architecture/tree/main/ao2).

## Successor Boundary

AO2 replaces the deprecated AO Operator / AO Runtime execution path for active
AO work. New execution, provider-free command, SDD command, runtime behavior,
and evaluator-closure work belongs here. Typed state, evidence readback,
retention, and observer workflows belong in
[`ao2-control-plane`](https://github.com/uesugitorachiyo/ao2-control-plane).
The runtime used by AO2 is the in-repo `crates/ao2-runtime` workspace crate;
AO2 does not depend on the deprecated standalone `ao-runtime` repository.

## AO2 Native Runtime And Platform Evidence

AO2's executable path uses the workspace `ao2-runtime` crate directly from
`crates/ao2-cli`; `Cargo.lock` must not contain standalone `ao-runtime` or
`ao-operator` packages. The Python guard
`tests/test_ao2_native_runtime_platform_evidence.py` enforces that boundary in
CI.

Pull request CI also builds and smokes AO2 release archives on Ubuntu, macOS,
and Windows through the hosted release archive smoke job. Native Windows release
downloads remain covered by `.github/workflows/windows-release-smoke.yml`.

## RSI Claim Boundary

AO2 currently supports `bounded_governed_rsi`: local-first Pulse continuation,
task generation, policy gates, replayable evidence, and operator-controlled
publish paths. AO2 does not currently prove
`full_autonomous_self_mutating_rsi`.

Run the local claim audit with:

```sh
npm run rsi:claim-readiness
```

The audit emits `ao2.rsi-claim-readiness-audit.v1` under
`target/rsi-claim-readiness/latest/summary.json`. It allows the bounded claim
when the local Pulse evidence surface is present and denies the full
self-mutating claim until AO2 has mutation authority evidence, live self-change
evidence, rollback evidence for failed self-change, control-plane observer
readback, and Covenant approval to publish that higher claim. Each run also
emits `blocker_delta` with schema
`ao2.rsi-claim-readiness-blocker-delta.v1`, comparing the current full-claim
blocker IDs with the previous `summary.json` in the same output directory when
one exists.

Run the governed self-change dry-run evidence generator with:

```sh
npm run rsi:self-change-dry-run
```

The dry-run emits `ao2.rsi-governed-self-change-dry-run.v1` under
`target/rsi-self-change-dry-run/latest/summary.json`, plus proposed and rollback
patch artifacts for the same change class. It also emits a
`covenant.live-self-change-authority.v1` authority packet candidate at
`target/rsi-self-change-dry-run/latest/live-self-change-authority.packet.json`.
That packet is marked as a dry-run candidate and is not valid evidence for
claim publication until live self-change execution and observer readback exist.
The script applies and rolls back the patches inside a temporary workspace to
prove the rollback path restores the target file. It does not apply the patch
to the repository, mutate the repository, use the network, require provider
keys, or publish the full RSI claim. It is evidence for a governed self-change
rehearsal, not proof of live autonomous self-mutation.

Run the explicit live self-change rehearsal with:

```sh
AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1 npm run rsi:live-self-change-rehearsal
```

The rehearsal emits `ao2.rsi-live-self-change-rehearsal.v1` under
`target/rsi-live-self-change-rehearsal/latest/summary.json`, plus proposed and
rollback patch artifacts. The command is refused unless
`AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1` is set. When enabled, it briefly mutates
`scripts/rsi-claim-readiness-audit.sh`, verifies the changed script, and
rolls the file back to its exact pre-run SHA-256. This is operator-gated live
self-change evidence with local rollback proof; it still does not publish the
full RSI claim. The command does not publish the full RSI claim because
control-plane observer readback, Covenant claim-publish
approval, and retained claim-level evidence are still required.

After `ao2-control-plane` reads back that rehearsal, retain the readback
artifact in AO2's local evidence chain with:

```sh
npm run rsi:live-self-change-readback-index
```

The index consumes AO2's `ao2.rsi-live-self-change-rehearsal.v1` summary and
the control-plane
`ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1` summary, then emits
`ao2.rsi-live-self-change-readback-evidence-index.v1` under
`target/rsi-live-self-change-readback-index/latest/summary.json`. It records
artifact names and SHA-256 values only. It does not mutate AO2, mutate
ao2-control-plane artifacts, publish claims, or approve RSI claims. It does not approve the full RSI claim. This retained readback evidence improves the claim audit input, but Covenant claim-publish approval and stronger claim-publish evidence remain required.

Run the local improvement evidence gate with:

```sh
npm run rsi:improvement-evidence-gate
```

The gate emits `ao2.rsi-improvement-evidence-gate.v1` under
`target/rsi-improvement-evidence-gate/latest/summary.json`. It measures
workflow-hardening coverage as `measured_improvement_percent` for enforced RSI
evidence checks. The default target is 5%, and the current gate compares the
previous six-check RSI evidence baseline with nine enforced checks after the
AO Blueprint authorization prerequisite, the release-readiness dashboard
readback, and improvement gate are added. The summary also includes
`control_surface_readback` so operators can read the result without inferring
authority from the score: `bounded_governed_rsi` is supported when evidence is
passing, the improvement score can be `target_exceeded`, and
`full_autonomous_self_mutating_rsi` remains denied by design. This is
workflow-hardening coverage and evidence of stronger verification coverage, not
proof that the full RSI claim is publishable.

Persist the local improvement trend with:

```sh
npm run rsi:improvement-trend
```

The trend command reads the latest improvement gate summary, appends a local
JSONL record to `target/rsi-improvement-trend/history.jsonl`, and emits
`ao2.rsi-improvement-trend.v1` under
`target/rsi-improvement-trend/latest/summary.json`. It records the current
`measured_improvement_percent`, the previous measurement when present, and
`delta_from_previous_percent`. The trend summary carries the same
`control_surface_readback` boundary as the evidence gate: bounded governed RSI
can improve while full autonomous RSI publication remains denied. It does not
publish or approve RSI claims.

Run the full local cross-repo RSI evidence chain with sibling
`ao2-control-plane` and `ao-covenant` checkouts:

```sh
npm run rsi:cross-repo-e2e
```

The E2E smoke emits `ao2.rsi-cross-repo-e2e.v1` under
`target/rsi-cross-repo-e2e/latest/summary.json`. It runs the AO2 live
self-change rehearsal, ao2-control-plane readback,
`rsi:live-self-change-readback-index`, the release-readiness dashboard
readback smoke, `rsi:claim-readiness`,
`rsi:blueprint-authorization-gate`, and Covenant `policy claim-publish-gate`,
then runs the improvement evidence gate. Set
`AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY` to a real AO Blueprint
`ao.blueprint.build-authorization.v0.1` output when replacing the local fixture.
The
expected final Covenant gate remains `publish_authority=false` with schema
`covenant.rsi-claim-publish-gate.v1`; the E2E summary also carries
`ao2.rsi-improvement-evidence-gate.v1`,
`ao2.rsi-improvement-trend.v1`,
`ao2.rsi-blueprint-authorization-gate.v1`,
`ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1`,
`release_readiness_dashboard_readback`, `dashboard_artifact`,
`control_plane_foundry_packet_readback`,
`ao.foundry.rsi-control-surface-packet.v0.1`,
`ao2.cp-ao-stack-rsi-chain-binding-readback.v1`,
`measured_improvement_percent`, and
`delta_from_previous_percent`.
`control_plane_foundry_packet_readback` records that ao2-control-plane can
consume Foundry's RSI control-surface packet as observer-only readback; it does
not approve RSI claims, publish claims, or expand AO2 authority.
The smoke proves the denial chain and 5% workflow-hardening measurement are
wired end to end, not that the full RSI claim is publishable.

Compose the operator closure packet for the current AO stack RSI boundary with:

```sh
npm run rsi:operator-closure-packet
```

The packet emits `ao2.rsi-operator-closure-packet.v1` under
`target/rsi-operator-closure-packet/latest/summary.json`, plus `closure.md`.
It reads the AO2 cross-repo E2E summary and the ao2-control-plane chain-binding
readback, then states the stable operator boundary in one place:
bounded governed RSI is supported; full autonomous RSI publication remains denied; and control-plane remains observer-only. It is local readback only; it
does not mutate repositories, approve RSI claims, publish claims, execute AO
work, or authorize AO Blueprint self-change.

Compose the operator-readable RSI baseline packet from an existing cross-repo
E2E summary with:

```sh
npm run rsi:baseline-packet
```

The packet emits `ao2.rsi-baseline-packet.v1` under
`target/rsi-baseline-packet/latest/summary.json`, plus `dashboard.html`. It
surfaces the RSI trend, Blueprint authorization, Covenant denial, and
control-plane readback inputs in one compact packet, including the
release-readiness dashboard readback for
`ao2-release-readiness-consumer/dashboard.html`. It fails closed unless the 5%
workflow-hardening metric is met and the full autonomous RSI claim remains
denied with `publish_authority=false`. The packet reads local evidence only; it
does not mutate repositories, publish claims, approve RSI claims, or authorize
AO Blueprint self-change.

After two baseline packets exist, compose the repeated-run RSI eligibility
packet with:

```sh
npm run rsi:eligibility-packet
```

The packet emits `ao2.rsi-eligibility-packet.v1` under
`target/rsi-eligibility-packet/latest/summary.json`, plus `dashboard.html`. It
fails closed unless both baseline packets are ready, both preserve
`claim_publish_decision=deny` and `claim_publish_authority=false`, and both are
backed by tiered AO Blueprint authorization that is not self-authorized by RSI.
Both baselines must also carry the control-plane release-readiness dashboard
readback with `dashboard_link_ready=true`.
This is eligibility/readback evidence only; it does not publish claims, approve
RSI claims, mutate repositories, or authorize AO Blueprint self-change. CI
uploads it as `ao2-rsi-eligibility-packet`.

## Why AO2?

Most agent systems focus on doing work. AO2 focuses on making the work
reviewable after the fact.

AO2 is built around local evidence:

- what objective was run;
- which policy and readiness gates executed;
- what commands, patches, and artifacts were produced;
- which evaluator concerns were rejected or accepted;
- what evidence supports a completed run;
- what can be replayed, audited, exported, or published to an observer.

That makes AO2 useful for autonomous or overnight work because the operator does
not have to trust terminal scrollback or a vague "done" message. The run leaves
behind structured records that can be inspected locally and, when desired,
published to a read-only control plane.

## Status

This public export is prepared from AO2 `0.4.80`. It is intentionally
local-first:

- no provider API-key authentication paths;
- no bundled runtime evidence or generated release artifacts;
- no private git history;
- no control-plane mutation authority.

## Quick Start

```sh
git clone https://github.com/uesugitorachiyo/ao2.git
cd ao2
npm run verify
npm run build:release
```

Run the governed demo locally:

```sh
tmpdir=$(mktemp -d /tmp/ao2-demo.XXXXXX)
cp -R fixtures/discount-service "$tmpdir/discount-service"
cargo run -p ao2-cli --bin ao2 -- \
  run examples/risky-pr-run/risky-pr.yaml \
  --target "$tmpdir/discount-service" \
  --run-id demo-run
```

Build a local release archive:

```sh
npm run package:local
tmpdir=$(mktemp -d /tmp/ao2-release.XXXXXX)
archive=$(ls dist/ao2-0.4.80-*.tar.gz | head -1)
tar -xzf "$archive" -C "$tmpdir"
sh "$tmpdir/verify-release.sh"
```

Release archives also include `Verify-Release.ps1` for native Windows
checksum verification before install.

## Install From Stable Public Release

The current stable public release is
[`v0.4.80`](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.4.80).
It publishes release archives for macOS, Ubuntu/Linux x86_64,
Ubuntu/Linux aarch64, and Windows, plus `SHA256SUMS`, signed provenance, and
release-readiness JSON evidence.
The overview video is available at
[https://youtu.be/p222b0iCpbg](https://youtu.be/p222b0iCpbg).

Download and verify a macOS archive:

```sh
mkdir -p dist-release
gh release download v0.4.80 --repo uesugitorachiyo/ao2 \
  --pattern ao2-0.4.80-macos-aarch64.tar.gz \
  --pattern SHA256SUMS \
  --dir dist-release
(cd dist-release && grep 'ao2-0.4.80-macos-aarch64.tar.gz' SHA256SUMS | shasum -a 256 -c -)
```

Use the same release base URL for Linux and Windows archives:

```text
https://github.com/uesugitorachiyo/ao2/releases/download/v0.4.80/ao2-0.4.80-linux-x86_64.tar.gz
https://github.com/uesugitorachiyo/ao2/releases/download/v0.4.80/ao2-0.4.80-linux-aarch64.tar.gz
https://github.com/uesugitorachiyo/ao2/releases/download/v0.4.80/ao2-0.4.80-windows-x86_64.tar.gz
```

Run the Phase 1 promotion wrapper after starting a local ao2-control-plane
instance and placing the control-plane bearer token in an environment variable:

```sh
export AO2_PHASE1_CONTROL_PLANE_URL=http://127.0.0.1:3000
export AO2_PHASE1_API_TOKEN_ENV=AO2_CP_API_TOKEN
export AO2_CP_API_TOKEN=<redacted-local-token>
npm run phase1:prepare-prerequisites
npm run phase1:promote
```

The wrapper publishes through `--api-token-env AO2_CP_API_TOKEN` so the bearer
token stays out of process arguments, URLs, logs, and generated evidence. To
capture the read-only control-plane dashboard in the same local run:

```sh
AO2_PHASE1_DASHBOARD_SNAPSHOT=1 npm run phase1:promote
npm run phase1:dashboard-snapshot
```

Run the native Windows release smoke on a Windows host after building or
downloading the current archive:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\smoke-windows-release.ps1 `
  -Archive .\dist-windows\ao2-0.4.80-windows-x86_64.tar.gz
```

The main CI workflow in `.github/workflows/ci.yml` runs on pull request and
`main` push, and can also be dispatched manually. Release workflows such as
`release-gate.yml` and `public-release-build.yml` remain manual operator gates.
Branch protection requirements and the scheduled/manual read-only drift check
are documented in `docs/BRANCH-PROTECTION.md`, including the full local audit
for stale required checks in active branch rulesets.

## Release readiness evidence chain

The canonical CI readiness signal is
`ao2-release-readiness-final-closure-verifier`. It only passes after the
upstream release-readiness artifacts have been produced, consumed, and checked
for publication closure.

```text
ao2-release-readiness -> ao2-release-readiness-hosted-artifact-gate
-> ao2-release-readiness-consumer
-> ao2-release-readiness-final-closure-verifier
```

Use the final verifier artifact to decide whether the public AO2 release
readiness evidence chain is closed. Earlier artifacts remain useful for
debugging the specific gate that produced them. The consumer artifact also
includes `dashboard.html`, which gives operators an RSI eligibility readback
without granting autonomous RSI claim-publication authority.

## Pulse Auto-Advance Evidence

Pulse auto-advance can continue local AO2 work without opening a pull request.
Even in no-PR mode, it is not silent: it writes local evidence for each
iteration so an operator can answer "what happened while I was away?"

The primary local evidence surfaces are:

- `target/pulse-auto-advance/latest/summary.json` - current run status,
  completed iteration count, task results, direct-main publish status, and
  next-packet generation status.
- `target/pulse-auto-advance/latest/task-executor/iteration-XX/summary.json` -
  per-iteration task executor summaries.
- `target/pulse-auto-advance/latest/logs/` - per-command logs for task
  execution, PR/CI gate refresh, direct-main publishing, and next-task
  generation.
- `.ao2-local/pulse/latest/` - the latest generated packet, board,
  eval-loop, operator prompt, resume metadata, and structured task manifest.
- `.ao2-local/pulse/pulse-auto-advance-ledger.jsonl` - append-only local
  ledger entries keyed by eval-loop digest.
- `.ao2-local/pulse/pr-ci-gate.json` - local PR/CI gate state when the loop is
  waiting on review, merge, or CI.

When direct-main publishing is enabled, Pulse also records
`target/pulse-auto-advance/latest/direct-main-publish/summary.json`. If there
are no source changes to commit, the publisher can exit successfully with
`status=skipped`; the local Pulse evidence still records the iteration, logs,
and generated next-task packet.

This keeps the MVP local-first: PRs and GitHub CI are useful review surfaces,
but they are not required for AO2 to leave an auditable local record.

## Pulse Event-Loop Runtime

AO2 contains a typed, durable, cross-platform Pulse event-loop runtime in Rust.
It can execute a bounded loop over a command, reading a decision file
(supporting native AO2 and legacy-compatible decision schemas), and writing durable
summary evidence:

```sh
ao2 pulse run-loop \
  --command "npm run pulse:generate-next" \
  --decision-file "target/pulse-next-recommended-tasks/ao2-event-loop-decision.json" \
  --max-chain-runs 3 \
  --max-runtime-seconds 2700 \
  --out-dir "target/pulse-event-loop"
```

## Documentation

- [Install](docs/INSTALL.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Product requirements](docs/PRD.md)
- [Risky PR Run SDD](docs/SDD-risky-pr-run.md)
- [Schemas and interfaces](docs/SCHEMAS-AND-INTERFACES.md)
- [Implementation slices](docs/IMPLEMENTATION-SLICES.md)
- [Security](docs/SECURITY.md)
- [Verification](docs/VERIFICATION.md)
- [Public release verification](docs/release/PUBLIC-RELEASE-VERIFICATION.md)

## License

AO2 is licensed under `Apache-2.0`. See `LICENSE`.

Third-party dependency license metadata is tracked in
[`docs/THIRD-PARTY-LICENSES.md`](docs/THIRD-PARTY-LICENSES.md).
