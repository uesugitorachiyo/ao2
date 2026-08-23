# AO2 Install And Update Guide

This guide covers public AO2 stable release install, update, rollback, offline
verification, and uninstall workflows.

For a first AO2 install, use the sections through [Uninstall](#uninstall). Later
sections describe advanced local operation surfaces and are not required for the
first 30 minutes.

AO2 now has a stable public release:
[`v0.5.11`](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.11).
The overview video is available at
[https://youtu.be/pGhPooqC3hQ](https://youtu.be/pGhPooqC3hQ). Release
archives are private-first in trust boundary and public-stable in distribution;
the normal flow is:

1. Download the release assets.
2. Verify aggregate checksums and GitHub artifact attestations.
3. Install the correct archive for the current OS.
4. Confirm the installed binary identity with `ao2 version --json`.
5. Run `ao2 doctor` to confirm install, PATH, release provenance, local tools,
   and provider health.

## Install AO2 v0.5.11 Stable

[`v0.5.11`](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.11)
is the current stable public AO2 release. It is qualified with
[AO2 Control Plane v0.1.19](https://github.com/uesugitorachiyo/ao2-control-plane/releases/tag/v0.1.19).

Choose one supported archive:

- `ao2-0.5.11-macos-aarch64.tar.gz`
- `ao2-0.5.11-linux-x86_64.tar.gz`
- `ao2-0.5.11-windows-x86_64.tar.gz`

Download the complete public asset set, then verify its checksums before using
an archive:

```sh
mkdir -p ao2-stable && cd ao2-stable
gh release download v0.5.11 --repo uesugitorachiyo/ao2
shasum -a 256 -c SHA256SUMS
```

If `gh release download` asks for GitHub CLI authentication, download the
selected archive directly from the public release and verify only that selected
archive line:

```sh
mkdir -p ao2-stable && cd ao2-stable
base_url="https://github.com/uesugitorachiyo/ao2/releases/download/v0.5.11"
curl -fLO "$base_url/SHA256SUMS"
curl -fLO "$base_url/ao2-0.5.11-macos-aarch64.tar.gz"
grep '  ao2-0.5.11-macos-aarch64.tar.gz$' SHA256SUMS > SHA256SUMS.selected
shasum -a 256 -c SHA256SUMS.selected
```

On macOS and Linux, extract the archive for the host and run its offline
verification before installation:

```sh
tar -xzf ao2-0.5.11-<platform>.tar.gz
./verify-release.sh
AO2_INSTALL_DIR="$HOME/.local/bin" ./install.sh
export PATH="$HOME/.local/bin:$PATH"
ao2 version --json
ao2 doctor --json
```

If you install into a different directory, add that directory to `PATH` before
running `ao2 doctor`; doctor discovers the verified custom installation from
the matching PATH binary. When the directory is not on `PATH`, run the binary
by full path and pass the same directory explicitly:

```sh
/path/to/bin/ao2 doctor --json --install-dir /path/to/bin
```

On Windows, extract the archive, run `Verify-Release.ps1`, then run
`install.ps1` and confirm the installed identity with
`ao2.exe version --json`.

The `v0.5.12` Windows archive also places `ao2-windows-worker.cmd` beside the
archive manifest. It requires Python 3.11 or newer and may be checked before
configuration with `ao2-windows-worker.cmd --help`, including from an extract
directory whose path contains spaces. This launcher is not present in the
Linux or macOS archives and does not start a listener by itself.

AO2 `v0.5.11` publishes aggregate checksums and GitHub attestations, but not the
detached RSA provenance files required by the `v0.5.11` `install update`
command. For that released binary, use the verified archive installer shown
above. Do not synthesize detached signatures or claim rollback preservation.

Current source builds support an explicit public-checksum update mode after the
selected archive row has been verified:

```sh
ao2 install update \
  --archive ao2-0.5.11-<platform>.tar.gz \
  --public-checksum-manifest SHA256SUMS
ao2 version --json
```

This mode verifies a strict, regular-file, size-bounded aggregate manifest and
then validates the archive's complete embedded offline contract. It records
`signature_verified=false`, `public_checksum_verified=true`, the manifest
digest, and `verification_mode=public_checksum_manifest`. Signed private
release updates continue to use `--provenance-dir` and do not downgrade to
aggregate-checksum verification.

The source-build update preserves the previous binary as the rollback copy.
Restore it with:

```sh
ao2 install rollback
ao2 version --json
```

To reinstall explicitly without a rollback copy, download and verify the
matching archive from the
[`v0.5.11` release](https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.11)
and run its installer.

Uninstall from the default Unix location with:

```sh
rm -f "$HOME/.local/bin/ao2" \
  "$HOME/.local/bin/ao2.rollback" \
  "$HOME/.local/bin/ao2.install-verification.json"
```

For Windows and custom installation directories, use the platform-specific
removal commands in [Uninstall](#uninstall). Uninstall does not remove
repository-local run evidence.

## Verify Downloaded Release Assets

From the repository checkout:

```sh
npm run release:download-verify
```

This downloads the GitHub release with `gh release download`, verifies every
asset listed in `SHA256SUMS`, and records rollback evidence. It verifies signed
provenance when `ao2-release-signing-public.pem` and the detached sidecars are
present; otherwise current source builds use the explicit public-checksum mode.

For already-local release assets:

```sh
npm run release:verify-provenance
npm run release:gate
```

The current stable public release line is `v0.5.11`.

To publish the complete private release from a clean checkout, use the guarded
shipper:

```sh
AO2_RELEASE_SHIP_CONFIRM=ship-v0.5.2 \
AO2_UBUNTU_SSH_TARGET=ao2-ubuntu-nucx \
AO2_WINDOWS_SSH_TARGET=win-hp255-via-ubuntu \
npm run release:ship
```

When `AO2_UBUNTU_SSH_TARGET` or `AO2_LINUX_X86_64_SSH_TARGET` is set, the
Linux x86_64 package step builds natively on that Ubuntu host instead of using
local Docker emulation. The produced archive is copied back into
`dist-linux-x86_64/` and still goes through the same provenance, release gate,
download verification, and rollback smoke path.

It runs `npm run verify`, builds and signs all release archives, requires the
strict three-OS smoke with native Ubuntu x86_64 and native Windows, runs the
release gate, creates or updates the private GitHub release, downloads the
published assets, runs native Ubuntu and Windows download verification, and
writes a release doctor JSON report. It then writes and verifies a signed
release comparison bundle before reporting success. The default audit outputs
are `target/release-download/<tag>/release-comparison-result.json`,
`target/release-download/<tag>/release-comparison-verification.json`, and a
bundle under `target/release-comparison-bundles/`; CI can override those paths
with `AO2_RELEASE_COMPARISON_DIR`, `AO2_RELEASE_COMPARISON_RESULT`, and
`AO2_RELEASE_COMPARISON_VERIFICATION`.

`ao2 install update` fails closed unless the signed archive also contains
`RELEASE-VERIFICATION.json`, `SHA256SUMS`, installer scripts, verifier scripts,
manifest, README, VERSION, and packaged binary checksum coverage. Successful
JSON output includes `offline_verification.status = "verified"` before the
binary is copied into the install directory. It also writes
`<binary>.install-verification.json` beside the installed binary with schema
`ao2.install-verification-evidence.v1`; `ao2 doctor --json` reads that sidecar
under `install.verification_evidence`, and release evidence bundles require it
as a checksum-covered artifact with verified offline status and read-only
control-plane trust-boundary fields.

The direct archive installers (`install.sh` and `install.ps1`) also write the
same `<binary>.install-verification.json` sidecar after packaged-binary checksum
verification. Release smoke runs fail if that sidecar is missing or trust-unsafe.

## macOS

Install or update from a verified local archive:

```sh
ao2 install update \
  --archive dist/ao2-0.5.2-macos-aarch64.tar.gz \
  --provenance-dir dist-provenance
ao2 version --json
ao2 doctor --json
ao2 provider matrix --json
```

Default install path:

```text
~/.local/bin/ao2
```

## Ubuntu

Install or update from a verified local archive. Use the x86_64 archive for
normal Intel/AMD Ubuntu hosts:

```sh
ao2 install update \
  --archive dist-linux-x86_64/ao2-0.5.2-linux-x86_64.tar.gz \
  --provenance-dir dist-provenance
ao2 version --json
ao2 doctor --json
ao2 provider matrix --json
```

Use the aarch64 archive for ARM Ubuntu hosts:

```sh
ao2 install update \
  --archive dist-linux/ao2-0.5.2-linux-aarch64.tar.gz \
  --provenance-dir dist-provenance
ao2 version --json
ao2 doctor --json
ao2 provider matrix --json
```

Default install path:

```text
~/.local/bin/ao2
```

## Windows

Install or update from PowerShell:

```powershell
ao2.exe install update `
  --archive dist-windows\ao2-0.5.2-windows-x86_64.tar.gz `
  --provenance-dir dist-provenance
ao2.exe version --json
ao2.exe doctor --json
ao2.exe provider matrix --json
```

Default install path:

```text
%LOCALAPPDATA%\AO2\bin\ao2.exe
```

## Remote Release URL

When release assets are available at a base URL:

```sh
ao2 install update \
  --release-base-url https://github.com/uesugitorachiyo/ao2/releases/download/v0.5.2
```

The updater downloads the target archive, checksum, signature, provenance JSON,
provenance signature, and public key, then verifies the archive before copying
the binary into the install directory.

Private GitHub releases may require `gh release download` first unless the URL
is reachable with the current environment.

## Upgrade Check

Check release metadata before updating:

```sh
ao2 upgrade check \
  --release-url https://api.github.com/repos/uesugitorachiyo/ao2/releases/latest
```

For private repos, download or save release metadata first when direct API
access is not available:

```sh
ao2 upgrade check --release-file release.json
```

The command prints the current version, latest version, update availability,
and release assets in JSON.

## Upgrade Apply

Apply a signed update from release metadata and already-downloaded private
assets:

```sh
ao2 upgrade apply \
  --release-file release.json \
  --asset-dir target/release-download/v0.5.2
```

For a directly reachable release asset base URL:

```sh
ao2 upgrade apply \
  --release-file release.json \
  --release-base-url https://github.com/uesugitorachiyo/ao2/releases/download/v0.5.2
```

`upgrade apply` selects the archive for the current target, copies or downloads
the archive, checksum, detached signature, public key, and provenance files,
then reuses the same signed `ao2 install update` verification path. If an
existing binary is present, rollback remains available through
`ao2 install rollback`.

For private GitHub releases where `gh` is authenticated:

```sh
ao2 upgrade apply \
  --github-release v0.5.2 \
  --repo uesugitorachiyo/ao2
```

## Rollback

`ao2 install update` keeps the previous installed binary beside the active
binary as `<binary>.rollback` when one exists and keeps the latest install
verification sidecar as `<binary>.install-verification.json`. Restore the
rollback binary with:

```sh
ao2 install rollback
ao2 version --json
ao2 doctor --json
ao2 provider matrix --json
```

Use `--install-dir` for non-default installs:

```sh
ao2 install rollback --install-dir /path/to/bin
```

### Windows-safe rollback

On Windows, do not run rollback from the same installed `ao2.exe` that is being
restored. Windows can keep the running executable locked, and AO2 will stop with
`rollback_status=blocked_active_executable` instead of overwriting the active
process.

Use an extracted or alternate `ao2.exe` runner from the verified archive:

```powershell
$Ao2Bin = Join-Path $env:LOCALAPPDATA "AO2\bin"
.\bin\ao2.exe install rollback --install-dir $Ao2Bin --target-label windows-x86_64
& (Join-Path $Ao2Bin "ao2.exe") version --json
& (Join-Path $Ao2Bin "ao2.exe") doctor --json
```

For a custom install directory, replace `$Ao2Bin` with the same directory used
for install or update.

## Uninstall

Remove the active binary, rollback copy, and install-verification sidecar from
the same directory used during installation. For the default Unix location:

```sh
rm -f "$HOME/.local/bin/ao2" \
  "$HOME/.local/bin/ao2.rollback" \
  "$HOME/.local/bin/ao2.install-verification.json"
```

For the default Windows PowerShell location:

```powershell
$Ao2Bin = Join-Path $env:LOCALAPPDATA "AO2\bin"
$Ao2Files = @(
  (Join-Path $Ao2Bin "ao2.exe")
  (Join-Path $Ao2Bin "ao2.exe.rollback")
  (Join-Path $Ao2Bin "ao2.exe.install-verification.json")
)
Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $Ao2Files
if ((Test-Path $Ao2Bin) -and -not (Get-ChildItem -Force $Ao2Bin)) {
  Remove-Item $Ao2Bin
}
```

When `AO2_INSTALL_DIR` was set during installation, apply the same removals in
that directory. Uninstall does not remove per-repository `.ao2/` run evidence,
configuration, or downloaded release assets. Remove retained state separately
only after reviewing it.

## Evidence Cockpit

Generate and open a local evidence cockpit for a completed run:

```sh
ao2 report <run-id> --target /path/to/repo --open
```

The command prints both `report=<path>` and `open_target=<path>` so scripts can
record the generated HTML artifact.

Serve a completed run locally:

```sh
ao2 cockpit serve <run-id> --target /path/to/repo
```

For automated smoke checks, `--port 0 --once` binds an available local port,
prints `url=http://127.0.0.1:<port>/`, serves one request, then exits.

Browse run history from the CLI:

```sh
ao2 runs list --target /path/to/repo --json
ao2 runs show <run-id> --target /path/to/repo --json
```

Generate and serve a local cockpit index for all runs in a repo:

```sh
ao2 cockpit index --target /path/to/repo
ao2 cockpit serve --target /path/to/repo --index
```

## Local Workbench

Generate and open the operator workbench for a repository:

```sh
ao2 workbench export --target /path/to/repo --open
```

The workbench is a local browser screen bundled into the signed CLI. It shows
run history, replay/digest status, provider health, task templates, and
copyable operator commands.

Serve the same workbench over a local HTTP listener:

```sh
ao2 workbench serve --target /path/to/repo --port 8732
```

For automated smoke checks, use `--port 0 --once` to bind an available local
port, serve one request, and exit.

By default `ao2 workbench serve` creates one local admin token and prints it to
stderr as `api_token=<token>`. For multiple local operators, keep one admin
token and add explicit operator tokens:

```sh
ao2 workbench serve \
  --target /path/to/repo \
  --port 8732 \
  --api-token admin-token \
  --operator-token viewer:viewer:viewer-token \
  --operator-token ops:operator:operator-token \
  --enable-execution
```

The `--operator-token` format is `<operator-id>:<role>:<token>`. Valid roles are
`viewer`, `operator`, and `admin`. Viewer tokens can read runs, provider health,
provider readiness, queue status, job details, and queue audit events. Operator
tokens can also launch command previews, start queued runs, cancel/retry jobs,
and export support bundles. Admin tokens currently have operator permissions and
are reserved for future admin-only workbench settings. When more than one
operator is configured, the workbench HTML must be opened with a token, for
example `http://127.0.0.1:8732/?token=viewer-token`; this prevents the browser
page from leaking the admin token to a viewer session.

When served locally, the workbench exposes token-protected API endpoints:

```text
GET  /api/runs?token=<token>
GET  /api/runs/evidence?token=<token>&run_id=<run-id>
GET  /api/runs/evidence/diff?token=<token>&left_run_id=<run-id>&right_run_id=<run-id>
GET  /api/runs/evidence/changes?token=<token>&run_id=<run-id>
GET  /api/support/latest?token=<token>
POST /api/runs/evidence/export?token=<token>
GET  /api/templates?token=<token>
GET  /api/doctor?token=<token>
GET  /api/release-health?token=<token>
GET  /api/release-history?token=<token>
POST /api/release-comparison?token=<token>
GET  /api/release-comparison/verify?token=<token>&bundle_dir=<bundle-dir>
GET  /api/release-comparison/latest?token=<token>&bundle_root=<bundle-root>
POST /api/release-retention/prune?token=<token>
GET  /api/provider-matrix?token=<token>
POST /api/provider-smoke?token=<token>
POST /api/provider-pilot/preflight?token=<token>
POST /api/launch?token=<token>
POST /api/queue/start?token=<token>
POST /api/queue/cancel?token=<token>
POST /api/queue/retry?token=<token>
GET  /api/queue?token=<token>
GET  /api/queue/job?token=<token>&job_id=<job-id>
GET  /api/queue/job/logs?token=<token>&job_id=<job-id>&tail_bytes=<bytes>
GET  /api/queue/audit?token=<token>&action=<action>&job_id=<job-id>
POST /api/queue/export?token=<token>
GET  /queue/job?token=<token>&job_id=<job-id>
```

Read endpoints require at least the `viewer` role. Operator endpoints
(`/api/runs/evidence/export`, `/api/provider-smoke`,
`/api/provider-pilot/preflight`, `/api/launch`, `/api/queue/start`,
`/api/queue/cancel`, `/api/queue/retry`, and `/api/queue/export`) require at
least the `operator` role. A valid viewer token that calls an operator endpoint
receives `insufficient_operator_role`.

The exported and served workbench includes a `Provider Readiness` panel backed
by the same data as `ao2 provider matrix --json`. It shows each provider's
doctor availability, default timeout, sandbox/digest patch boundary, parsed
transcript fields, and policy invariants. `/api/provider-matrix` returns the
same JSON for viewer-or-higher tokens.

Use `ao2 provider contract --provider codex --json` to inspect the Phase 1
Codex adapter boundary before running a live provider smoke or pilot. The
contract report uses schema `ao2.provider-contract.v1` and includes phase,
same-contract-as, doctor output, sandbox/digest execution boundary, side-effect
boundary, live execution guard env, prompt command, transcript fields, policy
invariants, and evidence contract. The Workbench renders the same information
in a read-only `Provider Contracts` table for `scripted`, `codex`, and
`claude`; static exports can inspect it without `--enable-execution`.

Use the same command as a fail-closed CI/operator gate:

```sh
ao2 provider contract --verify --require codex --json
```

Verification returns schema `ao2.provider-contract-verification.v1` with
`status=verified` only when every required provider keeps the expected contract.
For Codex and Claude, the gate requires Phase 1, `same_contract_as=scripted`,
the sandbox/digest execution boundary, exact-digest side-effect boundary, live
guard env, prompt command shape, and core policy/evidence invariants. Unknown
or drifting providers return non-zero with JSON `status=failed` and structured
reasons. The release smoke scripts run this verification through the installed
packaged binary before accepting macOS and Ubuntu archives.

The Workbench also renders `Provider Contract Verification` with the same
fail-closed result. When served locally, `/api/provider-contracts` returns the
same schema for viewer-or-higher tokens so operators can refresh the gate from
the UI without copying commands manually.

The Run Queue includes a `Summary` control for each run. When served locally,
it calls `/api/runs/evidence` with a viewer-or-higher token and returns schema
`ao2.workbench-run-evidence-summary.v1`. The summary is read-only and composes
existing local evidence: replay status, event/artifact counts, digest failure
count, provider scorecard status, parsed provider summaries, closure verdicts,
and cockpit/evidence-pack links. It does not require `--enable-execution` and
never starts provider or queue work.

The Run Evidence Diff control compares two local run summaries through
`/api/runs/evidence/diff` and returns schema
`ao2.workbench-run-evidence-diff.v1`. It reports status/verdict changes,
digest failure delta, provider summary delta, score delta when both scorecards
exist, closure verdict changes, and cockpit/evidence-pack links for both runs.
It is viewer-token protected, read-only, and does not require
`--enable-execution`.

The `Changed Since Previous` control compares a selected run to the previous
local run through `/api/runs/evidence/changes` and returns schema
`ao2.workbench-run-evidence-changes.v1`. It wraps the same diff schema with
selected/baseline run metadata so operators can inspect what changed without
manually choosing both selectors. It is viewer-token protected, read-only, and
does not require `--enable-execution`.

Operators can export the currently selected summary, manual diff, or changed
evidence as a local support handoff artifact from the Workbench. The
`Export Summary`, `Export Diff`, `Export Changes`, and
`Export Verification Evidence` controls post to
`/api/runs/evidence/export`, which writes schema
`ao2.workbench-evidence-export.v1` under
`.ao2/workbench/evidence-exports/`. Evidence export requires an operator token
because it writes a local file, but it does not require `--enable-execution`,
does not invoke providers, and does not mutate queue state.

The launch form also includes `Provider Safety Warnings`. The warnings are
derived from the selected provider's readiness entry and show unavailable
provider blockers, timeout seconds, sandbox/digest patch boundary, and the
direct-write policy invariant before an operator builds a command or starts a
queued run. `/api/launch` and `/api/queue/start` return the same
`provider_warnings` array for API clients and audit tooling.

When served with `--enable-execution`, the workbench also renders a
`Run Provider Smoke` control. It calls token-protected `/api/provider-smoke`,
runs the same deterministic local scripted smoke as
`ao2 provider smoke-all --target . --json`, and appends the result to
`.ao2/provider-smoke/history.json`. The endpoint requires both an operator
token and explicit execution mode; static exports and viewer tokens cannot
start provider smoke.

`/api/launch` is intentionally a command-preview endpoint. It validates the
selected task template and provider, then returns the exact `ao2 run ...`
command for the operator to execute in the local shell.

Browser-triggered run execution is disabled unless the operator explicitly
starts the server with execution mode enabled:

```sh
ao2 workbench serve \
  --target /path/to/repo \
  --port 8732 \
  --enable-execution \
  --queue-retention 100 \
  --support-signing-key /path/to/support-signing-key.pem \
  --support-signer-id workbench-lead
```

When `--enable-execution` is present, `/api/queue/start` enqueues a governed
run and `/api/queue` reports queued/running/accepted/rejected/failed/cancelled
jobs with evidence-pack and cockpit paths. Queue history is persisted to
`.ao2/workbench/queue.json`, so completed and failed job history survives
workbench server restarts. If the server restarts while a job is queued or
running, that stale job is marked `interrupted` in the queue history. The
default queue history retention is 100 jobs; use `--queue-retention <count>` to
keep a smaller or larger local history.

Operators can cancel queued/running jobs with `/api/queue/cancel` and retry
failed/rejected/cancelled/interrupted jobs with `/api/queue/retry`. The
workbench UI renders Open Evidence, Open Cockpit, Logs, Details, Cancel, and
Retry controls when those actions apply. `/api/queue/job/logs` returns a
bounded live tail of stdout/stderr while the job is running and refreshes in
the inline `Logs` panel. `/api/queue/job` returns a single job plus its full
persisted stdout/stderr logs from `.ao2/workbench/jobs/<job-id>/`.
`/queue/job` renders the same job as HTML with stdout/stderr, evidence links,
queue wait, run duration, exit code, retry count, and a normalized failure
diagnosis. Diagnostics include failure kind, timeout detection, exit code,
stderr/stdout excerpts, primary error, and recovery actions such as checking
local provider OAuth, prompt-file paths, or stalled provider CLI state.
`/api/queue` accepts optional `status=<status>` and `template=<template>`
filters.

Queue records include `queued_at_ms`, `started_at_ms`, `finished_at_ms`,
`queue_wait_ms`, `duration_ms`, `exit_code`, and `retry_count`. Operator queue
actions are appended to `.ao2/workbench/audit.jsonl` as JSONL. `/api/queue/audit`
returns those audit events and accepts optional `action` and `job_id` filters.
`/api/queue/export` writes a support bundle under
`.ao2/workbench/support-bundles/` with the queue snapshot, audit events, and
per-job stdout/stderr logs. It also attaches any existing Workbench evidence
exports from `.ao2/workbench/evidence-exports/` as `evidence_exports`, including
each export's path, SHA256, kind, timestamp, and JSON content. When
`--support-signing-key` is provided, each export directory also includes
`support-bundle-metadata.json`,
`support-bundle-metadata.json.sig`, and
`support-bundle-signing-public.pem`; the export response reports
`support_metadata.signature_verified`, signer id, metadata SHA256, signature
SHA256, and public-key SHA256. Signed metadata records the evidence export
count so `ao2 workbench support-verify` can detect attachment-count drift. The
token is still required for every API and HTML detail request.

The workbench also renders a `Latest Support Packet` panel in static exports
and served sessions. It is backed by viewer-token protected
`/api/support/latest`, which verifies the newest local support bundle before
returning schema `ao2.workbench-support-latest.v1` with bundle path, bundle
SHA256, queue/audit/log/evidence counts, signed trust metadata, and attached
evidence export summaries. After an operator exports a new support bundle from
the UI, the panel refreshes so operators can open the newest bundle and inspect
evidence exports without copying CLI commands.

Verify a copied workbench support bundle before relying on it:

```sh
ao2 workbench support-verify \
  --bundle-dir /path/to/support-bundle-<timestamp> \
  --json
```

Inspect a copied workbench support bundle without creating an import case:

```sh
ao2 workbench support-inspect \
  --bundle-dir /path/to/support-bundle-<timestamp> \
  --json
```

Import a verified workbench support bundle into an offline support case:

```sh
ao2 workbench support-import \
  --bundle-dir /path/to/support-bundle-<timestamp> \
  --out-dir workbench-support-cases \
  --json
```

Import verifies the bundle before writing case artifacts, copies the bundle to
`bundle/`, writes `import-summary.json`, and renders `index.html` with support
bundle trust status. `support-verify --json`, `support-inspect --json`, and
`support-import --json` include `evidence_export_count` plus concise summaries
for attached summary/diff/changed-evidence exports. They also include
`queue_job_diagnoses` for failed, cancelled, interrupted, timed-out, or errored
Workbench jobs. Each diagnosis carries the run/job IDs, provider, status,
failure kind, exit code, timeout state, primary error, stdout/stderr excerpts,
and recovery actions. Non-JSON `support-inspect` prints the evidence export
count, per-export run subjects, and one-line queue diagnosis summaries.
Imported support-case HTML renders both a `Queue Failure Diagnostics` table and
an `Evidence Exports` table. Exported and served workbench HTML also shows the
latest local support packet, trust status, queue failure diagnostics, and
evidence export table when a bundle exists under
`.ao2/workbench/support-bundles/`.

## Local Control Plane Snapshot

Generate a read-only snapshot for a future `ao2-control-plane` ingest worker:

```sh
ao2 control-plane ingest --target /path/to/repo --json
```

By default this writes:

```text
.ao2/control-plane/snapshot.json
```

The snapshot derives from existing AO2 artifacts only: run summaries, evidence
pack paths, `.ao2/workbench/queue.json`, `.ao2/workbench/audit.jsonl`, and
`.ao2/provider-smoke/history.json` when provider smoke has run.

Render the snapshot as a local static dashboard:

```sh
ao2 control-plane export --target /path/to/repo --open
```

By default this writes `.ao2/control-plane/index.html` and prints
`control_plane=<path>`.

Serve the same read-only control-plane view over a token-protected local HTTP
listener:

```sh
ao2 control-plane serve \
  --target /path/to/repo \
  --port 8733 \
  --api-token local-control-plane-token
```

Open `http://127.0.0.1:8733/?token=local-control-plane-token`. The API endpoint
`GET /api/control-plane/snapshot?token=<token>` returns the raw snapshot JSON.
The local control-plane dashboard is read-only; it does not start, cancel, retry,
or approve AO2 runs.

Build a local fleet snapshot from multiple repository snapshots:

```sh
ao2 control-plane index \
  --target /path/to/repo-a \
  --target /path/to/repo-b \
  --out /path/to/fleet-snapshot.json \
  --json
```

Regenerate each repository snapshot and write the fleet snapshot in one step:

```sh
ao2 control-plane refresh \
  --target /path/to/repo-a \
  --target /path/to/repo-b \
  --out /path/to/fleet-snapshot.json \
  --json
```

Save reusable fleet sources and refresh from them:

```sh
ao2 control-plane sources save \
  --target /path/to/repo-a \
  --target /path/to/repo-b \
  --out /path/to/fleet-sources.json \
  --json

ao2 control-plane refresh \
  --sources /path/to/fleet-sources.json \
  --out /path/to/fleet-snapshot.json \
  --history /path/to/fleet-history \
  --json
```

Compare, prune, and export retained fleet history:

```sh
ao2 control-plane history diff \
  --history /path/to/fleet-history \
  --json

ao2 control-plane history prune \
  --history /path/to/fleet-history \
  --keep 10 \
  --json

ao2 control-plane history export \
  --history /path/to/fleet-history \
  --out /path/to/fleet-history/index.html \
  --json

ao2 control-plane health \
  --fleet /path/to/fleet-snapshot.json \
  --history /path/to/fleet-history \
  --record /path/to/fleet-health \
  --json

ao2 control-plane health-trend \
  --history /path/to/fleet-health \
  --json

ao2 control-plane health-prune \
  --history /path/to/fleet-health \
  --keep 25 \
  --json

ao2 control-plane health-export \
  --history /path/to/fleet-health \
  --out /path/to/fleet-health/index.html \
  --json
```

The fleet snapshot uses schema `ao2.control-plane-fleet-snapshot.v1` and can be
rendered or served without copying individual repository commands. Fleet HTML
includes local text and status filters for repository and run rows plus a
`Fleet Health` alert panel. `ao2 control-plane health` also returns
`provider_readiness` with schema `ao2.provider-readiness-rollup.v1`; it counts
repositories with ready scripted provider smoke, missing history, and provider
verdict totals. Missing provider smoke history produces a
`provider_smoke_missing` health alert so operators can see which repos still
need the local smoke run. Recorded health checks are stored as local
`health-history.json` timelines and can be exported as a static `AO2 Fleet
Health Trend` dashboard. Passing `--health-history` into fleet export or serve
also embeds the trend in the fleet dashboard and exposes it from the local API:

```sh
ao2 control-plane export \
  --fleet /path/to/fleet-snapshot.json \
  --health-history /path/to/fleet-health \
  --open

ao2 control-plane serve \
  --fleet /path/to/fleet-snapshot.json \
  --health-history /path/to/fleet-health \
  --port 8733 \
  --api-token local-control-plane-token
```

Create a portable fleet support bundle:

```sh
ao2 control-plane bundle \
  --fleet /path/to/fleet-snapshot.json \
  --health-history /path/to/fleet-health \
  --out-dir /path/to/support-bundle \
  --signing-key /path/to/support-signing-key.pem \
  --signer-id support-lead \
  --json
```

The bundle directory contains `fleet-bundle.json`, `fleet-snapshot.json`,
`SHA256SUMS`, and a `.tar.gz` archive for handoff. When `--health-history` is
provided it also includes `health-history.json`, copied health entry files,
`health-trend.json`, and `health-trend.html` in the same checksum manifest.
When `--signing-key` is provided it also includes
`support-bundle-metadata.json`, `support-bundle-metadata.json.sig`, and
`support-bundle-signing-public.pem`; verify, inspect, and import report whether
that metadata signature verifies.

Verify a bundle directory:

```sh
ao2 control-plane bundle-verify \
  --bundle-dir /path/to/support-bundle/fleet-bundle-<timestamp> \
  --json
```

Inspect a transferred archive without creating a support case:

```sh
ao2 control-plane bundle-inspect \
  --archive /path/to/support-bundle/fleet-bundle-<timestamp>.tar.gz \
  --json
```

For an already-extracted bundle, use `--bundle-dir` instead of `--archive`.
The inspect command verifies `SHA256SUMS` and prints repository/run counts,
health history entry count, health trend, and verified file paths.

Import a transferred archive into an offline support case:

```sh
ao2 control-plane bundle-import \
  --archive /path/to/support-bundle/fleet-bundle-<timestamp>.tar.gz \
  --out-dir /path/to/imported-support-cases \
  --json
```

For an already-extracted bundle, use `--bundle-dir` instead of `--archive`.
The import verifies `SHA256SUMS`, writes `import-summary.json`, keeps the
verified bundle files under `bundle/`, and renders a static `index.html`.

## Three-OS Smoke

Run the release smoke across macOS, Ubuntu Docker, Windows archive/static
validation, and the optional native Windows host:

```sh
npm run smoke:three-os
```

For the current no-hosted-CI release-readiness path, use the stricter
macOS-orchestrated command from `docs/LOCAL-SELF-HOSTED-VERIFICATION.md`:

```sh
AO2_WINDOWS_SSH_TARGET=windows \
AO2_REQUIRE_NATIVE_WINDOWS_SMOKE=1 \
scripts/smoke-three-os-release.sh
```

Build all local release assets and sign provenance:

```sh
npm run release:build-all
```

Defaults:

```text
AO2_WINDOWS_SSH_TARGET=antho@10.0.0.96
AO2_WINDOWS_SSH_IDENTITY=~/.ssh/ao_operator_to_windows_ed25519
AO2_REQUIRE_NATIVE_WINDOWS_SMOKE=0
AO2_WINDOWS_SSH_ATTEMPTS=2
AO2_WINDOWS_SSH_CONNECT_TIMEOUT=10
AO2_WINDOWS_WAKE_MAC=
AO2_WINDOWS_WAKE_BROADCAST=10.0.0.255
AO2_WINDOWS_WAKE_WAIT_SECONDS=0
AO2_WINDOWS_WAKE_INTERVAL_SECONDS=10
```

When direct Mac-to-Windows SSH is blocked but the Ubuntu lab host can reach the
Windows host, use the checked local SSH alias:

```sh
AO2_WINDOWS_SSH_TARGET=win-hp255-via-ubuntu npm run smoke:three-os
```

By default, `npm run smoke:three-os` records native Windows execution as
`windows_native_smoke=passed` when the SSH host is reachable, or
`windows_native_smoke=skipped` when the SSH identity or host is unavailable.
Mac install smoke, Ubuntu Docker install smoke, release provenance, and Windows
archive/static validation must still pass. Set
`AO2_REQUIRE_NATIVE_WINDOWS_SMOKE=1` for a strict gate that fails the command
when the native Windows host is unavailable or the Windows install smoke fails.
Each run writes both `report.md` and `summary.json`; the JSON summary uses
schema `ao2.three-os-smoke-summary.v1` and records local smoke status, native
Windows requirement mode, Windows native status, skip reason, wake hosts, and
SSH probe counts. The smoke script also validates that summary through:

```sh
ao2 release smoke-summary --summary target/three-os-smoke/<run>/summary.json
```

Add `--require-native-windows` to make the verifier fail closed unless the
summary records `windows_native_smoke=passed`. If the summary points at a
Windows smoke log, the verifier also scans that log for hard package and
PowerShell verifier failures, preventing a false pass when the outer script
printed `windows_native_smoke=passed` after an inner Windows error.

To rerun the combined release gate without rerunning the full three-OS smoke,
use the standalone wrapper. By default it uses the newest
`target/three-os-smoke/*/summary.json`:

```sh
npm run release:gate
```

Check release asset availability and provenance from a downloaded private
release directory. When `release-rollback-summary.json` exists in that
directory, the JSON output also includes Mac, Ubuntu, and Windows rollback
health under `release.rollback`:

```sh
ao2 doctor --json \
  --release v0.5.2 \
  --release-asset-dir target/release-download/v0.5.2 \
  --provenance-dir target/release-download/v0.5.2
```

Run native Windows verification against downloaded private release assets:

```sh
AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY=1 \
AO2_WINDOWS_SSH_TARGET=win-hp255-via-ubuntu \
npm run release:download-verify
```

The Workbench also exposes a read-only `Release Health` panel. When served with
an API token, its `/api/release-health` endpoint runs the same release-aware
doctor checks for asset availability, signed provenance, provenance tag
matching, installed binary state, and downloaded rollback evidence when present.
The panel includes editable release, asset directory, and provenance directory
fields, then renders Mac, Ubuntu x86_64, and Windows rollback status cards with
links to the recorded evidence paths.
It also includes Release History controls backed by `/api/release-history` so
operators can scan `target/release-download`, compare recent releases, and open
doctor or rollback evidence for each version. The history response includes a
trend summary, per-release health scores, changed-field lists, and regression
markers. Operators can export the current Release History view as a Workbench
evidence export; the next support-bundle export attaches that file and signs it
with the existing Workbench support metadata path.

When Workbench is served with an operator token and `--support-signing-key`, the
Release History panel can also generate signed release comparison bundles through
`POST /api/release-comparison` and verify existing bundles through
`GET /api/release-comparison/verify`. Generation is intentionally server-keyed:
the private key never moves to the browser, and the response includes the same
`ao2.release-comparison-verification.v1` report produced by the CLI verifier.
Viewer tokens can also call `GET /api/release-comparison/latest` to scan a
bundle root, choose the newest bundle that verifies, and return schema
`ao2.workbench-latest-release-comparison.v1` with the verified bundle path and
comparison verification report.
Operators can attach that verification report to the next signed Workbench
support bundle with `kind=release-comparison-verification&bundle_dir=<bundle-dir>`
on `/api/runs/evidence/export`. The export verifies the bundle first and rejects
unverified comparison artifacts.

Build a release-line control-plane support bundle from explicit evidence
surfaces. The build command can generate the required report-contract
verification from the static report and index, then verify the generated bundle
and checksum manifest:

```sh
ao2 release support-bundle-build \
  --release-assembly /path/to/release-assembly.json \
  --readiness /path/to/readiness.json \
  --handoff /path/to/handoff.json \
  --cockpit /path/to/cockpit.json \
  --evaluator-decision /path/to/evaluator-decision.json \
  --storage-support /path/to/storage-support.json \
  --replay /path/to/replay.json \
  --report-target /path/to/ao2-target \
  --report-run-id <run-id> \
  --report /path/to/report/index.html \
  --report-index /path/to/report/index.json \
  --install-verification /path/to/install-verification.json \
  --hosted-release-smoke /path/to/hosted-release-smoke.json \
  --operator-evidence /path/to/operator-evidence.json \
  --out-dir /path/to/release-support-bundle \
  --json

ao2 release support-bundle-verify \
  --bundle /path/to/release-support-bundle/release-support-bundle.json \
  --checksums /path/to/release-support-bundle/SHA256SUMS \
  --json
```

Use `--report-contract-verification /path/to/report-contract-verification.json`
instead of the report inputs only when the verification JSON was already
generated by `ao2 report verify`.

The build command writes `release-support-bundle.json` and `SHA256SUMS`, then
runs the same strict verifier before reporting success. The bundle also includes
the public `portable_bundle_manifest`, `ci_evidence_index`, canonical per-surface
digests, hosted release archive smoke evidence, and a canonical bundle digest in
`SHA256SUMS` so the ao2-control-plane offline verifier can check the same
handoff. Verification fails closed if replay was not accepted, digest failures
are present, report contract verification is missing or failed, operator
evidence is missing, hosted release archive smoke evidence is missing or
trust-unsafe, candidate-correlation triage is absent from the release assembly,
readiness,
handoff, or cockpit surfaces, the factory-v3 evaluator-closer is not the release
acceptance owner, or the control plane appears as a release approver instead of
a read-only observer.

The Workbench signed evidence publish form can also send a real run-derived
operator packet to ao2-control-plane. Choose `Operator Packet` in the form, or
post `kind=operator-packet&run_id=<run-id>&control_plane_url=<url>&api_token=<token>`
to `/api/runs/evidence/publish`. The server signs the generated
`ao2.operator-evidence-packet.v1` with `--support-signing-key` and posts it to
the read-only `/api/v1/operator-packet/signed` control-plane endpoint.
For a local end-to-end proof, run
`npm run smoke:workbench-operator-packet-control-plane`; during development you
can set `AO2_WORKBENCH_OPERATOR_PACKET_CP_PROFILE=debug` to use debug binaries.
The smoke stores its ignored evidence under
`target/workbench-operator-packet-control-plane-smoke/` and emits
`ao2.workbench-operator-packet-control-plane-smoke.v1`.

The Release Evidence Retention controls call
`POST /api/release-retention/prune` with `dry_run=1` for preview and
`dry_run=0` for prune. The operator-token endpoint keeps the newest matching
`v*` release-download directories and newest `release-comparison-*` bundle
directories, returning schema `ao2.workbench-release-retention-prune.v1` with
kept and removed paths. It never deletes arbitrary files outside those matched
directory patterns.

Release ship runs the same retention policy locally before the expensive
Mac/Ubuntu/Windows packaging and smoke stages:

```sh
npm run release:retention-preflight
```

By default the preflight keeps the newest three `target/release-download/v*`
directories and newest three `target/release-comparison-bundles/release-comparison-*`
directories, then prints `release_retention_preflight=passed`. Tune the keep
counts with `AO2_RELEASE_RETENTION_KEEP_RELEASES` and
`AO2_RELEASE_RETENTION_KEEP_BUNDLES`. Set `AO2_RELEASE_RETENTION_PRUNE=0` for a
dry run that reports what would be pruned without deleting generated release
evidence.

Release ship also runs `npm run smoke:workbench-release-comparison-export`
after signed release comparison verification. The smoke starts a token-protected
Workbench server with a temporary support-signing key, posts
`kind=release-comparison-verification` to `/api/runs/evidence/export`, exports a
redaction preview through `/api/queue/export-preview`, exports a signed support
bundle through `/api/queue/export`, and verifies the attachment with
`ao2 workbench support-inspect`. The preview uses schema
`ao2.workbench-support-bundle-preview.v1`, includes
`ao2.workbench-support-redaction-preview.v1`, and reports `would_write_bundle`
as `false` so operators can inspect redaction coverage before writing a bundle.
The same redaction policy is applied when `/api/queue/export` writes the support
bundle body. It masks common captured support-log secret shapes, including
provider API keys, Twilio auth tokens, Supabase service-role keys, AI intake
keys, bearer authorization headers, API-key headers, password fields, and URL
query-string secrets such as `token`, `access_token`, `api_key`, and
`signature`. Exported bundles now include
`ao2.workbench-support-redaction-audit.v1` with total redaction count,
per-secret-class counts, redacted field paths, run IDs, and redacted excerpts.
`ao2 workbench support-inspect` and `ao2 workbench support-import` preserve the
same audit so operators can review redaction coverage after handoff without
opening raw logs.
The resulting artifacts are written under
`target/release-download/<tag>/workbench-release-comparison-export-smoke/`.
The temporary support-signing private key is removed during smoke cleanup; the
retained artifacts include only the export JSON, support preview JSON, support
bundle export JSON, inspect JSON, and Workbench logs.

Provider-pilot acceptance bundles can be attached to the same signed Workbench
support handoff. Post
`kind=provider-pilot-acceptance&acceptance_bundle=<provider-pilot-acceptance.json>`
to `/api/runs/evidence/export`; AO2 verifies the bundle uses
`ao2.codex-provider-pilot-acceptance.v1`,
`ao2.claude-provider-pilot-acceptance.v1`, or
`ao2.antigravity-provider-pilot-acceptance.v1`, has `status=passed`, replay
`accepted`, zero digest failures, and a ready score before writing the evidence
export. The next `/api/queue/export` support bundle includes provider, run id,
score, replay status, digest-failure count, evidence-pack path, and cockpit
path in its signed support summaries.

For scripted verification:

```sh
AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE=target/codex-provider-pilot/<timestamp>/provider-pilot-acceptance.json \
npm run smoke:workbench-provider-pilot-acceptance-export
```

The smoke calls both the direct `kind=provider-pilot-acceptance` evidence export
and `/api/provider-pilot/acceptance/export-latest`, loads
`/api/provider-pilot/cost-ledger` and `/api/provider-pilot/cost-trend`,
previews support-bundle redaction through `/api/queue/export-preview`, then
verifies the signed support bundle contains two provider-pilot acceptance
evidence exports.

The Provider Pilot panel can also filter the latest acceptance lookup before
loading or exporting evidence. Operator sessions can filter by provider, replay
status, minimum score, sort order, and history limit; those controls are passed
to both `/api/provider-pilot/acceptance/latest` and
`/api/provider-pilot/acceptance/export-latest`. The JSON response includes
`acceptance_filter`, `history_total_count`, and the limited
`acceptance_history` rows so the UI shows exactly which bundles qualified.
The same response includes `acceptance_trend`, which compares the newest
matching bundle against the previous matching bundle and reports score delta,
regression status, best/worst score, accepted count, and ready count. The
Workbench output renders that trend line before the history table.

Operators can also load a budget and usage ledger for retained provider-pilot
acceptance bundles:

```sh
ao2 provider cost-ledger --acceptance-root target/provider-pilot-acceptance --json
ao2 provider cost-trend --acceptance-root target/provider-pilot-acceptance --json
```

The ledger recursively scans `provider-pilot-acceptance.json` files, verifies
each accepted bundle, reads each evidence pack's `provider_summaries`, and
returns schema `ao2.provider-cost-ledger.v1` with total configured budget,
observed provider cost where available, token totals, and per-provider budget
enforcement counts. The Workbench `Load Cost Ledger` control calls the same
contract at `/api/provider-pilot/cost-ledger`.

The trend command returns schema `ao2.provider-cost-trend.v1`. It groups the
same verified ledger entries by release tag, reports per-release budget,
observed cost, token totals, provider rollups, and latest-vs-previous deltas.
The Workbench `Load Cost Trend` control calls the same contract at
`/api/provider-pilot/cost-trend` and renders an accessible SVG chart comparing
configured budget against observed provider cost across the latest retained
releases, with the table preserved as the detailed fallback.

When `AO2_RELEASE_CODEX_PILOT_ACCEPTANCE=1` is enabled during release ship, the
release flow also runs this Workbench export smoke against the just-created
Codex provider-pilot acceptance bundle and stores artifacts under
`target/release-download/<tag>/workbench-provider-pilot-acceptance-export-smoke/`.

For a portable signed release comparison bundle outside the Workbench, run:

```sh
ao2 release compare \
  --release-download-dir target/release-download \
  --out-dir target/release-comparison-bundles \
  --signing-key .release-signing/ao2-release-signing-key.pem \
  --signer-id release-lead \
  --json
```

The command writes `release-comparison.json` with schema
`ao2.release-comparison-bundle.v1`, `release-history.json`, `SHA256SUMS`, and,
when a signing key is provided, signed
`ao2.release-comparison-metadata.v1` metadata plus a public key. The bundle
records the same Release History trend fields used by Workbench, so offline
reviewers can verify the latest private release, health score, regression
count, and supporting evidence paths from one directory.

Verify a signed comparison bundle before attaching it to an operational handoff:

```sh
ao2 release compare-verify \
  --bundle-dir target/release-comparison-bundles/release-comparison-<timestamp> \
  --json
```

The verifier reads `release-comparison.json`, `release-history.json`,
`SHA256SUMS`, signed metadata, signature, and public key from the bundle
directory. It returns schema `ao2.release-comparison-verification.v1` with
`status=verified` only when the manifest hashes, metadata signature, and
metadata trend fields all match the bundle contents.

Use the combined release gate when the release provenance, all three archive
signatures, and the three-OS smoke summary must pass together:

```sh
ao2 release gate \
  --summary target/three-os-smoke/<run>/summary.json \
  --provenance-dir dist-provenance \
  --macos-archive dist-macos/ao2-<version>-macos-aarch64.tar.gz \
  --linux-archive dist-linux/ao2-<version>-linux-x86_64.tar.gz \
  --windows-archive dist-windows/ao2-<version>-windows-x86_64.tar.gz \
  --require-native-windows
```

## Provider Fast Start

Initialize local AO2 state and provider presets:

```sh
ao2 init --target .
ao2 provider list
ao2 provider doctor --provider scripted
```

Run a real-project template without locating a YAML file:

```sh
ao2 run --template bug-fix \
  --target . \
  --provider scripted \
  --provider-prompt-file ./repair.sh
```

For Rust crate bug fixes, choose the Cargo-aware template so
the verifier evidence is `cargo test`:

```sh
ao2 run examples/task-templates/rust-cargo-bug-fix.yaml \
  --target /path/to/rust-crate \
  --provider scripted \
  --provider-prompt-file ./repair.sh
```

That path works with the published `v0.5.2` binary when run from an AO2
checkout at commit `a1e82b0adb723dd5ae2be6d93355ffdc2caa549d` or newer.
Binaries built from that commit or newer can also use the embedded-template
shortcut:

```sh
ao2 run --template rust-cargo-bug-fix \
  --target /path/to/rust-crate \
  --provider scripted \
  --provider-prompt-file ./repair.sh
```

The default `bug-fix` template verifies with `python -m pytest`. Do not use it
for Rust runs unless the project really is driven by pytest. This guidance
only selects the workflow template and verifier; it does not require a new
binary release, tag, upload, deployment, or publication step.

Historical beta evidence for the v0.5.2 release train is indexed in
[`docs/beta/v0.5.0-beta.1-canary-closeout.md`](beta/v0.5.0-beta.1-canary-closeout.md).

For live providers, replace `scripted` with `codex` or `claude` after local CLI
OAuth login is working.

Run the default-safe Codex provider smoke profile:

```sh
npm run smoke:provider:codex
```

By default this prints `codex_provider_smoke=skipped` and does not call the
model. After local Codex CLI OAuth login is ready, explicitly enable the live
smoke:

```sh
AO2_LIVE_CODEX_SMOKE=1 npm run smoke:provider:codex
```

The smoke script unsets `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`, checks
`ao2 adapter doctor --provider codex`, runs Codex only through AO2's sandboxed
provider smoke path, and prints the smoke report and history paths.

Run the default-safe Claude Code provider smoke profile:

```sh
npm run smoke:provider:claude
```

By default this prints `claude_provider_smoke=skipped` and does not call the
model. After local Claude Code CLI OAuth login is ready, explicitly enable the
live smoke:

```sh
AO2_LIVE_CLAUDE_SMOKE=1 npm run smoke:provider:claude
```

The smoke script unsets `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`, checks
`ao2 adapter doctor --provider claude`, runs Claude only through AO2's
sandboxed provider smoke path, and prints the smoke report and history paths.

Score provider evidence quality for an existing run:

```sh
ao2 provider score --target . --run-id <run-id> --json
```

The scorecard returns schema `ao2.provider-evidence-scorecard.v1`, a 0-100
score, and a verdict:

- `ready`: score is 90 or higher and evidence is suitable for provider pilot
  review.
- `warn`: score is 70-89 and an operator should inspect the failed or partial
  dimensions before relying on the run.
- `fail`: score is below 70 and the provider evidence is not pilot-ready.

Dimensions cover replay integrity, parsed provider summaries, changed-file
evidence, blocker hygiene, and sandbox/policy boundary evidence.

Run the aggregate local provider smoke before promoting a provider pilot:

```sh
ao2 provider smoke-all --target . --json
```

`smoke-all` runs a deterministic scripted provider repair in a disposable
`.ao2/provider-smoke/` repository and reports Codex/Claude doctor status without
making live model calls. The output uses schema `ao2.provider-smoke-all.v1` and
includes a scorecard for the scripted smoke. Each run is also appended to
`.ao2/provider-smoke/history.json` with schema
`ao2.provider-smoke-history.v1`; the command output includes `history_path` and
`history_entry_count`.

To bring Codex into the same readiness loop, opt in twice: pass
`--live-provider codex` and set `AO2_LIVE_CODEX_SMOKE=1`. Without the
environment gate, Codex reports `verdict: guarded` and AO2 does not call the
provider.

```sh
AO2_LIVE_CODEX_SMOKE=1 \
  ao2 provider smoke-all --target . --live-provider codex --json
```

The Codex run uses the same sandbox copy, digest patch promotion, evidence
pack, provider scorecard, smoke history, and control-plane readiness rollup as
the scripted provider. The workbench `Run Provider Smoke` panel exposes the
same Codex option when served with `--enable-execution`; it still requires an
operator token and the server process must have `AO2_LIVE_CODEX_SMOKE=1`.

Claude uses the same readiness loop and guard model. Pass
`--live-provider claude` and set `AO2_LIVE_CLAUDE_SMOKE=1`; without the
environment gate, Claude reports `verdict: guarded` and AO2 does not call the
provider.

```sh
AO2_LIVE_CLAUDE_SMOKE=1 \
  ao2 provider smoke-all --target . --live-provider claude --json
```

The workbench `Run Provider Smoke` panel exposes the same Claude option when
served with `--enable-execution`; it still requires an operator token and the
server process must have `AO2_LIVE_CLAUDE_SMOKE=1`.

Before starting a provider pilot, run the fail-closed readiness gate against
the latest provider smoke history:

```sh
ao2 provider gate --target . --json
```

By default the gate requires the scripted provider smoke to be ready with a
score of at least 90. To require live provider proof as well, pass each provider
explicitly:

```sh
ao2 provider gate --target . --require codex --require claude --json
```

The gate reads `.ao2/provider-smoke/history.json` only. It does not call Codex,
Claude, or any model. Missing history, missing required providers, guarded
providers, non-ready verdicts, and scores below `--minimum-score` return
`verdict: not_ready` and a non-zero exit.

After the gate passes, prepare a real-provider pilot command without executing
it:

```sh
ao2 provider pilot --target . --provider codex --provider-prompt-file ./pilot-prompt.txt --json
ao2 provider pilot --target . --provider claude --template test-generation --provider-prompt-file ./pilot-prompt.txt --json
```

`provider pilot` requires a ready gate for the requested provider. It
materializes the selected workflow template under `.ao2/generated-workflows/`
and returns schema `ao2.provider-pilot-plan.v1` with a copyable
`ao2 run --template ... --provider ... --provider-prompt-file ...` command. It
does not call Codex, Claude, or run the workflow. Blocked gates return
`status: blocked`, embed the readiness gate report, and exit non-zero.

The Workbench exposes the same provider pilot contract in the `Provider Pilot`
panel:

```sh
ao2 workbench serve --target . --api-token <token>
```

An operator token can build Codex/Claude pilot previews from the browser. The
Workbench endpoint is `/api/provider-pilot`, returns the same
`ao2.provider-pilot-plan.v1` schema as the CLI, reads provider smoke history
only, and does not require `--enable-execution` because it does not start a run
or call a provider.

Before building or starting a provider pilot, operators can run
`/api/provider-pilot/preflight` from the `Preflight Provider Pilot` control.
Preflight validates provider, template, and prompt file inputs locally, then
reads the same provider readiness gate. It returns
`ao2.workbench-provider-pilot-preflight.v1` with per-check status,
`can_start`, and the embedded `ao2.provider-pilot-plan.v1` report when local
inputs are valid. It does not require `--enable-execution` and never invokes a
provider or queues work.

To start a ready provider pilot from the browser, serve the Workbench with
execution enabled:

```sh
ao2 workbench serve --target . --enable-execution --api-token <token>
```

The `Start Provider Pilot` control posts to `/api/provider-pilot/start`. It is
operator-only, requires `--enable-execution`, rechecks the same provider
readiness gate, and enqueues the run in the persistent Workbench queue. Blocked
provider gates return the same `ao2.provider-pilot-plan.v1` blocked report and
do not create queue jobs.

The same panel includes an `Acceptance Bundle` field and `Export Acceptance
Evidence` control. Use `Load Latest Acceptance` to ask
`/api/provider-pilot/acceptance/latest` for the newest valid Codex, Claude, or
Antigravity `provider-pilot-acceptance.json` under
`target/provider-pilot-acceptance`.
The response includes `acceptance_history` for the verified bundles checked in
newest-first order. Operators can paste a bundle path manually and click
`Export Acceptance Evidence`, or click `Export Latest Acceptance` to have the
Workbench load and export the newest verified bundle in one guarded operator
action. Workbench posts `kind=provider-pilot-acceptance` to
`/api/runs/evidence/export`, refreshes the latest support packet, and links the
export, evidence pack, and cockpit without requiring operators to copy curl
commands.

For a release-grade Phase 1 Codex acceptance run, use the guarded live provider
pilot smoke:

```sh
AO2_LIVE_CODEX_PILOT=1 npm run smoke:provider:codex-pilot
```

The script requires local Codex CLI OAuth, unsets `OPENAI_API_KEY` and
`ANTHROPIC_API_KEY` around provider execution, runs live Codex smoke, builds a
provider pilot plan, executes the Risky PR Run through the Codex adapter, runs
replay and provider scoring, verifies `python3 -m pytest`, and writes schema
`ao2.codex-provider-pilot-acceptance.v1` under
`target/codex-provider-pilot/<timestamp>/provider-pilot-acceptance.json`.

To make that Codex acceptance bundle an explicit private release gate, opt in
when running release ship:

```sh
AO2_RELEASE_CODEX_PILOT_ACCEPTANCE=1 \
AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD=1.00 \
AO2_RELEASE_SHIP_CONFIRM=ship-v0.5.2 \
npm run release:ship
```

Release ship keeps this gate off by default so publishing does not accidentally
invoke a live provider. When enabled, it runs the acceptance smoke with
`AO2_LIVE_CODEX_PILOT=1`, uses the release binary from `target/release/ao2`,
and writes the bundle under
`target/provider-pilot-acceptance/<tag>/`. The acceptance bundle records the
configured `AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD`, timeout, and repair
budget. Codex CLI does not currently expose a direct max-budget flag in
`codex exec`, so AO2 records the configured cap and bounds exposure with the
provider timeout and single repair attempt.

Before pruning `target/`, preserve the live Codex, Claude, and Antigravity
acceptance bundles as durable release evidence:

```sh
npm run release:preserve-provider-acceptance
```

The preservation step validates all three bundles are passed, live-source
evidence, score at least 90, replay accepted, and have zero digest failures. It
copies the bundles to `target/release-evidence/provider-pilot-acceptance/<tag>/`
and writes `summary.json` with schema
`ao2.provider-pilot-acceptance-preservation.v1`.

Claude has the same explicit acceptance-smoke shape:

```sh
AO2_LIVE_CLAUDE_PILOT=1 npm run smoke:provider:claude-pilot
```

The Claude pilot writes schema `ao2.claude-provider-pilot-acceptance.v1` under
`target/claude-provider-pilot/<timestamp>/provider-pilot-acceptance.json`.
It is also guarded by local Claude Code CLI OAuth and unsets provider API-key
environment variables around live provider calls. Claude is invoked only inside
AO2's disposable sandbox copy with Bash/Read/Write/Edit tools available; AO2
still requires its own patch preview, digest replay, and approval boundary
before any sandbox changes can be applied back to the target repository.

To make the Claude acceptance bundle an explicit private release gate, opt in
with the matching release flag:

```sh
AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE=1 \
AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD=1.00 \
AO2_RELEASE_SHIP_CONFIRM=ship-v0.5.2 \
npm run release:ship
```

Release ship keeps this gate off by default for the same reason as Codex: it
must never invoke a paid or locally authenticated provider by accident. When
enabled, it runs the acceptance smoke with `AO2_LIVE_CLAUDE_PILOT=1`, uses the
release binary from `target/release/ao2`, writes the acceptance bundle under
`target/provider-pilot-acceptance/<tag>/claude/`, and verifies that bundle can
be exported through the Workbench support-evidence path. Claude Code receives
the configured budget as `--max-budget-usd` for every provider-backed execution
in the smoke.

Antigravity has the same explicit acceptance-smoke shape:

```sh
AO2_LIVE_ANTIGRAVITY_PILOT=1 npm run smoke:provider:antigravity-pilot
```

The Antigravity pilot writes schema
`ao2.antigravity-provider-pilot-acceptance.v1` under
`target/antigravity-provider-pilot/<timestamp>/provider-pilot-acceptance.json`.
It is guarded by local Antigravity CLI OAuth through `agy`, unsets provider
API-key environment variables around live provider calls, and runs only inside
AO2's disposable sandbox copy before AO2 replay and scoring close the bundle.

To make the Antigravity acceptance bundle an explicit private release gate, opt
in with the matching release flag:

```sh
AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE=1 \
AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD=1.00 \
AO2_RELEASE_SHIP_CONFIRM=ship-v0.5.2 \
npm run release:ship
```

Release ship keeps this gate off by default. When enabled, it runs the
acceptance smoke with `AO2_LIVE_ANTIGRAVITY_PILOT=1`, uses the release binary
from `target/release/ao2`, writes the acceptance bundle under
`target/provider-pilot-acceptance/<tag>/antigravity/`, and verifies that bundle
can be exported through the Workbench support-evidence path. Antigravity CLI
does not currently expose a direct max-budget flag in the current AO2 adapter,
so AO2 records the configured cap and bounds exposure with timeout and repair
budget.

Workbench launch and queue forms accept `minimum_score`. When this value is set,
AO2 checks the named run's provider scorecard before building a launch preview
or enqueuing execution:

```sh
ao2 workbench serve --target . --enable-execution --api-token <token>
```

If the score is missing or lower than the threshold, the API fails closed with
`minimum_provider_score_not_met`.
