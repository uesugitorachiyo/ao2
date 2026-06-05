#!/bin/sh
# gate-with-replacement.sh
#
# Composite "ship-ready" gate: runs the replacement-parity verifier FIRST
# (asserting AO2 has full coverage of every factory-v3 critical-path schema),
# THEN runs the canonical `release gate` (asserting cross-OS archives,
# signed provenance, and three-OS smoke evidence are all present and valid).
#
# Exits non-zero if any constituent gate fails. Emits a single rollup
# `ao2.release-gate-with-replacement-parity.v1` so CI and humans can read
# one verdict.
#
# Trust boundary preserved: all checks run AO2-native; factory-v3 is only
# invoked read-only as parity oracle (inside Step 1). No AO artifacts
# mutated, no control-plane mutations.
#
# Usage:
#   bash scripts/gate-with-replacement.sh
#
# Output:
#   target/release-gate-with-replacement/<ts>/rollup.json
set -eu

AO2_ROOT="${AO2_ROOT:-$PWD}"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-$AO2_ROOT/target/release-gate-with-replacement/$TS}"
mkdir -p "$OUT_DIR"
OUT_DIR=$(CDPATH= cd -- "$OUT_DIR" && pwd)

echo "=== gate-with-replacement ts=$TS out=$OUT_DIR ==="

overall_status="PASS"
record_step() {
  name="$1"; status="$2"; detail="$3"
  echo "step=$name status=$status detail=$detail" >> "$OUT_DIR/steps.log"
  if [ "$status" != "PASS" ]; then
    overall_status="FAIL"
  fi
}

# ------- Stage 0: no factory-v3 green-path regression -------
echo "--- Stage 0: no factory-v3 green-path regression ---"
if bash "$AO2_ROOT/scripts/verify-no-factory-v3-green-path.sh" \
     > "$OUT_DIR/stage0-no-factory-v3-green-path.log" 2>&1
then
  STATUS=$(grep -oE 'no_factory_v3_green_path_status=[a-z]+' "$OUT_DIR/stage0-no-factory-v3-green-path.log" | sed 's/no_factory_v3_green_path_status=//')
  FAILURES=$(grep -oE 'no_factory_v3_green_path_failures=[0-9]+' "$OUT_DIR/stage0-no-factory-v3-green-path.log" | sed 's/no_factory_v3_green_path_failures=//')
  if [ "$STATUS" = "passed" ]; then
    record_step "no_factory_v3_green_path" PASS "failures=$FAILURES"
  else
    record_step "no_factory_v3_green_path" FAIL "status=$STATUS failures=$FAILURES"
  fi
else
  record_step "no_factory_v3_green_path" FAIL "guard exit non-zero"
fi

# ------- Stage A: replacement-parity (4-step composite) -------
echo "--- Stage A: replacement-parity verifier (4-step composite) ---"
if [ "$overall_status" = "PASS" ]; then
  if bash "$AO2_ROOT/scripts/verify-replacement-parity.sh" \
       > "$OUT_DIR/stageA-replacement-parity.log" 2>&1
  then
    VERDICT=$(grep -oE 'replacement_parity_verification_verdict=[A-Z]+' "$OUT_DIR/stageA-replacement-parity.log" | sed 's/replacement_parity_verification_verdict=//')
    PASSED=$(grep -oE 'replacement_parity_verification_passed=[0-9]+/[0-9]+' "$OUT_DIR/stageA-replacement-parity.log" | sed 's/replacement_parity_verification_passed=//')
    if [ "$VERDICT" = "PASS" ]; then
      record_step "replacement_parity" PASS "passed=$PASSED"
    else
      record_step "replacement_parity" FAIL "verdict=$VERDICT passed=$PASSED"
    fi
  else
    record_step "replacement_parity" FAIL "stage A exit non-zero"
  fi
else
  echo "--- Stage A: SKIPPED because Stage 0 FAILED ---"
  record_step "replacement_parity" SKIPPED "stage 0 failed, stage A not run"
fi

# Only run stage B if stage A passed — release-gate is meaningless if
# replacement-parity is broken.
if [ "$overall_status" = "PASS" ]; then
  # ------- Stage B: canonical release-gate -------
  echo "--- Stage B: canonical release-gate ---"
  if (cd "$AO2_ROOT" && bash scripts/release-gate.sh) \
       > "$OUT_DIR/stageB-release-gate.log" 2>&1
  then
    if grep -q 'release_gate=passed' "$OUT_DIR/stageB-release-gate.log"; then
      VER=$(grep -oE 'release_gate_version=[^ ]+' "$OUT_DIR/stageB-release-gate.log" | sed 's/release_gate_version=//' | head -1)
      record_step "release_gate" PASS "version=$VER"
    else
      record_step "release_gate" FAIL "release_gate=passed not in log"
    fi
  else
    record_step "release_gate" FAIL "release-gate exit non-zero"
  fi
else
  echo "--- Stage B: SKIPPED because Stage A FAILED ---"
  record_step "release_gate" SKIPPED "stage A failed, stage B not run"
fi

# ------- Emit rollup -------
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

rollup = {
    "schema_version": "ao2.release-gate-with-replacement-parity.v1",
    "generated_at_utc": ts,
    "ao2_git_head": git_head,
    "overall_verdict": overall_status,
    "counts": {
        "total_stages": total,
        "passed": passed,
        "non_passed": total - passed,
    },
    "trust_boundary": {
        "role": "ao2_canonical_full_release_gate",
        "ao2_role": "canonical_producer",
        "factory_v3_role": "parity_oracle_only",
        "mutates_ao_artifacts": False,
        "mutates_control_plane": False,
    },
    "stages": steps,
    "next_action": (
        "release-gate-with-replacement PASS — AO2 ready to ship the cross-OS release"
        if overall_status == "PASS"
        else "investigate the FAIL stage(s) before shipping"
    ),
}

report_path = os.path.join(out_dir, "rollup.json")
with open(report_path, "w", encoding="utf-8") as fh:
    json.dump(rollup, fh, indent=2, sort_keys=True)

print(f"gate_with_replacement_out={out_dir}")
print(f"gate_with_replacement_rollup={report_path}")
print(f"gate_with_replacement_verdict={overall_status}")
print(f"gate_with_replacement_passed={passed}/{total}")
PY

[ "$overall_status" = "PASS" ] || exit 1
