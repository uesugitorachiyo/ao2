#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL_ROOT:-$ROOT/target/rsi-live-self-change-rehearsal/latest}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

if [[ "${AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL:-}" != "1" ]]; then
  python3 - "$SUMMARY" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_path = Path(sys.argv[1])
payload = {
    "schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "refused_missing_operator_flag",
    "required_operator_flag": "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1",
    "claim_boundary": {
        "bounded_governed_rsi": "allowed",
        "full_autonomous_self_mutating_rsi": "denied",
    },
    "trust_boundary": {
        "local_only": True,
        "uses_network": False,
        "requires_provider_api_key": False,
        "stores_credentials": False,
        "mutates_repositories": False,
        "applies_patch": False,
        "rollback_applied": False,
        "publishes_claims": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
  echo "summary=$SUMMARY"
  echo "live_self_change_rehearsal=refused"
  exit 1
fi

python3 - "$ROOT" "$OUT_ROOT" <<'PY'
import difflib
import hashlib
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
out_root = Path(sys.argv[2])
target_rel = "scripts/rsi-claim-readiness-audit.sh"
target = root / target_rel
proposed_patch = out_root / "proposed-live-self-change.patch"
rollback_patch = out_root / "rollback-live-self-change.patch"
summary_path = out_root / "summary.json"
marker = "# AO2 RSI live self-change rehearsal marker."


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def unified_diff(before: str, after: str, before_name: str, after_name: str) -> str:
    diff = difflib.unified_diff(
        before.splitlines(keepends=True),
        after.splitlines(keepends=True),
        fromfile=before_name,
        tofile=after_name,
    )
    text = "".join(diff)
    if not text.endswith("\n"):
        text += "\n"
    return text


def write_summary(status: str, payload: dict) -> None:
    base = {
        "schema_version": "ao2.rsi-live-self-change-rehearsal.v1",
        "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "status": status,
    }
    base.update(payload)
    summary_path.write_text(json.dumps(base, indent=2, sort_keys=True) + "\n", encoding="utf-8")


original_bytes = target.read_bytes()
original_text = original_bytes.decode("utf-8")
before_sha = sha256_bytes(original_bytes)

if marker in original_text:
    write_summary(
        "failed_marker_already_present",
        {
            "target_files": [target_rel],
            "trust_boundary": {
                "local_only": True,
                "uses_network": False,
                "requires_provider_api_key": False,
                "stores_credentials": False,
                "mutates_repositories": False,
                "applies_patch": False,
                "rollback_applied": False,
                "publishes_claims": False,
            },
        },
    )
    raise SystemExit("live self-change rehearsal marker already present")

needle = "set -euo pipefail\n\n"
if needle not in original_text:
    write_summary(
        "failed_target_shape_changed",
        {
            "target_files": [target_rel],
            "trust_boundary": {
                "local_only": True,
                "uses_network": False,
                "requires_provider_api_key": False,
                "stores_credentials": False,
                "mutates_repositories": False,
                "applies_patch": False,
                "rollback_applied": False,
                "publishes_claims": False,
            },
        },
    )
    raise SystemExit("target verification script shape changed")

mutated_text = original_text.replace(needle, f"set -euo pipefail\n{marker}\n\n", 1)
proposed_patch.write_text(
    unified_diff(
        original_text,
        mutated_text,
        f"a/{target_rel}",
        f"b/{target_rel}",
    ),
    encoding="utf-8",
)
rollback_patch.write_text(
    unified_diff(
        mutated_text,
        original_text,
        f"a/{target_rel}",
        f"b/{target_rel}",
    ),
    encoding="utf-8",
)

mutation_sha = None
rollback_sha = None
rollback_applied = False
try:
    target.write_text(mutated_text, encoding="utf-8")
    mutation_sha = sha256_file(target)
    subprocess.run(["bash", "-n", str(target)], cwd=root, check=True)
finally:
    target.write_bytes(original_bytes)
    rollback_sha = sha256_file(target)
    rollback_applied = True
    subprocess.run(["bash", "-n", str(target)], cwd=root, check=True)

if mutation_sha == before_sha:
    write_summary(
        "failed_no_mutation",
        {
            "self_change": {
                "mode": "live_rehearsal",
                "repository": "ao2",
                "change_class": "verification_path_hardening",
                "target_files": [target_rel],
                "target_before_sha256": {target_rel: before_sha},
                "target_after_mutation_sha256": mutation_sha,
                "target_after_rollback_sha256": rollback_sha,
                "applies_patch": True,
                "repository_restored": rollback_sha == before_sha,
            },
            "trust_boundary": {
                "local_only": True,
                "uses_network": False,
                "requires_provider_api_key": False,
                "stores_credentials": False,
                "mutates_repositories": True,
                "applies_patch": True,
                "rollback_applied": rollback_applied,
                "publishes_claims": False,
            },
        },
    )
    raise SystemExit("live rehearsal mutation did not change target")

if rollback_sha != before_sha:
    write_summary(
        "failed_rollback_mismatch",
        {
            "self_change": {
                "mode": "live_rehearsal",
                "repository": "ao2",
                "change_class": "verification_path_hardening",
                "target_files": [target_rel],
                "target_before_sha256": {target_rel: before_sha},
                "target_after_mutation_sha256": mutation_sha,
                "target_after_rollback_sha256": rollback_sha,
                "applies_patch": True,
                "repository_restored": False,
            },
            "trust_boundary": {
                "local_only": True,
                "uses_network": False,
                "requires_provider_api_key": False,
                "stores_credentials": False,
                "mutates_repositories": True,
                "applies_patch": True,
                "rollback_applied": rollback_applied,
                "publishes_claims": False,
            },
        },
    )
    raise SystemExit("live rehearsal rollback did not restore target")

write_summary(
    "live_rehearsal_passed",
    {
        "claim_boundary": {
            "bounded_governed_rsi": "allowed",
            "full_autonomous_self_mutating_rsi": "denied",
        },
        "self_change": {
            "mode": "live_rehearsal",
            "repository": "ao2",
            "change_class": "verification_path_hardening",
            "target_files": [target_rel],
            "target_before_sha256": {target_rel: before_sha},
            "target_after_mutation_sha256": mutation_sha,
            "target_after_rollback_sha256": rollback_sha,
            "applies_patch": True,
            "repository_restored": True,
            "proposed_patch": {
                "path": proposed_patch.name,
                "sha256": sha256_file(proposed_patch),
            },
        },
        "rollback": {
            "mode": "live_rehearsal",
            "status": "passed",
            "same_change_class": True,
            "rollback_patch": {
                "path": rollback_patch.name,
                "sha256": sha256_file(rollback_patch),
            },
        },
        "live_self_change_evidence": {
            "status": "passed",
            "evidence_paths": [
                "summary.json",
                "proposed-live-self-change.patch",
                "rollback-live-self-change.patch",
            ],
        },
        "observer_readback": {
            "status": "missing",
            "observer": "ao2-control-plane",
            "evidence_paths": [],
        },
        "full_claim_blockers": [
            "observer_readback",
            "covenant_claim_publish_approval",
            "retained_claim_level_evidence",
        ],
        "trust_boundary": {
            "local_only": True,
            "uses_network": False,
            "requires_provider_api_key": False,
            "stores_credentials": False,
            "mutates_repositories": True,
            "applies_patch": True,
            "rollback_applied": True,
            "publishes_claims": False,
        },
    },
)
print(f"summary={summary_path}")
print("live_self_change_rehearsal=passed")
print("rollback=passed")
PY
