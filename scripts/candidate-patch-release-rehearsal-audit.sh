#!/usr/bin/env bash
set -euo pipefail

BUNDLE_ROOT="${1:-${AO2_CANDIDATE_PATCH_REHEARSAL_BUNDLE:-target/candidate-patch-release-rehearsal/report}}"
AUDIT_JSON="${AO2_CANDIDATE_PATCH_REHEARSAL_AUDIT_JSON:-$BUNDLE_ROOT/candidate-patch-release-rehearsal-audit.json}"

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
elif command -v python >/dev/null 2>&1; then
  python_bin="python"
else
  echo "missing Python interpreter: python3 or python required" >&2
  exit 1
fi

"$python_bin" - "$BUNDLE_ROOT" "$AUDIT_JSON" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

bundle_root = Path(sys.argv[1]).resolve()
audit_path = Path(sys.argv[2]).resolve()
summary_path = bundle_root / "summary.json"
closure_path = bundle_root / "closure.html"
logs_root = bundle_root / "logs"
expected_logs = [
    "release_evidence_closure",
    "release_readiness_static",
    "release_readiness_regression_gate",
    "retention_preflight",
    "artifact_consumer",
    "post_merge_canary",
]
allowed_absence_canaries = [
    "provider_key_or_token_literal_absent:OPENAI_API_KEY=",
    "provider_key_or_token_literal_absent:ANTHROPIC_API_KEY=",
]
token_canaries = [
    "OPENAI_API_KEY=",
    "ANTHROPIC_API_KEY=",
    "ghp_",
    "github_pat_",
    "sk-ant-",
    "sk-proj-",
    "Bearer ",
]

def every_env_canary_occurrence_is_allowed(text: str, canary: str) -> bool:
    prefix = "provider_key_or_token_literal_absent:"
    start = 0
    found = False
    while True:
        idx = text.find(canary, start)
        if idx == -1:
            return found
        found = True
        if not text[max(0, idx - len(prefix)):idx] == prefix:
            return False
        start = idx + len(canary)

failures = []
if not summary_path.is_file():
    failures.append(f"missing summary: {summary_path}")
if not closure_path.is_file():
    failures.append(f"missing closure html: {closure_path}")
if not logs_root.is_dir():
    failures.append(f"missing logs directory: {logs_root}")

summary = {}
if summary_path.is_file():
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

required_summary_values = {
    "schema_version": "ao2.public-release-train-drill.v1",
    "status": "passed",
    "ci_safe_mode": True,
}
for key, expected in required_summary_values.items():
    if summary.get(key) != expected:
        failures.append(f"unexpected summary {key}: {summary.get(key)!r}")

release_train_manifest = summary.get("release_train_manifest", {})
release_targets = summary.get("release_targets", {})
if release_train_manifest.get("schema_version") != "ao2.release-train-manifest.v1":
    failures.append("release_train_manifest schema was not ao2.release-train-manifest.v1")
if release_train_manifest.get("selected_train") != "next_patch":
    failures.append(f"unexpected manifest selected_train: {release_train_manifest.get('selected_train')!r}")
if release_targets.get("selected_train") != "next_patch":
    failures.append(f"unexpected release target selected_train: {release_targets.get('selected_train')!r}")

expected_targets = {
    "ao2": {"tag": "v0.4.81", "version": "0.4.81"},
    "ao2_control_plane": {"tag": "v0.1.14", "version": "0.1.14"},
    "promotion_confirm": "promote-stable-v0.4.81-v0.1.14",
    "public_operator_confirm": "public-release-reviewed-v0.4.81-v0.1.14",
}
for key, expected in expected_targets.items():
    if release_targets.get(key) != expected:
        failures.append(f"unexpected release target {key}: {release_targets.get(key)!r}")

publish_guards = summary.get("publish_guards", {})
if publish_guards.get("refuses_publish_side_effects_by_default") is not True:
    failures.append("publish guard did not refuse side effects by default")
if publish_guards.get("tag_push_publish_deploy") != "not executed by this drill":
    failures.append("tag/push/publish/deploy guard was not explicit")

trust_boundary = summary.get("trust_boundary", {})
if trust_boundary.get("local_only") is not True:
    failures.append("trust boundary local_only was not true")
if trust_boundary.get("stores_credentials") is not False:
    failures.append("trust boundary stores_credentials was not false")

checks = summary.get("checks", [])
if not isinstance(checks, list) or not checks:
    failures.append("checks must be a non-empty list")
for check in checks if isinstance(checks, list) else []:
    if check.get("status") != "passed":
        failures.append(f"check did not pass: {check}")

required_files = [
    {"name": "summary.json", "path": str(summary_path), "present": summary_path.is_file()},
    {"name": "closure.html", "path": str(closure_path), "present": closure_path.is_file()},
]
for log_name in expected_logs:
    log_path = logs_root / f"{log_name}.log"
    present = log_path.is_file()
    required_files.append({"name": f"logs/{log_name}.log", "path": str(log_path), "present": present})
    if not present:
        failures.append(f"missing expected log: {log_path}")

token_matches = []
for path in sorted(bundle_root.rglob("*")) if bundle_root.exists() else []:
    if not path.is_file():
        continue
    if path.resolve() == audit_path:
        continue
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        continue
    for canary in token_canaries:
        if canary not in text:
            continue
        if canary in {"OPENAI_API_KEY=", "ANTHROPIC_API_KEY="} and any(
            allowed in text for allowed in allowed_absence_canaries
        ):
            if every_env_canary_occurrence_is_allowed(text, canary):
                continue
        if canary in text:
            token_matches.append({"file": str(path), "canary": canary})
            continue

if token_matches:
    failures.append("credential material canary appeared in candidate rehearsal bundle")

bundle_files = []
for path in sorted(bundle_root.rglob("*")) if bundle_root.exists() else []:
    if path.is_file():
        bundle_files.append(
            {
                "path": str(path),
                "relative_path": str(path.relative_to(bundle_root)),
                "size_bytes": path.stat().st_size,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            }
        )

status = "passed" if not failures else "failed"
audit = {
    "schema_version": "ao2.candidate-patch-release-rehearsal-audit.v1",
    "status": status,
    "bundle_root": str(bundle_root),
    "source_summary": str(summary_path),
    "release_targets": release_targets,
    "required_files": required_files,
    "bundle_files": bundle_files,
    "failures": failures,
    "token_scan": {
        "allowed_absence_canaries": allowed_absence_canaries,
        "credential_material_included": bool(token_matches),
        "matches": token_matches,
    },
    "trust_boundary": {
        "local_only": True,
        "mutates_github_releases": False,
        "mutates_git_tags": False,
        "stores_credentials": False,
    },
}
audit_path.parent.mkdir(parents=True, exist_ok=True)
audit_path.write_text(json.dumps(audit, indent=2, sort_keys=True) + "\n", encoding="utf-8")
if status == "passed":
    print("candidate_rehearsal_bundle_audit=passed")
else:
    print("candidate_rehearsal_bundle_audit=failed")
print(f"candidate_rehearsal_bundle_audit_summary={audit_path}")
if status != "passed":
    raise SystemExit(1)
PY
