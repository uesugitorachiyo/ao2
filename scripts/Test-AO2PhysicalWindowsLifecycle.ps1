$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Get-SourceVersion {
    param([string]$CargoTomlPath)

    $match = [regex]::Match((Get-Content -LiteralPath $CargoTomlPath -Raw), '(?m)^version\s*=\s*"([^"]+)"\s*$')
    if (-not $match.Success) {
        throw "AO2 workspace version was not found"
    }
    return $match.Groups[1].Value
}

function Get-Sha256 {
    param([string]$Path)

    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Invoke-QuietNativeCommand {
    param(
        [string]$FilePath,
        [string[]]$ArgumentList
    )

    $nativeExitCode = -1
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        # Windows PowerShell 5.1 promotes native stderr to NativeCommandError.
        $ErrorActionPreference = "Continue"
        & $FilePath @ArgumentList *> $null
        $nativeExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    return [int]$nativeExitCode
}

function Get-VerifiedVersion {
    param(
        [string]$Binary,
        [string]$ExpectedVersion,
        [string]$ExpectedCommit,
        [string]$ExpectedProfile
    )

    $versionText = (& $Binary version --json 2>$null | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "AO2 version command failed"
    }
    $versionResult = $versionText | ConvertFrom-Json
    if ($versionResult.version -ne $ExpectedVersion) {
        throw "AO2 version does not match source version"
    }
    if ($versionResult.git_commit -ne $ExpectedCommit) {
        throw "AO2 version does not match source commit"
    }
    if ($versionResult.build_profile -ne $ExpectedProfile) {
        throw "AO2 version does not match expected build profile"
    }
    return $versionResult
}

function ConvertTo-NormalizedWindowsPath {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        throw "path must not be empty"
    }
    return [System.IO.Path]::GetFullPath($Path).Replace("/", "\").TrimEnd("\").ToLowerInvariant()
}

function Test-TextBindsExactPath {
    param(
        [string]$Text,
        [string]$ExpectedPath
    )

    if ([string]::IsNullOrWhiteSpace($Text)) {
        return $false
    }
    $normalizedText = $Text.Replace("/", "\").ToLowerInvariant()
    $normalizedExpected = ConvertTo-NormalizedWindowsPath -Path $ExpectedPath
    $pathPattern = "(?<![A-Za-z0-9_.\\-])" + [regex]::Escape($normalizedExpected) + "(?![A-Za-z0-9_.\\-])"
    return [regex]::IsMatch($normalizedText, $pathPattern)
}

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$sourceSha = (& git -C $repositoryRoot rev-parse HEAD 2>$null).Trim().ToLowerInvariant()
$version = Get-SourceVersion -CargoTomlPath (Join-Path $repositoryRoot "Cargo.toml")
$result = [ordered]@{
    schema_version = "ao2.physical-windows-lifecycle-probe.v1"
    source_sha = $sourceSha
    version = $version
    scheduled_task = [ordered]@{
        task_name = "AO2 Windows Outbound Worker"
        registered = $false
        enabled = $false
        state = "Unknown"
        last_task_result = -1
        result_acceptable = $false
        action_matches_worker = $false
    }
    persistent_outbound_worker = [ordered]@{
        probe_process_id = [int]$PID
        process_id = 0
        parent_process_id = 0
        probe_parent_is_worker = $false
        worker_executable_is_python = $false
        worker_script_matches = $false
        ancestry_verified = $false
        outbound_only = $false
    }
    installed_candidate_lifecycle = [ordered]@{
        exact_head = $sourceSha
        source_version_verified = $false
        debug_prior_built = $false
        release_candidate_built = $false
        candidate_package_created = $false
        package_manifest_verified = $false
        package_provenance_verified = $false
        install_completed = $false
        install_verification_verified = $false
        candidate_use_verified = $false
        candidate_digest = ""
        prior_digest = ""
        installed_candidate_digest = ""
        rollback_runner_separate = $false
        rollback_status = "not_run"
        rollback_completed = $false
        installed_rollback_digest = ""
        rollback_use_verified = $false
        uninstall_completed = $false
        temp_cleanup_completed = $false
        windows_safe = $false
    }
    safety_boundaries = [ordered]@{
        inbound_http = $false
        arbitrary_remote_execution = $false
        credential_changes = $false
        release_mutation = $false
        self_hosted_public_repository_runner = $false
    }
    hosted_windows_equivalence_exceptions = @(
        "portable test suites remain owned by hosted native Windows",
        "this probe covers only physical-Windows lifecycle evidence"
    )
}

$workRoot = Join-Path ([System.IO.Path]::GetTempPath()) "ao2-physical-windows-lifecycle-$sourceSha"
$lifecycleSucceeded = $false
$failureStage = "scheduled-task"
$previousBuildCommit = $env:AO2_BUILD_GIT_COMMIT
$previousBuildProfile = $env:AO2_BUILD_PROFILE
$previousPackagedCommit = $env:AO2_PACKAGED_GIT_COMMIT
$previousPackagedProfile = $env:AO2_PACKAGED_BUILD_PROFILE
$previousInstallDir = $env:AO2_INSTALL_DIR
try {
    $task = Get-ScheduledTask -TaskName "AO2 Windows Outbound Worker" -ErrorAction Stop
    $taskInfo = Get-ScheduledTaskInfo -TaskName "AO2 Windows Outbound Worker" -ErrorAction Stop
    $result.scheduled_task.registered = $true
    $result.scheduled_task.enabled = [bool]$task.Settings.Enabled
    $result.scheduled_task.state = [string]$task.State
    $result.scheduled_task.last_task_result = [int64]$taskInfo.LastTaskResult
    $result.scheduled_task.result_acceptable = (
        ($result.scheduled_task.state -eq "Running" -and $result.scheduled_task.last_task_result -eq 267009) -or
        ($result.scheduled_task.state -eq "Ready" -and $result.scheduled_task.last_task_result -eq 0)
    )
    if (-not $result.scheduled_task.enabled -or -not $result.scheduled_task.result_acceptable) {
        throw "AO2 Windows Outbound Worker Scheduled Task state/result is not acceptable"
    }

    $failureStage = "worker-ancestry"
    $workerScript = Join-Path $repositoryRoot "scripts\ao2_windows_outbound_worker.py"
    $taskActions = @($task.Actions)
    if ($taskActions.Count -ne 1) {
        throw "AO2 Windows Outbound Worker Scheduled Task must have exactly one action"
    }
    $taskAction = $taskActions[0]
    $taskExecutable = [System.IO.Path]::GetFileName([string]$taskAction.Execute).ToLowerInvariant()
    $result.scheduled_task.action_matches_worker = (
        $taskExecutable -eq "powershell.exe" -and
        (Test-TextBindsExactPath -Text ([string]$taskAction.Arguments) -ExpectedPath $workerScript)
    )
    if (-not $result.scheduled_task.action_matches_worker) {
        throw "Scheduled Task action does not bind the exact outbound worker script"
    }

    $probeProcesses = @(Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $PID" -ErrorAction Stop)
    if ($probeProcesses.Count -ne 1) {
        throw "current lifecycle probe process is ambiguous or missing"
    }
    $probeProcess = $probeProcesses[0]
    $workerProcessId = [int]$probeProcess.ParentProcessId
    $workerProcesses = @(
        Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $workerProcessId" -ErrorAction Stop
    )
    if ($workerProcesses.Count -ne 1) {
        throw "lifecycle probe parent worker process is ambiguous or missing"
    }
    $worker = $workerProcesses[0]
    $workerExecutable = [System.IO.Path]::GetFileName([string]$worker.ExecutablePath).ToLowerInvariant()
    $result.persistent_outbound_worker.probe_parent_is_worker = (
        [int]$probeProcess.ParentProcessId -eq [int]$worker.ProcessId
    )
    $result.persistent_outbound_worker.worker_executable_is_python = (
        $workerExecutable -match '^python(?:3(?:\.\d+)?)?\.exe$'
    )
    $result.persistent_outbound_worker.worker_script_matches = (
        Test-TextBindsExactPath -Text ([string]$worker.CommandLine) -ExpectedPath $workerScript
    )
    $result.persistent_outbound_worker.process_id = [int]$worker.ProcessId
    $result.persistent_outbound_worker.parent_process_id = [int]$worker.ParentProcessId
    $result.persistent_outbound_worker.ancestry_verified = (
        $result.persistent_outbound_worker.probe_parent_is_worker -and
        $result.persistent_outbound_worker.worker_executable_is_python -and
        $result.persistent_outbound_worker.worker_script_matches -and
        $result.scheduled_task.action_matches_worker
    )
    if (-not $result.persistent_outbound_worker.ancestry_verified) {
        throw "current lifecycle probe parent is not the exact Scheduled Task outbound worker"
    }
    $result.persistent_outbound_worker.outbound_only = $true

    $failureStage = "source-cleanliness"
    $cleanTree = @(& git -C $repositoryRoot status --porcelain 2>$null).Count -eq 0
    if (-not $cleanTree -or $sourceSha -notmatch '^[0-9a-f]{40}$') {
        throw "source checkout is not an exact clean Git head"
    }
    if (Test-Path -LiteralPath $workRoot) {
        Remove-Item -LiteralPath $workRoot -Recurse -Force
    }

    $failureStage = "workspace-preparation"
    $targetRoot = Join-Path $workRoot "target"
    $distRoot = Join-Path $workRoot "dist"
    $extractRoot = Join-Path $workRoot "extract"
    $installRoot = Join-Path $workRoot "install"
    New-Item -ItemType Directory -Path $targetRoot, $distRoot, $extractRoot, $installRoot -Force | Out-Null

    $failureStage = "debug-build"
    $env:AO2_BUILD_GIT_COMMIT = $sourceSha
    $env:AO2_BUILD_PROFILE = "debug"
    $debugBuildExitCode = Invoke-QuietNativeCommand -FilePath cargo -ArgumentList @(
        "build", "--locked", "-p", "ao2-cli", "--bin", "ao2", "--target-dir", $targetRoot
    )
    if ($debugBuildExitCode -ne 0) {
        throw "exact-head debug prior build failed"
    }
    $failureStage = "debug-identity"
    $priorBinary = Join-Path $targetRoot "debug\ao2.exe"
    $priorVersion = Get-VerifiedVersion $priorBinary $version $sourceSha "debug"
    $priorDigest = Get-Sha256 $priorBinary
    $result.installed_candidate_lifecycle.debug_prior_built = $true
    $result.installed_candidate_lifecycle.prior_digest = $priorDigest

    $failureStage = "release-build"
    $env:AO2_BUILD_PROFILE = "release"
    $releaseBuildExitCode = Invoke-QuietNativeCommand -FilePath cargo -ArgumentList @(
        "build", "--locked", "--release", "-p", "ao2-cli", "--bin", "ao2", "--target-dir", $targetRoot
    )
    if ($releaseBuildExitCode -ne 0) {
        throw "exact-head release candidate build failed"
    }
    $failureStage = "release-identity"
    $candidateBinary = Join-Path $targetRoot "release\ao2.exe"
    $candidateVersion = Get-VerifiedVersion $candidateBinary $version $sourceSha "release"
    $candidateDigest = Get-Sha256 $candidateBinary
    if ($candidateDigest -eq $priorDigest) {
        throw "debug prior and release candidate digests must differ"
    }
    $result.installed_candidate_lifecycle.release_candidate_built = $true
    $result.installed_candidate_lifecycle.source_version_verified = $true
    $result.installed_candidate_lifecycle.candidate_digest = $candidateDigest

    $failureStage = "package"
    $env:AO2_PACKAGED_GIT_COMMIT = $sourceSha
    $env:AO2_PACKAGED_BUILD_PROFILE = "release"
    $packageText = (& $candidateBinary release package `
        --out-dir $distRoot `
        --version $version `
        --target-label "windows-x86_64" `
        --binary $candidateBinary 2>$null | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "repository-owned AO2 release package command failed"
    }
    $package = $packageText | ConvertFrom-Json
    $archive = [string]$package.archive
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) {
        throw "release package archive was not created"
    }
    $result.installed_candidate_lifecycle.candidate_package_created = $true

    $failureStage = "package-extraction"
    $extractExitCode = Invoke-QuietNativeCommand -FilePath tar -ArgumentList @(
        "-xzf", $archive, "-C", $extractRoot
    )
    if ($extractExitCode -ne 0) {
        throw "release package extraction failed"
    }
    $failureStage = "package-manifest"
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $extractRoot "RELEASE-MANIFEST.json") | ConvertFrom-Json
    if (
        $manifest.schema_version -ne "ao2.release-manifest.v1" -or
        $manifest.version -ne $version -or
        $manifest.target -ne "windows-x86_64" -or
        $manifest.binary -ne "ao2.exe" -or
        $manifest.binary_path -ne "bin/ao2.exe" -or
        $manifest.binary_sha256 -ne $candidateDigest
    ) {
        throw "release package manifest did not bind the exact-head candidate"
    }
    $result.installed_candidate_lifecycle.package_manifest_verified = $true

    $failureStage = "package-provenance"
    $provenance = Get-Content -Raw -LiteralPath (Join-Path $extractRoot "BUILD-PROVENANCE.json") | ConvertFrom-Json
    if (
        $provenance.schema_version -ne "ao2.build-provenance.v1" -or
        $provenance.version -ne $version -or
        $provenance.git_commit -ne $sourceSha -or
        $provenance.build_profile -ne "release"
    ) {
        throw "release package provenance did not bind the exact-head candidate"
    }
    $result.installed_candidate_lifecycle.package_provenance_verified = $true

    $failureStage = "install"
    $env:AO2_INSTALL_DIR = $installRoot
    & (Join-Path $extractRoot "install.ps1") *> $null
    $installedBinary = Join-Path $installRoot "ao2.exe"
    $installSidecar = Join-Path $installRoot "ao2.exe.install-verification.json"
    if (
        -not (Test-Path -LiteralPath $installedBinary -PathType Leaf) -or
        -not (Test-Path -LiteralPath $installSidecar -PathType Leaf)
    ) {
        throw "release package installer did not create the candidate and verification sidecar"
    }
    $result.installed_candidate_lifecycle.install_completed = $true

    $failureStage = "install-verification"
    $installVerification = Get-Content -Raw -LiteralPath $installSidecar | ConvertFrom-Json
    if (
        $installVerification.schema_version -ne "ao2.install-verification-evidence.v1" -or
        $installVerification.status -ne "verified" -or
        $installVerification.version -ne $version -or
        $installVerification.target -ne "windows-x86_64" -or
        $installVerification.provider_api_keys_required -ne $false -or
        $installVerification.control_plane_approves_release -ne $false -or
        $installVerification.mutates_ao_artifacts -ne $false -or
        $installVerification.release_acceptance_owner -ne "factory-v3 evaluator-closer" -or
        $installVerification.offline_verification.schema_version -ne "ao2.release-archive-offline-verification.v1" -or
        $installVerification.offline_verification.status -ne "verified" -or
        $installVerification.offline_verification.checksum_coverage_verified -ne $true -or
        $installVerification.offline_verification.provider_api_keys_required -ne $false -or
        $installVerification.offline_verification.control_plane_approves_release -ne $false -or
        $installVerification.offline_verification.mutates_ao_artifacts -ne $false
    ) {
        throw "install verification sidecar is invalid"
    }
    $result.installed_candidate_lifecycle.install_verification_verified = $true

    $failureStage = "candidate-use"
    $installedCandidateDigest = Get-Sha256 $installedBinary
    if ($installedCandidateDigest -ne $candidateDigest) {
        throw "installed candidate digest does not match packaged candidate"
    }
    $null = Get-VerifiedVersion $installedBinary $version $sourceSha "release"
    $result.installed_candidate_lifecycle.installed_candidate_digest = $installedCandidateDigest
    $result.installed_candidate_lifecycle.candidate_use_verified = $true

    $failureStage = "rollback"
    $rollbackBinary = "$installedBinary.rollback"
    Copy-Item -LiteralPath $priorBinary -Destination $rollbackBinary -Force
    if ((Get-Sha256 $rollbackBinary) -ne $priorDigest) {
        throw "seeded rollback binary does not match distinct debug prior"
    }
    $rollbackRunner = Join-Path $extractRoot "bin\ao2.exe"
    if ((Resolve-Path $rollbackRunner).Path -eq (Resolve-Path $installedBinary).Path) {
        throw "Windows-safe rollback runner is not separate from installed candidate"
    }
    $result.installed_candidate_lifecycle.rollback_runner_separate = $true

    $rollbackText = (& $rollbackRunner install rollback --install-dir $installRoot `
        --target-label "windows-x86_64" 2>$null | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "Windows-safe rollback command failed"
    }
    $rollback = $rollbackText | ConvertFrom-Json
    if ($rollback.status -ne "rolled_back") {
        throw "Windows-safe rollback status is invalid"
    }
    $result.installed_candidate_lifecycle.rollback_status = [string]$rollback.status

    $installedRollbackDigest = Get-Sha256 $installedBinary
    if ($installedRollbackDigest -ne $priorDigest -or $installedRollbackDigest -eq $candidateDigest) {
        throw "Windows-safe rollback did not install the distinct debug prior"
    }
    $null = Get-VerifiedVersion $installedBinary $version $sourceSha "debug"
    $result.installed_candidate_lifecycle.installed_rollback_digest = $installedRollbackDigest
    $result.installed_candidate_lifecycle.rollback_completed = $true
    $result.installed_candidate_lifecycle.rollback_use_verified = $true
    $result.installed_candidate_lifecycle.windows_safe = $true

    $failureStage = "uninstall"
    Remove-Item -LiteralPath $installedBinary, $rollbackBinary, $installSidecar -Force -ErrorAction SilentlyContinue
    $result.installed_candidate_lifecycle.uninstall_completed = (
        -not (Test-Path -LiteralPath $installedBinary) -and
        -not (Test-Path -LiteralPath $rollbackBinary) -and
        -not (Test-Path -LiteralPath $installSidecar)
    )
    if (-not $result.installed_candidate_lifecycle.uninstall_completed) {
        throw "installed candidate lifecycle artifacts were not removed"
    }
    $lifecycleSucceeded = $true
}
catch {
    $lifecycleSucceeded = $false
    [Console]::Error.WriteLine("physical_windows_lifecycle_failure_stage=$failureStage")
}
finally {
    $env:AO2_BUILD_GIT_COMMIT = $previousBuildCommit
    $env:AO2_BUILD_PROFILE = $previousBuildProfile
    $env:AO2_PACKAGED_GIT_COMMIT = $previousPackagedCommit
    $env:AO2_PACKAGED_BUILD_PROFILE = $previousPackagedProfile
    $env:AO2_INSTALL_DIR = $previousInstallDir
    if (Test-Path -LiteralPath $workRoot) {
        Remove-Item -LiteralPath $workRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    $result.installed_candidate_lifecycle.temp_cleanup_completed = -not (Test-Path -LiteralPath $workRoot)
    if (-not $lifecycleSucceeded -or -not $result.installed_candidate_lifecycle.temp_cleanup_completed) {
        $result.installed_candidate_lifecycle.windows_safe = $false
    }
}

$result | ConvertTo-Json -Compress -Depth 5
if (-not $lifecycleSucceeded) {
    exit 1
}
