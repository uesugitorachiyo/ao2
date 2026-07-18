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

- `mode`: `diagnostic`, `targeted`, or `full`
- `repositories` or `repos`: names from the canonical AO stack inventory
- `timeout_seconds`: a bounded value from 30 through 3600 seconds

The payload must not provide command text, PowerShell text, executable paths,
working directories, shell fragments, or environment variables. Repository
names are resolved only beneath `C:\ao\factory`; traversal, separators,
absolute paths, drive-letter paths, duplicates, unknown repositories, and the
archived `agy-swarms` reference implementation are rejected before execution.

The canonical inventory is machine-readable at
`docs/windows-stack-qualification-inventory.json`. The reviewed
repository-to-command profiles live in
`scripts/ao2_windows_outbound_worker.py::WINDOWS_REPOSITORY_PROFILES`.
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
