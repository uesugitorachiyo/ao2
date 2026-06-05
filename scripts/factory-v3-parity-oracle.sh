#!/bin/sh
# factory-v3-parity-oracle.sh
#
# Runs every AO2-native producer that has a factory-v3 Python counterpart
# and confirms byte-equal parity. Emits a parity-report.json artifact that
# the Phase 1 promotion dashboard can ingest as "factory_v3 parity oracle
# evidence". On any divergence, exits non-zero so CI / release-gate can
# block.
#
# Trust boundary preserved: AO2 owns the canonical output; factory-v3 is
# the read-only oracle. This script does not mutate factory-v3 artifacts.
#
# Usage:
#   bash scripts/factory-v3-parity-oracle.sh [--factory-root <path>] \
#                                             [--ao2-bin <path>] \
#                                             [--out <dir>]
set -eu

AO2_ROOT="${AO2_ROOT:-$PWD}"
FACTORY_V3_ROOT="${FACTORY_V3_ROOT:-$AO2_ROOT/../factory-v3}"
SNAPSHOT_DIR="${AO2_PARITY_ORACLE_SNAPSHOT_DIR:-$AO2_ROOT/scripts/parity-oracle-snapshots/factory-v3-20260604}"
default_ao2_bin() {
  if [ -x "$AO2_ROOT/target/release/ao2" ]; then
    printf "%s" "$AO2_ROOT/target/release/ao2"
  elif [ -x "$AO2_ROOT/target/release/ao2.exe" ]; then
    printf "%s" "$AO2_ROOT/target/release/ao2.exe"
  else
    printf "%s" "$AO2_ROOT/target/release/ao2"
  fi
}

AO2_BIN="${AO2_BIN:-$(default_ao2_bin)}"
PARITY_OUT="${PARITY_OUT:-$AO2_ROOT/target/factory-v3-parity-oracle/$(date -u +%Y%m%dT%H%M%SZ)}"

while [ $# -gt 0 ]; do
  case "$1" in
    --factory-root) FACTORY_V3_ROOT="$2"; shift 2 ;;
    --ao2-bin) AO2_BIN="$2"; shift 2 ;;
    --out) PARITY_OUT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 64 ;;
  esac
done

if [ ! -x "$AO2_BIN" ]; then
  echo "ao2 binary not found at $AO2_BIN; build with 'cargo build --release -p ao2-cli'" >&2
  exit 1
fi
ORACLE_MODE="live"
if [ ! -d "$FACTORY_V3_ROOT" ]; then
  ORACLE_MODE="snapshot"
  if [ ! -d "$SNAPSHOT_DIR" ]; then
    echo "factory-v3 root not found at $FACTORY_V3_ROOT and snapshot not found at $SNAPSHOT_DIR" >&2
    exit 1
  fi
  FACTORY_V3_ROOT="$SNAPSHOT_DIR"
fi

python_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf "%s" "$1"
  fi
}

mkdir -p "$PARITY_OUT"
PARITY_OUT=$(CDPATH= cd -- "$PARITY_OUT" && pwd)
PARITY_OUT_PY=$(python_path "$PARITY_OUT")

snapshot_file() {
  path="$SNAPSHOT_DIR/$1"
  if [ ! -f "$path" ]; then
    echo "factory-v3 parity snapshot missing: $path" >&2
    exit 1
  fi
  printf "%s" "$path"
}

sha256_file() {
  python3 - "$1" <<'PY'
import hashlib
import sys

with open(sys.argv[1], "rb") as fh:
    print(hashlib.sha256(fh.read()).hexdigest())
PY
}

REPORT="$PARITY_OUT/parity-report.json"
AO2_TABLE="$PARITY_OUT/ao2-bridge-mapping.json"
F3_TABLE="$PARITY_OUT/factory-v3-bridge-mapping.json"
AO2_TABLE_CANON="$PARITY_OUT/ao2-bridge-mapping.canonical.json"
F3_TABLE_CANON="$PARITY_OUT/factory-v3-bridge-mapping.canonical.json"
AO2_TABLE_PY=$(python_path "$AO2_TABLE")
F3_TABLE_PY=$(python_path "$F3_TABLE")
AO2_TABLE_CANON_PY=$(python_path "$AO2_TABLE_CANON")
F3_TABLE_CANON_PY=$(python_path "$F3_TABLE_CANON")

# ------------------------------------------------------------------
# Check 1: factory-v3/ao-operator-ao2-provider-contract/v1 mapping
# ------------------------------------------------------------------

"$AO2_BIN" factory bridge-mapping > "$AO2_TABLE"
AO2_DIGEST=$("$AO2_BIN" factory bridge-mapping --digest | tr -d '[:space:]')

if [ "$ORACLE_MODE" = "live" ]; then
  (
    cd "$FACTORY_V3_ROOT"
    PYTHONPATH=scripts python3 -c "
import json, hashlib, sys
sys.path.insert(0, 'scripts')
from ao_operator_ao2_provider_contract import mapping_table, mapping_digest
print(json.dumps(mapping_table(), sort_keys=True, separators=(',', ':')))
" > "$F3_TABLE"
  )

  F3_DIGEST=$(
    cd "$FACTORY_V3_ROOT" && PYTHONPATH=scripts python3 -c "
import sys; sys.path.insert(0, 'scripts')
from ao_operator_ao2_provider_contract import mapping_digest
print(mapping_digest())
" | tr -d '[:space:]'
  )
else
  cp "$(snapshot_file bridge-mapping.canonical.json)" "$F3_TABLE"
  F3_DIGEST=$(sha256_file "$F3_TABLE")
fi

# Canonicalise both sides through Python's json.dumps(sort_keys=True,
# separators=(',', ':')) so whitespace differences cannot create false
# divergence. The digest is content-addressable over the same canonical
# form, so canonical-equal IS digest-equal — we record both for audit.
python3 - "$AO2_TABLE_PY" "$F3_TABLE_PY" "$AO2_TABLE_CANON_PY" "$F3_TABLE_CANON_PY" <<'PY'
import json, sys
ao2_in, f3_in, ao2_out, f3_out = sys.argv[1:]
with open(ao2_in) as f: ao2 = json.load(f)
with open(f3_in) as f: f3 = json.load(f)
open(ao2_out, 'w').write(json.dumps(ao2, sort_keys=True, separators=(',', ':')))
open(f3_out, 'w').write(json.dumps(f3, sort_keys=True, separators=(',', ':')))
PY

BRIDGE_DIGEST_PARITY="false"
BRIDGE_TABLE_PARITY="false"
if [ "$AO2_DIGEST" = "$F3_DIGEST" ]; then
  BRIDGE_DIGEST_PARITY="true"
fi
if cmp -s "$AO2_TABLE_CANON" "$F3_TABLE_CANON"; then
  BRIDGE_TABLE_PARITY="true"
fi

BRIDGE_VERDICT="FAIL"
if [ "$BRIDGE_DIGEST_PARITY" = "true" ] && [ "$BRIDGE_TABLE_PARITY" = "true" ]; then
  BRIDGE_VERDICT="PASS"
fi

# ------------------------------------------------------------------
# Check 2: per-task canonical-role resolution over a fixture runspec
# ------------------------------------------------------------------

RUNSPEC_FIXTURE="$AO2_ROOT/scripts/parity-oracle-fixtures/ao-dev-v1-runspec.json"
AO2_BRIDGE_OUT="$PARITY_OUT/ao2-bridge-evidence.json"
AO2_ROLES_JSON="$PARITY_OUT/ao2-resolved-roles.json"
F3_ROLES_JSON="$PARITY_OUT/factory-v3-resolved-roles.json"
RUNSPEC_FIXTURE_PY=$(python_path "$RUNSPEC_FIXTURE")
AO2_BRIDGE_OUT_PY=$(python_path "$AO2_BRIDGE_OUT")
AO2_ROLES_JSON_PY=$(python_path "$AO2_ROLES_JSON")
F3_ROLES_JSON_PY=$(python_path "$F3_ROLES_JSON")

if [ ! -f "$RUNSPEC_FIXTURE" ]; then
  echo "runspec fixture missing: $RUNSPEC_FIXTURE" >&2
  exit 1
fi

"$AO2_BIN" factory bridge \
  --runspec "$RUNSPEC_FIXTURE" \
  --json \
  --now-ms 1716772800000 > "$AO2_BRIDGE_OUT"

python3 - "$AO2_BRIDGE_OUT_PY" "$AO2_ROLES_JSON_PY" <<'PY'
import json
import sys
bridge_in, roles_out = sys.argv[1:]
with open(bridge_in) as f: doc = json.load(f)
out = []
for t in doc['governed_run_plan']['tasks']:
    out.append({
        'role_id': t['role_id'],
        'canonical_role': t['canonical_role'],
        'ao2_provider_contract_slug': t['provider_contract'],
        'evidence_obligation': t['evidence_obligation'],
        'sandbox': t['sandbox'],
        'closure_owner': t['closure_owner'],
    })
open(roles_out, 'w').write(json.dumps(out, sort_keys=True, separators=(',', ':')))
PY

if [ "$ORACLE_MODE" = "live" ]; then
  (
    cd "$FACTORY_V3_ROOT"
    PYTHONPATH=scripts python3 - "$RUNSPEC_FIXTURE_PY" <<'PY'
import json, sys
sys.path.insert(0, 'scripts')
from ao_operator_ao2_provider_contract import resolve_runspec
with open(sys.argv[1]) as f: rs = json.load(f)
out = []
for r in resolve_runspec(rs):
    out.append({
        'role_id': r['role_id'],
        'canonical_role': r['canonical_role'],
        'ao2_provider_contract_slug': r['ao2_provider_contract_slug'],
        'evidence_obligation': r['evidence_obligation'],
        'sandbox': r['sandbox'],
        'closure_owner': r['closure_owner'],
    })
print(json.dumps(out, sort_keys=True, separators=(',', ':')))
PY
  ) > "$F3_ROLES_JSON"
else
  cp "$(snapshot_file resolved-roles.canonical.json)" "$F3_ROLES_JSON"
fi

# Canonicalize both sides through the same Python JSON dumper so trailing
# newlines or write-side whitespace differences cannot create false divergence.
RUNSPEC_PARITY=$(python3 - "$AO2_ROLES_JSON_PY" "$F3_ROLES_JSON_PY" <<'PY'
import json
import sys
a = json.load(open(sys.argv[1]))
b = json.load(open(sys.argv[2]))
print('true' if json.dumps(a, sort_keys=True) == json.dumps(b, sort_keys=True) else 'false')
PY
)
RUNSPEC_VERDICT="FAIL"
[ "$RUNSPEC_PARITY" = "true" ] && RUNSPEC_VERDICT="PASS"

# ------------------------------------------------------------------
# Check 3: factory-v3/ao2-release-evaluator-decision/v1 byte-equal parity
# ------------------------------------------------------------------
#
# Phase 2 W2 P0. AO2 is the canonical producer of
# factory-v3/ao2-release-evaluator-decision/v1; factory-v3 Python remains
# in a read-only audit role. Both producers must emit byte-equal JSON
# (under canonical sort+separators) for the same logical input.

EVAL_READINESS_FIXTURE="$AO2_ROOT/scripts/parity-oracle-fixtures/release-evaluator-decision-readiness.json"
EVAL_HANDOFF_FIXTURE="$AO2_ROOT/scripts/parity-oracle-fixtures/release-evaluator-decision-handoff.json"
EVAL_SUPPORT_FIXTURE="$AO2_ROOT/scripts/parity-oracle-fixtures/release-evaluator-decision-support-bundle.json"
EVAL_AO2_OUT="$PARITY_OUT/ao2-release-evaluator-decision.json"
EVAL_F3_OUT="$PARITY_OUT/factory-v3-release-evaluator-decision.json"
EVAL_AO2_CANON="$PARITY_OUT/ao2-release-evaluator-decision.canonical.json"
EVAL_F3_CANON="$PARITY_OUT/factory-v3-release-evaluator-decision.canonical.json"
EVAL_F3_SCRIPT="$FACTORY_V3_ROOT/scripts/ao2_release_evaluator_decision.py"

EVAL_DECISION_PARITY="false"
EVAL_DECISION_VERDICT="FAIL"

if [ -f "$EVAL_READINESS_FIXTURE" ] && [ -f "$EVAL_HANDOFF_FIXTURE" ] \
   && [ -f "$EVAL_SUPPORT_FIXTURE" ] \
   && { [ "$ORACLE_MODE" = "snapshot" ] || [ -f "$EVAL_F3_SCRIPT" ]; }; then

  "$AO2_BIN" release evaluator-decision-build \
    --readiness "$EVAL_READINESS_FIXTURE" \
    --handoff-checklist "$EVAL_HANDOFF_FIXTURE" \
    --support-bundle-status "$EVAL_SUPPORT_FIXTURE" \
    --write-json "$EVAL_AO2_OUT" > /dev/null

  if [ "$ORACLE_MODE" = "live" ]; then
    python3 "$EVAL_F3_SCRIPT" \
      --readiness "$EVAL_READINESS_FIXTURE" \
      --handoff-checklist "$EVAL_HANDOFF_FIXTURE" \
      --support-bundle-status "$EVAL_SUPPORT_FIXTURE" \
      --write-json "$EVAL_F3_OUT" > /dev/null
  else
    cp "$(snapshot_file release-evaluator-decision.canonical.json)" "$EVAL_F3_OUT"
  fi

  # Canonicalize both payloads. Strip the `evidence` block which carries
  # absolute fixture paths — those will be identical here because both
  # producers receive the same fixtures, but the parity assertion is on
  # the *decision logic* not the input paths.
  python3 - "$EVAL_AO2_OUT" "$EVAL_F3_OUT" "$EVAL_AO2_CANON" "$EVAL_F3_CANON" <<'CANON'
import json, sys
ao2_in, f3_in, ao2_out, f3_out = sys.argv[1:]
with open(ao2_in) as f: ao2 = json.load(f)
with open(f3_in) as f: f3 = json.load(f)
for doc in (ao2, f3):
    doc.pop('evidence', None)
open(ao2_out, 'w').write(json.dumps(ao2, sort_keys=True, separators=(',', ':')))
open(f3_out, 'w').write(json.dumps(f3, sort_keys=True, separators=(',', ':')))
CANON

  if [ "$ORACLE_MODE" = "snapshot" ]; then
    cp "$(snapshot_file release-evaluator-decision.canonical.json)" "$EVAL_F3_CANON"
  fi

  if cmp -s "$EVAL_AO2_CANON" "$EVAL_F3_CANON"; then
    EVAL_DECISION_PARITY="true"
    EVAL_DECISION_VERDICT="PASS"
  fi
else
  echo "evaluator-decision parity check skipped: missing fixtures or factory-v3 script" >&2
fi

# ------------------------------------------------------------------
# Check 4: factory-v3/ao2-release-handoff-checklist/v1 byte-equal parity
# ------------------------------------------------------------------
#
# Phase 2 W2 P1. AO2 is the canonical producer; factory-v3 Python is the
# read-only audit oracle.

HANDOFF_FIXTURE="$AO2_ROOT/scripts/parity-oracle-fixtures/release-handoff-checklist-input.json"
HANDOFF_AO2_OUT="$PARITY_OUT/ao2-release-handoff-checklist.json"
HANDOFF_F3_OUT="$PARITY_OUT/factory-v3-release-handoff-checklist.json"
HANDOFF_AO2_CANON="$PARITY_OUT/ao2-release-handoff-checklist.canonical.json"
HANDOFF_F3_CANON="$PARITY_OUT/factory-v3-release-handoff-checklist.canonical.json"
HANDOFF_F3_SCRIPT="$FACTORY_V3_ROOT/scripts/ao2_release_handoff_checklist.py"

HANDOFF_DECISION_PARITY="false"
HANDOFF_DECISION_VERDICT="FAIL"

if [ -f "$HANDOFF_FIXTURE" ] \
   && { [ "$ORACLE_MODE" = "snapshot" ] || [ -f "$HANDOFF_F3_SCRIPT" ]; }; then
  "$AO2_BIN" release handoff-checklist-build \
    --handoff "$HANDOFF_FIXTURE" \
    --write-json "$HANDOFF_AO2_OUT" > /dev/null

  if [ "$ORACLE_MODE" = "live" ]; then
    python3 "$HANDOFF_F3_SCRIPT" \
      --handoff "$HANDOFF_FIXTURE" \
      --write-json "$HANDOFF_F3_OUT" > /dev/null
  else
    cp "$(snapshot_file release-handoff-checklist.canonical.json)" "$HANDOFF_F3_OUT"
  fi

  python3 - "$HANDOFF_AO2_OUT" "$HANDOFF_F3_OUT" \
    "$HANDOFF_AO2_CANON" "$HANDOFF_F3_CANON" <<'CANON'
import json, sys
ao2_in, f3_in, ao2_out, f3_out = sys.argv[1:]
with open(ao2_in) as f: ao2 = json.load(f)
with open(f3_in) as f: f3 = json.load(f)
open(ao2_out, 'w').write(json.dumps(ao2, sort_keys=True, separators=(',', ':')))
open(f3_out, 'w').write(json.dumps(f3, sort_keys=True, separators=(',', ':')))
CANON

  if [ "$ORACLE_MODE" = "snapshot" ]; then
    cp "$(snapshot_file release-handoff-checklist.canonical.json)" "$HANDOFF_F3_CANON"
  fi

  if cmp -s "$HANDOFF_AO2_CANON" "$HANDOFF_F3_CANON"; then
    HANDOFF_DECISION_PARITY="true"
    HANDOFF_DECISION_VERDICT="PASS"
  fi
else
  echo "handoff-checklist parity check skipped: missing fixtures or factory-v3 script" >&2
fi

# ------------------------------------------------------------------
# Check 5: factory-v3/ao2-watchdog-no-active-ao2-runs-attestation/v1
# ------------------------------------------------------------------
#
# Phase 2 W2 P4. AO2 is the canonical producer; factory-v3 Python is the
# read-only parity oracle. `produced_at_ms` is excluded from the canonical
# comparison because the factory-v3 producer stamps wall-clock time while AO2
# supports fixed timestamps for reproducible release-gate evidence.

WATCHDOG_QUEUE_FIXTURE="$PARITY_OUT/ao2-watchdog-queue-list-fixture.json"
WATCHDOG_AO2_OUT="$PARITY_OUT/ao2-watchdog-no-active-runs-attestation.json"
WATCHDOG_F3_OUT="$PARITY_OUT/factory-v3-watchdog-no-active-runs-attestation.json"
WATCHDOG_AO2_CANON="$PARITY_OUT/ao2-watchdog-no-active-runs-attestation.canonical.json"
WATCHDOG_F3_CANON="$PARITY_OUT/factory-v3-watchdog-no-active-runs-attestation.canonical.json"
WATCHDOG_F3_SCRIPT="$FACTORY_V3_ROOT/scripts/ao2_watchdog_cancel_authority_producer.py"

WATCHDOG_ATTESTATION_PARITY="false"
WATCHDOG_ATTESTATION_VERDICT="FAIL"

cat > "$WATCHDOG_QUEUE_FIXTURE" <<'JSON'
{
  "schema_version": "ao2.factory-v3-compat-workbench-queue-list.v1",
  "owner": "ao2-workbench-queue",
  "factory_v3_role": "parity_oracle_only",
  "control_plane_role": "read_only_observer_after_signed_evidence",
  "queue_path": "/tmp/ao2-workbench-queue.json",
  "entry_count": 3,
  "continuity_contract": null,
  "entries": [
    {"run_id": "r-complete", "status": "completed"},
    {"run_id": "r-cancelled", "status": "cancelled"},
    {"run_id": "r-unknown", "status": ""}
  ]
}
JSON

if [ "$ORACLE_MODE" = "snapshot" ] || [ -f "$WATCHDOG_F3_SCRIPT" ]; then
  "$AO2_BIN" factory cancel-authority \
    --queue-list-json "$WATCHDOG_QUEUE_FIXTURE" \
    --produced-at-ms 1716772800000 \
    --out "$WATCHDOG_AO2_OUT" \
    --json > /dev/null

  if [ "$ORACLE_MODE" = "live" ]; then
    python3 "$WATCHDOG_F3_SCRIPT" \
      --queue-list-json "$WATCHDOG_QUEUE_FIXTURE" \
      --out "$WATCHDOG_F3_OUT" > /dev/null
  else
    cp "$(snapshot_file watchdog-no-active-runs-attestation.canonical.json)" "$WATCHDOG_F3_OUT"
  fi

  python3 - "$WATCHDOG_AO2_OUT" "$WATCHDOG_F3_OUT" \
    "$WATCHDOG_AO2_CANON" "$WATCHDOG_F3_CANON" <<'CANON'
import json, sys
ao2_in, f3_in, ao2_out, f3_out = sys.argv[1:]
with open(ao2_in) as f: ao2 = json.load(f)
with open(f3_in) as f: f3 = json.load(f)
for doc in (ao2, f3):
    doc.pop('produced_at_ms', None)
open(ao2_out, 'w').write(json.dumps(ao2, sort_keys=True, separators=(',', ':')))
open(f3_out, 'w').write(json.dumps(f3, sort_keys=True, separators=(',', ':')))
CANON

  if [ "$ORACLE_MODE" = "snapshot" ]; then
    cp "$(snapshot_file watchdog-no-active-runs-attestation.canonical.json)" "$WATCHDOG_F3_CANON"
  fi

  if cmp -s "$WATCHDOG_AO2_CANON" "$WATCHDOG_F3_CANON"; then
    WATCHDOG_ATTESTATION_PARITY="true"
    WATCHDOG_ATTESTATION_VERDICT="PASS"
  fi
else
  echo "watchdog no-active-runs attestation parity check skipped: missing factory-v3 script" >&2
fi

# ------------------------------------------------------------------
# Emit consolidated parity report
# ------------------------------------------------------------------

NUM_CHECKS=5
NUM_PASSED=0
[ "$BRIDGE_VERDICT" = "PASS" ] && NUM_PASSED=$((NUM_PASSED + 1))
[ "$RUNSPEC_VERDICT" = "PASS" ] && NUM_PASSED=$((NUM_PASSED + 1))
[ "$EVAL_DECISION_VERDICT" = "PASS" ] && NUM_PASSED=$((NUM_PASSED + 1))
[ "$HANDOFF_DECISION_VERDICT" = "PASS" ] && NUM_PASSED=$((NUM_PASSED + 1))
[ "$WATCHDOG_ATTESTATION_VERDICT" = "PASS" ] && NUM_PASSED=$((NUM_PASSED + 1))
OVERALL="PASS"
[ "$NUM_PASSED" -ne "$NUM_CHECKS" ] && OVERALL="FAIL"

SNAPSHOT_MANIFEST=""
if [ "$ORACLE_MODE" = "snapshot" ]; then
  SNAPSHOT_MANIFEST="$(snapshot_file manifest.json)"
  FACTORY_V3_SOURCE_HEAD=$(python3 - "$SNAPSHOT_MANIFEST" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as fh:
    print(json.load(fh).get("source_git_head", "unknown"))
PY
)
else
  FACTORY_V3_SOURCE_HEAD=$(git -C "$FACTORY_V3_ROOT" rev-parse HEAD 2>/dev/null || printf "unknown")
fi

python3 - "$AO2_ROOT" "$FACTORY_V3_ROOT" "$AO2_BIN" "$REPORT" \
  "$AO2_DIGEST" "$F3_DIGEST" "$BRIDGE_DIGEST_PARITY" "$BRIDGE_TABLE_PARITY" \
  "$BRIDGE_VERDICT" "$NUM_CHECKS" "$NUM_PASSED" "$OVERALL" \
  "$AO2_TABLE" "$F3_TABLE" "$AO2_TABLE_CANON" "$F3_TABLE_CANON" \
  "$RUNSPEC_FIXTURE" "$AO2_ROLES_JSON" "$F3_ROLES_JSON" \
  "$RUNSPEC_PARITY" "$RUNSPEC_VERDICT" "$AO2_BRIDGE_OUT" \
  "$EVAL_READINESS_FIXTURE" "$EVAL_HANDOFF_FIXTURE" "$EVAL_SUPPORT_FIXTURE" \
  "$EVAL_AO2_OUT" "$EVAL_F3_OUT" "$EVAL_AO2_CANON" "$EVAL_F3_CANON" \
  "$EVAL_DECISION_PARITY" "$EVAL_DECISION_VERDICT" \
  "$HANDOFF_FIXTURE" "$HANDOFF_AO2_OUT" "$HANDOFF_F3_OUT" \
  "$HANDOFF_AO2_CANON" "$HANDOFF_F3_CANON" \
  "$HANDOFF_DECISION_PARITY" "$HANDOFF_DECISION_VERDICT" \
  "$WATCHDOG_QUEUE_FIXTURE" "$WATCHDOG_AO2_OUT" "$WATCHDOG_F3_OUT" \
  "$WATCHDOG_AO2_CANON" "$WATCHDOG_F3_CANON" \
  "$WATCHDOG_ATTESTATION_PARITY" "$WATCHDOG_ATTESTATION_VERDICT" \
  "$ORACLE_MODE" "$SNAPSHOT_MANIFEST" "$FACTORY_V3_SOURCE_HEAD" <<'PY'
import json, sys, subprocess, os, datetime
(ao2_root, factory_root, ao2_bin, report_path,
 ao2_digest, f3_digest, digest_parity, table_parity, bridge_verdict,
 num_checks, num_passed, overall,
 ao2_table, f3_table, ao2_table_canon, f3_table_canon,
 runspec_fixture, ao2_roles_json, f3_roles_json,
 runspec_parity, runspec_verdict, ao2_bridge_out,
 eval_readiness_fixture, eval_handoff_fixture, eval_support_fixture,
 eval_ao2_out, eval_f3_out, eval_ao2_canon, eval_f3_canon,
 eval_decision_parity, eval_decision_verdict,
 handoff_fixture, handoff_ao2_out, handoff_f3_out,
 handoff_ao2_canon, handoff_f3_canon,
 handoff_decision_parity, handoff_decision_verdict,
 watchdog_queue_fixture, watchdog_ao2_out, watchdog_f3_out,
 watchdog_ao2_canon, watchdog_f3_canon,
 watchdog_attestation_parity, watchdog_attestation_verdict,
 oracle_mode, snapshot_manifest, factory_v3_source_head) = sys.argv[1:]

def git_head(repo):
    try:
        return subprocess.check_output(
            ['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip()
    except Exception:
        return 'unknown'

report = {
    'schema_version': 'ao2.factory-v3-parity-oracle/v1',
    'generated_at': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
    'overall_verdict': overall,
    'counts': {
        'total_checks': int(num_checks),
        'passed': int(num_passed),
        'failed': int(num_checks) - int(num_passed),
    },
    'trust_boundary': {
        'role': 'parity_oracle_only',
        'ao2_role': 'canonical_producer',
        'factory_v3_role': 'parity_oracle_only',
        'mutates_ao_artifacts': False,
    },
    'sources': {
        'ao2_root': ao2_root,
        'ao2_bin': ao2_bin,
        'ao2_git_head': git_head(ao2_root),
        'factory_v3_root': factory_root,
        'factory_v3_git_head': factory_v3_source_head,
        'oracle_mode': oracle_mode,
        'snapshot_manifest': snapshot_manifest or None,
    },
    'checks': [
        {
            'name': 'factory_v3_ao_operator_ao2_provider_contract_mapping',
            'schema': 'factory-v3/ao-operator-ao2-provider-contract/v1',
            'ao2_producer': 'ao2 factory bridge-mapping',
            'factory_v3_producer': 'scripts/ao_operator_ao2_provider_contract.py mapping_table()',
            'ao2_digest': ao2_digest,
            'factory_v3_digest': f3_digest,
            'digest_parity': digest_parity == 'true',
            'canonical_table_parity': table_parity == 'true',
            'verdict': bridge_verdict,
            'artifacts': {
                'ao2_table': ao2_table,
                'factory_v3_table': f3_table,
                'ao2_table_canonical': ao2_table_canon,
                'factory_v3_table_canonical': f3_table_canon,
            },
        },
        {
            'name': 'factory_v3_runspec_role_resolution',
            'schema': 'factory-v3/ao-operator-ao2-provider-contract/v1#resolve_runspec',
            'ao2_producer': 'ao2 factory bridge --runspec <fixture> --json',
            'factory_v3_producer': 'scripts/ao_operator_ao2_provider_contract.py resolve_runspec(runspec)',
            'fixture': runspec_fixture,
            'canonical_resolution_parity': runspec_parity == 'true',
            'verdict': runspec_verdict,
            'artifacts': {
                'ao2_bridge_evidence': ao2_bridge_out,
                'ao2_resolved_roles': ao2_roles_json,
                'factory_v3_resolved_roles': f3_roles_json,
            },
        },
        {
            'name': 'factory_v3_release_evaluator_decision_parity',
            'schema': 'factory-v3/ao2-release-evaluator-decision/v1',
            'ao2_producer': 'ao2 release evaluator-decision-build --readiness ... --handoff-checklist ... --support-bundle-status ...',
            'factory_v3_producer': 'scripts/ao2_release_evaluator_decision.py',
            'fixtures': {
                'readiness': eval_readiness_fixture,
                'handoff_checklist': eval_handoff_fixture,
                'support_bundle_status': eval_support_fixture,
            },
            'canonical_decision_parity': eval_decision_parity == 'true',
            'verdict': eval_decision_verdict,
            'artifacts': {
                'ao2_decision': eval_ao2_out,
                'factory_v3_decision': eval_f3_out,
                'ao2_decision_canonical': eval_ao2_canon,
                'factory_v3_decision_canonical': eval_f3_canon,
            },
        },
        {
            'name': 'factory_v3_release_handoff_checklist_parity',
            'schema': 'factory-v3/ao2-release-handoff-checklist/v1',
            'ao2_producer': 'ao2 release handoff-checklist-build --handoff <fixture>',
            'factory_v3_producer': 'scripts/ao2_release_handoff_checklist.py',
            'fixture': handoff_fixture,
            'canonical_checklist_parity': handoff_decision_parity == 'true',
            'verdict': handoff_decision_verdict,
            'artifacts': {
                'ao2_checklist': handoff_ao2_out,
                'factory_v3_checklist': handoff_f3_out,
                'ao2_checklist_canonical': handoff_ao2_canon,
                'factory_v3_checklist_canonical': handoff_f3_canon,
            },
        },
        {
            'name': 'factory_v3_ao2_watchdog_no_active_runs_attestation_parity',
            'schema': 'factory-v3/ao2-watchdog-no-active-ao2-runs-attestation/v1',
            'ao2_producer': 'ao2 factory cancel-authority --queue-list-json <fixture>',
            'factory_v3_producer': 'scripts/ao2_watchdog_cancel_authority_producer.py',
            'fixture': watchdog_queue_fixture,
            'canonical_attestation_parity': watchdog_attestation_parity == 'true',
            'verdict': watchdog_attestation_verdict,
            'comparison_note': 'produced_at_ms is excluded from canonical comparison because factory-v3 stamps wall-clock time.',
            'artifacts': {
                'ao2_attestation': watchdog_ao2_out,
                'factory_v3_attestation': watchdog_f3_out,
                'ao2_attestation_canonical': watchdog_ao2_canon,
                'factory_v3_attestation_canonical': watchdog_f3_canon,
            },
        },
    ],
}
with open(report_path, 'w') as f:
    f.write(json.dumps(report, indent=2, sort_keys=True) + '\n')
PY

printf "factory_v3_parity_oracle_out=%s\n" "$PARITY_OUT"
printf "factory_v3_parity_oracle_report=%s\n" "$REPORT"
printf "factory_v3_parity_oracle_overall=%s\n" "$OVERALL"
printf "factory_v3_parity_oracle_passed=%s/%s\n" "$NUM_PASSED" "$NUM_CHECKS"
[ "$OVERALL" = "PASS" ] || exit 1
