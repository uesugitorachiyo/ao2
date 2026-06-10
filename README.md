# AO2

[Watch the AO2 overview video](https://youtu.be/p222b0iCpbg)

[![Latest release](https://img.shields.io/github/v/release/uesugitorachiyo/ao2?include_prereleases&label=latest%20release)](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.4.80)

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

## Install From Public Prerelease

The current public prerelease is
[`v0.4.80`](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.4.80).
It publishes release archives for macOS, Ubuntu/Linux x86_64, and Windows,
plus `SHA256SUMS` and release-readiness JSON evidence.

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

## Documentation

- [Install](docs/INSTALL.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Product requirements](docs/PRD.md)
- [Risky PR Run SDD](docs/SDD-risky-pr-run.md)
- [Schemas and interfaces](docs/SCHEMAS-AND-INTERFACES.md)
- [Implementation slices](docs/IMPLEMENTATION-SLICES.md)
- [Security](docs/SECURITY.md)
- [Verification](docs/VERIFICATION.md)

## License

AO2 is licensed under `MIT OR Apache-2.0`, at your option. See `LICENSE`,
`LICENSE-MIT`, and `LICENSE-APACHE`.

Third-party dependency license metadata is tracked in
[`docs/THIRD-PARTY-LICENSES.md`](docs/THIRD-PARTY-LICENSES.md).
