#!/bin/sh
set -eu

AO2_BIN="${AO2_BIN:-target/release/ao2}"
AO2_PHASE1_CP_SMOKE_ROOT="${AO2_PHASE1_CP_SMOKE_ROOT:-$PWD/target/phase1-control-plane-readback/$(date +%Y%m%d%H%M%S)}"
AO2_PHASE1_DECISION="${AO2_PHASE1_DECISION:-}"
AO2_PHASE1_CP_BASE_URL="${AO2_PHASE1_CP_BASE_URL:-}"
AO2_PHASE1_CP_TOKEN="${AO2_PHASE1_CP_TOKEN:-phase1-control-plane-readback-smoke}"
AO2_PHASE1_CP_SIGNER_ID="${AO2_PHASE1_CP_SIGNER_ID:-phase1-control-plane-readback-smoke}"
AO2_CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-../ao2-control-plane}"
AO2_PHASE1_CP_PORT="${AO2_PHASE1_CP_PORT:-18744}"

ao2_cmd() {
  if [ -x "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  else
    cargo run -p ao2-cli --quiet -- "$@"
  fi
}

require_file() {
  label="$1"
  path="$2"
  if [ -z "$path" ] || [ ! -f "$path" ]; then
    echo "$label not found: $path" >&2
    exit 1
  fi
}

latest_phase1_decision() {
  find target/phase1-governed-promotion target/phase1-replacement-promotion \
    -path '*/phase1-promotion-decision.json' -type f 2>/dev/null \
    | sort \
    | tail -1
}

materialize_phase1_decision_fixture() {
  fixture_root="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-decision-fixture"
  mkdir -p "$fixture_root/governed-run-evidence/macos" \
    "$fixture_root/governed-run-evidence/ubuntu" \
    "$fixture_root/governed-run-evidence/windows" \
    "$fixture_root/factory-project-run-readback/macos" \
    "$fixture_root/factory-project-run-readback/ubuntu" \
    "$fixture_root/factory-project-run-readback/windows"

  node - "$fixture_root" <<'NODE'
const fs = require('fs');
const path = require('path');
const root = process.argv[2];

function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), {recursive: true});
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function governedRun(runId) {
  return {
    schema_version: 'ao2.factory-v3-compat-governed-run.v1',
    status: 'accepted',
    run_id: runId,
    plan: {
      ao2_native_plan: {
        role_contract_discovery: {
          mode: 'auto_discovered_from_ao_runspec_layout',
          loaded_count: 7
        }
      }
    },
    run_result_verification: {
      status: 'accepted'
    },
    pack_evidence: {
      status: 'produced',
      signature: {
        signature_verified: true
      }
    },
    evaluator_decision: {
      verdict: 'accepted',
      signature: {
        signature_verified: true
      }
    },
    evaluator_decision_verification: {
      status: 'accepted',
      signature_verified: true
    },
    governed_run_checklist: {
      ao2_planned_factory_compat_workflow: true,
      ao2_queue_executed_factory_compat_workflow: true,
      ao2_verified_primary_run_result: true,
      ao2_packed_primary_evidence: true,
      ao2_signed_evaluator_closure: true,
      ao2_auto_loaded_role_contracts: true,
      factory_v3_drives_workflow: false
    },
    artifacts: {
      governed_run: `target/${runId}/governed-run.json`,
      run_result_verification: `target/${runId}/run-result-verification.json`,
      evidence_pack: `target/${runId}/evidence-pack.json`,
      evaluator_decision: `target/${runId}/evaluator-decision.json`
    },
    factory_v3_role: 'parity_oracle_only',
    ao2_decision_owner: 'ao2-native-governed-run',
    control_plane_role: 'read_only_observer_after_signed_evidence'
  };
}

function factoryProjectRunSummary(os) {
  return {
    schema_version: 'ao2.factory-project-run-smoke.v1',
    status: 'passed',
    host_os: os,
    run_id: `factory-project-run-${os}`,
    factory_project_schema: 'ao2.factory-project-run.v1',
    queued_auto_replacement_packet: `target/${os}/queued/factory-replacement-packet.json`,
    queued_auto_replacement_packet_archive: `target/${os}/queued/factory-replacement-packet.tgz`,
    queued_auto_replacement_packet_status: 'packaged',
    queued_auto_replacement_packet_verification: `target/${os}/queued/factory-replacement-packet-verification.json`,
    queued_auto_replacement_packet_verification_status: 'accepted',
    queued_auto_replacement_packet_verification_checksums_verified: true,
    queued_auto_replacement_packet_verification_trust_boundary_verified: true,
    queued_replacement_packet: `target/${os}/factory-replacement-packet.json`,
    queued_replacement_packet_archive: `target/${os}/factory-replacement-packet.tgz`,
    queued_replacement_packet_schema: 'ao2.factory-replacement-packet.v1',
    queued_replacement_packet_status: 'packaged',
    queued_replacement_packet_sha256: 'a'.repeat(64),
    queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver: true,
    queued_replacement_packet_factory_v3_role: 'evaluator_closer_and_sampling_auditor',
    queued_replacement_packet_verification: `target/${os}/factory-replacement-packet-verification.json`,
    queued_replacement_packet_verification_schema: 'ao2.factory-replacement-packet-verification.v1',
    queued_replacement_packet_verification_status: 'accepted',
    queued_replacement_packet_verification_checksums_verified: true,
    queued_replacement_packet_verification_trust_boundary_verified: true,
    queued_replacement_packet_verification_ao2_replacement_driver_verified: true,
    queued_replacement_packet_verification_factory_v3_evaluator_closer_verified: true
  };
}

writeJson(path.join(root, 'release-gate.json'), {
  schema: 'ao2.release-gate.v1',
  status: 'verified',
  release: {
    provenance_verified: true,
    archive_count: 4
  },
  smoke: {
    status: 'verified'
  },
  obligation_gates: {
    status: 'verified'
  },
  obligation_gate_signing: {
    status: 'verified'
  },
  governed_run_evidence: {
    schema: 'ao2.release-governed-run-evidence-verification.v1',
    status: 'verified',
    accepted_os: ['macos', 'ubuntu', 'windows'],
    missing_os: [],
    duplicate_os: [],
    unknown_os: [],
    input_errors: [],
    reasons: []
  },
  factory_project_run_readback: {
    schema: 'ao2.release-factory-project-run-readback-verification.v1',
    status: 'verified',
    required_os: ['macos', 'ubuntu', 'windows'],
    accepted_os: ['macos', 'ubuntu', 'windows'],
    missing_os: [],
    duplicate_os: [],
    unknown_os: [],
    input_errors: [],
    per_os: [
      {os: 'macos', status: 'accepted'},
      {os: 'ubuntu', status: 'accepted'},
      {os: 'windows', status: 'accepted'}
    ],
    reasons: []
  },
  reasons: []
});

for (const os of ['macos', 'ubuntu', 'windows']) {
  writeJson(
    path.join(root, 'governed-run-evidence', os, 'governed-run.json'),
    governedRun(`phase1-readback-${os}`)
  );
  writeJson(
    path.join(root, 'factory-project-run-readback', os, 'factory-project-run-summary.json'),
    factoryProjectRunSummary(os)
  );
}

writeJson(path.join(root, 'provider-acceptance-preservation.json'), {
  schema: 'ao2.provider-pilot-acceptance-preservation.v1',
  status: 'passed',
  tag: 'phase1-readback-smoke',
  providers: {
    codex: {
      schema_version: 'ao2.codex-provider-pilot-acceptance.v1',
      source_class: 'live',
      run_id: 'phase1-readback-codex-provider-pilot',
      smoke_score: 100,
      minimum_score: 90,
      replay_status: 'accepted',
      digest_failures: 0,
      preserved: 'target/release-evidence/provider-pilot-acceptance/phase1-readback-smoke/codex/provider-pilot-acceptance.json'
    },
    claude: {
      schema_version: 'ao2.claude-provider-pilot-acceptance.v1',
      source_class: 'live',
      run_id: 'phase1-readback-claude-provider-pilot',
      smoke_score: 100,
      minimum_score: 90,
      replay_status: 'accepted',
      digest_failures: 0,
      preserved: 'target/release-evidence/provider-pilot-acceptance/phase1-readback-smoke/claude/provider-pilot-acceptance.json'
    },
    antigravity: {
      schema_version: 'ao2.antigravity-provider-pilot-acceptance.v1',
      source_class: 'live',
      run_id: 'phase1-readback-antigravity-provider-pilot',
      smoke_score: 100,
      minimum_score: 90,
      replay_status: 'accepted',
      digest_failures: 0,
      preserved: 'target/release-evidence/provider-pilot-acceptance/phase1-readback-smoke/antigravity/provider-pilot-acceptance.json'
    }
  }
});
NODE

  ao2_cmd release phase1-decision-build \
    --release-gate "$fixture_root/release-gate.json" \
    --governed-run-evidence "$fixture_root/governed-run-evidence/macos/governed-run.json" \
    --governed-run-evidence "$fixture_root/governed-run-evidence/ubuntu/governed-run.json" \
    --governed-run-evidence "$fixture_root/governed-run-evidence/windows/governed-run.json" \
    --factory-project-run-summary "$fixture_root/factory-project-run-readback/macos/factory-project-run-summary.json" \
    --factory-project-run-summary "$fixture_root/factory-project-run-readback/ubuntu/factory-project-run-summary.json" \
    --factory-project-run-summary "$fixture_root/factory-project-run-readback/windows/factory-project-run-summary.json" \
    --provider-acceptance-preservation "$fixture_root/provider-acceptance-preservation.json" \
    --operator "$AO2_PHASE1_CP_SIGNER_ID" \
    --rationale "AO2 self-contained Phase 1 readback smoke materialized governed-run fixture evidence." \
    --out "$fixture_root/phase1-promotion-decision.json" \
    --checklist-out "$fixture_root/phase1-promotion-checklist.json" \
    --json > "$fixture_root/phase1-decision-build.json"

  printf "%s\n" "$fixture_root/phase1-promotion-decision.json"
}

mkdir -p "$AO2_PHASE1_CP_SMOKE_ROOT"
AO2_PHASE1_CP_SMOKE_ROOT=$(CDPATH= cd -- "$AO2_PHASE1_CP_SMOKE_ROOT" && pwd)

if [ -z "$AO2_PHASE1_DECISION" ]; then
  AO2_PHASE1_DECISION=$(latest_phase1_decision)
fi
if [ -z "$AO2_PHASE1_DECISION" ]; then
  AO2_PHASE1_DECISION=$(materialize_phase1_decision_fixture)
fi
require_file "Phase 1 promotion decision" "$AO2_PHASE1_DECISION"

signing_key="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-control-plane-readback-signing-key.pem"
publish_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-decision-publish.json"
history_fetch_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-history-fetch.json"
history_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-history.json"
latest_decision_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-latest-decision.json"
signature_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-latest-decision-signature.json"
dashboard_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-dashboard.json"
dashboard_html="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-dashboard.html"
operator_panel_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-operator-panel.json"
operator_panel_html="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-operator-panel.html"
operator_support_bundle_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-operator-support-bundle.json"
operator_support_bundle_download_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-operator-support-bundle-download.json"
operator_support_bundle_checksums="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-operator-support-bundle-SHA256SUMS"
operator_support_bundle_verify_html="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-operator-support-bundle-verify.html"
operator_support_bundle_verify_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-operator-support-bundle-verify.json"
summary_json="$AO2_PHASE1_CP_SMOKE_ROOT/phase1-control-plane-readback-summary.json"
server_log="$AO2_PHASE1_CP_SMOKE_ROOT/ao2-cp-server.log"
server_err="$AO2_PHASE1_CP_SMOKE_ROOT/ao2-cp-server.err"
server_data="$AO2_PHASE1_CP_SMOKE_ROOT/control-plane-data"

server_pid=""
cleanup() {
  if [ -n "$server_pid" ]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
  rm -f "$signing_key"
}
trap cleanup EXIT

if [ -z "$AO2_PHASE1_CP_BASE_URL" ]; then
  if [ ! -d "$AO2_CONTROL_PLANE_ROOT" ]; then
    echo "AO2_CONTROL_PLANE_ROOT does not exist: $AO2_CONTROL_PLANE_ROOT" >&2
    exit 1
  fi
  bind="127.0.0.1:$AO2_PHASE1_CP_PORT"
  AO2_PHASE1_CP_BASE_URL="http://$bind"
  mkdir -p "$server_data"
  (
    cd "$AO2_CONTROL_PLANE_ROOT"
      AO2_CP_BIND="$bind" \
      AO2_CP_DATA_DIR="$server_data" \
      AO2_CP_API_TOKEN="$AO2_PHASE1_CP_TOKEN" \
      cargo run -p ao2-cp-server --bin ao2-cp-server --quiet
  ) >"$server_log" 2>"$server_err" &
  server_pid=$!

  attempt=1
  while [ "$attempt" -le 200 ]; do
    if curl -fsS "$AO2_PHASE1_CP_BASE_URL/healthz" >/dev/null 2>&1; then
      break
    fi
    if ! kill -0 "$server_pid" >/dev/null 2>&1; then
      cat "$server_log" >&2 || true
      cat "$server_err" >&2 || true
      exit 1
    fi
    sleep 0.1
    attempt=$((attempt + 1))
  done
  if [ "$attempt" -gt 200 ]; then
    cat "$server_log" >&2 || true
    cat "$server_err" >&2 || true
    echo "ao2-control-plane did not become healthy at $AO2_PHASE1_CP_BASE_URL" >&2
    exit 1
  fi
fi

ao2_cmd workbench support-keygen --out "$signing_key" --bits 2048 >/dev/null

ao2_cmd release phase1-decision-publish \
  --decision "$AO2_PHASE1_DECISION" \
  --signing-key "$signing_key" \
  --signer-id "$AO2_PHASE1_CP_SIGNER_ID" \
  --control-plane-url "$AO2_PHASE1_CP_BASE_URL" \
  --api-token "$AO2_PHASE1_CP_TOKEN" \
  --json > "$publish_json"

ao2_cmd release phase1-history-fetch \
  --control-plane-url "$AO2_PHASE1_CP_BASE_URL" \
  --api-token "$AO2_PHASE1_CP_TOKEN" \
  --out "$history_json" \
  --json > "$history_fetch_json"

decision_sha=$(node -e "const fs=require('fs'); const j=JSON.parse(fs.readFileSync(process.argv[1], 'utf8')); process.stdout.write(j.receipt.sha256 || '')" "$publish_json")
if [ -z "$decision_sha" ]; then
  echo "phase1 decision publish receipt missing sha256" >&2
  exit 1
fi

auth_header="Authorization: Bearer $AO2_PHASE1_CP_TOKEN"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/decision/latest" -o "$latest_decision_json"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/decision/$decision_sha/signature" -o "$signature_json"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/dashboard.json" -o "$dashboard_json"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/dashboard" -o "$dashboard_html"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/operator-panel.json" -o "$operator_panel_json"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/operator-panel" -o "$operator_panel_html"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/operator-support-bundle.json" -o "$operator_support_bundle_json"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/operator-support-bundle/download" -o "$operator_support_bundle_download_json"
curl -fsS -H "$auth_header" "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/operator-support-bundle/SHA256SUMS" -o "$operator_support_bundle_checksums"
curl -fsS -H "$auth_header" -H "Content-Type: application/json" \
  --data-binary "@$operator_support_bundle_download_json" \
  "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/operator-support-bundle/verify" \
  -o "$operator_support_bundle_verify_html"
curl -fsS -H "$auth_header" -H "Content-Type: application/json" \
  --data-binary "@$operator_support_bundle_download_json" \
  "$AO2_PHASE1_CP_BASE_URL/api/v1/phase1/promotion/operator-support-bundle/verify.json" \
  -o "$operator_support_bundle_verify_json"

node - "$publish_json" "$history_json" "$latest_decision_json" "$signature_json" "$dashboard_json" "$operator_panel_json" "$operator_support_bundle_json" "$operator_support_bundle_download_json" "$operator_support_bundle_checksums" "$operator_support_bundle_verify_json" "$summary_json" "$AO2_PHASE1_CP_BASE_URL" "$AO2_PHASE1_DECISION" <<'NODE'
const fs = require('fs');
const crypto = require('crypto');
const [
  publishPath,
  historyPath,
  latestDecisionPath,
  signaturePath,
  dashboardPath,
  operatorPanelPath,
  operatorSupportBundlePath,
  operatorSupportBundleDownloadPath,
  operatorSupportBundleChecksumsPath,
  operatorSupportBundleVerifyPath,
  summaryPath,
  baseUrl,
  decisionPath,
] = process.argv.slice(2);

function read(path) {
  return JSON.parse(fs.readFileSync(path, 'utf8'));
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

const publish = read(publishPath);
const history = read(historyPath);
const latestDecision = read(latestDecisionPath);
const signature = read(signaturePath);
const dashboard = read(dashboardPath);
const panel = read(operatorPanelPath);
const operatorSupportBundle = read(operatorSupportBundlePath);
const operatorSupportBundleDownload = read(operatorSupportBundleDownloadPath);
const operatorSupportBundleVerify = read(operatorSupportBundleVerifyPath);
const operatorSupportBundleChecksums = fs.readFileSync(operatorSupportBundleChecksumsPath, 'utf8');

assert(publish.schema_version === 'ao2.phase1-promotion-decision-control-plane-publish.v1', 'unexpected publish schema');
assert(publish.signed === true, 'publish result must be signed');
assert(publish.checklist_publish && publish.checklist_publish.status === 'posted', 'checklist must be posted before decision');
assert(history.schema_version === 'ao2.cp-phase1-promotion-history.v1', 'unexpected history schema');
assert((history.counts && history.counts.checklists || 0) >= 1, 'history missing checklist');
assert((history.counts && history.counts.signed_decisions || 0) >= 1, 'history missing signed decision');
assert(latestDecision.schema === 'factory-v3/ao2-phase1-promotion-decision/v1', 'latest decision schema mismatch');
assert(latestDecision.decision === 'promote_phase1_candidate', 'latest decision is not promote_phase1_candidate');
assert(signature.schema_version === 'ao2.cp-phase1-promotion-decision-signature.v1', 'signature sidecar schema mismatch');
assert(signature.signature && signature.signature.signature_verified === true, 'signature_verified must be true');
assert(dashboard.schema_version === 'ao2.cp-phase1-promotion-dashboard.v1', 'dashboard schema mismatch');
assert(dashboard.decision_artifact && dashboard.decision_artifact.decision_mode === 'governed_run_primary', 'dashboard must show governed_run_primary');
assert((dashboard.decision_artifact.governed_run_evidence_count || 0) >= 3, 'dashboard missing governed-run evidence count');
assert(dashboard.decision_artifact.signature && dashboard.decision_artifact.signature.signature_verified === true, 'dashboard must show verified decision signature');
assert(panel.schema_version === 'ao2.cp-phase1-operator-panel.v1', 'operator panel schema mismatch');
assert(panel.badges && panel.badges.decision_mode === 'governed_run_primary', 'operator panel must show governed_run_primary');
assert(operatorSupportBundle.schema_version === 'ao2.cp-phase1-operator-support-bundle.v1', 'operator support bundle schema mismatch');
assert(operatorSupportBundleDownload.schema_version === 'ao2.cp-phase1-operator-support-bundle.v1', 'operator support bundle download schema mismatch');
assert(operatorSupportBundle.trust_boundary && operatorSupportBundle.trust_boundary.role === 'read_only_observer', 'operator support bundle trust boundary mismatch');
assert(operatorSupportBundle.mutates_ao_artifacts === false, 'operator support bundle must not mutate AO artifacts');
assert(operatorSupportBundleVerify.schema_version === 'ao2.cp-phase1-operator-support-bundle-verification.v1', 'operator support bundle verification schema mismatch');
assert(operatorSupportBundleVerify.status === 'verified', 'operator support bundle verification must pass');
assert(operatorSupportBundleChecksums.includes('ao2.cp-phase1-operator-support-bundle-checksums.v1'), 'operator support bundle checksums schema missing');
assert(operatorSupportBundleChecksums.includes('ao2-phase1-operator-support-bundle.json'), 'operator support bundle checksums filename missing');
const supportBundleDownloadSha256 = crypto.createHash('sha256')
  .update(fs.readFileSync(operatorSupportBundleDownloadPath))
  .digest('hex');
assert(operatorSupportBundleChecksums.includes(`${supportBundleDownloadSha256}  ao2-phase1-operator-support-bundle.json`), 'operator support bundle checksum mismatch');
assert(JSON.stringify({publish, history, latestDecision, signature, dashboard, panel, operatorSupportBundle, operatorSupportBundleDownload, operatorSupportBundleVerify}).indexOf('Bearer ') === -1, 'bearer token leaked into artifacts');
assert(operatorSupportBundleChecksums.indexOf('Bearer ') === -1, 'bearer token leaked into operator support bundle checksums');

const summary = {
  schema_version: 'ao2.phase1-control-plane-readback-smoke.v1',
  status: 'passed',
  control_plane_url: baseUrl,
  phase1_decision: decisionPath,
  decision_sha256: publish.receipt.sha256,
  checklist_sha256: publish.checklist_publish.canonical_sha256,
  dashboard_state: dashboard.state,
  dashboard_decision_mode: dashboard.decision_artifact.decision_mode,
  governed_run_evidence_count: dashboard.decision_artifact.governed_run_evidence_count,
  signature_verified: signature.signature.signature_verified,
  operator_support_bundle: {
    schema_version: operatorSupportBundle.schema_version,
    verification_schema_version: operatorSupportBundleVerify.schema_version,
    verification_status: operatorSupportBundleVerify.status,
    checksums_schema_version: 'ao2.cp-phase1-operator-support-bundle-checksums.v1',
    download_sha256: supportBundleDownloadSha256,
    trust_boundary: operatorSupportBundle.trust_boundary,
  },
  history_counts: history.counts,
  trust_boundary: {
    role: 'read_only_observer',
    mutates_ao_artifacts: false,
    release_acceptance_owner: 'factory-v3 evaluator-closer'
  },
  artifacts: {
    publish: publishPath,
    history: historyPath,
    latest_decision: latestDecisionPath,
    signature: signaturePath,
    dashboard: dashboardPath,
    operator_panel: operatorPanelPath,
    operator_support_bundle: operatorSupportBundlePath,
    operator_support_bundle_download: operatorSupportBundleDownloadPath,
    operator_support_bundle_checksums: operatorSupportBundleChecksumsPath,
    operator_support_bundle_verify: operatorSupportBundleVerifyPath
  }
};
fs.writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
NODE

grep -q 'governed_run_primary' "$dashboard_html"
grep -q 'governed_run_primary' "$operator_panel_html"
grep -q 'verified' "$operator_support_bundle_verify_html"

printf "phase1_control_plane_readback_root=%s\n" "$AO2_PHASE1_CP_SMOKE_ROOT"
printf "phase1_control_plane_readback_summary=%s\n" "$summary_json"
printf "phase1_control_plane_readback_publish=%s\n" "$publish_json"
printf "phase1_control_plane_readback_history=%s\n" "$history_json"
printf "phase1_control_plane_readback_dashboard=%s\n" "$dashboard_json"
printf "phase1_control_plane_readback_operator_panel=%s\n" "$operator_panel_json"
printf "phase1_control_plane_readback_operator_support_bundle=%s\n" "$operator_support_bundle_json"
printf "phase1_control_plane_readback_operator_support_bundle_verify=%s\n" "$operator_support_bundle_verify_json"
printf "phase1_control_plane_readback=passed\n"
