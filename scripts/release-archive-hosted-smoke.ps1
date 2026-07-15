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
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath $SummaryJson

Write-Output "release_archive_hosted_smoke=passed"
Write-Output "target=$TargetLabel"
Write-Output "summary=$SummaryJson"
Write-Output "install_verification_evidence=$InstallVerificationEvidence"
