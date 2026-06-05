#!/usr/bin/env bash
# orchestrator-always-bad mock provider: every attempt returns the
# same V5-failing candidate. Used to prove the §6 attempt budget
# exhausts at exactly 3.

set -euo pipefail
cat > /dev/null   # drain stdin envelope
fixture_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cat "$fixture_dir/candidate.json"
