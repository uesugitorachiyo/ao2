#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_PHASE2_HARDENING_ROOT:-$ROOT/target/provider-phase2-contract-hardening/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
AO2_BIN="${AO2_BIN:-$ROOT/target/release/ao2}"

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

if [ ! -x "$AO2_BIN" ]; then
  run_step build_release npm run build:release
else
  printf "ao2 binary exists\n" >"$LOG_DIR/build_release.log"
  printf "0\n" >"$LOG_DIR/build_release.log.exit-code"
fi

# Operator contract shape: provider contract --verify --require codex
run_step codex_contract \
  "$AO2_BIN" provider contract --verify --require codex --json

# Operator contract shape: provider contract --verify --require claude
run_step claude_contract \
  "$AO2_BIN" provider contract --verify --require claude --json

run_step no_factory_v3 \
  npm run verify:no-factory-v3

run_step replacement_parity \
  npm run verify:replacement

run_step provider_contract_tests \
  cargo test -p ao2-cli cli_provider_contract --test cli_provider

run_step adapter_patch_digest_tests \
  cargo test -p ao2-cli cli_adapter_patch_preview_and_apply_promotes_exact_digest --test cli_adapter

run_step provider_score_tests \
  cargo test -p ao2-cli cli_provider_score --test cli_provider

run_step adapter_transcript_tests \
  cargo test -p ao2-adapters

python3 - "$OUT_ROOT" "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = out_root / "logs"
names = [
    "build_release",
    "codex_contract",
    "claude_contract",
    "no_factory_v3",
    "replacement_parity",
    "provider_contract_tests",
    "adapter_patch_digest_tests",
    "provider_score_tests",
    "adapter_transcript_tests",
]
checks = []
for name in names:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({
        "name": name,
        "status": "passed" if code == 0 else "failed",
        "exit_code": code,
        "log": str(log_dir / f"{name}.log"),
    })
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.provider-phase2-contract-hardening.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "contract_surfaces": {
        "transcript_parsing_corpus": "provider transcript summary tests",
        "sandbox_patch_digest_boundary": "sandbox patch apply rejects mismatched digests",
        "exact_approval_enforcement": "provider contract --verify --require codex",
        "blocker_taxonomy": "provider transcript blocker parsing",
        "fail_closed_live_guards": "unknown required provider contract failure",
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
