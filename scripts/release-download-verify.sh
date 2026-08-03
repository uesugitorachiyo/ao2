#!/bin/sh
set -eu

AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v$AO2_VERSION}"
AO2_RELEASE_VERSION="${AO2_RELEASE_TAG#v}"
AO2_RELEASE_DOWNLOAD_DIR="${AO2_RELEASE_DOWNLOAD_DIR:-target/release-download/$AO2_RELEASE_TAG}"
AO2_RELEASE_ROLLBACK_VERIFY="${AO2_RELEASE_ROLLBACK_VERIFY:-0}"
AO2_ROLLBACK_SEED_BIN="${AO2_ROLLBACK_SEED_BIN:-target/release/ao2}"
AO2_NATIVE_UBUNTU_DOWNLOAD_VERIFY="${AO2_NATIVE_UBUNTU_DOWNLOAD_VERIFY:-0}"
AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY="${AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY:-0}"
AO2_LINUX_X86_64_SMOKE_MODE="${AO2_LINUX_X86_64_SMOKE_MODE:-remote}"
AO2_UBUNTU_SSH_TARGET="${AO2_UBUNTU_SSH_TARGET:-ao2-ubuntu-nucx}"
AO2_WINDOWS_SSH_TARGET="${AO2_WINDOWS_SSH_TARGET:-win-hp255-via-ubuntu}"
AO2_WINDOWS_SSH_CONNECT_TIMEOUT="${AO2_WINDOWS_SSH_CONNECT_TIMEOUT:-20}"
AO2_WINDOWS_REMOTE_ROOT="${AO2_WINDOWS_REMOTE_ROOT:-C:/ao2-public-test/ao2-release-download-verify}"
AO2_RELEASE_ROLLBACK_SUMMARY="$AO2_RELEASE_DOWNLOAD_DIR/release-rollback-summary.json"

case "$AO2_LINUX_X86_64_SMOKE_MODE" in
  remote|docker) ;;
  *)
    echo "invalid AO2_LINUX_X86_64_SMOKE_MODE: $AO2_LINUX_X86_64_SMOKE_MODE (expected remote|docker)" >&2
    exit 2
    ;;
esac

rm -rf "$AO2_RELEASE_DOWNLOAD_DIR"
mkdir -p "$AO2_RELEASE_DOWNLOAD_DIR"

macos_rollback_status="skipped"
macos_rollback_marker=""
macos_rollback_log=""
ubuntu_rollback_status="skipped"
ubuntu_rollback_marker=""
ubuntu_rollback_log=""
windows_rollback_status="skipped"
windows_rollback_marker=""
windows_rollback_log=""

write_rollback_summary() {
  if [ "$AO2_RELEASE_ROLLBACK_VERIFY" = "1" ] &&
    [ "$macos_rollback_status" = "passed" ] &&
    [ "$ubuntu_rollback_status" = "passed" ] &&
    [ "$windows_rollback_status" = "passed" ]; then
    rollback_status="verified"
  elif [ "$AO2_RELEASE_ROLLBACK_VERIFY" = "1" ]; then
    rollback_status="incomplete"
  else
    rollback_status="not_checked"
  fi

  cat > "$AO2_RELEASE_ROLLBACK_SUMMARY" <<JSON
{
  "schema_version": "ao2.release-rollback-summary.v1",
  "release_tag": "$AO2_RELEASE_TAG",
  "release_repo": "$AO2_RELEASE_REPO",
  "status": "$rollback_status",
  "platforms": {
    "macos-aarch64": {
      "status": "$macos_rollback_status",
      "marker": "$macos_rollback_marker",
      "log": "$macos_rollback_log"
    },
    "linux-x86_64": {
      "status": "$ubuntu_rollback_status",
      "marker": "$ubuntu_rollback_marker",
      "log": "$ubuntu_rollback_log"
    },
    "windows-x86_64": {
      "status": "$windows_rollback_status",
      "marker": "$windows_rollback_marker",
      "log": "$windows_rollback_log"
    }
  }
}
JSON
}

gh release download "$AO2_RELEASE_TAG" --repo "$AO2_RELEASE_REPO" --dir "$AO2_RELEASE_DOWNLOAD_DIR" --clobber

if [ ! -f "$AO2_RELEASE_DOWNLOAD_DIR/SHA256SUMS" ]; then
  echo "missing release checksum manifest: $AO2_RELEASE_DOWNLOAD_DIR/SHA256SUMS" >&2
  exit 1
fi

if command -v shasum >/dev/null 2>&1; then
  (cd "$AO2_RELEASE_DOWNLOAD_DIR" && shasum -a 256 -c SHA256SUMS)
elif command -v sha256sum >/dev/null 2>&1; then
  (cd "$AO2_RELEASE_DOWNLOAD_DIR" && sha256sum -c SHA256SUMS)
else
  echo "missing checksum verifier: shasum or sha256sum required" >&2
  exit 1
fi
printf "release_checksum_verify=passed\n"

release_provenance_status="skipped_missing_public_key"
if [ -f "$AO2_RELEASE_DOWNLOAD_DIR/ao2-release-signing-public.pem" ]; then
  AO2_MACOS_ARCHIVE="$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-macos-aarch64.tar.gz" \
  AO2_LINUX_ARCHIVE="$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-linux-aarch64.tar.gz" \
  AO2_LINUX_X86_64_ARCHIVE="$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-linux-x86_64.tar.gz" \
  AO2_WINDOWS_ARCHIVE="$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-windows-x86_64.tar.gz" \
  AO2_RELEASE_PROVENANCE_DIR="$AO2_RELEASE_DOWNLOAD_DIR" \
    sh scripts/release-verify-provenance.sh
  release_provenance_status="passed"
fi
printf "release_provenance_status=%s\n" "$release_provenance_status"
if [ "$release_provenance_status" = "skipped_missing_public_key" ]; then
  printf "release_provenance_verify=skipped_missing_public_key\n"
fi

if [ "$AO2_NATIVE_UBUNTU_DOWNLOAD_VERIFY" = "1" ]; then
  ubuntu_log="$AO2_RELEASE_DOWNLOAD_DIR/ubuntu-download-verify.log"
  case "$AO2_LINUX_X86_64_SMOKE_MODE" in
    remote)
      AO2_LINUX_X86_64_ARCHIVE="$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-linux-x86_64.tar.gz" \
      AO2_UBUNTU_SSH_TARGET="$AO2_UBUNTU_SSH_TARGET" \
      AO2_LINUX_X86_64_REMOTE_LOG="$ubuntu_log" \
      AO2_RELEASE_ROLLBACK_VERIFY="$AO2_RELEASE_ROLLBACK_VERIFY" \
        sh scripts/smoke-linux-release-remote.sh
      grep -q "linux_x86_64_remote_smoke=passed" "$ubuntu_log"
      ;;
    docker)
      AO2_RELEASE_SMOKE_LEG=linux_x86_64 \
      AO2_SMOKE_ROOT="$AO2_RELEASE_DOWNLOAD_DIR/linux-x86_64-download-smoke" \
      AO2_LINUX_X86_64_ARCHIVE="$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-linux-x86_64.tar.gz" \
      AO2_LINUX_X86_64_SMOKE_MODE=docker \
      AO2_LINUX_X86_64_DOCKER_LOG="$ubuntu_log" \
      AO2_RELEASE_ROLLBACK_VERIFY="$AO2_RELEASE_ROLLBACK_VERIFY" \
        sh scripts/smoke-release-archives.sh
      grep -q "linux_x86_64_docker_smoke=passed" "$ubuntu_log"
      ;;
  esac
  if [ "$AO2_RELEASE_ROLLBACK_VERIFY" = "1" ]; then
    grep -q "linux_x86_64_install_rollback=passed" "$ubuntu_log"
    ubuntu_rollback_status="passed"
    ubuntu_rollback_marker="ubuntu_download_rollback=passed"
    ubuntu_rollback_log="$ubuntu_log"
    printf "ubuntu_download_rollback=passed\n"
  fi
  printf "ubuntu_download_verify_log=%s\n" "$ubuntu_log"
  printf "ubuntu_download_verify=passed\n"
fi

if [ "$AO2_RELEASE_ROLLBACK_VERIFY" = "1" ] && [ "$(uname -s)" = "Darwin" ]; then
  macos_rollback_root="$AO2_RELEASE_DOWNLOAD_DIR/macos-rollback"
  macos_install_dir="$macos_rollback_root/bin"
  rm -rf "$macos_rollback_root"
  mkdir -p "$macos_install_dir"
  if [ ! -x "$AO2_ROLLBACK_SEED_BIN" ]; then
    cargo build --release -p ao2-cli >/dev/null
  fi
  cp "$AO2_ROLLBACK_SEED_BIN" "$macos_install_dir/ao2"
  chmod 755 "$macos_install_dir/ao2"
  # Keep the install-target path cold on Darwin until rollback finishes; macOS
  # can kill a previously executed Mach-O path after that path is overwritten.
  if [ -f "$AO2_RELEASE_DOWNLOAD_DIR/ao2-release-signing-public.pem" ]; then
    "$AO2_ROLLBACK_SEED_BIN" install update \
      --archive "$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-macos-aarch64.tar.gz" \
      --provenance-dir "$AO2_RELEASE_DOWNLOAD_DIR" \
      --install-dir "$macos_install_dir" > "$macos_rollback_root/update.json"
  else
    "$AO2_ROLLBACK_SEED_BIN" install update \
      --archive "$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-macos-aarch64.tar.gz" \
      --public-checksum-manifest "$AO2_RELEASE_DOWNLOAD_DIR/SHA256SUMS" \
      --install-dir "$macos_install_dir" > "$macos_rollback_root/update.json"
  fi
  grep -q '"rollback_binary"' "$macos_rollback_root/update.json"
  "$AO2_ROLLBACK_SEED_BIN" install rollback --install-dir "$macos_install_dir" --target-label macos-aarch64 > "$macos_rollback_root/rollback.json"
  grep -q '"status": "rolled_back"' "$macos_rollback_root/rollback.json"
  "$macos_install_dir/ao2" version --json > "$macos_rollback_root/version-after-rollback.json"
  grep -q '"target": "macos-aarch64"' "$macos_rollback_root/version-after-rollback.json"
  macos_rollback_status="passed"
  macos_rollback_marker="macos_download_rollback=passed"
  macos_rollback_log="$macos_rollback_root/rollback.json"
  printf "macos_download_rollback_runner=%s\n" "$AO2_ROLLBACK_SEED_BIN"
  printf "macos_download_rollback_dir=%s\n" "$macos_rollback_root"
  printf "macos_download_rollback=passed\n"
fi

if [ "$AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY" = "1" ]; then
  windows_log="$AO2_RELEASE_DOWNLOAD_DIR/windows-download-verify.log"
  ssh -o BatchMode=yes -o ConnectTimeout="$AO2_WINDOWS_SSH_CONNECT_TIMEOUT" "$AO2_WINDOWS_SSH_TARGET" \
    "powershell -NoProfile -ExecutionPolicy Bypass -Command \"New-Item -ItemType Directory -Force -Path '$AO2_WINDOWS_REMOTE_ROOT' | Out-Null; if (Test-Path '$AO2_WINDOWS_REMOTE_ROOT/run') { Remove-Item -Recurse -Force '$AO2_WINDOWS_REMOTE_ROOT/run' }\"" > "$windows_log" 2>&1
  scp -o BatchMode=yes -o ConnectTimeout="$AO2_WINDOWS_SSH_CONNECT_TIMEOUT" \
    scripts/smoke-windows-release.ps1 \
    "$AO2_RELEASE_DOWNLOAD_DIR/ao2-$AO2_RELEASE_VERSION-windows-x86_64.tar.gz" \
    "$AO2_WINDOWS_SSH_TARGET:$AO2_WINDOWS_REMOTE_ROOT/" >> "$windows_log" 2>&1
  ssh -o BatchMode=yes -o ConnectTimeout="$AO2_WINDOWS_SSH_CONNECT_TIMEOUT" "$AO2_WINDOWS_SSH_TARGET" \
    "powershell -NoProfile -ExecutionPolicy Bypass -File \"$AO2_WINDOWS_REMOTE_ROOT/smoke-windows-release.ps1\" -Archive \"$AO2_WINDOWS_REMOTE_ROOT/ao2-$AO2_RELEASE_VERSION-windows-x86_64.tar.gz\" -SmokeRoot \"$AO2_WINDOWS_REMOTE_ROOT/run\"" >> "$windows_log" 2>&1
  grep -q "windows_install_smoke=passed" "$windows_log"
  if [ "$AO2_RELEASE_ROLLBACK_VERIFY" = "1" ]; then
    grep -q "windows_install_rollback=passed" "$windows_log"
    windows_rollback_status="passed"
    windows_rollback_marker="windows_download_rollback=passed"
    windows_rollback_log="$windows_log"
    printf "windows_download_rollback=passed\n"
  fi
  printf "windows_download_verify_log=%s\n" "$windows_log"
  printf "windows_download_verify=passed\n"
fi

write_rollback_summary
printf "release_rollback_summary=%s\n" "$AO2_RELEASE_ROLLBACK_SUMMARY"
printf "release_download_dir=%s\n" "$AO2_RELEASE_DOWNLOAD_DIR"
printf "release_download_verify=passed\n"
