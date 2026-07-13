#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
AO2_CONTROL_PLANE_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"

if [ ! -f "$AO2_CONTROL_PLANE_ROOT/Cargo.toml" ]; then
  echo "AO2 Control Plane checkout not found: $AO2_CONTROL_PLANE_ROOT" >&2
  echo "set AO2_CONTROL_PLANE_ROOT to the ao2-control-plane repository" >&2
  exit 2
fi

unset OPENAI_API_KEY ANTHROPIC_API_KEY

cd "$ROOT"
cargo test -p ao2-core --test core_types \
  event_hash_vectors_preserve_legacy_and_policy_bound_migration_contracts -- --exact

cd "$AO2_CONTROL_PLANE_ROOT"
cargo test -p ao2-cp-schema --test canonical \
  ao2_canonical_v1_matches_shared_golden_vectors -- --exact
cargo test -p ao2-cp-server --test evidence_pack \
  post_signed_evidence_pack_verifies_over_exact_bytes_not_reserialization -- --exact
cargo test -p ao2-cp-server --test evidence_pack \
  post_signed_evidence_pack_stores_exact_signed_bytes_ignoring_evidence_pack_field -- --exact

printf 'ao2_evidence_compatibility_gate=passed\n'
