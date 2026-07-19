$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Get-SourceVersion {
    param([string]$CargoTomlPath)

    $match = [regex]::Match((Get-Content -LiteralPath $CargoTomlPath -Raw), '(?m)^version\s*=\s*"([^"]+)"\s*$')
    if (-not $match.Success) {
        throw "ao2 CLI version was not found"
    }
    return $match.Groups[1].Value
}

function Test-WorkerAncestry {
    param([object]$WorkerProcess)

    $parentId = [int]$WorkerProcess.ParentProcessId
    for ($depth = 0; $depth -lt 12 -and $parentId -gt 0; $depth++) {
        $parent = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $parentId" -ErrorAction SilentlyContinue
        if ($null -eq $parent) {
            return $false
        }
        if ($parent.Name -in @("taskeng.exe", "taskhostw.exe")) {
            return $true
        }
        $parentId = [int]$parent.ParentProcessId
    }
    return $false
}

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$sourceSha = (& git -C $repositoryRoot rev-parse HEAD 2>$null).Trim().ToLowerInvariant()
$version = Get-SourceVersion -CargoTomlPath (Join-Path $repositoryRoot "Cargo.toml")
$completedAt = [DateTime]::UtcNow.ToString("o")
$result = [ordered]@{
    schema_version = "ao2.physical-windows-lifecycle-probe.v1"
    source_sha = $sourceSha
    version = $version
    request_id = "physical-windows-lifecycle-$sourceSha"
    result_id = "physical-windows-lifecycle-$sourceSha"
    completed_at = $completedAt
    scheduled_task = [ordered]@{
        task_name = "AO2 Windows Outbound Worker"
        registered = $false
        enabled = $false
        state = "Unknown"
    }
    persistent_outbound_worker = [ordered]@{
        process_id = 0
        parent_process_id = 0
        ancestry_verified = $false
        outbound_only = $true
    }
    installed_candidate_lifecycle = [ordered]@{
        exact_head = $sourceSha
        package_built = $false
        install_completed = $false
        use_completed = $false
        rollback_completed = $false
        uninstall_completed = $false
        windows_safe = $true
    }
    safety_boundaries = [ordered]@{
        inbound_http = $false
        arbitrary_remote_execution = $false
        credential_changes = $false
        release_mutation = $false
    }
    hosted_windows_equivalence_exceptions = @(
        "portable test suites remain owned by hosted native Windows",
        "this probe covers only physical-Windows lifecycle evidence"
    )
}

$workRoot = Join-Path ([System.IO.Path]::GetTempPath()) "ao2-physical-windows-lifecycle-$sourceSha"
try {
    $task = Get-ScheduledTask -TaskName "AO2 Windows Outbound Worker" -ErrorAction Stop
    $result.scheduled_task.registered = $true
    $result.scheduled_task.enabled = [bool]$task.Settings.Enabled
    $result.scheduled_task.state = [string]$task.State

    $workerScript = Join-Path $repositoryRoot "scripts\ao2_windows_outbound_worker.py"
    $workerPattern = [regex]::Escape($workerScript)
    $worker = Get-CimInstance -ClassName Win32_Process -Filter "Name = 'python.exe'" |
        Where-Object { $_.CommandLine -match $workerPattern } |
        Select-Object -First 1
    if ($null -ne $worker) {
        $result.persistent_outbound_worker.process_id = [int]$worker.ProcessId
        $result.persistent_outbound_worker.parent_process_id = [int]$worker.ParentProcessId
        $result.persistent_outbound_worker.ancestry_verified = Test-WorkerAncestry -WorkerProcess $worker
    }

    $cleanTree = (& git -C $repositoryRoot status --porcelain 2>$null).Count -eq 0
    if (-not $cleanTree -or $sourceSha -notmatch '^[0-9a-f]{40}$') {
        throw "source checkout is not an exact clean Git head"
    }

    $targetRoot = Join-Path $workRoot "target"
    $installRoot = Join-Path $workRoot "installed"
    $rollbackRoot = Join-Path $workRoot "rollback"
    New-Item -ItemType Directory -Path $targetRoot, $installRoot, $rollbackRoot -Force | Out-Null
    & cargo build --locked -p ao2-cli --bin ao2 --target-dir $targetRoot *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "isolated candidate package build failed"
    }
    $result.installed_candidate_lifecycle.package_built = $true

    $candidateBinary = Join-Path $targetRoot "debug\ao2.exe"
    $installedBinary = Join-Path $installRoot "ao2.exe"
    Copy-Item -LiteralPath $candidateBinary -Destination $installedBinary -Force
    $result.installed_candidate_lifecycle.install_completed = Test-Path -LiteralPath $installedBinary -PathType Leaf

    & $installedBinary --version *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "isolated installed candidate use failed"
    }
    $result.installed_candidate_lifecycle.use_completed = $true

    $rollbackBinary = Join-Path $rollbackRoot "ao2.exe"
    Copy-Item -LiteralPath $candidateBinary -Destination $rollbackBinary -Force
    Move-Item -LiteralPath $rollbackBinary -Destination $installedBinary -Force
    $result.installed_candidate_lifecycle.rollback_completed = Test-Path -LiteralPath $installedBinary -PathType Leaf

    Remove-Item -LiteralPath $installRoot -Recurse -Force
    $result.installed_candidate_lifecycle.uninstall_completed = -not (Test-Path -LiteralPath $installRoot)
}
catch {
    $result.installed_candidate_lifecycle.windows_safe = $false
}
finally {
    if (Test-Path -LiteralPath $workRoot) {
        Remove-Item -LiteralPath $workRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$result | ConvertTo-Json -Compress -Depth 5
