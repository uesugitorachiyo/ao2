#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RSI_CP_RELEASE_READINESS_DASHBOARD_SMOKE_ROOT:-$ROOT/target/rsi-control-plane-release-readiness-dashboard-smoke/latest}"
CP_ROOT="${AO2_CONTROL_PLANE_REPO:-${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}}"
BIND="${AO2_RSI_CP_RELEASE_READINESS_DASHBOARD_SMOKE_BIND:-127.0.0.1:19881}"
SUMMARY="$OUT_ROOT/summary.json"
FIXTURE_ROOT="$OUT_ROOT/fixture"
FIXTURE_SUMMARY="$FIXTURE_ROOT/release-train-summary.json"
FIXTURE_DASHBOARD="$FIXTURE_ROOT/dashboard.html"
BRIDGE_ROOT="$OUT_ROOT/release-train-control-plane-bridge"

CP_ROOT="$(cd "$CP_ROOT" && pwd)"

rm -rf "$OUT_ROOT"
mkdir -p "$FIXTURE_ROOT"

python3 - "$FIXTURE_SUMMARY" "$FIXTURE_DASHBOARD" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_path = Path(sys.argv[1]).resolve()
dashboard_path = Path(sys.argv[2]).resolve()
dashboard_path.write_text(
    """<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>AO2 Release Readiness Consumer</title></head>
<body>
  <h1>AO2 Release Readiness Consumer</h1>
  <p>RSI control-plane dashboard readback fixture.</p>
</body>
</html>
""",
    encoding="utf-8",
)
payload = {
    "schema_version": "ao2.public-release-train-drill.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "release_readiness_artifact_consumer_contract": {
        "status": "passed",
        "source_summary": str(dashboard_path.parent / "release-readiness-summary.json"),
        "required_check": "ci_release_readiness_artifact_consumer_job",
        "release_readiness_status": "passed",
        "check_detail": "RSI smoke fixture requires the control-plane readback to surface the dashboard artifact link.",
        "dashboard": str(dashboard_path),
        "dashboard_artifact": "ao2-release-readiness-consumer/dashboard.html",
        "dashboard_schema_version": "ao2.release-readiness-artifact-consumer.v1",
    },
    "checks": [
        {
            "name": "artifact_consumer",
            "status": "passed",
            "exit_code": 0,
            "log": str(dashboard_path.parent / "artifact-consumer.log"),
        },
        {
            "name": "rsi_eligibility_readback",
            "status": "passed",
            "exit_code": 0,
            "log": str(dashboard_path.parent / "rsi-eligibility-readback.log"),
        },
    ],
    "publish_guards": {
        "tag_push_publish_deploy": "not executed by this drill",
        "refuses_publish_side_effects_by_default": True,
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
        "mutates_repositories": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo build --release -p ao2-cp-server --manifest-path "$CP_ROOT/Cargo.toml"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  AO2_RELEASE_TRAIN_CP_BRIDGE_ROOT="$BRIDGE_ROOT" \
  AO2_RELEASE_TRAIN_SUMMARY="$FIXTURE_SUMMARY" \
  AO2_CONTROL_PLANE_ROOT="$CP_ROOT" \
  AO2_CP_RELEASE_TRAIN_BRIDGE_SMOKE_BIND="$BIND" \
  npm run release:train-control-plane-bridge -- \
    --summary "$FIXTURE_SUMMARY" \
    --control-plane-root "$CP_ROOT" \
    --out-root "$BRIDGE_ROOT" \
    --bind "$BIND"

python3 - "$SUMMARY" "$FIXTURE_SUMMARY" "$FIXTURE_DASHBOARD" "$BRIDGE_ROOT/latest" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_path = Path(sys.argv[1]).resolve()
fixture_summary_path = Path(sys.argv[2]).resolve()
dashboard_path = Path(sys.argv[3]).resolve()
bridge_latest = Path(sys.argv[4]).resolve()
bridge_summary_path = bridge_latest / "summary.json"
smoke_summary_path = bridge_latest / "control-plane-smoke" / "summary.json"
json_readback_path = bridge_latest / "control-plane-smoke" / "release-train-readback.json"
html_readback_path = bridge_latest / "control-plane-smoke" / "release-train-readback.html"

def load_json(path: Path) -> dict:
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))

bridge = load_json(bridge_summary_path)
smoke = load_json(smoke_summary_path)
observer_text = json_readback_path.read_text(encoding="utf-8") if json_readback_path.is_file() else ""
html = html_readback_path.read_text(encoding="utf-8") if html_readback_path.is_file() else ""
observer = json.loads(observer_text) if observer_text else {}
release_train = observer.get("release_train") if isinstance(observer.get("release_train"), dict) else {}
contract = (
    release_train.get("release_readiness_artifact_consumer_contract")
    if isinstance(release_train.get("release_readiness_artifact_consumer_contract"), dict)
    else {}
)

expected_artifact = "ao2-release-readiness-consumer/dashboard.html"
expected_schema = "ao2.release-readiness-artifact-consumer.v1"
local_dashboard = str(dashboard_path)
checks = []

def add_check(name: str, passed: bool, detail: str = "") -> None:
    checks.append({
        "name": name,
        "status": "passed" if passed else "failed",
        "detail": detail,
    })

add_check("bridge_schema", bridge.get("schema_version") == "ao2.release-train-control-plane-bridge.v1")
add_check("bridge_status", bridge.get("status") == "passed")
add_check("bridge_smoke_passed", bridge.get("control_plane", {}).get("smoke") == "passed")
add_check("smoke_schema", smoke.get("schema_version") == "ao2.cp-release-train-bridge-smoke.v1")
add_check("smoke_status", smoke.get("status") == "passed")
add_check("observer_schema", observer.get("schema_version") == "ao2.cp-release-train-readback.v1")
add_check("control_plane_role", observer.get("control_plane_role") == "read-only-observer")
add_check("control_plane_does_not_approve", observer.get("control_plane_approves_release") is False)
add_check("control_plane_does_not_mutate", observer.get("mutates_ao_artifacts") is False)
add_check("consumer_contract_status", contract.get("status") == "passed")
add_check("dashboard_artifact_json", contract.get("dashboard_artifact") == expected_artifact)
add_check("dashboard_schema_json", contract.get("dashboard_schema_version") == expected_schema)
add_check("local_dashboard_redacted_json", local_dashboard not in observer_text)
add_check("local_dashboard_redacted_html", local_dashboard not in html)
add_check("dashboard_artifact_html", expected_artifact in html)
add_check("dashboard_schema_html", expected_schema in html)
add_check("html_title", "AO2 Release Train Readback" in html)
add_check("html_dashboard_label", "AO2 release-readiness consumer dashboard" in html)
add_check("provider_key_names_absent_json", "OPENAI_API_KEY" not in observer_text and "ANTHROPIC_API_KEY" not in observer_text)
add_check("provider_key_names_absent_html", "OPENAI_API_KEY" not in html and "ANTHROPIC_API_KEY" not in html)

status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.rsi-control-plane-release-readiness-dashboard-smoke.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "dashboard_link_ready": status == "passed",
    "dashboard_artifact": expected_artifact,
    "dashboard_schema_version": expected_schema,
    "claim_publish_decision": "deny",
    "claim_publish_authority": False,
    "control_plane_approves_release": observer.get("control_plane_approves_release"),
    "mutates_ao_artifacts": observer.get("mutates_ao_artifacts"),
    "sources": {
        "fixture_release_train_summary": str(fixture_summary_path),
        "fixture_dashboard": str(dashboard_path),
        "bridge_summary": str(bridge_summary_path),
        "control_plane_smoke_summary": str(smoke_summary_path),
        "json_readback": str(json_readback_path),
        "html_readback": str(html_readback_path),
    },
    "checks": checks,
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "publishes_claims": False,
        "approves_rsi_claims": False,
        "mutates_repositories": False,
        "control_plane_approves_release": False,
        "mutates_ao_artifacts": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"dashboard_link_ready={str(payload['dashboard_link_ready']).lower()}")
print("claim_publish_decision=deny publish_authority=false")
if status != "passed":
    for check in checks:
        if check["status"] != "passed":
            print(f"failed={check['name']} {check['detail']}", file=sys.stderr)
    raise SystemExit(1)
PY
