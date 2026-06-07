#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RISKY_PR_GOLDEN_ROOT:-$ROOT/target/risky-pr-golden-path/$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_ID="${AO2_RISKY_PR_GOLDEN_RUN_ID:-risky-pr-golden-path}"
AO2_BIN="${AO2_BIN:-$ROOT/target/release/ao2}"

require_file() {
  if [ ! -f "$1" ]; then
    echo "missing required file: $1" >&2
    exit 1
  fi
}

mkdir -p "$OUT_ROOT"

echo "golden_root=$OUT_ROOT"
echo "run_id=$RUN_ID"

if [ ! -x "$AO2_BIN" ]; then
  echo "=== build ao2 ==="
  cargo build --release -p ao2-cli
fi

FIXTURE_ROOT="$OUT_ROOT/fixture"
TARGET="$FIXTURE_ROOT/discount-service"
mkdir -p "$FIXTURE_ROOT"
cp -R "$ROOT/fixtures/discount-service" "$TARGET"

echo "=== run until exact approval ==="
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" \
  run "$ROOT/examples/risky-pr-run/risky-pr.yaml" \
  --target "$TARGET" \
  --run-id "$RUN_ID" \
  --pause-for-approval > "$OUT_ROOT/run-paused.txt"

TICKET="$(awk -F= '/approval_ticket_id=/{print $2}' "$OUT_ROOT/run-paused.txt" | tail -n1)"
if [ -z "$TICKET" ]; then
  echo "approval ticket was not emitted" >&2
  exit 1
fi

echo "=== grant exact approval ==="
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" \
  approve "$TICKET" \
  --target "$TARGET" \
  --approver human:risky-pr-golden-path > "$OUT_ROOT/approve.txt"

echo "=== resume to accepted closure ==="
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" \
  run --resume "$RUN_ID" \
  --target "$TARGET" > "$OUT_ROOT/resume.txt"

echo "=== replay and render report ==="
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" replay "$RUN_ID" --target "$TARGET" > "$OUT_ROOT/replay.json"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" export "$RUN_ID" --target "$TARGET" > "$OUT_ROOT/export.txt"

REPORT="$OUT_ROOT/cockpit/index.html"
COCKPIT_INDEX="$OUT_ROOT/cockpit/runs.html"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" report "$RUN_ID" --target "$TARGET" --out "$REPORT" > "$OUT_ROOT/report.txt"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" cockpit index --target "$TARGET" --out "$COCKPIT_INDEX" > "$OUT_ROOT/cockpit-index.txt"

EVIDENCE_PACK="$TARGET/.ao2/runs/$RUN_ID/evidence-pack/evidence-pack.json"
SUMMARY="$OUT_ROOT/summary.json"
require_file "$EVIDENCE_PACK"
require_file "$REPORT"
require_file "$COCKPIT_INDEX"

python3 - "$SUMMARY" "$RUN_ID" "$TARGET" "$EVIDENCE_PACK" "$REPORT" "$COCKPIT_INDEX" "$OUT_ROOT/replay.json" <<'PY'
import json
import sys
from pathlib import Path

summary_path, run_id, target, evidence_path, report_path, cockpit_index_path, replay_path = sys.argv[1:]

def load_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))

def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)

def text_contains(value, *needles):
    text = "\n".join(strings(value)).lower()
    return all(needle.lower() in text for needle in needles)

def fail(message):
    raise SystemExit(message)

pack = load_json(evidence_path)
replay = load_json(replay_path)
report_html = Path(report_path).read_text(encoding="utf-8", errors="replace")
cockpit_html = Path(cockpit_index_path).read_text(encoding="utf-8", errors="replace")

if pack.get("schema_version") != "ao2.evidence-pack.v1":
    fail("evidence pack schema changed")
if pack.get("run_id") != run_id:
    fail("evidence pack run_id mismatch")
if pack.get("verdict") != "accepted":
    fail("evidence pack verdict is not accepted")
if replay.get("status") != "accepted":
    fail("replay status is not accepted")
if replay.get("digest_failures") not in ([], None):
    fail("replay has digest failures")

required_markers = {"policy_denied_git_push", "review_missing_tests"}
observed_markers = set()
for value in strings(pack):
    for marker in required_markers:
        if marker in value:
            observed_markers.add(marker)
if observed_markers != required_markers:
    fail(f"missing risky-run markers: {sorted(required_markers - observed_markers)}")

policy_denial_observed = text_contains(pack, "git push", "denied") or text_contains(pack, "policy_denied_git_push")
exact_approval_observed = text_contains(pack, "approval", "granted") or text_contains(pack, "approval", "approved")
evaluator_rejection_observed = text_contains(pack, "rejected", "missing") or text_contains(pack, "review_missing_tests")
evaluator_acceptance_observed = text_contains(pack, "accepted")
acceptance_evidence_observed = text_contains(pack, "test") and text_contains(pack, "evidence")

checks = {
    "policy_denial_observed": policy_denial_observed,
    "exact_approval_observed": exact_approval_observed,
    "evaluator_rejection_observed": evaluator_rejection_observed,
    "evaluator_acceptance_observed": evaluator_acceptance_observed,
    "acceptance_evidence_observed": acceptance_evidence_observed,
}
failed = [name for name, passed in checks.items() if not passed]
if failed:
    fail(f"golden-path evidence checks failed: {failed}")

for heading in [
    "Local Run Record",
    "Static Export Evidence",
    "Policy Decisions",
    "Approvals",
    "Artifacts",
    "Evaluator Closure Evidence",
    "Closure Reports",
    "Replay Evidence",
    "Replay",
    "Run Markers",
]:
    if heading not in report_html:
        fail(f"report missing section: {heading}")
if run_id not in cockpit_html or "evidence" not in cockpit_html.lower():
    fail("cockpit index does not link the golden run evidence")

summary = {
    "schema_version": "ao2.risky-pr-golden-path.v1",
    "status": "passed",
    "run_id": run_id,
    "target": target,
    "replay_status": replay.get("status"),
    "event_count": replay.get("event_count"),
    "artifact_count": replay.get("artifact_count"),
    "digest_failure_count": len(replay.get("digest_failures") or []),
    "evidence_pack": evidence_path,
    "report": report_path,
    "cockpit_index": cockpit_index_path,
    **checks,
}
Path(summary_path).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

echo "summary=$SUMMARY"
echo "status=passed"
