#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CANDIDATE_BUNDLE="${AO2_CANDIDATE_READINESS_CANDIDATE_BUNDLE:-$ROOT/target/candidate-patch-release-rehearsal/report}"
MANIFEST_PARITY_SUMMARY="${AO2_CANDIDATE_READINESS_MANIFEST_PARITY_SUMMARY:-$ROOT/target/release-train-manifest-parity/latest/summary.json}"
CONTROL_PLANE_BRIDGE_ROOT="${AO2_CANDIDATE_READINESS_CONTROL_PLANE_BRIDGE_ROOT:-$ROOT/target/release-train-control-plane-bridge/latest}"
CONTROL_PLANE_BRIDGE_SUMMARY="${AO2_CANDIDATE_READINESS_CONTROL_PLANE_BRIDGE_SUMMARY:-}"
OUT_ROOT="${AO2_CANDIDATE_READINESS_PACKET_ROOT:-$ROOT/target/candidate-readiness-packet/latest}"

usage() {
  cat >&2 <<'EOF'
usage: candidate-readiness-packet.sh [options]

Options:
  --candidate-bundle <path>              Candidate rehearsal bundle root.
  --manifest-parity-summary <path>       Release-train manifest parity summary.
  --control-plane-bridge-root <path>     Release-train control-plane bridge root.
  --control-plane-bridge-summary <path>  Release-train control-plane bridge summary.
  --out-root <path>                      Candidate readiness packet output root.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --candidate-bundle)
      CANDIDATE_BUNDLE="${2:-}"
      if [ -z "$CANDIDATE_BUNDLE" ]; then
        echo "--candidate-bundle requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --manifest-parity-summary)
      MANIFEST_PARITY_SUMMARY="${2:-}"
      if [ -z "$MANIFEST_PARITY_SUMMARY" ]; then
        echo "--manifest-parity-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --control-plane-bridge-root)
      CONTROL_PLANE_BRIDGE_ROOT="${2:-}"
      if [ -z "$CONTROL_PLANE_BRIDGE_ROOT" ]; then
        echo "--control-plane-bridge-root requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --control-plane-bridge-summary)
      CONTROL_PLANE_BRIDGE_SUMMARY="${2:-}"
      if [ -z "$CONTROL_PLANE_BRIDGE_SUMMARY" ]; then
        echo "--control-plane-bridge-summary requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --out-root)
      OUT_ROOT="${2:-}"
      if [ -z "$OUT_ROOT" ]; then
        echo "--out-root requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
else
  echo "missing Python interpreter: python3 or python required" >&2
  exit 1
fi

if [ -z "$CONTROL_PLANE_BRIDGE_SUMMARY" ]; then
  CONTROL_PLANE_BRIDGE_SUMMARY="$CONTROL_PLANE_BRIDGE_ROOT/summary.json"
fi

"$python_bin" - "$CANDIDATE_BUNDLE" "$MANIFEST_PARITY_SUMMARY" "$CONTROL_PLANE_BRIDGE_ROOT" "$CONTROL_PLANE_BRIDGE_SUMMARY" "$OUT_ROOT" <<'PY'
import html
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

candidate_bundle = Path(sys.argv[1])
manifest_parity_summary_path = Path(sys.argv[2])
control_plane_bridge_root = Path(sys.argv[3])
control_plane_bridge_summary_path = Path(sys.argv[4])
out_root = Path(sys.argv[5])

PACKET_SCHEMA = "ao2.candidate-readiness-packet.v1"
RELEASE_TRAIN_SCHEMA = "ao2.public-release-train-drill.v1"
CANDIDATE_AUDIT_SCHEMA = "ao2.candidate-patch-release-rehearsal-audit.v1"
MANIFEST_PARITY_SCHEMA = "ao2.release-train-manifest-parity.v1"
BRIDGE_SCHEMA = "ao2.release-train-control-plane-bridge.v1"
SMOKE_SCHEMA = "ao2.cp-release-train-bridge-smoke.v1"

candidate_summary_path = candidate_bundle / "summary.json"
candidate_audit_path = candidate_bundle / "candidate-patch-release-rehearsal-audit.json"
candidate_closure_path = candidate_bundle / "closure.html"

failures = []


def load_json(path, label):
    if not path.is_file():
        failures.append(f"missing {label}: {path}")
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        failures.append(f"invalid {label} json: {path}: {exc}")
        return {}


def expect(condition, message):
    if not condition:
        failures.append(message)


def target_without_selected_train(targets):
    return {key: value for key, value in targets.items() if key != "selected_train"}


def status_from(payload):
    status = payload.get("status")
    return status if isinstance(status, str) else "missing"


candidate_summary = load_json(candidate_summary_path, "candidate release-train summary")
candidate_audit = load_json(candidate_audit_path, "candidate rehearsal audit")
manifest_parity = load_json(manifest_parity_summary_path, "release-train manifest parity summary")
control_plane_bridge = load_json(
    control_plane_bridge_summary_path,
    "release-train control-plane bridge summary",
)

release_targets = candidate_summary.get("release_targets", {})
if not isinstance(release_targets, dict):
    failures.append("candidate release_targets must be an object")
    release_targets = {}

candidate_manifest = candidate_summary.get("release_train_manifest", {})
candidate_guards = candidate_summary.get("publish_guards", {})
candidate_trust = candidate_summary.get("trust_boundary", {})
candidate_checks = candidate_summary.get("checks", [])

expect(candidate_summary.get("schema_version") == RELEASE_TRAIN_SCHEMA, "candidate summary schema mismatch")
expect(candidate_summary.get("status") == "passed", "candidate summary status was not passed")
expect(candidate_summary.get("ci_safe_mode") is True, "candidate summary ci_safe_mode was not true")
expect(candidate_manifest.get("schema_version") == "ao2.release-train-manifest.v1", "candidate manifest schema mismatch")
expect(candidate_manifest.get("selected_train") == "next_patch", "candidate selected_train was not next_patch")
expect(release_targets.get("selected_train") == "next_patch", "candidate release_targets selected_train was not next_patch")
expect(isinstance(candidate_checks, list) and bool(candidate_checks), "candidate checks must be a non-empty list")
for check in candidate_checks if isinstance(candidate_checks, list) else []:
    expect(check.get("status") == "passed", f"candidate check did not pass: {check}")
expect(candidate_guards.get("refuses_publish_side_effects_by_default") is True, "candidate publish guard did not refuse side effects")
expect(candidate_trust.get("local_only") is True, "candidate trust_boundary local_only was not true")
expect(candidate_trust.get("stores_credentials") is False, "candidate trust_boundary stores_credentials was not false")
expect(candidate_closure_path.is_file(), f"missing candidate closure html: {candidate_closure_path}")

candidate_audit_trust = candidate_audit.get("trust_boundary", {})
candidate_token_scan = candidate_audit.get("token_scan", {})
expect(candidate_audit.get("schema_version") == CANDIDATE_AUDIT_SCHEMA, "candidate audit schema mismatch")
expect(candidate_audit.get("status") == "passed", "candidate audit status was not passed")
expect(candidate_audit.get("release_targets") == release_targets, "candidate audit release_targets did not match candidate summary")
expect(candidate_token_scan.get("credential_material_included") is False, "candidate audit found credential material")
expect(candidate_audit_trust.get("mutates_github_releases") is False, "candidate audit could mutate GitHub releases")
expect(candidate_audit_trust.get("mutates_git_tags") is False, "candidate audit could mutate git tags")
expect(candidate_audit_trust.get("stores_credentials") is False, "candidate audit could store credentials")

parity_next_patch = manifest_parity.get("next_patch", {})
expect(manifest_parity.get("schema_version") == MANIFEST_PARITY_SCHEMA, "manifest parity schema mismatch")
expect(manifest_parity.get("status") == "passed", "manifest parity status was not passed")
expect(manifest_parity.get("byte_identical") is True, "manifest parity byte_identical was not true")
expect(manifest_parity.get("schema_aligned") is True, "manifest parity schema_aligned was not true")
expect(manifest_parity.get("target_aligned") is True, "manifest parity target_aligned was not true")
expect(parity_next_patch == target_without_selected_train(release_targets), "manifest parity next_patch targets did not match candidate release_targets")

bridge_release_train = control_plane_bridge.get("release_train", {})
bridge_control_plane = control_plane_bridge.get("control_plane", {})
bridge_trust = control_plane_bridge.get("trust_boundary", {})
expect(control_plane_bridge.get("schema_version") == BRIDGE_SCHEMA, "control-plane bridge schema mismatch")
expect(control_plane_bridge.get("status") == "passed", "control-plane bridge status was not passed")
expect(bridge_release_train.get("schema_version") == RELEASE_TRAIN_SCHEMA, "control-plane bridge release_train schema mismatch")
expect(bridge_release_train.get("status") == "passed", "control-plane bridge release_train status was not passed")
expect(bridge_control_plane.get("observer_schema") == SMOKE_SCHEMA, "control-plane observer schema mismatch")
expect(bridge_control_plane.get("role") == "read-only-observer", "control-plane bridge role was not read-only-observer")
expect(bridge_control_plane.get("credential_material_included") is False, "control-plane bridge included credential material")
expect(bridge_control_plane.get("credential_material_in_urls") is False, "control-plane bridge included credential material in urls")
expect(bridge_control_plane.get("smoke") == "passed", "control-plane readback smoke was not passed")
expect(bridge_trust.get("stores_credentials") is False, "control-plane bridge could store credentials")
expect(bridge_trust.get("control_plane_approves_release") is False, "control-plane bridge could approve release")
expect(bridge_trust.get("mutates_ao2_artifacts") is False, "control-plane bridge could mutate AO2 artifacts")
expect(bridge_trust.get("mutates_observer_storage") is False, "control-plane bridge could mutate observer storage")

smoke_summary_hint = bridge_control_plane.get("smoke_summary")
if isinstance(smoke_summary_hint, str) and smoke_summary_hint:
    smoke_summary_path = Path(smoke_summary_hint)
else:
    smoke_summary_path = control_plane_bridge_root / "control-plane-smoke" / "summary.json"
smoke_summary = load_json(smoke_summary_path, "control-plane readback smoke summary")
expect(smoke_summary.get("schema_version") == SMOKE_SCHEMA, "control-plane readback smoke schema mismatch")
expect(smoke_summary.get("status") == "passed", "control-plane readback smoke status was not passed")

status = "passed" if not failures else "failed"
candidate_readiness_ready = status == "passed"

components = {
    "candidate_rehearsal": {
        "schema_version": candidate_summary.get("schema_version"),
        "status": status_from(candidate_summary),
        "source_summary": str(candidate_summary_path),
        "audit_summary": str(candidate_audit_path),
        "ci_safe_mode": candidate_summary.get("ci_safe_mode"),
        "selected_train": release_targets.get("selected_train"),
    },
    "manifest_parity": {
        "schema_version": manifest_parity.get("schema_version"),
        "status": status_from(manifest_parity),
        "source_summary": str(manifest_parity_summary_path),
        "byte_identical": manifest_parity.get("byte_identical"),
        "schema_aligned": manifest_parity.get("schema_aligned"),
        "target_aligned": manifest_parity.get("target_aligned"),
    },
    "control_plane_bridge": {
        "schema_version": control_plane_bridge.get("schema_version"),
        "status": status_from(control_plane_bridge),
        "source_summary": str(control_plane_bridge_summary_path),
        "role": bridge_control_plane.get("role"),
        "credential_material_included": bridge_control_plane.get("credential_material_included"),
        "credential_material_in_urls": bridge_control_plane.get("credential_material_in_urls"),
    },
    "control_plane_readback": {
        "schema_version": smoke_summary.get("schema_version"),
        "status": status_from(smoke_summary),
        "source_summary": str(smoke_summary_path),
        "json_endpoint": bridge_control_plane.get("json_endpoint"),
        "html_endpoint": bridge_control_plane.get("html_endpoint"),
    },
}

summary_path = out_root / "summary.json"
packet_path = out_root / "packet.md"
dashboard_path = out_root / "dashboard.html"
summary = {
    "schema_version": PACKET_SCHEMA,
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "candidate_readiness_ready": candidate_readiness_ready,
    "release_targets": release_targets,
    "components": components,
    "sources": {
        "candidate_bundle": str(candidate_bundle),
        "candidate_summary": str(candidate_summary_path),
        "candidate_audit": str(candidate_audit_path),
        "manifest_parity_summary": str(manifest_parity_summary_path),
        "control_plane_bridge_root": str(control_plane_bridge_root),
        "control_plane_bridge_summary": str(control_plane_bridge_summary_path),
        "control_plane_readback_summary": str(smoke_summary_path),
    },
    "operator_packet": str(packet_path),
    "dashboard": str(dashboard_path),
    "failures": failures,
    "trust_boundary": {
        "local_only": True,
        "mutates_github_releases": False,
        "mutates_git_tags": False,
        "mutates_ao2_artifacts": False,
        "mutates_observer_storage": False,
        "stores_credentials": False,
        "credential_material_included": False,
        "control_plane_approves_release": False,
    },
}

out_root.mkdir(parents=True, exist_ok=True)
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

target_lines = []
for key in ("ao2", "ao2_control_plane"):
    value = release_targets.get(key, {})
    if isinstance(value, dict):
        target_lines.append(f"- {key}: {value.get('tag', 'unknown')} ({value.get('version', 'unknown')})")
target_text = "\n".join(target_lines) if target_lines else "- unavailable"

component_lines = []
for name, payload in components.items():
    component_lines.append(f"- {name}: {payload.get('status', 'missing')}")
component_text = "\n".join(component_lines)
failure_text = "\n".join(f"- {failure}" for failure in failures) if failures else "- none"

packet_path.write_text(
    "\n".join(
        [
            "# AO2 Candidate Readiness Packet",
            "",
            f"Status: {status}",
            f"Candidate readiness ready: {str(candidate_readiness_ready).lower()}",
            "",
            "## Release Targets",
            target_text,
            "",
            "## Evidence Components",
            component_text,
            "",
            "## Trust Boundary",
            "- Reads local evidence only",
            "- Does not create releases, push tags, publish packages, or store credentials",
            "- Keeps ao2-control-plane as a read-only observer",
            "",
            "## Failures",
            failure_text,
            "",
        ]
    ),
    encoding="utf-8",
)

rows = []
for name, payload in components.items():
    rows.append(
        "<tr>"
        f"<td>{html.escape(name)}</td>"
        f"<td>{html.escape(str(payload.get('status', 'missing')))}</td>"
        f"<td>{html.escape(str(payload.get('schema_version', '')))}</td>"
        "</tr>"
    )
failures_html = "".join(f"<li>{html.escape(failure)}</li>" for failure in failures) or "<li>none</li>"
dashboard_path.write_text(
    "\n".join(
        [
            "<!doctype html>",
            "<html lang=\"en\">",
            "<head>",
            "  <meta charset=\"utf-8\">",
            "  <title>AO2 Candidate Readiness Packet</title>",
            "  <style>",
            "    body { font-family: system-ui, sans-serif; margin: 2rem; color: #1f2937; }",
            "    table { border-collapse: collapse; width: 100%; max-width: 960px; }",
            "    th, td { border: 1px solid #d1d5db; padding: 0.5rem; text-align: left; }",
            "    th { background: #f3f4f6; }",
            "    .status { font-weight: 700; }",
            "  </style>",
            "</head>",
            "<body>",
            "  <h1>AO2 Candidate Readiness Packet</h1>",
            f"  <p class=\"status\">Status: {html.escape(status)}</p>",
            f"  <p>Candidate readiness ready: {html.escape(str(candidate_readiness_ready).lower())}</p>",
            "  <h2>Components</h2>",
            "  <table>",
            "    <thead><tr><th>Component</th><th>Status</th><th>Schema</th></tr></thead>",
            f"    <tbody>{''.join(rows)}</tbody>",
            "  </table>",
            "  <h2>Failures</h2>",
            f"  <ul>{failures_html}</ul>",
            "</body>",
            "</html>",
            "",
        ]
    ),
    encoding="utf-8",
)

if status == "passed":
    print("candidate_readiness_packet=passed")
else:
    print("candidate_readiness_packet=failed")
print(f"candidate_readiness_packet_summary={summary_path}")
print(f"candidate_readiness_packet_markdown={packet_path}")
print(f"candidate_readiness_packet_dashboard={dashboard_path}")
if status != "passed":
    raise SystemExit(1)
PY
