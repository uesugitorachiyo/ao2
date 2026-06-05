#!/bin/sh
set -eu

# ao2-release-provenance.json.sig
# release_provenance_verify=passed is emitted by the native AO2 command.

AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
if [ "${AO2_MACOS_ARCHIVE+x}" != "x" ]; then
  AO2_MACOS_ARCHIVE="dist/ao2-$AO2_VERSION-macos-aarch64.tar.gz"
  if [ ! -f "$AO2_MACOS_ARCHIVE" ]; then
    AO2_MACOS_ARCHIVE=""
  fi
fi
AO2_LINUX_ARCHIVE="${AO2_LINUX_ARCHIVE:-dist-linux/ao2-$AO2_VERSION-linux-aarch64.tar.gz}"
AO2_LINUX_X86_64_ARCHIVE="${AO2_LINUX_X86_64_ARCHIVE:-dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz}"
AO2_WINDOWS_ARCHIVE="${AO2_WINDOWS_ARCHIVE:-dist-windows/ao2-$AO2_VERSION-windows-x86_64.tar.gz}"
AO2_RELEASE_PROVENANCE_DIR="${AO2_RELEASE_PROVENANCE_DIR:-dist-provenance}"
AO2_RELEASE_PUBLIC_KEY="${AO2_RELEASE_PUBLIC_KEY:-$AO2_RELEASE_PROVENANCE_DIR/ao2-release-signing-public.pem}"

run_ao2() {
  if [ -n "${AO2_BIN:-}" ]; then
    "$AO2_BIN" "$@"
  elif [ -x target/release/ao2 ]; then
    target/release/ao2 "$@"
  elif [ -x target/debug/ao2 ]; then
    target/debug/ao2 "$@"
  elif [ -x ../../target/debug/ao2 ]; then
    ../../target/debug/ao2 "$@"
  else
    cargo run --quiet -p ao2-cli -- "$@"
  fi
}

set -- release verify-provenance
if [ -n "$AO2_MACOS_ARCHIVE" ]; then
  set -- "$@" --macos-archive "$AO2_MACOS_ARCHIVE"
fi
set -- "$@" \
  --linux-archive "$AO2_LINUX_ARCHIVE" \
  --linux-x86-64-archive "$AO2_LINUX_X86_64_ARCHIVE" \
  --windows-archive "$AO2_WINDOWS_ARCHIVE" \
  --provenance-dir "$AO2_RELEASE_PROVENANCE_DIR" \
  --public-key "$AO2_RELEASE_PUBLIC_KEY"

run_ao2 "$@"
