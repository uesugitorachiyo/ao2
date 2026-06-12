#!/bin/sh
# verify-no-factory-v3-green-path.sh
#
# Phase 2 guardrail: fail if AO2 green-path automation starts invoking
# factory-v3 as an executor again. factory-v3 may remain as a read-only
# parity oracle, audit reference, evaluator-closer owner, or explicit public
# mirror source for ao-operator export.
set -eu

AO2_ROOT="${AO2_ROOT:-$PWD}"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-$AO2_ROOT/target/no-factory-v3-green-path/$TS}"
mkdir -p "$OUT_DIR"
OUT_DIR=$(CDPATH= cd -- "$OUT_DIR" && pwd)

python3 - "$AO2_ROOT" "$OUT_DIR" "$TS" <<'PY'
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ao2_root = Path(sys.argv[1]).resolve()
out_dir = Path(sys.argv[2]).resolve()
ts = sys.argv[3]

scan_roots = [
    "package.json",
    "scripts",
    "crates/ao2-cli/src",
    "crates/ao2-runtime/src",
]

factory_ref = re.compile(r"factory[-_]v3", re.IGNORECASE)
required_path = re.compile(
    r"(\.\./factory-v3|/factory-v3\b|factory-v3-cli|factory_v3_cli|FACTORY_V3_ROOT|"
    r"(python3?|bash|sh|node|npm|npx|uv|cargo)\s+[^#\n]*factory[-_]v3)",
    re.IGNORECASE,
)

allowed_by_file = {
    "scripts/factory-v3-parity-oracle.sh": "parity_oracle_script",
    "scripts/verify-replacement-parity.sh": "parity_oracle_wrapper",
}


def is_text_file(path: Path) -> bool:
    try:
        path.read_text(encoding="utf-8")
        return True
    except UnicodeDecodeError:
        return False
    except OSError:
        return False


def iter_files() -> list[Path]:
    files: list[Path] = []
    for root in scan_roots:
        path = ao2_root / root
        if path.is_file():
            files.append(path)
        elif path.is_dir():
            for child in path.rglob("*"):
                if child.is_file() and ".git" not in child.parts and is_text_file(child):
                    files.append(child)
    return sorted(files)


def classify(rel: str, line: str) -> tuple[bool, str]:
    normalized = line.lower()
    has_required_path = bool(required_path.search(line))
    if rel in allowed_by_file:
        return True, allowed_by_file[rel]
    if rel == "scripts/verify-no-factory-v3-green-path.sh":
        return True, "guardrail_self_reference"
    if rel == "scripts/gate-with-replacement.sh" and (
        "no-factory-v3" in line or "no_factory_v3" in line or "no factory-v3" in line
    ):
        return True, "guardrail_invocation"
    if rel.startswith("scripts/") and (
        "verify:no-factory-v3" in line or "no_factory_v3" in line or "no factory-v3" in line
    ):
        return True, "guardrail_invocation"
    if rel.startswith("scripts/parity-oracle-fixtures/"):
        return True, "parity_oracle_fixture"
    if rel.startswith("scripts/parity-oracle-snapshots/"):
        return True, "parity_oracle_snapshot"
    if rel.startswith("crates/") and not has_required_path:
        return True, "rust_compat_or_schema_reference"
    if rel.startswith("scripts/") and line.lstrip().startswith("#") and not has_required_path:
        return True, "script_comment_reference"
    if "factory-v3/" in line:
        return True, "historical_schema_name"
    if "ao_operator_runspec" in line:
        return True, "legacy_runspec_input"
    if rel == "package.json" and (
        '"parity:factory-v3"' in line or '"verify:no-factory-v3"' in line
    ):
        return True, "manual_guard_or_parity_script"
    if rel == "crates/ao2-cli/src/main.rs" and (
        "factory-v3-root" in normalized or "factory_v3_root" in line
    ):
        return True, "skill_contract_manifest_source_only"
    if "parity_oracle_only" in line or "parity oracle" in normalized:
        return True, "parity_oracle_only"
    if "audit-only" in normalized or "read-only audit" in normalized:
        return True, "audit_reference_only"
    if "evaluator-closer" in normalized:
        return True, "evaluator_closer_owner"
    if "factory_v3_drives_workflow" in line and "false" in normalized:
        return True, "non_driving_contract"
    if "factory_v3_required_to_decide" in line and "false" in normalized:
        return True, "non_decision_contract"
    if "ao2_replaces_factory_v3_workflow_driver" in line and "true" in normalized:
        return True, "ao2_replacement_driver_contract"
    if "queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver" in line:
        return True, "ao2_replacement_driver_contract"
    if "queued_replacement_packet_factory_v3_role" in line:
        return True, "evaluator_closer_owner"
    if "queued_replacement_packet_verification_factory_v3_evaluator_closer_verified" in line:
        return True, "evaluator_closer_owner"
    if "factory_v3_evaluator_closer_verified" in line:
        return True, "evaluator_closer_owner"
    if "factory_v3_evaluator_closer_required" in line:
        return True, "evaluator_closer_owner"
    if "boundary roles" in normalized and "workflow driver" in normalized:
        return True, "ao2_replacement_driver_contract"
    if "factory_v3_role" in line and (
        "evaluator_closer" in normalized or "sampling_auditor" in normalized
    ):
        return True, "evaluator_closer_owner"
    if rel == "scripts/release-ship.sh" and (
        "AO2_SYNC_AO_OPERATOR_SOURCE" in line
        or "mirror_run_pair ao-operator" in line
        or "ao-operator (factory-v3)" in line
    ):
        return True, "public_mirror_source_only"
    if rel == "scripts/smoke-phase1-control-plane-readback.sh" and (
        "factory-v3/ao2-phase1-promotion" in line
        or "factory-v3 evaluator-closer" in line
    ):
        return True, "historical_schema_readback"
    if "schema" in normalized and "factory-v3/" in line:
        return True, "historical_schema_name"
    if "ao2.factory-v3-compat" in line:
        return True, "compat_schema_name"
    return False, "unclassified_green_path_factory_v3_reference"


candidates = []
failures = []
for file_path in iter_files():
    rel = file_path.relative_to(ao2_root).as_posix()
    try:
        lines = file_path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        failures.append({
            "path": rel,
            "line": 0,
            "code": "scan_failed",
            "message": str(exc),
        })
        continue
    for number, line in enumerate(lines, start=1):
        if not factory_ref.search(line):
            continue
        allowed, reason = classify(rel, line)
        candidate = {
            "path": rel,
            "line": number,
            "classification": reason,
            "required_path_pattern": bool(required_path.search(line)),
        }
        candidates.append(candidate)
        if not allowed:
            failures.append({
                **candidate,
                "code": "factory_v3_green_path_reference",
                "message": "factory-v3 reference is not classified as parity, audit, evaluator-closer, schema compatibility, or mirror-only",
            })

head = subprocess.check_output(
    ["git", "rev-parse", "HEAD"], cwd=ao2_root, text=True
).strip()

report = {
    "schema_version": "ao2.no-factory-v3-green-path.v1",
    "generated_at_utc": ts,
    "ao2_git_head": head,
    "status": "passed" if not failures else "failed",
    "scan_roots": scan_roots,
    "candidate_count": len(candidates),
    "failure_count": len(failures),
    "failures": failures,
    "trust_boundary": {
        "ao2_role": "canonical_producer",
        "factory_v3_role": "parity_oracle_or_audit_reference_only",
        "control_plane_role": "read_only_observer",
        "mutates_ao_artifacts": False,
        "mutates_control_plane": False,
    },
}

report_path = out_dir / "no-factory-v3-green-path.json"
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

print(f"no_factory_v3_green_path_out={out_dir}")
print(f"no_factory_v3_green_path_report={report_path}")
print(f"no_factory_v3_green_path_candidates={len(candidates)}")
print(f"no_factory_v3_green_path_failures={len(failures)}")
print(f"no_factory_v3_green_path_status={report['status']}")

if failures:
    for failure in failures[:20]:
        print(
            f"failure={failure['path']}:{failure['line']} "
            f"classification={failure['classification']}",
            file=sys.stderr,
        )
    sys.exit(1)
PY
