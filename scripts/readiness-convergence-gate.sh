#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_READINESS_CONVERGENCE_ROOT:-$ROOT/target/readiness-convergence/latest}"
SUMMARY="$OUT_ROOT/summary.json"
REPORT="$OUT_ROOT/report.md"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$REPORT" <<'PY'
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path


root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
report_path = Path(sys.argv[4]).resolve()


def latest_summary(glob_pattern):
    matches = [path for path in root.glob(glob_pattern) if path.is_file()]
    if not matches:
        return None
    return max(matches, key=lambda path: path.stat().st_mtime)


def configured_path(env_name, fallback):
    configured = os.environ.get(env_name)
    if configured:
        return Path(configured).expanduser().resolve()
    if isinstance(fallback, Path):
        return fallback.resolve()
    discovered = latest_summary(fallback)
    return discovered.resolve() if discovered else root / fallback


components = [
    {
        "id": "risky_pr_product_readiness",
        "env": "AO2_READINESS_CONVERGENCE_RISKY_PR_PRODUCT_READINESS",
        "path": root / "target/risky-pr-product-readiness/latest/summary.json",
        "schema": "ao2.risky-pr-product-readiness-gate.v1",
        "allowed_statuses": ["passed"],
    },
    {
        "id": "release_evidence_closure",
        "env": "AO2_READINESS_CONVERGENCE_RELEASE_EVIDENCE_CLOSURE",
        "path": root / "target/release-evidence-closure/latest/summary.json",
        "schema": "ao2.release-evidence-closure.v1",
        "allowed_statuses": ["accepted"],
    },
    {
        "id": "release_readiness_static",
        "env": "AO2_READINESS_CONVERGENCE_RELEASE_READINESS_STATIC",
        "path": "target/release-readiness/*/summary.json",
        "schema": "ao2.release-readiness-local.v1",
        "allowed_statuses": ["passed"],
    },
    {
        "id": "release_readiness_regression",
        "env": "AO2_READINESS_CONVERGENCE_RELEASE_READINESS_REGRESSION",
        "path": "target/release-readiness-regression-gate/*/summary.json",
        "schema": "ao2.release-readiness-regression-gate.v1",
        "allowed_statuses": ["passed"],
    },
    {
        "id": "release_asset_publication_readiness",
        "env": "AO2_READINESS_CONVERGENCE_RELEASE_ASSET_PUBLICATION_READINESS",
        "path": root / "target/release-asset-publication-readiness/latest/summary.json",
        "schema": "ao2.release-asset-publication-readiness.v1",
        "allowed_statuses": ["passed"],
        "requires_publish_guards": True,
    },
    {
        "id": "public_ship_dry_run",
        "env": "AO2_READINESS_CONVERGENCE_PUBLIC_SHIP_DRY_RUN",
        "path": root / "target/public-ship-dry-run/latest/summary.json",
        "schema": "ao2.public-ship-dry-run.v1",
        "allowed_statuses": ["passed"],
        "requires_publish_guards": True,
    },
    {
        "id": "release_cutover_readiness_lock",
        "env": "AO2_READINESS_CONVERGENCE_RELEASE_CUTOVER_LOCK",
        "path": root / "target/release-cutover-readiness-lock/latest/summary.json",
        "schema": "ao2.release-cutover-readiness-lock.v1",
        "allowed_statuses": ["passed"],
        "requires_publish_guards": True,
    },
    {
        "id": "pulse_terminal_eval_loop_schema_compatibility",
        "env": "AO2_READINESS_CONVERGENCE_PULSE_TERMINAL_SCHEMA",
        "path": root / "target/pulse-terminal-eval-loop-schema-compatibility/latest/summary.json",
        "schema": "ao2.pulse-terminal-eval-loop-schema-compatibility.v1",
        "allowed_statuses": ["passed"],
    },
    {
        "id": "pulse_auto_advance_integration_gate",
        "env": "AO2_READINESS_CONVERGENCE_PULSE_AUTO_ADVANCE",
        "path": root / "target/pulse-auto-advance-integration-gate/latest/summary.json",
        "schema": "ao2.pulse-auto-advance-integration-gate.v1",
        "allowed_statuses": ["passed"],
    },
    {
        "id": "pulse_resume_dry_run",
        "env": "AO2_READINESS_CONVERGENCE_PULSE_RESUME",
        "path": root / "target/pulse-resume/latest/summary.json",
        "schema": "ao2.pulse-resume.v1",
        "allowed_statuses": ["dry_run"],
    },
    {
        "id": "pulse_daemon_status",
        "env": "AO2_READINESS_CONVERGENCE_PULSE_DAEMON",
        "path": root / "target/pulse-daemon/latest/summary.json",
        "schema": "ao2.pulse-daemon.v1",
        "allowed_statuses": ["stopped"],
    },
]


def iter_dicts(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from iter_dicts(child)
    elif isinstance(value, list):
        for child in value:
            yield from iter_dicts(child)


def forbidden_trust_boundary_findings(payload):
    forbidden_true_keys = {
        "auth_value_stored",
        "credential_material_in_urls",
        "credential_material_included",
        "stores_credentials",
        "mutates_release",
        "mutates_github_releases",
        "control_plane_approves_release",
        "control_plane_approves_rsi",
    }
    findings = []
    for item in iter_dicts(payload):
        for key, value in item.items():
            if key in forbidden_true_keys and value is True:
                findings.append(f"{key}=true")
    return findings


def publish_guard_findings(payload):
    findings = []
    found_guard = False
    for item in iter_dicts(payload):
        guards = item.get("publish_guards")
        if not isinstance(guards, dict):
            continue
        found_guard = True
        for key, value in guards.items():
            value_text = str(value).lower()
            if "not executed" not in value_text and "not_executed" not in value_text:
                findings.append(f"{key}={value}")
    if not found_guard:
        findings.append("publish_guards missing")
    return findings


component_results = []
blocking_next_actions = []

for component in components:
    component_path = configured_path(component["env"], component["path"])
    result = {
        "component_id": component["id"],
        "summary_path": str(component_path),
        "expected_schema": component["schema"],
        "allowed_statuses": component["allowed_statuses"],
        "status": "passed",
        "observed_schema": None,
        "observed_status": None,
        "findings": [],
    }

    if not component_path.is_file():
        result["status"] = "failed"
        result["findings"].append("summary missing")
    else:
        try:
            payload = json.loads(component_path.read_text(encoding="utf-8"))
        except Exception as exc:
            payload = {}
            result["status"] = "failed"
            result["findings"].append(f"summary unreadable: {exc}")

        if payload:
            result["observed_schema"] = payload.get("schema_version")
            result["observed_status"] = payload.get("status")
            if result["observed_schema"] != component["schema"]:
                result["status"] = "failed"
                result["findings"].append("schema mismatch")
            if result["observed_status"] not in component["allowed_statuses"]:
                result["status"] = "failed"
                result["findings"].append("status not allowed")
            for finding in forbidden_trust_boundary_findings(payload):
                result["status"] = "failed"
                result["findings"].append(f"trust boundary violation: {finding}")
            if component.get("requires_publish_guards"):
                for finding in publish_guard_findings(payload):
                    result["status"] = "failed"
                    result["findings"].append(f"publish guard violation: {finding}")

    component_results.append(result)
    if result["status"] != "passed":
        blocking_next_actions.append(
            {
                "component_id": component["id"],
                "summary_path": str(component_path),
                "action": "repair_or_regenerate_readiness_evidence",
                "findings": result["findings"],
            }
        )

readiness_converged = not blocking_next_actions
status = "passed" if readiness_converged else "failed"
recommended_next_action = (
    "operator_release_decision_required"
    if readiness_converged
    else "repair_readiness_evidence"
)
continue_pulse_loop = not readiness_converged

payload = {
    "schema_version": "ao2.readiness-convergence-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "report_md": str(report_path),
    "readiness_converged": readiness_converged,
    "continue_pulse_loop": continue_pulse_loop,
    "recommended_next_action": recommended_next_action,
    "decision": {
        "operator_release_decision_required": readiness_converged,
        "release_mutation_authority": False,
        "control_plane_observer_only": True,
    },
    "rsi_claim_boundary": {
        "bounded_governed_rsi": "supported",
        "full_autonomous_self_mutating_rsi": "denied",
        "claim_publish_authority": False,
        "improvement_score_means": "evidence coverage improvement, not full autonomous RSI authority",
    },
    "components": component_results,
    "blocking_next_actions": blocking_next_actions,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "mutates_release": False,
        "control_plane_role": "read_only_observer",
    },
}

summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

lines = [
    "# AO2 Readiness Convergence Gate",
    "",
    f"- status: {status}",
    f"- readiness_converged: {str(readiness_converged).lower()}",
    f"- continue_pulse_loop: {str(continue_pulse_loop).lower()}",
    f"- recommended_next_action: {recommended_next_action}",
    "- bounded_governed_rsi: supported",
    "- full_autonomous_self_mutating_rsi: denied",
    "- claim_publish_authority: false",
    "- control_plane_role: read_only_observer",
    "",
    "## Components",
    "",
    "| component | status | observed status | summary |",
    "| --- | --- | --- | --- |",
]
for component in component_results:
    lines.append(
        "| {component_id} | {status} | {observed_status} | {summary_path} |".format(
            component_id=component["component_id"],
            status=component["status"],
            observed_status=component.get("observed_status"),
            summary_path=component["summary_path"],
        )
    )
if blocking_next_actions:
    lines.extend(["", "## Blocking Next Actions", ""])
    for blocker in blocking_next_actions:
        lines.append(
            "- {component_id}: {findings}".format(
                component_id=blocker["component_id"],
                findings=", ".join(blocker["findings"]),
            )
        )
else:
    lines.extend(
        [
            "",
            "## Operator Decision",
            "",
            "Readiness evidence has converged. Stop repeating the Pulse evidence loop and request an explicit operator release decision.",
        ]
    )
report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"status={status}")
print(f"recommended_next_action={recommended_next_action}")
if status != "passed":
    raise SystemExit(1)
PY
