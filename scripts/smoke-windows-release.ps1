param(
    [Parameter(Mandatory = $true)]
    [string]$Archive,

    [string]$SmokeRoot = (Join-Path $env:TEMP ("ao2-windows-smoke-" + [Guid]::NewGuid().ToString("N")))
)

$ErrorActionPreference = "Stop"

if (!(Test-Path $Archive)) {
    throw "missing Windows release archive: $Archive"
}

$ArchivePath = (Resolve-Path $Archive).Path
$ExtractDir = Join-Path $SmokeRoot "extract"
$InstallDir = Join-Path $SmokeRoot "bin"
$RepoDir = Join-Path $SmokeRoot "repo"
$WorkflowPath = Join-Path $SmokeRoot "workflow.yaml"
$PromptPath = Join-Path $SmokeRoot "prompt.ps1"

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    $Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Value, $Utf8NoBom)
}

New-Item -ItemType Directory -Force -Path $ExtractDir, $InstallDir, (Join-Path $RepoDir "src") | Out-Null
Write-Utf8NoBom -Path (Join-Path $RepoDir "README.md") -Value "AO2 Windows release smoke target`n"
git -C $RepoDir init | Out-Null
git -C $RepoDir config user.email "ao2-release-smoke@example.invalid"
git -C $RepoDir config user.name "AO2 Release Smoke"
git -C $RepoDir add README.md
git -C $RepoDir commit -m "Initialize AO2 Windows release smoke target" | Out-Null
tar -xzf $ArchivePath -C $ExtractDir

$ManifestPath = Join-Path $ExtractDir "RELEASE-MANIFEST.json"
if (!(Test-Path $ManifestPath)) {
    throw "missing release manifest: $ManifestPath"
}
$Manifest = Get-Content $ManifestPath -Raw | ConvertFrom-Json
if ($Manifest.schema_version -ne "ao2.release-manifest.v1") {
    throw "unexpected release manifest schema: $($Manifest.schema_version)"
}
if ($Manifest.binary -ne "ao2.exe") {
    throw "unexpected release manifest binary: $($Manifest.binary)"
}
if ($Manifest.binary_path -ne "bin/ao2.exe") {
    throw "unexpected release manifest binary path: $($Manifest.binary_path)"
}

$env:AO2_INSTALL_DIR = $InstallDir
& (Join-Path $ExtractDir "install.ps1")

$Ao2 = Join-Path $InstallDir "ao2.exe"
$RollbackRunner = Join-Path $ExtractDir "bin/ao2.exe"
if (!(Test-Path $Ao2)) {
    throw "installed ao2.exe was not found: $Ao2"
}
if (!(Test-Path $RollbackRunner)) {
    throw "rollback runner ao2.exe was not found: $RollbackRunner"
}
if ((Resolve-Path $RollbackRunner).Path -eq (Resolve-Path $Ao2).Path) {
    throw "Windows-safe rollback runner must be separate from installed ao2.exe"
}
# If this script ever uses the installed $Ao2 for rollback, Windows can block
# the active executable replacement with rollback_status=blocked_active_executable.
Write-Output "Windows-safe rollback runner=$RollbackRunner"
Write-Output "rollback_runner=$RollbackRunner"

& $Ao2 --help | Out-Null
& $Ao2 version --json | Out-Null
Copy-Item -Force $Ao2 "$Ao2.rollback"
$Rollback = & $RollbackRunner install rollback --install-dir $InstallDir --target-label windows-x86_64 | ConvertFrom-Json
if ($Rollback.status -ne "rolled_back") {
    throw "unexpected rollback status: $($Rollback.status)"
}
& $Ao2 version --json | Out-Null
Write-Output "windows_install_rollback=passed"
& $Ao2 adapter doctor --provider scripted | Out-Null
& $Ao2 provider matrix --json | Out-Null

@'
id: windows-install-smoke-repair
version: smoke
template_kind: real_project
objective: Verify installed AO2 can run a scripted real-project repair on Windows.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: powershell -NoProfile -Command if ((Get-Content src/value.txt -Raw).Trim() -ne 'ok') { throw 'expected ok after repair' }
acceptance:
  - Installed AO2 runs a scripted repair.
  - Replay has zero digest failures.
'@ | ForEach-Object { Write-Utf8NoBom -Path $WorkflowPath -Value $_ }

@'
New-Item -ItemType Directory -Force -Path src | Out-Null
if ($env:AO2_REPAIR_VERIFIER_OUTPUT) {
    Set-Content -Encoding utf8 src/value.txt "ok"
} else {
    Set-Content -Encoding utf8 src/value.txt "bad"
}
Write-Output "Summary: wrote repair-aware Windows smoke value"
Write-Output "Changed files: src/value.txt"
'@ | ForEach-Object { Write-Utf8NoBom -Path $PromptPath -Value $_ }

$RunOutput = & $Ao2 run $WorkflowPath `
    --target $RepoDir `
    --run-id windows-install-smoke-repair `
    --provider scripted `
    --provider-prompt-file $PromptPath `
    --max-repair-attempts 1

$ApprovalCount = 0
while (($RunOutput -join "`n") -match "status=WaitingForApproval") {
    $EvidencePackPath = Join-Path $RepoDir ".ao2/runs/windows-install-smoke-repair/evidence-pack/evidence-pack.json"
    $EvidencePack = Get-Content $EvidencePackPath -Raw | ConvertFrom-Json
    $PendingApproval = $EvidencePack.approvals |
        Where-Object { $_.requested_action -eq "sandbox:apply" -and $_.status -eq "pending" } |
        Select-Object -First 1
    if (!$PendingApproval -or !$PendingApproval.ticket_id) {
        throw "windows smoke run requested approval but no pending sandbox:apply ticket was found"
    }
    $ApproveOutput = & $Ao2 approve $PendingApproval.ticket_id `
        --target $RepoDir `
        --approver human:release-smoke
    if (($ApproveOutput -join "`n") -notmatch "status=approved") {
        throw "windows smoke approval did not approve:`n$($ApproveOutput -join "`n")"
    }
    $ApprovalCount += 1
    if ($ApprovalCount -gt 2) {
        throw "windows smoke requested too many approvals"
    }
    $RunOutput = & $Ao2 run --resume windows-install-smoke-repair `
        --target $RepoDir
}

if ($ApprovalCount -ne 2) {
    throw "windows smoke expected 2 approvals, got $ApprovalCount"
}

if (($RunOutput -join "`n") -notmatch "status=Accepted") {
    throw "windows smoke run did not accept:`n$($RunOutput -join "`n")"
}

$Replay = & $Ao2 replay windows-install-smoke-repair --target $RepoDir | ConvertFrom-Json
if ($Replay.status -ne "accepted") {
    throw "unexpected replay status: $($Replay.status)"
}
if ($Replay.digest_failures.Count -ne 0) {
    throw "replay digest failures: $($Replay.digest_failures -join ', ')"
}

$Value = (Get-Content (Join-Path $RepoDir "src/value.txt") -Raw).Trim()
if ($Value -ne "ok") {
    throw "expected repaired value ok, got $Value"
}

$Evidence = Join-Path $RepoDir ".ao2/runs/windows-install-smoke-repair/evidence-pack/evidence-pack.json"
$Cockpit = Join-Path $RepoDir ".ao2/runs/windows-install-smoke-repair/cockpit/index.html"

Write-Output "windows_evidence=$Evidence"
Write-Output "windows_cockpit=$Cockpit"
Write-Output "windows_install_smoke=passed"
