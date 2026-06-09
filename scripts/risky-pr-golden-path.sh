#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RISKY_PR_GOLDEN_ROOT:-$ROOT/target/risky-pr-golden-path/$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_ID="${AO2_RISKY_PR_GOLDEN_RUN_ID:-risky-pr-golden-path}"
AO2_BIN_EXPLICIT="${AO2_BIN:-}"
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

if [ -z "$AO2_BIN_EXPLICIT" ]; then
  echo "=== build ao2 ==="
  cargo build --release -p ao2-cli
elif [ ! -x "$AO2_BIN" ]; then
  echo "explicit AO2_BIN is not executable: $AO2_BIN" >&2
  exit 1
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
REPORT_INDEX="$OUT_ROOT/cockpit/index.report.json"
COCKPIT_INDEX="$OUT_ROOT/cockpit/runs.html"
RELEASE_SUPPORT_INPUTS="$OUT_ROOT/release-support-inputs"
RELEASE_SUPPORT_BUNDLE_DIR="$OUT_ROOT/release-support-bundle"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" report "$RUN_ID" --target "$TARGET" --out "$REPORT" > "$OUT_ROOT/report.txt"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" report verify --target "$TARGET" --run-id "$RUN_ID" --report "$REPORT" --index "$REPORT_INDEX" > "$OUT_ROOT/report-verify.json"
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" cockpit index --target "$TARGET" --out "$COCKPIT_INDEX" > "$OUT_ROOT/cockpit-index.txt"

EVIDENCE_PACK="$TARGET/.ao2/runs/$RUN_ID/evidence-pack/evidence-pack.json"
SUMMARY="$OUT_ROOT/summary.json"
ARTIFACT_MANIFEST="$OUT_ROOT/artifact-manifest.json"
require_file "$EVIDENCE_PACK"
require_file "$REPORT"
require_file "$REPORT_INDEX"
require_file "$OUT_ROOT/report-verify.json"
require_file "$COCKPIT_INDEX"

mkdir -p "$RELEASE_SUPPORT_INPUTS"
python3 - "$RELEASE_SUPPORT_INPUTS" "$RUN_ID" "$TARGET" "$EVIDENCE_PACK" "$REPORT" "$REPORT_INDEX" "$OUT_ROOT/replay.json" <<'PY'
import json
import sys
from pathlib import Path

inputs_dir, run_id, target, evidence_pack, report, report_index, replay = sys.argv[1:]
inputs = Path(inputs_dir)

def write(name, payload):
    (inputs / name).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

trust_boundary = {
    "control_plane_role": "read_only_observer",
    "control_plane_approves_release": False,
    "release_acceptance_owner": "factory-v3 evaluator-closer",
}
candidate_correlation = {
    "status": "matched",
    "blockers": [],
    "release_version": "0.4.80",
    "three_os_version": "0.4.80",
    "evaluator_decision": "accepted",
    "codex_acceptance": "accepted",
    "claude_acceptance": "accepted",
}
source = {
    "producer": "risky-pr-golden-path",
    "run_id": run_id,
    "target": target,
    "evidence_pack": evidence_pack,
    "report": report,
    "report_index": report_index,
    "replay": replay,
}

write("release-assembly.json", {
    "schema_version": "ao2.cp-release-assembly.v1",
    "status": "assembled",
    "candidate_correlation": "matched",
    "candidate_correlation_detail": candidate_correlation,
    "control_plane_approves_release": False,
    "source": source,
})
write("readiness.json", {
    "schema_version": "ao2.cp-release-readiness.v1",
    "status": "ready",
    "candidate_correlation": candidate_correlation,
    "operator_decision": {
        "control_plane_approves_release": False,
        "factory_v3_evaluator_closer_required": True,
        "release_acceptance_owner": "factory-v3 evaluator-closer",
    },
    "source": source,
})
write("handoff.json", {
    "schema_version": "factory-v3/ao2-release-handoff-checklist/v1",
    "status": "ready_for_evaluator_closer",
    "candidate_correlation": candidate_correlation,
    "trust_boundary": trust_boundary,
    "source": source,
})
write("cockpit.json", {
    "schema_version": "ao2.cp-release-cockpit.v1",
    "status": "ready",
    "candidate_correlation": candidate_correlation,
    "source": source,
})
write("evaluator-decision.json", {
    "schema_version": "factory-v3/ao2-release-evaluator-decision/v1",
    "status": "accepted",
    "decision": "accept_phase1_release_candidate",
    "trust_boundary": trust_boundary,
    "source": source,
})
write("storage-support.json", {
    "schema_version": "ao2.cp-storage-support.v1",
    "status": "ready",
    "source": source,
})
write("operator-evidence.json", {
    "factory_v3_evaluator_closer_required": True,
    "release_acceptance_owner": "factory-v3 evaluator-closer",
    "control_plane_role": "read_only_observer",
    "control_plane_approves_release": False,
    "source": source,
})
write("install-verification.json", {
    "schema_version": "ao2.install-verification-evidence.v1",
    "status": "verified",
    "offline_verification": {
        "status": "verified",
    },
    "provider_api_keys_required": False,
    "control_plane_approves_release": False,
    "mutates_ao_artifacts": False,
    "source": source,
})
PY

echo "=== build release support bundle ==="
env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  "$AO2_BIN" release support-bundle-build \
  --release-assembly "$RELEASE_SUPPORT_INPUTS/release-assembly.json" \
  --readiness "$RELEASE_SUPPORT_INPUTS/readiness.json" \
  --handoff "$RELEASE_SUPPORT_INPUTS/handoff.json" \
  --cockpit "$RELEASE_SUPPORT_INPUTS/cockpit.json" \
  --evaluator-decision "$RELEASE_SUPPORT_INPUTS/evaluator-decision.json" \
  --storage-support "$RELEASE_SUPPORT_INPUTS/storage-support.json" \
  --replay "$OUT_ROOT/replay.json" \
  --report-target "$TARGET" \
  --report-run-id "$RUN_ID" \
  --report "$REPORT" \
  --report-index "$REPORT_INDEX" \
  --install-verification "$RELEASE_SUPPORT_INPUTS/install-verification.json" \
  --operator-evidence "$RELEASE_SUPPORT_INPUTS/operator-evidence.json" \
  --out-dir "$RELEASE_SUPPORT_BUNDLE_DIR" \
  --json > "$OUT_ROOT/release-support-bundle-build.json"

RELEASE_SUPPORT_BUNDLE="$RELEASE_SUPPORT_BUNDLE_DIR/release-support-bundle.json"
RELEASE_SUPPORT_CHECKSUMS="$RELEASE_SUPPORT_BUNDLE_DIR/SHA256SUMS"
require_file "$OUT_ROOT/release-support-bundle-build.json"
require_file "$RELEASE_SUPPORT_BUNDLE"
require_file "$RELEASE_SUPPORT_CHECKSUMS"

python3 - "$SUMMARY" "$ARTIFACT_MANIFEST" "$RUN_ID" "$TARGET" "$EVIDENCE_PACK" "$REPORT" "$REPORT_INDEX" "$COCKPIT_INDEX" "$OUT_ROOT/replay.json" "$OUT_ROOT/report-verify.json" "$OUT_ROOT/release-support-bundle-build.json" "$RELEASE_SUPPORT_BUNDLE" "$RELEASE_SUPPORT_CHECKSUMS" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

summary_path, artifact_manifest_path, run_id, target, evidence_path, report_path, report_index_path, cockpit_index_path, replay_path, report_verify_path, release_support_build_path, release_support_bundle_path, release_support_checksums_path = sys.argv[1:]

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

def sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def json_schema_version(path):
    try:
        payload = load_json(path)
    except Exception:
        return None
    return payload.get("schema_version") or payload.get("schema")

pack = load_json(evidence_path)
replay = load_json(replay_path)
report_index = load_json(report_index_path)
report_verify = load_json(report_verify_path)
release_support_build = load_json(release_support_build_path)
release_support_bundle = load_json(release_support_bundle_path)
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
if report_index.get("schema_version") != "ao2.risky-pr-static-report-index.v1":
    fail("report index schema changed")
if report_index.get("run_id") != run_id:
    fail("report index run_id mismatch")
if report_index.get("status") != "accepted":
    fail("report index status is not accepted")
if report_index.get("closure_verdict") != "accepted":
    fail("report index closure verdict is not accepted")
if (report_index.get("replay") or {}).get("status") != "accepted":
    fail("report index replay status is not accepted")
if report_verify.get("schema_version") != "ao2.report-contract-verification.v1":
    fail("report verify schema changed")
if report_verify.get("contract_schema_version") != "ao2.report-contract.v1":
    fail("report verify contract schema changed")
if report_verify.get("status") != "passed":
    fail("report verify did not pass")
if release_support_build.get("schema_version") != "ao2.release-support-bundle-build.v1":
    fail("release support bundle build schema changed")
if release_support_build.get("status") != "built":
    fail("release support bundle build did not complete")
if release_support_build.get("report_contract_verification_source") != "generated_report_verify":
    fail("release support bundle did not generate report contract verification")
if (release_support_build.get("verification") or {}).get("status") != "passed":
    fail("release support bundle verification did not pass")
if release_support_bundle.get("schema_version") != "ao2.cp-release-support-bundle.v1":
    fail("release support bundle schema changed")
if (release_support_bundle.get("report_contract_verification") or {}).get("status") != "passed":
    fail("release support bundle report contract verification did not pass")
if (release_support_bundle.get("install_verification") or {}).get("schema_version") != "ao2.install-verification-evidence.v1":
    fail("release support bundle missing install verification evidence")
if (release_support_bundle.get("install_verification") or {}).get("status") != "verified":
    fail("release support bundle install verification did not pass")
approval_boundary = report_index.get("approval_boundary") or {}
denied_request_digests = approval_boundary.get("denied_request_digests") or []
approved_action_digests = approval_boundary.get("approved_action_digests") or []
if not denied_request_digests:
    fail("report index missing denied request digest visibility")
if not approved_action_digests:
    fail("report index missing approved action digest visibility")
if denied_request_digests[0] == approved_action_digests[0]:
    fail("report index denied and approved digests must differ")
for key in [
    "objective",
    "denied_actions",
    "approved_actions",
    "test_evidence",
    "closure_verdict",
    "export_path",
    "replay_status",
    "report_contract",
]:
    if (report_index.get("operator_answers") or {}).get(key) is not True:
        fail(f"report index operator answer missing: {key}")

report_contract = report_index.get("report_contract") or {}
required_report_sections = report_contract.get("required_sections") or []
present_report_sections = set(report_contract.get("present_sections") or [])
missing_report_sections = report_contract.get("missing_sections") or []
report_contract_complete = report_contract.get("complete") is True
if not required_report_sections:
    fail("report index missing required_report_sections")
if missing_report_sections:
    fail(f"report index missing required section: {missing_report_sections[0]}")
for section in required_report_sections:
    if section not in present_report_sections:
        fail(f"report index missing required section: {section}")
if not report_contract_complete:
    fail("report index report_contract_complete is false")

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
for heading in ["Request Digest", "Action Digest"]:
    if heading not in report_html:
        fail(f"report missing digest column: {heading}")
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
    "report_index": report_index_path,
    "report_verify": report_verify_path,
    "release_support_bundle_build": release_support_build_path,
    "release_support_bundle": release_support_bundle_path,
    "release_support_checksums": release_support_checksums_path,
    "artifact_manifest": artifact_manifest_path,
    "release_support_bundle_sha256": release_support_build.get("bundle_sha256"),
    "release_support_bundle_verification_status": (release_support_build.get("verification") or {}).get("status"),
    "required_report_sections": required_report_sections,
    "present_report_sections": sorted(present_report_sections),
    "report_contract_complete": report_contract_complete,
    "cockpit_index": cockpit_index_path,
    **checks,
}
Path(summary_path).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

artifact_root = Path(summary_path).parent
artifact_inputs = [
    ("summary.json", summary_path),
    ("report-verify.json", report_verify_path),
    ("release-support-bundle-build.json", release_support_build_path),
    ("release-support-bundle/release-support-bundle.json", release_support_bundle_path),
    ("release-support-bundle/SHA256SUMS", release_support_checksums_path),
    ("cockpit/index.report.json", report_index_path),
]
artifacts = []
for relative_path, path in artifact_inputs:
    artifact_path = Path(path)
    if not artifact_path.is_file():
        fail(f"artifact manifest missing file: {relative_path}")
    try:
        observed_relative_path = artifact_path.relative_to(artifact_root).as_posix()
    except ValueError:
        observed_relative_path = relative_path
    if observed_relative_path != relative_path:
        fail(f"artifact manifest relative path mismatch: {relative_path} != {observed_relative_path}")
    artifacts.append({
        "relative_path": relative_path,
        "path": relative_path,
        "size_bytes": artifact_path.stat().st_size,
        "sha256": sha256_file(artifact_path),
        "schema_version": json_schema_version(artifact_path),
    })

manifest = {
    "schema_version": "ao2.risky-pr-golden-artifact-manifest.v1",
    "status": "indexed",
    "run_id": run_id,
    "artifact_root": ".",
    "artifact_count": len(artifacts),
    "artifacts": artifacts,
}
Path(artifact_manifest_path).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

echo "artifact_manifest=$ARTIFACT_MANIFEST"
echo "summary=$SUMMARY"
echo "status=passed"
