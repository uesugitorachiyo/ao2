#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT:-$ROOT/target/stable-release-evidence-packet/latest}"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY:-$ROOT/target/stable-promotion-workflow/latest/summary.json}"
AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY="${AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY:-$ROOT/target/operator-release-evidence-bundle/latest/summary.json}"
SUMMARY="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/summary.json"
DASHBOARD="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/dashboard.html"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --out-root)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT" ]; then
        echo "--out-root requires a path" >&2
        exit 2
      fi
      SUMMARY="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/summary.json"
      DASHBOARD="$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT/dashboard.html"
      shift 2
      ;;
    --stable-summary)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY" ]; then
        echo "--stable-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --operator-summary)
      AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY="${2:-}"
      if [ -z "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY" ]; then
        echo "--operator-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 [--out-root <path>] [--stable-summary <path>] [--operator-summary <path>]" >&2
      exit 2
      ;;
  esac
done

rm -rf "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT"
mkdir -p "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT"

python3 - "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY" \
  "$AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY" \
  "$SUMMARY" "$DASHBOARD" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

STABLE_SCHEMA = "ao2.stable-promotion-workflow.v1"
OPERATOR_SCHEMA = "ao2.operator-release-evidence-bundle.v1"
PACKET_SCHEMA = "ao2.stable-release-evidence-packet.v1"

stable_summary_path = Path(sys.argv[1]).resolve()
operator_summary_path = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
dashboard_path = Path(sys.argv[4]).resolve()


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


blockers = []
stable = {}
operator = {}

if not stable_summary_path.is_file():
    blockers.append(
        {
            "code": "stable_promotion_summary_missing",
            "severity": "blocking",
            "path": str(stable_summary_path),
        }
    )
else:
    stable = load_json(stable_summary_path)

if not operator_summary_path.is_file():
    blockers.append(
        {
            "code": "operator_evidence_summary_missing",
            "severity": "blocking",
            "path": str(operator_summary_path),
        }
    )
else:
    operator = load_json(operator_summary_path)

stable_schema_ok = stable.get("schema_version") == STABLE_SCHEMA
stable_status = stable.get("status")
stable_ready = (
    stable_schema_ok
    and stable_status in {"already_stable", "ready_to_promote"}
    and stable.get("post_release_evidence_ready") is True
    and stable.get("evidence_gate_status") == "passed"
)
operator_schema_ok = operator.get("schema_version") == OPERATOR_SCHEMA
operator_checks = operator.get("checks") if isinstance(operator.get("checks"), list) else []
passed_operator_checks = [
    check for check in operator_checks if check.get("status") == "passed"
]
operator_ready = (
    operator_schema_ok
    and operator.get("status") == "passed"
    and operator.get("operator_release_evidence_ready") is True
    and len(passed_operator_checks) == len(operator_checks)
)

if stable and not stable_schema_ok:
    blockers.append(
        {
            "code": "stable_promotion_schema_mismatch",
            "severity": "blocking",
            "expected": STABLE_SCHEMA,
            "actual": stable.get("schema_version"),
        }
    )
if stable and not stable_ready:
    blockers.append(
        {
            "code": "stable_promotion_not_ready",
            "severity": "blocking",
            "status": stable_status,
            "post_release_evidence_ready": stable.get("post_release_evidence_ready"),
            "evidence_gate_status": stable.get("evidence_gate_status"),
        }
    )
if operator and not operator_schema_ok:
    blockers.append(
        {
            "code": "operator_evidence_schema_mismatch",
            "severity": "blocking",
            "expected": OPERATOR_SCHEMA,
            "actual": operator.get("schema_version"),
        }
    )
if operator and not operator_ready:
    blockers.append(
        {
            "code": "operator_evidence_not_ready",
            "severity": "blocking",
            "status": operator.get("status"),
            "operator_release_evidence_ready": operator.get("operator_release_evidence_ready"),
            "passed_check_count": len(passed_operator_checks),
            "check_count": len(operator_checks),
        }
    )

source_trust = [
    stable.get("trust_boundary", {}) if isinstance(stable.get("trust_boundary"), dict) else {},
    operator.get("trust_boundary", {}) if isinstance(operator.get("trust_boundary"), dict) else {},
]
stable_release_evidence_ready = stable_ready and operator_ready and not blockers
payload = {
    "schema_version": PACKET_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed" if stable_release_evidence_ready else "failed",
    "stable_release_evidence_ready": stable_release_evidence_ready,
    "sources": {
        "stable_promotion_summary": str(stable_summary_path),
        "operator_evidence_summary": str(operator_summary_path),
    },
    "stable_promotion": {
        "schema_version": stable.get("schema_version"),
        "status": stable_status,
        "post_release_evidence_ready": stable.get("post_release_evidence_ready"),
        "evidence_gate_status": stable.get("evidence_gate_status"),
        "promotion_status": stable.get("promotion_status"),
        "blocker_count": len(stable.get("blockers", []))
        if isinstance(stable.get("blockers"), list)
        else None,
        "components": stable.get("components", []),
    },
    "operator_evidence": {
        "schema_version": operator.get("schema_version"),
        "status": operator.get("status"),
        "operator_release_evidence_ready": operator.get("operator_release_evidence_ready"),
        "check_count": len(operator_checks),
        "passed_check_count": len(passed_operator_checks),
        "checks": operator_checks,
    },
    "blockers": blockers,
    "trust_boundary": {
        "mutates_releases": False,
        "stores_credentials": False,
        "reads_local_evidence_only": True,
        "source_mutates_releases": [
            trust.get("mutates_releases", trust.get("mutates_github_releases"))
            for trust in source_trust
            if "mutates_releases" in trust or "mutates_github_releases" in trust
        ],
        "source_stores_credentials": [
            trust.get("stores_credentials", trust.get("credential_material_included"))
            for trust in source_trust
            if "stores_credentials" in trust or "credential_material_included" in trust
        ],
    },
    "dashboard": str(dashboard_path),
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

rows = []
for check in operator_checks:
    rows.append(
        "<tr>"
        f"<td>{html.escape(str(check.get('component', '')))}</td>"
        f"<td>{html.escape(str(check.get('platform', '')))}</td>"
        f"<td>{html.escape(str(check.get('artifact', '')))}</td>"
        f"<td>{html.escape(str(check.get('status', '')))}</td>"
        "</tr>"
    )
blocker_rows = []
for blocker in blockers:
    blocker_rows.append(
        "<tr>"
        f"<td>{html.escape(str(blocker.get('code', '')))}</td>"
        f"<td>{html.escape(str(blocker.get('severity', '')))}</td>"
        f"<td><code>{html.escape(json.dumps(blocker, sort_keys=True))}</code></td>"
        "</tr>"
    )
if not blocker_rows:
    blocker_rows.append('<tr><td colspan="3">No blockers</td></tr>')

dashboard_path.write_text(
    f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Stable Release Evidence Packet</title>
  <style>
    body {{ font-family: system-ui, sans-serif; margin: 2rem; color: #111827; }}
    h1, h2 {{ margin-bottom: 0.4rem; }}
    code {{ background: #f3f4f6; padding: 0.1rem 0.25rem; border-radius: 4px; }}
    table {{ border-collapse: collapse; width: 100%; margin: 1rem 0 2rem; }}
    th, td {{ border: 1px solid #d1d5db; padding: 0.5rem; text-align: left; vertical-align: top; }}
    th {{ background: #f9fafb; }}
    .status {{ font-weight: 700; }}
  </style>
</head>
<body>
  <h1>Stable Release Evidence Packet</h1>
  <p><code>{PACKET_SCHEMA}</code></p>
  <p class="status">Status: {html.escape(payload["status"])}</p>
  <p>Stable release evidence ready: {str(stable_release_evidence_ready).lower()}</p>
  <h2>Source Summaries</h2>
  <table>
    <tr><th>Source</th><th>Path</th><th>Status</th></tr>
    <tr><td>Stable promotion</td><td><code>{html.escape(str(stable_summary_path))}</code></td><td>{html.escape(str(stable_status))}</td></tr>
    <tr><td>Operator evidence</td><td><code>{html.escape(str(operator_summary_path))}</code></td><td>{html.escape(str(operator.get("status")))}</td></tr>
  </table>
  <h2>Operator Checks</h2>
  <table>
    <tr><th>Component</th><th>Platform</th><th>Artifact</th><th>Status</th></tr>
    {''.join(rows)}
  </table>
  <h2>Blockers</h2>
  <table>
    <tr><th>Code</th><th>Severity</th><th>Details</th></tr>
    {''.join(blocker_rows)}
  </table>
</body>
</html>
""",
    encoding="utf-8",
)

print(f"summary={summary_path}")
print(f"dashboard={dashboard_path}")
print(f"status={payload['status']}")
print(f"stable_release_evidence_ready={str(stable_release_evidence_ready).lower()}")
if not stable_release_evidence_ready:
    raise SystemExit(1)
PY
