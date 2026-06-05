#!/bin/sh
# build-provider-readiness.sh
#
# AO2-native producer for the factory-v3/hermes-provider-phase1-readiness/v1
# schema. Produces the artifact from live AO2 evidence:
#   - `ao2 provider contract --verify --json` per provider (codex, claude,
#     antigravity, scripted)
#   - `ao2 provider matrix --json` for doctor and policy invariants
#   - `ao2 provider gate --json` for codex_gate/codex_pilot verdicts
#
# Trust boundary: AO2 is the canonical producer for this artifact. factory-v3
# remains the parity oracle only. The output is a faithful AO2-side rendering
# of the readiness state — including blocked subdimensions — so the long-lived
# control-plane can either accept it as `observed` (if gate ready) or
# `superseded_by_live_acceptance` (if live acceptance carries the verdict).
#
# Usage:
#   bash scripts/build-provider-readiness.sh
#
# Output:
#   target/provider-readiness/<ts>/provider-readiness.json
#
# Exits non-zero if any provider contract verification fails (since posting a
# readiness artifact with unverified contracts is rejected by the control plane).
set -eu

AO2_ROOT="${AO2_ROOT:-$PWD}"
default_ao2_bin() {
  if [ -x "$AO2_ROOT/target/release/ao2" ]; then
    printf "%s" "$AO2_ROOT/target/release/ao2"
  elif [ -x "$AO2_ROOT/target/release/ao2.exe" ]; then
    printf "%s" "$AO2_ROOT/target/release/ao2.exe"
  else
    printf "%s" "$AO2_ROOT/target/release/ao2"
  fi
}

AO2_BIN="${AO2_BIN:-$(default_ao2_bin)}"
if [ ! -x "$AO2_BIN" ]; then
  echo "ao2 binary not executable at $AO2_BIN; build with 'cargo build --release -p ao2-cli'" >&2
  exit 2
fi

TS="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-target/provider-readiness/$TS}"
mkdir -p "$OUT_DIR"
OUT_DIR=$(CDPATH= cd -- "$OUT_DIR" && pwd)

PROVIDERS="scripted codex claude antigravity"
for p in $PROVIDERS; do
  if ! "$AO2_BIN" provider contract --provider "$p" --verify --json > "$OUT_DIR/contract-$p.json" 2> "$OUT_DIR/contract-$p.err"; then
    echo "provider contract verify for $p FAILED (see $OUT_DIR/contract-$p.err)" >&2
    exit 3
  fi
done

# matrix gives doctor info (no extra args needed)
"$AO2_BIN" provider matrix --json > "$OUT_DIR/provider-matrix.json"

# gate is allowed to be not_ready; we capture its verdict but treat it as
# advisory rather than blocking publication. The dashboard interprets
# not_ready + live_acceptance_complete = superseded_by_live_acceptance.
"$AO2_BIN" provider gate --json > "$OUT_DIR/provider-gate.json" 2> "$OUT_DIR/provider-gate.err" || true

GIT_COMMIT=$(git -C "$(dirname "$AO2_BIN")/../.." rev-parse HEAD 2>/dev/null || git rev-parse HEAD)

python3 - "$OUT_DIR" "$GIT_COMMIT" "$TS" <<'PY'
import json
import os
import sys
import hashlib

out_dir, git_commit, ts = sys.argv[1], sys.argv[2], sys.argv[3]


def load(name):
    with open(os.path.join(out_dir, name), "r", encoding="utf-8") as fh:
        return json.load(fh)


def safe_load(name):
    try:
        return load(name)
    except Exception:
        return None


contracts = {}
for provider in ("scripted", "codex", "claude", "antigravity"):
    blob = load(f"contract-{provider}.json")
    status = blob.get("status", "unknown")
    contract_rows = blob.get("contracts", []) or []
    row = contract_rows[0] if contract_rows else {}
    contracts[provider] = {
        "status": status,
        "phase": row.get("phase"),
        "execution_boundary": row.get("execution_boundary"),
        "live_execution_guard_env": row.get("live_execution_guard_env") or None,
        "doctor": row.get("doctor", {}),
        "evidence_contract": row.get("evidence_contract", []),
        "policy_invariants": row.get("policy_invariants", []),
    }

matrix = safe_load("provider-matrix.json") or {}
gate = safe_load("provider-gate.json") or {}

# Map gate verdicts per provider into the codex_gate / codex_pilot shape.
gate_providers = {p.get("provider"): p for p in (gate.get("providers") or [])}

def gate_verdict(provider):
    entry = gate_providers.get(provider, {})
    return entry.get("verdict") or "unknown"


codex_gate = gate_verdict("codex")
codex_pilot = "ready" if codex_gate == "ready" else "blocked"
scripted_gate = gate_verdict("scripted")

# Overall readiness artifact status:
#   - "passed" if every provider contract is verified (regardless of gate;
#     the control-plane's dashboard maps gate==not_ready + acceptance_complete
#     to superseded_by_live_acceptance, which is a green outcome).
#   - "failed" otherwise.
all_verified = all(c.get("status") == "verified" for c in contracts.values())
artifact_status = "passed" if all_verified else "failed"

readiness = {
    "schema": "factory-v3/hermes-provider-phase1-readiness/v1",
    "status": artifact_status,
    "live_provider_policy": "not_run_by_default",
    "required_live_provider_pilots": [],
    "contracts": {
        provider: {"status": meta.get("status", "unknown")}
        for provider, meta in contracts.items()
        if provider in ("codex", "claude", "antigravity")
    },
    "scripted_gate": {"verdict": scripted_gate},
    "codex_gate": {"verdict": codex_gate},
    "codex_pilot": {"status": codex_pilot},
    "ao2_provenance": {
        "producer": "ao2-native",
        "command": "scripts/build-provider-readiness.sh",
        "ao2_git_commit": git_commit,
        "generated_at_utc": ts,
        "evidence_inputs": [
            "ao2 provider contract --verify (per provider)",
            "ao2 provider matrix",
            "ao2 provider gate",
        ],
        "contract_detail": contracts,
        "trust_boundary": {
            "role": "ao2_canonical_producer",
            "factory_v3_role": "parity_oracle_only",
            "mutates_ao_artifacts": False,
        },
    },
}

artifact_path = os.path.join(out_dir, "provider-readiness.json")
canonical = json.dumps(readiness, sort_keys=True, separators=(",", ":")).encode()
sha = hashlib.sha256(canonical).hexdigest()
with open(artifact_path, "w", encoding="utf-8") as fh:
    json.dump(readiness, fh, indent=2, sort_keys=True)

manifest = {
    "schema_version": "ao2.provider-readiness-build-manifest.v1",
    "artifact_path": artifact_path,
    "artifact_sha256_canonical": sha,
    "artifact_status": artifact_status,
    "all_contracts_verified": all_verified,
    "codex_gate_verdict": codex_gate,
    "codex_pilot_status": codex_pilot,
    "generated_at_utc": ts,
    "ao2_git_commit": git_commit,
    "inputs": {
        "contract-scripted.json": os.path.exists(os.path.join(out_dir, "contract-scripted.json")),
        "contract-codex.json": os.path.exists(os.path.join(out_dir, "contract-codex.json")),
        "contract-claude.json": os.path.exists(os.path.join(out_dir, "contract-claude.json")),
        "contract-antigravity.json": os.path.exists(os.path.join(out_dir, "contract-antigravity.json")),
        "provider-matrix.json": bool(matrix),
        "provider-gate.json": bool(gate),
    },
    "trust_boundary": {
        "role": "ao2_canonical_producer",
        "factory_v3_role": "parity_oracle_only",
        "mutates_ao_artifacts": False,
    },
}

with open(os.path.join(out_dir, "manifest.json"), "w", encoding="utf-8") as fh:
    json.dump(manifest, fh, indent=2, sort_keys=True)

print(f"provider_readiness_artifact={artifact_path}")
print(f"provider_readiness_status={artifact_status}")
print(f"provider_readiness_sha256_canonical={sha}")
print(f"codex_gate_verdict={codex_gate}")
print(f"codex_pilot_status={codex_pilot}")
PY

echo "provider_readiness_out=$OUT_DIR"
