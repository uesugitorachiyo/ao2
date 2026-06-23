#!/usr/bin/env bash
set -euo pipefail

repository="${AO2_GITHUB_REPOSITORY:-uesugitorachiyo/ao2}"
branch="${AO2_BRANCH_PROTECTION_BRANCH:-main}"
out="${AO2_BRANCH_PROTECTION_AUDIT:-}"
mode="${AO2_BRANCH_PROTECTION_MODE:-full}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

if [[ "$mode" == "full" ]]; then
  gh api "repos/${repository}/branches/${branch}/protection" >"${tmpdir}/protection.json"
elif [[ "$mode" == "limited" ]]; then
  gh api "repos/${repository}/branches/${branch}" >"${tmpdir}/branch.json"
else
  echo "unsupported AO2_BRANCH_PROTECTION_MODE: ${mode}" >&2
  exit 2
fi

python3 - "$repository" "$branch" "$mode" "$tmpdir" "$out" <<'PY'
import datetime
import json
import pathlib
import sys

repository, branch, mode, tmpdir, out = sys.argv[1:]
tmpdir = pathlib.Path(tmpdir)

required_checks = [
    'Cargo deny (supply chain)',
    'Release archive hosted smoke macos-latest',
    'Release archive hosted smoke ubuntu-latest',
    'Release archive hosted smoke windows-latest',
    'Verify macos-latest / build-release',
    'Verify macos-latest / clippy',
    'Verify macos-latest / test-cli-approval-control-plane',
    'Verify macos-latest / test-cli-approval-core',
    'Verify macos-latest / test-cli-approval-factory-other',
    'Verify macos-latest / test-cli-approval-factory-plan',
    'Verify macos-latest / test-cli-approval-factory-project',
    'Verify macos-latest / test-cli-approval-factory-queue',
    'Verify macos-latest / test-cli-approval-plugin',
    'Verify macos-latest / test-cli-approval-pulse-provider-release',
    'Verify macos-latest / test-cli-approval-workbench-core',
    'Verify macos-latest / test-cli-approval-workbench-project',
    'Verify macos-latest / test-cli-approval-workbench-provider',
    'Verify macos-latest / test-cli-approval-workbench-queue',
    'Verify macos-latest / test-cli-approval-workbench-release-run-support',
    'Verify macos-latest / test-cli-non-approval',
    'Verify macos-latest / test-workspace-non-cli',
    'Verify ubuntu-latest / build-release',
    'Verify ubuntu-latest / clippy',
    'Verify ubuntu-latest / fmt',
    'Verify ubuntu-latest / test-cli-approval-control-plane',
    'Verify ubuntu-latest / test-cli-approval-core',
    'Verify ubuntu-latest / test-cli-approval-factory-other',
    'Verify ubuntu-latest / test-cli-approval-factory-plan',
    'Verify ubuntu-latest / test-cli-approval-factory-project',
    'Verify ubuntu-latest / test-cli-approval-factory-queue',
    'Verify ubuntu-latest / test-cli-approval-plugin',
    'Verify ubuntu-latest / test-cli-approval-pulse-provider-release',
    'Verify ubuntu-latest / test-cli-approval-workbench-core',
    'Verify ubuntu-latest / test-cli-approval-workbench-project',
    'Verify ubuntu-latest / test-cli-approval-workbench-provider',
    'Verify ubuntu-latest / test-cli-approval-workbench-queue',
    'Verify ubuntu-latest / test-cli-approval-workbench-release-run-support',
    'Verify ubuntu-latest / test-cli-contract-support',
    'Verify ubuntu-latest / test-cli-factory-bridge',
    'Verify ubuntu-latest / test-cli-factory-cancel',
    'Verify ubuntu-latest / test-cli-release-gate-signing-rejections',
    'Verify ubuntu-latest / test-cli-release-gate-signing-sidecars',
    'Verify ubuntu-latest / test-cli-release-gate-signing-verified',
    'Verify ubuntu-latest / test-cli-release-packaging',
    'Verify ubuntu-latest / test-cli-release-support',
    'Verify ubuntu-latest / test-cli-sdd',
    'Verify ubuntu-latest / test-workspace-non-cli',
    'Verify windows-latest / build-release',
    'Verify windows-latest / clippy',
    'Verify windows-latest / test-cli-approval-control-plane',
    'Verify windows-latest / test-cli-approval-core',
    'Verify windows-latest / test-cli-approval-factory-other',
    'Verify windows-latest / test-cli-approval-factory-plan',
    'Verify windows-latest / test-cli-approval-factory-project',
    'Verify windows-latest / test-cli-approval-factory-queue',
    'Verify windows-latest / test-cli-approval-plugin',
    'Verify windows-latest / test-cli-approval-pulse-provider-release',
    'Verify windows-latest / test-cli-approval-workbench-core',
    'Verify windows-latest / test-cli-approval-workbench-project',
    'Verify windows-latest / test-cli-approval-workbench-provider',
    'Verify windows-latest / test-cli-approval-workbench-queue',
    'Verify windows-latest / test-cli-approval-workbench-release-run-support',
    'Verify windows-latest / test-cli-non-approval',
    'Verify windows-latest / test-workspace-non-cli',
]

if mode == "full":
    protection = json.loads((tmpdir / "protection.json").read_text())
    required_status_checks = protection.get("required_status_checks") or {}
    observed_checks = required_status_checks.get("contexts") or []
    checks = {
        "branch_protection_api_available": True,
        "required_status_checks_strict": required_status_checks.get("strict") is True,
        "required_status_checks_complete": False,
        "enforce_admins_enabled": (protection.get("enforce_admins") or {}).get("enabled") is True,
        "required_linear_history_enabled": (protection.get("required_linear_history") or {}).get("enabled") is True,
        "force_pushes_disabled": (protection.get("allow_force_pushes") or {}).get("enabled") is False,
        "deletions_disabled": (protection.get("allow_deletions") or {}).get("enabled") is False,
    }
else:
    branch_info = json.loads((tmpdir / "branch.json").read_text())
    protection = branch_info.get("protection") or {}
    required_status_checks = protection.get("required_status_checks") or {}
    observed_checks = required_status_checks.get("contexts") or []
    checks = {
        "branch_metadata_api_available": True,
        "branch_protected": branch_info.get("protected") is True,
        "required_status_checks_complete": False,
        "required_status_checks_enforced_for_everyone": required_status_checks.get("enforcement_level") == "everyone",
    }

observed_check_set = set(observed_checks)
missing_checks = [check for check in required_checks if check not in observed_check_set]
unexpected_checks = [check for check in observed_checks if check not in set(required_checks)]
checks["required_status_checks_complete"] = not missing_checks and not unexpected_checks

errors = []
for name, passed in checks.items():
    if not passed:
        errors.append(name)
if missing_checks:
    errors.append(f"missing required status checks: {', '.join(missing_checks)}")
if unexpected_checks:
    errors.append(f"unexpected required status checks: {', '.join(unexpected_checks)}")

audit = {
    "schema_version": "ao2.branch-protection-audit.v1",
    "status": "passed" if not errors else "blocked",
    "checked_at": datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
    "repository": repository,
    "branch": branch,
    "mode": mode,
    "required_checks": required_checks,
    "observed_checks": observed_checks,
    "checks": checks,
    "errors": errors,
}

rendered = json.dumps(audit, indent=2, sort_keys=True) + "\n"
if out:
    pathlib.Path(out).write_text(rendered)
else:
    sys.stdout.write(rendered)

if audit["status"] != "passed":
    sys.exit(1)
PY
