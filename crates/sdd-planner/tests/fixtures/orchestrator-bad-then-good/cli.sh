#!/usr/bin/env bash
# orchestrator-bad-then-good mock provider:
# - attempt 1 → V5-failing candidate (mutates_ao_artifacts=true)
# - attempt 2+ → clean candidate
#
# State (attempt counter) lives in $SDD_MOCK_STATE_DIR, which each test
# must point at a fresh tempdir.

set -euo pipefail

state_dir="${SDD_MOCK_STATE_DIR:?SDD_MOCK_STATE_DIR required}"
mkdir -p "$state_dir"
attempt_file="$state_dir/attempt-count"
attempt=$(( $(cat "$attempt_file" 2>/dev/null || echo 0) + 1 ))
echo "$attempt" > "$attempt_file"

cat > /dev/null   # drain stdin envelope

fixture_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ "$attempt" -lt 2 ]; then
    cat "$fixture_dir/attempt-1.json"
else
    cat "$fixture_dir/attempt-2.json"
fi
