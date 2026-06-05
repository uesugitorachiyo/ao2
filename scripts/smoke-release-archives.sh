#!/bin/sh
set -eu

AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_MACOS_ARCHIVE="${AO2_MACOS_ARCHIVE:-dist/ao2-$AO2_VERSION-macos-aarch64.tar.gz}"
AO2_LINUX_ARCHIVE="${AO2_LINUX_ARCHIVE:-dist-linux/ao2-$AO2_VERSION-linux-aarch64.tar.gz}"
AO2_LINUX_X86_64_ARCHIVE="${AO2_LINUX_X86_64_ARCHIVE:-dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz}"
AO2_UBUNTU_SSH_TARGET="${AO2_UBUNTU_SSH_TARGET:-ao2-ubuntu-nucx}"
AO2_WINDOWS_ARCHIVE="${AO2_WINDOWS_ARCHIVE:-dist-windows/ao2-$AO2_VERSION-windows-x86_64.tar.gz}"
AO2_RELEASE_PROVENANCE_DIR="${AO2_RELEASE_PROVENANCE_DIR:-dist-provenance}"
AO2_UBUNTU_IMAGE="${AO2_UBUNTU_IMAGE:-ubuntu:24.04}"
AO2_SMOKE_ROOT="${AO2_SMOKE_ROOT:-$PWD/target/release-smoke/$(date +%Y%m%d%H%M%S)}"
AO2_RELEASE_SMOKE_LEG="${AO2_RELEASE_SMOKE_LEG:-all}"

case "$AO2_RELEASE_SMOKE_LEG" in
  macos|ubuntu|linux_x86_64|windows_static|all) ;;
  *)
    echo "invalid AO2_RELEASE_SMOKE_LEG: $AO2_RELEASE_SMOKE_LEG (expected macos|ubuntu|linux_x86_64|windows_static|all)" >&2
    exit 2
    ;;
esac

should_run_release_smoke_leg() {
  [ "$AO2_RELEASE_SMOKE_LEG" = "all" ] || [ "$AO2_RELEASE_SMOKE_LEG" = "$1" ]
}

if should_run_release_smoke_leg macos && [ ! -f "$AO2_MACOS_ARCHIVE" ]; then
  echo "missing macOS release archive: $AO2_MACOS_ARCHIVE" >&2
  exit 1
fi

if should_run_release_smoke_leg ubuntu && [ ! -f "$AO2_LINUX_ARCHIVE" ]; then
  echo "missing Linux release archive: $AO2_LINUX_ARCHIVE" >&2
  exit 1
fi

if should_run_release_smoke_leg linux_x86_64 && [ ! -f "$AO2_LINUX_X86_64_ARCHIVE" ]; then
  echo "missing Linux x86_64 release archive: $AO2_LINUX_X86_64_ARCHIVE" >&2
  exit 1
fi

if should_run_release_smoke_leg windows_static && [ ! -f "$AO2_WINDOWS_ARCHIVE" ]; then
  echo "missing Windows release archive: $AO2_WINDOWS_ARCHIVE" >&2
  exit 1
fi

mkdir -p "$AO2_SMOKE_ROOT"
AO2_SMOKE_ROOT=$(CDPATH= cd -- "$AO2_SMOKE_ROOT" && pwd)
echo "smoke_root=$AO2_SMOKE_ROOT"
echo "release_smoke_leg=$AO2_RELEASE_SMOKE_LEG"

if [ "$AO2_RELEASE_SMOKE_LEG" = "all" ] && [ -d "$AO2_RELEASE_PROVENANCE_DIR" ]; then
  echo "== Release provenance signature smoke =="
  AO2_RELEASE_PROVENANCE_DIR="$AO2_RELEASE_PROVENANCE_DIR" sh scripts/release-verify-provenance.sh
elif [ "$AO2_RELEASE_SMOKE_LEG" = "all" ]; then
  echo "release_provenance_verify=skipped (missing $AO2_RELEASE_PROVENANCE_DIR)"
fi

if should_run_release_smoke_leg macos; then
macos_archive_name=$(basename "$AO2_MACOS_ARCHIVE")
macos_archive_dir=$(CDPATH= cd -- "$(dirname -- "$AO2_MACOS_ARCHIVE")" && pwd)
echo "== macOS install and scripted repair smoke =="
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64|Darwin-aarch64)
    work="$AO2_SMOKE_ROOT/macos"
    extract="$work/extract"
    install_dir="$work/bin"
    repo="$work/repo"
    mkdir -p "$extract" "$repo/src"
    tar -xzf "$macos_archive_dir/$macos_archive_name" -C "$extract"
    test -f "$extract/RELEASE-MANIFEST.json"
    grep -q '"schema_version": "ao2.release-manifest.v1"' "$extract/RELEASE-MANIFEST.json"
    grep -q '"binary": "ao2"' "$extract/RELEASE-MANIFEST.json"
    AO2_INSTALL_DIR="$install_dir" sh "$extract/install.sh"
    "$install_dir/ao2" --help >/dev/null
    "$install_dir/ao2" version --json >/dev/null
    "$install_dir/ao2" adapter doctor --provider scripted >/dev/null
    "$install_dir/ao2" provider matrix --json >/dev/null
    "$install_dir/ao2" provider contract --verify --require codex --json >"$work/provider-contract-verify.json"
    grep -q '"schema": "ao2.provider-contract-verification.v1"' "$work/provider-contract-verify.json"
    grep -q '"status": "verified"' "$work/provider-contract-verify.json"
    echo "provider_contract_verify=passed"
    cat > "$work/workflow.yaml" <<'YAML'
id: macos-install-smoke-repair
version: smoke
template_kind: real_project
objective: Verify installed AO2 can run a scripted real-project repair on macOS.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test "$(cat src/value.txt)" = ok
acceptance:
  - Installed AO2 runs a scripted repair.
  - Replay has zero digest failures.
YAML
    cat > "$work/prompt.sh" <<'SH'
mkdir -p src
if [ -n "${AO2_REPAIR_VERIFIER_OUTPUT:-}" ]; then
  printf "ok\n" > src/value.txt
else
  printf "bad\n" > src/value.txt
fi
printf "Summary: wrote repair-aware smoke value\n"
printf "Changed files: src/value.txt\n"
SH
    /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY "$install_dir/ao2" run "$work/workflow.yaml" \
      --target "$repo" \
      --run-id macos-install-smoke-repair \
      --provider scripted \
      --provider-prompt-file "$work/prompt.sh" \
      --max-repair-attempts 1 >/tmp/ao2-macos-run.out
    grep -q "status=Accepted" /tmp/ao2-macos-run.out
    /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY "$install_dir/ao2" replay macos-install-smoke-repair --target "$repo" >/tmp/ao2-macos-replay.json
    grep -q "\"digest_failures\": \\[\\]" /tmp/ao2-macos-replay.json
    test "$(cat "$repo/src/value.txt")" = ok
    printf "macos_evidence=%s\n" "$repo/.ao2/runs/macos-install-smoke-repair/evidence-pack/evidence-pack.json"
    printf "macos_cockpit=%s\n" "$repo/.ao2/runs/macos-install-smoke-repair/cockpit/index.html"
    echo "macos_install_smoke=passed"
    ;;
  *)
    echo "macos_install_smoke=skipped (host is not macOS arm64)"
    ;;
esac
fi

if should_run_release_smoke_leg ubuntu; then
linux_archive_name=$(basename "$AO2_LINUX_ARCHIVE")
linux_archive_dir=$(CDPATH= cd -- "$(dirname -- "$AO2_LINUX_ARCHIVE")" && pwd)
echo "== Ubuntu install and scripted repair smoke =="
docker run --rm \
  -v "$linux_archive_dir":/dist:ro \
  -v "$AO2_SMOKE_ROOT":/smoke \
  "$AO2_UBUNTU_IMAGE" \
  sh -lc '
    set -eu
    work=/smoke/ubuntu
    extract="$work/extract"
    install_dir="$work/bin"
    repo="$work/repo"
    mkdir -p "$extract" "$repo/src"
    tar -xzf "/dist/'"$linux_archive_name"'" -C "$extract"
    test -f "$extract/RELEASE-MANIFEST.json"
    grep -q "\"schema_version\": \"ao2.release-manifest.v1\"" "$extract/RELEASE-MANIFEST.json"
    grep -q "\"binary\": \"ao2\"" "$extract/RELEASE-MANIFEST.json"
    AO2_INSTALL_DIR="$install_dir" sh "$extract/install.sh"
    "$install_dir/ao2" --help >/dev/null
    "$install_dir/ao2" version --json >/dev/null
    "$install_dir/ao2" adapter doctor --provider scripted >/dev/null
    "$install_dir/ao2" provider matrix --json >/dev/null
    "$install_dir/ao2" provider contract --verify --require codex --json >"$work/provider-contract-verify.json"
    grep -q "\"schema\": \"ao2.provider-contract-verification.v1\"" "$work/provider-contract-verify.json"
    grep -q "\"status\": \"verified\"" "$work/provider-contract-verify.json"
    echo "provider_contract_verify=passed"
    cat > "$work/workflow.yaml" <<'"'"'YAML'"'"'
id: ubuntu-install-smoke-repair
version: smoke
template_kind: real_project
objective: Verify installed AO2 can run a scripted real-project repair.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test "$(cat src/value.txt)" = ok
acceptance:
  - Installed AO2 runs a scripted repair.
  - Replay has zero digest failures.
YAML
    cat > "$work/prompt.sh" <<'"'"'SH'"'"'
mkdir -p src
if [ -n "${AO2_REPAIR_VERIFIER_OUTPUT:-}" ]; then
  printf "ok\n" > src/value.txt
else
  printf "bad\n" > src/value.txt
fi
printf "Summary: wrote repair-aware smoke value\n"
printf "Changed files: src/value.txt\n"
SH
    "$install_dir/ao2" run "$work/workflow.yaml" \
      --target "$repo" \
      --run-id ubuntu-install-smoke-repair \
      --provider scripted \
      --provider-prompt-file "$work/prompt.sh" \
      --max-repair-attempts 1 >/tmp/ao2-run.out
    grep -q "status=Accepted" /tmp/ao2-run.out
    "$install_dir/ao2" replay ubuntu-install-smoke-repair --target "$repo" >/tmp/ao2-replay.json
    grep -q "\"digest_failures\": \\[\\]" /tmp/ao2-replay.json
    test "$(cat "$repo/src/value.txt")" = ok
    printf "ubuntu_evidence=%s\n" "$repo/.ao2/runs/ubuntu-install-smoke-repair/evidence-pack/evidence-pack.json"
    printf "ubuntu_cockpit=%s\n" "$repo/.ao2/runs/ubuntu-install-smoke-repair/cockpit/index.html"
  '
printf "ubuntu_host_evidence=%s\n" "$AO2_SMOKE_ROOT/ubuntu/repo/.ao2/runs/ubuntu-install-smoke-repair/evidence-pack/evidence-pack.json"
printf "ubuntu_host_cockpit=%s\n" "$AO2_SMOKE_ROOT/ubuntu/repo/.ao2/runs/ubuntu-install-smoke-repair/cockpit/index.html"
echo "ubuntu_install_smoke=passed"
fi

if should_run_release_smoke_leg linux_x86_64; then
linux_x86_64_archive_name=$(basename "$AO2_LINUX_X86_64_ARCHIVE")
linux_x86_64_archive_dir=$(CDPATH= cd -- "$(dirname -- "$AO2_LINUX_X86_64_ARCHIVE")" && pwd)
echo "== Native Ubuntu x86_64 install and scripted repair smoke =="
linux_x86_64_log="$AO2_SMOKE_ROOT/linux-x86_64-remote.log"
AO2_LINUX_X86_64_ARCHIVE="$linux_x86_64_archive_dir/$linux_x86_64_archive_name" \
AO2_UBUNTU_SSH_TARGET="$AO2_UBUNTU_SSH_TARGET" \
AO2_LINUX_X86_64_REMOTE_LOG="$linux_x86_64_log" \
  scripts/smoke-linux-release-remote.sh
grep -q "linux_x86_64_remote_smoke=passed" "$linux_x86_64_log"
printf "linux_x86_64_remote_log=%s\n" "$linux_x86_64_log"
fi

if should_run_release_smoke_leg windows_static; then
echo "== Windows archive and installer static smoke =="
windows_archive_name=$(basename "$AO2_WINDOWS_ARCHIVE")
windows_archive_dir=$(CDPATH= cd -- "$(dirname -- "$AO2_WINDOWS_ARCHIVE")" && pwd)
work="$AO2_SMOKE_ROOT/windows"
mkdir -p "$work"
tar -xzf "$windows_archive_dir/$windows_archive_name" -C "$work"
test -f "$work/install.ps1"
test -f "$work/bin/ao2.exe"
test -f "$work/RELEASE-MANIFEST.json"
grep -q '"schema_version": "ao2.release-manifest.v1"' "$work/RELEASE-MANIFEST.json"
grep -q '"binary": "ao2.exe"' "$work/RELEASE-MANIFEST.json"
grep -q "bin/ao2.exe" "$work/SHA256SUMS"
expected=$(awk '$2 == "bin/ao2.exe" { print $1 }' "$work/SHA256SUMS")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$work/bin/ao2.exe" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$work/bin/ao2.exe" | awk '{ print $1 }')
fi
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for Windows archive binary" >&2
  exit 1
fi
if command -v pwsh >/dev/null 2>&1; then
  install_dir="$work/install-bin"
  (cd "$work" && AO2_INSTALL_DIR="$install_dir" pwsh -NoProfile -ExecutionPolicy Bypass -File ./install.ps1 >/dev/null)
  test -f "$install_dir/ao2.exe"
  echo "windows_installer_pwsh=passed"
else
  echo "windows_installer_pwsh=skipped (pwsh unavailable on this host)"
fi
printf "windows_archive=%s\n" "$windows_archive_dir/$windows_archive_name"
echo "windows_static_smoke=passed"
fi

echo "release archive smoke completed leg=$AO2_RELEASE_SMOKE_LEG"
