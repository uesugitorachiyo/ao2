#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/operator-release-evidence-audit.yml"
PACKAGE_JSON="$ROOT/package.json"
VERIFICATION_DOC="$ROOT/docs/VERIFICATION.md"
PUBLIC_RELEASE_DOC="$ROOT/docs/release/PUBLIC-RELEASE-VERIFICATION.md"

fail() {
  echo "operator release evidence workflow contract failed: $*" >&2
  exit 1
}

require_file() {
  local path="$1"
  [ -f "$path" ] || fail "missing file: $path"
}

require_text() {
  local path="$1"
  local needle="$2"
  grep -Fq -- "$needle" "$path" || fail "missing '$needle' in $path"
}

require_file "$WORKFLOW"
require_file "$PACKAGE_JSON"
require_file "$VERIFICATION_DOC"
require_file "$PUBLIC_RELEASE_DOC"

require_text "$WORKFLOW" "name: Operator Release Evidence Audit"
require_text "$WORKFLOW" "workflow_dispatch:"
require_text "$WORKFLOW" "schedule:"
require_text "$WORKFLOW" "permissions:"
require_text "$WORKFLOW" "contents: read"
require_text "$WORKFLOW" "actions: read"
require_text "$WORKFLOW" "AO2_OPERATOR_RELEASE_EVIDENCE_ROOT=target/operator-release-evidence-bundle"
require_text "$WORKFLOW" "npm run release:operator-evidence-bundle"
require_text "$WORKFLOW" "ao2.operator-release-evidence-bundle.v1"
require_text "$WORKFLOW" "operator_release_evidence_ready"
require_text "$WORKFLOW" "actions/upload-artifact"
require_text "$WORKFLOW" "ao2-operator-release-evidence-bundle"
require_text "$WORKFLOW" "target/operator-release-evidence-bundle"
require_text "$WORKFLOW" "mutates_releases"
require_text "$WORKFLOW" "stores_credentials"

require_text "$PACKAGE_JSON" "\"release:operator-evidence-workflow-contract\""
require_text "$VERIFICATION_DOC" "Operator Release Evidence Audit"
require_text "$VERIFICATION_DOC" "ao2-public-release-pair-digest-audit"
require_text "$VERIFICATION_DOC" "ao2.public-release-pair-digest-audit.v1"
require_text "$PUBLIC_RELEASE_DOC" "ao2-operator-release-evidence-bundle"
require_text "$PUBLIC_RELEASE_DOC" "ao2-public-release-pair-digest-audit"
require_text "$PUBLIC_RELEASE_DOC" "ao2.public-release-pair-digest-audit.v1"

echo "operator_release_evidence_workflow_contract=passed"
