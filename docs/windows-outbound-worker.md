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
`sync_ao_stack`, `ao2_doctor`, and `timeout_fixture`. The worker rejects
arbitrary command text and stores a local idempotency ledger under
`%LOCALAPPDATA%\AO2\windows-outbound-worker`.
