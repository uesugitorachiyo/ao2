#!/usr/bin/env bash
# ao2.pulse-gate-lib.v1

ao2_gate_run_step() {
  local log_dir="$1"
  local name="$2"
  shift 2
  local log="$log_dir/$name.log"
  mkdir -p "$log_dir"
  set +e
  "$@" >"$log" 2>&1
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

ao2_gate_write_component_summary() {
  local summary="$1"
  local schema="$2"
  local status="$3"
  local artifact_root="$4"
  python3 - "$summary" "$schema" "$status" "$artifact_root" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary = Path(sys.argv[1])
schema = sys.argv[2]
status = sys.argv[3]
artifact_root = sys.argv[4]
payload = {
    "schema_version": schema,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": artifact_root,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

ao2_gate_forbidden_string_scan() {
  local log_dir="$1"
  shift
  local log="$log_dir/forbidden_string_scan.log"
  local provider_one="OPENAI""_API_KEY"
  local provider_two="ANTHROPIC""_API_KEY"
  local push_cmd="git push"
  local origin_word="origin"
  local release_cmd="gh release"
  local create_word="create"
  local private_root="/Users/torachiyouesugi/Documents/pri""vate"
  local pattern="${provider_one}|${provider_two}|${push_cmd} ${origin_word}|${release_cmd} ${create_word}|${private_root}"
  mkdir -p "$log_dir"
  set +e
  if command -v rg >/dev/null 2>&1; then
    rg "$pattern" "$@" >"$log" 2>&1
  else
    grep -R -n -E "$pattern" "$@" >"$log" 2>&1
  fi
  local code=$?
  set -e
  if [ "$code" = "1" ]; then
    printf "0\n" >"$log.exit-code"
  else
    printf "%s\n" "$code" >"$log.exit-code"
  fi
}
