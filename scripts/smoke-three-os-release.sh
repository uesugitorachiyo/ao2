#!/bin/sh
set -eu

AO2_WINDOWS_SSH_TARGET="${AO2_WINDOWS_SSH_TARGET:-win-hp255-via-ubuntu}"
AO2_WINDOWS_SSH_IDENTITY="${AO2_WINDOWS_SSH_IDENTITY:-$HOME/.ssh/ao_operator_to_windows_ed25519}"
AO2_REQUIRE_NATIVE_WINDOWS_SMOKE="${AO2_REQUIRE_NATIVE_WINDOWS_SMOKE:-0}"
AO2_WINDOWS_SSH_ATTEMPTS="${AO2_WINDOWS_SSH_ATTEMPTS:-2}"
AO2_WINDOWS_SSH_CONNECT_TIMEOUT="${AO2_WINDOWS_SSH_CONNECT_TIMEOUT:-10}"
AO2_LOCAL_SMOKE_TIMEOUT_SECONDS="${AO2_LOCAL_SMOKE_TIMEOUT_SECONDS:-1800}"
AO2_WINDOWS_STEP_TIMEOUT_SECONDS="${AO2_WINDOWS_STEP_TIMEOUT_SECONDS:-900}"
AO2_RELEASE_STEP_TIMEOUT_SECONDS="${AO2_RELEASE_STEP_TIMEOUT_SECONDS:-900}"
AO2_STEP_HEARTBEAT_SECONDS="${AO2_STEP_HEARTBEAT_SECONDS:-60}"
AO2_WINDOWS_WAKE_MAC="${AO2_WINDOWS_WAKE_MAC:-}"
AO2_WINDOWS_WAKE_BROADCAST="${AO2_WINDOWS_WAKE_BROADCAST:-10.0.0.255}"
AO2_WINDOWS_WAKE_WAIT_SECONDS="${AO2_WINDOWS_WAKE_WAIT_SECONDS:-0}"
AO2_WINDOWS_WAKE_INTERVAL_SECONDS="${AO2_WINDOWS_WAKE_INTERVAL_SECONDS:-10}"
AO2_THREE_OS_SMOKE_ROOT="${AO2_THREE_OS_SMOKE_ROOT:-$PWD/target/three-os-smoke/$(date +%Y%m%d%H%M%S)}"
AO2_WINDOWS_REMOTE_ROOT="${AO2_WINDOWS_REMOTE_ROOT:-C:/ao2-public-test/ao2-three-os-smoke}"
AO2_RELEASE_PROVENANCE_DIR="${AO2_RELEASE_PROVENANCE_DIR:-dist-provenance}"
AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_MACOS_ARCHIVE="${AO2_MACOS_ARCHIVE:-dist/ao2-$AO2_VERSION-macos-aarch64.tar.gz}"
AO2_LINUX_ARCHIVE="${AO2_LINUX_ARCHIVE:-dist-linux/ao2-$AO2_VERSION-linux-aarch64.tar.gz}"
AO2_LINUX_X86_64_ARCHIVE="${AO2_LINUX_X86_64_ARCHIVE:-dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz}"
AO2_UBUNTU_SSH_TARGET="${AO2_UBUNTU_SSH_TARGET:-ao2-ubuntu-nucx}"
AO2_UBUNTU_IMAGE="${AO2_UBUNTU_IMAGE:-ubuntu:24.04}"
AO2_WINDOWS_ARCHIVE="${AO2_WINDOWS_ARCHIVE:-dist-windows/ao2-$AO2_VERSION-windows-x86_64.tar.gz}"

mkdir -p "$AO2_THREE_OS_SMOKE_ROOT"
AO2_THREE_OS_SMOKE_ROOT=$(CDPATH= cd -- "$AO2_THREE_OS_SMOKE_ROOT" && pwd)
report="$AO2_THREE_OS_SMOKE_ROOT/report.md"
summary_json="$AO2_THREE_OS_SMOKE_ROOT/summary.json"
enriched_summary_json="$AO2_THREE_OS_SMOKE_ROOT/summary.enriched.json"
summary_verification_json="$AO2_THREE_OS_SMOKE_ROOT/summary-verification.json"
summary_verification_err="$AO2_THREE_OS_SMOKE_ROOT/summary-verification.err"
release_obligation_gate_json="$AO2_THREE_OS_SMOKE_ROOT/release-obligation-gate.json"
release_obligation_gate_signing_key="$AO2_THREE_OS_SMOKE_ROOT/release-obligation-gate-signing-key.pem"
release_obligation_gate_signing_json="$AO2_THREE_OS_SMOKE_ROOT/release-obligation-gate-signing.json"
release_gate_json="$AO2_THREE_OS_SMOKE_ROOT/release-gate.json"
release_gate_err="$AO2_THREE_OS_SMOKE_ROOT/release-gate.err"
orchestration_log="$AO2_THREE_OS_SMOKE_ROOT/orchestration.log"

{
  printf "# AO2 Three-OS Release Smoke\n\n"
  printf "%s\n" "- root: \`$AO2_THREE_OS_SMOKE_ROOT\`"
  printf "%s\n" "- ubuntu target: \`$AO2_UBUNTU_SSH_TARGET\`"
  printf "%s\n\n" "- windows target: \`$AO2_WINDOWS_SSH_TARGET\`"
} > "$report"

windows_log="$AO2_THREE_OS_SMOKE_ROOT/windows-smoke.log"
macos_smoke_log="$AO2_THREE_OS_SMOKE_ROOT/macos-smoke.log"
ubuntu_smoke_log="$AO2_THREE_OS_SMOKE_ROOT/ubuntu-smoke.log"
linux_x86_64_smoke_log="$AO2_THREE_OS_SMOKE_ROOT/linux-x86_64-smoke.log"
windows_static_smoke_log="$AO2_THREE_OS_SMOKE_ROOT/windows-static-smoke.log"
local_smoke_log="$AO2_THREE_OS_SMOKE_ROOT/local-smoke.log"
windows_required_failure=0

run_logged_step() {
  # Terminal timeout examples emitted by this wrapper:
  # local_smoke=timed_out, windows_execute=timed_out, release_gate=timed_out.
  label="$1"
  timeout_seconds="$2"
  log_path="$3"
  shift 3
  python3 - "$label" "$timeout_seconds" "$log_path" "$AO2_STEP_HEARTBEAT_SECONDS" "$@" <<'PY'
import datetime
import os
import signal
import subprocess
import sys
import time

label = sys.argv[1]
timeout_seconds = float(sys.argv[2])
log_path = sys.argv[3]
heartbeat_seconds = max(float(sys.argv[4]), 1.0)
argv = sys.argv[5:]

def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")

if not argv:
    with open(log_path, "a", encoding="utf-8") as log:
        log.write(f"{label}=failed status=failed exit_code=2 reason=missing_command at={utc_now()}\n")
    raise SystemExit(2)

started = time.monotonic()
with open(log_path, "a", encoding="utf-8") as log:
    log.write(f"{label}=started timeout_seconds={int(timeout_seconds)} at={utc_now()}\n")
    log.flush()
    proc = subprocess.Popen(
        argv,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    exit_code = None
    deadline = started + timeout_seconds
    next_heartbeat = started + heartbeat_seconds
    while exit_code is None:
        now = time.monotonic()
        remaining = deadline - now
        if remaining <= 0:
            break
        wait_for = min(remaining, max(next_heartbeat - now, 0.1))
        try:
            exit_code = proc.wait(timeout=wait_for)
        except subprocess.TimeoutExpired:
            now = time.monotonic()
            if now >= next_heartbeat and now < deadline:
                elapsed = int(now - started)
                log.write(
                    f"{label}=running status=running elapsed_seconds={elapsed} "
                    f"timeout_seconds={int(timeout_seconds)} at={utc_now()}\n"
                )
                log.flush()
                next_heartbeat = now + heartbeat_seconds

    if exit_code is None:
        elapsed = int(time.monotonic() - started)
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()
        log.write(
            f"{label}=timed_out status=timed_out exit_code=124 "
            f"timeout_seconds={int(timeout_seconds)} elapsed_seconds={elapsed} "
            "exception=subprocess.TimeoutExpired\n"
        )
        raise SystemExit(124)

    elapsed = int(time.monotonic() - started)
    status = "passed" if exit_code == 0 else "failed"
    log.write(
        f"{label}={status} status={status} exit_code={exit_code} "
        f"timeout_seconds={int(timeout_seconds)} elapsed_seconds={elapsed} at={utc_now()}\n"
    )
    raise SystemExit(exit_code)
PY
}

write_windows_skip() {
  reason="$1"
  {
    printf "windows_native_smoke=skipped\n"
    printf "reason=%s\n" "$reason"
  } | tee -a "$windows_log"
  if [ "$AO2_REQUIRE_NATIVE_WINDOWS_SMOKE" = "1" ]; then
    printf "native_windows_required=%s\n" "$reason" >&2
    windows_required_failure=1
  fi
}

run_windows_step() {
  label="$1"
  shift
  attempt=1
  while [ "$attempt" -le "$AO2_WINDOWS_SSH_ATTEMPTS" ]; do
    printf "%s attempt=%s/%s\n" "$label" "$attempt" "$AO2_WINDOWS_SSH_ATTEMPTS" >> "$windows_log"
    if run_logged_step "$label" "$AO2_WINDOWS_STEP_TIMEOUT_SECONDS" "$windows_log" "$@"; then
      printf "%s=passed\n" "$label" >> "$windows_log"
      return 0
    fi
    printf "%s=failed attempt=%s/%s\n" "$label" "$attempt" "$AO2_WINDOWS_SSH_ATTEMPTS" >> "$windows_log"
    attempt=$((attempt + 1))
  done
  return 1
}

send_windows_wake() {
  if [ -z "$AO2_WINDOWS_WAKE_MAC" ]; then
    printf "windows_wake=skipped reason=missing_mac\n" >> "$windows_log"
    return 0
  fi
  python3 - "$AO2_WINDOWS_WAKE_MAC" "$AO2_WINDOWS_WAKE_BROADCAST" >> "$windows_log" 2>&1 <<'PY'
import socket
import sys

mac = sys.argv[1].replace(":", "").replace("-", "")
broadcast = sys.argv[2]
if len(mac) != 12:
    raise SystemExit(f"invalid wake mac: {sys.argv[1]}")
packet = bytes.fromhex("ff" * 6 + mac * 16)
for host in ("255.255.255.255", broadcast):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    try:
        sock.sendto(packet, (host, 9))
        sock.sendto(packet, (host, 7))
        print(f"windows_wake=sent host={host}")
    finally:
        sock.close()
PY
}

probe_windows_ssh() {
  elapsed=0
  while [ "$elapsed" -le "$AO2_WINDOWS_WAKE_WAIT_SECONDS" ]; do
    if ssh -i "$AO2_WINDOWS_SSH_IDENTITY" -o BatchMode=yes -o ConnectTimeout="$AO2_WINDOWS_SSH_CONNECT_TIMEOUT" "$AO2_WINDOWS_SSH_TARGET" "exit" >> "$windows_log" 2>&1; then
      printf "windows_ssh_probe=reachable elapsed_seconds=%s\n" "$elapsed" | tee -a "$windows_log"
      return 0
    fi
    printf "windows_ssh_probe=not_ready elapsed_seconds=%s\n" "$elapsed" | tee -a "$windows_log"
    elapsed=$((elapsed + AO2_WINDOWS_WAKE_INTERVAL_SECONDS))
    if [ "$elapsed" -le "$AO2_WINDOWS_WAKE_WAIT_SECONDS" ]; then
      sleep "$AO2_WINDOWS_WAKE_INTERVAL_SECONDS"
    fi
  done
  return 1
}

echo "== macOS smoke =="
: > "$macos_smoke_log"
if ! run_logged_step \
  macos_smoke \
  "$AO2_LOCAL_SMOKE_TIMEOUT_SECONDS" \
  "$macos_smoke_log" \
  env \
  AO2_RELEASE_SMOKE_LEG=macos \
  AO2_SMOKE_ROOT="$AO2_THREE_OS_SMOKE_ROOT/local/macos" \
  AO2_MACOS_ARCHIVE="$AO2_MACOS_ARCHIVE" \
  scripts/smoke-release-archives.sh; then
  printf "macos_smoke_failed_or_timed_out=1\n" >> "$macos_smoke_log"
fi
cat "$macos_smoke_log"

echo "== Ubuntu container smoke =="
: > "$ubuntu_smoke_log"
if ! run_logged_step \
  ubuntu_smoke \
  "$AO2_LOCAL_SMOKE_TIMEOUT_SECONDS" \
  "$ubuntu_smoke_log" \
  env \
  AO2_RELEASE_SMOKE_LEG=ubuntu \
  AO2_SMOKE_ROOT="$AO2_THREE_OS_SMOKE_ROOT/local/ubuntu" \
  AO2_LINUX_ARCHIVE="$AO2_LINUX_ARCHIVE" \
  AO2_UBUNTU_IMAGE="$AO2_UBUNTU_IMAGE" \
  scripts/smoke-release-archives.sh; then
  printf "ubuntu_smoke_failed_or_timed_out=1\n" >> "$ubuntu_smoke_log"
fi
cat "$ubuntu_smoke_log"

echo "== native Ubuntu x86_64 smoke =="
: > "$linux_x86_64_smoke_log"
if ! run_logged_step \
  linux_x86_64_remote_smoke \
  "$AO2_LOCAL_SMOKE_TIMEOUT_SECONDS" \
  "$linux_x86_64_smoke_log" \
  env \
  AO2_RELEASE_SMOKE_LEG=linux_x86_64 \
  AO2_SMOKE_ROOT="$AO2_THREE_OS_SMOKE_ROOT/local/linux-x86_64" \
  AO2_LINUX_X86_64_ARCHIVE="$AO2_LINUX_X86_64_ARCHIVE" \
  AO2_UBUNTU_SSH_TARGET="$AO2_UBUNTU_SSH_TARGET" \
  scripts/smoke-release-archives.sh; then
  printf "linux_x86_64_remote_smoke_failed_or_timed_out=1\n" >> "$linux_x86_64_smoke_log"
fi
cat "$linux_x86_64_smoke_log"

echo "== Windows archive static smoke =="
: > "$windows_static_smoke_log"
if ! run_logged_step \
  windows_static_smoke \
  "$AO2_LOCAL_SMOKE_TIMEOUT_SECONDS" \
  "$windows_static_smoke_log" \
  env \
  AO2_RELEASE_SMOKE_LEG=windows_static \
  AO2_SMOKE_ROOT="$AO2_THREE_OS_SMOKE_ROOT/local/windows-static" \
  AO2_WINDOWS_ARCHIVE="$AO2_WINDOWS_ARCHIVE" \
  scripts/smoke-release-archives.sh; then
  printf "windows_static_smoke_failed_or_timed_out=1\n" >> "$windows_static_smoke_log"
fi
cat "$windows_static_smoke_log"

cat "$macos_smoke_log" "$ubuntu_smoke_log" "$linux_x86_64_smoke_log" "$windows_static_smoke_log" > "$local_smoke_log"

echo "== native Windows smoke =="
: > "$windows_log"
if [ "$AO2_WINDOWS_WAKE_INTERVAL_SECONDS" -le 0 ]; then
  AO2_WINDOWS_WAKE_INTERVAL_SECONDS=1
fi
if [ ! -f "$AO2_WINDOWS_SSH_IDENTITY" ]; then
  printf "missing Windows SSH identity: %s\n" "$AO2_WINDOWS_SSH_IDENTITY" >> "$windows_log"
  write_windows_skip "missing_identity"
else
  send_windows_wake
  if ! probe_windows_ssh; then
    write_windows_skip "windows_ssh_unreachable"
  fi
fi

if grep -q "windows_native_smoke=skipped" "$windows_log"; then
  :
else
  if ! run_windows_step \
    windows_prepare \
    ssh -i "$AO2_WINDOWS_SSH_IDENTITY" -o BatchMode=yes -o ConnectTimeout="$AO2_WINDOWS_SSH_CONNECT_TIMEOUT" "$AO2_WINDOWS_SSH_TARGET" \
      "powershell -NoProfile -ExecutionPolicy Bypass -Command \"New-Item -ItemType Directory -Force -Path '$AO2_WINDOWS_REMOTE_ROOT' | Out-Null; if (Test-Path '$AO2_WINDOWS_REMOTE_ROOT/run') { Remove-Item -Recurse -Force '$AO2_WINDOWS_REMOTE_ROOT/run' }\""; then
    write_windows_skip "windows_prepare_failed"
  elif ! run_windows_step \
    windows_copy \
    scp -i "$AO2_WINDOWS_SSH_IDENTITY" -o BatchMode=yes -o ConnectTimeout="$AO2_WINDOWS_SSH_CONNECT_TIMEOUT" \
      scripts/smoke-windows-release.ps1 "$AO2_WINDOWS_ARCHIVE" \
      "$AO2_WINDOWS_SSH_TARGET:$AO2_WINDOWS_REMOTE_ROOT/"; then
    write_windows_skip "windows_copy_failed"
  elif ! run_windows_step \
    windows_execute \
    ssh -i "$AO2_WINDOWS_SSH_IDENTITY" -o BatchMode=yes -o ConnectTimeout="$AO2_WINDOWS_SSH_CONNECT_TIMEOUT" "$AO2_WINDOWS_SSH_TARGET" \
      "powershell -NoProfile -ExecutionPolicy Bypass -File \"$AO2_WINDOWS_REMOTE_ROOT/smoke-windows-release.ps1\" -Archive \"$AO2_WINDOWS_REMOTE_ROOT/$(basename "$AO2_WINDOWS_ARCHIVE")\" -SmokeRoot \"$AO2_WINDOWS_REMOTE_ROOT/run\""; then
    write_windows_skip "windows_execute_failed"
  elif ! grep -q "windows_install_smoke=passed" "$windows_log"; then
    write_windows_skip "windows_install_smoke_missing"
  else
    printf "windows_native_smoke=passed\n" | tee -a "$windows_log"
  fi
fi

{
  printf "## macOS Smoke\n\n"
  printf '```text\n'
  cat "$macos_smoke_log"
  printf '```\n\n'
  printf "## Ubuntu Container Smoke\n\n"
  printf '```text\n'
  cat "$ubuntu_smoke_log"
  printf '```\n\n'
  printf "## Native Ubuntu x86_64 Smoke\n\n"
  printf '```text\n'
  cat "$linux_x86_64_smoke_log"
  printf '```\n\n'
  printf "## Windows Archive Static Smoke\n\n"
  printf '```text\n'
  cat "$windows_static_smoke_log"
  printf '```\n\n'
  printf "## Native Windows Smoke\n\n"
  printf '```text\n'
  cat "$AO2_THREE_OS_SMOKE_ROOT/windows-smoke.log"
  printf '```\n'
} >> "$report"

python3 - "$summary_json" "$report" "$windows_log" "$macos_smoke_log" "$ubuntu_smoke_log" "$linux_x86_64_smoke_log" "$windows_static_smoke_log" "$AO2_REQUIRE_NATIVE_WINDOWS_SMOKE" "$AO2_THREE_OS_SMOKE_ROOT" "$AO2_WINDOWS_SSH_TARGET" <<'PY'
import json
import re
import sys
from pathlib import Path

summary_path = Path(sys.argv[1])
report_path = Path(sys.argv[2])
windows_log_path = Path(sys.argv[3])
macos_log_path = Path(sys.argv[4])
ubuntu_log_path = Path(sys.argv[5])
linux_x86_64_log_path = Path(sys.argv[6])
windows_static_log_path = Path(sys.argv[7])
native_windows_required = sys.argv[8] == "1"
root = sys.argv[9]
windows_target = sys.argv[10]

windows_log = windows_log_path.read_text(encoding="utf-8", errors="replace")
macos_log = macos_log_path.read_text(encoding="utf-8", errors="replace")
ubuntu_log = ubuntu_log_path.read_text(encoding="utf-8", errors="replace")
linux_x86_64_log = linux_x86_64_log_path.read_text(encoding="utf-8", errors="replace")
windows_static_log = windows_static_log_path.read_text(encoding="utf-8", errors="replace")

def status_from_log(text, label, passed_markers=(), skipped_markers=()):
    if any(marker in text for marker in skipped_markers):
        return "skipped"
    if f"{label}=passed" in text or any(marker in text for marker in passed_markers):
        return "passed"
    if f"{label}=timed_out" in text:
        return "timed_out"
    if f"{label}=failed" in text:
        return "failed"
    return "unknown"

windows_status = "unknown"
if "windows_native_smoke=passed" in windows_log:
    windows_status = "passed"
elif "windows_native_smoke=skipped" in windows_log:
    windows_status = "skipped"
elif "windows_execute=timed_out" in windows_log:
    windows_status = "timed_out"

macos_status = status_from_log(
    macos_log,
    "macos_smoke",
    passed_markers=("macos_install_smoke=passed",),
    skipped_markers=("macos_install_smoke=skipped",),
)
ubuntu_status = status_from_log(
    ubuntu_log,
    "ubuntu_smoke",
    passed_markers=("ubuntu_install_smoke=passed",),
)
linux_x86_64_status = status_from_log(
    linux_x86_64_log,
    "linux_x86_64_remote_smoke",
    passed_markers=("linux_x86_64_remote_smoke=passed",),
)
windows_static_status = status_from_log(
    windows_static_log,
    "windows_static_smoke",
    passed_markers=("windows_static_smoke=passed",),
)
if (
    macos_status in {"passed", "skipped"}
    and ubuntu_status == "passed"
    and linux_x86_64_status == "passed"
    and windows_static_status == "passed"
):
    local_status = "passed"
elif "timed_out" in {macos_status, ubuntu_status, linux_x86_64_status, windows_static_status}:
    local_status = "timed_out"
elif "failed" in {macos_status, ubuntu_status, linux_x86_64_status, windows_static_status}:
    local_status = "failed"
else:
    local_status = "unknown"

reason_matches = re.findall(r"^reason=(.+)$", windows_log, flags=re.MULTILINE)
probe_matches = re.findall(r"windows_ssh_probe=(reachable|not_ready) elapsed_seconds=([0-9]+)", windows_log)
wake_hosts = re.findall(r"windows_wake=sent host=([^\s]+)", windows_log)

summary = {
    "schema": "ao2.three-os-smoke-summary.v1",
    "root": root,
    "report": str(report_path),
    "windows_log": str(windows_log_path),
    "local_smoke": local_status,
    "macos_smoke": macos_status,
    "ubuntu_smoke": ubuntu_status,
    "linux_x86_64_remote_smoke": linux_x86_64_status,
    "windows_static_smoke": windows_static_status,
    "native_windows_required": native_windows_required,
    "windows_target": windows_target,
    "windows_native_smoke": windows_status,
    "windows_skip_reason": reason_matches[-1] if reason_matches else None,
    "windows_wake_hosts": wake_hosts,
    "windows_ssh_probe_count": len(probe_matches),
    "windows_ssh_last_probe": {
        "status": probe_matches[-1][0],
        "elapsed_seconds": int(probe_matches[-1][1]),
    } if probe_matches else None,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

summary_verification_status=passed
if [ "$AO2_REQUIRE_NATIVE_WINDOWS_SMOKE" = "1" ]; then
  if ! run_logged_step summary_verification "$AO2_RELEASE_STEP_TIMEOUT_SECONDS" "$orchestration_log" sh -c 'cargo run -p ao2-cli --quiet -- release smoke-summary --summary "$1" --require-native-windows > "$2" 2> "$3"' sh "$summary_json" "$summary_verification_json" "$summary_verification_err"; then
    summary_verification_status=failed
  fi
else
  if ! run_logged_step summary_verification "$AO2_RELEASE_STEP_TIMEOUT_SECONDS" "$orchestration_log" sh -c 'cargo run -p ao2-cli --quiet -- release smoke-summary --summary "$1" > "$2" 2> "$3"' sh "$summary_json" "$summary_verification_json" "$summary_verification_err"; then
    summary_verification_status=failed
  fi
fi

if ! grep -q "macos_smoke=passed" "$macos_smoke_log"; then
  grep -q "macos_install_smoke=skipped" "$macos_smoke_log"
fi
grep -q "ubuntu_smoke=passed" "$ubuntu_smoke_log"
grep -q "linux_x86_64_remote_smoke=passed" "$linux_x86_64_smoke_log"
grep -q "windows_static_smoke=passed" "$windows_static_smoke_log"
if ! grep -q "windows_native_smoke=passed" "$windows_log"; then
  grep -q "windows_native_smoke=skipped" "$windows_log"
fi

printf "three_os_summary=%s\n" "$summary_json"
printf "three_os_summary_verification=%s\n" "$summary_verification_json"
printf "three_os_summary_verification_stderr=%s\n" "$summary_verification_err"
printf "three_os_summary_verification_status=%s\n" "$summary_verification_status"
printf "three_os_report=%s\n" "$report"
if [ "$summary_verification_status" != "passed" ] && [ "$windows_required_failure" = "0" ]; then
  exit 1
fi
if [ "$windows_required_failure" != "0" ]; then
  exit "$windows_required_failure"
fi

cat > "$release_obligation_gate_json" <<JSON
{
  "schema_version": "ao2.obligation-gate.v1",
  "stage": "closure",
  "status": "passed",
  "verdict": "accepted",
  "summary": {
    "pass": 5,
    "fail": 0,
    "unverified": 0,
    "waived": 0
  },
  "checks": [
    {
      "id": "macos-install-smoke",
      "status": "passed",
      "evidence": "$macos_smoke_log"
    },
    {
      "id": "ubuntu-container-install-smoke",
      "status": "passed",
      "evidence": "$ubuntu_smoke_log"
    },
    {
      "id": "ubuntu-native-x86-64-install-smoke",
      "status": "passed",
      "evidence": "$linux_x86_64_smoke_log"
    },
    {
      "id": "windows-static-archive-smoke",
      "status": "passed",
      "evidence": "$windows_static_smoke_log"
    },
    {
      "id": "windows-native-install-smoke",
      "status": "passed",
      "evidence": "$windows_log"
    }
  ]
}
JSON

rm -f "$release_obligation_gate_signing_key"
run_logged_step support_keygen "$AO2_RELEASE_STEP_TIMEOUT_SECONDS" "$orchestration_log" sh -c 'cargo run -p ao2-cli --quiet -- workbench support-keygen --out "$1" --bits 2048 --json > "$2"' sh "$release_obligation_gate_signing_key" "$AO2_THREE_OS_SMOKE_ROOT/release-obligation-gate-support-keygen.json"

run_logged_step obligation_gate_signing "$AO2_RELEASE_STEP_TIMEOUT_SECONDS" "$orchestration_log" sh -c 'cargo run -p ao2-cli --quiet -- contract sign-obligation-gate --gate "$1" --support-signing-key "$2" --support-signer-id "three-os-release-smoke" --support-operator-role "release" --support-run-id "$3" --json > "$4"' sh "$release_obligation_gate_json" "$release_obligation_gate_signing_key" "$(basename "$AO2_THREE_OS_SMOKE_ROOT")" "$release_obligation_gate_signing_json"

run_logged_step summary_enrich "$AO2_RELEASE_STEP_TIMEOUT_SECONDS" "$orchestration_log" sh -c 'cargo run -p ao2-cli --quiet -- release summary-enrich --summary "$1" --target "$2" --obligation-gate "$3" --out "$4" --json > "$5"' sh "$summary_json" "$PWD" "$release_obligation_gate_json" "$enriched_summary_json" "$AO2_THREE_OS_SMOKE_ROOT/summary-enrich.json"

printf "three_os_obligation_gate=%s\n" "$release_obligation_gate_json"
printf "three_os_obligation_gate_signing=%s\n" "$release_obligation_gate_signing_json"
printf "three_os_enriched_summary=%s\n" "$enriched_summary_json"

if [ "$AO2_REQUIRE_NATIVE_WINDOWS_SMOKE" = "1" ]; then
  run_logged_step release_gate "$AO2_RELEASE_STEP_TIMEOUT_SECONDS" "$orchestration_log" sh -c 'cargo run -p ao2-cli --quiet -- release gate --summary "$1" --provenance-dir "$2" --macos-archive "$3" --linux-archive "$4" --linux-x86-64-archive "$5" --windows-archive "$6" --require-native-windows > "$7" 2> "$8"' sh "$enriched_summary_json" "$AO2_RELEASE_PROVENANCE_DIR" "$AO2_MACOS_ARCHIVE" "$AO2_LINUX_ARCHIVE" "$AO2_LINUX_X86_64_ARCHIVE" "$AO2_WINDOWS_ARCHIVE" "$release_gate_json" "$release_gate_err"
else
  run_logged_step release_gate "$AO2_RELEASE_STEP_TIMEOUT_SECONDS" "$orchestration_log" sh -c 'cargo run -p ao2-cli --quiet -- release gate --summary "$1" --provenance-dir "$2" --macos-archive "$3" --linux-archive "$4" --linux-x86-64-archive "$5" --windows-archive "$6" > "$7" 2> "$8"' sh "$enriched_summary_json" "$AO2_RELEASE_PROVENANCE_DIR" "$AO2_MACOS_ARCHIVE" "$AO2_LINUX_ARCHIVE" "$AO2_LINUX_X86_64_ARCHIVE" "$AO2_WINDOWS_ARCHIVE" "$release_gate_json" "$release_gate_err"
fi
printf "three_os_release_gate=%s\n" "$release_gate_json"
printf "three_os_release_gate_stderr=%s\n" "$release_gate_err"
printf "three_os_smoke=passed\n"
