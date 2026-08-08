# Ready-to-Ship Release Gate

Status: AO2 `v0.5.10` published and qualified; stable-train reconciliation complete

## Current Stable Release

The current stable train is AO2 `v0.5.10` with AO2 Control Plane `v0.1.19`.
AO2 `v0.5.10` was published from qualified source
`9f4f8a8cf596127a982627b4af25c90a9a842095`. Its five public release assets,
checksums, attestations, three native platform installs, doctor checks,
governed fixtures, and explicit companion compatibility completed release
qualification before this documentation reconciliation.

The release is available at
<https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.10>. This document is a
readback of the completed release; it does not authorize tag or release
mutation, asset replacement, package publication, deployment, or preparation
of a later patch candidate.

## Official Gate

`release-gate.yml` is the official ready-to-ship workflow for the AO2 private
release line. It is intentionally `workflow_dispatch` only: a release gate is a
human-initiated release-line decision, not a push-to-main side effect.

The local equivalent is:

```sh
npm run release:build-all
npm run verify:no-factory-v3
npm run gate:full
```

Expected terminal evidence:

```text
no_factory_v3_green_path_status=passed
gate_with_replacement_verdict=PASS
gate_with_replacement_passed=3/3
```

## Stage Contract

`npm run gate:full` must pass all three stages:

1. `no_factory_v3_green_path` — blocks new unclassified factory-v3 green-path
   dependencies.
2. `replacement_parity` — verifies AO2-native producer coverage and the
   read-only factory-v3 parity oracle.
3. `release_gate` — verifies cross-OS archives, signed provenance, and
   release smoke evidence.

## Required Artifacts

The workflow must upload:

- `no-factory-v3-green-path-<sha>` from
  `target/no-factory-v3-green-path/`
- `gate-full-rollup-<sha>` from
  `target/release-gate-with-replacement/`

## Trust Boundary

- AO2 is the canonical producer.
- factory-v3 is parity oracle, audit reference, or evaluator-closer owner only.
- ao2-control-plane is a read-only observer.
- No provider API-key authentication is introduced.
- No bearer tokens or secrets are stored in workflow artifacts.
