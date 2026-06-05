#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_FRONTIER_LENGTHY_GATE_ROOT:-$ROOT/target/frontier-lengthy-gate/latest}"
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

run_step cross_repo_control_plane \
  env AO2_CROSS_REPO_CP_OBSERVER_ROOT="$OUT_ROOT/cross-repo-control-plane-observer" \
    npm run control-plane:cross-repo-observer

run_step release_install_update_fixture \
  env AO2_RELEASE_INSTALL_UPDATE_ROOT="$OUT_ROOT/release-install-update-fixture" \
    npm run release:install-update-fixture

run_step workbench_browser_qa \
  env AO2_WORKBENCH_BROWSER_QA_ROOT="$OUT_ROOT/workbench-browser-qa" \
    npm run workbench:browser-qa

run_step provider_adversarial_corpus \
  env AO2_PROVIDER_ADVERSARIAL_CORPUS_ROOT="$OUT_ROOT/provider-adversarial-corpus" \
    npm run provider:adversarial-corpus

run_step dr_retention_snapshot \
  env AO2_DR_RETENTION_SNAPSHOT_ROOT="$OUT_ROOT/dr-retention-long-run-snapshot" \
    npm run release:dr-retention-snapshot

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
components = [
    ("cross_repo_control_plane", "cross-repo-control-plane-observer/summary.json", "control-plane:cross-repo-observer"),
    ("release_install_update_fixture", "release-install-update-fixture/summary.json", "release:install-update-fixture"),
    ("workbench_browser_qa", "workbench-browser-qa/summary.json", "workbench:browser-qa"),
    ("provider_adversarial_corpus", "provider-adversarial-corpus/summary.json", "provider:adversarial-corpus"),
    ("dr_retention_snapshot", "dr-retention-long-run-snapshot/summary.json", "release:dr-retention-snapshot"),
]
checks = []
component_summaries = {}
for name, rel_summary, command in components:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    summary = out_root / rel_summary
    component_summaries[name] = str(summary)
    checks.append({"name": name, "command": command, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log"), "summary": str(summary)})
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.frontier-lengthy-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "component_summaries": component_summaries,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
