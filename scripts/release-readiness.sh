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
import html
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
    "release:readiness:regression-gate",
    "smoke:evidence-control-plane",
]:
    add(f"package_script:{name}", "passed" if name in scripts else "failed", scripts.get(name, "missing"))

ci = read(".github/workflows/ci.yml")
add("ci_pull_request_enabled", "passed" if re.search(r"(?m)^\s*pull_request:\s*$", ci) else "failed")
add("ci_main_push_enabled", "passed" if re.search(r"(?m)^\s*branches:\s*\[\s*main\s*\]\s*$", ci) else "failed")
add("ci_read_only_permissions", "passed" if "permissions:\n  contents: read" in ci else "failed")

required_ci_os = ["ubuntu-latest", "macos-latest", "windows-latest"]

def workflow_job_block(job_name):
    match = re.search(
        rf"(?ms)^  {re.escape(job_name)}:\n(?P<body>.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)",
        ci,
    )
    return match.group("body") if match else None

def add_job_matrix_os_check(job_name, expected_os):
    block = workflow_job_block(job_name)
    if block is None:
        add(f"ci_job_required_os:{job_name}", "failed", "job_missing")
        return
    missing = [os_name for os_name in expected_os if os_name not in block]
    add(
        f"ci_job_required_os:{job_name}",
        "passed" if not missing else "failed",
        "required_os=" + ",".join(expected_os) + (" missing=" + ",".join(missing) if missing else ""),
    )

add_job_matrix_os_check("verify", required_ci_os)
add_job_matrix_os_check("release-archive-hosted-smoke", required_ci_os)
add_job_matrix_os_check("workbench-operator-packet-control-plane-smoke", required_ci_os)
add_job_matrix_os_check("non_approval_required_check_compat", ["macos-latest", "windows-latest"])

operator_index = workflow_job_block("workbench-operator-packet-control-plane-smoke-index")
operator_index_ok = (
    operator_index is not None
    and "needs: workbench-operator-packet-control-plane-smoke" in operator_index
    and "AO2_WORKBENCH_OPERATOR_PACKET_CP_INDEX_REQUIRED_OS: ubuntu-latest,macos-latest,windows-latest" in operator_index
)
add(
    "ci_workbench_operator_packet_smoke_index_requires_all_os",
    "passed" if operator_index_ok else "failed",
    "requires ubuntu-latest,macos-latest,windows-latest uploaded smoke artifacts",
)

release_readiness_artifacts = workflow_job_block("release-readiness-artifacts")
release_readiness_artifacts_ok = (
    release_readiness_artifacts is not None
    and "scripts/release-readiness.sh --static-only" in release_readiness_artifacts
    and "ao2-release-readiness" in release_readiness_artifacts
    and "target/release-readiness-ci" in release_readiness_artifacts
)
add(
    "ci_release_readiness_static_artifact_job",
    "passed" if release_readiness_artifacts_ok else "failed",
    "runs static release readiness and uploads target/release-readiness-ci",
)

release_train_bridge_artifacts = workflow_job_block("release-train-control-plane-bridge-artifacts")
release_train_bridge_artifacts_ok = (
    release_train_bridge_artifacts is not None
    and "ao2-release-train-control-plane-bridge" in release_train_bridge_artifacts
    and "target/release-train-control-plane-bridge-ci" in release_train_bridge_artifacts
    and "ao2.release-train-control-plane-bridge.v1" in release_train_bridge_artifacts
    and "ao2.cp-release-train-bridge-smoke.v1" in release_train_bridge_artifacts
)
add(
    "ci_release_train_control_plane_bridge_artifact_job",
    "passed" if release_train_bridge_artifacts_ok else "failed",
    "runs release train control-plane bridge and uploads read-only bridge evidence",
)

release_readiness_artifact_consumer = workflow_job_block("release-readiness-artifact-consumer")
release_readiness_artifact_consumer_ok = (
    release_readiness_artifact_consumer is not None
    and "needs: [release-readiness-artifacts, release-train-control-plane-bridge-artifacts]" in release_readiness_artifact_consumer
    and "actions/download-artifact@v8.0.1" in release_readiness_artifact_consumer
    and "name: ao2-release-readiness" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-release-readiness" in release_readiness_artifact_consumer
    and "name: ao2-release-train-control-plane-bridge" in release_readiness_artifact_consumer
    and "target/release-readiness-consumer/ao2-release-train-control-plane-bridge" in release_readiness_artifact_consumer
    and "ao2.release-readiness-local.v1" in release_readiness_artifact_consumer
    and "ao2.release-train-control-plane-bridge.v1" in release_readiness_artifact_consumer
    and "ci_job_required_os:verify" in release_readiness_artifact_consumer
    and "ci_job_required_os:release-archive-hosted-smoke" in release_readiness_artifact_consumer
    and "ci_job_required_os:workbench-operator-packet-control-plane-smoke" in release_readiness_artifact_consumer
    and "ci_release_readiness_static_artifact_job" in release_readiness_artifact_consumer
    and "ci_release_train_control_plane_bridge_artifact_job" in release_readiness_artifact_consumer
)
add(
    "ci_release_readiness_artifact_consumer_job",
    "passed" if release_readiness_artifact_consumer_ok else "failed",
    "downloads release-readiness and release-train bridge artifacts and validates schema/status/core cross-OS checks",
)

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
report_md_path = summary_path.with_name("report.md")
report_html_path = summary_path.with_name("report.html")
summary = {
    "schema_version": "ao2.release-readiness-local.v1",
    "status": status,
    "mode": mode,
    "ao2_root": str(root),
    "control_plane_root_exists": cp_root.is_dir(),
    "report_md": str(report_md_path),
    "report_html": str(report_html_path),
    "checks": checks,
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

def compact(value, limit=600):
    value = str(value or "").replace("\n", " ").strip()
    if len(value) > limit:
        return value[: limit - 3] + "..."
    return value

lines = [
    "# AO2 Release Readiness",
    "",
    f"- Schema: `{summary['schema_version']}`",
    f"- Status: `{status}`",
    f"- Mode: `{mode}`",
    f"- AO2 root: `{root}`",
    f"- Control-plane root exists: `{cp_root.is_dir()}`",
    "",
    "| Check | Status | Detail |",
    "| --- | --- | --- |",
]
for check in checks:
    name = compact(check["name"]).replace("|", "\\|")
    check_status = compact(check["status"]).replace("|", "\\|")
    detail = compact(check.get("detail", "")).replace("|", "\\|")
    lines.append(f"| `{name}` | `{check_status}` | {detail} |")
report_md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

rows = []
for check in checks:
    rows.append(
        "<tr>"
        f"<td><code>{html.escape(compact(check['name']))}</code></td>"
        f"<td><code>{html.escape(compact(check['status']))}</code></td>"
        f"<td>{html.escape(compact(check.get('detail', '')))}</td>"
        "</tr>"
    )
report_html_path.write_text(
    "<!doctype html>\n"
    "<html><head><meta charset=\"utf-8\"><title>AO2 Release Readiness</title>"
    "<style>body{font-family:system-ui,sans-serif;margin:2rem;line-height:1.45}"
    "table{border-collapse:collapse;width:100%}td,th{border:1px solid #ddd;padding:.4rem;text-align:left}"
    "th{background:#f5f5f5}code{white-space:pre-wrap}</style></head><body>"
    "<h1>AO2 Release Readiness</h1>"
    f"<p><strong>Status:</strong> <code>{html.escape(status)}</code></p>"
    f"<p><strong>Mode:</strong> <code>{html.escape(mode)}</code></p>"
    f"<p><strong>Schema:</strong> <code>{html.escape(summary['schema_version'])}</code></p>"
    "<table><thead><tr><th>Check</th><th>Status</th><th>Detail</th></tr></thead><tbody>"
    + "".join(rows)
    + "</tbody></table></body></html>\n",
    encoding="utf-8",
)
print(f"summary={summary_path}")
print(f"report_md={report_md_path}")
print(f"report_html={report_html_path}")
print(f"status={status}")
if status != "passed":
    for check in checks:
        if check["status"] != "passed":
            print(f"failed={check['name']} {check.get('detail', '')}", file=sys.stderr)
    raise SystemExit(1)
PY
