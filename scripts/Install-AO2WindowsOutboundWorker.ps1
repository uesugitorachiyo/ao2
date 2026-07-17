# Installs the AO2 Windows outbound worker as a local Windows Scheduled Task.
# Run this on the Windows worker. Do not invoke it through the task board.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$ControlPlaneUrl,
    [Parameter(Mandatory=$true)][string]$ApiTokenFile,
    [string]$FactoryRoot = "C:\ao\factory",
    [string]$NodeId = "windows-hp255_g10",
    [string]$WorkerScript = "C:\ao\factory\ao2\scripts\ao2_windows_outbound_worker.py",
    [string]$Python = "python",
    [string]$TaskName = "AO2 Windows Outbound Worker"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $ApiTokenFile)) { throw "API token file not found: $ApiTokenFile" }
if (-not (Test-Path $WorkerScript)) { throw "Worker script not found: $WorkerScript" }

$StateRoot = Join-Path $env:LOCALAPPDATA "AO2\windows-outbound-worker"
New-Item -ItemType Directory -Force -Path $StateRoot | Out-Null

$Args = @(
    "-NoProfile",
    "-ExecutionPolicy", "Bypass",
    "-Command",
    "& '$Python' '$WorkerScript' --control-plane-url '$ControlPlaneUrl' --api-token-file '$ApiTokenFile' --factory-root '$FactoryRoot' --node-id '$NodeId'"
)
$Action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument ($Args -join " ")
$Trigger = New-ScheduledTaskTrigger -AtLogOn
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)

Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Settings $Settings -Description "AO2 outbound polling worker. Opens no inbound Windows HTTP listener." -Force | Out-Null
Start-ScheduledTask -TaskName $TaskName

[ordered]@{
    schema = "ao2.windows-outbound-worker-install.v1"
    status = "started"
    task_name = $TaskName
    node_id = $NodeId
    factory_root = $FactoryRoot
    worker_script = $WorkerScript
    state_root = $StateRoot
    inbound_http_enabled = $false
} | ConvertTo-Json -Depth 4
