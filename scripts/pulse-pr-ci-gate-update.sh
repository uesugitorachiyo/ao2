#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_PR_CI_GATE_UPDATE_ROOT:-$ROOT/target/pulse-pr-ci-gate-update/latest}"
SUMMARY="$OUT_ROOT/summary.json"
STATE="${AO2_PULSE_PR_CI_GATE_UPDATE_STATE:-$ROOT/.ao2-local/pulse/pr-ci-gate.json}"
SOURCE_JSON="${AO2_PULSE_PR_CI_GATE_UPDATE_SOURCE_JSON:-}"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT" "$(dirname "$STATE")"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$STATE" "$SOURCE_JSON" <<'PY'
import json
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
state_path = Path(sys.argv[4]).resolve()
source_json = sys.argv[5]

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def normalize_status(value) -> str:
    text = str(value or "").strip().upper()
    if text in {"", "NONE", "NULL"}:
        return "PENDING"
    if text in {"SUCCESS", "PASSED"}:
        return "SUCCESS"
    if text in {"NEUTRAL", "SKIPPED"}:
        return text
    if text in {"FAILURE", "FAILED", "ERROR", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED"}:
        return text
    if text in {"PENDING", "QUEUED", "IN_PROGRESS", "REQUESTED", "EXPECTED"}:
        return "PENDING"
    return text

def check_is_green(check: dict) -> bool:
    conclusion = normalize_status(check.get("conclusion"))
    state = normalize_status(check.get("state") or check.get("status"))
    if conclusion in {"SUCCESS", "NEUTRAL", "SKIPPED"}:
        return True
    if conclusion != "PENDING":
        return False
    return state in {"SUCCESS", "NEUTRAL", "SKIPPED"}

def load_pr_view():
    if source_json:
        source_path = Path(source_json).resolve()
        data = json.loads(source_path.read_text(encoding="utf-8"))
        return data, {"kind": "fixture", "path": str(source_path)}

    gh_path = shutil.which("gh")
    if gh_path is None:
        return None, {"kind": "gh", "status": "unavailable", "reason": "gh_not_found"}

    command = [
        gh_path,
        "pr",
        "view",
        "--json",
        "number,state,isDraft,headRefName,mergeStateStatus,url,statusCheckRollup",
    ]
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if result.returncode != 0:
        stderr = result.stderr.strip()
        lowered = stderr.lower()
        if "no pull requests found" in lowered or "no pull request found" in lowered:
            return None, {"kind": "gh", "status": "no_open_pr", "command": "gh pr view"}
        return None, {
            "kind": "gh",
            "status": "failed",
            "command": "gh pr view",
            "exit_code": int(result.returncode),
            "stderr": stderr,
        }
    return json.loads(result.stdout), {"kind": "gh", "status": "passed", "command": "gh pr view"}

def materialize_state(pr_view, source: dict) -> dict:
    base = {
        "schema_version": "ao2.pulse-pr-ci-gate.v1",
        "generated_at_utc": utc_now(),
        "status": "passed",
        "reason": "passed",
        "source": source,
        "branch": None,
        "pr": None,
        "required_checks": [],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }

    if pr_view is None:
        if source.get("status") in {"unavailable", "failed"}:
            base["status"] = "waiting"
            base["reason"] = "waiting_for_pr_merge_or_ci"
            base["detail"] = source.get("reason") or source.get("status")
        else:
            base["reason"] = "no_open_pr"
        return base

    checks = []
    for item in pr_view.get("statusCheckRollup") or []:
        if not isinstance(item, dict):
            continue
        status = normalize_status(item.get("conclusion") or item.get("state") or item.get("status"))
        checks.append({
            "name": str(item.get("name") or item.get("context") or "unnamed-check"),
            "status": status,
            "state": normalize_status(item.get("state") or item.get("status")),
            "conclusion": normalize_status(item.get("conclusion")),
        })

    pr_state = str(pr_view.get("state") or "").upper()
    is_draft = bool(pr_view.get("isDraft"))
    checks_green = all(check_is_green(check) for check in checks)
    pr_merged_or_closed = pr_state in {"MERGED", "CLOSED"}

    base.update({
        "branch": pr_view.get("headRefName"),
        "pr": {
            "number": pr_view.get("number"),
            "state": pr_state,
            "is_draft": is_draft,
            "url": pr_view.get("url"),
        },
        "merge_state_status": pr_view.get("mergeStateStatus"),
        "required_checks": checks,
    })

    if is_draft:
        base["status"] = "waiting"
        base["reason"] = "waiting_for_pr_merge_or_ci"
        base["detail"] = "pr_draft"
    elif not pr_merged_or_closed:
        base["status"] = "waiting"
        base["reason"] = "waiting_for_pr_merge_or_ci"
        base["detail"] = "pr_open"
    elif not checks_green:
        base["status"] = "waiting"
        base["reason"] = "waiting_for_pr_merge_or_ci"
        base["detail"] = "required_checks_not_green"
    else:
        base["status"] = "passed"
        base["reason"] = "pr_merged_or_no_open_pr"
    return base

pr_view, source = load_pr_view()
state = materialize_state(pr_view, source)
state_path.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n", encoding="utf-8")

summary = {
    "schema_version": "ao2.pulse-pr-ci-gate-update.v1",
    "generated_at_utc": utc_now(),
    "status": state["status"],
    "reason": state["reason"],
    "state_path": str(state_path),
    "source": source,
    "pr_ci_gate": state,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"state={state_path}")
print(f"status={summary['status']}")
PY
