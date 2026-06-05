#!/bin/sh
set -eu

# One-command Phase 1 replacement promotion wrapper.
# Chains:
#   ao2 factory replacement-smoke-gate
#   scripts/release-gate.sh
#   ao2 release phase1-decision-build
#   ao2 release phase1-promotion-inputs-publish (optional)
#   ao2 release phase1-decision-publish (optional)

timestamp=$(date -u +%Y%m%dT%H%M%SZ)
AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_PHASE1_PROMOTION_ROOT="${AO2_PHASE1_PROMOTION_ROOT:-target/phase1-replacement-promotion/$timestamp}"

path_join() {
  base="$1"
  child="$2"
  case "$base" in
    *\\*)
      base=${base%\\}
      printf "%s\\%s" "$base" "$child"
      ;;
    *)
      base=${base%/}
      printf "%s/%s" "$base" "$child"
      ;;
  esac
}

AO2_MACOS_REPLACEMENT_SMOKE="${AO2_MACOS_REPLACEMENT_SMOKE:-}"
AO2_UBUNTU_REPLACEMENT_SMOKE="${AO2_UBUNTU_REPLACEMENT_SMOKE:-}"
AO2_WINDOWS_REPLACEMENT_SMOKE="${AO2_WINDOWS_REPLACEMENT_SMOKE:-}"
AO2_MACOS_GOVERNED_RUN_EVIDENCE="${AO2_MACOS_GOVERNED_RUN_EVIDENCE:-}"
AO2_UBUNTU_GOVERNED_RUN_EVIDENCE="${AO2_UBUNTU_GOVERNED_RUN_EVIDENCE:-}"
AO2_WINDOWS_GOVERNED_RUN_EVIDENCE="${AO2_WINDOWS_GOVERNED_RUN_EVIDENCE:-}"
AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY="${AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY:-}"
AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY="${AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY:-}"
AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY="${AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY:-}"
AO2_PROJECT_RUN_SUMMARY_ROOT="${AO2_PROJECT_RUN_SUMMARY_ROOT:-target}"
AO2_PROVIDER_ACCEPTANCE_PRESERVATION="${AO2_PROVIDER_ACCEPTANCE_PRESERVATION:-target/release-evidence/provider-pilot-acceptance/$AO2_VERSION/summary.json}"
AO2_REPLACEMENT_SMOKE_GATE="${AO2_REPLACEMENT_SMOKE_GATE:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "replacement-smoke-gate.json")}"
AO2_RELEASE_GATE_OUT="${AO2_RELEASE_GATE_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "release-gate.json")}"
AO2_RELEASE_GATE_ERR="${AO2_RELEASE_GATE_ERR:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "release-gate.err")}"
AO2_PHASE1_DECISION_OUT="${AO2_PHASE1_DECISION_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-promotion-decision.json")}"
AO2_PHASE1_CHECKLIST_OUT="${AO2_PHASE1_CHECKLIST_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-promotion-checklist.json")}"
AO2_PHASE1_DECISION_BUILD_OUT="${AO2_PHASE1_DECISION_BUILD_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-decision-build.json")}"
AO2_PHASE1_PROMOTION_INPUTS="${AO2_PHASE1_PROMOTION_INPUTS:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "promotion-inputs.json")}"
AO2_PHASE1_PROMOTION_INPUTS_VERIFY="${AO2_PHASE1_PROMOTION_INPUTS_VERIFY:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "promotion-inputs-verification.json")}"
AO2_PHASE1_EVIDENCE_BUNDLE_DIR="${AO2_PHASE1_EVIDENCE_BUNDLE_DIR:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "evidence-bundle")}"
AO2_PHASE1_EVIDENCE_BUNDLE_OUT="${AO2_PHASE1_EVIDENCE_BUNDLE_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-evidence-bundle.json")}"
AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT="${AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-evidence-bundle-verification.json")}"
AO2_PHASE1_OPERATOR="${AO2_PHASE1_OPERATOR:-release-lead}"
AO2_PHASE1_RATIONALE="${AO2_PHASE1_RATIONALE:-AO2 owns the replacement run path and all Phase 1 gates are verified.}"
AO2_PHASE1_PROMOTION_PUBLISH="${AO2_PHASE1_PROMOTION_PUBLISH:-0}"
AO2_PHASE1_INPUTS_PUBLISH_OUT="${AO2_PHASE1_INPUTS_PUBLISH_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-inputs-publish.json")}"
AO2_PHASE1_DECISION_PUBLISH_OUT="${AO2_PHASE1_DECISION_PUBLISH_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-decision-publish.json")}"
AO2_PHASE1_CP_HISTORY_GATE_OUT="${AO2_PHASE1_CP_HISTORY_GATE_OUT:-$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-cp-history-gate.json")}"
AO2_PHASE1_PROMOTION_PREFLIGHT="${AO2_PHASE1_PROMOTION_PREFLIGHT:-0}"
AO2_BIN="${AO2_BIN:-}"

require_file() {
  label="$1"
  path="$2"
  if [ -z "$path" ] || [ ! -f "$path" ]; then
    echo "missing $label: $path" >&2
    exit 1
  fi
}

ao2_cmd() {
  if [ -n "$AO2_BIN" ]; then
    "$AO2_BIN" "$@"
  elif [ -x "target/release/ao2" ]; then
    target/release/ao2 "$@"
  else
    cargo run -p ao2-cli --quiet -- "$@"
  fi
}

write_phase1_promotion_inputs() {
  mkdir -p "$(dirname "$AO2_PHASE1_PROMOTION_INPUTS")"
  python3 - \
    "$AO2_PHASE1_PROMOTION_INPUTS" \
    "$AO2_VERSION" \
    "$replacement_smoke_mode" \
    "$AO2_MACOS_REPLACEMENT_SMOKE" \
    "$AO2_UBUNTU_REPLACEMENT_SMOKE" \
    "$AO2_WINDOWS_REPLACEMENT_SMOKE" \
    "$AO2_MACOS_GOVERNED_RUN_EVIDENCE" \
    "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" \
    "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" \
    "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" \
    "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" \
    "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" \
    "$AO2_PROVIDER_ACCEPTANCE_PRESERVATION" \
    "$AO2_REPLACEMENT_SMOKE_GATE" \
    "$AO2_RELEASE_GATE_OUT" \
    "$AO2_PHASE1_DECISION_OUT" \
    "$AO2_PHASE1_CHECKLIST_OUT" \
    "$AO2_PHASE1_EVIDENCE_BUNDLE_OUT" <<'PY'
import json
import sys

(
    out,
    version,
    replacement_smoke_mode,
    macos_smoke,
    ubuntu_smoke,
    windows_smoke,
    macos_governed,
    ubuntu_governed,
    windows_governed,
    macos_project,
    ubuntu_project,
    windows_project,
    provider_acceptance,
    replacement_smoke_gate,
    release_gate,
    phase1_decision,
    phase1_checklist,
    evidence_bundle,
) = sys.argv[1:]

manifest = {
    "schema_version": "ao2.phase1-replacement-promotion-inputs.v1",
    "release_version": version,
    "replacement_smoke_mode": replacement_smoke_mode,
    "trust_boundary": {
        "control_plane_role": "read_only_observer",
        "mutates_ao_artifacts": False,
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "control_plane_approves_release": False,
    },
    "inputs": {
        "replacement_smoke": {
            "macos": macos_smoke,
            "ubuntu": ubuntu_smoke,
            "windows": windows_smoke,
        },
        "governed_run_evidence": {
            "macos": macos_governed,
            "ubuntu": ubuntu_governed,
            "windows": windows_governed,
        },
        "factory_project_run_summary": {
            "macos": macos_project,
            "ubuntu": ubuntu_project,
            "windows": windows_project,
        },
        "provider_acceptance_preservation": provider_acceptance,
    },
    "outputs": {
        "replacement_smoke_gate": replacement_smoke_gate,
        "release_gate": release_gate,
        "phase1_decision": phase1_decision,
        "phase1_checklist": phase1_checklist,
        "phase1_evidence_bundle": evidence_bundle,
    },
}

with open(out, "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2, sort_keys=True)
    f.write("\n")
PY
}

verify_phase1_promotion_inputs() {
  mode="$1"
  verification_stdout=$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "promotion-inputs-verification.$mode.stdout")
  ao2_cmd release phase1-promotion-inputs-verify \
    --manifest "$AO2_PHASE1_PROMOTION_INPUTS" \
    --out "$AO2_PHASE1_PROMOTION_INPUTS_VERIFY" \
    --mode "$mode" \
    --json > "$verification_stdout"
}

discover_factory_project_run_summary() {
  platform="$1"
  # Auto-discovers summaries from local factory-project-run-smoke output and
  # morning-cross-os-readback remote readback output when explicit env vars are absent.
  python3 - "$AO2_PROJECT_RUN_SUMMARY_ROOT" "$platform" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
platform = sys.argv[2]

if not root.exists():
    sys.exit(0)


def normalize(value):
    return str(value or "").strip().lower().replace("_", "-")


def json_platform(path):
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return ""
    for key in ("host_os", "target_os", "os", "platform"):
        value = normalize(data.get(key))
        if value:
            return value
    return ""


def path_matches(path_text):
    if platform == "macos":
        return (
            "macos" in path_text
            or "darwin" in path_text
            or "factory-project-run-smoke" in path_text
        )
    if platform == "ubuntu":
        return "ubuntu" in path_text or "linux" in path_text
    if platform == "windows":
        return "windows" in path_text or "/win-" in path_text or "\\win-" in path_text
    return False


def value_matches(value):
    if platform == "macos":
        return value in {"macos", "darwin", "mac"}
    if platform == "ubuntu":
        return value in {"ubuntu", "linux"}
    if platform == "windows":
        return value in {"windows", "win32", "win"}
    return False


candidates = []
for path in root.rglob("factory-project-run-summary.json"):
    path_text = str(path).lower()
    value = json_platform(path)
    if value_matches(value) or (not value and path_matches(path_text)):
        candidates.append(path)

if candidates:
    print(sorted(str(path) for path in candidates)[-1])
PY
}

if [ -z "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" ]; then
  AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY=$(discover_factory_project_run_summary macos)
fi
if [ -z "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" ]; then
  AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY=$(discover_factory_project_run_summary ubuntu)
fi
if [ -z "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" ]; then
  AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY=$(discover_factory_project_run_summary windows)
fi

mkdir -p "$AO2_PHASE1_PROMOTION_ROOT"

require_file "macOS governed-run evidence" "$AO2_MACOS_GOVERNED_RUN_EVIDENCE"
require_file "Ubuntu governed-run evidence" "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE"
require_file "Windows governed-run evidence" "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE"
require_file "macOS factory project-run summary" "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY"
require_file "Ubuntu factory project-run summary" "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY"
require_file "Windows factory project-run summary" "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY"
require_file "provider acceptance preservation" "$AO2_PROVIDER_ACCEPTANCE_PRESERVATION"

replacement_smoke_mode="governed_run_primary"
if [ -n "$AO2_MACOS_REPLACEMENT_SMOKE$AO2_UBUNTU_REPLACEMENT_SMOKE$AO2_WINDOWS_REPLACEMENT_SMOKE" ]; then
  require_file "macOS replacement smoke" "$AO2_MACOS_REPLACEMENT_SMOKE"
  require_file "Ubuntu replacement smoke" "$AO2_UBUNTU_REPLACEMENT_SMOKE"
  require_file "Windows replacement smoke" "$AO2_WINDOWS_REPLACEMENT_SMOKE"
  replacement_smoke_mode="legacy_replacement_smoke_bound"
fi

write_phase1_promotion_inputs
verify_phase1_promotion_inputs preflight

if [ "$AO2_PHASE1_PROMOTION_PREFLIGHT" = "1" ]; then
  if [ "$AO2_PHASE1_PROMOTION_PUBLISH" = "1" ]; then
    AO2_PHASE1_SIGNING_KEY="${AO2_PHASE1_SIGNING_KEY:-}"
    AO2_PHASE1_CONTROL_PLANE_URL="${AO2_PHASE1_CONTROL_PLANE_URL:-}"
    AO2_PHASE1_API_TOKEN_ENV="${AO2_PHASE1_API_TOKEN_ENV:-}"
    require_file "Phase 1 signing key" "$AO2_PHASE1_SIGNING_KEY"
    if [ -z "$AO2_PHASE1_CONTROL_PLANE_URL" ]; then
      echo "missing AO2_PHASE1_CONTROL_PLANE_URL" >&2
      exit 1
    fi
    if [ -z "$AO2_PHASE1_API_TOKEN_ENV" ]; then
      echo "missing AO2_PHASE1_API_TOKEN_ENV; publish uses env-token auth to avoid leaking bearer tokens in process args" >&2
      exit 1
    fi
  fi

  printf "phase1_replacement_promotion_root=%s\n" "$AO2_PHASE1_PROMOTION_ROOT"
  printf "macos_replacement_smoke=%s\n" "$AO2_MACOS_REPLACEMENT_SMOKE"
  printf "ubuntu_replacement_smoke=%s\n" "$AO2_UBUNTU_REPLACEMENT_SMOKE"
  printf "windows_replacement_smoke=%s\n" "$AO2_WINDOWS_REPLACEMENT_SMOKE"
  printf "replacement_smoke_mode=%s\n" "$replacement_smoke_mode"
  printf "macos_governed_run_evidence=%s\n" "$AO2_MACOS_GOVERNED_RUN_EVIDENCE"
  printf "ubuntu_governed_run_evidence=%s\n" "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE"
  printf "windows_governed_run_evidence=%s\n" "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE"
  printf "macos_factory_project_run_summary=%s\n" "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY"
  printf "ubuntu_factory_project_run_summary=%s\n" "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY"
  printf "windows_factory_project_run_summary=%s\n" "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY"
  printf "provider_acceptance_preservation=%s\n" "$AO2_PROVIDER_ACCEPTANCE_PRESERVATION"
  printf "phase1_promotion_inputs=%s\n" "$AO2_PHASE1_PROMOTION_INPUTS"
  printf "phase1_promotion_inputs_verification=%s\n" "$AO2_PHASE1_PROMOTION_INPUTS_VERIFY"
  printf "replacement_smoke_gate=%s\n" "$AO2_REPLACEMENT_SMOKE_GATE"
  printf "release_gate=%s\n" "$AO2_RELEASE_GATE_OUT"
  printf "phase1_decision=%s\n" "$AO2_PHASE1_DECISION_OUT"
  printf "phase1_checklist=%s\n" "$AO2_PHASE1_CHECKLIST_OUT"
  printf "phase1_evidence_bundle_archive=%s\n" "$AO2_PHASE1_EVIDENCE_BUNDLE_DIR"
  printf "phase1_evidence_bundle_verification=%s\n" "$AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT"
  printf "phase1_publish=%s\n" "$AO2_PHASE1_PROMOTION_PUBLISH"
  printf "phase1_replacement_promotion_preflight=passed\n"
  exit 0
fi

if [ "$replacement_smoke_mode" = "legacy_replacement_smoke_bound" ]; then
  # ao2 factory replacement-smoke-gate
  ao2_cmd factory replacement-smoke-gate \
    --smoke "macos=$AO2_MACOS_REPLACEMENT_SMOKE" \
    --smoke "ubuntu=$AO2_UBUNTU_REPLACEMENT_SMOKE" \
    --smoke "windows=$AO2_WINDOWS_REPLACEMENT_SMOKE" \
    --out "$AO2_REPLACEMENT_SMOKE_GATE" \
    --json > "$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "replacement-smoke-gate.build.json")"

  export AO2_REPLACEMENT_SMOKE_GATE
else
  unset AO2_REPLACEMENT_SMOKE_GATE
fi
export AO2_MACOS_GOVERNED_RUN_EVIDENCE
export AO2_UBUNTU_GOVERNED_RUN_EVIDENCE
export AO2_WINDOWS_GOVERNED_RUN_EVIDENCE
export AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY
export AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY
export AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY
export AO2_RELEASE_GATE_OUT
export AO2_RELEASE_GATE_ERR
scripts/release-gate.sh > "$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "release-gate.stdout")"
verify_phase1_promotion_inputs decision-gate

# ao2 release phase1-decision-build
if [ "$replacement_smoke_mode" = "legacy_replacement_smoke_bound" ]; then
  ao2_cmd release phase1-decision-build \
    --release-gate "$AO2_RELEASE_GATE_OUT" \
    --replacement-smoke-gate "$AO2_REPLACEMENT_SMOKE_GATE" \
    --governed-run-evidence "$AO2_MACOS_GOVERNED_RUN_EVIDENCE" \
    --governed-run-evidence "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" \
    --governed-run-evidence "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" \
    --factory-project-run-summary "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" \
    --factory-project-run-summary "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" \
    --factory-project-run-summary "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" \
    --provider-acceptance-preservation "$AO2_PROVIDER_ACCEPTANCE_PRESERVATION" \
    --operator "$AO2_PHASE1_OPERATOR" \
    --rationale "$AO2_PHASE1_RATIONALE" \
    --out "$AO2_PHASE1_DECISION_OUT" \
    --checklist-out "$AO2_PHASE1_CHECKLIST_OUT" \
    --json > "$AO2_PHASE1_DECISION_BUILD_OUT"
else
  ao2_cmd release phase1-decision-build \
    --release-gate "$AO2_RELEASE_GATE_OUT" \
    --governed-run-evidence "$AO2_MACOS_GOVERNED_RUN_EVIDENCE" \
    --governed-run-evidence "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" \
    --governed-run-evidence "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" \
    --factory-project-run-summary "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" \
    --factory-project-run-summary "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" \
    --factory-project-run-summary "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" \
    --provider-acceptance-preservation "$AO2_PROVIDER_ACCEPTANCE_PRESERVATION" \
    --operator "$AO2_PHASE1_OPERATOR" \
    --rationale "$AO2_PHASE1_RATIONALE" \
    --out "$AO2_PHASE1_DECISION_OUT" \
    --checklist-out "$AO2_PHASE1_CHECKLIST_OUT" \
    --json > "$AO2_PHASE1_DECISION_BUILD_OUT"
fi

# ao2 release evidence-bundle
if [ "$replacement_smoke_mode" = "legacy_replacement_smoke_bound" ]; then
  ao2_cmd release evidence-bundle \
    --out-dir "$AO2_PHASE1_EVIDENCE_BUNDLE_DIR" \
    --artifact "release-gate=$AO2_RELEASE_GATE_OUT" \
    --artifact "phase1-decision=$AO2_PHASE1_DECISION_OUT" \
    --artifact "phase1-checklist=$AO2_PHASE1_CHECKLIST_OUT" \
    --artifact "provider-acceptance=$AO2_PROVIDER_ACCEPTANCE_PRESERVATION" \
    --artifact "macos-governed-run=$AO2_MACOS_GOVERNED_RUN_EVIDENCE" \
    --artifact "ubuntu-governed-run=$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" \
    --artifact "windows-governed-run=$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" \
    --artifact "macos-factory-project-run-summary=$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" \
    --artifact "ubuntu-factory-project-run-summary=$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" \
    --artifact "windows-factory-project-run-summary=$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" \
    --artifact "phase1-promotion-inputs=$AO2_PHASE1_PROMOTION_INPUTS" \
    --artifact "phase1-promotion-inputs-verification=$AO2_PHASE1_PROMOTION_INPUTS_VERIFY" \
    --artifact "replacement-smoke-gate=$AO2_REPLACEMENT_SMOKE_GATE" \
    --json > "$AO2_PHASE1_EVIDENCE_BUNDLE_OUT"
else
  ao2_cmd release evidence-bundle \
    --out-dir "$AO2_PHASE1_EVIDENCE_BUNDLE_DIR" \
    --artifact "release-gate=$AO2_RELEASE_GATE_OUT" \
    --artifact "phase1-decision=$AO2_PHASE1_DECISION_OUT" \
    --artifact "phase1-checklist=$AO2_PHASE1_CHECKLIST_OUT" \
    --artifact "provider-acceptance=$AO2_PROVIDER_ACCEPTANCE_PRESERVATION" \
    --artifact "macos-governed-run=$AO2_MACOS_GOVERNED_RUN_EVIDENCE" \
    --artifact "ubuntu-governed-run=$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" \
    --artifact "windows-governed-run=$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" \
    --artifact "macos-factory-project-run-summary=$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" \
    --artifact "ubuntu-factory-project-run-summary=$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" \
    --artifact "windows-factory-project-run-summary=$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" \
    --artifact "phase1-promotion-inputs=$AO2_PHASE1_PROMOTION_INPUTS" \
    --artifact "phase1-promotion-inputs-verification=$AO2_PHASE1_PROMOTION_INPUTS_VERIFY" \
    --json > "$AO2_PHASE1_EVIDENCE_BUNDLE_OUT"
fi
AO2_PHASE1_EVIDENCE_BUNDLE_ARCHIVE=$(
  python3 - "$AO2_PHASE1_EVIDENCE_BUNDLE_OUT" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as f:
    print(json.load(f)["archive"])
PY
)
require_file "Phase 1 evidence bundle archive" "$AO2_PHASE1_EVIDENCE_BUNDLE_ARCHIVE"

# ao2 release evidence-bundle-verify
ao2_cmd release evidence-bundle-verify \
  --bundle "$AO2_PHASE1_EVIDENCE_BUNDLE_ARCHIVE" \
  --json > "$AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT"

if [ "$AO2_PHASE1_PROMOTION_PUBLISH" = "1" ]; then
  AO2_PHASE1_SIGNING_KEY="${AO2_PHASE1_SIGNING_KEY:-}"
  AO2_PHASE1_CONTROL_PLANE_URL="${AO2_PHASE1_CONTROL_PLANE_URL:-}"
  AO2_PHASE1_API_TOKEN_ENV="${AO2_PHASE1_API_TOKEN_ENV:-}"
  require_file "Phase 1 signing key" "$AO2_PHASE1_SIGNING_KEY"
  if [ -z "$AO2_PHASE1_CONTROL_PLANE_URL" ]; then
    echo "missing AO2_PHASE1_CONTROL_PLANE_URL" >&2
    exit 1
  fi
  if [ -z "$AO2_PHASE1_API_TOKEN_ENV" ]; then
    echo "missing AO2_PHASE1_API_TOKEN_ENV; publish uses env-token auth to avoid leaking bearer tokens in process args" >&2
    exit 1
  fi
  # ao2 release phase1-promotion-inputs-publish
  ao2_cmd release phase1-promotion-inputs-publish \
    --verification "$AO2_PHASE1_PROMOTION_INPUTS_VERIFY" \
    --control-plane-url "$AO2_PHASE1_CONTROL_PLANE_URL" \
    --api-token-env "$AO2_PHASE1_API_TOKEN_ENV" \
    --json > "$AO2_PHASE1_INPUTS_PUBLISH_OUT"

  # ao2 release phase1-decision-publish
  ao2_cmd release phase1-decision-publish \
    --decision "$AO2_PHASE1_DECISION_OUT" \
    --signing-key "$AO2_PHASE1_SIGNING_KEY" \
    --signer-id "$AO2_PHASE1_OPERATOR" \
    --control-plane-url "$AO2_PHASE1_CONTROL_PLANE_URL" \
    --api-token-env "$AO2_PHASE1_API_TOKEN_ENV" \
    --json > "$AO2_PHASE1_DECISION_PUBLISH_OUT"

  scripts/check_phase1_cp_history.py \
    --control-plane-url "$AO2_PHASE1_CONTROL_PLANE_URL" \
    --api-token-env "$AO2_PHASE1_API_TOKEN_ENV" \
    --out "$AO2_PHASE1_CP_HISTORY_GATE_OUT" > "$(path_join "$AO2_PHASE1_PROMOTION_ROOT" "phase1-cp-history-gate.stdout")"
fi

printf "phase1_replacement_promotion_root=%s\n" "$AO2_PHASE1_PROMOTION_ROOT"
printf "replacement_smoke_gate=%s\n" "${AO2_REPLACEMENT_SMOKE_GATE:-}"
printf "replacement_smoke_mode=%s\n" "$replacement_smoke_mode"
printf "release_gate=%s\n" "$AO2_RELEASE_GATE_OUT"
printf "phase1_promotion_inputs=%s\n" "$AO2_PHASE1_PROMOTION_INPUTS"
printf "phase1_promotion_inputs_verification=%s\n" "$AO2_PHASE1_PROMOTION_INPUTS_VERIFY"
if [ "$AO2_PHASE1_PROMOTION_PUBLISH" = "1" ]; then
  printf "phase1_inputs_publish=%s\n" "$AO2_PHASE1_INPUTS_PUBLISH_OUT"
  printf "phase1_cp_history_gate=%s\n" "$AO2_PHASE1_CP_HISTORY_GATE_OUT"
fi
printf "phase1_decision=%s\n" "$AO2_PHASE1_DECISION_OUT"
printf "phase1_checklist=%s\n" "$AO2_PHASE1_CHECKLIST_OUT"
printf "phase1_evidence_bundle_archive=%s\n" "$AO2_PHASE1_EVIDENCE_BUNDLE_ARCHIVE"
printf "phase1_evidence_bundle_verification=%s\n" "$AO2_PHASE1_EVIDENCE_BUNDLE_VERIFY_OUT"
printf "phase1_replacement_promotion=passed\n"
