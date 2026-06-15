#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_RELEASE_READINESS_REGRESSION_ROOT:-$ROOT/target/release-readiness-regression-gate/$(date -u +%Y%m%dT%H%M%SZ)}"
SUMMARY="$OUT_ROOT/summary.json"
HOSTED_ARTIFACT_REQUIRED="${AO2_RELEASE_READINESS_REGRESSION_HOSTED_ARTIFACT_REQUIRED:-0}"
HOSTED_ARTIFACT_FIXTURE_DIR="${AO2_RELEASE_READINESS_REGRESSION_HOSTED_ARTIFACT_FIXTURE_DIR:-}"
ONLY_HOSTED_ARTIFACT_GATE="${AO2_RELEASE_READINESS_REGRESSION_ONLY_HOSTED_ARTIFACT_GATE:-0}"
# Default control-plane smoke: ../ao2-control-plane/scripts/smoke-long-lived-dev.sh

mkdir -p "$OUT_ROOT"

run_step() {
  name="$1"
  log="$2"
  shift 2
  echo "step=$name"
  set +e
  "$@" >"$log" 2>&1
  status="$?"
  set -e
  printf "%s\n" "$status" >"$log.exit-code"
  return 0
}

hosted_release_readiness_artifact_gate() {
  local gate_root="$OUT_ROOT/hosted-release-readiness-artifact-gate"
  local artifact_dir="$gate_root/ao2-release-readiness"
  rm -rf "$gate_root"
  mkdir -p "$artifact_dir"

  if [ -n "$HOSTED_ARTIFACT_FIXTURE_DIR" ]; then
    if [ ! -d "$HOSTED_ARTIFACT_FIXTURE_DIR" ]; then
      echo "hosted artifact fixture dir not found: $HOSTED_ARTIFACT_FIXTURE_DIR" >&2
      return 1
    fi
    cp -R "$HOSTED_ARTIFACT_FIXTURE_DIR"/. "$artifact_dir"/
    printf "fixture\n" >"$gate_root/source.txt"
  elif command -v gh >/dev/null 2>&1; then
    local downloaded=0
    while IFS= read -r run_id; do
      [ -n "$run_id" ] || continue
      rm -rf "$artifact_dir"
      mkdir -p "$artifact_dir"
      if gh run download "$run_id" --repo uesugitorachiyo/ao2 --name ao2-release-readiness --dir "$artifact_dir"; then
        printf "%s\n" "$run_id" >"$gate_root/source-run-id.txt"
        downloaded=1
        break
      fi
    done < <(gh run list \
      --repo uesugitorachiyo/ao2 \
      --branch main \
      --workflow ci.yml \
      --status success \
      --limit 10 \
      --json databaseId \
      --jq '.[].databaseId')
    if [ "$downloaded" != "1" ]; then
      if [ "$HOSTED_ARTIFACT_REQUIRED" = "1" ]; then
        echo "missing ao2-release-readiness artifact from successful main CI runs" >&2
        return 1
      fi
      printf "skipped: no hosted ao2-release-readiness artifact available\n" >"$gate_root/skip-reason.txt"
    fi
  else
    if [ "$HOSTED_ARTIFACT_REQUIRED" = "1" ]; then
      echo "gh is required for hosted release-readiness artifact gate" >&2
      return 1
    fi
    printf "skipped: gh unavailable\n" >"$gate_root/skip-reason.txt"
  fi

  python3 - "$gate_root" "$artifact_dir" "$HOSTED_ARTIFACT_REQUIRED" <<'PY'
import json
import sys
from pathlib import Path

gate_root = Path(sys.argv[1])
artifact_dir = Path(sys.argv[2])
required = sys.argv[3] == "1"
summary_path = gate_root / "summary.json"

def fail(message, payload=None):
    detail = f": {json.dumps(payload, sort_keys=True)}" if payload is not None else ""
    summary_path.write_text(
        json.dumps(
            {
                "schema_version": "ao2.release-readiness-hosted-artifact-gate.v1",
                "status": "failed",
                "error": message,
                "artifact_dir": str(artifact_dir),
                "required": required,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    raise SystemExit(message + detail)

skip_reason = gate_root / "skip-reason.txt"
if skip_reason.is_file() and not required:
    summary_path.write_text(
        json.dumps(
            {
                "schema_version": "ao2.release-readiness-hosted-artifact-gate.v1",
                "status": "skipped",
                "skip_reason": skip_reason.read_text(encoding="utf-8").strip(),
                "artifact_dir": str(artifact_dir),
                "required": required,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"summary={summary_path}")
    print("status=skipped")
    raise SystemExit(0)

required_files = ["summary.json", "report.md", "report.html", "artifact-closure-index.json"]
missing = [name for name in required_files if not (artifact_dir / name).is_file()]
if missing:
    fail("hosted ao2-release-readiness artifact missing required files", {"missing": missing})

readiness_summary = json.loads((artifact_dir / "summary.json").read_text(encoding="utf-8"))
closure_index = json.loads((artifact_dir / "artifact-closure-index.json").read_text(encoding="utf-8"))
report_md = (artifact_dir / "report.md").read_text(encoding="utf-8")

if readiness_summary.get("schema_version") != "ao2.release-readiness-local.v1":
    fail("unexpected hosted release readiness schema", readiness_summary)
if readiness_summary.get("status") != "passed":
    fail("hosted release readiness did not pass", readiness_summary)
if closure_index.get("schema_version") != "ao2.release-artifact-closure-index.v1":
    fail("unexpected hosted release readiness artifact closure schema", closure_index)
if closure_index.get("status") != "passed":
    fail("hosted release readiness artifact closure did not pass", closure_index)

public_pair_digest_gate = closure_index.get("public_pair_digest_gate", {})
gate_ok = (
    public_pair_digest_gate.get("schema_version") == "ao2.public-release-pair-digest-audit.v1"
    and public_pair_digest_gate.get("status") == "passed"
    and public_pair_digest_gate.get("archive_parity_status") == "passed"
    and public_pair_digest_gate.get("required_summary_field") == "public_pair_digest_audit"
    and public_pair_digest_gate.get("required_archive_scope") == "full_archive_parity"
    and public_pair_digest_gate.get("required_check") == "release_public_pair_digest_audit_contract"
    and public_pair_digest_gate.get("required_artifact") == "ao2-public-release-pair-digest-audit"
)
if not gate_ok:
    fail("hosted release readiness public pair digest gate was not ready", closure_index)
for needle in [
    "public_pair_digest_audit.archive_parity_status=passed",
    "ao2-public-release-pair-digest-audit",
    "full_archive_parity",
]:
    if needle not in report_md:
        fail("hosted release readiness report missing public pair digest gate detail", {"needle": needle})

payload = {
    "schema_version": "ao2.release-readiness-hosted-artifact-gate.v1",
    "status": "passed",
    "artifact_dir": str(artifact_dir),
    "required": required,
    "readiness_schema_version": readiness_summary.get("schema_version"),
    "artifact_closure_schema_version": closure_index.get("schema_version"),
    "public_pair_digest_gate": public_pair_digest_gate,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "source": "github_actions_artifact_download",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print("status=passed")
PY
}

run_step hosted_release_readiness_artifact_gate "$OUT_ROOT/hosted-release-readiness-artifact-gate.log" \
  hosted_release_readiness_artifact_gate
hosted_artifact_status="$(cat "$OUT_ROOT/hosted-release-readiness-artifact-gate.log.exit-code")"

if [ "$ONLY_HOSTED_ARTIFACT_GATE" = "1" ]; then
  python3 - "$OUT_ROOT" "$SUMMARY" "$hosted_artifact_status" <<'PY'
import json
import sys
from pathlib import Path

out_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
exit_code = int(sys.argv[3])
gate_summary_path = out_root / "hosted-release-readiness-artifact-gate" / "summary.json"
gate_summary = json.loads(gate_summary_path.read_text(encoding="utf-8"))
payload = {
    "schema_version": "ao2.release-readiness-regression-gate.v1",
    "status": "passed" if exit_code == 0 and gate_summary.get("status") == "passed" else "failed",
    "artifact_root": str(out_root),
    "checks": [
        {
            "name": "hosted_release_readiness_artifact_gate",
            "status": "passed" if exit_code == 0 else "failed",
            "exit_code": exit_code,
            "log": str(out_root / "hosted-release-readiness-artifact-gate.log"),
        }
    ],
    "hosted_release_readiness_artifact_gate": gate_summary,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
  exit 0
fi

run_step release_readiness_static "$OUT_ROOT/release-readiness-static.log" \
  env AO2_RELEASE_READINESS_ROOT="$OUT_ROOT/release-readiness-static" npm run release:readiness:static
release_status="$(cat "$OUT_ROOT/release-readiness-static.log.exit-code")"

run_step phase1_operator_golden "$OUT_ROOT/phase1-operator-golden.log" \
  env AO2_PHASE1_OPERATOR_SMOKE_ROOT="$OUT_ROOT/phase1-operator-golden" npm run smoke:phase1-operator-golden
phase1_status="$(cat "$OUT_ROOT/phase1-operator-golden.log.exit-code")"

run_step pulse_local_mirror "$OUT_ROOT/pulse-local-mirror.log" \
  env AO2_PULSE_LOCAL_MIRROR_DEST="$ROOT/.ao2-local/pulse/latest" npm run pulse:local-mirror
pulse_status="$(cat "$OUT_ROOT/pulse-local-mirror.log.exit-code")"

run_step pulse_resume_dry_run "$OUT_ROOT/pulse-resume-dry-run.log" \
  env AO2_PULSE_RESUME_JSON="$ROOT/.ao2-local/pulse/latest/resume.json" \
    AO2_PULSE_RESUME_ROOT="$OUT_ROOT/pulse-resume" \
    npm run pulse:resume -- --dry-run
pulse_resume_status="$(cat "$OUT_ROOT/pulse-resume-dry-run.log.exit-code")"

run_step ci_artifact_download_contract "$OUT_ROOT/ci-artifact-download-contract.log" \
  env AO2_CI_ARTIFACT_DOWNLOAD_ROOT="$ROOT/target/ci-artifacts/latest" \
    npm run artifacts:ci-download-contract
ci_artifact_status="$(cat "$OUT_ROOT/ci-artifact-download-contract.log.exit-code")"

run_step artifact_index "$OUT_ROOT/artifact-index.log" \
  env AO2_ARTIFACT_INDEX_ROOT="$OUT_ROOT/artifact-index" npm run artifacts:index
artifact_status="$(cat "$OUT_ROOT/artifact-index.log.exit-code")"

run_step artifact_health "$OUT_ROOT/artifact-health.log" \
  env \
    AO2_ARTIFACT_HEALTH_INDEX="$OUT_ROOT/artifact-index/artifact-index.json" \
    AO2_ARTIFACT_HEALTH_ROOT="$OUT_ROOT/artifact-health" \
    AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS="ao2/target/ci-artifacts ao2/.ao2-local/pulse/latest ao2-control-plane/target/ci-artifacts" \
    AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS="ao2/target/release-readiness-ci ao2/target/release-evidence-closure ao2/target/release-readiness-regression-gate ao2/target/phase1-promotion-golden ao2/target/pulse-real-execute-containment ao2-control-plane/target/dr-restore-drill" \
    AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1 \
    npm run artifacts:health
artifact_health_status="$(cat "$OUT_ROOT/artifact-health.log.exit-code")"

run_step release_artifact_consumer_smoke "$OUT_ROOT/release-artifact-consumer-smoke.log" \
  env AO2_RELEASE_ARTIFACT_CONSUMER_ROOT="$OUT_ROOT/release-artifact-consumer-smoke" \
    npm run release:artifact-consumer-smoke -- --dry-run
consumer_status="$(cat "$OUT_ROOT/release-artifact-consumer-smoke.log.exit-code")"

if [ -x "$CP_ROOT/scripts/smoke-long-lived-dev.sh" ]; then
  run_step control_plane_long_lived_smoke "$OUT_ROOT/control-plane-long-lived-smoke.log" \
    env AO2_CP_LONG_LIVED_SMOKE_ROOT="$OUT_ROOT/control-plane-long-lived-smoke" \
      "$CP_ROOT/scripts/smoke-long-lived-dev.sh"
  cp_status="$(cat "$OUT_ROOT/control-plane-long-lived-smoke.log.exit-code")"
else
  cp_status="127"
  printf "missing control-plane smoke script: %s\n" "$CP_ROOT/scripts/smoke-long-lived-dev.sh" \
    >"$OUT_ROOT/control-plane-long-lived-smoke.log"
  printf "%s\n" "$cp_status" >"$OUT_ROOT/control-plane-long-lived-smoke.log.exit-code"
fi

python3 - "$OUT_ROOT" "$SUMMARY" "$release_status" "$hosted_artifact_status" "$phase1_status" "$pulse_status" "$pulse_resume_status" "$ci_artifact_status" "$artifact_status" "$artifact_health_status" "$consumer_status" "$cp_status" <<'PY'
import json
import sys
from pathlib import Path

out_root = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
codes = {
    "release_readiness_static": int(sys.argv[3]),
    "hosted_release_readiness_artifact_gate": int(sys.argv[4]),
    "phase1_operator_golden": int(sys.argv[5]),
    "pulse_local_mirror": int(sys.argv[6]),
    "pulse_resume_dry_run": int(sys.argv[7]),
    "ci_artifact_download_contract": int(sys.argv[8]),
    "artifact_index": int(sys.argv[9]),
    "artifact_health": int(sys.argv[10]),
    "release_artifact_consumer_smoke": int(sys.argv[11]),
    "control_plane_long_lived_smoke": int(sys.argv[12]),
}

checks = []
for name, exit_code in codes.items():
    log = out_root / f"{name.replace('_', '-')}.log"
    checks.append({
        "name": name,
        "status": "passed" if exit_code == 0 else "failed",
        "exit_code": exit_code,
        "log": str(log),
    })

payload = {
    "schema_version": "ao2.release-readiness-regression-gate.v1",
    "status": "passed" if all(code == 0 for code in codes.values()) else "failed",
    "artifact_root": str(out_root),
    "checks": checks,
    "hosted_release_readiness_artifact_gate": json.loads(
        (out_root / "hosted-release-readiness-artifact-gate" / "summary.json").read_text(encoding="utf-8")
    ),
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_role": "read_only_observer",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
if payload["status"] != "passed":
    raise SystemExit(1)
PY
