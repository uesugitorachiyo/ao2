#!/bin/sh
# morning-dispatch-cross-os-readback.sh
#
# Single-command dispatch for the cross-OS readback smoke. It SSHes into
# ao2-ubuntu-nucx and win-hp255-via-ubuntu, uploads working-tree snapshots
# of ao2 + ao2-control-plane, runs the Phase 1 readback smoke on
# each, and captures artifacts under target/morning-cross-os-readback/<ts>/<host>/.
#
# Reason this is staged for `!` execution: the overnight Claude Code session
# could not SSH directly to either remote host (auto-mode classifier blocks
# Production/Remote Shell Reads without explicit per-target authorization).
# Wrapping the dispatch in a host-local script the user `!`-runs preserves
# the explicit-authorization rule while keeping the workflow one keystroke
# away.
#
# Usage:
#   bash scripts/morning-dispatch-cross-os-readback.sh
#
# Env overrides:
#   UBUNTU_HOST   - default ao2-ubuntu-nucx
#   WINDOWS_HOST  - default win-hp255-via-ubuntu
#   AO2_CROSS_OS_REMOTE_BASE - default ao2-cross-os-readback under remote home
#   AO2_REMOTE_ROOT - default $AO2_CROSS_OS_REMOTE_BASE/ao2
#   AO2_CP_REMOTE_ROOT - default $AO2_CROSS_OS_REMOTE_BASE/ao2-control-plane
set -eu

UBUNTU_HOST="${UBUNTU_HOST:-ao2-ubuntu-nucx}"
WINDOWS_HOST="${WINDOWS_HOST:-win-hp255-via-ubuntu}"
AO2_CROSS_OS_REMOTE_BASE="${AO2_CROSS_OS_REMOTE_BASE:-ao2-cross-os-readback}"
AO2_REMOTE_ROOT="${AO2_REMOTE_ROOT:-$AO2_CROSS_OS_REMOTE_BASE/ao2}"
AO2_CP_REMOTE_ROOT="${AO2_CP_REMOTE_ROOT:-$AO2_CROSS_OS_REMOTE_BASE/ao2-control-plane}"

LOCAL_OUT_ROOT="${LOCAL_OUT_ROOT:-/tmp/ao2-public/ao2/target/morning-cross-os-readback/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$LOCAL_OUT_ROOT"
LOCAL_OUT_ROOT=$(CDPATH= cd -- "$LOCAL_OUT_ROOT" && pwd)
AO2_ARCHIVE="$LOCAL_OUT_ROOT/ao2-source.tgz"
AO2_CP_ARCHIVE="$LOCAL_OUT_ROOT/ao2-control-plane-source.tgz"

create_worktree_archive() {
  repo_root="$1"
  archive="$2"
  (
    cd "$repo_root"
    {
      git ls-files -z
      git ls-files --others --exclude-standard -z
    } | tar --null -czf "$archive" -T -
  )
}

create_worktree_archive /tmp/ao2-public/ao2 "$AO2_ARCHIVE"
create_worktree_archive /tmp/ao2-public/ao2-control-plane "$AO2_CP_ARCHIVE"

echo "morning_cross_os_readback_out=$LOCAL_OUT_ROOT"
echo "ao2_archive=$AO2_ARCHIVE"
echo "ao2_control_plane_archive=$AO2_CP_ARCHIVE"

dispatch_unix_host() {
  host="$1"
  out_dir="$LOCAL_OUT_ROOT/$host"
  mkdir -p "$out_dir"
  echo "===dispatching $host (unix)==="
  # Probe first so we get a clean reachability error if SSH is dead.
  if ! ssh -o ConnectTimeout=10 -o BatchMode=yes "$host" 'hostname && uname -a' > "$out_dir/probe.log" 2>&1; then
    echo "$host: SSH probe FAILED" | tee -a "$out_dir/probe.log"
    echo "$host: dispatch_status=ssh_unreachable" > "$out_dir/dispatch_status.txt"
    return 1
  fi

  ssh -o ConnectTimeout=30 "$host" "
    set -eu
    rm -rf \"\$HOME/$AO2_REMOTE_ROOT\" \"\$HOME/$AO2_CP_REMOTE_ROOT\"
    mkdir -p \"\$HOME/$AO2_REMOTE_ROOT\" \"\$HOME/$AO2_CP_REMOTE_ROOT\"
  " > "$out_dir/prepare.log" 2> "$out_dir/prepare.err"
  scp -q "$AO2_ARCHIVE" "$host:$AO2_REMOTE_ROOT/source.tgz" > "$out_dir/scp-ao2.log" 2>&1
  scp -q "$AO2_CP_ARCHIVE" "$host:$AO2_CP_REMOTE_ROOT/source.tgz" > "$out_dir/scp-cp.log" 2>&1

  ssh -o ConnectTimeout=30 "$host" "
    set -eu
    cd \"\$HOME/$AO2_REMOTE_ROOT\"
    tar -xzf source.tgz
    cd \"\$HOME/$AO2_CP_REMOTE_ROOT\"
    tar -xzf source.tgz
    cd \"\$HOME/$AO2_REMOTE_ROOT\"
    export PATH=\"\$HOME/.cargo/bin:\$PATH\"
    [ ! -f \"\$HOME/.cargo/env\" ] || . \"\$HOME/.cargo/env\"
    cargo build --release --bin ao2 --quiet
    AO2_BIN=target/release/ao2 AO2_CONTROL_PLANE_ROOT=\"\$HOME/$AO2_CP_REMOTE_ROOT\" bash scripts/smoke-phase1-control-plane-readback.sh
    AO2_BIN=target/release/ao2 bash scripts/smoke-factory-greenfield-run.sh
    AO2_BIN=target/release/ao2 bash scripts/smoke-factory-app-run.sh
    AO2_BIN=target/release/ao2 bash scripts/smoke-factory-project-run.sh
  " > "$out_dir/readback-smoke.log" 2> "$out_dir/readback-smoke.err"
  rc=$?
  echo "$host: dispatch_rc=$rc" | tee "$out_dir/dispatch_status.txt"

  # Pull back the per-host summary artifact for local inspection
  ssh "$host" "ls -td \"\$HOME/$AO2_REMOTE_ROOT/target/phase1-control-plane-readback/\"* 2>/dev/null | head -1" \
    > "$out_dir/remote_smoke_root.txt" 2>/dev/null || true
  remote_root=$(cat "$out_dir/remote_smoke_root.txt" 2>/dev/null || echo "")
  if [ -n "$remote_root" ]; then
    scp -q "$host:$remote_root/phase1-control-plane-readback-summary.json" \
      "$out_dir/phase1-control-plane-readback-summary.json" 2>/dev/null || true
  fi
  ssh "$host" "ls -td \"\$HOME/$AO2_REMOTE_ROOT/target/factory-greenfield-run-smoke/\"* 2>/dev/null | head -1" \
    > "$out_dir/remote_factory_greenfield_root.txt" 2>/dev/null || true
  remote_greenfield_root=$(cat "$out_dir/remote_factory_greenfield_root.txt" 2>/dev/null || echo "")
  if [ -n "$remote_greenfield_root" ]; then
    scp -q "$host:$remote_greenfield_root/factory-greenfield-run-summary.json" \
      "$out_dir/factory-greenfield-run-summary.json" 2>/dev/null || true
  fi
  ssh "$host" "ls -td \"\$HOME/$AO2_REMOTE_ROOT/target/factory-app-run-smoke/\"* 2>/dev/null | head -1" \
    > "$out_dir/remote_factory_app_root.txt" 2>/dev/null || true
  remote_app_root=$(cat "$out_dir/remote_factory_app_root.txt" 2>/dev/null || echo "")
  if [ -n "$remote_app_root" ]; then
    scp -q "$host:$remote_app_root/factory-app-run-summary.json" \
      "$out_dir/factory-app-run-summary.json" 2>/dev/null || true
  fi
  ssh "$host" "ls -td \"\$HOME/$AO2_REMOTE_ROOT/target/factory-project-run-smoke/\"* 2>/dev/null | head -1" \
    > "$out_dir/remote_factory_project_root.txt" 2>/dev/null || true
  remote_project_root=$(cat "$out_dir/remote_factory_project_root.txt" 2>/dev/null || echo "")
  if [ -n "$remote_project_root" ]; then
    scp -q "$host:$remote_project_root/factory-project-run-summary.json" \
      "$out_dir/factory-project-run-summary.json" 2>/dev/null || true
  fi
  return $rc
}

dispatch_windows_host() {
  host="$1"
  out_dir="$LOCAL_OUT_ROOT/$host"
  mkdir -p "$out_dir"
  echo "===dispatching $host (windows-powershell-git-bash)==="
  if ! ssh -o ConnectTimeout=10 -o BatchMode=yes "$host" 'powershell -NoProfile -Command "hostname; $PSVersionTable.PSVersion.ToString()"' > "$out_dir/probe.log" 2>&1; then
    echo "$host: SSH probe FAILED" | tee -a "$out_dir/probe.log"
    echo "$host: dispatch_status=ssh_unreachable" > "$out_dir/dispatch_status.txt"
    return 1
  fi

  windows_run="$out_dir/windows-readback.ps1"
  ssh -o ConnectTimeout=30 "$host" "powershell -NoProfile -ExecutionPolicy Bypass -Command \"\$ErrorActionPreference = 'Stop'; \$base = Join-Path \$env:USERPROFILE '$AO2_CROSS_OS_REMOTE_BASE'; \$ao2 = Join-Path \$env:USERPROFILE '$AO2_REMOTE_ROOT'; \$cp = Join-Path \$env:USERPROFILE '$AO2_CP_REMOTE_ROOT'; Remove-Item -Recurse -Force \$ao2,\$cp -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force -Path \$base,\$ao2,\$cp | Out-Null\"" > "$out_dir/prepare.log" 2> "$out_dir/prepare.err"

  scp -q "$AO2_ARCHIVE" "$host:$AO2_REMOTE_ROOT/source.tgz" > "$out_dir/scp-ao2.log" 2>&1
  scp -q "$AO2_CP_ARCHIVE" "$host:$AO2_CP_REMOTE_ROOT/source.tgz" > "$out_dir/scp-cp.log" 2>&1

  cat > "$windows_run" <<PS1
\$ErrorActionPreference = "Stop"
\$ao2 = Join-Path \$env:USERPROFILE '$AO2_REMOTE_ROOT'
\$cp = Join-Path \$env:USERPROFILE '$AO2_CP_REMOTE_ROOT'
Set-Location -LiteralPath \$ao2
tar -xzf source.tgz
Set-Location -LiteralPath \$cp
tar -xzf source.tgz
Set-Location -LiteralPath \$ao2
cargo build --release --bin ao2 --quiet
\$env:AO2_BIN = "target/release/ao2.exe"
\$env:AO2_CONTROL_PLANE_ROOT = \$cp
& 'C:\Program Files\Git\bin\bash.exe' scripts/smoke-phase1-control-plane-readback.sh
if (\$LASTEXITCODE -ne 0) { exit \$LASTEXITCODE }
& 'C:\Program Files\Git\bin\bash.exe' scripts/smoke-factory-greenfield-run.sh
if (\$LASTEXITCODE -ne 0) { exit \$LASTEXITCODE }
& 'C:\Program Files\Git\bin\bash.exe' scripts/smoke-factory-app-run.sh
if (\$LASTEXITCODE -ne 0) { exit \$LASTEXITCODE }
& 'C:\Program Files\Git\bin\bash.exe' scripts/smoke-factory-project-run.sh
if (\$LASTEXITCODE -ne 0) { exit \$LASTEXITCODE }
PS1
  scp -q "$windows_run" "$host:$AO2_CROSS_OS_REMOTE_BASE/windows-readback.ps1" > "$out_dir/scp-run.log" 2>&1
  ssh -o ConnectTimeout=30 "$host" "powershell -NoProfile -ExecutionPolicy Bypass -File %USERPROFILE%\\$AO2_CROSS_OS_REMOTE_BASE\\windows-readback.ps1" > "$out_dir/readback-smoke.log" 2> "$out_dir/readback-smoke.err"
  rc=$?
  echo "$host: dispatch_rc=$rc" | tee "$out_dir/dispatch_status.txt"

  scp -q "$host:$AO2_REMOTE_ROOT/target/phase1-control-plane-readback/*/phase1-control-plane-readback-summary.json" \
    "$out_dir/phase1-control-plane-readback-summary.json" 2>/dev/null || true
  scp -q "$host:$AO2_REMOTE_ROOT/target/factory-greenfield-run-smoke/*/factory-greenfield-run-summary.json" \
    "$out_dir/factory-greenfield-run-summary.json" 2>/dev/null || true
  scp -q "$host:$AO2_REMOTE_ROOT/target/factory-app-run-smoke/*/factory-app-run-summary.json" \
    "$out_dir/factory-app-run-summary.json" 2>/dev/null || true
  scp -q "$host:$AO2_REMOTE_ROOT/target/factory-project-run-smoke/*/factory-project-run-summary.json" \
    "$out_dir/factory-project-run-summary.json" 2>/dev/null || true
  return $rc
}

# Run Ubuntu first (faster, more reliable). Don't abort on Ubuntu failure;
# we still want to attempt Windows so the user gets both reports.
overall=0
dispatch_unix_host "$UBUNTU_HOST" || overall=1
dispatch_windows_host "$WINDOWS_HOST" || overall=1

echo "===SUMMARY==="
for host in "$UBUNTU_HOST" "$WINDOWS_HOST"; do
  out_dir="$LOCAL_OUT_ROOT/$host"
  if [ -f "$out_dir/phase1-control-plane-readback-summary.json" ]; then
    echo "$host: artifact_pulled=true"
    python3 -c "
import json
s = json.load(open('$out_dir/phase1-control-plane-readback-summary.json'))
print('  status=' + s.get('status', '?') + ' decision_mode=' + s.get('dashboard_decision_mode', '?') + ' signature_verified=' + str(s.get('signature_verified', '?')))
"
  else
    echo "$host: artifact_pulled=false (dispatch may have failed; see $out_dir/readback-smoke.err)"
  fi
  if [ -f "$out_dir/factory-greenfield-run-summary.json" ]; then
    echo "$host: factory_greenfield_artifact_pulled=true"
    python3 -c "
import json
s = json.load(open('$out_dir/factory-greenfield-run-summary.json'))
print('  factory_greenfield_status=' + s.get('status', '?') + ' schema=' + s.get('factory_greenfield_schema', '?') + ' evaluator_decision_status=' + s.get('evaluator_decision_status', '?'))
"
  else
    echo "$host: factory_greenfield_artifact_pulled=false (see $out_dir/readback-smoke.err)"
  fi
  if [ -f "$out_dir/factory-app-run-summary.json" ]; then
    echo "$host: factory_app_artifact_pulled=true"
    python3 -c "
import json
s = json.load(open('$out_dir/factory-app-run-summary.json'))
print('  factory_app_status=' + s.get('status', '?') + ' product_fixture=' + s.get('product_fixture', '?') + ' schema=' + s.get('factory_app_schema', '?') + ' release_review_artifacts_ready=' + str(s.get('release_review_artifacts_ready', '?')) + ' app_run_bundle_status=' + s.get('app_run_bundle_status', '?') + ' evaluator_decision_status=' + s.get('evaluator_decision_status', '?'))
"
  else
    echo "$host: factory_app_artifact_pulled=false (see $out_dir/readback-smoke.err)"
  fi
  if [ -f "$out_dir/factory-project-run-summary.json" ]; then
    echo "$host: factory_project_artifact_pulled=true"
    python3 -c "
import json
s = json.load(open('$out_dir/factory-project-run-summary.json'))
print('  factory_project_status=' + s.get('status', '?') + ' product_fixture=' + s.get('product_fixture', '?') + ' schema=' + s.get('factory_project_schema', '?') + ' app_run_count=' + str(s.get('app_run_count', '?')) + ' release_review_package_status=' + s.get('release_review_package_status', '?') + ' project_acceptance_review_status=' + s.get('project_acceptance_review_status', '?') + ' project_acceptance_review_signature_status=' + s.get('project_acceptance_review_signature_status', '?') + ' project_start_acceptance_review_status=' + s.get('project_start_acceptance_review_status', '?') + ' queued_project_acceptance_review_status=' + s.get('queued_project_acceptance_review_status', '?') + ' project_start_bundle_verification_status=' + s.get('project_start_bundle_verification_status', '?') + ' queued_project_start_bundle_verification_status=' + s.get('queued_project_start_bundle_verification_status', '?') + ' project_start_operator_summary_status=' + s.get('project_start_operator_summary_status', '?') + ' queued_project_start_operator_summary_status=' + s.get('queued_project_start_operator_summary_status', '?') + ' queued_project_start_queue_status=' + s.get('queued_project_start_queue_status', '?') + ' queued_project_start_queue_status_schema=' + s.get('queued_project_start_queue_status_schema', '?') + ' queued_project_start_queue_status_read_only=' + str(s.get('queued_project_start_queue_status_read_only', '?')) + ' queued_project_start_latest_queue_status=' + s.get('queued_project_start_latest_queue_status', '?') + ' queued_project_start_latest_queue_status_schema=' + s.get('queued_project_start_latest_queue_status_schema', '?') + ' queued_project_start_latest_queue_status_matches_run_id_selector=' + str(s.get('queued_project_start_latest_queue_status_matches_run_id_selector', '?')) + ' queued_project_start_closure_status=' + s.get('queued_project_start_closure_status', '?') + ' queued_project_start_closure_schema=' + s.get('queued_project_start_closure_schema', '?') + ' queued_project_start_closure_latest_selector_matches_run_id_selector=' + str(s.get('queued_project_start_closure_latest_selector_matches_run_id_selector', '?')) + ' queued_project_start_closure_verification_status=' + s.get('queued_project_start_closure_verification_status', '?') + ' queued_project_start_closure_verification_schema=' + s.get('queued_project_start_closure_verification_schema', '?') + ' queued_project_start_closure_verification_checksums_verified=' + str(s.get('queued_project_start_closure_verification_checksums_verified', '?')))
print('  queued_auto_replacement_packet_status=' + s.get('queued_auto_replacement_packet_status', '?') + ' queued_auto_replacement_packet_verification_status=' + s.get('queued_auto_replacement_packet_verification_status', '?') + ' queued_replacement_packet_status=' + s.get('queued_replacement_packet_status', '?') + ' queued_replacement_packet_verification_status=' + s.get('queued_replacement_packet_verification_status', '?') + ' queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver=' + str(s.get('queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver', '?')) + ' queued_replacement_packet_verification_ao2_replacement_driver_verified=' + str(s.get('queued_replacement_packet_verification_ao2_replacement_driver_verified', '?')) + ' queued_replacement_packet_verification_factory_v3_evaluator_closer_verified=' + str(s.get('queued_replacement_packet_verification_factory_v3_evaluator_closer_verified', '?')))
"
  else
    echo "$host: factory_project_artifact_pulled=false (see $out_dir/readback-smoke.err)"
  fi
done

echo "morning_cross_os_readback=overall=$overall"
exit $overall
