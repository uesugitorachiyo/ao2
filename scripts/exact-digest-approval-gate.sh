#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_EXACT_DIGEST_APPROVAL_GATE_ROOT:-$ROOT/target/exact-digest-approval-gate/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="$OUT_ROOT/fixtures"
RISKY_ROOT="$OUT_ROOT/risky-pr-golden"
RISKY_SUMMARY="${AO2_EXACT_DIGEST_APPROVAL_GATE_RISKY_SUMMARY:-}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR" "$FIXTURE_DIR"

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

if [ -z "$RISKY_SUMMARY" ]; then
  run_step risky_pr_golden \
    env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
      AO2_RISKY_PR_GOLDEN_ROOT="$RISKY_ROOT" \
      npm run risky-pr:golden
  RISKY_SUMMARY="$RISKY_ROOT/summary.json"
else
  printf "%s\n" "0" >"$LOG_DIR/risky_pr_golden.log.exit-code"
  printf "using existing risky summary: %s\n" "$RISKY_SUMMARY" >"$LOG_DIR/risky_pr_golden.log"
fi

run_step tampered_approval_request \
  env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
    cargo test -p ao2-runtime --test approval_replay approve_rejects_tampered_approval_request

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$FIXTURE_DIR" "$RISKY_SUMMARY" "$ROOT/scripts/exact-digest-approval-gate.sh" <<'PY'
import copy
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
fixture_dir = Path(sys.argv[4]).resolve()
risky_summary_path = Path(sys.argv[5]).resolve()
script_path = Path(sys.argv[6]).resolve()


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)


def sha256ish(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def check(name, passed, evidence=None):
    return {
        "name": name,
        "status": "passed" if passed else "failed",
        "evidence": evidence or {},
    }


def write_fixture(name, payload):
    path = fixture_dir / f"{name}.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


risky_exit = int((log_dir / "risky_pr_golden.log.exit-code").read_text(encoding="utf-8").strip())
tamper_exit = int((log_dir / "tampered_approval_request.log.exit-code").read_text(encoding="utf-8").strip())
risky_summary = load_json(risky_summary_path) if risky_summary_path.is_file() else {}
evidence_pack_path = Path(str(risky_summary.get("evidence_pack", ""))).resolve()
report_path = Path(str(risky_summary.get("report", ""))).resolve()
report_index_path = Path(str(risky_summary.get("report_index", ""))).resolve()
replay_path = risky_summary_path.parent / "replay.json"
approve_path = risky_summary_path.parent / "approve.txt"
run_paused_path = risky_summary_path.parent / "run-paused.txt"

pack = load_json(evidence_pack_path) if evidence_pack_path.is_file() else {}
report_index = load_json(report_index_path) if report_index_path.is_file() else {}
replay = load_json(replay_path) if replay_path.is_file() else {}
report_html = report_path.read_text(encoding="utf-8", errors="replace") if report_path.is_file() else ""
approve_text = approve_path.read_text(encoding="utf-8", errors="replace") if approve_path.is_file() else ""
run_paused_text = run_paused_path.read_text(encoding="utf-8", errors="replace") if run_paused_path.is_file() else ""
pack_text = "\n".join(strings(pack)).lower()

policy_decisions = [item for item in pack.get("policy_decisions") or [] if isinstance(item, dict)]
approvals = [item for item in pack.get("approvals") or [] if isinstance(item, dict)]
denied_git_push = [
    item
    for item in policy_decisions
    if item.get("action") == "git:push" and str(item.get("decision", "")).lower() == "deny"
]
approved_actions = [
    item
    for item in approvals
    if str(item.get("status", "")).lower() == "approved" and sha256ish(item.get("action_digest"))
]
allowed_write = [
    item
    for item in policy_decisions
    if item.get("action") == "filesystem:write_file"
    and str(item.get("decision", "")).lower() == "allow"
    and sha256ish(item.get("request_digest"))
]
approved_digests = {item["action_digest"] for item in approved_actions}
denied_digests = {
    item.get("request_digest")
    for item in denied_git_push
    if sha256ish(item.get("request_digest"))
}
allowed_digests = {item.get("request_digest") for item in allowed_write}

modified_approval = copy.deepcopy(approved_actions[0]) if approved_actions else {}
if modified_approval:
    modified_approval["action_digest"] = "0" * 64
    modified_approval["status"] = "approved"
modified_fixture_path = write_fixture("modified_digest_approval_attempt", modified_approval)

broad_action_denied = bool(denied_git_push) and "policy_denied_git_push" in pack_text
exact_action_approved = (
    bool(approved_actions)
    and approved_digests == allowed_digests
    and approved_digests.isdisjoint(denied_digests)
    and "status=approved" in approve_text
    and "approval_status=pending" in run_paused_text
)
modified_digest_rejected = (
    bool(modified_approval)
    and modified_approval["action_digest"] not in approved_digests
    and tamper_exit == 0
)
replay_digest_integrity = replay.get("status") == "accepted" and replay.get("digest_failures") in ([], None)
operator_answers = report_index.get("operator_answers") or {}
report_exposes_digest_boundary = (
    report_index.get("schema_version") == "ao2.risky-pr-static-report-index.v1"
    and operator_answers.get("denied_actions") is True
    and operator_answers.get("approved_actions") is True
    and operator_answers.get("replay_status") is True
    and "Policy Decisions" in report_html
    and "Approvals" in report_html
    and "Replay Evidence" in report_html
)
provider_keys_unset = "env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY" in script_path.read_text(encoding="utf-8")

checks = [
    check(
        "risky_pr_golden",
        risky_exit == 0 and risky_summary.get("schema_version") == "ao2.risky-pr-golden-path.v1",
        {"summary": str(risky_summary_path), "exit_code": risky_exit},
    ),
    check(
        "broad_action_denied",
        broad_action_denied,
        {
            "denied_actions": [
                {
                    "action": item.get("action"),
                    "resource": item.get("resource"),
                    "request_digest": item.get("request_digest"),
                }
                for item in denied_git_push
            ]
        },
    ),
    check(
        "exact_action_approved",
        exact_action_approved,
        {
            "approved_digests": sorted(approved_digests),
            "allowed_write_digests": sorted(allowed_digests),
            "denied_digests": sorted(denied_digests),
            "approve_output": str(approve_path),
            "pause_output": str(run_paused_path),
        },
    ),
    check(
        "modified_digest_rejected",
        modified_digest_rejected,
        {
            "fixture": str(modified_fixture_path),
            "attempted_digest": modified_approval.get("action_digest"),
            "approved_digests": sorted(approved_digests),
            "runtime_tamper_test": str(log_dir / "tampered_approval_request.log"),
            "runtime_tamper_exit_code": tamper_exit,
        },
    ),
    check(
        "replay_digest_integrity",
        replay_digest_integrity,
        {
            "replay": str(replay_path),
            "status": replay.get("status"),
            "digest_failures": replay.get("digest_failures"),
        },
    ),
    check(
        "report_exposes_digest_boundary",
        report_exposes_digest_boundary,
        {"report": str(report_path), "report_index": str(report_index_path)},
    ),
    check(
        "provider_keys_unset",
        provider_keys_unset,
        {"uses_provider_api_keys": False},
    ),
]

status = "passed" if all(item["status"] == "passed" for item in checks) else "failed"
payload = {
    "schema_version": "ao2.exact-digest-approval-gate.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "risky_pr_golden_summary": str(risky_summary_path),
    "evidence_pack": str(evidence_pack_path),
    "report": str(report_path),
    "report_index": str(report_index_path),
    "replay": str(replay_path),
    "checks": checks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "uses_provider_api_keys": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
