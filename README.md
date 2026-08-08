# AO2

[Watch the AO2 overview video](https://youtu.be/pGhPooqC3hQ)

[![Latest release](https://img.shields.io/github/v/release/uesugitorachiyo/ao2?label=latest%20stable%20release)](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.10)

AO2 is the governed execution runtime for local agent work. It compiles and runs scoped workflows, enforces policy and exact-digest approvals, invokes execution adapters, evaluates results, and emits replayable evidence. Use AO2 when an authorized plan is ready to execute and the run must remain reviewable, reproducible, and bound to its approved inputs.

The first public workflow is the `Risky PR Run`:

```text
objective -> workflow compile -> scoped plan -> policy-denied risky action
-> exact-digest approval -> patch/evidence -> reviewer concern
-> evaluator rejection -> correction -> evaluator acceptance -> evidence export
```

## How it fits in AO

- **Primary responsibility:** Governed execution and run-evidence production.
- **Inputs:** Workflows, scoped plans, Covenant decisions, approvals, and adapter settings.
- **Outputs:** Execution events, patches, evaluations, closure records, and evidence bundles.
- **Upstream:** AO Forge and AO Covenant.
- **Downstream:** AO2 Control Plane, AO Arena, AO Crucible, and AO Sentinel.

See the
[AO Architecture guide](https://github.com/uesugitorachiyo/ao-architecture)
and the
[AO2 component page](https://github.com/uesugitorachiyo/ao-architecture/blob/main/components/ao2.md)
for the cross-repository flow.

<!--
Legacy documentation-test compatibility tokens (not rendered):
npm run rsi:eligibility-packet
ao2.rsi-eligibility-packet.v1
ao2-rsi-eligibility-packet
npm run rsi:claim-readiness
bounded_governed_rsi
full_autonomous_self_mutating_rsi
npm run rsi:self-change-dry-run
ao2.rsi-governed-self-change-dry-run.v1
npm run rsi:live-self-change-rehearsal
AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1
ao2.rsi-live-self-change-rehearsal.v1
rolls the file back
does not publish the full RSI claim
npm run rsi:live-self-change-readback-index
ao2.rsi-live-self-change-readback-evidence-index.v1
ao2.cp-ao2-rsi-live-self-change-rehearsal-readback.v1
does not approve the full RSI claim
npm run live-mutation:dry-run-packet
ao2.live-mutation-dry-run-packet.v1
does not apply the patch
does not call providers
temporary workspace
npm run rsi:cross-repo-e2e
ao2.rsi-cross-repo-e2e.v1
target/rsi-cross-repo-e2e/latest/summary.json
ao2.rsi-improvement-evidence-gate.v1
ao2.rsi-improvement-trend.v1
ao2.rsi-blueprint-authorization-gate.v1
ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1
release_readiness_dashboard_readback
control_plane_foundry_packet_readback
ao.foundry.rsi-control-surface-packet.v0.1
ao2.cp-ao-stack-rsi-chain-binding-readback.v1
dashboard_artifact
AO2_RSI_BLUEPRINT_AUTHORIZATION_SUMMARY
measured_improvement_percent
control_surface_readback
target_exceeded
workflow-hardening coverage
covenant.rsi-claim-publish-gate.v1
publish_authority=false
npm run rsi:operator-closure-packet
ao2.rsi-operator-closure-packet.v1
target/rsi-operator-closure-packet/latest/summary.json
closure.md
bounded governed RSI is supported
full autonomous RSI publication remains denied
control-plane remains observer-only
-->

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

This public export is prepared from AO2 `0.5.10`. It is intentionally
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

Validate a sanitized historical GitHub issue repair pack locally:

```sh
cargo run -p ao2-cli --bin ao2 -- issue repair-pack validate \
  --manifest /path/to/manifest.json \
  --root /path/to/pack-root \
  --json
```

This command is validation-only. It does not unpack the source archive, execute
a repair, access the network, invoke Git or GitHub, mutate a repository, or
grant authority. The manifest and all referenced artifacts must be direct
children of the repair pack root. See
[the GitHub issue repair-pack contract](docs/contracts/GITHUB-ISSUE-REPAIR-PACK.md).

Build a local release archive:

```sh
npm run package:local
tmpdir=$(mktemp -d /tmp/ao2-release.XXXXXX)
archive=$(ls "dist/ao2-$(scripts/current-version.sh)-"*.tar.gz | head -1)
tar -xzf "$archive" -C "$tmpdir"
sh "$tmpdir/verify-release.sh"
```

Release archives also include `Verify-Release.ps1` for native Windows
checksum verification before install.

## Install From Stable Public Release

The current stable public release is
[`v0.5.10`](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.10).
It publishes native release archives for macOS aarch64, Linux x86_64, and
Windows x86_64, plus `promotion-plan.json` and aggregate `SHA256SUMS`. The
expected compatible stable companion is
[AO2 Control Plane v0.1.19](https://github.com/uesugitorachiyo/ao2-control-plane/releases/tag/v0.1.19).
The overview video is available at
[https://youtu.be/pGhPooqC3hQ](https://youtu.be/pGhPooqC3hQ).

Download and verify a macOS archive:

```sh
mkdir -p dist-release
gh release download v0.5.10 --repo uesugitorachiyo/ao2 \
  --pattern ao2-0.5.10-macos-aarch64.tar.gz \
  --pattern SHA256SUMS \
  --dir dist-release
(cd dist-release && grep 'ao2-0.5.10-macos-aarch64.tar.gz' SHA256SUMS | shasum -a 256 -c -)
```

Use the same release base URL for Linux and Windows archives:

```text
https://github.com/uesugitorachiyo/ao2/releases/download/v0.5.10/ao2-0.5.10-linux-x86_64.tar.gz
https://github.com/uesugitorachiyo/ao2/releases/download/v0.5.10/ao2-0.5.10-windows-x86_64.tar.gz
```

## First 30 Minutes With AO2

Use the [first 30 minutes guide](docs/FIRST-30-MINUTES.md) to verify the
public archive, install AO2, run `ao2 doctor`, and execute the governed demo.
For install, rollback, and uninstall details, see [Install](docs/INSTALL.md).
For common support cases, see [Troubleshooting](docs/TROUBLESHOOTING.md).

For Rust/Cargo work with the published `v0.5.10` binary, run the Cargo workflow
file by path from an AO2 checkout at the `v0.5.10` tag or newer:

```sh
ao2 run examples/task-templates/rust-cargo-bug-fix.yaml \
  --target /path/to/rust-crate \
  --provider codex \
  --provider-prompt-file prompt.txt
```

Use `cargo test` as the verifier for Rust projects.

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
  -Archive .\dist-windows\ao2-0.5.10-windows-x86_64.tar.gz
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
includes `dashboard.html`, which gives operators a run-eligibility readback
without granting execution or publication authority.

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
- [First 30 Minutes With AO2](docs/FIRST-30-MINUTES.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Product requirements](docs/PRD.md)
- [Risky PR Run SDD](docs/SDD-risky-pr-run.md)
- [Schemas and interfaces](docs/SCHEMAS-AND-INTERFACES.md)
- [Implementation slices](docs/IMPLEMENTATION-SLICES.md)
- [Security](docs/SECURITY.md)
- [Verification](docs/VERIFICATION.md)
- [Public release verification](docs/release/PUBLIC-RELEASE-VERIFICATION.md)
- [AO2 v0.5.10 stable release notes](docs/release/v0.5.10-stable.md)

## License

AO2 is licensed under `Apache-2.0`. See `LICENSE`.

Third-party dependency license metadata is tracked in
[`docs/THIRD-PARTY-LICENSES.md`](docs/THIRD-PARTY-LICENSES.md).
