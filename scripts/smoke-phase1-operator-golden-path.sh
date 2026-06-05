#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PHASE1_OPERATOR_SMOKE_ROOT:-$ROOT/target/phase1-operator-golden-path/$(date -u +%Y%m%dT%H%M%SZ)}"
READBACK_ROOT="$OUT_ROOT/control-plane-readback"
READBACK_LOG="$OUT_ROOT/control-plane-readback.log"
SUMMARY="$OUT_ROOT/summary.json"
SMOKE_TOKEN="${AO2_PHASE1_CP_TOKEN:-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef}"

mkdir -p "$OUT_ROOT"

AO2_PHASE1_CP_SMOKE_ROOT="$READBACK_ROOT" \
  AO2_PHASE1_CP_TOKEN="$SMOKE_TOKEN" \
  "$ROOT/scripts/smoke-phase1-control-plane-readback.sh" \
  >"$READBACK_LOG" 2>&1

node - "$READBACK_ROOT" "$READBACK_LOG" "$SUMMARY" "$SMOKE_TOKEN" <<'NODE'
const fs = require('fs');
const path = require('path');

const [readbackRoot, readbackLog, summaryPath, token] = process.argv.slice(2);
const readbackSummaryPath = path.join(readbackRoot, 'phase1-control-plane-readback-summary.json');

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function containsForbiddenText(file) {
  const text = fs.readFileSync(file, 'utf8');
  return text.includes('Authorization: Bearer') || (token && text.includes(token));
}

const readback = readJson(readbackSummaryPath);
assert(readback.status === 'passed', 'readback smoke did not pass');
assert(readback.schema_version === 'ao2.phase1-control-plane-readback-smoke.v1', 'unexpected readback schema');
assert(readback.operator_support_bundle, 'missing operator support bundle readback summary');
assert(readback.operator_support_bundle.schema_version === 'ao2.cp-phase1-operator-support-bundle.v1', 'unexpected operator support bundle schema');
assert(readback.operator_support_bundle.verification_schema_version === 'ao2.cp-phase1-operator-support-bundle-verification.v1', 'unexpected operator support bundle verification schema');
assert(readback.operator_support_bundle.checksums_schema_version === 'ao2.cp-phase1-operator-support-bundle-checksums.v1', 'unexpected operator support bundle checksums schema');
assert(readback.operator_support_bundle.verification_status === 'verified', 'operator support bundle verification must pass');
assert(readback.signature_verified === true, 'signed decision must verify');
assert(readback.dashboard_decision_mode === 'governed_run_primary', 'dashboard must show governed_run_primary');
assert((readback.governed_run_evidence_count || 0) >= 3, 'dashboard must show three governed-run evidence entries');
assert(readback.trust_boundary && readback.trust_boundary.role === 'read_only_observer', 'control plane must remain read-only observer');
assert(readback.trust_boundary && readback.trust_boundary.mutates_ao_artifacts === false, 'control plane must not mutate AO artifacts');

const artifacts = readback.artifacts || {};
for (const [name, file] of Object.entries(artifacts)) {
  assert(fs.existsSync(file), `missing readback artifact: ${name}`);
  assert(!containsForbiddenText(file), `token leaked into readback artifact: ${name}`);
}
assert(fs.existsSync(readbackLog), 'missing readback log');
assert(!containsForbiddenText(readbackLog), 'token leaked into readback log');

const summary = {
  schema_version: 'ao2.phase1-operator-golden-path-smoke.v1',
  status: 'passed',
  operator_flow: 'signed_phase1_decision_publish_readback_dashboard',
  control_plane_role: 'read_only_observer_for_signed_phase1_evidence',
  release_acceptance_owner: 'factory-v3 evaluator-closer',
  evidence_requirement: 'evidence must exist before evaluator closure accepts a run',
  readback_summary: readbackSummaryPath,
  readback_log: readbackLog,
  decision_sha256: readback.decision_sha256,
  checklist_sha256: readback.checklist_sha256,
  dashboard_state: readback.dashboard_state,
  dashboard_decision_mode: readback.dashboard_decision_mode,
  governed_run_evidence_count: readback.governed_run_evidence_count,
  signature_verified: readback.signature_verified,
  operator_support_bundle: readback.operator_support_bundle,
  artifacts
};

fs.writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
NODE

printf "phase1_operator_golden_root=%s\n" "$OUT_ROOT"
printf "phase1_operator_golden_summary=%s\n" "$SUMMARY"
printf "phase1_operator_golden=passed\n"
