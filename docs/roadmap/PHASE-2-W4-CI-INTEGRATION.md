# Phase 2 W4 — `gate:full` CI integration design

**Status:** Drop-in ready (updated 2026-05-27, includes Stage 0
`verify:no-factory-v3` guard)
**Anchor:** [PHASE-2-FACTORY-V3-RETIREMENT.md](./PHASE-2-FACTORY-V3-RETIREMENT.md) W4

## Goal

Make `npm run gate:full` the canonical CI gate that the release line
runs before any artifact ships. After Phase 2 W4 lands, no release
branch will be tagged or promoted without `gate:full` passing in a
clean GitHub Actions environment.

## Current workflow inventory

The repo has three GitHub Actions workflows today:

| Workflow | Trigger | Job summary |
| --- | --- | --- |
| `.github/workflows/ci.yml` | `workflow_dispatch` | 3-OS matrix (ubuntu/macos/windows); `npm run verify` + `npm run build:release`; uploads `ao2` binary as artifact |
| `.github/workflows/private-release-build.yml` | `workflow_dispatch` | ubuntu-latest only; `npm run release:build-all`; uploads `dist/*.tar.gz`, `dist-linux/*.tar.gz`, `dist-windows/*.tar.gz`, `dist-provenance/*` |
| `.github/workflows/windows-release-smoke.yml` | `workflow_dispatch` | windows-latest only; downloads a pinned-version release archive; runs `smoke-windows-release.ps1` |

All three are `workflow_dispatch` (manual trigger). Phase 2 W4 keeps
`workflow_dispatch` as the trigger to avoid surprise runs — release
gates should be a human decision, not a push-to-main side effect.

## Proposed new workflow: `release-gate.yml`

```yaml
name: Release Gate (full)

on:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  gate-full:
    name: Release-gate-with-replacement-parity
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy,rustfmt
          targets: x86_64-pc-windows-gnu

      - name: Install Node (for npm scripts)
        uses: actions/setup-node@v4
        with:
          node-version: '20'

      - name: Run no factory-v3 green-path guard
        run: npm run verify:no-factory-v3

      - name: Build release assets (3 archives + provenance)
        run: npm run release:build-all

      - name: Run gate:full
        run: npm run gate:full

      - name: Upload no factory-v3 guard report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: no-factory-v3-green-path-${{ github.sha }}
          path: target/no-factory-v3-green-path/
          if-no-files-found: error
          retention-days: 90

      - name: Upload gate:full rollup
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: gate-full-rollup-${{ github.sha }}
          path: target/release-gate-with-replacement/
          if-no-files-found: error
          retention-days: 90
```

### Design notes

1. **`fetch-depth: 0`** — gate:full's Stage A embeds `git rev-parse
   HEAD` in the rollup, and the parity oracle prefers full history.
   Cheap on this repo (small history).

2. **No three-OS matrix here** — `gate:full` is the *gate*, not the
   *build*. It runs on ubuntu-latest and asserts that the cross-OS
   archives produced by `release:build-all` (which already
   cross-compiles for windows-gnu and linux-aarch64 in the same job)
   all verify. Adding a matrix here would only verify the gate logic
   itself, not the cross-OS artifacts.

3. **`workflow_dispatch` trigger** — explicit human-initiated runs
   only. Phase 2 has no automatic release line; that's a Phase 3
   decision.

4. **Stage 0 guard** — `npm run verify:no-factory-v3` runs before the
   expensive release build and again inside `gate:full`. The standalone
   preflight gives quick CI feedback and uploads a dedicated
   `ao2.no-factory-v3-green-path.v1` report.

5. **`if: always()` on rollup upload** — even when gate:full FAILs, the
   rollup is uploaded so the failure can be diagnosed from the artifact
   without re-running.

6. **`retention-days: 90`** — release-gate rollups are auditable
   release evidence; 90 days matches release-cadence assumptions.

7. **`permissions: contents: read`** — minimum required. No write
   permissions; gate:full does not mutate state.

## Updates to existing workflows

### `ci.yml` — no change

CI on per-OS verify+build remains as-is. It catches per-OS
breakages. It does NOT run gate:full because that requires
release-archive cross-compilation which is a heavier dependency.

### `private-release-build.yml` — add gate:full

After `release:build-all` succeeds, add a `gate:full` step. This
chains the build with verification in the same workflow run so
the uploaded artifacts are always verified-clean.

Diff:

```yaml
      - name: Build release assets
        run: npm run release:build-all

      # ADD:
      - name: Run no factory-v3 green-path guard
        run: npm run verify:no-factory-v3

      # ADD:
      - name: Run gate:full
        run: npm run gate:full

      # ADD:
      - name: Upload no factory-v3 guard report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: no-factory-v3-green-path-${{ github.sha }}
          path: target/no-factory-v3-green-path/
          if-no-files-found: error
          retention-days: 90

      # ADD:
      - name: Upload gate:full rollup
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: gate-full-rollup-${{ github.sha }}
          path: target/release-gate-with-replacement/
          if-no-files-found: error
          retention-days: 90

      - name: Upload release assets
        uses: actions/upload-artifact@v4
        ...
```

### `windows-release-smoke.yml` — no change in W4

Windows smoke remains a separate manual workflow. (Phase 2 W4
follow-up: chain it after `release-gate.yml` succeeds via
`workflow_run` trigger.)

## Acceptance criteria for W4

- [x] `.github/workflows/release-gate.yml` exists and is dispatchable
- [ ] A manual `workflow_dispatch` run of `release-gate.yml` against
      HEAD `903beb0` returns success
- [ ] The uploaded `gate-full-rollup-<sha>` artifact contains a
      `rollup.json` with `overall_verdict: "PASS"`
- [ ] The uploaded `no-factory-v3-green-path-<sha>` artifact contains a
      guard report with `failure_count: 0`
- [x] `private-release-build.yml` runs gate:full as part of its
      pipeline and uploads its rollup
- [ ] No factory-v3 invocation in any green-path workflow step
      (parity oracle inside gate:full is the only allowed reference,
      and it is read-only)
- [x] Release-line documentation
      (`docs/release/READY-TO-SHIP.md`) calls `release-gate.yml` the
      official ready-to-ship gate

## Out of scope for W4

- Automatic trigger on push/tag (Phase 3)
- Slack/email notification on gate failure (Phase 3)
- Multi-version regression matrix (Phase 3)
- Per-PR gate:full runs (too slow; not needed)

## Risk + mitigation

| Risk | Mitigation |
| --- | --- |
| GitHub-hosted runners can't cross-compile linux-aarch64 | `release:build-all` already handles this with `cross`; if the workflow proves flaky, switch to a self-hosted aarch64 runner |
| Workflow secret leakage in logs | `gate:full` does not need any secret. Confirm in the first manual dispatch by reading the run log end-to-end |
| Long workflow runtime (build-all + gate:full + cross-OS) | Acceptable — release gates are not per-PR. Target ≤ 30 minutes end-to-end |
| Provenance signing key handling in CI | Out of scope for W4 — current `release:build-all` signs locally; W4 only verifies. Key rotation lands in W5 as part of the CP runbook |

## How to land W4

1. Open a PR that adds `.github/workflows/release-gate.yml` (full
   content above).
2. Manually dispatch the workflow against the PR branch HEAD.
3. Confirm success and the uploaded rollup.
4. Add the gate:full step to `private-release-build.yml` in the same
   PR (or a follow-up PR within the same milestone).
5. Update `README.md` to call `release-gate.yml` the official
   ready-to-ship gate.

## Reproducible verification (local equivalent)

The CI workflow is functionally equivalent to running locally:

```sh
npm run release:build-all
npm run verify:no-factory-v3
npm run gate:full
```

All commands must return exit 0. The guard emits
`no_factory_v3_green_path_status=passed`; the final gate emits
`gate_with_replacement_verdict=PASS gate_with_replacement_passed=3/3`.
