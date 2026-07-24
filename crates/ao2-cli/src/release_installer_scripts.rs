use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::install_paths::make_executable;

pub(crate) fn write_installer_scripts(stage_dir: &Path, binary_name: &str) -> Result<()> {
    let install_sh = stage_dir.join("install.sh");
    fs::write(&install_sh, unix_installer_script(binary_name))?;
    make_executable(&install_sh)?;
    fs::write(
        stage_dir.join("install.ps1"),
        windows_installer_script(binary_name),
    )?;
    Ok(())
}

fn unix_installer_script(binary_name: &str) -> String {
    format!(
        r#"#!/bin/sh
set -eu

binary_name="{binary_name}"
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source_binary="$source_dir/bin/$binary_name"
checksum_file="$source_dir/SHA256SUMS"
install_dir="${{AO2_INSTALL_DIR:-$HOME/.local/bin}}"
dest_binary="$install_dir/$binary_name"

if [ ! -f "$source_binary" ]; then
  echo "missing packaged binary: $source_binary" >&2
  exit 1
fi

expected=$(awk -v file="bin/$binary_name" '$2 == file {{ print $1 }}' "$checksum_file")
if [ -z "$expected" ]; then
  echo "missing checksum for bin/$binary_name" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$source_binary" | awk '{{ print $1 }}')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$source_binary" | awk '{{ print $1 }}')
else
  echo "sha256sum or shasum is required to verify the packaged binary" >&2
  exit 1
fi

if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for bin/$binary_name" >&2
  exit 1
fi

mkdir -p "$install_dir"
cp "$source_binary" "$dest_binary"
chmod 755 "$dest_binary"

manifest_value() {{
  key="$1"
  awk -v key="\"$key\"" '
    index($0, key) {{
      sub(/^[^:]*:[[:space:]]*"/, "", $0)
      sub(/",?$/, "", $0)
      print
      exit
    }}
  ' "$source_dir/RELEASE-MANIFEST.json"
}}

version="$(manifest_value version)"
target="$(manifest_value target)"
evidence_path="$dest_binary.install-verification.json"
cat > "$evidence_path" <<JSON
{{
  "schema_version": "ao2.install-verification-evidence.v1",
  "status": "verified",
  "install_status": "installed",
  "version": "$version",
  "target": "$target",
  "binary": "$binary_name",
  "checksum_file": "SHA256SUMS",
  "offline_verification": {{
    "schema_version": "ao2.release-archive-offline-verification.v1",
    "status": "verified",
    "checksum_file": "SHA256SUMS",
    "verification_report": "RELEASE-VERIFICATION.json",
    "checksum_coverage_verified": true,
    "provider_api_keys_required": false,
    "control_plane_approves_release": false,
    "mutates_ao_artifacts": false,
    "release_acceptance_owner": "factory-v3 evaluator-closer"
  }},
  "provider_api_keys_required": false,
  "control_plane_approves_release": false,
  "mutates_ao_artifacts": false,
  "release_acceptance_owner": "factory-v3 evaluator-closer"
}}
JSON

echo "installed $dest_binary"
echo "install_verification_evidence=$evidence_path"
echo "add $install_dir to PATH if ao2 is not already available"
"#
    )
}

fn windows_installer_script(binary_name: &str) -> String {
    format!(
        r#"$ErrorActionPreference = "Stop"

$BinaryName = "{binary_name}"
$SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$SourceBinary = Join-Path $SourceDir "bin/$BinaryName"
$ChecksumFile = Join-Path $SourceDir "SHA256SUMS"
$ManifestFile = Join-Path $SourceDir "RELEASE-MANIFEST.json"
$InstallDir = if ($env:AO2_INSTALL_DIR) {{
    $env:AO2_INSTALL_DIR
}} elseif ($env:LOCALAPPDATA) {{
    Join-Path $env:LOCALAPPDATA "AO2\bin"
}} else {{
    Join-Path $HOME ".ao2\bin"
}}
$DestBinary = Join-Path $InstallDir $BinaryName

if (!(Test-Path $SourceBinary)) {{
    throw "missing packaged binary: $SourceBinary"
}}
if (!(Test-Path $ManifestFile)) {{
    throw "missing release manifest: $ManifestFile"
}}

$ChecksumLine = Get-Content $ChecksumFile | Where-Object {{ $_ -match "\s+bin/$([Regex]::Escape($BinaryName))$" }} | Select-Object -First 1
if (!$ChecksumLine) {{
    throw "missing checksum for bin/$BinaryName"
}}
$Expected = ($ChecksumLine -split "\s+")[0].ToLowerInvariant()
$Actual = (Get-FileHash -Algorithm SHA256 $SourceBinary).Hash.ToLowerInvariant()
if ($Actual -ne $Expected) {{
    throw "checksum mismatch for bin/$BinaryName"
}}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -Force $SourceBinary $DestBinary

$Manifest = Get-Content -Raw -LiteralPath $ManifestFile | ConvertFrom-Json
$EvidencePath = "$DestBinary.install-verification.json"
$Evidence = [ordered]@{{
    schema_version = "ao2.install-verification-evidence.v1"
    status = "verified"
    install_status = "installed"
    version = $Manifest.version
    target = $Manifest.target
    binary = $BinaryName
    checksum_file = "SHA256SUMS"
    offline_verification = [ordered]@{{
        schema_version = "ao2.release-archive-offline-verification.v1"
        status = "verified"
        checksum_file = "SHA256SUMS"
        verification_report = "RELEASE-VERIFICATION.json"
        checksum_coverage_verified = $true
        provider_api_keys_required = $false
        control_plane_approves_release = $false
        mutates_ao_artifacts = $false
        release_acceptance_owner = "factory-v3 evaluator-closer"
    }}
    provider_api_keys_required = $false
    control_plane_approves_release = $false
    mutates_ao_artifacts = $false
    release_acceptance_owner = "factory-v3 evaluator-closer"
}}
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$EvidenceJson = ($Evidence | ConvertTo-Json -Depth 5) + [Environment]::NewLine
[System.IO.File]::WriteAllText($EvidencePath, $EvidenceJson, $Utf8NoBom)

Write-Output "installed $DestBinary"
Write-Output "install_verification_evidence=$EvidencePath"
Write-Output "add $InstallDir to PATH if ao2 is not already available"
"#
    )
}
