#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CP_ROOT="${AO2_CONTROL_PLANE_ROOT:-$ROOT/../ao2-control-plane}"
OUT_ROOT="${AO2_RELEASE_READINESS_ROOT:-$ROOT/target/release-readiness/$(date -u +%Y%m%dT%H%M%SZ)}"
MODE="default"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --static-only)
      MODE="static-only"
      shift
      ;;
    --full)
      MODE="full"
      shift
      ;;
    *)
      echo "usage: scripts/release-readiness.sh [--static-only|--full]" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$OUT_ROOT"
SUMMARY="$OUT_ROOT/summary.json"

echo "release_readiness_root=$OUT_ROOT"
echo "mode=$MODE"

python3 - "$ROOT" "$CP_ROOT" "$MODE" "$SUMMARY" <<'PY'
import json
import re
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1])
cp_root = Path(sys.argv[2])
mode = sys.argv[3]
summary_path = Path(sys.argv[4])

checks = []

def add(name, status, detail=""):
    checks.append({"name": name, "status": status, "detail": detail})

def read(path):
    return (root / path).read_text(encoding="utf-8")

def run(args, cwd=root):
    return subprocess.run(args, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

package = json.loads(read("package.json"))
scripts = package.get("scripts", {})
for name in [
    "risky-pr:golden",
    "release:readiness",
    "release:readiness:static",
    "smoke:evidence-control-plane",
]:
    add(f"package_script:{name}", "passed" if name in scripts else "failed", scripts.get(name, "missing"))

ci = read(".github/workflows/ci.yml")
add("ci_pull_request_enabled", "passed" if re.search(r"(?m)^\s*pull_request:\s*$", ci) else "failed")
add("ci_main_push_enabled", "passed" if re.search(r"(?m)^\s*branches:\s*\[\s*main\s*\]\s*$", ci) else "failed")
add("ci_read_only_permissions", "passed" if "permissions:\n  contents: read" in ci else "failed")

for workflow in [".github/workflows/release-gate.yml", ".github/workflows/public-release-build.yml"]:
    text = read(workflow)
    manual_only = (
        re.search(r"(?m)^\s*workflow_dispatch:\s*$", text)
        and not re.search(r"(?m)^\s*pull_request:\s*$", text)
        and not re.search(r"(?m)^\s*push:\s*$", text)
    )
    add(f"manual_release_workflow:{workflow}", "passed" if manual_only else "failed")

for script in ["scripts/risky-pr-golden-path.sh", "scripts/release-readiness.sh", "scripts/smoke-evidence-pack-control-plane.sh"]:
    path = root / script
    add(f"script_present:{script}", "passed" if path.is_file() else "failed")
    add(f"script_executable:{script}", "passed" if path.exists() and path.stat().st_mode & 0o100 else "failed")

for forbidden in ["OPENAI_API_" + "KEY=", "ANTHROPIC_API_" + "KEY=", "cat target/long-lived-control-plane/" + "api-token"]:
    combined = "\n".join((root / path).read_text(encoding="utf-8", errors="replace") for path in [
        "scripts/risky-pr-golden-path.sh",
        "scripts/release-readiness.sh",
        "scripts/smoke-evidence-pack-control-plane.sh",
    ])
    add(f"provider_key_or_token_literal_absent:{forbidden}", "passed" if forbidden not in combined else "failed")

if mode != "static-only":
    for repo, expected_min in [("uesugitorachiyo/ao2", 1), ("uesugitorachiyo/ao2-control-plane", 1)]:
        result = run(["gh", "api", f"repos/{repo}/branches/main/protection"])
        if result.returncode != 0:
            add(f"branch_protection:{repo}", "failed", result.stderr.strip() or result.stdout.strip())
            continue
        protection = json.loads(result.stdout)
        contexts = protection.get("required_status_checks", {}).get("contexts") or []
        force_pushes = protection.get("allow_force_pushes", {}).get("enabled")
        deletions = protection.get("allow_deletions", {}).get("enabled")
        ok = len(contexts) >= expected_min and force_pushes is False and deletions is False
        add(f"branch_protection:{repo}", "passed" if ok else "failed", f"contexts={len(contexts)} force_pushes={force_pushes} deletions={deletions}")

    for repo in ["uesugitorachiyo/ao2", "uesugitorachiyo/ao2-control-plane"]:
        result = run(["gh", "run", "list", "--repo", repo, "--branch", "main", "--workflow", "CI", "--limit", "1", "--json", "databaseId,status,conclusion,headSha,url"])
        if result.returncode != 0:
            add(f"latest_main_ci:{repo}", "failed", result.stderr.strip() or result.stdout.strip())
            continue
        runs = json.loads(result.stdout)
        latest = runs[0] if runs else {}
        ok = latest.get("status") == "completed" and latest.get("conclusion") == "success"
        add(f"latest_main_ci:{repo}", "passed" if ok else "failed", json.dumps(latest, sort_keys=True))

if mode == "full":
    full_commands = [
        ["npm", "run", "risky-pr:golden"],
        ["npm", "run", "smoke:evidence-control-plane"],
        ["npm", "run", "verify:no-factory-v3"],
    ]
    for command in full_commands:
        result = run(command)
        add("full_command:" + " ".join(command), "passed" if result.returncode == 0 else "failed", (result.stdout + "\n" + result.stderr)[-4000:])

status = "passed" if all(check["status"] == "passed" for check in checks) else "failed"
summary = {
    "schema_version": "ao2.release-readiness-local.v1",
    "status": status,
    "mode": mode,
    "ao2_root": str(root),
    "control_plane_root_exists": cp_root.is_dir(),
    "checks": checks,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    for check in checks:
        if check["status"] != "passed":
            print(f"failed={check['name']} {check.get('detail', '')}", file=sys.stderr)
    raise SystemExit(1)
PY
