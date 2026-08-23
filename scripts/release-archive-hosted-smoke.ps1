$ErrorActionPreference = "Stop"

$TargetLabel = if ($env:AO2_RELEASE_HOSTED_TARGET_LABEL) { $env:AO2_RELEASE_HOSTED_TARGET_LABEL } else { "windows-x86_64" }
$Version = if ($env:AO2_RELEASE_HOSTED_VERSION) { $env:AO2_RELEASE_HOSTED_VERSION } else { (Get-Content -Raw -LiteralPath "package.json" | ConvertFrom-Json).version }
$Root = if ($env:AO2_RELEASE_HOSTED_SMOKE_ROOT) { $env:AO2_RELEASE_HOSTED_SMOKE_ROOT } else { Join-Path "target/release-archive-hosted-smoke" $TargetLabel }
$SummaryJson = if ($env:AO2_RELEASE_HOSTED_SMOKE_JSON) { $env:AO2_RELEASE_HOSTED_SMOKE_JSON } else { Join-Path $Root "summary.json" }
$Binary = if ($env:AO2_RELEASE_HOSTED_BINARY) { $env:AO2_RELEASE_HOSTED_BINARY } else { "target/release/ao2.exe" }
$ExpectedCommit = if ($env:AO2_BUILD_GIT_COMMIT) { $env:AO2_BUILD_GIT_COMMIT } elseif ($env:GITHUB_SHA) { $env:GITHUB_SHA } else { (git rev-parse HEAD) }
$Dist = Join-Path $Root "dist"
$Extract = Join-Path $Root "extract"
$InstallDir = Join-Path $Root "bin"
$Archive = Join-Path $Dist "ao2-$Version-$TargetLabel.tar.gz"

if (Test-Path -LiteralPath $Root) {
    Remove-Item -Recurse -Force -LiteralPath $Root
}
New-Item -ItemType Directory -Force -Path $Dist, $Extract, $InstallDir | Out-Null

if (!(Test-Path -LiteralPath $Binary -PathType Leaf)) {
    throw "missing hosted release binary: $Binary"
}

$env:AO2_PACKAGED_GIT_COMMIT = $ExpectedCommit
$env:AO2_PACKAGED_BUILD_PROFILE = "release"
cargo run -p ao2-cli -- release package `
    --out-dir $Dist `
    --version $Version `
    --target-label $TargetLabel `
    --binary $Binary | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root "package.json")

if (!(Test-Path -LiteralPath $Archive -PathType Leaf)) {
    throw "missing hosted release archive: $Archive"
}
tar -xzf $Archive -C $Extract
if ($LASTEXITCODE -ne 0) {
    throw "archive extraction failed"
}

$ManifestPath = Join-Path $Extract "RELEASE-MANIFEST.json"
$Manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json
if ($Manifest.schema_version -ne "ao2.release-manifest.v1") { throw "unexpected manifest schema: $($Manifest.schema_version)" }
if ($Manifest.target -ne $TargetLabel) { throw "unexpected manifest target: $($Manifest.target)" }
if ($Manifest.binary -ne "ao2.exe") { throw "unexpected manifest binary: $($Manifest.binary)" }

$ProvenancePath = Join-Path $Extract "BUILD-PROVENANCE.json"
$Provenance = Get-Content -Raw -LiteralPath $ProvenancePath | ConvertFrom-Json
if ($Provenance.version -ne $Version) { throw "unexpected provenance version: $($Provenance.version)" }
if ($Provenance.git_commit -ne $ExpectedCommit) { throw "unexpected provenance git_commit: $($Provenance.git_commit)" }
if ($Provenance.build_profile -ne "release") { throw "unexpected provenance build_profile: $($Provenance.build_profile)" }

$WorkerPackage = Join-Path $Root "worker package with spaces"
Copy-Item -LiteralPath $Extract -Destination $WorkerPackage -Recurse
$WorkerLauncher = Join-Path $WorkerPackage "ao2-windows-worker.cmd"
if (!(Test-Path -LiteralPath $WorkerLauncher -PathType Leaf)) { throw "missing packaged Windows worker launcher" }
& $WorkerLauncher --help | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root "windows-worker-help.txt")
if ($LASTEXITCODE -ne 0) { throw "packaged Windows worker --help failed" }

$FactoryRoot = Join-Path $Root "factory root with spaces"
$LeaseId = "release-smoke-lease"
$ScratchRoot = Join-Path (Join-Path $FactoryRoot ".ao2-physical-host-leases") $LeaseId
$Now = [DateTimeOffset]::UtcNow
$UtcFormat = "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
$Lease = [ordered]@{
    schema_version = "ao2.physical-host-exclusive-lease.v1"
    lease_id = $LeaseId
    node_id = "windows-release-smoke"
    purpose = "windows_stack_qualification"
    operator_approved = $true
    operator_approval_id = "release-smoke-approval"
    issued_at = $Now.AddMinutes(-1).ToString($UtcFormat)
    expires_at = $Now.AddMinutes(10).ToString($UtcFormat)
    heartbeat_at = $Now.ToString($UtcFormat)
    exclusive_use_confirmed = $true
    interactive_sessions_active = 0
    overlapping_lease_ids = @()
    command_profile = "windows_stack_qualification:lifecycle_noop"
    scratch_root = $ScratchRoot
    cleanup_roots = @($ScratchRoot)
    natural_completion_only = $true
    abort_requested = $false
    released = $false
    allow_broad_process_termination = $false
    allow_graphical_session_mutation = $false
}
$LeasePath = Join-Path $Root "offline lease with spaces.json"
$LeaseJson = $Lease | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($LeasePath, $LeaseJson, [System.Text.UTF8Encoding]::new($false))
$LeaseSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $LeasePath).Hash.ToLowerInvariant()
$WorkerValidationPath = Join-Path $Root "windows-worker-offline-validation.json"
& $WorkerLauncher `
    --validate-physical-host-lease $LeasePath `
    --physical-host-lease-sha256 $LeaseSha256 `
    --physical-host-lease-profile "windows_stack_qualification:lifecycle_noop" `
    --node-id "windows-release-smoke" `
    --factory-root $FactoryRoot | Set-Content -Encoding UTF8 -LiteralPath $WorkerValidationPath
if ($LASTEXITCODE -ne 0) { throw "packaged Windows worker offline lease validation failed" }
$WorkerValidation = Get-Content -Raw -LiteralPath $WorkerValidationPath | ConvertFrom-Json
if ($WorkerValidation.status -ne "accepted") { throw "packaged Windows worker offline lease was not accepted" }

$env:AO2_INSTALL_DIR = $InstallDir
& (Join-Path $Extract "install.ps1") | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root "install.txt")
if ($LASTEXITCODE -ne 0) {
    throw "install.ps1 failed"
}

$Installed = Join-Path $InstallDir "ao2.exe"
$InstallVerificationEvidence = Join-Path $InstallDir "ao2.exe.install-verification.json"
if (!(Test-Path -LiteralPath $Installed -PathType Leaf)) { throw "missing installed ao2.exe" }
if (!(Test-Path -LiteralPath $InstallVerificationEvidence -PathType Leaf)) { throw "missing install verification evidence" }

$InstallVerification = Get-Content -Raw -LiteralPath $InstallVerificationEvidence | ConvertFrom-Json
if ($InstallVerification.schema_version -ne "ao2.install-verification-evidence.v1") { throw "unexpected install verification schema" }
if ($InstallVerification.status -ne "verified") { throw "install verification status must be verified" }
if ($InstallVerification.provider_api_keys_required -ne $false) { throw "install verification must not require provider API keys" }
if ($InstallVerification.control_plane_approves_release -ne $false) { throw "control plane must not approve release" }
if ($InstallVerification.mutates_ao_artifacts -ne $false) { throw "install verification must not mutate AO artifacts" }
if ($InstallVerification.release_acceptance_owner -ne "factory-v3 evaluator-closer") { throw "unexpected release acceptance owner" }

& $Installed --help | Out-Null
& $Installed version --json | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root "version.json")
& $Installed adapter doctor --provider scripted | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root "scripted-doctor.txt")
& $Installed provider matrix --json | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root "provider-matrix.json")

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $SummaryJson) | Out-Null
[ordered]@{
    schema_version = "ao2.release-archive-hosted-smoke.v1"
    status = "passed"
    target = $TargetLabel
    version = $Version
    archive = $Archive
    installed_binary = $Installed
    install_verification_evidence = $InstallVerificationEvidence
    install_verification_schema = "ao2.install-verification-evidence.v1"
    provider_api_keys_required = $false
    control_plane_approves_release = $false
    mutates_ao_artifacts = $false
    release_acceptance_owner = "factory-v3 evaluator-closer"
    windows_worker_launcher = "passed"
    windows_worker_python_requirement = ">=3.11"
    provider_calls = 0
    credential_use = 0
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath $SummaryJson

$CandidateDist = Join-Path (Split-Path -Parent $SummaryJson) "dist"
New-Item -ItemType Directory -Force -Path $CandidateDist | Out-Null
$CandidateArchive = Join-Path $CandidateDist (Split-Path -Leaf $Archive)
if ([System.IO.Path]::GetFullPath($Archive) -ne [System.IO.Path]::GetFullPath($CandidateArchive)) {
    Copy-Item -LiteralPath $Archive -Destination $CandidateArchive -Force
}
$SourceArchiveDigest = (Get-FileHash -Algorithm SHA256 -LiteralPath $Archive).Hash
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $CandidateArchive).Hash -ne $SourceArchiveDigest) {
    throw "staged native candidate archive digest mismatch"
}

Write-Output "release_archive_hosted_smoke=passed"
Write-Output "target=$TargetLabel"
Write-Output "summary=$SummaryJson"
Write-Output "install_verification_evidence=$InstallVerificationEvidence"
