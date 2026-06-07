#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_RELEASE_EVIDENCE_CLOSURE_ROOT:-$ROOT/target/release-evidence-closure/latest}"
SUMMARY="$OUT_ROOT/summary.json"
HTML="$OUT_ROOT/closure.html"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="${AO2_RELEASE_EVIDENCE_CLOSURE_FIXTURE_DIR:-}"
CP_RESTORE_ROOT="${AO2_RELEASE_EVIDENCE_CLOSURE_CP_RESTORE_ROOT:-$CP_ROOT/target/dr-restore-drill/release-evidence-closure}"

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

ci_env=(env AO2_CI_ARTIFACT_DOWNLOAD_ROOT="$ROOT/target/ci-artifacts/latest")
if [ -n "$FIXTURE_DIR" ]; then
  ci_env+=(AO2_CI_ARTIFACT_DOWNLOAD_FIXTURE_DIR="$FIXTURE_DIR")
fi

canary_env=(env AO2_LOCAL_CANARY_ROOT="$OUT_ROOT/local-canary")
if [ -n "$FIXTURE_DIR" ]; then
  canary_env+=(AO2_CI_ARTIFACT_DOWNLOAD_FIXTURE_DIR="$FIXTURE_DIR")
fi

run_step ci_artifact_download_contract \
  "${ci_env[@]}" npm run artifacts:ci-download-contract

run_step local_canary \
  "${canary_env[@]}" npm run local:canary

run_step phase1_promotion_golden \
  env AO2_PHASE1_PROMOTION_GOLDEN_ROOT="$OUT_ROOT/phase1-promotion-golden" \
    npm run phase1:promotion-golden

run_step pulse_execute_safety_corpus \
  env AO2_PULSE_EXECUTE_CORPUS_ROOT="$OUT_ROOT/pulse-execute-safety-corpus" \
    npm run pulse:execute-safety-corpus

run_step pulse_real_execute_containment \
  env AO2_PULSE_REAL_EXECUTE_ROOT="$OUT_ROOT/pulse-real-execute-containment" \
    npm run pulse:real-execute-containment

run_step control_plane_restore_negative \
  "$CP_ROOT/scripts/cp-dr-restore-drill.sh" \
    --negative-only \
    --work-dir "$CP_RESTORE_ROOT" \
    --out "$CP_RESTORE_ROOT/dr-restore-report.json"

run_step artifact_index \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" npm run artifacts:index

run_step artifact_health \
  env \
    AO2_ARTIFACT_HEALTH_INDEX="$OUT_ROOT/artifact-index/artifact-index.json" \
    AO2_ARTIFACT_HEALTH_ROOT="$OUT_ROOT/artifact-health" \
    AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS="ao2/target/ci-artifacts ao2/.ao2-local/pulse/latest ao2-control-plane/target/ci-artifacts ao2-control-plane/target/dr-restore-drill" \
    AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS="ao2/target/release-evidence-closure ao2/target/release-readiness-ci ao2/target/release-readiness-regression-gate ao2/target/phase1-promotion-golden ao2/target/pulse-real-execute-containment" \
    AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1 \
    npm run artifacts:health

python3 - "$OUT_ROOT" "$SUMMARY" "$HTML" "$CP_RESTORE_ROOT" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
html_path = Path(sys.argv[3]).resolve()
cp_restore_root = Path(sys.argv[4]).resolve()
log_dir = out_root / "logs"

steps = [
    ("ci_artifact_download_contract", "target/ci-artifacts/latest/summary.json"),
    ("local_canary", "target/release-evidence-closure/latest/local-canary/local-canary-summary.json"),
    ("phase1_promotion_golden", "target/release-evidence-closure/latest/phase1-promotion-golden/summary.json"),
    ("pulse_execute_safety_corpus", "target/release-evidence-closure/latest/pulse-execute-safety-corpus/summary.json"),
    ("pulse_real_execute_containment", "target/release-evidence-closure/latest/pulse-real-execute-containment/summary.json"),
    ("control_plane_restore_negative", str(cp_restore_root / "dr-restore-report.json")),
    ("artifact_index", "target/release-evidence-closure/latest/artifact-index/artifact-index.json"),
    ("artifact_health", "target/release-evidence-closure/latest/artifact-health/summary.json"),
]

checks = []
for name, evidence in steps:
    log = log_dir / f"{name}.log"
    exit_code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({
        "name": name,
        "status": "passed" if exit_code == 0 else "failed",
        "exit_code": exit_code,
        "log": str(log),
        "evidence": evidence,
    })

payload = {
    "schema_version": "ao2.release-evidence-closure.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "accepted" if all(item["exit_code"] == 0 for item in checks) else "rejected",
    "artifact_root": str(out_root),
    "closure_html": str(html_path),
    "evidence_rule": "evidence must exist before evaluator closure accepts a run",
    "checks": checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

rows = "\n".join(
    "<tr>"
    f"<td>{html.escape(item['name'])}</td>"
    f"<td>{html.escape(item['status'])}</td>"
    f"<td>{item['exit_code']}</td>"
    f"<td><code>{html.escape(item['evidence'])}</code></td>"
    "</tr>"
    for item in checks
)
html_path.write_text(
    "<!doctype html>\n"
    "<html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Release Evidence Closure</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px;color:#172026}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body>"
    "<h1>AO2 Release Evidence Closure</h1>"
    f"<p>Schema: <code>{payload['schema_version']}</code></p>"
    f"<p>Status: <code>{payload['status']}</code></p>"
    "<p>Evidence must exist before evaluator closure accepts a run.</p>"
    "<table><thead><tr><th>Check</th><th>Status</th><th>Exit</th><th>Evidence</th></tr></thead>"
    f"<tbody>{rows}</tbody></table>"
    "</body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"closure={html_path}")
print(f"status={payload['status']}")
if payload["status"] != "accepted":
    raise SystemExit(1)
PY
