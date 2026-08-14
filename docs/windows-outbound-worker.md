# AO2 Windows Outbound Worker

The Windows worker polls the Mac-hosted AO2 Control Plane outbound and posts
task-board result evidence back to `/api/v1/ai/task-board`. It does not open an
inbound Windows HTTP endpoint.

Run the worker directly for a foreground smoke:

```powershell
python C:\ao\factory\ao2\scripts\ao2_windows_outbound_worker.py `
  --control-plane-url http://10.0.0.160:18745 `
  --api-token-file C:\ao\secrets\ao2-cp-api-token.txt `
  --factory-root C:\ao\factory `
  --node-id windows-hp255_g10
```

Install persistence locally on Windows:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass `
  -File C:\ao\factory\ao2\scripts\Install-AO2WindowsOutboundWorker.ps1 `
  -ControlPlaneUrl http://10.0.0.160:18745 `
  -ApiTokenFile C:\ao\secrets\ao2-cp-api-token.txt `
  -FactoryRoot C:\ao\factory `
  -NodeId windows-hp255_g10
```

The task board allowlist is explicit: `status`, `publish_capability`,
`sync_ao_stack`, `ao2_doctor`, `timeout_fixture`, and
`windows_stack_qualification`. The worker rejects arbitrary command text and
stores a local idempotency ledger under
`%LOCALAPPDATA%\AO2\windows-outbound-worker`.

`status` and `publish_capability` are read-only observer probes and may remain
unsigned. Every command-executing action requires an
`ao2.cross-host.execution-authorization.v1` envelope signed with the AO2
release key. The worker verifies the RSA/SHA-256 signature, pins the public key
to the published AO2 release-key SHA-256, binds the signature to the canonical
action digest, node, and request identifier, and rejects authorizations older
than their maximum 15-minute window. The Control Plane bearer authenticates
storage access only and is not worker-execution authority.

Create the unsigned task board without embedding a credential, then authorize
it on the initiating host immediately before upload:

```bash
mkdir -p target/windows-control
openssl pkey \
  -in .release-signing/ao2-release-signing-key.pem \
  -pubout \
  -out target/windows-control/ao2-release-signing-public.pem
test "$(shasum -a 256 target/windows-control/ao2-release-signing-public.pem | awk '{print $1}')" = \
  "7fedf62781b08a50abff300425f47c79b72f76f7208024a951d0533ebdb8f28c"
python3 scripts/authorize_windows_control_task.py \
  --input target/windows-control/task-board.json \
  --output target/windows-control/task-board.authorized.json \
  --private-key .release-signing/ao2-release-signing-key.pem \
  --public-key target/windows-control/ao2-release-signing-public.pem \
  --ttl-seconds 300
```

Upload only `task-board.authorized.json`. The private key is read locally by
the signer and is never included in the board, logs, worker state, or Control
Plane storage. Altering action parameters after signing invalidates the action
digest. An unsigned, expired, wrong-node, untrusted-key, or replayed mutation
fails before the worker claims the request.

Completed action results are written atomically to a local
`result-outbox\` directory under the same state root before the worker attempts
to publish them to the Mac Control Plane. If result publication fails because
the Control Plane or network path is temporarily unavailable, the worker leaves
the original sanitized result in that outbox and retries publication on later
polls. It does not replace a completed action result with `worker_exception`
merely because the HTTP result post failed.

`ao2_doctor` first runs an installed `ao2` binary from PATH or the standard
local AO2 install directory. If no installed binary exists during
pre-publication qualification, it runs the fixed repository-owned fallback:
`cargo run --manifest-path C:\ao\factory\ao2\Cargo.toml --target-dir C:\ao\factory\.ao2-worker-target\ao2-doctor -p ao2-cli --bin ao2 -- doctor --json`.
The fallback uses the fixed target directory so it never rebuilds or removes
the shared repository `target\debug\ao2.exe` while another Windows check holds
that executable open.
The task payload does not provide command text for that fallback.

`windows_stack_qualification` is a fixed native-Windows verification action.
Its payload may only select:

- `mode`: `diagnostic`, `targeted`, `full`, `physical_bounded`, legacy
  `physical_unique`, or `toolchain`
- `repositories` or `repos`: names from the canonical AO stack inventory
- `timeout_seconds`: a bounded value from 30 through 3600 seconds

`physical_bounded` and legacy `physical_unique` additionally require
`physical_host_lease_base64` and `physical_host_lease_sha256`. The decoded
strict JSON contract is
`ao2.physical-host-exclusive-lease.v1` or
`ao2.physical-host-exclusive-lease.v2`, is limited to 16 KiB, rejects duplicate
or unknown keys, and binds the node, operator approval record, purpose, issuance,
expiry, heartbeat, exclusive-use preflight, command profile, unique scratch
root, and cleanup root. Version 1 requires zero active interactive sessions.
Version 2 accepts either zero sessions with `interactive_session_state=none`,
or one locked session with `interactive_session_state=locked`; both require
`interactive_ao_workloads_active=0`. Unlocked, unknown, multiple, or busy
sessions fail closed. Every version rejects overlap, abort, release, broad
process termination, and graphical-session mutation. The lease lasts at most
15 minutes and its heartbeat may be at most two minutes old. Its digest, ID,
and scratch root are copied into the sanitized qualification result. A missing,
altered, stale, unsafe, or inapplicable lease fails before any child command
runs.

The lease supplements the signed execution authorization; it does not replace
it or grant release, deployment, publication, provider, credential, arbitrary
command, session-management, or cleanup authority.

New release qualification and fixed lifecycle checks use the separate strict
`ao2.physical-host-bounded-lease.v1` contract. Its
`isolation_mode=bounded_shared` permits non-negative counts of interactive
sessions, unrelated AO workloads, and SSH connections. Multiple SSH
connections do not create a conflict. The lease is accepted only when its
conflicting lease, workload, and scratch lists are empty and
`resource_limits_satisfied=true`. It retains the same digest, freshness,
approval, unique scratch, cleanup, natural-completion, no-broad-kill, and
no-graphical-session-mutation boundaries. `physical_bounded` accepts this
contract for release qualification; concrete lease, workload, scratch, or
resource conflicts still fail closed. Legacy `physical_unique` remains
available for preserved historical evidence. Neither mode authorizes
host-global work.

The same parser can validate a regular non-symlink lease file offline on any
host without starting the worker or contacting the Control Plane:

```text
python scripts/ao2_windows_outbound_worker.py \
  --validate-physical-host-lease /bounded/scratch/lease.json \
  --physical-host-lease-sha256 <sha256> \
  --physical-host-lease-profile ubuntu_stack_qualification:lifecycle_noop \
  --node-id <physical-node-id> \
  --factory-root /bounded/factory
```

The profile selector accepts the fixed Windows physical qualification plus
Ubuntu and Windows no-op lifecycle profiles. The worker action always uses the
exclusive Windows profile; lifecycle profiles are available only to offline
fixed wrappers.

The payload must not provide command text, PowerShell text, executable paths,
working directories, shell fragments, or environment variables. Repository
names are resolved only beneath `C:\ao\factory`; traversal, separators,
absolute paths, drive-letter paths, duplicates, unknown repositories, and the
archived `agy-swarms` reference implementation are rejected before execution.

The canonical inventory is machine-readable at
`docs/windows-stack-qualification-inventory.json`. The reviewed
repository-to-command profiles live in
`scripts/ao2_windows_outbound_worker.py::WINDOWS_REPOSITORY_PROFILES`.
AO Next uses fixed Cargo workspace test and release-build commands for physical
Windows qualification; task payloads cannot replace those commands.
The AO2 full profile runs Cargo build/test gates with
`--target-dir C:\ao\factory\.ao2-worker-target\ao2-full`, avoiding the shared
repository `target\debug\ao2.exe` when that executable is locked by another
Windows check or installed AO2 process. The same profile also runs
`npm run verify` with a fixed `CARGO_TARGET_DIR` pointing at that isolated
target directory, because the npm verifier re-enters Cargo.

After synchronizing a new AO2 commit that changes the worker source, restart the
local Windows Scheduled Task from an elevated Windows PowerShell session:

```powershell
Stop-ScheduledTask -TaskName "AO2 Windows Outbound Worker"
Start-ScheduledTask -TaskName "AO2 Windows Outbound Worker"
```

Do not expose the worker token, do not run this restart through the task board,
and do not add any inbound Windows listener.
