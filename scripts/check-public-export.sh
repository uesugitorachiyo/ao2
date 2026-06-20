#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() {
  echo "public export check failed: $*" >&2
  exit 1
}

require_file() {
  test -f "$1" || fail "missing required file: $1"
}

reject_path() {
  if find . -path "$1" -print -quit | grep -q .; then
    fail "forbidden generated/private path present: $1"
  fi
}

require_file README.md
require_file LICENSE
require_file docs/SECURITY.md
require_file public-export-manifest.json

reject_path "./.ao2"
reject_path "./.ao2-local"
reject_path "./.release-signing"
reject_path "./.gstack"
reject_path "./.pytest_cache"
reject_path "./target"
reject_path "./dist"
reject_path "./dist-*"
reject_path "./dist-provenance"
reject_path "./docs/status"
reject_path "./docs/superpowers"
reject_path "./docs/AGENT-COORDINATION.md"
reject_path "./docs/NEXT-ACTIONS.md"

if git ls-files | grep -E '(^|/)[.]DS_Store$' >/dev/null; then
  fail "tracked .DS_Store present"
fi

scan_files="$(mktemp)"
find . -type f \
  -not -path "./.git/*" \
  -not -path "./target/*" \
  -not -path "./dist/*" \
  -not -path "./dist-*/*" \
  -not -path "./dist-provenance/*" \
  -not -path "./scripts/check-public-export.sh" \
  -print > "$scan_files"

secret_matches="$(grep -aEn -- '-----BEGIN (RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----|(OPENAI_API_KEY|ANTHROPIC_API_KEY)[[:space:]]*=[[:space:]]*(sk|anthropic)-[A-Za-z0-9._=-]{20,}|Authorization:[[:space:]]*Bearer[[:space:]]+(ghp_[A-Za-z0-9_]{20,}|[A-Za-z0-9._=-]{32,})' $(cat "$scan_files") || true)"
if printf '%s\n' "$secret_matches" | grep -avE 'canary|secret|preview|test|should|redact|example|contains|assert|Never expose|BEGIN PRIVATE KEY' | grep -q .; then
  printf '%s\n' "$secret_matches" >&2
  fail "real-looking private key, bearer token, or provider-key value found"
fi

private_path_matches="$(grep -aEn '/Users/[A-Za-z0-9._-]+|C:\\Users\\[A-Za-z0-9._-]+|C:\\\\Users\\\\[A-Za-z0-9._-]+|github[.]com/[^[:space:]]+/ao2[^[:space:]]*[-]private|ao2[^[:space:]]*[-]private' $(cat "$scan_files") || true)"
if printf '%s\n' "$private_path_matches" | grep -avE 'canary|redact|assert|contains|tests/|scripts/release-readiness.sh|scripts/lib/pulse-gate-lib.sh' | grep -q .; then
  printf '%s\n' "$private_path_matches" >&2
  fail "private path or private repo reference found"
fi

if grep -aEn 'ao2-0\.3\.1|v0\.3\.1|ao2-0\.1\.0-windows' $(cat "$scan_files"); then
  fail "stale public release version reference found"
fi

if ! grep -q '"version": "0.4.80"' package.json; then
  fail "package.json does not advertise ao2 version 0.4.80"
fi

rm -f "$scan_files"
echo "public export check passed: ao2"
