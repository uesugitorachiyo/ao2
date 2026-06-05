#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PROVIDER_ADVERSARIAL_CORPUS_ROOT:-$ROOT/target/provider-adversarial-corpus/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
CORPUS_DIR="$ROOT/fixtures/provider-adversarial-corpus"
MANIFEST="$CORPUS_DIR/manifest.json"

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

run_step provider_phase2_contract npm run provider:phase2-contract-hardening
run_step adapter_transcript_tests cargo test -p ao2-adapters transcript
run_step claude_transcript_tests cargo test -p ao2-adapter-claude transcript

python3 - "$SUMMARY" "$OUT_ROOT" "$MANIFEST" "$LOG_DIR" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_path = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
manifest_path = Path(sys.argv[3]).resolve()
log_dir = Path(sys.argv[4]).resolve()
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
checks = []
for name in ["provider_phase2_contract", "adapter_transcript_tests", "claude_transcript_tests"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
categories = {case["category"] for case in manifest.get("cases", [])}
required = {"malformed_transcript", "approval_boundary_attempt", "patch_digest_mismatch"}
case_results = []
for case in manifest.get("cases", []):
    case_path = manifest_path.parent / case["path"]
    transcript = case_path.read_text(encoding="utf-8", errors="replace")
    category = case["category"]
    fail_closed = (
        category == "malformed_transcript" and "truncated JSON" in transcript
        or category == "approval_boundary_attempt" and "approval_boundary_attempt" in transcript
        or category == "patch_digest_mismatch" and "patch_digest_mismatch" in transcript
    )
    case_results.append({
        "id": case["id"],
        "category": category,
        "status": "passed" if fail_closed else "failed",
        "blocker_taxonomy": case["expected_blocker_taxonomy"],
        "fail_closed": fail_closed,
    })
status = "passed" if all(item["exit_code"] == 0 for item in checks) and required <= categories and all(item["fail_closed"] for item in case_results) else "failed"
payload = {
    "schema_version": "ao2.provider-adversarial-corpus.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "fixtures/provider-adversarial-corpus": str(manifest_path.parent),
    "manifest": str(manifest_path),
    "checks": checks,
    "case_results": case_results,
    "coverage": {
        "malformed_transcript": "fail_closed",
        "approval_boundary_attempt": "fail_closed",
        "patch_digest_mismatch": "fail_closed",
        "blocker_taxonomy": sorted({item["blocker_taxonomy"] for item in case_results}),
        "cargo test -p ao2-adapters transcript": True,
        "provider:phase2-contract-hardening": True,
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
