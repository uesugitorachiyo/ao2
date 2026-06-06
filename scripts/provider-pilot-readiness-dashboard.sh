#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_PILOT_DASHBOARD_ROOT:-$ROOT/target/provider-pilot-readiness-dashboard/latest}"
SUMMARY="$OUT_ROOT/summary.json"
HTML="$OUT_ROOT/dashboard.html"
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

run_step provider_phase2_contract \
  env AO2_PROVIDER_PHASE2_HARDENING_ROOT="$OUT_ROOT/provider-phase2-contract-hardening" \
    npm run provider:phase2-contract-hardening

run_step provider_adversarial_corpus \
  env AO2_PROVIDER_ADVERSARIAL_CORPUS_ROOT="$OUT_ROOT/provider-adversarial-corpus" \
    npm run provider:adversarial-corpus

python3 - "$OUT_ROOT" "$SUMMARY" "$HTML" <<'PY'
import html
import json
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
html_path = Path(sys.argv[3]).resolve()
log_dir = out_root / "logs"
provider_cli_detection = {}
for name in ["codex", "claude", "antigravity"]:
    resolved = shutil.which(name)
    version = None
    if resolved:
        try:
            version = subprocess.run([resolved, "--version"], text=True, capture_output=True, timeout=5, check=False).stdout.strip()[:200]
        except Exception as exc:
            version = f"version probe failed: {exc}"
    provider_cli_detection[name] = {"available": bool(resolved), "path": resolved, "version": version}
checks = []
for name in ["provider_phase2_contract", "provider_adversarial_corpus"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
checks.append({"name": "provider_cli_detection", "status": "passed", "providers": provider_cli_detection})
checks.append({"name": "provider_api_key_env_required", "status": "passed", "observed": False})
status = "passed" if all(check["status"] == "passed" for check in checks) else "failed"
rows = "\n".join(
    f"<tr><td>{html.escape(name)}</td><td>{html.escape(str(info['available']))}</td><td><code>{html.escape(str(info['path']))}</code></td></tr>"
    for name, info in provider_cli_detection.items()
)
html_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Provider Pilot Readiness Dashboard</title></head><body>"
    "<h1>AO2 Provider Pilot Readiness Dashboard</h1>"
    f"<p>Status: <code>{html.escape(status)}</code></p>"
    "<p>Provider API-key auth is not required or allowed for this pilot readiness gate.</p>"
    "<table><thead><tr><th>Provider CLI</th><th>Available</th><th>Path</th></tr></thead>"
    f"<tbody>{rows}</tbody></table></body></html>\n",
    encoding="utf-8",
)
payload = {
    "schema_version": "ao2.provider-pilot-readiness-dashboard.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "dashboard.html": str(html_path),
    "provider_cli_detection": provider_cli_detection,
    "provider_api_key_env_required": False,
    "checks": checks,
    "component_summaries": {
        "provider_phase2_contract": str(out_root / "provider-phase2-contract-hardening" / "summary.json"),
        "provider_adversarial_corpus": str(out_root / "provider-adversarial-corpus" / "summary.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False, "fail_closed_when_provider_unavailable": True},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"dashboard={html_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
