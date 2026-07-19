#!/bin/sh
set -eu

AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_MACOS_ARCHIVE="${AO2_MACOS_ARCHIVE:-dist/ao2-$AO2_VERSION-macos-aarch64.tar.gz}"
AO2_LINUX_ARCHIVE="${AO2_LINUX_ARCHIVE:-dist-linux/ao2-$AO2_VERSION-linux-aarch64.tar.gz}"
AO2_LINUX_X86_64_ARCHIVE="${AO2_LINUX_X86_64_ARCHIVE:-dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz}"
AO2_LINUX_X86_64_SMOKE_MODE="${AO2_LINUX_X86_64_SMOKE_MODE:-remote}"
AO2_LINUX_X86_64_DOCKER_LOG="${AO2_LINUX_X86_64_DOCKER_LOG:-}"
AO2_UBUNTU_SSH_TARGET="${AO2_UBUNTU_SSH_TARGET:-ao2-ubuntu-nucx}"
AO2_WINDOWS_ARCHIVE="${AO2_WINDOWS_ARCHIVE:-dist-windows/ao2-$AO2_VERSION-windows-x86_64.tar.gz}"
AO2_RELEASE_PROVENANCE_DIR="${AO2_RELEASE_PROVENANCE_DIR:-dist-provenance}"
AO2_UBUNTU_IMAGE="${AO2_UBUNTU_IMAGE:-ubuntu:24.04}"
AO2_SMOKE_ROOT="${AO2_SMOKE_ROOT:-$PWD/target/release-smoke/$(date +%Y%m%d%H%M%S)}"
AO2_RELEASE_SMOKE_LEG="${AO2_RELEASE_SMOKE_LEG:-all}"
AO2_RELEASE_SMOKE_JSON="${AO2_RELEASE_SMOKE_JSON:-}"
AO2_RELEASE_ROLLBACK_VERIFY="${AO2_RELEASE_ROLLBACK_VERIFY:-1}"

macos_status="skipped"
macos_install_verification_evidence=""
ubuntu_status="skipped"
ubuntu_install_verification_evidence=""
linux_x86_64_status="skipped"
windows_static_status="skipped"
windows_installer_status="skipped"
windows_install_verification_evidence=""

case "$AO2_RELEASE_SMOKE_LEG" in
  macos|ubuntu|linux_x86_64|windows_static|all) ;;
  *)
    echo "invalid AO2_RELEASE_SMOKE_LEG: $AO2_RELEASE_SMOKE_LEG (expected macos|ubuntu|linux_x86_64|windows_static|all)" >&2
    exit 2
    ;;
esac

case "$AO2_LINUX_X86_64_SMOKE_MODE" in
  remote|docker) ;;
  *)
    echo "invalid AO2_LINUX_X86_64_SMOKE_MODE: $AO2_LINUX_X86_64_SMOKE_MODE (expected remote|docker)" >&2
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
    git init -q "$repo"
    git -C "$repo" config user.name "AO2 Test"
    git -C "$repo" config user.email "ao2-test@example.invalid"
    printf "initial\n" > "$repo/src/value.txt"
    git -C "$repo" add -A
    git -C "$repo" commit -q -m fixture
    tar -xzf "$macos_archive_dir/$macos_archive_name" -C "$extract"
    test -f "$extract/RELEASE-MANIFEST.json"
    grep -q '"schema_version": "ao2.release-manifest.v1"' "$extract/RELEASE-MANIFEST.json"
    grep -q '"binary": "ao2"' "$extract/RELEASE-MANIFEST.json"
    AO2_INSTALL_DIR="$install_dir" sh "$extract/install.sh"
    install_evidence="$install_dir/ao2.install-verification.json"
    test -f "$install_evidence"
    grep -q '"schema_version": "ao2.install-verification-evidence.v1"' "$install_evidence"
    grep -q '"status": "verified"' "$install_evidence"
    grep -q '"provider_api_keys_required": false' "$install_evidence"
    grep -q '"control_plane_approves_release": false' "$install_evidence"
    grep -q '"mutates_ao_artifacts": false' "$install_evidence"
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
    approval_count=0
    while grep -q "status=WaitingForApproval" /tmp/ao2-macos-run.out; do
      ticket_id=$(jq -r '
        .approvals[]
        | select(.requested_action == "sandbox:apply" and .status == "pending")
        | .ticket_id
      ' "$repo/.ao2/runs/macos-install-smoke-repair/evidence-pack/evidence-pack.json")
      test -n "$ticket_id"
      "$install_dir/ao2" approve "$ticket_id" \
        --target "$repo" \
        --approver human:release-smoke >/tmp/ao2-macos-approve.out
      grep -q "status=approved" /tmp/ao2-macos-approve.out
      approval_count=$((approval_count + 1))
      test "$approval_count" -le 2
      "$install_dir/ao2" run --resume macos-install-smoke-repair \
        --target "$repo" >/tmp/ao2-macos-run.out
    done
    test "$approval_count" -eq 2
    grep -q "status=Accepted" /tmp/ao2-macos-run.out
    /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY "$install_dir/ao2" replay macos-install-smoke-repair --target "$repo" >/tmp/ao2-macos-replay.json
    grep -q "\"digest_failures\": \\[\\]" /tmp/ao2-macos-replay.json
    test "$(cat "$repo/src/value.txt")" = ok
    printf "macos_evidence=%s\n" "$repo/.ao2/runs/macos-install-smoke-repair/evidence-pack/evidence-pack.json"
    printf "macos_cockpit=%s\n" "$repo/.ao2/runs/macos-install-smoke-repair/cockpit/index.html"
    printf "macos_install_verification_evidence=%s\n" "$install_evidence"
    macos_install_verification_evidence="$install_evidence"
    macos_status="passed"
    echo "macos_install_smoke=passed"
    ;;
  *)
    macos_status="skipped"
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
    export DEBIAN_FRONTEND=noninteractive
    if ! command -v git >/dev/null 2>&1 || ! command -v jq >/dev/null 2>&1; then
      apt-get update >/dev/null
      apt-get install -y --no-install-recommends ca-certificates git jq >/dev/null
      rm -rf /var/lib/apt/lists/*
    fi
    work=/smoke/ubuntu
    extract="$work/extract"
    install_dir="$work/bin"
    repo="$work/repo"
    mkdir -p "$extract" "$repo/src"
    git init -q "$repo"
    git -C "$repo" config user.name "AO2 Test"
    git -C "$repo" config user.email "ao2-test@example.invalid"
    printf "initial\n" > "$repo/src/value.txt"
    git -C "$repo" add -A
    git -C "$repo" commit -q -m fixture
    tar -xzf "/dist/'"$linux_archive_name"'" -C "$extract"
    test -f "$extract/RELEASE-MANIFEST.json"
    grep -q "\"schema_version\": \"ao2.release-manifest.v1\"" "$extract/RELEASE-MANIFEST.json"
    grep -q "\"binary\": \"ao2\"" "$extract/RELEASE-MANIFEST.json"
    AO2_INSTALL_DIR="$install_dir" sh "$extract/install.sh"
    install_evidence="$install_dir/ao2.install-verification.json"
    test -f "$install_evidence"
    grep -q "\"schema_version\": \"ao2.install-verification-evidence.v1\"" "$install_evidence"
    grep -q "\"status\": \"verified\"" "$install_evidence"
    grep -q "\"provider_api_keys_required\": false" "$install_evidence"
    grep -q "\"control_plane_approves_release\": false" "$install_evidence"
    grep -q "\"mutates_ao_artifacts\": false" "$install_evidence"
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
    approval_count=0
    while grep -q "status=WaitingForApproval" /tmp/ao2-run.out; do
      ticket_id=$(jq -r ".approvals[] | select(.requested_action == \"sandbox:apply\" and .status == \"pending\") | .ticket_id" \
        "$repo/.ao2/runs/ubuntu-install-smoke-repair/evidence-pack/evidence-pack.json")
      test -n "$ticket_id"
      "$install_dir/ao2" approve "$ticket_id" \
        --target "$repo" \
        --approver human:release-smoke >/tmp/ao2-approve.out
      grep -q "status=approved" /tmp/ao2-approve.out
      approval_count=$((approval_count + 1))
      test "$approval_count" -le 2
      "$install_dir/ao2" run --resume ubuntu-install-smoke-repair \
        --target "$repo" >/tmp/ao2-run.out
    done
    test "$approval_count" -eq 2
    grep -q "status=Accepted" /tmp/ao2-run.out
    "$install_dir/ao2" replay ubuntu-install-smoke-repair --target "$repo" >/tmp/ao2-replay.json
    grep -q "\"digest_failures\": \\[\\]" /tmp/ao2-replay.json
    test "$(cat "$repo/src/value.txt")" = ok
    printf "ubuntu_evidence=%s\n" "$repo/.ao2/runs/ubuntu-install-smoke-repair/evidence-pack/evidence-pack.json"
    printf "ubuntu_cockpit=%s\n" "$repo/.ao2/runs/ubuntu-install-smoke-repair/cockpit/index.html"
  '
printf "ubuntu_host_evidence=%s\n" "$AO2_SMOKE_ROOT/ubuntu/repo/.ao2/runs/ubuntu-install-smoke-repair/evidence-pack/evidence-pack.json"
printf "ubuntu_host_cockpit=%s\n" "$AO2_SMOKE_ROOT/ubuntu/repo/.ao2/runs/ubuntu-install-smoke-repair/cockpit/index.html"
ubuntu_install_verification_evidence="$AO2_SMOKE_ROOT/ubuntu/bin/ao2.install-verification.json"
test -f "$ubuntu_install_verification_evidence"
grep -q '"schema_version": "ao2.install-verification-evidence.v1"' "$ubuntu_install_verification_evidence"
printf "ubuntu_install_verification_evidence=%s\n" "$ubuntu_install_verification_evidence"
ubuntu_status="passed"
echo "ubuntu_install_smoke=passed"
fi

if should_run_release_smoke_leg linux_x86_64; then
linux_x86_64_archive_name=$(basename "$AO2_LINUX_X86_64_ARCHIVE")
linux_x86_64_archive_dir=$(CDPATH= cd -- "$(dirname -- "$AO2_LINUX_X86_64_ARCHIVE")" && pwd)
case "$AO2_LINUX_X86_64_SMOKE_MODE" in
  remote)
    echo "== Native Ubuntu x86_64 install and scripted repair smoke =="
    linux_x86_64_log="$AO2_SMOKE_ROOT/linux-x86_64-remote.log"
    AO2_LINUX_X86_64_ARCHIVE="$linux_x86_64_archive_dir/$linux_x86_64_archive_name" \
    AO2_UBUNTU_SSH_TARGET="$AO2_UBUNTU_SSH_TARGET" \
    AO2_LINUX_X86_64_REMOTE_LOG="$linux_x86_64_log" \
    AO2_RELEASE_ROLLBACK_VERIFY="$AO2_RELEASE_ROLLBACK_VERIFY" \
    scripts/smoke-linux-release-remote.sh
    grep -q "linux_x86_64_remote_smoke=passed" "$linux_x86_64_log"
    printf "linux_x86_64_remote_log=%s\n" "$linux_x86_64_log"
    ;;
  docker)
    echo "== Docker Linux x86_64 install and scripted repair smoke =="
    linux_x86_64_log="${AO2_LINUX_X86_64_DOCKER_LOG:-$AO2_SMOKE_ROOT/linux-x86_64-docker.log}"
    AO2_LINUX_X86_64_ARCHIVE="$linux_x86_64_archive_dir/$linux_x86_64_archive_name" \
    AO2_LINUX_X86_64_DOCKER_LOG="$linux_x86_64_log" \
    AO2_LINUX_X86_64_SMOKE_ROOT="$AO2_SMOKE_ROOT/linux-x86_64-docker" \
    AO2_LINUX_X86_64_IMAGE="$AO2_UBUNTU_IMAGE" \
    AO2_RELEASE_ROLLBACK_VERIFY="$AO2_RELEASE_ROLLBACK_VERIFY" \
    scripts/smoke-linux-release-docker.sh
    grep -q "linux_x86_64_docker_smoke=passed" "$linux_x86_64_log"
    if [ "$AO2_RELEASE_ROLLBACK_VERIFY" = "1" ]; then
      grep -q "linux_x86_64_install_rollback=passed" "$linux_x86_64_log"
    fi
    printf "linux_x86_64_docker_log=%s\n" "$linux_x86_64_log"
    ;;
esac
linux_x86_64_status="passed"
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
  windows_install_verification_evidence="$install_dir/ao2.exe.install-verification.json"
  test -f "$windows_install_verification_evidence"
  grep -q '"schema_version":  "ao2.install-verification-evidence.v1"' "$windows_install_verification_evidence" || grep -q '"schema_version": "ao2.install-verification-evidence.v1"' "$windows_install_verification_evidence"
  printf "windows_install_verification_evidence=%s\n" "$windows_install_verification_evidence"
  windows_installer_status="passed"
  echo "windows_installer_pwsh=passed"
else
  windows_installer_status="skipped"
  echo "windows_installer_pwsh=skipped (pwsh unavailable on this host)"
fi
printf "windows_archive=%s\n" "$windows_archive_dir/$windows_archive_name"
windows_static_status="passed"
echo "windows_static_smoke=passed"
fi

echo "release archive smoke completed leg=$AO2_RELEASE_SMOKE_LEG"

if [ -n "$AO2_RELEASE_SMOKE_JSON" ]; then
  mkdir -p "$(dirname -- "$AO2_RELEASE_SMOKE_JSON")"
  export AO2_RELEASE_SMOKE_JSON
  export AO2_RELEASE_SMOKE_LEG AO2_SMOKE_ROOT
  export macos_status macos_install_verification_evidence
  export ubuntu_status ubuntu_install_verification_evidence
  export linux_x86_64_status AO2_LINUX_X86_64_SMOKE_MODE
  export windows_static_status windows_installer_status windows_install_verification_evidence
  python3 - <<'PY'
import json
import os
from pathlib import Path

out = Path(os.environ["AO2_RELEASE_SMOKE_JSON"])
summary = {
    "schema_version": "ao2.release-archive-smoke.v1",
    "status": "passed",
    "release_smoke_leg": os.environ["AO2_RELEASE_SMOKE_LEG"],
    "smoke_root": os.environ["AO2_SMOKE_ROOT"],
    "legs": {
        "macos": {
            "status": os.environ["macos_status"],
            "install_verification_evidence": os.environ["macos_install_verification_evidence"],
        },
        "ubuntu": {
            "status": os.environ["ubuntu_status"],
            "install_verification_evidence": os.environ["ubuntu_install_verification_evidence"],
        },
        "linux_x86_64": {
            "status": os.environ["linux_x86_64_status"],
            "smoke_mode": os.environ["AO2_LINUX_X86_64_SMOKE_MODE"],
        },
        "windows_static": {
            "status": os.environ["windows_static_status"],
            "installer_status": os.environ["windows_installer_status"],
            "install_verification_evidence": os.environ["windows_install_verification_evidence"],
        },
    },
    "install_verification_schema": "ao2.install-verification-evidence.v1",
    "provider_api_keys_required": False,
    "control_plane_approves_release": False,
    "mutates_ao_artifacts": False,
    "release_acceptance_owner": "factory-v3 evaluator-closer",
}
out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
  printf "release_archive_smoke_json=%s\n" "$AO2_RELEASE_SMOKE_JSON"
fi
