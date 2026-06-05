#!/bin/sh
set -eu

AO2_PHASE1_PROMOTION_ROOTS="${AO2_PHASE1_PROMOTION_ROOTS:-target/phase1-replacement-promotion target/phase1-governed-promotion}"
AO2_PHASE1_EVIDENCE_BUNDLE="${AO2_PHASE1_EVIDENCE_BUNDLE:-}"
AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT="${AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT:-target/phase1-evidence-bundle-latest-verification.json}"
AO2_BIN="${AO2_BIN:-}"

ao2_cmd() {
  if [ -n "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  elif [ -x target/release/ao2 ]; then
    target/release/ao2 "$@"
  else
    cargo run --quiet --release -p ao2-cli -- "$@"
  fi
}

stat_mtime() {
  stat -f "%m" "$1" 2>/dev/null || stat -c "%Y" "$1"
}

if [ -z "$AO2_PHASE1_EVIDENCE_BUNDLE" ]; then
  AO2_PHASE1_EVIDENCE_BUNDLE="$(
    for root in $AO2_PHASE1_PROMOTION_ROOTS; do
      if [ -d "$root" ]; then
        find "$root" -path '*/evidence-bundle/ao2-release-evidence-bundle-*.tar.gz' -type f
      fi
    done | while IFS= read -r bundle; do
      [ -n "$bundle" ] || continue
      printf "%s\t%s\n" "$(stat_mtime "$bundle")" "$bundle"
    done | sort -rn | head -n 1 | cut -f 2-
  )"
fi

if [ -z "$AO2_PHASE1_EVIDENCE_BUNDLE" ]; then
  echo "no Phase 1 evidence bundle archive found" >&2
  exit 1
fi

if [ ! -f "$AO2_PHASE1_EVIDENCE_BUNDLE" ]; then
  echo "Phase 1 evidence bundle archive is missing: $AO2_PHASE1_EVIDENCE_BUNDLE" >&2
  exit 1
fi

mkdir -p "$(dirname "$AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT")"

ao2_cmd release evidence-bundle-verify \
  --bundle "$AO2_PHASE1_EVIDENCE_BUNDLE" \
  --json > "$AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT"

printf "phase1_evidence_bundle_archive=%s\n" "$AO2_PHASE1_EVIDENCE_BUNDLE"
printf "phase1_evidence_bundle_verification=%s\n" "$AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT"
printf "phase1_evidence_bundle_verify=passed\n"
