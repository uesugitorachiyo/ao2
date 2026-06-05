# AO2

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
```

Run the native Windows release smoke on a Windows host after building or
downloading the current archive:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\smoke-windows-release.ps1 `
  -Archive .\dist-windows\ao2-0.4.80-windows-x86_64.tar.gz
```

The GitHub workflows in `.github/workflows/` are manual-only templates.

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
