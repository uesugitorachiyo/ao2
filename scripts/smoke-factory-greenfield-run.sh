#!/bin/sh
set -eu

# Dogfood the factory-facing greenfield replacement entry point:
# ao2 factory greenfield-run

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
AO2_FACTORY_GREENFIELD_RUN_ROOT="${AO2_FACTORY_GREENFIELD_RUN_ROOT:-$repo_root/target/factory-greenfield-run-smoke/$timestamp}"
AO2_BIN="${AO2_BIN:-$repo_root/target/release/ao2}"

ao2_cmd() {
  if [ -x "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  else
    cargo run -p ao2-cli --quiet -- "$@"
  fi
}

mkdir -p "$AO2_FACTORY_GREENFIELD_RUN_ROOT"
AO2_FACTORY_GREENFIELD_RUN_ROOT=$(CDPATH= cd -- "$AO2_FACTORY_GREENFIELD_RUN_ROOT" && pwd)

target="$AO2_FACTORY_GREENFIELD_RUN_ROOT/discount-service-target"
spec="$AO2_FACTORY_GREENFIELD_RUN_ROOT/factory-greenfield-discount.md"
prompt="$AO2_FACTORY_GREENFIELD_RUN_ROOT/provider-prompt.sh"
signing_key="$AO2_FACTORY_GREENFIELD_RUN_ROOT/factory-greenfield-signing-key.pem"
run_out="$AO2_FACTORY_GREENFIELD_RUN_ROOT/run"
summary_json="$AO2_FACTORY_GREENFIELD_RUN_ROOT/factory-greenfield-run-summary.json"
run_json="$AO2_FACTORY_GREENFIELD_RUN_ROOT/factory-greenfield-run.json"

rm -rf "$target" "$run_out"
mkdir -p "$target" "$run_out"
cp -R "$repo_root/fixtures/discount-service/." "$target/"

cat > "$target/tests/test_discount_boundaries.py" <<'PY'
import pytest

from discount_service.discounts import calculate_discount


def test_rejects_negative_price():
    with pytest.raises(ValueError):
        calculate_discount(-1, 0.10)


def test_rejects_discount_rate_above_one():
    with pytest.raises(ValueError):
        calculate_discount(100, 1.25)
PY

cat > "$spec" <<'MD'
# Factory Greenfield Discount Service

Build a governed discount service from a plain greenfield spec.

Acceptance:
- The implementation rejects negative prices.
- The implementation rejects discount rates outside 0..1.
- The verifier can run with `python -m pytest -q`.
MD

cat > "$prompt" <<'SH'
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: factory greenfield run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 17\n'
SH

ao2_cmd workbench support-keygen --out "$signing_key" --bits 2048 >/dev/null

ao2_cmd factory greenfield-run \
  --spec "$spec" \
  --target "$target" \
  --run-id factory-greenfield-smoke \
  --verifier-command "python -m pytest -q" \
  --provider scripted \
  --provider-prompt-file "$prompt" \
  --signing-key "$signing_key" \
  --signer-id factory-greenfield-smoke \
  --out-dir "$run_out" \
  --json > "$run_json"

node - "$run_json" "$summary_json" "$AO2_FACTORY_GREENFIELD_RUN_ROOT" <<'NODE'
const fs = require('fs');
const [runPath, summaryPath, root] = process.argv.slice(2);
const run = JSON.parse(fs.readFileSync(runPath, 'utf8'));

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

assert(run.schema_version === 'ao2.factory-greenfield-run.v1', 'unexpected factory greenfield schema');
assert(run.status === 'accepted', 'factory greenfield run must be accepted');
assert(run.factory_replacement_boundary.factory_v3_drives_workflow === false, 'factory-v3 must not drive workflow');
assert(run.factory_replacement_boundary.factory_v3_role === 'parity_oracle_only', 'factory-v3 role mismatch');
assert(run.factory_replacement_boundary.control_plane_role === 'read_only_observer_after_signed_evidence', 'control-plane role mismatch');
assert(run.factory_replacement_boundary.release_acceptance_owner === 'factory-v3 evaluator-closer', 'release owner mismatch');
assert(run.factory_replacement_boundary.control_plane_approves_release === false, 'control plane must not approve release');
assert(run.factory_replacement_boundary.mutates_ao_artifacts === false, 'must not mutate AO artifacts');
assert(run.greenfield.greenfield_governed_run_checklist.ao2_executed_generated_governed_plan === true, 'governed plan not executed');
assert(run.greenfield.governed_run.evaluator_decision_verification.status === 'accepted', 'evaluator decision not accepted');

for (const key of ['factory_greenfield_run', 'greenfield_governed_run', 'governed_run', 'evidence_pack', 'evaluator_decision']) {
  assert(fs.existsSync(run.artifacts[key]), `missing artifact ${key}: ${run.artifacts[key]}`);
}

assert(JSON.stringify(run).indexOf('Bearer ') === -1, 'bearer token leaked into artifact');

const summary = {
  schema_version: 'ao2.factory-greenfield-run-smoke.v1',
  status: 'passed',
  root,
  run_id: run.run_id,
  run_status: run.status,
  factory_greenfield_schema: run.schema_version,
  factory_v3_drives_workflow: false,
  factory_v3_role: 'parity_oracle_only',
  control_plane_role: run.factory_replacement_boundary.control_plane_role,
  release_acceptance_owner: run.factory_replacement_boundary.release_acceptance_owner,
  control_plane_approves_release: run.factory_replacement_boundary.control_plane_approves_release,
  mutates_ao_artifacts: run.factory_replacement_boundary.mutates_ao_artifacts,
  evaluator_decision_status: run.greenfield.governed_run.evaluator_decision_verification.status,
  artifacts: run.artifacts
};
fs.writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
NODE

rm -f "$signing_key"

printf "factory_greenfield_run_root=%s\n" "$AO2_FACTORY_GREENFIELD_RUN_ROOT"
printf "factory_greenfield_run_summary=%s\n" "$summary_json"
printf "factory_greenfield_run=passed\n"
