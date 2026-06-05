#!/bin/sh
# verify-replacement-parity.sh
#
# Composite verifier for "AO2 replaces factory-v3" readiness. Runs the
# AO2-native producers AND the factory-v3 parity oracle in sequence, then
# emits a single rollup report.
#
# Exits non-zero if any constituent check fails. This is the single command
# CI / release-gate should call to certify replacement parity.
#
# Trust boundary preserved: all AO2-native producers write into target/.
# factory-v3 is invoked read-only as the parity oracle. No factory-v3
# artifacts are mutated.
#
# Usage:
#   bash scripts/verify-replacement-parity.sh
#
# Output:
#   target/replacement-parity-verification/<ts>/replacement-parity.json
set -eu

AO2_ROOT="${AO2_ROOT:-$PWD}"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-$AO2_ROOT/target/replacement-parity-verification/$TS}"
mkdir -p "$OUT_DIR"
OUT_DIR=$(CDPATH= cd -- "$OUT_DIR" && pwd)

native_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf "%s" "$1"
  fi
}

default_ao2_bin() {
  if [ -x "$AO2_ROOT/target/release/ao2" ]; then
    printf "%s" "$AO2_ROOT/target/release/ao2"
  elif [ -x "$AO2_ROOT/target/release/ao2.exe" ]; then
    printf "%s" "$AO2_ROOT/target/release/ao2.exe"
  else
    printf "%s" "$AO2_ROOT/target/release/ao2"
  fi
}

echo "=== verify-replacement-parity ts=$TS out=$OUT_DIR ==="

overall_status="PASS"
record_step() {
  # $1=name, $2=status (PASS|FAIL), $3=detail
  name="$1"; status="$2"; detail="$3"
  echo "step=$name status=$status detail=$detail" >> "$OUT_DIR/steps.log"
  if [ "$status" != "PASS" ]; then
    overall_status="FAIL"
  fi
}

# ------- Step 1: AO2-native provider-readiness producer -------
echo "--- Step 1: AO2-native provider-readiness producer ---"
if bash "$AO2_ROOT/scripts/build-provider-readiness.sh" \
     > "$OUT_DIR/step1-build-readiness.log" 2>&1
then
  ART=$(grep -oE 'provider_readiness_artifact=[^ ]+' "$OUT_DIR/step1-build-readiness.log" | sed 's/provider_readiness_artifact=//')
  STATUS=$(grep -oE 'provider_readiness_status=[a-z_]+' "$OUT_DIR/step1-build-readiness.log" | sed 's/provider_readiness_status=//')
  if [ "$STATUS" = "passed" ]; then
    record_step "provider_readiness_producer" PASS "artifact=$ART status=$STATUS"
  else
    record_step "provider_readiness_producer" FAIL "status=$STATUS (expected passed)"
  fi
else
  record_step "provider_readiness_producer" FAIL "build script exit non-zero"
fi

# ------- Step 2: factory-v3 parity oracle -------
echo "--- Step 2: factory-v3 parity oracle ---"
if bash "$AO2_ROOT/scripts/factory-v3-parity-oracle.sh" \
     --out "$OUT_DIR/step2-parity-oracle" \
     > "$OUT_DIR/step2-parity-oracle.log" 2>&1
then
  OVERALL=$(grep -oE 'factory_v3_parity_oracle_overall=[A-Z]+' "$OUT_DIR/step2-parity-oracle.log" | sed 's/factory_v3_parity_oracle_overall=//')
  PASSED=$(grep -oE 'factory_v3_parity_oracle_passed=[0-9]+/[0-9]+' "$OUT_DIR/step2-parity-oracle.log" | sed 's/factory_v3_parity_oracle_passed=//')
  if [ "$OVERALL" = "PASS" ]; then
    record_step "factory_v3_parity_oracle" PASS "passed=$PASSED"
  else
    record_step "factory_v3_parity_oracle" FAIL "overall=$OVERALL passed=$PASSED"
  fi
else
  record_step "factory_v3_parity_oracle" FAIL "parity oracle exit non-zero"
fi

# ------- Step 3: provider contract verify for all required providers -------
echo "--- Step 3: provider contract verify (all required) ---"
AO2_BIN="${AO2_BIN:-$(default_ao2_bin)}"
contract_ok=0
contract_total=0
mkdir -p "$OUT_DIR/step3-contracts"
for provider in scripted codex claude antigravity; do
  contract_total=$((contract_total + 1))
  if "$AO2_BIN" provider contract --provider "$provider" --verify --json \
       > "$OUT_DIR/step3-contracts/$provider.json" 2> "$OUT_DIR/step3-contracts/$provider.err"
  then
    contract_json=$(native_path "$OUT_DIR/step3-contracts/$provider.json")
    status=$(python3 - "$contract_json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as fh:
    d = json.load(fh)
print(d.get("status", "unknown"))
PY
)
    if [ "$status" = "verified" ]; then
      contract_ok=$((contract_ok + 1))
    fi
  fi
done
if [ "$contract_ok" = "$contract_total" ]; then
  record_step "provider_contract_verify_all" PASS "verified=$contract_ok/$contract_total"
else
  record_step "provider_contract_verify_all" FAIL "verified=$contract_ok/$contract_total"
fi

# ------- Step 4: license-provenance gate (forbidden licenses + signed provenance) -------
echo "--- Step 4: license-provenance gate ---"
if (cd "$AO2_ROOT" && bash scripts/license-provenance-gate.sh) \
     > "$OUT_DIR/step4-license-provenance.log" 2>&1
then
  # license-provenance-gate emits `license_provenance_gate=passed` on success.
  if grep -q 'license_provenance_gate=passed' "$OUT_DIR/step4-license-provenance.log"; then
    DETAIL=$(grep -E 'license_provenance_gate|release_provenance_verify' "$OUT_DIR/step4-license-provenance.log" | tr '\n' ';' | sed 's/;$//')
    record_step "license_provenance_gate" PASS "$DETAIL"
  else
    record_step "license_provenance_gate" FAIL "passed line not found in log"
  fi
else
  record_step "license_provenance_gate" FAIL "license-provenance-gate exit non-zero"
fi

# ------- Emit rollup report -------
python3 - "$OUT_DIR" "$overall_status" "$TS" "$AO2_ROOT" <<'PY'
import json
import os
import re
import sys
import subprocess

out_dir, overall_status, ts, ao2_root = sys.argv[1:]

steps = []
with open(os.path.join(out_dir, "steps.log"), "r", encoding="utf-8") as fh:
    for raw in fh:
        line = raw.strip()
        if not line:
            continue
        # step=<name> status=<status> detail=<detail>
        match = re.match(r"step=(\S+)\s+status=(\S+)\s+detail=(.*)", line)
        if not match:
            continue
        steps.append({
            "name": match.group(1),
            "status": match.group(2),
            "detail": match.group(3),
        })

git_head = subprocess.check_output(
    ["git", "rev-parse", "HEAD"], cwd=ao2_root, text=True
).strip()

passed = sum(1 for s in steps if s["status"] == "PASS")
total = len(steps)

report = {
    "schema_version": "ao2.replacement-parity-verification.v1",
    "generated_at_utc": ts,
    "ao2_git_head": git_head,
    "overall_verdict": overall_status,
    "counts": {
        "total_steps": total,
        "passed": passed,
        "failed": total - passed,
    },
    "trust_boundary": {
        "role": "ao2_canonical_with_factory_v3_parity_oracle",
        "ao2_role": "canonical_producer",
        "factory_v3_role": "parity_oracle_only",
        "mutates_ao_artifacts": False,
        "mutates_control_plane": False,
    },
    "steps": steps,
    "next_action": (
        "all replacement-parity steps passed — AO2 may proceed with the next "
        "release-line decision"
        if overall_status == "PASS"
        else "investigate the FAIL step(s) and re-run before promoting AO2 over factory-v3"
    ),
}

report_path = os.path.join(out_dir, "replacement-parity.json")
with open(report_path, "w", encoding="utf-8") as fh:
    json.dump(report, fh, indent=2, sort_keys=True)

print(f"replacement_parity_verification_out={out_dir}")
print(f"replacement_parity_verification_report={report_path}")
print(f"replacement_parity_verification_verdict={overall_status}")
print(f"replacement_parity_verification_passed={passed}/{total}")
PY

[ "$overall_status" = "PASS" ] || exit 1
