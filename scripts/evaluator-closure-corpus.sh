#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_EVALUATOR_CLOSURE_CORPUS_ROOT:-$ROOT/target/evaluator-closure-corpus/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
CASE_DIR="$OUT_ROOT/cases"
RISKY_ROOT="$OUT_ROOT/risky-pr-golden"
RISKY_SUMMARY="${AO2_EVALUATOR_CLOSURE_CORPUS_RISKY_SUMMARY:-}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR" "$CASE_DIR"

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

python3 - "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$CASE_DIR" "$RISKY_SUMMARY" <<'PY'
import copy
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out_root = Path(sys.argv[1]).resolve()
summary_path = Path(sys.argv[2]).resolve()
log_dir = Path(sys.argv[3]).resolve()
case_dir = Path(sys.argv[4]).resolve()
risky_summary_path = Path(sys.argv[5]).resolve()

CASE_IDS = [
    "missing_test_evidence",
    "unresolved_high_concern",
    "invalid_artifact_digest",
    "unapproved_risky_action",
    "accepted_after_correction",
]


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


def last_closure(pack):
    closures = pack.get("closures") or []
    if not closures:
        return {}
    return closures[-1] if isinstance(closures[-1], dict) else {}


def artifact_path(uri, source_pack_path):
    if not isinstance(uri, str) or not uri:
        return None
    if uri.startswith("file://"):
        return Path(uri[7:])
    path = Path(uri)
    if path.is_absolute():
        return path
    return (source_pack_path.parent / path).resolve()


def artifact_digest_failures(pack, source_pack_path):
    failures = []
    for index, artifact in enumerate(pack.get("artifacts") or []):
        if not isinstance(artifact, dict):
            failures.append({"index": index, "reason": "artifact_not_object"})
            continue
        digest = artifact.get("digest") or artifact.get("sha256")
        uri = artifact.get("uri") or artifact.get("path")
        if not digest or not uri:
            continue
        path = artifact_path(uri, source_pack_path)
        if path is None or not path.is_file():
            failures.append({"index": index, "reason": "artifact_file_missing", "uri": uri})
            continue
        expected = str(digest)
        if expected.startswith("sha256:"):
            expected = expected.split(":", 1)[1]
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            failures.append(
                {
                    "index": index,
                    "reason": "artifact_digest_mismatch",
                    "uri": uri,
                    "expected": expected,
                    "actual": actual,
                }
            )
    return failures


def evaluate(pack, source_pack_path):
    closure = last_closure(pack)
    text = "\n".join(strings(pack)).lower()
    artifact_text = "\n".join(strings(pack.get("artifacts") or [])).lower()
    closure_text = "\n".join(strings(pack.get("closures") or [])).lower()
    artifact_failures = artifact_digest_failures(pack, source_pack_path)
    unresolved = closure.get("unresolved_concerns") or []
    blockers = closure.get("blockers") or []
    approvals = pack.get("approvals") or []
    approved_actions = [
        approval
        for approval in approvals
        if isinstance(approval, dict) and str(approval.get("status", "")).lower() == "approved"
    ]
    checks = {
        "evidence_pack_schema": pack.get("schema_version") == "ao2.evidence-pack.v1",
        "accepted_verdict": pack.get("verdict") == "accepted",
        "test_evidence_present": "test" in artifact_text or (
            "test" in closure_text and "evidence" in closure_text
        ),
        "no_unresolved_concerns": len(unresolved) == 0,
        "no_blockers": len(blockers) == 0,
        "artifact_digests_valid": len(artifact_failures) == 0,
        "exact_approval_observed": bool(approved_actions) and "approval" in text,
    }
    reasons = [name for name, passed in checks.items() if not passed]
    return {
        "actual": "accepted" if not reasons else "rejected",
        "checks": checks,
        "reasons": reasons,
        "artifact_digest_failures": artifact_failures,
    }


def remove_test_evidence(pack):
    mutated = copy.deepcopy(pack)
    mutated["artifacts"] = [
        artifact
        for artifact in mutated.get("artifacts") or []
        if "test" not in json.dumps(artifact, sort_keys=True).lower()
    ]
    for closure in mutated.get("closures") or []:
        if isinstance(closure, dict):
            closure["acceptance_criteria_results"] = [
                item
                for item in closure.get("acceptance_criteria_results") or []
                if "test" not in json.dumps(item, sort_keys=True).lower()
            ]
            closure["evidence_refs"] = [
                item
                for item in closure.get("evidence_refs") or []
                if "test" not in json.dumps(item, sort_keys=True).lower()
            ]
    return mutated


def add_unresolved_high_concern(pack):
    mutated = copy.deepcopy(pack)
    closure = last_closure(mutated)
    closure.setdefault("unresolved_concerns", []).append("HIGH: unresolved verification concern")
    return mutated


def corrupt_artifact_digest(pack):
    mutated = copy.deepcopy(pack)
    artifacts = mutated.get("artifacts") or []
    if artifacts and isinstance(artifacts[0], dict):
        if "digest" in artifacts[0]:
            artifacts[0]["digest"] = "0" * 64
        else:
            artifacts[0]["sha256"] = "0" * 64
    return mutated


def remove_risky_approval(pack):
    mutated = copy.deepcopy(pack)
    for approval in mutated.get("approvals") or []:
        if isinstance(approval, dict):
            approval["status"] = "pending"
    return mutated


risky_exit = int((log_dir / "risky_pr_golden.log.exit-code").read_text(encoding="utf-8").strip())
risky_summary = load_json(risky_summary_path) if risky_summary_path.is_file() else {}
source_pack_path = Path(str(risky_summary.get("evidence_pack", ""))).resolve()
source_pack = load_json(source_pack_path) if source_pack_path.is_file() else {}

case_specs = [
    ("missing_test_evidence", remove_test_evidence(source_pack), "rejected"),
    ("unresolved_high_concern", add_unresolved_high_concern(source_pack), "rejected"),
    ("invalid_artifact_digest", corrupt_artifact_digest(source_pack), "rejected"),
    ("unapproved_risky_action", remove_risky_approval(source_pack), "rejected"),
    ("accepted_after_correction", copy.deepcopy(source_pack), "accepted"),
]

cases = []
for case_id, fixture, expected in case_specs:
    fixture_path = case_dir / f"{case_id}.json"
    fixture_path.write_text(json.dumps(fixture, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    result = evaluate(fixture, source_pack_path)
    case_status = "passed" if result["actual"] == expected else "failed"
    cases.append(
        {
            "id": case_id,
            "expected": expected,
            "actual": result["actual"],
            "status": case_status,
            "fixture": str(fixture_path),
            "checks": result["checks"],
            "reasons": result["reasons"],
            "artifact_digest_failures": result["artifact_digest_failures"],
        }
    )

preconditions = {
    "risky_pr_golden_exit_zero": risky_exit == 0,
    "risky_pr_golden_schema": risky_summary.get("schema_version") == "ao2.risky-pr-golden-path.v1",
    "source_evidence_pack_present": bool(source_pack),
    "source_evidence_pack_schema": source_pack.get("schema_version") == "ao2.evidence-pack.v1",
    "source_evidence_pack_accepted": source_pack.get("verdict") == "accepted",
    "case_id_coverage": sorted(CASE_IDS) == sorted(case["id"] for case in cases),
}
status = "passed" if all(preconditions.values()) and all(case["status"] == "passed" for case in cases) else "failed"
payload = {
    "schema_version": "ao2.evaluator-closure-corpus.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "risky_pr_golden_summary": str(risky_summary_path),
    "source_evidence_pack": str(source_pack_path),
    "preconditions": preconditions,
    "cases": cases,
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
