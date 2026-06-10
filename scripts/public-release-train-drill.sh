#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_RELEASE_TRAIN_DRILL_ROOT:-$ROOT/target/public-release-train-drill/latest}"
SUMMARY="$OUT_ROOT/summary.json"
HTML="$OUT_ROOT/closure.html"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="${AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR:-}"
PULSE_SOURCE="${AO2_RELEASE_TRAIN_PULSE_SOURCE:-$OUT_ROOT/release-train-pulse-seed}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

mkdir -p "$PULSE_SOURCE/loop-next"
cat >"$PULSE_SOURCE/packet.md" <<'EOF'
# AO2 Release Train Pulse Seed

Local release-train rehearsal seed. This exists so production-readiness drills
do not depend on stale overnight Pulse daemon output.
EOF
cat >"$PULSE_SOURCE/board.md" <<'EOF'
# Release Train Board

- Verify release-readiness static evidence.
- Verify release-readiness artifact consumer contract.
- Keep publish side effects disabled.
EOF
cat >"$PULSE_SOURCE/executor-evidence.json" <<'EOF'
{
  "schema_version": "ao2.release-train-pulse-seed-executor-evidence.v1",
  "status": "passed",
  "trust_boundary": {
    "local_only": true,
    "stores_credentials": false
  }
}
EOF
cat >"$PULSE_SOURCE/loop-next/pulse-eval-loop.json" <<'EOF'
{
  "schema_version": "ao2.release-train-pulse-seed-eval-loop.v1",
  "status": "passed",
  "tasks": [
    {
      "id": "release-train-readiness-consumer-contract",
      "status": "passed"
    }
  ]
}
EOF
export AO2_PULSE_LOCAL_MIRROR_SOURCE="$PULSE_SOURCE"

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
static_env=(env AO2_RELEASE_READINESS_ROOT="$OUT_ROOT/release-readiness-static")
regression_env=(env AO2_RELEASE_READINESS_REGRESSION_ROOT="$OUT_ROOT/release-readiness-regression-gate")
if [ -n "$FIXTURE_DIR" ]; then
  closure_env+=(AO2_RELEASE_EVIDENCE_CLOSURE_FIXTURE_DIR="$FIXTURE_DIR")
  regression_env+=(AO2_CI_ARTIFACT_DOWNLOAD_FIXTURE_DIR="$FIXTURE_DIR")
fi

run_step release_evidence_closure \
  "${closure_env[@]}" npm run release:evidence-closure

run_step release_readiness_static \
  "${static_env[@]}" npm run release:readiness:static

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
    "release_readiness_static",
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

release_readiness_static_summary = out_root / "release-readiness-static" / "summary.json"
release_readiness_artifact_consumer_contract = {
    "status": "missing",
    "source_summary": str(release_readiness_static_summary),
    "required_check": "ci_release_readiness_artifact_consumer_job",
}
try:
    release_readiness_static = json.loads(release_readiness_static_summary.read_text(encoding="utf-8"))
    checks_by_name = {
        item.get("name"): item
        for item in release_readiness_static.get("checks", [])
        if isinstance(item, dict)
    }
    consumer_check = checks_by_name.get("ci_release_readiness_artifact_consumer_job", {})
    release_readiness_artifact_consumer_contract.update({
        "status": "passed" if consumer_check.get("status") == "passed" else "failed",
        "release_readiness_status": release_readiness_static.get("status"),
        "check_detail": consumer_check.get("detail"),
    })
except FileNotFoundError:
    release_readiness_artifact_consumer_contract["error"] = "release readiness static summary missing"
except json.JSONDecodeError as exc:
    release_readiness_artifact_consumer_contract.update({"status": "failed", "error": str(exc)})

publish_guards = {
    "refuses_publish_side_effects_by_default": True,
    "tag_push_publish_deploy": "not executed by this drill",
    "release:download-verify": "referenced as install_update_smoke_reference after real release assets exist",
    "install_update_smoke_reference": True,
}
status = (
    "passed"
    if all(item["exit_code"] == 0 for item in checks)
    and release_readiness_artifact_consumer_contract["status"] == "passed"
    else "failed"
)
payload = {
    "schema_version": "ao2.public-release-train-drill.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checks": checks,
    "publish_guards": publish_guards,
    "release_readiness_artifact_consumer_contract": release_readiness_artifact_consumer_contract,
    "component_summaries": {
        "release_evidence_closure": str(out_root / "release-evidence-closure" / "summary.json"),
        "release_readiness_static": str(release_readiness_static_summary),
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
    "<p>Release readiness artifact consumer contract: "
    f"<code>{html.escape(release_readiness_artifact_consumer_contract['status'])}</code>.</p>"
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
