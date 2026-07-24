use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::install_paths::make_executable;

pub(crate) fn write_release_verifier_scripts(stage_dir: &Path) -> Result<()> {
    let verify_sh = stage_dir.join("verify-release.sh");
    fs::write(&verify_sh, unix_release_verifier_script())?;
    make_executable(&verify_sh)?;
    fs::write(
        stage_dir.join("Verify-Release.ps1"),
        windows_release_verifier_script(),
    )?;
    Ok(())
}

fn unix_release_verifier_script() -> &'static str {
    r#"#!/bin/sh
set -eu

source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
checksum_file="$source_dir/SHA256SUMS"
manifest_file="$source_dir/RELEASE-MANIFEST.json"
report_file="$source_dir/RELEASE-VERIFICATION.json"

if [ ! -f "$checksum_file" ]; then
  echo "missing checksum file: $checksum_file" >&2
  exit 1
fi
if [ ! -f "$manifest_file" ]; then
  echo "missing release manifest: $manifest_file" >&2
  exit 1
fi
if [ ! -f "$report_file" ]; then
  echo "missing release verification report: $report_file" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$source_dir" && sha256sum -c SHA256SUMS >/dev/null)
elif command -v shasum >/dev/null 2>&1; then
  (cd "$source_dir" && shasum -a 256 -c SHA256SUMS >/dev/null)
else
  echo "sha256sum or shasum is required to verify the release archive" >&2
  exit 1
fi

printf '{\n'
printf '  "schema_version": "ao2.release-archive-offline-verification.v1",\n'
printf '  "status": "verified",\n'
printf '  "checksum_file": "SHA256SUMS",\n'
printf '  "manifest": "RELEASE-MANIFEST.json",\n'
printf '  "verification_report": "RELEASE-VERIFICATION.json"\n'
printf '}\n'
"#
}

fn windows_release_verifier_script() -> &'static str {
    r#"$ErrorActionPreference = "Stop"

$SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ChecksumFile = Join-Path $SourceDir "SHA256SUMS"
$ManifestFile = Join-Path $SourceDir "RELEASE-MANIFEST.json"
$ReportFile = Join-Path $SourceDir "RELEASE-VERIFICATION.json"

if (!(Test-Path $ChecksumFile)) {
    throw "missing checksum file: $ChecksumFile"
}
if (!(Test-Path $ManifestFile)) {
    throw "missing release manifest: $ManifestFile"
}
if (!(Test-Path $ReportFile)) {
    throw "missing release verification report: $ReportFile"
}

Get-Content $ChecksumFile | ForEach-Object {
    $Line = $_.Trim()
    if (!$Line) {
        return
    }
    $Parts = $Line -split "\s+", 2
    if ($Parts.Length -ne 2) {
        throw "invalid SHA256SUMS line: $Line"
    }
    $Expected = $Parts[0].ToLowerInvariant()
    $RelativePath = $Parts[1].Trim()
    if ([IO.Path]::IsPathRooted($RelativePath) -or $RelativePath.Contains("..")) {
        throw "unsafe SHA256SUMS path: $RelativePath"
    }
    $Path = Join-Path $SourceDir $RelativePath
    if (!(Test-Path $Path)) {
        throw "missing checksummed file: $RelativePath"
    }
    $Actual = (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) {
        throw "checksum mismatch for $RelativePath"
    }
}

[ordered]@{
    schema_version = "ao2.release-archive-offline-verification.v1"
    status = "verified"
    checksum_file = "SHA256SUMS"
    manifest = "RELEASE-MANIFEST.json"
    verification_report = "RELEASE-VERIFICATION.json"
} | ConvertTo-Json -Depth 3
"#
}
