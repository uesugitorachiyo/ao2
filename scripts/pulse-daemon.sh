#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ACTION="${1:-status}"
LABEL="${AO2_PULSE_DAEMON_LABEL:-com.ao2.pulse-auto-advance}"
TMUX_SESSION="${AO2_PULSE_DAEMON_TMUX_SESSION:-ao2-pulse-auto-advance}"
BACKEND="${AO2_PULSE_DAEMON_BACKEND:-auto}"
SLEEP_SECONDS="${AO2_PULSE_DAEMON_SLEEP_SECONDS:-10}"
OUT_ROOT="${AO2_PULSE_DAEMON_ROOT:-$ROOT/target/pulse-daemon/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
LOCAL_ROOT="$ROOT/.ao2-local/pulse"
PID_FILE="$LOCAL_ROOT/pulse-auto-advance.pid"
STOP_FILE="$LOCAL_ROOT/STOP"
PLIST_DIR="$HOME/Library/LaunchAgents"
PLIST="$PLIST_DIR/$LABEL.plist"
BOOTSTRAP_TARGET="gui/$(id -u)"

mkdir -p "$OUT_ROOT" "$LOG_DIR" "$LOCAL_ROOT" "$PLIST_DIR"

write_plist() {
  mkdir -p "$PLIST_DIR" "$LOG_DIR"
  cat >"$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL</string>
  <key>WorkingDirectory</key>
  <string>$ROOT</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/sh</string>
    <string>$ROOT/scripts/pulse-auto-advance.sh</string>
    <string>--forever</string>
    <string>--sleep-seconds</string>
    <string>$SLEEP_SECONDS</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>StandardOutPath</key>
  <string>$LOG_DIR/stdout.log</string>
  <key>StandardErrorPath</key>
  <string>$LOG_DIR/stderr.log</string>
</dict>
</plist>
EOF
}

launchctl_print() {
  launchctl print "$BOOTSTRAP_TARGET/$LABEL" >"$LOG_DIR/launchctl-print.log" 2>&1
}

launchctl_bootstrap() {
  launchctl bootstrap "$BOOTSTRAP_TARGET" "$PLIST" >"$LOG_DIR/launchctl-bootstrap.log" 2>&1
}

launchctl_kickstart() {
  launchctl kickstart -k "$BOOTSTRAP_TARGET/$LABEL" >"$LOG_DIR/launchctl-kickstart.log" 2>&1
}

launchctl_bootout() {
  launchctl bootout "$BOOTSTRAP_TARGET/$LABEL" >"$LOG_DIR/launchctl-bootout.log" 2>&1
}

tmux_has_session() {
  tmux has-session -t "$TMUX_SESSION" >"$LOG_DIR/tmux-has-session.log" 2>&1
}

tmux_start() {
  tmux new-session -d -s "$TMUX_SESSION" -c "$ROOT" "$ROOT/scripts/pulse-auto-advance.sh --forever --sleep-seconds $SLEEP_SECONDS" >"$LOG_DIR/tmux-start.log" 2>&1
}

tmux_stop() {
  tmux kill-session -t "$TMUX_SESSION" >"$LOG_DIR/tmux-stop.log" 2>&1
}

start_launchctl() {
  write_plist
  if launchctl_print; then
    launchctl_kickstart || true
  else
    launchctl_bootstrap || return 1
  fi
  sleep 2
  python3 - "$BOOTSTRAP_TARGET" "$LABEL" <<'PY'
import subprocess
import sys
target = sys.argv[1]
label = sys.argv[2]
result = subprocess.run(["launchctl", "print", f"{target}/{label}"], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
if result.returncode != 0:
    raise SystemExit(1)
for line in result.stdout.splitlines():
    if line.strip().startswith("pid ="):
        raise SystemExit(0)
raise SystemExit(1)
PY
}

start_tmux() {
  if tmux_has_session; then
    tmux_stop || true
  fi
  tmux_start
}

case "$ACTION" in
  start)
    rm -f "$STOP_FILE"
    if [ "$BACKEND" = "tmux" ]; then
      start_tmux
    elif [ "$BACKEND" = "launchctl" ]; then
      start_launchctl || {
        cat "$LOG_DIR/launchctl-bootstrap.log" >&2
        exit 1
      }
    else
      if ! start_launchctl; then
        launchctl_bootout || true
        start_tmux
      fi
    fi
    ;;
  stop)
    printf "operator_stop\n" >"$STOP_FILE"
    launchctl_bootout || true
    tmux_stop || true
    ;;
  restart)
    printf "operator_stop\n" >"$STOP_FILE"
    launchctl_bootout || true
    tmux_stop || true
    sleep 1
    rm -f "$STOP_FILE"
    if [ "$BACKEND" = "tmux" ]; then
      start_tmux
    elif [ "$BACKEND" = "launchctl" ]; then
      start_launchctl || {
        cat "$LOG_DIR/launchctl-bootstrap.log" >&2
        exit 1
      }
    else
      if ! start_launchctl; then
        launchctl_bootout || true
        start_tmux
      fi
    fi
    ;;
  status)
    ;;
  *)
    echo "usage: $0 {start|status|stop|restart}" >&2
    exit 2
    ;;
esac

python3 - "$ACTION" "$ROOT" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$LABEL" "$PLIST" "$STOP_FILE" "$BOOTSTRAP_TARGET" "$BACKEND" "$TMUX_SESSION" <<'PY'
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

action = sys.argv[1]
root = Path(sys.argv[2]).resolve()
out_root = Path(sys.argv[3]).resolve()
summary = Path(sys.argv[4]).resolve()
log_dir = Path(sys.argv[5]).resolve()
label = sys.argv[6]
plist = Path(sys.argv[7]).expanduser()
stop_file = Path(sys.argv[8]).resolve()
bootstrap_target = sys.argv[9]
requested_backend = sys.argv[10]
tmux_session = sys.argv[11]

def run(command):
    return subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)

launchctl = run(["launchctl", "print", f"{bootstrap_target}/{label}"])
launchctl_loaded = launchctl.returncode == 0
launchctl_output = launchctl.stdout + launchctl.stderr
pid = None
for line in launchctl_output.splitlines():
    stripped = line.strip()
    if stripped.startswith("pid ="):
        try:
            pid = int(stripped.split("=", 1)[1].strip())
        except ValueError:
            pid = None

process_alive = False
if pid is not None:
    process_alive = run(["kill", "-0", str(pid)]).returncode == 0

tmux = run(["tmux", "has-session", "-t", tmux_session])
tmux_loaded = tmux.returncode == 0
tmux_pid = None
tmux_process_alive = False
if tmux_loaded:
    pane_pid_result = run(["tmux", "list-panes", "-t", tmux_session, "-F", "#{pane_pid}"])
    if pane_pid_result.returncode == 0:
        first = pane_pid_result.stdout.strip().splitlines()
        if first:
            try:
                tmux_pid = int(first[0].strip())
            except ValueError:
                tmux_pid = None
    if tmux_pid is not None:
        tmux_process_alive = run(["kill", "-0", str(tmux_pid)]).returncode == 0

heartbeat = root / "target" / "pulse-auto-advance" / "latest" / "summary.json"
heartbeat_payload = {}
if heartbeat.is_file():
    try:
        heartbeat_payload = json.loads(heartbeat.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        heartbeat_payload = {}

active_backend = "launchctl" if process_alive else ("tmux" if tmux_process_alive else None)
active = process_alive or tmux_process_alive

if action == "stop":
    status = "stopped" if stop_file.is_file() and not active else "attention"
elif action in {"start", "restart"}:
    status = "running" if active else "attention"
else:
    status = "running" if active else "stopped"

payload = {
    "schema_version": "ao2.pulse-daemon.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "action": action,
    "status": status,
    "label": label,
    "bootstrap_target": bootstrap_target,
    "requested_backend": requested_backend,
    "active_backend": active_backend,
    "plist": str(plist),
    "plist_exists": plist.is_file(),
    "launchctl_loaded": launchctl_loaded,
    "pid": pid,
    "process_alive": process_alive,
    "tmux_session": tmux_session,
    "tmux_loaded": tmux_loaded,
    "tmux_pid": tmux_pid,
    "tmux_process_alive": tmux_process_alive,
    "stop_file": str(stop_file),
    "stop_file_present": stop_file.is_file(),
    "heartbeat_summary": str(heartbeat),
    "heartbeat_status": heartbeat_payload.get("status"),
    "heartbeat_reason": heartbeat_payload.get("reason"),
    "heartbeat_generated_at_utc": heartbeat_payload.get("generated_at_utc"),
    "logs": {
        "stdout": str(log_dir / "stdout.log"),
        "stderr": str(log_dir / "stderr.log"),
        "launchctl_print": str(log_dir / "launchctl-print.log"),
        "launchctl_bootstrap": str(log_dir / "launchctl-bootstrap.log"),
        "launchctl_kickstart": str(log_dir / "launchctl-kickstart.log"),
        "launchctl_bootout": str(log_dir / "launchctl-bootout.log"),
        "tmux_start": str(log_dir / "tmux-start.log"),
        "tmux_stop": str(log_dir / "tmux-stop.log"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary}")
print(f"status={status}")
if action in {"start", "restart"} and status != "running":
    raise SystemExit(1)
if action == "stop" and status == "attention":
    raise SystemExit(1)
PY
