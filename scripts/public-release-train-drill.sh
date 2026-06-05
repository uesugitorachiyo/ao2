#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_RELEASE_TRAIN_DRILL_ROOT:-$ROOT/target/public-release-train-drill/latest}"
SUMMARY="$OUT_ROOT/summary.json"
HTML="$OUT_ROOT/closure.html"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="${AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR:-}"

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

closure_env=(env AO2_RELEASE_EVIDENCE_CLOSURE_ROOT="$OUT_ROOT/release-evidence-closure")
regression_env=(env AO2_RELEASE_READINESS_REGRESSION_ROOT="$OUT_ROOT/release-readiness-regression-gate")
if [ -n "$FIXTURE_DIR" ]; then
  closure_env+=(AO2_RELEASE_EVIDENCE_CLOSURE_FIXTURE_DIR="$FIXTURE_DIR")
  regression_env+=(AO2_CI_ARTIFACT_DOWNLOAD_FIXTURE_DIR="$FIXTURE_DIR")
fi

run_step release_evidence_closure \
  "${closure_env[@]}" npm run release:evidence-closure

run_step release_readiness_regression_gate \
  "${regression_env[@]}" npm run release:readiness:regression-gate

run_step retention_preflight \
  env AO2_RELEASE_RETENTION_PRUNE=0 npm run release:retention-preflight

# Rehearsal consumer command shape: release:artifact-consumer-smoke -- --dry-run
consumer_args=(--dry-run)
if [ -n "$FIXTURE_DIR" ]; then
  consumer_args=(--fixture-dir "$FIXTURE_DIR" --require-artifact ao2-python-guard --require-schema ao2.python-guard-ci-artifacts.v1)
fi
run_step artifact_consumer \
  env AO2_RELEASE_ARTIFACT_CONSUMER_ROOT="$OUT_ROOT/release-artifact-consumer" \
    npm run release:artifact-consumer-smoke -- "${consumer_args[@]}"

run_step post_merge_canary \
  env AO2_POST_MERGE_CANARY_ROOT="$OUT_ROOT/post-merge-canary" npm run post-merge:canary

python3 - "$OUT_ROOT" "$SUMMARY" "$HTML" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
html_path = Path(sys.argv[3]).resolve()
log_dir = out_root / "logs"
names = [
    "release_evidence_closure",
    "release_readiness_regression_gate",
    "retention_preflight",
    "artifact_consumer",
    "post_merge_canary",
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
publish_guards = {
    "refuses_publish_side_effects_by_default": True,
    "tag_push_publish_deploy": "not executed by this drill",
    "release:download-verify": "referenced as install_update_smoke_reference after real release assets exist",
    "install_update_smoke_reference": True,
}
status = "passed" if all(item["exit_code"] == 0 for item in checks) else "failed"
payload = {
    "schema_version": "ao2.public-release-train-drill.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "publish_guards": publish_guards,
    "component_summaries": {
        "release_evidence_closure": str(out_root / "release-evidence-closure" / "summary.json"),
        "release_readiness_regression_gate": str(out_root / "release-readiness-regression-gate" / "summary.json"),
        "post_merge_canary": str(out_root / "post-merge-canary" / "post-merge-canary.json"),
    },
    "closure_html": str(html_path),
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
rows = "\n".join(
    "<tr>"
    f"<td>{html.escape(item['name'])}</td>"
    f"<td>{html.escape(item['status'])}</td>"
    f"<td>{item['exit_code']}</td>"
    f"<td><code>{html.escape(item['log'])}</code></td>"
    "</tr>"
    for item in checks
)
html_path.write_text(
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 Public Release Train Drill</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #d7dde2;padding:8px;text-align:left}"
    "th{background:#f3f6f8}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}</style>"
    "</head><body><h1>AO2 Public Release Train Drill</h1>"
    f"<p>Status: <code>{html.escape(status)}</code></p>"
    "<p>No tag, push, publish, or deploy side effects are executed by this rehearsal.</p>"
    "<table><thead><tr><th>Check</th><th>Status</th><th>Exit</th><th>Log</th></tr></thead>"
    f"<tbody>{rows}</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"closure={html_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
