#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_LIVE_PILOT_CORPUS_ROOT:-$ROOT/target/provider-live-pilot-corpus/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

run_step() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  set +e
  "$@" >"$log" 2>&1
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

run_step pilot_readiness_dashboard \
  env AO2_PROVIDER_PILOT_DASHBOARD_ROOT="$OUT_ROOT/provider-pilot-readiness-dashboard" \
    npm run provider:pilot-readiness-dashboard

run_step adversarial_corpus \
  env AO2_PROVIDER_ADVERSARIAL_CORPUS_ROOT="$OUT_ROOT/provider-adversarial-corpus" \
    npm run provider:adversarial-corpus

if [ "${AO2_LIVE_CODEX_SMOKE:-0}" = "1" ]; then
  run_step live_codex_smoke npm run smoke:provider:codex
else
  printf "live Codex smoke skipped; set AO2_LIVE_CODEX_SMOKE=1\n" >"$LOG_DIR/live_codex_smoke.log"
  printf "0\n" >"$LOG_DIR/live_codex_smoke.log.exit-code"
fi

if [ "${AO2_LIVE_CLAUDE_SMOKE:-0}" = "1" ]; then
  run_step live_claude_smoke npm run smoke:provider:claude
else
  printf "live Claude smoke skipped; set AO2_LIVE_CLAUDE_SMOKE=1\n" >"$LOG_DIR/live_claude_smoke.log"
  printf "0\n" >"$LOG_DIR/live_claude_smoke.log.exit-code"
fi

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
names = ["pilot_readiness_dashboard", "adversarial_corpus", "live_codex_smoke", "live_claude_smoke"]
checks = []
for name in names:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.provider-live-pilot-corpus.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "live_provider_modes": {
        "codex": "enabled" if "AO2_LIVE_CODEX_SMOKE=1" in (log_dir / "live_codex_smoke.log").read_text(encoding="utf-8", errors="replace") else "guarded_optional",
        "claude": "enabled" if "AO2_LIVE_CLAUDE_SMOKE=1" in (log_dir / "live_claude_smoke.log").read_text(encoding="utf-8", errors="replace") else "guarded_optional",
    },
    "provider_api_key_env_required": False,
    "coverage": {
        "approval_denial": "adversarial_corpus",
        "digest_mismatch": "adversarial_corpus",
        "transcript_parser": "adversarial_corpus",
        "local_cli_detection": "provider_pilot_readiness_dashboard",
    },
    "component_summaries": {
        "pilot_readiness_dashboard": str(out_root / "provider-pilot-readiness-dashboard" / "summary.json"),
        "adversarial_corpus": str(out_root / "provider-adversarial-corpus" / "summary.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False, "provider_api_key_env_required": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
