#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INPUT_ROOT="${AO2_WORKBENCH_OPERATOR_PACKET_CP_INDEX_INPUT:-${1:-$ROOT/target/workbench-operator-packet-control-plane-smoke-ci-artifacts}}"
OUT_ROOT="${AO2_WORKBENCH_OPERATOR_PACKET_CP_INDEX_ROOT:-$ROOT/target/workbench-operator-packet-control-plane-smoke-index/latest}"
REQUIRED_OS="${AO2_WORKBENCH_OPERATOR_PACKET_CP_INDEX_REQUIRED_OS:-ubuntu-latest,macos-latest,windows-latest}"

python_command() {
  if [ -n "${PYTHON:-}" ]; then
    printf "%s\n" "$PYTHON"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    command -v python3
    return
  fi
  if command -v python >/dev/null 2>&1; then
    command -v python
    return
  fi
  echo "missing python interpreter; set PYTHON=/path/to/python" >&2
  return 1
}

PYTHON_BIN="$(python_command)"
mkdir -p "$OUT_ROOT"

"$PYTHON_BIN" - "$INPUT_ROOT" "$OUT_ROOT/summary.json" "$REQUIRED_OS" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

input_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
required_os = [item.strip() for item in sys.argv[3].split(",") if item.strip()]

def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def os_for_path(path: Path):
    text = path.as_posix()
    for os_name in required_os:
        if os_name in text:
            return os_name
    return None

checks = []
summaries_by_os = {os_name: [] for os_name in required_os}

if not input_root.exists():
    checks.append({
        "name": "input_root_exists",
        "status": "failed",
        "reason": f"missing input root {input_root}",
    })
else:
    checks.append({"name": "input_root_exists", "status": "passed"})
    for path in sorted(input_root.rglob("summary.json")):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except Exception as exc:
            checks.append({
                "name": "parse_summary",
                "status": "failed",
                "path": str(path),
                "reason": str(exc),
            })
            continue
        if payload.get("schema_version") != "ao2.workbench-operator-packet-control-plane-smoke.v1":
            continue
        os_name = os_for_path(path)
        if os_name is None:
            checks.append({
                "name": "summary_os",
                "status": "failed",
                "path": str(path),
                "reason": "missing_os",
            })
            continue
        summaries_by_os[os_name].append((path, payload))

os_results = []
for os_name in required_os:
    candidates = summaries_by_os.get(os_name, [])
    if not candidates:
        os_results.append({
            "os": os_name,
            "status": "failed",
            "reason": "missing_os",
        })
        continue

    path, payload = candidates[-1]
    operator_packet = payload.get("operator_packet", {})
    validations = {
        "schema": payload.get("schema_version") == "ao2.workbench-operator-packet-control-plane-smoke.v1",
        "status": payload.get("status") == "passed",
        "token_leak": payload.get("token_leak_detected") is False,
        "read_only_observer": payload.get("read_only_observer") is True,
        "evidence_pack": operator_packet.get("evidence_pack_schema_version") == "ao2.evidence-pack.v1",
        "evaluator_closure": operator_packet.get("evaluator_closure_verdict") == "accepted",
        "replay": operator_packet.get("replay_status") == "accepted",
        "provider_score": operator_packet.get("provider_score_present") is True,
    }
    failed = [name for name, ok in validations.items() if not ok]
    if failed:
        os_results.append({
            "os": os_name,
            "status": "failed",
            "reason": "operator_packet_validation_failed",
            "failed": failed,
            "summary_path": str(path),
            "summary_sha256": sha256_file(path),
        })
    else:
        os_results.append({
            "os": os_name,
            "status": "passed",
            "summary_path": str(path),
            "summary_sha256": sha256_file(path),
            "published_sha256": payload.get("published_sha256", ""),
            "run_id": payload.get("run_id", ""),
        })

checks.extend({
    "name": f"{item['os']}_operator_packet_smoke_summary",
    "status": item["status"],
    "reason": item.get("reason", ""),
} for item in os_results)

status = "passed" if checks and all(item.get("status") == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.workbench-operator-packet-control-plane-smoke-index.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "input_root": str(input_root),
    "required_os": required_os,
    "checks": checks,
    "os_summaries": os_results,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "validates_uploaded_smoke_evidence": True,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
